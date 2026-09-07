//! Resource and prompt dispatch through the same providers as the legacy path.

use dcc_mcp_jsonrpc::{
    GetPromptParams, JsonRpcRequest, JsonRpcResponse, ListPromptsResult, ListResourcesResult,
    RESOURCE_NOT_ENABLED_ERROR, ReadResourceParams, decode_cursor, encode_cursor, error_codes,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::rmcp_providers::ProviderError;
use crate::rmcp_registry_context::RegistryContext;
use crate::server_state::ServerState;

const LIST_PAGE_SIZE: usize = 64;

pub(super) fn handle_request(
    state: &ServerState,
    context: &RegistryContext,
    request: &JsonRpcRequest,
    id: Value,
) -> Value {
    let enabled = match request.method.as_str() {
        "resources/list" | "resources/read" => {
            state.features.enable_resources && context.resource_provider.is_some()
        }
        "prompts/list" | "prompts/get" => {
            state.features.enable_prompts && context.prompt_provider.is_some()
        }
        _ => false,
    };
    if !enabled {
        return serde_json::to_value(JsonRpcResponse::method_not_found(Some(id), &request.method))
            .unwrap_or(Value::Null);
    }
    let result = dispatch(state, context, request);
    let response = match result {
        Ok(result) => JsonRpcResponse::success(Some(id), result),
        Err(error) => {
            let (code, message) = match &error {
                ProviderError::NotFound(_) => {
                    (error_codes::INVALID_PARAMS, "Resource or prompt not found")
                }
                ProviderError::NotEnabled(_) => (
                    RESOURCE_NOT_ENABLED_ERROR,
                    "Requested provider resource is not enabled",
                ),
                ProviderError::MissingArg(_) => (
                    error_codes::INVALID_PARAMS,
                    "Invalid or missing provider parameters",
                ),
                ProviderError::Internal(_) => {
                    (error_codes::INTERNAL_ERROR, "Provider operation failed")
                }
            };
            tracing::debug!(error = %error, method = %request.method, "stateless provider request failed");
            JsonRpcResponse::error(Some(id), code, message)
        }
    };
    serde_json::to_value(response).unwrap_or(Value::Null)
}

fn invalid_params() -> ProviderError {
    ProviderError::MissingArg("Invalid provider parameters".to_string())
}

fn serialize<T: Serialize>(value: T) -> Result<Value, ProviderError> {
    serde_json::to_value(value)
        .map_err(|_| ProviderError::Internal("Result serialization failed".into()))
}

fn dispatch(
    state: &ServerState,
    context: &RegistryContext,
    request: &JsonRpcRequest,
) -> Result<Value, ProviderError> {
    match request.method.as_str() {
        "resources/list" | "resources/read" => {
            let provider = context
                .resource_provider
                .as_ref()
                .filter(|_| state.features.enable_resources)
                .ok_or_else(|| ProviderError::NotEnabled("Resources".into()))?;
            if request.method == "resources/list" {
                let offset = list_offset(request)?;
                let mut resources = provider.list_resources(&state.catalog);
                resources.sort_by(|a, b| a.uri.cmp(&b.uri));
                let (resources, next_cursor) = page(resources, offset)?;
                serialize(ListResourcesResult {
                    resources,
                    next_cursor,
                })
            } else {
                let params: ReadResourceParams =
                    serde_json::from_value(request.params.clone().ok_or_else(invalid_params)?)
                        .map_err(|_| invalid_params())?;
                if params.uri.trim().is_empty() {
                    return Err(invalid_params());
                }
                serialize(provider.read_resource(&params.uri, &state.catalog)?)
            }
        }
        "prompts/list" | "prompts/get" => {
            let provider = context
                .prompt_provider
                .as_ref()
                .filter(|_| state.features.enable_prompts)
                .ok_or_else(|| ProviderError::NotEnabled("Prompts".into()))?;
            if request.method == "prompts/list" {
                let offset = list_offset(request)?;
                let mut prompts = provider.list_prompts(&state.catalog);
                let meta = if prompts.is_empty() {
                    provider
                        .prompt_diagnostics(&state.catalog)
                        .filter(Value::is_object)
                        .map(|diagnostics| json!({"dcc.prompt_diagnostics": diagnostics}))
                } else {
                    None
                };
                prompts.sort_by(|a, b| a.name.cmp(&b.name));
                let (prompts, next_cursor) = page(prompts, offset)?;
                serialize(ListPromptsResult {
                    meta,
                    prompts,
                    next_cursor,
                })
            } else {
                let params: GetPromptParams =
                    serde_json::from_value(request.params.clone().ok_or_else(invalid_params)?)
                        .map_err(|_| invalid_params())?;
                if params.name.trim().is_empty() {
                    return Err(invalid_params());
                }
                serialize(provider.get_prompt(&params.name, &params.arguments, &state.catalog)?)
            }
        }
        _ => Err(ProviderError::NotEnabled("Method".into())),
    }
}

fn list_offset(request: &JsonRpcRequest) -> Result<usize, ProviderError> {
    let Some(params) = request.params.as_ref() else {
        return Ok(0);
    };
    let params = params.as_object().ok_or_else(invalid_params)?;
    let Some(cursor) = params.get("cursor") else {
        return Ok(0);
    };
    let cursor = cursor.as_str().ok_or_else(invalid_params)?;
    // The shared decoder slices ASCII byte pairs; reject other input before it.
    if cursor.len() > 40 || !cursor.is_ascii() {
        return Err(invalid_params());
    }
    decode_cursor(cursor).ok_or_else(invalid_params)
}

fn page<T>(rows: Vec<T>, offset: usize) -> Result<(Vec<T>, Option<String>), ProviderError> {
    if offset > rows.len() {
        return Err(invalid_params());
    }
    let end = offset.saturating_add(LIST_PAGE_SIZE).min(rows.len());
    let next_cursor = (end < rows.len()).then(|| encode_cursor(end));
    Ok((
        rows.into_iter().skip(offset).take(LIST_PAGE_SIZE).collect(),
        next_cursor,
    ))
}

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;
