//! Privacy-safe skill-path projections for the admin dashboard.

use std::hash::{Hash, Hasher};
use std::path::Path;

use serde_json::{Value, json};

/// Project a raw skill path into a privacy-safe admin row.
#[must_use]
pub fn skill_path_row(path: &str, source: &str, id: Option<i64>, ordinal: usize) -> Value {
    let hash = skill_path_hash(path);
    let status = skill_path_status(path);
    let source_label = friendly_source_label(source);
    let tail = safe_path_tail(path);
    let display_path = if tail.is_empty() {
        format!("{source_label} #{}", id.unwrap_or(ordinal as i64))
    } else {
        format!("{source_label} · {tail}")
    };
    let mut row = json!({
        "path": display_path,
        "display_path": display_path,
        "source_label": source_label,
        "path_tail": tail,
        "path_alias": format!("skill-path:{hash}"),
        "path_hash": hash,
        "path_redacted": true,
        "source": source,
        "status": status,
        "exists": status == "present",
        "package": Value::Null,
        "version": Value::Null,
    });
    if let Some(id) = id
        && let Some(obj) = row.as_object_mut()
    {
        obj.insert("id".to_string(), json!(id));
    }
    row
}

/// Return the stable, normalized identifier used to deduplicate skill paths.
#[must_use]
pub fn skill_path_hash(path: &str) -> String {
    let mut hasher = StableHasher::new();
    path.replace('\\', "/")
        .to_ascii_lowercase()
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn friendly_source_label(source: &str) -> String {
    if source.trim().is_empty() {
        return "Skill path".to_string();
    }
    let normalized: String = source
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    let key = normalized.split_whitespace().next().unwrap_or("");
    match key {
        "bundled" => "Bundled".to_string(),
        "admin" | "admincustom" => "Admin custom".to_string(),
        "env" | "envvar" => "Env var".to_string(),
        "explicit" | "explicitarg" => "Explicit arg".to_string(),
        "local" | "localdev" => "Local dev".to_string(),
        "platform" => "Platform".to_string(),
        "user" => "User".to_string(),
        "team" => "Team".to_string(),
        "repo" => "Repo".to_string(),
        "system" => "System".to_string(),
        _ => {
            let safe = safe_source_label(source);
            let mut chars = safe.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => "Skill path".to_string(),
            }
        }
    }
}

fn safe_path_tail(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let components: Vec<&str> = normalized
        .split('/')
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        .filter(|c| !(c.len() == 2 && c.ends_with(':')))
        .collect();
    let take = components.len().min(2);
    components[components.len() - take..]
        .iter()
        .map(|c| {
            if !username.is_empty() && c.to_ascii_lowercase() == username {
                "~".to_string()
            } else {
                (*c).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn safe_source_label(source: &str) -> String {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return "skill_path".to_string();
    }
    trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.') {
                ch
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

fn skill_path_status(path: &str) -> &'static str {
    if path.trim().is_empty() {
        return "missing";
    }
    if Path::new(path).exists() {
        "present"
    } else {
        "missing"
    }
}

struct StableHasher(u64);

impl StableHasher {
    fn new() -> Self {
        Self(0xcbf29ce484222325)
    }
}

impl Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_redacted_and_labelled() {
        let row = skill_path_row("G:/studio/pipeline/skills", "bundled", None, 1);
        assert_eq!(row["path_redacted"], json!(true));
        assert_eq!(row["source_label"], json!("Bundled"));
        assert_eq!(row["path_tail"], json!("pipeline/skills"));
        assert!(!row["display_path"].as_str().unwrap().contains("G:/studio"));
    }

    #[test]
    fn rows_redact_username_components() {
        let _guard = dcc_mcp_test_utils::EnvVarGuard::set("USERNAME", Some("alice"));
        let row = skill_path_row("C:/Users/alice/skills", "user", None, 1);
        assert_eq!(row["path_tail"], json!("~/skills"));
    }

    #[test]
    fn hashes_normalize_case_and_separators() {
        assert_eq!(
            skill_path_hash("C:\\Studio\\Skills"),
            skill_path_hash("c:/studio/skills")
        );
    }
}
