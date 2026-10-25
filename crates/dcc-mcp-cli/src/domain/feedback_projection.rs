//! Default diagnostic projection: retain typed categories, never arbitrary text.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value};

const MAX_DEPTH: usize = 6;
const MAX_ITEMS: usize = 64;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectionReview {
    pub mode: &'static str,
    pub included_fields: usize,
    /// Counts use fixed categories; input keys and values are never echoed.
    pub omitted_fields: BTreeMap<&'static str, usize>,
    pub review_required: bool,
    pub secret_free_guarantee: bool,
    pub limitation: &'static str,
}

pub struct DiagnosticProjection {
    public_identifiers: BTreeSet<String>,
    review: ProjectionReview,
}

impl DiagnosticProjection {
    pub fn new(public_identifiers: impl IntoIterator<Item = String>) -> Self {
        Self {
            public_identifiers: public_identifiers.into_iter().collect(),
            review: ProjectionReview {
                mode: "allowlist",
                included_fields: 0,
                omitted_fields: BTreeMap::new(),
                review_required: true,
                secret_free_guarantee: false,
                limitation: "This is a minimal diagnostic summary, not a publishable Finding or a guarantee that all confidential information has been detected. Review the exact issue draft separately before authorizing publication.",
            },
        }
    }

    pub fn project(&mut self, value: &Value) -> Value {
        self.object(value, 0)
    }

    pub fn finish(self) -> ProjectionReview {
        self.review
    }

    fn omit(&mut self, category: &'static str) {
        *self.review.omitted_fields.entry(category).or_default() += 1;
    }

    fn omitted_tree(&mut self, key: &str, value: &Value, depth: usize) {
        self.omit(omission_category(key));
        if depth >= MAX_DEPTH {
            return;
        }
        match value {
            Value::Object(fields) => {
                for (key, value) in fields.iter().take(MAX_ITEMS) {
                    self.omitted_tree(key, value, depth + 1);
                }
            }
            Value::Array(items) => {
                for item in items.iter().take(MAX_ITEMS) {
                    if let Value::Object(fields) = item {
                        for (key, value) in fields.iter().take(MAX_ITEMS) {
                            self.omitted_tree(key, value, depth + 1);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn object(&mut self, value: &Value, depth: usize) -> Value {
        let Some(fields) = value.as_object() else {
            self.omit("invalid_or_free_text_values");
            return Value::Object(Map::new());
        };
        if depth >= MAX_DEPTH {
            self.omit("bounded_collections");
            return Value::Object(Map::new());
        }
        let mut result = Map::new();
        for (key, value) in fields {
            if let Some(projected) = self.field(key, value, depth) {
                result.insert(key.clone(), projected);
            } else {
                self.omitted_tree(key, value, depth);
            }
        }
        Value::Object(result)
    }

    fn field(&mut self, key: &str, value: &Value, depth: usize) -> Option<Value> {
        if matches!(
            key,
            "cli"
                | "control"
                | "local"
                | "inventory"
                | "adapter_imports"
                | "gateway"
                | "server_binary"
                | "version_matrix"
                | "verify"
                | "rollback"
                | "evidence"
                | "error"
                | "direct_control"
                | "installed"
                | "runtime"
                | "bundled_core"
                | "status"
        ) && value.is_object()
        {
            return Some(self.object(value, depth + 1));
        }
        if matches!(key, "probes" | "steps" | "records") {
            let items = value.as_array()?;
            if items.len() > MAX_ITEMS {
                self.omit("bounded_collections");
            }
            let mut projected = Vec::new();
            for item in items.iter().take(MAX_ITEMS) {
                if item.is_object() {
                    projected.push(self.object(item, depth + 1));
                } else {
                    self.omit("commands_and_free_text");
                }
            }
            return Some(Value::Array(projected));
        }
        let projected = if matches!(
            key,
            "version"
                | "adapter_version"
                | "core_version"
                | "host_version"
                | "python_version"
                | "dist_version"
                | "module_version"
                | "min_core_version"
        ) {
            let raw = value.as_str()?;
            let version = public_version(raw)?;
            if raw.contains('+') {
                self.omit("version_build_metadata");
            } else if version != raw {
                self.omit("version_normalized");
            }
            Value::String(version)
        } else if matches!(key, "dcc_type" | "adapter" | "distribution" | "name") {
            let text = value.as_str()?;
            if !self.public_identifiers.contains(text) {
                return None;
            }
            Value::String(text.to_owned())
        } else if matches!(
            key,
            "ok" | "healthy"
                | "running"
                | "ready"
                | "imported"
                | "found"
                | "attempted"
                | "directly_usable"
                | "auto_start_enabled"
                | "gateway_stats_recorded"
                | "payload_fields_excluded"
        ) {
            Value::Bool(value.as_bool()?)
        } else if matches!(
            key,
            "total"
                | "count"
                | "failures"
                | "unavailable"
                | "failure_count"
                | "evicted"
                | "not_ready"
                | "exit_code"
        ) {
            Value::Number(value.as_i64()?.into())
        } else if matches!(key, "exception_type" | "error_kind" | "error_type") {
            let text = value
                .as_str()?
                .strip_prefix("builtins.")
                .unwrap_or(value.as_str()?);
            let category = match text {
                "ImportError" | "ModuleNotFoundError" | "import_error" => "import_error",
                "TimeoutError" | "timeout" => "timeout",
                "PermissionError" | "permission_error" => "permission_error",
                "FileNotFoundError" | "missing_file" => "missing_file",
                "TypeError" | "ValueError" | "invalid_value" => "invalid_value",
                "RuntimeError" | "startup_failed" | "runtime_error" => "runtime_error",
                "unclassified" => "unclassified",
                _ => {
                    self.omit("unrecognized_error_text");
                    "unclassified"
                }
            };
            Value::String(category.to_owned())
        } else if matches!(
            key,
            "status"
                | "phase"
                | "stage"
                | "failure_stage"
                | "severity"
                | "source"
                | "python_source"
                | "dist_source"
                | "route"
                | "stream"
                | "level"
                | "event"
                | "os"
        ) {
            let text = value.as_str()?;
            if !known_category(text) {
                return None;
            }
            Value::String(text.to_owned())
        } else {
            return None;
        };
        self.review.included_fields += 1;
        Some(projected)
    }
}

fn known_category(value: &str) -> bool {
    matches!(
        value,
        "ok" | "success"
            | "succeeded"
            | "failed"
            | "ready"
            | "not_ready"
            | "unknown"
            | "unavailable"
            | "not_collected"
            | "degraded"
            | "missing"
            | "unimportable"
            | "version_mismatch"
            | "incompatible_core"
            | "blocked"
            | "not_available"
            | "not_attempted"
            | "install"
            | "startup"
            | "dispatch"
            | "skill"
            | "other"
            | "bootstrap"
            | "register"
            | "verify"
            | "detect"
            | "plan"
            | "download"
            | "rollback"
            | "blocker"
            | "workaround_found"
            | "suggestion"
            | "bundled"
            | "cache"
            | "explicit"
            | "local_registry"
            | "registry"
            | "cli_override"
            | "environment"
            | "inventory"
            | "importlib.metadata"
            | "pkg_resources"
            | "dist_info"
            | "dist_info_dir"
            | "adapter_bootstrap"
            | "local_mcp_direct"
            | "gateway"
            | "stdout"
            | "stderr"
            | "error"
            | "warning"
            | "info"
            | "debug"
            | "critical"
            | "dcc_host_error"
            | "windows"
            | "linux"
            | "macos"
    )
}

/// Retain release components; never return arbitrary SemVer build identifiers.
fn public_version(raw: &str) -> Option<String> {
    if raw.len() > 128 {
        return None;
    }
    let release = raw
        .split('+')
        .next()?
        .strip_prefix('v')
        .unwrap_or(raw.split('+').next()?);
    if let Ok(mut version) = semver::Version::parse(release) {
        if version.pre.as_str().split('.').any(|part| {
            !part.is_empty()
                && !part.bytes().all(|byte| byte.is_ascii_digit())
                && !matches!(part, "alpha" | "beta" | "rc" | "dev" | "post")
        }) {
            return None;
        }
        version.build = semver::BuildMetadata::EMPTY;
        return Some(version.to_string());
    }
    let parts = release.split('.').collect::<Vec<_>>();
    if !(1..=2).contains(&parts.len()) {
        return None;
    }
    parts
        .iter()
        .map(|part| part.parse::<u32>().ok().map(|number| number.to_string()))
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("."))
}

fn omission_category(key: &str) -> &'static str {
    let key = key.to_ascii_lowercase();
    if [
        "token",
        "secret",
        "password",
        "credential",
        "authorization",
        "api_key",
    ]
    .iter()
    .any(|part| key.contains(part))
    {
        "credentials"
    } else if key.contains("path")
        || key.ends_with("_dir")
        || matches!(key.as_str(), "home" | "directory")
    {
        "paths_and_private_files"
    } else if key.contains("prompt") || key.contains("chat") || key.contains("script") {
        "prompts_chats_and_scripts"
    } else if matches!(
        key.as_str(),
        "intent"
            | "observed"
            | "expected"
            | "message"
            | "traceback"
            | "repro"
            | "argv"
            | "next_steps"
    ) {
        "commands_and_free_text"
    } else if key.contains("user")
        || key.contains("machine")
        || key.contains("host")
        || key.ends_with("_id")
        || key.ends_with("_pid")
        || key == "fingerprint"
    {
        "private_identifiers"
    } else if key.contains("url") || key.contains("endpoint") || key == "port" {
        "endpoints"
    } else if key.contains("env") {
        "environment"
    } else {
        "unrecognized_fields_or_values"
    }
}

#[cfg(test)]
#[path = "feedback_projection_tests.rs"]
mod tests;
