//! Task-scoped attribution metadata for single and batched gateway calls.

use anyhow::Context;
use serde_json::{Value, json};

pub(crate) fn attach_agent_session_id(
    meta: Option<Value>,
    session_id: Option<&str>,
) -> anyhow::Result<Option<Value>> {
    let Some(session_id) = session_id else {
        return Ok(meta);
    };
    let session_id = session_id.trim();
    if session_id.is_empty() {
        anyhow::bail!("--agent-session-id must not be empty");
    }
    let mut meta = meta.unwrap_or_else(|| json!({}));
    let object = meta
        .as_object_mut()
        .context("call metadata must be a JSON object")?;
    let agent_context = object
        .entry("agent_context".to_string())
        .or_insert_with(|| json!({}));
    let agent_context = agent_context
        .as_object_mut()
        .context("call metadata agent_context must be a JSON object")?;
    if let Some(existing) = agent_context.get("session_id") {
        let existing = existing
            .as_str()
            .context("call metadata agent_context.session_id must be a string")?;
        if existing != session_id {
            anyhow::bail!("--agent-session-id conflicts with --meta-json agent_context.session_id");
        }
    }
    agent_context.insert("session_id".to_string(), json!(session_id));
    Ok(Some(meta))
}

pub(crate) fn attach_batch_agent_session_id(
    request: &mut Value,
    session_id: Option<&str>,
) -> anyhow::Result<()> {
    let Some(session_id) = session_id else {
        return Ok(());
    };
    let session_id = session_id.trim();
    if session_id.is_empty() {
        anyhow::bail!("--agent-session-id must not be empty");
    }
    let calls = request
        .get_mut("calls")
        .and_then(Value::as_array_mut)
        .context("batch request must contain a calls array")?;
    for call in calls {
        let object = call
            .as_object_mut()
            .context("each batch call must be a JSON object")?;
        let meta = object.remove("meta").filter(|value| !value.is_null());
        if let Some(meta) = attach_agent_session_id(meta, Some(session_id))? {
            object.insert("meta".to_string(), meta);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_session_id_merges_with_lease_metadata_and_rejects_conflicts() {
        let meta =
            attach_agent_session_id(Some(json!({"lease_owner": "workflow-42"})), Some("task-42"))
                .unwrap()
                .unwrap();
        assert_eq!(meta["lease_owner"], "workflow-42");
        assert_eq!(meta["agent_context"]["session_id"], "task-42");

        let error = attach_agent_session_id(
            Some(json!({"agent_context": {"session_id": "other-task"}})),
            Some("task-42"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("conflicts"));

        let error = attach_agent_session_id(None, Some("   ")).unwrap_err();
        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn agent_session_id_is_injected_into_every_batch_call() {
        let mut request = json!({
            "calls": [
                {"tool_slug": "maya.a.first", "arguments": {}},
                {"tool_slug": "maya.a.null-meta", "arguments": {}, "meta": null},
                {
                    "tool_slug": "maya.a.second",
                    "arguments": {},
                    "meta": {"lease_owner": "workflow-42"}
                }
            ]
        });

        attach_batch_agent_session_id(&mut request, Some("task-42")).unwrap();

        assert_eq!(
            request["calls"][0]["meta"]["agent_context"]["session_id"],
            "task-42"
        );
        assert_eq!(
            request["calls"][1]["meta"]["agent_context"]["session_id"],
            "task-42"
        );
        assert_eq!(request["calls"][2]["meta"]["lease_owner"], "workflow-42");
        assert_eq!(
            request["calls"][2]["meta"]["agent_context"]["session_id"],
            "task-42"
        );
    }
}
