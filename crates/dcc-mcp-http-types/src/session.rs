//! Pure session-scoped wire/value types.
//!
//! Runtime session storage, SSE broadcast channels, TTL eviction, and dynamic
//! tool registries remain in `dcc-mcp-http`. This module only contains
//! lightweight values used in session-scoped MCP logging.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Session-scoped MCP logging threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionLogLevel {
    /// Emit all debug/info/warning/error messages.
    Debug,
    /// Emit info/warning/error messages.
    #[default]
    Info,
    /// Emit warning/error messages.
    Warning,
    /// Emit error messages only.
    Error,
}

impl SessionLogLevel {
    /// Parse MCP log level strings (case-insensitive).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warning" | "warn" => Some(Self::Warning),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Return the canonical MCP wire string for this level.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// Whether a message at `message_level` should be emitted under this threshold.
    #[must_use]
    pub fn allows(self, message_level: Self) -> bool {
        self.rank() <= message_level.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Self::Debug => 10,
            Self::Info => 20,
            Self::Warning => 30,
            Self::Error => 40,
        }
    }
}

/// A retained per-session log message for error correlation (`details.log_tail`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionLogMessage {
    /// Message severity.
    pub level: SessionLogLevel,
    /// Logger name that emitted the message.
    pub logger: String,
    /// Structured log payload.
    pub data: Value,
    /// Optional request id used to correlate messages with a failed tool call.
    pub request_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_default_is_info() {
        assert_eq!(SessionLogLevel::default(), SessionLogLevel::Info);
    }

    #[test]
    fn log_level_parse_is_case_insensitive_and_accepts_warn_alias() {
        assert_eq!(
            SessionLogLevel::parse("DEBUG"),
            Some(SessionLogLevel::Debug)
        );
        assert_eq!(SessionLogLevel::parse("info"), Some(SessionLogLevel::Info));
        assert_eq!(
            SessionLogLevel::parse("warn"),
            Some(SessionLogLevel::Warning)
        );
        assert_eq!(
            SessionLogLevel::parse("warning"),
            Some(SessionLogLevel::Warning)
        );
        assert_eq!(
            SessionLogLevel::parse("error"),
            Some(SessionLogLevel::Error)
        );
        assert_eq!(SessionLogLevel::parse("trace"), None);
    }

    #[test]
    fn log_level_as_str_matches_mcp_wire_values() {
        assert_eq!(SessionLogLevel::Debug.as_str(), "debug");
        assert_eq!(SessionLogLevel::Info.as_str(), "info");
        assert_eq!(SessionLogLevel::Warning.as_str(), "warning");
        assert_eq!(SessionLogLevel::Error.as_str(), "error");
    }

    #[test]
    fn log_level_allows_messages_at_or_above_threshold() {
        assert!(SessionLogLevel::Debug.allows(SessionLogLevel::Debug));
        assert!(SessionLogLevel::Debug.allows(SessionLogLevel::Error));
        assert!(!SessionLogLevel::Warning.allows(SessionLogLevel::Info));
        assert!(SessionLogLevel::Warning.allows(SessionLogLevel::Error));
        assert!(SessionLogLevel::Error.allows(SessionLogLevel::Error));
    }

    #[test]
    fn log_level_serialises_as_lowercase() {
        assert_eq!(
            serde_json::to_string(&SessionLogLevel::Warning).unwrap(),
            "\"warning\""
        );
        let level: SessionLogLevel = serde_json::from_str("\"debug\"").unwrap();
        assert_eq!(level, SessionLogLevel::Debug);
    }

    #[test]
    fn session_log_message_round_trips_through_json() {
        let msg = SessionLogMessage {
            level: SessionLogLevel::Error,
            logger: "dcc.tool".to_owned(),
            data: serde_json::json!({"message": "failed"}),
            request_id: Some("req-1".to_owned()),
        };

        let encoded = serde_json::to_string(&msg).unwrap();
        let decoded: SessionLogMessage = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, msg);
    }
}
