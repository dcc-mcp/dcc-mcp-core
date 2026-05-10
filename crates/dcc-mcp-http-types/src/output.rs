//! Pure wire types for DCC output capture resources.
//!
//! Runtime components such as ring buffers, broadcast channels, and resource
//! producers remain in `dcc-mcp-http`.  This module only contains serialisable
//! value types shared across crate boundaries.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Which output stream an [`OutputEntry`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    /// Standard output stream.
    Stdout,
    /// Standard error stream.
    Stderr,
    /// DCC script editor / console output stream.
    ScriptEditor,
}

impl OutputStream {
    /// Return the stable wire string for this stream.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::ScriptEditor => "script_editor",
        }
    }
}

/// A single captured output line with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputEntry {
    /// Unix epoch nanoseconds when the line was captured.
    pub timestamp_ns: u128,
    /// DCC instance identifier (matches the resource URI segment).
    pub instance_id: String,
    /// Which output channel this came from.
    pub stream: OutputStream,
    /// The captured text (may include newlines).
    pub text: String,
}

impl OutputEntry {
    /// Create a new output entry with the current system timestamp.
    #[must_use]
    pub fn new(
        instance_id: impl Into<String>,
        stream: OutputStream,
        text: impl Into<String>,
    ) -> Self {
        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            timestamp_ns,
            instance_id: instance_id.into(),
            stream,
            text: text.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_stream_as_str_matches_wire_values() {
        assert_eq!(OutputStream::Stdout.as_str(), "stdout");
        assert_eq!(OutputStream::Stderr.as_str(), "stderr");
        assert_eq!(OutputStream::ScriptEditor.as_str(), "script_editor");
    }

    #[test]
    fn output_stream_serialises_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&OutputStream::Stdout).unwrap(),
            "\"stdout\""
        );
        assert_eq!(
            serde_json::to_string(&OutputStream::Stderr).unwrap(),
            "\"stderr\""
        );
        assert_eq!(
            serde_json::to_string(&OutputStream::ScriptEditor).unwrap(),
            "\"script_editor\""
        );
    }

    #[test]
    fn output_stream_deserialises_from_snake_case() {
        let stream: OutputStream = serde_json::from_str("\"script_editor\"").unwrap();
        assert_eq!(stream, OutputStream::ScriptEditor);
    }

    #[test]
    fn output_entry_new_populates_fields() {
        let entry = OutputEntry::new("maya-1", OutputStream::Stdout, "hello\n");

        assert!(entry.timestamp_ns > 0);
        assert_eq!(entry.instance_id, "maya-1");
        assert_eq!(entry.stream, OutputStream::Stdout);
        assert_eq!(entry.text, "hello\n");
    }

    #[test]
    fn output_entry_round_trips_through_json() {
        let entry = OutputEntry {
            timestamp_ns: 42,
            instance_id: "houdini-1".to_owned(),
            stream: OutputStream::Stderr,
            text: "warning".to_owned(),
        };

        let encoded = serde_json::to_string(&entry).unwrap();
        assert!(encoded.contains("\"stream\":\"stderr\""));
        let decoded: OutputEntry = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.timestamp_ns, entry.timestamp_ns);
        assert_eq!(decoded.instance_id, entry.instance_id);
        assert_eq!(decoded.stream, entry.stream);
        assert_eq!(decoded.text, entry.text);
    }
}
