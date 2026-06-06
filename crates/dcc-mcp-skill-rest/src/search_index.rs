//! Bounded search-index helpers for the per-DCC REST service.
//!
//! Keep full schemas behind `/v1/describe`; this module derives small aliases
//! and schema terms for `/v1/search` and gateway capability indexing.

use serde_json::Value;

use crate::service::CatalogAction;
use dcc_mcp_models::ToolAnnotations;

pub(crate) fn action_metadata(action: &CatalogAction) -> Value {
    let mut dcc = serde_json::Map::new();
    dcc.insert(
        "affinity".to_string(),
        Value::String(action.thread_affinity.as_str().to_string()),
    );
    dcc.insert(
        "execution".to_string(),
        Value::String(
            match action.execution {
                dcc_mcp_models::ExecutionMode::Sync => "sync",
                dcc_mcp_models::ExecutionMode::Async => "async",
            }
            .to_string(),
        ),
    );
    if let Some(timeout) = action.timeout_hint_secs {
        dcc.insert("timeoutHintSecs".to_string(), serde_json::json!(timeout));
    }
    dcc.insert(
        "enforceThreadAffinity".to_string(),
        Value::Bool(action.enforce_thread_affinity),
    );
    dcc.insert(
        "risk".to_string(),
        Value::String(action_risk(&action.annotations).to_string()),
    );
    if let Some(ref examples) = action.call_examples {
        dcc.insert("call_examples".to_string(), serde_json::json!(examples));
    }

    let mut out = serde_json::Map::new();
    out.insert("dcc".to_string(), Value::Object(dcc));
    if let Some(runtime) = &action.runtime {
        out.insert("runtime".to_string(), serde_json::json!(runtime));
    }
    Value::Object(out)
}

pub(crate) fn search_metadata(action: &CatalogAction) -> Option<Value> {
    let has_non_default_execution = action.thread_affinity.is_main()
        || action.execution.is_deferred()
        || action.timeout_hint_secs.is_some()
        || action.enforce_thread_affinity
        || !action.annotations.is_empty();
    let has_runtime = action.runtime.is_some();
    if !has_non_default_execution
        && !has_runtime
        && action.search_aliases.is_empty()
        && action.search_tokens.is_empty()
    {
        return None;
    }

    let mut metadata = if has_non_default_execution {
        action_metadata(action)
    } else {
        serde_json::json!({"dcc": {}})
    };
    if let Some(dcc) = metadata.get_mut("dcc").and_then(Value::as_object_mut) {
        if !action.search_aliases.is_empty() {
            dcc.insert(
                "searchAliases".to_string(),
                serde_json::json!(action.search_aliases),
            );
        }
        if !action.search_tokens.is_empty() {
            dcc.insert(
                "searchTokens".to_string(),
                serde_json::json!(action.search_tokens),
            );
        }
    }
    if let Some(runtime) = &action.runtime
        && let Some(obj) = metadata.as_object_mut()
    {
        obj.insert("runtime".to_string(), serde_json::json!(runtime));
    }
    Some(metadata)
}

pub(crate) fn search_haystack(action: &CatalogAction) -> String {
    let mut hay = String::new();
    for part in [
        action.action_name.as_str(),
        action.skill_name.as_str(),
        action.description.as_str(),
    ] {
        hay.push(' ');
        hay.push_str(&part.to_ascii_lowercase());
    }
    hay.push(' ');
    hay.push_str(&action.tags.join(" ").to_ascii_lowercase());
    hay.push(' ');
    hay.push_str(&action.search_aliases.join(" ").to_ascii_lowercase());
    for token in &action.search_tokens {
        hay.push(' ');
        hay.push_str(&search_token_text(token).to_ascii_lowercase());
    }
    hay
}

pub(crate) fn merged_search_aliases(
    skill_aliases: &[String],
    tool_aliases: &[String],
) -> Vec<String> {
    normalise_search_values(
        skill_aliases
            .iter()
            .chain(tool_aliases)
            .cloned()
            .collect::<Vec<_>>(),
        24,
    )
}

pub(crate) fn normalise_search_values(values: Vec<String>, limit: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(values.len().min(limit));
    let mut seen = std::collections::HashSet::new();
    for value in values {
        for item in value.split(',') {
            let token = item.split_whitespace().collect::<Vec<_>>().join(" ");
            let token = token.trim();
            if token.len() < 2 || token.len() > 64 {
                continue;
            }
            let key = token.to_ascii_lowercase();
            if seen.insert(key) {
                out.push(token.to_string());
            }
            if out.len() >= limit {
                return out;
            }
        }
    }
    out
}

pub(crate) fn schema_search_tokens(schema: &Value) -> Vec<String> {
    let mut tokens = Vec::new();
    collect_schema_search_tokens(schema, 0, &mut tokens);
    normalise_search_values(tokens, 48)
}

fn collect_schema_search_tokens(schema: &Value, depth: usize, out: &mut Vec<String>) {
    if depth > 2 || out.len() >= 48 {
        return;
    }
    let Some(obj) = schema.as_object() else {
        return;
    };

    if let Some(required) = obj.get("required").and_then(Value::as_array) {
        for field in required.iter().filter_map(Value::as_str) {
            out.push(format!("required:{field}"));
            if out.len() >= 48 {
                return;
            }
        }
    }

    let Some(props) = obj.get("properties").and_then(Value::as_object) else {
        return;
    };
    let mut names: Vec<&String> = props.keys().collect();
    names.sort();
    for name in names {
        out.push(format!("schema:{name}"));
        if out.len() >= 48 {
            return;
        }
        let Some(prop) = props.get(name) else {
            continue;
        };
        if let Some(description) = prop.get("description").and_then(Value::as_str) {
            let description = description
                .split_whitespace()
                .take(8)
                .collect::<Vec<_>>()
                .join(" ");
            if !description.is_empty() {
                out.push(format!("schema:{description}"));
            }
        }
        collect_schema_search_tokens(prop, depth + 1, out);
        if out.len() >= 48 {
            return;
        }
    }
}

fn search_token_text(token: &str) -> &str {
    token
        .strip_prefix("alias:")
        .or_else(|| token.strip_prefix("schema:"))
        .or_else(|| token.strip_prefix("required:"))
        .unwrap_or(token)
}

fn action_risk(annotations: &ToolAnnotations) -> &'static str {
    if annotations.destructive_hint == Some(true) {
        "destructive"
    } else if annotations.open_world_hint == Some(true) {
        "open-world"
    } else if annotations.read_only_hint == Some(true) {
        "read-only"
    } else {
        "mutation"
    }
}
