//! Request projection for the read-only `capabilities` command.
//! The CLI never invents capability evidence. It sends these arguments to the
//! existing `dcc_host_capabilities` backend tool and formats the summary that
//! tool returns.

use serde_json::{Map, Value, json};

pub const HOST_CAPABILITIES_TOOL: &str = "dcc_host_capabilities";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitiesRequest {
    pub dcc_type: String,
    pub instance_id: String,
    pub keys: Vec<String>,
    pub force_refresh: bool,
}

impl CapabilitiesRequest {
    #[must_use]
    pub fn arguments(&self) -> Value {
        let mut arguments = Map::new();
        if !self.keys.is_empty() {
            arguments.insert("keys".to_string(), json!(self.keys));
        }
        if self.force_refresh {
            arguments.insert("force_refresh".to_string(), Value::Bool(true));
        }
        Value::Object(arguments)
    }
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
