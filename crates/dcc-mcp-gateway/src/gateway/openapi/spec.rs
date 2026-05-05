//! OpenAPI spec parsing and MCP tool generation.

use serde_json::{Value, json};

use dcc_mcp_jsonrpc::McpTool;

use super::auth::AuthConfig;

/// A single HTTP operation extracted from an OpenAPI spec.
#[derive(Debug, Clone)]
pub struct OperationInfo {
    /// HTTP method in uppercase (e.g. `"GET"`, `"POST"`).
    pub method: String,
    /// Raw path template (e.g. `"/pets/{petId}"`).
    pub path: String,
    /// MCP tool name derived from the operation.
    pub tool_name: String,
    /// Human-readable description (from `summary` or `description`).
    pub description: String,
    /// JSON Schema for MCP `inputSchema` (merged from parameters + requestBody).
    pub input_schema: Value,
    /// Names of path-parameter placeholders (e.g. `["petId"]`).
    pub path_params: Vec<String>,
    /// Names of query parameters.
    pub query_params: Vec<String>,
    /// Whether this operation expects a JSON request body.
    pub has_body: bool,
}

/// Builder and container for an OpenAPI spec → MCP tool mapping.
///
/// # Usage
///
/// ```rust,ignore
/// let mount = OpenApiMount::from_spec_json(spec_value)
///     .base_url("https://api.example.com")
///     .auth(AuthConfig::bearer("$MY_API_TOKEN"))
///     .tool_prefix("example");
///
/// // Enumerate generated MCP tools.
/// let tools: Vec<McpTool> = mount.to_mcp_tools();
///
/// // Invoke one operation.
/// mount.call_operation_by_name("example__listPets", args, &http_client).await?;
/// ```
pub struct OpenApiMount {
    /// Parsed OpenAPI spec (only the `paths` and `components` sections are used).
    spec: Value,
    /// Base URL of the backend REST service (no trailing slash).
    base_url: String,
    /// Optional auth credentials forwarded to the backend on every call.
    pub(super) auth: Option<AuthConfig>,
    /// Optional prefix prepended to every generated tool name.
    tool_prefix: Option<String>,
    /// Cached list of extracted operations; populated lazily on first access.
    operations: std::sync::OnceLock<Vec<OperationInfo>>,
}

impl std::fmt::Debug for OpenApiMount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenApiMount")
            .field("base_url", &self.base_url)
            .field("tool_prefix", &self.tool_prefix)
            .field("has_auth", &self.auth.is_some())
            .finish()
    }
}

impl OpenApiMount {
    /// Create an [`OpenApiMount`] from an already-parsed JSON spec value.
    pub fn from_spec_json(spec: Value) -> Self {
        Self {
            spec,
            base_url: String::new(),
            auth: None,
            tool_prefix: None,
            operations: std::sync::OnceLock::new(),
        }
    }

    /// Set the backend base URL (no trailing slash).
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Attach auth credentials forwarded to the backend on every call.
    pub fn auth(mut self, config: AuthConfig) -> Self {
        self.auth = Some(config);
        self
    }

    /// Prefix prepended to every generated tool name, separated by `__`.
    pub fn tool_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.tool_prefix = Some(prefix.into());
        self
    }

    // ── Lazy operation cache ───────────────────────────────────────────────

    /// Return the list of operations extracted from the spec (cached after
    /// the first call).
    pub fn operations(&self) -> &[OperationInfo] {
        self.operations
            .get_or_init(|| extract_operations(&self.spec, self.tool_prefix.as_deref()))
    }

    // ── MCP surface ────────────────────────────────────────────────────────

    /// Generate one [`McpTool`] per HTTP operation in the spec.
    ///
    /// Tool names follow the pattern `{prefix}__{operationId}` or
    /// `{prefix}__{method}_{sanitised_path}` when `operationId` is absent.
    pub fn to_mcp_tools(&self) -> Vec<McpTool> {
        self.operations()
            .iter()
            .map(|op| McpTool {
                name: op.tool_name.clone(),
                description: op.description.clone(),
                input_schema: op.input_schema.clone(),
                output_schema: None,
                annotations: None,
                meta: None,
            })
            .collect()
    }

    /// Look up an operation by its MCP tool name.
    pub fn find_operation(&self, tool_name: &str) -> Option<&OperationInfo> {
        self.operations()
            .iter()
            .find(|op| op.tool_name == tool_name)
    }

    /// Fully-resolved URL for the given operation + args.
    ///
    /// Path parameters in `{param}` placeholders are substituted from `args`;
    /// query parameters are appended as `?key=value` pairs.
    pub fn resolve_url(&self, op: &OperationInfo, args: &Value) -> String {
        // Replace path params.
        let mut path = op.path.clone();
        for param in &op.path_params {
            if let Some(v) = args.get(param) {
                let val = value_to_string(v);
                path = path.replace(&format!("{{{param}}}"), &val);
            }
        }

        let mut url = format!("{}{}", self.base_url, path);

        // Append query params.
        let mut first = true;
        for param in &op.query_params {
            if let Some(v) = args.get(param) {
                let sep = if first { '?' } else { '&' };
                first = false;
                url.push(sep);
                url.push_str(&urlencod_pair(param, &value_to_string(v)));
            }
        }

        url
    }
}

// ── Spec parsing ──────────────────────────────────────────────────────────────

const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch", "head", "options"];

/// Extract all operations from the OpenAPI `paths` object.
fn extract_operations(spec: &Value, prefix: Option<&str>) -> Vec<OperationInfo> {
    let Some(paths) = spec.get("paths").and_then(Value::as_object) else {
        return Vec::new();
    };

    let mut ops = Vec::new();
    for (path, path_item) in paths {
        let Some(path_item_obj) = path_item.as_object() else {
            continue;
        };

        // Parameters defined at the path level (shared by all operations).
        let path_level_params = path_item_obj
            .get("parameters")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for method in HTTP_METHODS {
            let Some(operation) = path_item_obj.get(*method) else {
                continue;
            };

            let Some(op_obj) = operation.as_object() else {
                continue;
            };

            // Merge path-level + operation-level parameters.
            let mut params: Vec<Value> = path_level_params.clone();
            if let Some(op_params) = op_obj.get("parameters").and_then(Value::as_array) {
                // Operation-level overrides path-level params of the same name+in.
                for op_p in op_params {
                    let name = op_p.get("name").and_then(Value::as_str).unwrap_or("");
                    let location = op_p.get("in").and_then(Value::as_str).unwrap_or("");
                    params.retain(|p| {
                        !(p.get("name").and_then(Value::as_str).unwrap_or("") == name
                            && p.get("in").and_then(Value::as_str).unwrap_or("") == location)
                    });
                    params.push(op_p.clone());
                }
            }

            let operation_id = op_obj.get("operationId").and_then(Value::as_str);
            let summary = op_obj
                .get("summary")
                .and_then(Value::as_str)
                .or_else(|| op_obj.get("description").and_then(Value::as_str))
                .unwrap_or("")
                .to_string();

            let tool_name = build_tool_name(prefix, operation_id, method, path);
            let (input_schema, path_params, query_params) =
                build_input_schema(&params, op_obj, spec);
            let has_body = op_obj.get("requestBody").is_some();

            ops.push(OperationInfo {
                method: method.to_uppercase(),
                path: path.clone(),
                tool_name,
                description: summary,
                input_schema,
                path_params,
                query_params,
                has_body,
            });
        }
    }

    // Stable ordering: sort by (path, method) so the output is deterministic.
    ops.sort_by(|a, b| a.path.cmp(&b.path).then(a.method.cmp(&b.method)));
    ops
}

/// Derive a MCP tool name for an operation.
fn build_tool_name(
    prefix: Option<&str>,
    operation_id: Option<&str>,
    method: &str,
    path: &str,
) -> String {
    let base = if let Some(id) = operation_id {
        id.to_string()
    } else {
        // Sanitise path: replace `/`, `{`, `}`, `.`, `-` with `_`.
        let sanitised: String = path
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        // Trim leading/trailing underscores that come from the leading `/`.
        let sanitised = sanitised.trim_matches('_');
        format!("{method}_{sanitised}")
    };

    if let Some(p) = prefix {
        format!("{p}__{base}")
    } else {
        base
    }
}

/// Build the MCP `inputSchema` by merging path/query parameters and requestBody.
///
/// Returns `(schema, path_param_names, query_param_names)`.
fn build_input_schema(
    params: &[Value],
    op_obj: &serde_json::Map<String, Value>,
    spec: &Value,
) -> (Value, Vec<String>, Vec<String>) {
    let mut properties = serde_json::Map::new();
    let mut required: Vec<String> = Vec::new();
    let mut path_params = Vec::new();
    let mut query_params = Vec::new();

    for param in params {
        let name = match param.get("name").and_then(Value::as_str) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let location = param.get("in").and_then(Value::as_str).unwrap_or("");
        let is_required = param
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let description = param
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        // Schema for this parameter — fall back to a plain string type.
        let mut param_schema = param
            .get("schema")
            .cloned()
            .unwrap_or_else(|| json!({"type": "string"}));

        // Inline $ref resolution (one level deep).
        if let Some(ref_path) = param_schema
            .get("$ref")
            .and_then(Value::as_str)
            .map(str::to_string)
            && let Some(resolved) = resolve_ref(spec, &ref_path)
        {
            param_schema = resolved;
        }

        if !description.is_empty()
            && let Some(obj) = param_schema.as_object_mut()
        {
            obj.insert("description".to_string(), json!(description));
        }

        properties.insert(name.clone(), param_schema);

        match location {
            "path" => {
                path_params.push(name.clone());
                // Path params are always required by the HTTP spec.
                if !required.contains(&name) {
                    required.push(name);
                }
            }
            "query" => {
                query_params.push(name.clone());
                if is_required && !required.contains(&name) {
                    required.push(name.clone());
                }
            }
            _ => {
                // header / cookie params: included in schema but not tracked separately.
                if is_required && !required.contains(&name) {
                    required.push(name.clone());
                }
            }
        }
    }

    // Merge requestBody schema into the top-level properties under a `body` key,
    // or inline if the body schema uses named properties.
    if let Some(request_body) = op_obj.get("requestBody")
        && let Some((body_props, body_required)) = extract_body_schema(request_body, spec)
    {
        for (k, v) in body_props {
            properties.insert(k, v);
        }
        for r in body_required {
            if !required.contains(&r) {
                required.push(r);
            }
        }
    }

    let mut schema = json!({
        "type": "object",
        "properties": properties,
    });

    if !required.is_empty()
        && let Some(obj) = schema.as_object_mut()
    {
        obj.insert("required".to_string(), json!(required));
    }

    (schema, path_params, query_params)
}

/// Pull `properties` + `required` out of a `requestBody` content block.
fn extract_body_schema(
    request_body: &Value,
    spec: &Value,
) -> Option<(serde_json::Map<String, Value>, Vec<String>)> {
    let content = request_body.get("content")?.as_object()?;

    // Prefer `application/json`; fall back to the first content type.
    let media = content
        .get("application/json")
        .or_else(|| content.values().next())?;

    let mut schema = media.get("schema")?.clone();

    // Resolve top-level $ref.
    if let Some(ref_path) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        schema = resolve_ref(spec, &ref_path)?;
    }

    let props = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let req: Vec<String> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    Some((props, req))
}

/// Resolve a JSON Pointer–style `$ref` string against the spec document.
///
/// Only handles internal refs of the form `#/components/schemas/Foo`.
fn resolve_ref(spec: &Value, ref_path: &str) -> Option<Value> {
    let pointer = ref_path.strip_prefix('#')?;
    spec.pointer(pointer).cloned()
}

// ── URL helpers ───────────────────────────────────────────────────────────────

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Minimal percent-encoding for a single `key=value` query pair.
///
/// Only encodes characters that are not unreserved in RFC 3986 query strings.
fn urlencod_pair(key: &str, value: &str) -> String {
    format!("{}={}", urlenc(key), urlenc(value))
}

fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'*' | b'\'' => {
                out.push(b as char)
            }
            _ => {
                out.push('%');
                out.push(to_hex_digit(b >> 4));
                out.push(to_hex_digit(b & 0xf));
            }
        }
    }
    out
}

fn to_hex_digit(n: u8) -> char {
    if n < 10 {
        (b'0' + n) as char
    } else {
        (b'A' + n - 10) as char
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn petstore_spec() -> Value {
        json!({
            "openapi": "3.0.0",
            "info": {"title": "Petstore", "version": "1.0.0"},
            "paths": {
                "/pets": {
                    "get": {
                        "operationId": "listPets",
                        "summary": "List all pets",
                        "parameters": [
                            {
                                "name": "limit",
                                "in": "query",
                                "required": false,
                                "schema": {"type": "integer"}
                            }
                        ]
                    },
                    "post": {
                        "operationId": "createPet",
                        "summary": "Create a pet",
                        "requestBody": {
                            "required": true,
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "name": {"type": "string"},
                                            "tag": {"type": "string"}
                                        },
                                        "required": ["name"]
                                    }
                                }
                            }
                        }
                    }
                },
                "/pets/{petId}": {
                    "get": {
                        "operationId": "showPetById",
                        "summary": "Info for a specific pet",
                        "parameters": [
                            {
                                "name": "petId",
                                "in": "path",
                                "required": true,
                                "schema": {"type": "string"}
                            }
                        ]
                    },
                    "delete": {
                        "operationId": "deletePet",
                        "summary": "Delete a pet",
                        "parameters": [
                            {
                                "name": "petId",
                                "in": "path",
                                "required": true,
                                "schema": {"type": "string"}
                            }
                        ]
                    }
                }
            }
        })
    }

    #[test]
    fn generates_one_tool_per_operation() {
        let mount = OpenApiMount::from_spec_json(petstore_spec())
            .base_url("https://petstore.example.com")
            .tool_prefix("pet");
        let tools = mount.to_mcp_tools();
        // 4 operations: GET /pets, POST /pets, GET /pets/{petId}, DELETE /pets/{petId}
        assert_eq!(tools.len(), 4);

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"pet__listPets"));
        assert!(names.contains(&"pet__createPet"));
        assert!(names.contains(&"pet__showPetById"));
        assert!(names.contains(&"pet__deletePet"));
    }

    #[test]
    fn description_comes_from_summary() {
        let mount = OpenApiMount::from_spec_json(petstore_spec()).tool_prefix("pet");
        let tool = mount
            .to_mcp_tools()
            .into_iter()
            .find(|t| t.name == "pet__listPets")
            .unwrap();
        assert_eq!(tool.description, "List all pets");
    }

    #[test]
    fn query_param_in_schema() {
        let mount = OpenApiMount::from_spec_json(petstore_spec()).tool_prefix("pet");
        let op = mount.find_operation("pet__listPets").unwrap();
        assert!(op.query_params.contains(&"limit".to_string()));
        assert!(op.input_schema["properties"]["limit"].is_object());
    }

    #[test]
    fn path_param_in_schema_and_required() {
        let mount = OpenApiMount::from_spec_json(petstore_spec()).tool_prefix("pet");
        let op = mount.find_operation("pet__showPetById").unwrap();
        assert!(op.path_params.contains(&"petId".to_string()));
        let req = op.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert!(req.contains(&"petId"));
    }

    #[test]
    fn request_body_props_merged_into_schema() {
        let mount = OpenApiMount::from_spec_json(petstore_spec()).tool_prefix("pet");
        let op = mount.find_operation("pet__createPet").unwrap();
        assert!(op.has_body);
        assert!(op.input_schema["properties"]["name"].is_object());
        let req = op.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert!(req.contains(&"name"));
    }

    #[test]
    fn tool_name_fallback_without_operation_id() {
        let spec = json!({
            "openapi": "3.0.0",
            "paths": {
                "/items/{id}": {
                    "get": {
                        "summary": "Get item"
                    }
                }
            }
        });
        let mount = OpenApiMount::from_spec_json(spec).tool_prefix("svc");
        let tools = mount.to_mcp_tools();
        assert_eq!(tools.len(), 1);
        // Should be "svc__get_items__id_" or similar sanitised form
        assert!(tools[0].name.starts_with("svc__get_"));
    }

    #[test]
    fn resolve_url_substitutes_path_and_query_params() {
        let mount = OpenApiMount::from_spec_json(petstore_spec())
            .base_url("https://petstore.example.com")
            .tool_prefix("pet");
        let op = mount.find_operation("pet__listPets").unwrap();
        let url = mount.resolve_url(op, &json!({"limit": 10}));
        assert_eq!(url, "https://petstore.example.com/pets?limit=10");
    }

    #[test]
    fn resolve_url_with_path_param() {
        let mount = OpenApiMount::from_spec_json(petstore_spec())
            .base_url("https://petstore.example.com")
            .tool_prefix("pet");
        let op = mount.find_operation("pet__showPetById").unwrap();
        let url = mount.resolve_url(op, &json!({"petId": "abc-123"}));
        assert_eq!(url, "https://petstore.example.com/pets/abc-123");
    }
}
