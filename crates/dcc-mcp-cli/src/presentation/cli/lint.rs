use std::collections::BTreeSet;
use std::path::PathBuf;

use dcc_mcp_skills::validator::IssueSeverity;
use dcc_mcp_skills::{SkillValidationReport, validate_skill_dir};
use serde_json::Value;

use super::LintArgs;

pub(crate) struct LintCommandResult {
    pub(crate) value: Value,
    pub(crate) failed: bool,
}

pub(crate) fn collect_skill_dirs(
    root: &std::path::Path,
    out: &mut BTreeSet<PathBuf>,
    max_depth: usize,
) -> anyhow::Result<()> {
    collect_skill_dirs_at(root, out, max_depth, 0)
}

fn collect_skill_dirs_at(
    root: &std::path::Path,
    out: &mut BTreeSet<PathBuf>,
    max_depth: usize,
    depth: usize,
) -> anyhow::Result<()> {
    if root.join("SKILL.md").is_file() {
        out.insert(root.to_path_buf());
        return Ok(());
    }

    if !root.is_dir() {
        anyhow::bail!(
            "skill lint path does not exist or is not a directory: {}",
            root.display()
        );
    }
    if depth >= max_depth {
        return Ok(());
    }

    let entries = std::fs::read_dir(root).map_err(|err| {
        anyhow::anyhow!("cannot read skill lint path '{}': {err}", root.display())
    })?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        collect_skill_dirs_at(&path, out, max_depth, depth + 1)?;
    }
    Ok(())
}

fn issue_severity_label(severity: IssueSeverity) -> &'static str {
    match severity {
        IssueSeverity::Error => "error",
        IssueSeverity::Warning => "warning",
    }
}

fn lint_report_to_json(report: &SkillValidationReport) -> Value {
    let (errors, warnings) = report.counts();
    let issues: Vec<_> = report
        .issues
        .iter()
        .map(|issue| {
            serde_json::json!({
                "severity": issue_severity_label(issue.severity),
                "category": format!("{:?}", issue.category),
                "message": issue.message,
            })
        })
        .collect();
    serde_json::json!({
        "skill_dir": report.skill_dir.display().to_string(),
        "errors": errors,
        "warnings": warnings,
        "issues": issues,
    })
}

pub(crate) fn run_lint_cmd(args: &LintArgs) -> anyhow::Result<LintCommandResult> {
    let mut skill_dirs = BTreeSet::new();
    for root in &args.paths {
        collect_skill_dirs(root, &mut skill_dirs, args.max_depth)?;
    }

    let reports: Vec<_> = skill_dirs
        .iter()
        .map(|skill_dir| validate_skill_dir(skill_dir))
        .collect();
    let (errors, warnings) = reports.iter().fold((0, 0), |(e_acc, w_acc), report| {
        let (errors, warnings) = report.counts();
        (e_acc + errors, w_acc + warnings)
    });
    let failed = errors > 0 || (args.warnings_as_errors && warnings > 0);
    let reports_json: Vec<_> = reports.iter().map(lint_report_to_json).collect();
    let value = serde_json::json!({
        "checked": reports.len(),
        "errors": errors,
        "warnings": warnings,
        "failed": failed,
        "reports": reports_json,
    });

    Ok(LintCommandResult { value, failed })
}
