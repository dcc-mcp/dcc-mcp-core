//! Bridges [`ActionRegistry`] metadata into MCP `tools/list` responses and
//! routes `tools/call` requests back to [`ExecutorBridge`].

use dcc_mcp_actions::ActionRegistry;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

use crate::executor::ExecutorBridge;
use crate::error::HttpError;
use crate::types::{ToolCallResponse, ToolDescription, ToolListResponse};

/// Routes MCP tool calls through the executor bridge.
///
/// It holds a reference to the [`ActionRegistry`] for metadata and
/// an [`ExecutorBridge`] for dispatching execution to the DCC main thread.
#[derive(Clone)]
pub struct ToolRouter {
    registry: Arc<ActionRegistry>,
    bridge: ExecutorBridge,
}

impl ToolRouter {
    pub fn new(registry: Arc<ActionRegistry>, bridge: ExecutorBridge) -> Self {
        Self { registry, bridge }
    }

    /// Return the MCP `tools/list` result.
    pub fn list_tools(&self) -> ToolListResponse {
        let actions = self.registry.list_actions(None);
        let tools = actions
            .into_iter()
            .map(|meta| ToolDescription {
                name: meta.name.clone(),
                description: meta.description.clone(),
                input_schema: meta.input_schema.clone(),
            })
            .collect();
        ToolListResponse { tools }
    }

    /// Dispatch a `tools/call` request.
    ///
    /// The actual Python handler is invoked on the DCC main thread via the
    /// [`ExecutorBridge`].
    pub async fn call_tool(
        &self,
        name: String,
        arguments: HashMap<String, Value>,
    ) -> Result<ToolCallResponse, HttpError> {
        // Verify the tool exists before dispatching
        let meta = self.registry.get_action(&name, None)
            .ok_or_else(|| HttpError::tool_not_found(&name))?;

        let registry = Arc::clone(&self.registry);
        let args_json = Value::Object(arguments.into_iter().collect());

        let result = self
            .bridge
            .submit(move || {
                // This closure runs on the DCC main thread.
                // Call the Python dispatcher via the registry.
                let result_json = registry.call_action_json(&name, args_json)
                    .map_err(|e| e.to_string())?;
                Ok(result_json)
            })
            .await?;

        let text = match &result {
            Value::String(s) => s.clone(),
            other => serde_json::to_string_pretty(other)
                .unwrap_or_else(|_| other.to_string()),
        };
        Ok(ToolCallResponse::success(text))
    }
}
