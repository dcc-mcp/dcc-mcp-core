//! JSON-RPC 2.0 over WebSocket protocol types for the marketplace WS bridge.
//!
//! Implements the request/response/notification envelopes plus the method
//! dispatch, error codes, and operation state machine defined in PIP-1096 M2.

#![allow(dead_code)]

use dcc_mcp_jsonrpc::error_codes::INTERNAL_ERROR;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use dcc_mcp_jsonrpc::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
#[cfg(test)]
use serde_json::Value;

// ── Application error codes ───────────────────────────────────────────────

/// Domain error codes for marketplace operations (PIP-1096 M2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum MarketplaceErrorCode {
    PackageNotFound = -32000,
    DccMismatch = -32001,
    AlreadyInstalled = -32002,
    UnsupportedInstallType = -32003,
    SourceFetchFailed = -32004,
    SkillReloadFailed = -32005,
}

impl MarketplaceErrorCode {
    pub fn message(&self) -> &'static str {
        match self {
            Self::PackageNotFound => "PACKAGE_NOT_FOUND",
            Self::DccMismatch => "DCC_MISMATCH",
            Self::AlreadyInstalled => "ALREADY_INSTALLED",
            Self::UnsupportedInstallType => "UNSUPPORTED_INSTALL_TYPE",
            Self::SourceFetchFailed => "SOURCE_FETCH_FAILED",
            Self::SkillReloadFailed => "SKILL_RELOAD_FAILED",
        }
    }

    pub fn code(self) -> i64 {
        self as i64
    }
}

/// Map a `dcc_mcp_marketplace::MarketplaceError` to the appropriate JSON-RPC
/// application error code.
pub fn marketplace_error_to_rpc(err: &dcc_mcp_marketplace::MarketplaceError) -> (i64, String) {
    use dcc_mcp_marketplace::MarketplaceError;
    let code = match err {
        MarketplaceError::NotFound(_) => MarketplaceErrorCode::PackageNotFound,
        MarketplaceError::AlreadyInstalled { .. } => MarketplaceErrorCode::AlreadyInstalled,
        MarketplaceError::DccMismatch { .. } => MarketplaceErrorCode::DccMismatch,
        MarketplaceError::UnsupportedInstallType(_) => MarketplaceErrorCode::UnsupportedInstallType,
        MarketplaceError::Fetch(..) | MarketplaceError::Archive(..) => {
            MarketplaceErrorCode::SourceFetchFailed
        }
        _ => return (INTERNAL_ERROR, err.to_string()),
    };
    (code.code(), code.message().to_string())
}

// ── Method constants ──────────────────────────────────────────────────────

pub mod methods {
    pub const HELLO: &str = "hello";
    pub const CATALOG_LIST: &str = "marketplace.catalog.list";
    pub const INSTALLED_LIST: &str = "marketplace.installed.list";
    pub const INSTALL: &str = "marketplace.install";
    pub const UNINSTALL: &str = "marketplace.uninstall";
    pub const SOURCES_LIST: &str = "marketplace.sources.list";
    pub const SOURCES_ADD: &str = "marketplace.sources.add";
    pub const SOURCES_REMOVE: &str = "marketplace.sources.remove";
    pub const SUBSCRIBE: &str = "marketplace.subscribe";
    pub const PING: &str = "ping";
}

// ── Event types ───────────────────────────────────────────────────────────

pub mod events {
    pub const INSTALLED_CHANGED: &str = "marketplace.installed.changed";
    pub const OPERATION_PROGRESS: &str = "marketplace.operation.progress";
    pub const OPERATION_COMPLETED: &str = "marketplace.operation.completed";
    pub const OPERATION_FAILED: &str = "marketplace.operation.failed";
    pub const SKILLS_RELOADED: &str = "marketplace.skills.reloaded";
    pub const CONNECTION_STATE: &str = "gateway.connection.state";
    pub const HANDOFF: &str = "gateway.handoff";
}

/// Operation state machine phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationPhase {
    Queued,
    Fetching,
    Installing,
    Removing,
    Reloading,
}

// ── Subscribe params ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct SubscribeParams {
    pub topics: Vec<String>,
}

// ── Install params ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct InstallParams {
    pub name: String,
    pub dcc: String,
    #[serde(default)]
    pub source: Option<String>,
}

// ── Uninstall params ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct UninstallParams {
    pub name: String,
    pub dcc: String,
}

// ── Sources add/remove params ─────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct AddSourceParams {
    pub source: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoveSourceParams {
    pub name: String,
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_valid_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"version":"1.0"}}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, Some(Value::Number(1.into())));
        assert_eq!(req.method, "hello");
        assert!(req.params.is_some());
    }

    #[test]
    fn deserialize_notification_no_id() {
        let json = r#"{"jsonrpc":"2.0","method":"marketplace.installed.changed"}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert!(req.id.is_none());
    }

    #[test]
    fn serialize_success_response() {
        let resp = JsonRpcResponse::success(
            Some(serde_json::Value::Number(1.into())),
            serde_json::Value::String("ok".into()),
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""jsonrpc":"2.0""#));
        assert!(json.contains(r#""id":1"#));
        assert!(json.contains(r#""result":"ok""#));
    }

    #[test]
    fn serialize_error_response() {
        let resp = JsonRpcResponse::method_not_found(
            Some(serde_json::Value::Number(1.into())),
            "bad.method",
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""code":-32601"#));
        assert!(json.contains("Method not found"));
    }

    #[test]
    fn error_code_mapping_package_not_found() {
        let err = dcc_mcp_marketplace::MarketplaceError::NotFound("pkg".into());
        let (code, msg) = marketplace_error_to_rpc(&err);
        assert_eq!(code, MarketplaceErrorCode::PackageNotFound.code());
        assert_eq!(msg, "PACKAGE_NOT_FOUND");
    }

    #[test]
    fn error_code_mapping_already_installed() {
        let err = dcc_mcp_marketplace::MarketplaceError::AlreadyInstalled {
            name: "pkg".into(),
            dcc: "maya".into(),
            path: "/tmp".into(),
        };
        let (code, msg) = marketplace_error_to_rpc(&err);
        assert_eq!(code, MarketplaceErrorCode::AlreadyInstalled.code());
        assert_eq!(msg, "ALREADY_INSTALLED");
    }

    #[test]
    fn error_code_mapping_dcc_mismatch() {
        let err = dcc_mcp_marketplace::MarketplaceError::DccMismatch {
            name: "pkg".into(),
            dcc: "maya".into(),
        };
        let (code, msg) = marketplace_error_to_rpc(&err);
        assert_eq!(code, MarketplaceErrorCode::DccMismatch.code());
        assert_eq!(msg, "DCC_MISMATCH");
    }

    #[test]
    fn error_code_mapping_unsupported_install_type() {
        let err = dcc_mcp_marketplace::MarketplaceError::UnsupportedInstallType("zip".into());
        let (code, msg) = marketplace_error_to_rpc(&err);
        assert_eq!(code, MarketplaceErrorCode::UnsupportedInstallType.code());
        assert_eq!(msg, "UNSUPPORTED_INSTALL_TYPE");
    }

    #[test]
    fn error_code_mapping_fetch_failed() {
        // Archive is also mapped to SourceFetchFailed
        let err =
            dcc_mcp_marketplace::MarketplaceError::Archive("fetch error".into(), "test".into());
        let (code, msg) = marketplace_error_to_rpc(&err);
        assert_eq!(code, MarketplaceErrorCode::SourceFetchFailed.code());
        assert_eq!(msg, "SOURCE_FETCH_FAILED");
    }

    #[test]
    fn serialize_notification() {
        let notif =
            JsonRpcNotification::new("marketplace.installed.changed", serde_json::Value::Null);
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains(r#""method":"marketplace.installed.changed""#));
        // Notification must not have an id field.
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn operation_phase_serialization_installing() {
        let phase = OperationPhase::Installing;
        let json = serde_json::to_string(&phase).unwrap();
        assert_eq!(json, r#""installing""#);
    }

    #[test]
    fn operation_phase_serialization_removing() {
        let phase = OperationPhase::Removing;
        let json = serde_json::to_string(&phase).unwrap();
        assert_eq!(json, r#""removing""#);
    }

    #[test]
    fn deserialize_subscribe_params() {
        let json = r#"{"topics":["installed","operations"]}"#;
        let params: SubscribeParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.topics.len(), 2);
    }

    #[test]
    fn deserialize_install_params() {
        let json = r#"{"name":"mgear","dcc":"maya"}"#;
        let params: InstallParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.name, "mgear");
        assert_eq!(params.dcc, "maya");
        assert!(params.source.is_none());
    }

    #[test]
    fn deserialize_install_params_with_source() {
        let json = r#"{"name":"mgear","dcc":"maya","source":"https://example.com"}"#;
        let params: InstallParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.source.unwrap(), "https://example.com");
    }
}
