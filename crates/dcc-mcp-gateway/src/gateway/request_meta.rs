use serde_json::{Value, json};

use super::admin::trace::AgentContext;

/// Allowed top-level keys in client-supplied `_meta` that may be passed
/// through to adapter tool params. `agent_context` is always server-derived
/// and is appended separately.
const DEFAULT_META_ALLOWLIST: &[&str] = &[
    "credential_profile",
    "permission_hint",
    "project_scope",
    "search_id",
];

/// Filter a client-supplied `_meta` object to only contain allowlisted keys.
/// Prevents inline secrets or arbitrary client data from piggybacking on the
/// meta passthrough channel.
fn bounded_meta(meta: Value) -> Value {
    match meta {
        Value::Object(map) => {
            let filtered: serde_json::Map<String, Value> = map
                .into_iter()
                .filter(|(k, _)| DEFAULT_META_ALLOWLIST.contains(&k.as_str()))
                .collect();
            Value::Object(filtered)
        }
        _ => Value::Object(serde_json::Map::new()),
    }
}

pub(crate) fn meta_with_agent_context(
    meta: Option<Value>,
    agent_context: Option<&AgentContext>,
) -> Option<Value> {
    let Some(agent_context) = agent_context else {
        return meta.map(bounded_meta);
    };
    let agent_context_value = serde_json::to_value(agent_context).ok()?;
    match meta {
        Some(m) => {
            let mut filtered = bounded_meta(m);
            if let Value::Object(ref mut map) = filtered {
                map.insert("agent_context".to_string(), agent_context_value);
            }
            Some(filtered)
        }
        None => Some(json!({ "agent_context": agent_context_value })),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn bounded_meta_strips_unknown_fields() {
        let filtered = bounded_meta(json!({
            "credential_profile": "prod",
            "permission_hint": "read-only",
            "project_scope": "movie-42",
            "secret_token": "should-be-stripped",
            "inline_secret": "also-stripped",
        }));
        let obj = filtered.as_object().unwrap();
        assert!(obj.contains_key("credential_profile"));
        assert!(obj.contains_key("permission_hint"));
        assert!(obj.contains_key("project_scope"));
        assert!(!obj.contains_key("secret_token"));
        assert!(!obj.contains_key("inline_secret"));
    }

    #[test]
    fn bounded_meta_preserves_search_id() {
        let filtered = bounded_meta(json!({"search_id": "abc-123", "evil": "no"}));
        let obj = filtered.as_object().unwrap();
        assert_eq!(obj["search_id"], "abc-123");
        assert!(!obj.contains_key("evil"));
    }

    #[test]
    fn bounded_meta_drops_non_object_meta() {
        let filtered = bounded_meta(json!(["not", "an", "object"]));
        let obj = filtered.as_object().unwrap();
        assert!(obj.is_empty());
    }

    #[test]
    fn meta_with_agent_context_client_agent_context_is_replaced_by_server() {
        let agent_ctx = AgentContext {
            actor_id: Some("server-artist".to_string()),
            session_id: Some("s1".into()),
            ..AgentContext::default()
        };
        let merged = meta_with_agent_context(
            Some(json!({
                "agent_context": {"actor_id": "fake-client"},
                "credential_profile": "prod",
            })),
            Some(&agent_ctx),
        )
        .expect("merged meta");

        assert_eq!(merged["agent_context"]["actor_id"], "server-artist");
        assert_eq!(merged["agent_context"]["session_id"], "s1");
        assert_eq!(merged["credential_profile"], "prod");
    }

    #[test]
    fn meta_with_agent_context_no_meta_is_backward_compatible() {
        assert!(meta_with_agent_context(None, None).is_none());
    }

    #[test]
    fn meta_with_agent_context_only_agent_context_no_client_meta() {
        let agent_ctx = AgentContext {
            actor_id: Some("a1".to_string()),
            ..AgentContext::default()
        };
        let merged = meta_with_agent_context(None, Some(&agent_ctx)).expect("merged");
        assert_eq!(merged["agent_context"]["actor_id"], "a1");
        assert!(merged.get("credential_profile").is_none());
    }

    #[test]
    fn meta_with_agent_context_drops_non_object_client_meta() {
        let agent_ctx = AgentContext {
            actor_id: Some("server-artist".to_string()),
            ..AgentContext::default()
        };
        let merged = meta_with_agent_context(Some(json!("inline-secret")), Some(&agent_ctx))
            .expect("merged");

        assert_eq!(merged["agent_context"]["actor_id"], "server-artist");
        assert!(merged.get("upstream_meta").is_none());
    }
}
