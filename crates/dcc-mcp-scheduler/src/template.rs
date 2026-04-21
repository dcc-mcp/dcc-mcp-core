//! Minimal template engine for `{{trigger.payload.<path>}}` placeholders.
//!
//! A full Handlebars / Tera dependency would be overkill — we only need
//! dotted-path lookup into a JSON value, with the original string
//! returned unchanged when nothing matches.
//!
//! Supported syntax:
//!
//! * `{{trigger.payload.path}}` — dotted JSON path into `payload`.
//! * `{{trigger.schedule_id}}` — literal context field.
//! * `{{trigger.workflow}}` — literal context field.
//!
//! Anything else is left as-is. Placeholders that resolve to `null` are
//! rendered as the JSON null literal `null` rather than the string
//! `"null"` — this is what callers actually want when feeding the result
//! back into workflow inputs.

use serde_json::Value;

/// Context available to placeholder lookups.
#[derive(Debug, Clone)]
pub struct RenderCtx<'a> {
    /// Payload body (webhook JSON body or empty object for cron).
    pub payload: &'a Value,
    /// Id of the firing schedule.
    pub schedule_id: &'a str,
    /// Workflow name of the firing schedule.
    pub workflow: &'a str,
}

/// Render placeholders within a JSON value (recursive).
///
/// Strings that are **exactly** a single placeholder are replaced with
/// the underlying JSON value (preserving type). Strings that embed
/// placeholders as a substring are rendered as string interpolation.
#[must_use]
pub fn render_value(input: &Value, ctx: &RenderCtx<'_>) -> Value {
    match input {
        Value::String(s) => render_string(s, ctx),
        Value::Array(arr) => Value::Array(arr.iter().map(|v| render_value(v, ctx)).collect()),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), render_value(v, ctx));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn render_string(s: &str, ctx: &RenderCtx<'_>) -> Value {
    // Fast-path: whole-string placeholder → preserve value type.
    if let Some(path) = whole_placeholder(s) {
        return resolve(path, ctx).unwrap_or_else(|| Value::String(s.to_string()));
    }
    // Slow-path: substring interpolation → always yields a string.
    Value::String(interpolate(s, ctx))
}

fn whole_placeholder(s: &str) -> Option<&str> {
    let trimmed = s.trim();
    let inner = trimmed.strip_prefix("{{")?.strip_suffix("}}")?;
    // Disallow nested braces — treat as interpolation instead.
    if inner.contains("{{") || inner.contains("}}") {
        return None;
    }
    Some(inner.trim())
}

fn interpolate(s: &str, ctx: &RenderCtx<'_>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            // Unterminated — emit the rest verbatim and stop.
            out.push_str(&rest[open..]);
            return out;
        };
        let path = after[..close].trim();
        let replacement = resolve(path, ctx)
            .map(|v| match v {
                Value::String(s) => s,
                other => other.to_string(),
            })
            .unwrap_or_else(|| format!("{{{{{path}}}}}"));
        out.push_str(&replacement);
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    out
}

fn resolve(path: &str, ctx: &RenderCtx<'_>) -> Option<Value> {
    let path = path.trim();
    if let Some(rest) = path.strip_prefix("trigger.payload") {
        let json_path = rest.strip_prefix('.').unwrap_or("");
        return lookup_json(ctx.payload, json_path);
    }
    if path == "trigger.schedule_id" {
        return Some(Value::String(ctx.schedule_id.to_string()));
    }
    if path == "trigger.workflow" {
        return Some(Value::String(ctx.workflow.to_string()));
    }
    None
}

fn lookup_json(root: &Value, path: &str) -> Option<Value> {
    if path.is_empty() {
        return Some(root.clone());
    }
    let mut cur = root;
    for seg in path.split('.') {
        if seg.is_empty() {
            return None;
        }
        // Support numeric array indices (e.g. `items.0.name`).
        if let Ok(idx) = seg.parse::<usize>() {
            cur = cur.as_array().and_then(|a| a.get(idx))?;
        } else {
            cur = cur.as_object().and_then(|m| m.get(seg))?;
        }
    }
    Some(cur.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx<'a>(payload: &'a Value) -> RenderCtx<'a> {
        RenderCtx {
            payload,
            schedule_id: "sid",
            workflow: "wf",
        }
    }

    #[test]
    fn whole_string_preserves_type() {
        let payload = json!({"count": 5});
        let ctx = ctx(&payload);
        let out = render_value(&json!("{{trigger.payload.count}}"), &ctx);
        assert_eq!(out, json!(5));
    }

    #[test]
    fn substring_interpolates() {
        let payload = json!({"file": "a.ma"});
        let ctx = ctx(&payload);
        let out = render_value(&json!("uploaded: {{trigger.payload.file}}"), &ctx);
        assert_eq!(out, json!("uploaded: a.ma"));
    }

    #[test]
    fn unknown_placeholder_passthrough() {
        let payload = json!({});
        let ctx = ctx(&payload);
        let out = render_value(&json!("{{trigger.payload.missing}}"), &ctx);
        assert_eq!(out, json!("{{trigger.payload.missing}}"));
    }

    #[test]
    fn nested_object_render() {
        let payload = json!({"user": {"name": "alice"}});
        let ctx = ctx(&payload);
        let input = json!({"args": {"who": "{{trigger.payload.user.name}}"}});
        let out = render_value(&input, &ctx);
        assert_eq!(out, json!({"args": {"who": "alice"}}));
    }

    #[test]
    fn schedule_id_resolves() {
        let payload = json!({});
        let ctx = ctx(&payload);
        let out = render_value(&json!("{{trigger.schedule_id}}"), &ctx);
        assert_eq!(out, json!("sid"));
    }

    #[test]
    fn array_index_lookup() {
        let payload = json!({"items": [{"n": "a"}, {"n": "b"}]});
        let ctx = ctx(&payload);
        let out = render_value(&json!("{{trigger.payload.items.1.n}}"), &ctx);
        assert_eq!(out, json!("b"));
    }
}
