//! Parse tracing-style `.log` files for gateway admin log merge (`GET /admin/api/logs`).

use serde_json::{Value, json};

/// Parse one log line (tracing / log4rs style) into an admin log row JSON object.
///
/// `tracing-subscriber` default fmt may use **runs of spaces** between fields.
/// Use `split_whitespace()` for the first two tokens, then rejoin the remainder.
pub fn parse_gateway_file_log_line(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut it = trimmed.split_whitespace();
    let ts = it.next()?;
    if !ts.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    let level = it.next()?.to_lowercase();
    let rest: String = it.collect::<Vec<_>>().join(" ");
    if rest.is_empty() {
        return None;
    }
    let (target, message) = if let Some(idx) = rest.find(':') {
        (&rest[..idx], rest[idx + 1..].trim())
    } else {
        ("", rest.as_str())
    };
    Some(json!({
        "timestamp": ts,
        "level": level,
        "message": message,
        "source": "file",
        "event": null,
        "dcc_type": if target.is_empty() { None } else { Some(target) },
        "instance_id": null,
        "request_id": null,
        "tool": null,
        "success": level == "info" || level == "debug",
        "detail": null,
        "reason": null,
    }))
}

/// Default rolling log directory for the current platform (matches `dcc_mcp_logging` layout).
#[must_use]
pub fn default_gateway_log_dir() -> String {
    #[cfg(windows)]
    {
        let profile = std::env::var("USERPROFILE").unwrap_or_default();
        format!("{}\\AppData\\Local\\dcc-mcp\\log", profile)
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/.local/share/dcc-mcp/log", home)
    }
}

/// Read `*.log` files under `dir`, parse lines, sort by timestamp descending, keep `limit` rows.
///
/// Returns an empty vector when `dir` is empty or not a directory.
#[must_use]
pub fn read_gateway_log_dir_rows_recent(dir: &str, limit: usize) -> Vec<Value> {
    if dir.is_empty() {
        return Vec::new();
    }
    if !std::fs::metadata(dir).map(|m| m.is_dir()).unwrap_or(false) {
        return Vec::new();
    }
    let mut rows: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "log").unwrap_or(false)
                && let Ok(contents) = std::fs::read_to_string(&path)
            {
                for line in contents.lines() {
                    if let Some(row) = parse_gateway_file_log_line(line) {
                        rows.push(row);
                    }
                }
            }
        }
    }
    rows.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    rows.truncate(limit);
    rows
}

#[cfg(test)]
mod tests {
    use super::parse_gateway_file_log_line;

    #[test]
    fn tracing_double_space_after_level() {
        let line = "2026-05-16T12:00:00.000000Z  INFO  dcc_mcp_test: hello admin";
        let v = parse_gateway_file_log_line(line).expect("parsable tracing line");
        assert_eq!(v["timestamp"], "2026-05-16T12:00:00.000000Z");
        assert_eq!(v["level"], "info");
        assert_eq!(v["message"], "hello admin");
        assert_eq!(v["dcc_type"], "dcc_mcp_test");
        assert_eq!(v["success"], true);
    }

    #[test]
    fn tracing_single_space_still_works() {
        let line = "2026-05-16T12:00:00.000000Z INFO dcc_mcp_gateway: Registered";
        let v = parse_gateway_file_log_line(line).expect("parsable");
        assert_eq!(v["level"], "info");
        assert_eq!(v["message"], "Registered");
        assert_eq!(v["dcc_type"], "dcc_mcp_gateway");
    }

    #[test]
    fn rejects_non_timestamp_lines() {
        assert!(parse_gateway_file_log_line("not-a-log-line").is_none());
        assert!(parse_gateway_file_log_line("").is_none());
    }

    #[test]
    fn read_dir_respects_limit() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.log");
        std::fs::write(
            &p,
            "2026-05-16T12:00:00.000000Z INFO t: one\n2026-05-16T13:00:00.000000Z WARN t: two\n",
        )
        .unwrap();
        let rows = super::read_gateway_log_dir_rows_recent(&dir.path().to_string_lossy(), 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["message"], "two");
    }

    #[test]
    fn read_dir_empty_string_returns_empty() {
        let rows = super::read_gateway_log_dir_rows_recent("", 100);
        assert!(rows.is_empty());
    }

    #[test]
    fn read_dir_nonexistent_returns_empty() {
        let rows = super::read_gateway_log_dir_rows_recent("/nonexistent_dir_12345", 100);
        assert!(rows.is_empty());
    }

    #[test]
    fn error_level_has_success_false() {
        let line = "2026-05-16T12:00:00.000000Z ERROR target: something failed";
        let v = parse_gateway_file_log_line(line).expect("parsable");
        assert_eq!(v["level"], "error");
        assert_eq!(v["success"], false);
    }

    #[test]
    fn no_colon_target_yields_empty_dcc_type() {
        let line = "2026-05-16T12:00:00.000000Z INFO message without colon target";
        let v = parse_gateway_file_log_line(line).expect("parsable");
        assert!(v["dcc_type"].is_null());
        assert_eq!(v["message"], "message without colon target");
    }
}
