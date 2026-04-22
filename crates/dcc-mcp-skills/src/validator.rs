//! Skill directory validator — structured linting for SKILL.md and adjacent files.
//!
//! Inspired by Anthropic's skill-creator `quick_validate.py`, this module provides
//! a programmatic way to check that a skill directory follows the dcc-mcp-core
//! specification before it is loaded at runtime.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use dcc_mcp_models::SkillMetadata;
use dcc_mcp_utils::constants::is_supported_extension;

/// Severity level of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Hard error — the skill cannot be loaded or will malfunction.
    Error,
    /// Warning — the skill loads but violates a best-practice or spec recommendation.
    Warning,
    /// Info — purely informational, e.g. deprecation notices.
    Info,
}

/// Category of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueCategory {
    /// SKILL.md file itself (missing, unreadable, malformed frontmatter).
    SkillMd,
    /// YAML frontmatter fields (missing required fields, wrong types, bad values).
    Frontmatter,
    /// Script files in `scripts/` (missing, unsupported extension, referenced but absent).
    Scripts,
    /// Metadata files in `metadata/` (missing depends.md, etc.).
    Metadata,
    /// Tool declarations in `tools:` (missing names, descriptions, bad schema).
    Tools,
    /// Sidecar file references (`metadata.dcc-mcp.tools`, etc.).
    Sidecars,
    /// Dependency declarations (`depends`, `metadata/depends.md`).
    Dependencies,
}

/// A single validation issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillValidationIssue {
    pub severity: IssueSeverity,
    pub category: IssueCategory,
    pub message: String,
}

impl SkillValidationIssue {
    fn error(category: IssueCategory, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            category,
            message: message.into(),
        }
    }

    fn warn(category: IssueCategory, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            category,
            message: message.into(),
        }
    }

    fn info(category: IssueCategory, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Info,
            category,
            message: message.into(),
        }
    }
}

/// Complete validation report for a skill directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillValidationReport {
    pub skill_dir: PathBuf,
    pub issues: Vec<SkillValidationIssue>,
}

impl SkillValidationReport {
    /// Returns `true` if the report contains at least one error.
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error)
    }

    /// Returns `true` if the report contains no errors and no warnings.
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    /// Count issues by severity.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut errors = 0;
        let mut warnings = 0;
        let mut infos = 0;
        for i in &self.issues {
            match i.severity {
                IssueSeverity::Error => errors += 1,
                IssueSeverity::Warning => warnings += 1,
                IssueSeverity::Info => infos += 1,
            }
        }
        (errors, warnings, infos)
    }
}

/// Validate a skill directory and return a structured report.
///
/// This checks:
/// 1. `SKILL.md` exists and is readable.
/// 2. YAML frontmatter is well-formed and contains required fields.
/// 3. Field values follow naming and length constraints.
/// 4. Declared script files exist in `scripts/`.
/// 5. Declared sidecar files (tools, groups, prompts) exist.
/// 6. Dependency declarations are consistent.
/// 7. No legacy extension fields are present (info-level deprecation notice).
pub fn validate_skill_dir(skill_dir: &Path) -> SkillValidationReport {
    let mut report = SkillValidationReport {
        skill_dir: skill_dir.to_path_buf(),
        issues: Vec::new(),
    };

    // 1. File existence
    let skill_md_path = skill_dir.join("SKILL.md");
    if !skill_md_path.is_file() {
        report.issues.push(SkillValidationIssue::error(
            IssueCategory::SkillMd,
            "SKILL.md not found",
        ));
        return report;
    }

    let content = match std::fs::read_to_string(&skill_md_path) {
        Ok(c) => c,
        Err(e) => {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::SkillMd,
                format!("Cannot read SKILL.md: {e}"),
            ));
            return report;
        }
    };

    // 2. Frontmatter extraction
    let frontmatter = match extract_frontmatter(&content) {
        Some(f) => f,
        None => {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::SkillMd,
                "SKILL.md missing YAML frontmatter (must start with ---)",
            ));
            return report;
        }
    };

    // 3. YAML parse
    let raw_value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(frontmatter) {
        Ok(v) => v,
        Err(e) => {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Frontmatter,
                format!("Invalid YAML frontmatter: {e}"),
            ));
            return report;
        }
    };

    let meta: SkillMetadata = match serde_yaml_ng::from_value(raw_value.clone()) {
        Ok(m) => m,
        Err(e) => {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Frontmatter,
                format!("Cannot deserialize frontmatter into SkillMetadata: {e}"),
            ));
            return report;
        }
    };

    // 4. Field-level validation
    validate_frontmatter(&meta, skill_dir, &mut report);

    // 5. Tool declarations
    validate_tools(&meta, &mut report);

    // 6. Script existence
    validate_scripts(skill_dir, &meta, &mut report);

    // 7. Sidecar file references
    validate_sidecars(skill_dir, &raw_value, &mut report);

    // 8. Dependency consistency
    validate_dependencies(skill_dir, &meta, &mut report);

    // 9. Legacy extension fields (info)
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
    // Required fields
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

    // Name format: kebab-case, <=64 chars, no leading/trailing hyphen, no double hyphen
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
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
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

        // Name should match directory name
        let dir_name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
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

    // Description length
    if meta.description.len() > 1024 {
        report.issues.push(SkillValidationIssue::error(
            IssueCategory::Frontmatter,
            format!(
                "description exceeds 1024 characters ({} chars)",
                meta.description.len()
            ),
        ));
    }

    // Compatibility length
    if meta.compatibility.len() > 500 {
        report.issues.push(SkillValidationIssue::error(
            IssueCategory::Frontmatter,
            format!(
                "compatibility exceeds 500 characters ({} chars)",
                meta.compatibility.len()
            ),
        ));
    }

    // Version format (basic semver-like check)
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
    let group_names: HashSet<&str> = meta.groups.iter().map(|g| g.name.as_str()).collect();
    let tool_names: HashSet<&str> = meta.tools.iter().map(|t| t.name.as_str()).collect();

    for (idx, tool) in meta.tools.iter().enumerate() {
        // Name required
        if tool.name.is_empty() {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Tools,
                format!("tools[{}]: missing name", idx),
            ));
            continue;
        }

        // Duplicate detection
        if !seen_names.insert(&tool.name) {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Tools,
                format!("duplicate tool name: '{}'", tool.name),
            ));
        }

        // Name format: snake_case
        if !tool
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Tools,
                format!(
                    "tool '{}' name should use snake_case (lowercase, digits, underscores only)",
                    tool.name
                ),
            ));
        }

        // Description
        if tool.description.is_empty() {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Tools,
                format!("tool '{}': missing description", tool.name),
            ));
        }

        // Group reference validation
        if !tool.group.is_empty() && !group_names.contains(tool.group.as_str()) {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Tools,
                format!(
                    "tool '{}' references unknown group '{}'",
                    tool.name, tool.group
                ),
            ));
        }

        // next_tools validation: entries must exist and not be empty
        for next in &tool.next_tools.on_success {
            if next.is_empty() {
                report.issues.push(SkillValidationIssue::warn(
                    IssueCategory::Tools,
                    format!(
                        "tool '{}' next_tools.on_success contains empty entry",
                        tool.name
                    ),
                ));
            } else if !tool_names.contains(next.as_str()) {
                report.issues.push(SkillValidationIssue::warn(
                    IssueCategory::Tools,
                    format!(
                        "tool '{}' next_tools.on_success references unknown tool '{}'",
                        tool.name, next
                    ),
                ));
            }
        }
        for next in &tool.next_tools.on_failure {
            if next.is_empty() {
                report.issues.push(SkillValidationIssue::warn(
                    IssueCategory::Tools,
                    format!(
                        "tool '{}' next_tools.on_failure contains empty entry",
                        tool.name
                    ),
                ));
            } else if !tool_names.contains(next.as_str()) {
                report.issues.push(SkillValidationIssue::warn(
                    IssueCategory::Tools,
                    format!(
                        "tool '{}' next_tools.on_failure references unknown tool '{}'",
                        tool.name, next
                    ),
                ));
            }
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
            } else {
                // Check extension is supported
                if let Some(ext) = path.extension() {
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
    }

    // Warn if scripts/ directory is missing but tools are declared without source_file
    if !meta.tools.is_empty() && !scripts_dir.is_dir() {
        let has_source_files = meta.tools.iter().any(|t| !t.source_file.is_empty());
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

    // metadata.dcc-mcp.tools sidecar
    if let Some(tools_ref) = mapping
        .get(serde_yaml_ng::Value::String("metadata".into()))
        .and_then(|m| m.as_mapping())
        .and_then(|m| m.get(serde_yaml_ng::Value::String("dcc-mcp".into())))
        .and_then(|d| d.as_mapping())
        .and_then(|d| d.get(serde_yaml_ng::Value::String("tools".into())))
        .and_then(|v| v.as_str())
    {
        let path = skill_dir.join(tools_ref);
        if !path.is_file() {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Sidecars,
                format!(
                    "metadata.dcc-mcp.tools references sidecar '{}' which does not exist",
                    tools_ref
                ),
            ));
        }
    }

    // metadata.dcc-mcp.groups sidecar
    if let Some(groups_ref) = mapping
        .get(serde_yaml_ng::Value::String("metadata".into()))
        .and_then(|m| m.as_mapping())
        .and_then(|m| m.get(serde_yaml_ng::Value::String("dcc-mcp".into())))
        .and_then(|d| d.as_mapping())
        .and_then(|d| d.get(serde_yaml_ng::Value::String("groups".into())))
        .and_then(|v| v.as_str())
    {
        let path = skill_dir.join(groups_ref);
        if !path.is_file() {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Sidecars,
                format!(
                    "metadata.dcc-mcp.groups references sidecar '{}' which does not exist",
                    groups_ref
                ),
            ));
        }
    }

    // metadata.dcc-mcp.prompts sidecar
    if let Some(prompts_ref) = mapping
        .get(serde_yaml_ng::Value::String("metadata".into()))
        .and_then(|m| m.as_mapping())
        .and_then(|m| m.get(serde_yaml_ng::Value::String("dcc-mcp".into())))
        .and_then(|d| d.as_mapping())
        .and_then(|d| d.get(serde_yaml_ng::Value::String("prompts".into())))
        .and_then(|v| v.as_str())
    {
        let path = skill_dir.join(prompts_ref);
        if !path.is_file() {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Sidecars,
                format!(
                    "metadata.dcc-mcp.prompts references sidecar '{}' which does not exist",
                    prompts_ref
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
    // Check for empty dependency entries
    for (idx, dep) in meta.depends.iter().enumerate() {
        if dep.trim().is_empty() {
            report.issues.push(SkillValidationIssue::error(
                IssueCategory::Dependencies,
                format!("depends[{}] is empty or whitespace-only", idx),
            ));
        }
    }

    // If depends is declared, check that metadata/depends.md also exists (best practice)
    if !meta.depends.is_empty() {
        let depends_md = skill_dir.join("metadata").join("depends.md");
        if !depends_md.is_file() {
            report.issues.push(SkillValidationIssue::warn(
                IssueCategory::Dependencies,
                "depends are declared but metadata/depends.md is missing",
            ));
        }
    }

    // Check for depends.md without depends field
    let depends_md = skill_dir.join("metadata").join("depends.md");
    if depends_md.is_file() && meta.depends.is_empty() {
        report.issues.push(SkillValidationIssue::warn(
            IssueCategory::Dependencies,
            "metadata/depends.md exists but no depends declared in frontmatter",
        ));
    }
}

fn extract_frontmatter(content: &str) -> Option<&str> {
    const DELIMITER: &str = "---";
    if !content.starts_with(DELIMITER) {
        return None;
    }
    let after_first = &content[DELIMITER.len()..];
    let end = after_first.find("\n---")?;
    Some(after_first[..end].trim())
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
    for (key, _) in map.iter() {
        let Some(k) = key.as_str() else { continue };
        if SPEC_KEYS.contains(&k) {
            continue;
        }
        if LEGACY_KEYS.contains(&k) && !out.iter().any(|x: &String| x == k) {
            out.push(k.to_string());
        }
    }
    out
}

fn is_valid_version(v: &str) -> bool {
    // Very permissive: must contain at least one dot and only valid chars
    if !v.contains('.') {
        return false;
    }
    v.chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '+')
}

// ── Python bindings ──────────────────────────────────────────────────

#[cfg(feature = "python-bindings")]
mod python {
    use pyo3::prelude::*;

    use super::*;

    #[pyclass(name = "SkillValidationIssue", from_py_object)]
    #[derive(Clone)]
    pub struct PySkillValidationIssue {
        #[pyo3(get)]
        pub severity: String,
        #[pyo3(get)]
        pub category: String,
        #[pyo3(get)]
        pub message: String,
    }

    #[pyclass(name = "SkillValidationReport", from_py_object)]
    #[derive(Clone)]
    pub struct PySkillValidationReport {
        #[pyo3(get)]
        pub skill_dir: String,
        #[pyo3(get)]
        pub issues: Vec<PySkillValidationIssue>,
        #[pyo3(get)]
        pub has_errors: bool,
        #[pyo3(get)]
        pub is_clean: bool,
    }

    #[pyfunction(name = "validate_skill")]
    pub fn py_validate_skill(skill_dir: &str) -> PyResult<PySkillValidationReport> {
        let path = Path::new(skill_dir);
        let report = validate_skill_dir(path);
        Ok(report.into())
    }

    impl From<SkillValidationReport> for PySkillValidationReport {
        fn from(r: SkillValidationReport) -> Self {
            Self {
                skill_dir: r.skill_dir.to_string_lossy().to_string(),
                has_errors: r.has_errors(),
                is_clean: r.is_clean(),
                issues: r.issues.into_iter().map(Into::into).collect(),
            }
        }
    }

    impl From<SkillValidationIssue> for PySkillValidationIssue {
        fn from(i: SkillValidationIssue) -> Self {
            Self {
                severity: match i.severity {
                    IssueSeverity::Error => "error".to_string(),
                    IssueSeverity::Warning => "warning".to_string(),
                    IssueSeverity::Info => "info".to_string(),
                },
                category: format!("{:?}", i.category).to_lowercase(),
                message: i.message,
            }
        }
    }

    /// Register Python bindings for the validator module.
    pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_class::<PySkillValidationIssue>()?;
        m.add_class::<PySkillValidationReport>()?;
        m.add_function(wrap_pyfunction!(py_validate_skill, m)?)?;
        Ok(())
    }
}

#[cfg(feature = "python-bindings")]
pub use python::*;

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_skill_dir(tmp: &tempfile::TempDir, name: &str, content: &str) -> PathBuf {
        let dir = tmp.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let mut f = std::fs::File::create(dir.join("SKILL.md")).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        dir
    }

    #[test]
    fn test_missing_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("no-skill");
        std::fs::create_dir(&dir).unwrap();
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        assert_eq!(report.issues[0].category, IssueCategory::SkillMd);
    }

    #[test]
    fn test_missing_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(&tmp, "bad", "no frontmatter here\n");
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        assert!(report.issues[0].message.contains("frontmatter"));
    }

    #[test]
    fn test_missing_required_fields() {
        let tmp = tempfile::tempdir().unwrap();
        // name is not serde(default), so empty frontmatter fails deserialization.
        // Test description missing separately.
        let dir = make_skill_dir(&tmp, "empty", "---\nname: empty\n---\n");
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        let msgs: Vec<_> = report.issues.iter().map(|i| i.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("description")));
    }

    #[test]
    fn test_name_too_long() {
        let tmp = tempfile::tempdir().unwrap();
        let long_name = "a".repeat(65);
        let content = format!("---\nname: {}\ndescription: test\n---\n", long_name);
        let dir = make_skill_dir(&tmp, &long_name, &content);
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        assert!(report.issues.iter().any(|i| i.message.contains("64")));
    }

    #[test]
    fn test_name_not_kebab_case() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(
            &tmp,
            "bad-name",
            "---\nname: BadName\ndescription: test\n---\n",
        );
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("kebab-case"))
        );
    }

    #[test]
    fn test_description_too_long() {
        let tmp = tempfile::tempdir().unwrap();
        let long_desc = "x".repeat(1025);
        let content = format!("---\nname: my-skill\ndescription: {}\n---\n", long_desc);
        let dir = make_skill_dir(&tmp, "my-skill", &content);
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        assert!(report.issues.iter().any(|i| i.message.contains("1024")));
    }

    #[test]
    fn test_valid_skill_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(
            &tmp,
            "my-skill",
            "---\nname: my-skill\ndescription: A valid test skill\n---\n",
        );
        let report = validate_skill_dir(&dir);
        // Should be clean (no errors, no warnings)
        assert!(
            report.is_clean(),
            "expected clean report, got: {:?}",
            report.issues
        );
    }

    #[test]
    fn test_name_dir_mismatch_warns() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(
            &tmp,
            "actual-dir",
            "---\nname: different-name\ndescription: test\n---\n",
        );
        let report = validate_skill_dir(&dir);
        assert!(report.issues.iter().any(|i| {
            i.severity == IssueSeverity::Warning && i.message.contains("directory name")
        }));
    }

    #[test]
    fn test_missing_source_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(
            &tmp,
            "my-skill",
            "---\nname: my-skill\ndescription: test\ntools:\n  - name: do_thing\n    source_file: scripts/do_thing.py\n---\n",
        );
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("source_file"))
        );
    }

    #[test]
    fn test_legacy_fields_info() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(
            &tmp,
            "legacy-skill",
            "---\nname: legacy-skill\ndescription: test\ndcc: maya\ntags: [modeling]\n---\n",
        );
        let report = validate_skill_dir(&dir);
        assert!(
            report
                .issues
                .iter()
                .any(|i| { i.severity == IssueSeverity::Info && i.message.contains("Legacy") })
        );
    }

    #[test]
    fn test_duplicate_tool_names() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(
            &tmp,
            "dup-skill",
            "---\nname: dup-skill\ndescription: test\ntools:\n  - name: do_thing\n  - name: do_thing\n---\n",
        );
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("duplicate"))
        );
    }

    #[test]
    fn test_unknown_group_reference() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(
            &tmp,
            "grp-skill",
            "---\nname: grp-skill\ndescription: test\ntools:\n  - name: do_thing\n    group: nonexistent\n---\n",
        );
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("unknown group"))
        );
    }

    #[test]
    fn test_empty_dependency_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(
            &tmp,
            "dep-skill",
            "---\nname: dep-skill\ndescription: test\ndepends: ['other-skill', ' ']\n---\n",
        );
        let report = validate_skill_dir(&dir);
        assert!(report.has_errors());
        assert!(report.issues.iter().any(|i| i.message.contains("empty")));
    }

    #[test]
    fn test_unsupported_script_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(
            &tmp,
            "ext-skill",
            "---\nname: ext-skill\ndescription: test\ntools:\n  - name: do_thing\n    source_file: scripts/do_thing.txt\n---\n",
        );
        // Create the file so it exists (we want to test extension, not existence)
        let scripts_dir = dir.join("scripts");
        std::fs::create_dir_all(&scripts_dir).unwrap();
        std::fs::File::create(scripts_dir.join("do_thing.txt")).unwrap();
        let report = validate_skill_dir(&dir);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("unsupported extension"))
        );
    }
}
