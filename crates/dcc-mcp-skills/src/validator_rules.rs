use std::collections::HashSet;
use std::path::Path;

use dcc_mcp_models::SkillMetadata;
use dcc_mcp_utils::constants::is_supported_extension;

use crate::loader::extract_frontmatter;

use super::{IssueCategory, SkillValidationIssue, SkillValidationReport};

/// Validate a skill directory and return a structured report.
pub fn validate_skill_dir(skill_dir: &Path) -> SkillValidationReport {
    let mut report = SkillValidationReport {
        skill_dir: skill_dir.to_path_buf(),
        issues: Vec::new(),
    };

    let skill_md_path = skill_dir.join("SKILL.md");
    if !skill_md_path.is_file() {
        report.issues.push(SkillValidationIssue::error(
            IssueCategory::SkillMd,
            "SKILL.md not found",
        ));
        return report;
    }

    let content = match std::fs::read_to_string(&skill_md_path) {
        Ok(content) => content,
        Err(err) => {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::SkillMd,
                format!("Cannot read SKILL.md: {err}"),
            ));
            return report;
        }
    };

    let frontmatter = match extract_frontmatter(&content) {
        Some(frontmatter) => frontmatter,
        None => {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::SkillMd,
                "SKILL.md missing YAML frontmatter (must start with ---)",
            ));
            return report;
        }
    };

    let raw_value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(frontmatter) {
        Ok(value) => value,
        Err(err) => {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Frontmatter,
                format!("Invalid YAML frontmatter: {err}"),
            ));
            return report;
        }
    };

    let meta: SkillMetadata = match serde_yaml_ng::from_value(raw_value.clone()) {
        Ok(meta) => meta,
        Err(err) => {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Frontmatter,
                format!("Cannot deserialize frontmatter into SkillMetadata: {err}"),
            ));
            return report;
        }
    };

    validate_frontmatter(&meta, skill_dir, &mut report);
    validate_tools(&meta, &mut report);
    validate_scripts(skill_dir, &meta, &mut report);
    validate_sidecars(skill_dir, &raw_value, &mut report);
    validate_dependencies(skill_dir, &meta, &mut report);

    let legacy = detect_legacy_fields(&raw_value);
    if !legacy.is_empty() {
        report.issues.push(SkillValidationIssue::info(
            IssueCategory::Frontmatter,
            format!(
                "Legacy top-level extension field(s) detected: {:?}. \
                 Use metadata.dcc-mcp.* form instead (see migration guide).",
                legacy
            ),
        ));
    }

    report
}

fn validate_frontmatter(
    meta: &SkillMetadata,
    skill_dir: &Path,
    report: &mut SkillValidationReport,
) {
    if meta.name.is_empty() {
        report.issues.push(SkillValidationIssue::error(
            IssueCategory::Frontmatter,
            "Missing required field: name",
        ));
    }

    if meta.description.is_empty() {
        report.issues.push(SkillValidationIssue::error(
            IssueCategory::Frontmatter,
            "Missing required field: description",
        ));
    }

    if !meta.name.is_empty() {
        if meta.name.len() > 64 {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Frontmatter,
                format!("name exceeds 64 characters ({} chars)", meta.name.len()),
            ));
        }
        if !meta
            .name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Frontmatter,
                "name must be kebab-case (lowercase letters, digits, hyphens only)",
            ));
        }
        if meta.name.starts_with('-') || meta.name.ends_with('-') || meta.name.contains("--") {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Frontmatter,
                "name must not start/end with hyphen or contain consecutive hyphens",
            ));
        }

        let dir_name = skill_dir
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if meta.name != dir_name {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Frontmatter,
                format!(
                    "name ('{}') does not match directory name ('{}')",
                    meta.name, dir_name
                ),
            ));
        }
    }

    if meta.description.len() > 1024 {
        report.issues.push(SkillValidationIssue::error(
            IssueCategory::Frontmatter,
            format!(
                "description exceeds 1024 characters ({} chars)",
                meta.description.len()
            ),
        ));
    }

    if meta.compatibility.len() > 500 {
        report.issues.push(SkillValidationIssue::error(
            IssueCategory::Frontmatter,
            format!(
                "compatibility exceeds 500 characters ({} chars)",
                meta.compatibility.len()
            ),
        ));
    }

    if !meta.version.is_empty() && !is_valid_version(&meta.version) {
        report.issues.push(SkillValidationIssue::warn(
            IssueCategory::Frontmatter,
            format!(
                "version '{}' does not look like a valid semver",
                meta.version
            ),
        ));
    }
}

fn validate_tools(meta: &SkillMetadata, report: &mut SkillValidationReport) {
    let mut seen_names: HashSet<&str> = HashSet::new();
    let group_names: HashSet<&str> = meta
        .groups
        .iter()
        .map(|group| group.name.as_str())
        .collect();
    let tool_names: HashSet<&str> = meta.tools.iter().map(|tool| tool.name.as_str()).collect();

    for (idx, tool) in meta.tools.iter().enumerate() {
        if tool.name.is_empty() {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Tools,
                format!("tools[{idx}]: missing name"),
            ));
            continue;
        }

        if !seen_names.insert(&tool.name) {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Tools,
                format!("duplicate tool name: '{}'", tool.name),
            ));
        }

        if !tool
            .name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Tools,
                format!(
                    "tool '{}' name should use snake_case (lowercase, digits, underscores only)",
                    tool.name
                ),
            ));
        }

        if tool.description.is_empty() {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Tools,
                format!("tool '{}': missing description", tool.name),
            ));
        }

        if !tool.group.is_empty() && !group_names.contains(tool.group.as_str()) {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Tools,
                format!(
                    "tool '{}' references unknown group '{}'",
                    tool.name, tool.group
                ),
            ));
        }

        validate_next_tools(
            &tool.name,
            "on_success",
            &tool.next_tools.on_success,
            &tool_names,
            report,
        );
        validate_next_tools(
            &tool.name,
            "on_failure",
            &tool.next_tools.on_failure,
            &tool_names,
            report,
        );
    }
}

fn validate_next_tools(
    tool_name: &str,
    channel: &str,
    entries: &[String],
    tool_names: &HashSet<&str>,
    report: &mut SkillValidationReport,
) {
    for next in entries {
        if next.is_empty() {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Tools,
                format!("tool '{tool_name}' next_tools.{channel} contains empty entry"),
            ));
        } else if !tool_names.contains(next.as_str()) {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Tools,
                format!("tool '{tool_name}' next_tools.{channel} references unknown tool '{next}'"),
            ));
        }
    }
}

fn validate_scripts(skill_dir: &Path, meta: &SkillMetadata, report: &mut SkillValidationReport) {
    let scripts_dir = skill_dir.join("scripts");

    for tool in &meta.tools {
        if !tool.source_file.is_empty() {
            let path = skill_dir.join(&tool.source_file);
            if !path.is_file() {
                report.issues.push(SkillValidationIssue::error(
                    IssueCategory::Scripts,
                    format!(
                        "tool '{}' references source_file '{}' which does not exist",
                        tool.name, tool.source_file
                    ),
                ));
            } else if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy();
                if !is_supported_extension(&ext_str) {
                    report.issues.push(SkillValidationIssue::warn(
                        IssueCategory::Scripts,
                        format!(
                            "tool '{}' source_file '{}' has unsupported extension '.{}'",
                            tool.name, tool.source_file, ext_str
                        ),
                    ));
                }
            } else {
                report.issues.push(SkillValidationIssue::warn(
                    IssueCategory::Scripts,
                    format!(
                        "tool '{}' source_file '{}' has no extension",
                        tool.name, tool.source_file
                    ),
                ));
            }
        }
    }

    if !meta.tools.is_empty() && !scripts_dir.is_dir() {
        let has_source_files = meta.tools.iter().any(|tool| !tool.source_file.is_empty());
        if !has_source_files {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Scripts,
                "tools are declared but no scripts/ directory exists",
            ));
        }
    }
}

fn validate_sidecars(
    skill_dir: &Path,
    raw_value: &serde_yaml_ng::Value,
    report: &mut SkillValidationReport,
) {
    let Some(mapping) = raw_value.as_mapping() else {
        return;
    };

    validate_sidecar_file(skill_dir, mapping, "tools", report);
    validate_sidecar_file(skill_dir, mapping, "groups", report);
    validate_sidecar_file(skill_dir, mapping, "prompts", report);
}

fn validate_sidecar_file(
    skill_dir: &Path,
    mapping: &serde_yaml_ng::Mapping,
    key: &str,
    report: &mut SkillValidationReport,
) {
    if let Some(sidecar_ref) = mapping
        .get(serde_yaml_ng::Value::String("metadata".into()))
        .and_then(|metadata| metadata.as_mapping())
        .and_then(|metadata| metadata.get(serde_yaml_ng::Value::String("dcc-mcp".into())))
        .and_then(|dcc_mcp| dcc_mcp.as_mapping())
        .and_then(|dcc_mcp| dcc_mcp.get(serde_yaml_ng::Value::String(key.into())))
        .and_then(|value| value.as_str())
    {
        let path = skill_dir.join(sidecar_ref);
        if !path.is_file() {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Sidecars,
                format!(
                    "metadata.dcc-mcp.{key} references sidecar '{}' which does not exist",
                    sidecar_ref
                ),
            ));
        }
    }
}

fn validate_dependencies(
    skill_dir: &Path,
    meta: &SkillMetadata,
    report: &mut SkillValidationReport,
) {
    for (idx, dep) in meta.depends.iter().enumerate() {
        if dep.trim().is_empty() {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Dependencies,
                format!("depends[{idx}] is empty or whitespace-only"),
            ));
        }
    }

    if !meta.depends.is_empty() {
        let depends_md = skill_dir.join("metadata").join("depends.md");
        if !depends_md.is_file() {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Dependencies,
                "depends are declared but metadata/depends.md is missing",
            ));
        }
    }

    let depends_md = skill_dir.join("metadata").join("depends.md");
    if depends_md.is_file() && meta.depends.is_empty() {
        report.issues.push(SkillValidationIssue::warn(
            IssueCategory::Dependencies,
            "metadata/depends.md exists but no depends declared in frontmatter",
        ));
    }
}

fn detect_legacy_fields(root: &serde_yaml_ng::Value) -> Vec<String> {
    const SPEC_KEYS: &[&str] = &[
        "name",
        "description",
        "license",
        "compatibility",
        "metadata",
        "allowed-tools",
        "allowed_tools",
    ];
    const LEGACY_KEYS: &[&str] = &[
        "dcc",
        "version",
        "tags",
        "search-hint",
        "search_hint",
        "depends",
        "tools",
        "groups",
        "policy",
        "external_deps",
        "external-deps",
        "products",
        "allow_implicit_invocation",
        "allow-implicit-invocation",
    ];

    let Some(map) = root.as_mapping() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, _) in map {
        let Some(key_str) = key.as_str() else {
            continue;
        };
        if SPEC_KEYS.contains(&key_str) {
            continue;
        }
        if LEGACY_KEYS.contains(&key_str) && !out.iter().any(|seen: &String| seen == key_str) {
            out.push(key_str.to_string());
        }
    }
    out
}

fn is_valid_version(version: &str) -> bool {
    if !version.contains('.') {
        return false;
    }
    version
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == '+')
}
