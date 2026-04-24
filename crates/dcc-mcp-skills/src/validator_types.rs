use std::path::PathBuf;

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
    pub(crate) fn error(category: IssueCategory, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            category,
            message: message.into(),
        }
    }

    pub(crate) fn warn(category: IssueCategory, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            category,
            message: message.into(),
        }
    }

    pub(crate) fn info(category: IssueCategory, message: impl Into<String>) -> Self {
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
            .any(|issue| issue.severity == IssueSeverity::Error)
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
        for issue in &self.issues {
            match issue.severity {
                IssueSeverity::Error => errors += 1,
                IssueSeverity::Warning => warnings += 1,
                IssueSeverity::Info => infos += 1,
            }
        }
        (errors, warnings, infos)
    }
}
