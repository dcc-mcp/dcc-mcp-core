//! Workflow execution context — step outputs, template resolution, artefact
//! tracking.
//!
//! The executor maintains a shared [`WorkflowContext`] that every step reads
//! from (to resolve template variables like `{{steps.foo.output.bar}}` or
//! `{{steps.foo.file_refs[0].uri}}`) and writes to (to register its output
//! when it finishes).
//!
//! Template resolution supports two shapes:
//!
//! - **Whole-string reference** (`"{{steps.foo.output}}"`) — the resolved
//!   JSON value replaces the string entirely, preserving its type (object /
//!   array / number / bool).
//! - **Embedded reference** (`"prefix_{{steps.foo.output.name}}_suffix"`) —
//!   the resolved value is rendered as a string and spliced in.
//!
//! Dotted and bracket-indexed paths are supported:
//! `steps.foo.output.items[0].path`, `inputs.date`, `item.name`.

use std::collections::HashMap;

use dcc_mcp_artefact::FileRef;
use parking_lot::RwLock;
use serde_json::Value;

use crate::spec::StepId;

/// Output recorded by the executor after a step completes successfully.
#[derive(Debug, Clone)]
pub struct StepOutput {
    /// Raw tool output (whatever the dispatcher / remote returned).
    pub output: Value,
    /// File references attached to this step's output, if any.
    /// Populated from `output.file_refs` (array of FileRef objects) or
    /// from `output.context.file_refs`.
    pub file_refs: Vec<FileRef>,
}

impl StepOutput {
    /// Construct with the raw output and no file refs.
    pub fn from_value(output: Value) -> Self {
        let file_refs = extract_file_refs(&output);
        Self { output, file_refs }
    }
}

/// Pull `file_refs` out of a tool output, looking at both
/// `output.file_refs` and `output.context.file_refs` locations.
fn extract_file_refs(output: &Value) -> Vec<FileRef> {
    let mut out = Vec::new();
    let try_extract = |v: &Value, out: &mut Vec<FileRef>| {
        if let Some(arr) = v.as_array() {
            for entry in arr {
                if let Ok(fr) = serde_json::from_value::<FileRef>(entry.clone()) {
                    out.push(fr);
                }
            }
        }
    };
    if let Some(v) = output.get("file_refs") {
        try_extract(v, &mut out);
    }
    if let Some(ctx) = output.get("context") {
        if let Some(v) = ctx.get("file_refs") {
            try_extract(v, &mut out);
        }
    }
    out
}

/// Shared, interior-mutable execution context.
///
/// Cheap to `clone` — it's just an `Arc` around the inner state.
#[derive(Debug, Default, Clone)]
pub struct WorkflowContext {
    inner: std::sync::Arc<RwLock<WorkflowContextInner>>,
}

#[derive(Debug, Default)]
struct WorkflowContextInner {
    inputs: Value,
    steps: HashMap<String, StepOutput>,
    /// Stack of `foreach` item bindings. The top of the stack shadows earlier
    /// bindings so nested loops pick the innermost.
    item_stack: Vec<(String, Value)>,
}

impl WorkflowContext {
    /// Fresh context with the given workflow inputs.
    pub fn new(inputs: Value) -> Self {
        let inner = WorkflowContextInner {
            inputs,
            ..WorkflowContextInner::default()
        };
        Self {
            inner: std::sync::Arc::new(RwLock::new(inner)),
        }
    }

    /// Record the output of a completed step.
    pub fn record_step(&self, id: &StepId, output: StepOutput) {
        self.inner.write().steps.insert(id.0.clone(), output);
    }

    /// Snapshot of a recorded step output.
    pub fn step(&self, id: &str) -> Option<StepOutput> {
        self.inner.read().steps.get(id).cloned()
    }

    /// Snapshot of all step outputs (for persistence / reporting).
    pub fn steps_snapshot(&self) -> HashMap<String, StepOutput> {
        self.inner.read().steps.clone()
    }

    /// Push a foreach item binding. Returns a guard that pops on drop.
    pub fn push_item(&self, name: &str, value: Value) -> ItemGuard {
        self.inner
            .write()
            .item_stack
            .push((name.to_string(), value));
        ItemGuard {
            ctx: self.clone(),
            name: name.to_string(),
        }
    }

    /// Current innermost item binding for `name`, if any.
    fn lookup_item(&self, name: &str) -> Option<Value> {
        let g = self.inner.read();
        for (k, v) in g.item_stack.iter().rev() {
            if k == name {
                return Some(v.clone());
            }
        }
        None
    }

    /// Build the JSON root against which template references resolve.
    ///
    /// Shape:
    /// ```json
    /// {
    ///   "inputs": { ... },
    ///   "steps": { "<id>": { "output": ..., "file_refs": [...] }, ... },
    ///   "item": <innermost item binding or null>,
    ///   "<any foreach binding name>": <binding value>
    /// }
    /// ```
    pub fn as_json(&self) -> Value {
        let g = self.inner.read();
        let mut steps_obj = serde_json::Map::new();
        for (id, out) in g.steps.iter() {
            let fr_json = serde_json::to_value(&out.file_refs).unwrap_or(Value::Null);
            steps_obj.insert(
                id.clone(),
                serde_json::json!({
                    "output": out.output,
                    "file_refs": fr_json,
                }),
            );
        }
        let mut root = serde_json::json!({
            "inputs": g.inputs.clone(),
            "steps": Value::Object(steps_obj),
        });
        // Item bindings: promote each name to a top-level alias. The last
        // push wins since HashMap::insert overwrites.
        if let Some(obj) = root.as_object_mut() {
            let mut last_item: Option<Value> = None;
            for (k, v) in g.item_stack.iter() {
                obj.insert(k.clone(), v.clone());
                last_item = Some(v.clone());
            }
            obj.insert("item".to_string(), last_item.unwrap_or(Value::Null));
        }
        root
    }

    /// Render a JSON argument tree by replacing every `"{{…}}"` template
    /// reference against this context. Non-string leaves are returned
    /// unchanged.
    pub fn render(&self, args: &Value) -> Result<Value, TemplateError> {
        let root = self.as_json();
        render_value(args, &root, self)
    }
}

/// Guard popping a foreach item binding on drop.
#[must_use = "ItemGuard must be held for the lifetime of the iteration"]
pub struct ItemGuard {
    ctx: WorkflowContext,
    name: String,
}

impl Drop for ItemGuard {
    fn drop(&mut self) {
        let mut g = self.ctx.inner.write();
        // Pop the most recent entry with this name (should be the tail).
        if let Some(pos) = g.item_stack.iter().rposition(|(k, _)| k == &self.name) {
            g.item_stack.remove(pos);
        }
    }
}

// ── Template rendering ───────────────────────────────────────────────────

/// Template / path resolution errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TemplateError {
    /// Template reference could not be resolved against the context.
    #[error("template {template:?} references unknown path {path:?}")]
    UnknownPath {
        /// The raw template.
        template: String,
        /// The dotted/bracketed path that failed.
        path: String,
    },
    /// Template body was malformed (empty, unterminated, invalid chars).
    #[error("malformed template {template:?}: {reason}")]
    Malformed {
        /// The raw template.
        template: String,
        /// Human-readable reason.
        reason: String,
    },
}

fn render_value(v: &Value, root: &Value, ctx: &WorkflowContext) -> Result<Value, TemplateError> {
    match v {
        Value::String(s) => render_string(s, root, ctx),
        Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(render_value(item, root, ctx)?);
            }
            Ok(Value::Array(out))
        }
        Value::Object(obj) => {
            let mut out = serde_json::Map::with_capacity(obj.len());
            for (k, v) in obj {
                out.insert(k.clone(), render_value(v, root, ctx)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

fn render_string(s: &str, root: &Value, ctx: &WorkflowContext) -> Result<Value, TemplateError> {
    // Whole-string reference: replace entirely with the resolved value.
    let trimmed = s.trim();
    if let Some(body) = trimmed
        .strip_prefix("{{")
        .and_then(|r| r.strip_suffix("}}"))
    {
        if !trimmed.matches("{{").count() == 1 || trimmed.matches("}}").count() != 1 {
            // More than one marker — fall through to embedded mode.
        } else {
            let path = body.trim();
            let resolved =
                resolve_path(path, root, ctx).ok_or_else(|| TemplateError::UnknownPath {
                    template: s.to_string(),
                    path: path.to_string(),
                })?;
            return Ok(resolved);
        }
    }

    // Embedded mode: stringly substitute each `{{…}}` span.
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        match rest.find("{{") {
            None => {
                out.push_str(rest);
                break;
            }
            Some(i) => {
                out.push_str(&rest[..i]);
                let after = &rest[i + 2..];
                let close = after.find("}}").ok_or_else(|| TemplateError::Malformed {
                    template: s.to_string(),
                    reason: "unterminated '{{'".to_string(),
                })?;
                let path = after[..close].trim();
                if path.is_empty() {
                    return Err(TemplateError::Malformed {
                        template: s.to_string(),
                        reason: "empty reference".to_string(),
                    });
                }
                let resolved =
                    resolve_path(path, root, ctx).ok_or_else(|| TemplateError::UnknownPath {
                        template: s.to_string(),
                        path: path.to_string(),
                    })?;
                let rendered = match resolved {
                    Value::String(s) => s,
                    Value::Null => String::new(),
                    other => other.to_string(),
                };
                out.push_str(&rendered);
                rest = &after[close + 2..];
            }
        }
    }
    Ok(Value::String(out))
}

/// Resolve a dotted / bracket-indexed path against the context root.
///
/// Supported grammar:
///   path := ident ( '.' ident | '[' number ']' )*
fn resolve_path(path: &str, root: &Value, ctx: &WorkflowContext) -> Option<Value> {
    // Split into tokens — either ident or [number].
    let tokens = tokenize_path(path)?;
    if tokens.is_empty() {
        return None;
    }

    // The first token MUST be an ident (the root key).
    let (first_ident, rest) = match tokens.split_first() {
        Some((PathToken::Ident(s), rest)) => (s.clone(), rest),
        _ => return None,
    };

    // Resolve the root: prefer context item bindings (foreach), else fall
    // through to the JSON root object.
    let mut cur: Value = match ctx.lookup_item(&first_ident) {
        Some(v) => v,
        None => root.get(&first_ident).cloned().unwrap_or(Value::Null),
    };
    if matches!(cur, Value::Null)
        && root.get(&first_ident).is_none()
        && ctx.lookup_item(&first_ident).is_none()
    {
        return None;
    }

    for tok in rest {
        cur = match tok {
            PathToken::Ident(k) => cur.get(k).cloned().unwrap_or(Value::Null),
            PathToken::Index(i) => cur.get(*i).cloned().unwrap_or(Value::Null),
        };
    }
    Some(cur)
}

#[derive(Debug, Clone)]
enum PathToken {
    Ident(String),
    Index(usize),
}

fn tokenize_path(s: &str) -> Option<Vec<PathToken>> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'.' {
            i += 1;
            continue;
        }
        if c == b'[' {
            // Read digits until ']'.
            let start = i + 1;
            let end = start + bytes[start..].iter().position(|&b| b == b']')?;
            let num: usize = std::str::from_utf8(&bytes[start..end]).ok()?.parse().ok()?;
            out.push(PathToken::Index(num));
            i = end + 1;
            continue;
        }
        // Read an ident up to '.' or '['.
        let start = i;
        while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
            i += 1;
        }
        let ident = std::str::from_utf8(&bytes[start..i]).ok()?;
        if ident.is_empty() {
            return None;
        }
        out.push(PathToken::Ident(ident.to_string()));
    }
    Some(out)
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_plain_string_is_identity() {
        let ctx = WorkflowContext::new(json!({}));
        assert_eq!(ctx.render(&json!("hello")).unwrap(), json!("hello"));
    }

    #[test]
    fn render_whole_string_ref_preserves_type() {
        let ctx = WorkflowContext::new(json!({"n": 42, "name": "demo"}));
        assert_eq!(ctx.render(&json!("{{inputs.n}}")).unwrap(), json!(42));
        assert_eq!(
            ctx.render(&json!("{{inputs.name}}")).unwrap(),
            json!("demo")
        );
    }

    #[test]
    fn render_embedded_ref_stringifies() {
        let ctx = WorkflowContext::new(json!({"n": 3}));
        assert_eq!(
            ctx.render(&json!("count={{inputs.n}}")).unwrap(),
            json!("count=3")
        );
    }

    #[test]
    fn render_step_output_path() {
        let ctx = WorkflowContext::new(json!({}));
        ctx.record_step(
            &StepId("s1".into()),
            StepOutput::from_value(json!({"path": "/tmp/x"})),
        );
        let rendered = ctx.render(&json!("{{steps.s1.output.path}}")).unwrap();
        assert_eq!(rendered, json!("/tmp/x"));
    }

    #[test]
    fn render_file_ref_index() {
        let ctx = WorkflowContext::new(json!({}));
        ctx.record_step(
            &StepId("export".into()),
            StepOutput::from_value(json!({
                "file_refs": [{
                    "uri": "artefact://sha256/abc",
                    "created_at": "2020-01-01T00:00:00Z"
                }]
            })),
        );
        let rendered = ctx
            .render(&json!("{{steps.export.file_refs[0].uri}}"))
            .unwrap();
        assert_eq!(rendered, json!("artefact://sha256/abc"));
    }

    #[test]
    fn foreach_item_binding_shadows() {
        let ctx = WorkflowContext::new(json!({}));
        let _g = ctx.push_item("file", json!({"path": "/a"}));
        let rendered = ctx.render(&json!("{{file.path}}")).unwrap();
        assert_eq!(rendered, json!("/a"));
    }

    #[test]
    fn unknown_template_errors() {
        let ctx = WorkflowContext::new(json!({}));
        let err = ctx.render(&json!("{{nope.qux}}")).unwrap_err();
        assert!(matches!(err, TemplateError::UnknownPath { .. }));
    }

    #[test]
    fn malformed_template_errors() {
        let ctx = WorkflowContext::new(json!({}));
        let err = ctx.render(&json!("hello {{oops")).unwrap_err();
        assert!(matches!(err, TemplateError::Malformed { .. }));
    }

    #[test]
    fn nested_object_renders() {
        let ctx = WorkflowContext::new(json!({"date": "2024-01-01"}));
        let rendered = ctx
            .render(&json!({"date": "{{inputs.date}}", "nested": {"x": "{{inputs.date}}"}}))
            .unwrap();
        assert_eq!(
            rendered,
            json!({"date": "2024-01-01", "nested": {"x": "2024-01-01"}})
        );
    }
}
