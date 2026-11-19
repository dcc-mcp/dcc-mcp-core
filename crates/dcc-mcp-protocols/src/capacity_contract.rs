//! Shared capacity contract for JSON bridges (issue #2821).
//!
//! Runtime-unused helpers for declarations, clamping, byte budgets, and error data.

use serde_json::{Map, Value, json};

pub const CAPACITY_CAPABILITY_KEY: &str = "dcc-mcp.capacity.v1";
pub const CAPACITY_SCHEMA_VERSION: u64 = 1;
pub const LEGACY_RECEIVE_BYTES: u64 = 65535;
pub const MIN_RECEIVE_BYTES: u64 = 256;
pub const MAX_RECEIVE_BYTES: u64 = 1048576;
pub const MAX_U64: u64 = u64::MAX;
pub const OUTCOME_NOT_DISPATCHED: &str = "not_dispatched";
pub const OUTCOME_UNKNOWN: &str = "unknown";
pub const DIRECTION_REQUEST: &str = "request";
pub const DIRECTION_RESPONSE: &str = "response";
pub const ERROR_INVALID_CAPACITY_VALUE: &str = "invalid_capacity_value";
pub const ERROR_CONFLICTING_CAPACITY_DECLARATION: &str = "conflicting_capacity_declaration";
pub const ERROR_UNSUPPORTED_CAPACITY_VERSION: &str = "unsupported_capacity_version";
pub const ERROR_CODE_CAPACITY: &str = "capacity_error";
pub const COMPACT_ERROR_REQUIRED_FIELDS: [&str; 4] = ["code", "message", "data", "outcome"];
pub const COMPACT_ERROR_OPTIONAL_FIELDS: [&str; 1] = ["actual_bytes"];
pub const LEGACY_ASSUMED: &str = "legacy_assumed";
pub const DECLARED: &str = "declared";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectionalReceive {
    pub incoming: u64,
    pub outgoing: u64,
}

impl DirectionalReceive {
    pub fn new(incoming: u64, outgoing: u64) -> Result<Self, CapacityError> {
        validate_capacity(incoming)?;
        validate_capacity(outgoing)?;
        Ok(Self { incoming, outgoing })
    }

    pub fn to_json(self) -> Value {
        json!({"incoming": self.incoming, "outgoing": self.outgoing})
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityPolicy {
    min_bytes: u64,
    max_bytes: u64,
}

impl Default for CapacityPolicy {
    fn default() -> Self {
        Self {
            min_bytes: MIN_RECEIVE_BYTES,
            max_bytes: MAX_RECEIVE_BYTES,
        }
    }
}

impl CapacityPolicy {
    pub fn new(min_bytes: u64, max_bytes: u64) -> Result<Self, CapacityError> {
        if min_bytes == 0 || max_bytes < min_bytes {
            return Err(CapacityError::invalid(
                "policy requires 1 <= min_bytes <= max_bytes",
            ));
        }
        Ok(Self {
            min_bytes,
            max_bytes,
        })
    }

    pub fn min_bytes(self) -> u64 {
        self.min_bytes
    }

    pub fn max_bytes(self) -> u64 {
        self.max_bytes
    }

    pub fn clamp(self, value: u64) -> Result<u64, CapacityError> {
        if self.min_bytes == 0 || self.max_bytes < self.min_bytes {
            return Err(CapacityError::invalid(
                "policy requires 1 <= min_bytes <= max_bytes",
            ));
        }
        validate_capacity(value)?;
        Ok(value.clamp(self.min_bytes, self.max_bytes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Negotiation {
    Declared,
    LegacyAssumed,
}

impl Negotiation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Declared => DECLARED,
            Self::LegacyAssumed => LEGACY_ASSUMED,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectionalCapacityState {
    pub receive: DirectionalReceive,
    pub negotiation: Negotiation,
    pub declared_version: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteBudget {
    pub actual_bytes: usize,
    pub max_bytes: u64,
    pub fits: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityError {
    pub code: &'static str,
    pub message: String,
}

impl CapacityError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: ERROR_INVALID_CAPACITY_VALUE,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CapacityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CapacityError {}

fn validate_capacity(value: u64) -> Result<(), CapacityError> {
    if value == 0 {
        Err(CapacityError::invalid(
            "capacity values must be greater than zero",
        ))
    } else {
        Ok(())
    }
}

fn capacity_value(value: Option<&Value>) -> Result<u64, CapacityError> {
    value
        .and_then(Value::as_u64)
        .ok_or_else(|| CapacityError::invalid("capacity values must be positive integers"))
}
fn json_capacity_value(value: &Value) -> Result<u64, CapacityError> {
    value
        .as_u64()
        .ok_or_else(|| CapacityError::invalid("capacity values must be positive integers"))
}

pub fn parse_capacity_declaration(
    capabilities: Option<&Map<String, Value>>,
    policy: CapacityPolicy,
) -> Result<DirectionalCapacityState, CapacityError> {
    let legacy = policy.clamp(LEGACY_RECEIVE_BYTES)?;
    let declaration = match capabilities.and_then(|caps| caps.get(CAPACITY_CAPABILITY_KEY)) {
        None => {
            return Ok(DirectionalCapacityState {
                receive: DirectionalReceive::new(legacy, legacy)?,
                negotiation: Negotiation::LegacyAssumed,
                declared_version: None,
            });
        }
        Some(value) => value
            .as_object()
            .ok_or_else(|| CapacityError::invalid("capacity declaration must be an object"))?,
    };

    let version = match declaration.get("version") {
        None => CAPACITY_SCHEMA_VERSION,
        Some(Value::Number(number)) => number.as_u64().ok_or_else(|| CapacityError {
            code: ERROR_UNSUPPORTED_CAPACITY_VERSION,
            message: "unsupported capacity declaration version".to_string(),
        })?,
        Some(_) => {
            return Err(CapacityError {
                code: ERROR_UNSUPPORTED_CAPACITY_VERSION,
                message: "unsupported capacity declaration version".to_string(),
            });
        }
    };
    if version != CAPACITY_SCHEMA_VERSION {
        return Err(CapacityError {
            code: ERROR_UNSUPPORTED_CAPACITY_VERSION,
            message: "unsupported capacity declaration version".to_string(),
        });
    }

    let flat = declaration.contains_key("receive_max_bytes");
    let nested = declaration.contains_key("receive");
    if flat == nested {
        return Err(CapacityError {
            code: ERROR_CONFLICTING_CAPACITY_DECLARATION,
            message: "capacity declaration must use exactly one receive shape".to_string(),
        });
    }

    let receive = if flat {
        let limit = capacity_value(declaration.get("receive_max_bytes"))?;
        let clamped = policy.clamp(limit)?;
        DirectionalReceive::new(clamped, clamped)?
    } else {
        let directional = declaration
            .get("receive")
            .and_then(Value::as_object)
            .ok_or_else(|| CapacityError::invalid("receive declaration must be an object"))?;
        let incoming = directional
            .get("incoming")
            .map(json_capacity_value)
            .transpose()?
            .unwrap_or(LEGACY_RECEIVE_BYTES);
        let outgoing = directional
            .get("outgoing")
            .map(json_capacity_value)
            .transpose()?
            .unwrap_or(LEGACY_RECEIVE_BYTES);
        DirectionalReceive::new(policy.clamp(incoming)?, policy.clamp(outgoing)?)?
    };

    Ok(DirectionalCapacityState {
        receive,
        negotiation: Negotiation::Declared,
        declared_version: Some(CAPACITY_SCHEMA_VERSION),
    })
}

pub fn validate_final_bytes(
    serialized: &[u8],
    max_bytes: u64,
) -> Result<ByteBudget, CapacityError> {
    validate_capacity(max_bytes)?;
    let actual_bytes = serialized.len();
    Ok(ByteBudget {
        actual_bytes,
        max_bytes,
        fits: actual_bytes as u64 <= max_bytes,
    })
}

pub fn validate_final_text(text: &str, max_bytes: u64) -> Result<ByteBudget, CapacityError> {
    validate_final_bytes(text.as_bytes(), max_bytes)
}

pub fn capacity_error_data(
    direction: &str,
    max_bytes: u64,
    outcome: &str,
    actual_bytes: Option<u64>,
) -> Result<Value, CapacityError> {
    if !matches!(direction, DIRECTION_REQUEST | DIRECTION_RESPONSE) {
        return Err(CapacityError::invalid(
            "direction must be request or response",
        ));
    }
    if !matches!(outcome, OUTCOME_NOT_DISPATCHED | OUTCOME_UNKNOWN) {
        return Err(CapacityError::invalid(
            "outcome must be not_dispatched or unknown",
        ));
    }
    validate_capacity(max_bytes)?;
    let validated_actual = match actual_bytes {
        Some(actual) => {
            validate_capacity(actual)?;
            Some(actual)
        }
        None => None,
    };
    if outcome == OUTCOME_NOT_DISPATCHED
        && (direction != DIRECTION_REQUEST
            || validated_actual.is_none()
            || validated_actual.unwrap_or(0) <= max_bytes)
    {
        return Err(CapacityError::invalid(
            "not_dispatched requires a request whose actual_bytes exceeds max_bytes",
        ));
    }
    let mut data = Map::new();
    data.insert("direction".to_string(), json!(direction));
    data.insert("max_bytes".to_string(), json!(max_bytes));
    data.insert("outcome".to_string(), json!(outcome));
    if let Some(actual) = validated_actual {
        data.insert("actual_bytes".to_string(), json!(actual));
    }
    Ok(Value::Object(data))
}

pub fn make_compact_capacity_error(
    direction: &str,
    max_bytes: u64,
    error_budget_bytes: u64,
    correlation_id: &str,
    outcome: &str,
    actual_bytes: Option<u64>,
    message: Option<&str>,
) -> Result<Value, CapacityError> {
    let data = capacity_error_data(direction, max_bytes, outcome, actual_bytes)?;
    let text = message.unwrap_or_else(|| {
        if outcome == OUTCOME_NOT_DISPATCHED {
            "Request exceeded the byte budget."
        } else {
            "Result outcome is unknown; inspect state before retrying."
        }
    });
    let mut payload = Map::new();
    payload.insert("code".to_string(), json!(ERROR_CODE_CAPACITY));
    payload.insert("message".to_string(), json!(text));
    payload.insert("data".to_string(), data.clone());
    payload.insert("outcome".to_string(), json!(outcome));
    if let Some(actual) = data.get("actual_bytes") {
        payload.insert("actual_bytes".to_string(), actual.clone());
    }
    let wire = serde_json::to_vec(&Value::Object(payload.clone())).map_err(|error| {
        CapacityError::invalid(format!(
            "compact capacity error serialization failed: {}",
            error
        ))
    })?;
    if !validate_final_bytes(&wire, error_budget_bytes)?.fits {
        return Err(CapacityError::invalid(
            "compact capacity error exceeds error_budget_bytes",
        ));
    }
    if !correlation_id.is_empty() {
        let text = std::str::from_utf8(&wire)
            .map_err(|_| CapacityError::invalid("compact capacity error is not UTF-8"))?;
        let semantic_payload = serde_json::from_slice::<Value>(&wire).map_err(|error| {
            CapacityError::invalid(format!("compact capacity error decoding failed: {}", error))
        })?;
        if text.contains(correlation_id)
            || value_contains_correlation_id(&semantic_payload, correlation_id)
        {
            return Err(CapacityError::invalid(
                "compact capacity error must not contain correlation_id",
            ));
        }
    }
    Ok(Value::Object(payload))
}

fn value_contains_correlation_id(value: &Value, correlation_id: &str) -> bool {
    match value {
        Value::String(text) => text.contains(correlation_id),
        Value::Array(items) => items
            .iter()
            .any(|item| value_contains_correlation_id(item, correlation_id)),
        Value::Object(fields) => fields.iter().any(|(key, item)| {
            key.contains(correlation_id) || value_contains_correlation_id(item, correlation_id)
        }),
        _ => false,
    }
}

pub fn parse_capacity_error_data(data: &Value) -> Result<Value, CapacityError> {
    let object = data
        .as_object()
        .ok_or_else(|| CapacityError::invalid("capacity error data must be an object"))?;
    let direction = object
        .get("direction")
        .and_then(Value::as_str)
        .ok_or_else(|| CapacityError::invalid("direction must be request or response"))?;
    let outcome = object
        .get("outcome")
        .and_then(Value::as_str)
        .ok_or_else(|| CapacityError::invalid("outcome must be not_dispatched or unknown"))?;
    let max_bytes = capacity_value(object.get("max_bytes"))?;
    let actual_bytes = match object.get("actual_bytes") {
        None | Some(Value::Null) => None,
        Some(value) => Some(json_capacity_value(value)?),
    };
    capacity_error_data(direction, max_bytes, outcome, actual_bytes)
}
