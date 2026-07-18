//! Parse tracing-style `.log` files for gateway admin log merge (`GET /admin/api/logs`).

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Value, json};

const MAX_LOG_FILES_TO_SCAN: usize = 32;
const LOG_TAIL_BYTES: u64 = 256 * 1024;

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
    let (thread, body) = rest
        .split_once(' ')
        .filter(|(prefix, _)| prefix.starts_with("ThreadId("))
        .map_or((None, rest.as_str()), |(prefix, body)| {
            (Some(prefix), body.trim())
        });
    let (target, message) = body
        .split_once(": ")
        .map_or((None, body), |(target, message)| {
            (Some(target.trim()), message.trim())
        });
    Some(json!({
        "timestamp": ts,
        "level": level,
        "message": message,
        "source": "file",
        "event": null,
        "target": target,
        "thread": thread,
        "dcc_type": null,
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
    for path in recent_log_files(dir) {
        if let Ok(contents) = read_log_tail(&path, LOG_TAIL_BYTES) {
            for line in contents.lines() {
                if let Some(row) = parse_gateway_file_log_line(line) {
                    rows.push(row);
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

fn recent_log_files(dir: &str) -> Vec<PathBuf> {
    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| {
            let path = entry.path();
            if !path.extension().map(|e| e == "log").unwrap_or(false) {
                return None;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, path))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    files
        .into_iter()
        .take(MAX_LOG_FILES_TO_SCAN)
        .map(|(_, path)| path)
        .collect()
}

fn read_log_tail(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if start > 0
        && let Some(pos) = bytes.iter().position(|b| *b == b'\n')
    {
        bytes.drain(..=pos);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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
        assert_eq!(v["target"], "dcc_mcp_test");
        assert!(v["dcc_type"].is_null());
        assert_eq!(v["success"], true);
    }

    #[test]
    fn tracing_single_space_still_works() {
        let line = "2026-05-16T12:00:00.000000Z INFO dcc_mcp_gateway: Registered";
        let v = parse_gateway_file_log_line(line).expect("parsable");
        assert_eq!(v["level"], "info");
        assert_eq!(v["message"], "Registered");
        assert_eq!(v["target"], "dcc_mcp_gateway");
    }

    #[test]
    fn tracing_thread_prefix_is_separate_from_target() {
        let line = "2026-07-18T23:38:03.000000Z ERROR ThreadId(01) dcc_mcp_http_server:executor: deferred tool spent > 50 ms";
        let v = parse_gateway_file_log_line(line).expect("parsable");
        assert_eq!(v["thread"], "ThreadId(01)");
        assert_eq!(v["target"], "dcc_mcp_http_server:executor");
        assert_eq!(v["message"], "deferred tool spent > 50 ms");
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
    fn no_colon_target_yields_empty_target() {
        let line = "2026-05-16T12:00:00.000000Z INFO message without colon target";
        let v = parse_gateway_file_log_line(line).expect("parsable");
        assert!(v["dcc_type"].is_null());
        assert!(v["target"].is_null());
        assert_eq!(v["message"], "message without colon target");
    }

    #[test]
    fn read_dir_uses_tail_instead_of_full_file() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let p = dir.path().join("large.log");
        let mut contents = String::from("2026-05-16T12:00:00.000000Z INFO t: old\n");
        contents.push_str(&"x".repeat((super::LOG_TAIL_BYTES as usize) + 1024));
        contents.push('\n');
        contents.push_str("2026-05-16T13:00:00.000000Z WARN t: newest\n");
        std::fs::write(&p, contents).unwrap();

        let rows = super::read_gateway_log_dir_rows_recent(&dir.path().to_string_lossy(), 10);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["message"], "newest");
    }

    #[test]
    fn read_dir_scans_bounded_recent_file_set() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        for i in 0..(super::MAX_LOG_FILES_TO_SCAN + 4) {
            let p = dir.path().join(format!("{i:02}.log"));
            std::fs::write(
                &p,
                format!("2026-05-16T12:00:{i:02}.000000Z INFO t: row-{i}\n"),
            )
            .unwrap();
        }

        let rows = super::read_gateway_log_dir_rows_recent(&dir.path().to_string_lossy(), 100);

        assert!(rows.len() <= super::MAX_LOG_FILES_TO_SCAN);
    }
}
