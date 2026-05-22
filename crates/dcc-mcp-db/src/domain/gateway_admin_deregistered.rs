//! JSON shape persisted in gateway admin SQLite for auto-deregistered instances.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable envelope stored in SQLite for recently evicted gateway instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayDeregisteredInstanceJson {
    pub timestamp_ms: u64,
    pub reason: String,
    pub dcc_type: String,
    pub instance_id: String,
    pub entry: Value,
}
