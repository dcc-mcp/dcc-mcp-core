use std::collections::BTreeSet;
use std::path::PathBuf;

use dcc_mcp_http_server::{ExecutionContractReport, probe_skill_execution_contracts};
use dcc_mcp_skills::validator::IssueSeverity;
use dcc_mcp_skills::{SkillValidationReport, parse_skill_md, validate_skill_dir};
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

fn lint_report_to_json(
    report: &SkillValidationReport,
    execution_contract: &ExecutionContractReport,
) -> Value {
    let (static_errors, warnings) = report.counts();
    let errors = static_errors + execution_contract.issues.len();
    let mut issues: Vec<_> = report
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
    issues.extend(execution_contract.issues.iter().map(|issue| {
        serde_json::json!({
            "severity": "error",
            "category": "ExecutionContract",
            "message": issue.message,
            "tool": issue.tool,
            "declared": issue.declared,
            "observed": issue.observed,
        })
    }));
    serde_json::json!({
        "skill_dir": report.skill_dir.display().to_string(),
        "errors": errors,
        "warnings": warnings,
        "issues": issues,
        "execution_contract": execution_contract,
    })
}

pub(crate) async fn run_lint_cmd(args: &LintArgs) -> anyhow::Result<LintCommandResult> {
    let mut skill_dirs = BTreeSet::new();
    for root in &args.paths {
        collect_skill_dirs(root, &mut skill_dirs, args.max_depth)?;
    }

    let mut reports = Vec::with_capacity(skill_dirs.len());
    let mut execution_reports = Vec::with_capacity(skill_dirs.len());
    for skill_dir in &skill_dirs {
        reports.push(validate_skill_dir(skill_dir));
        let execution_report = match parse_skill_md(skill_dir) {
            Some(metadata) => probe_skill_execution_contracts(&metadata).await,
            None => ExecutionContractReport::default(),
        };
        execution_reports.push(execution_report);
    }
    let (errors, warnings) = reports.iter().zip(&execution_reports).fold(
        (0, 0),
        |(e_acc, w_acc), (report, execution_report)| {
            let (static_errors, warnings) = report.counts();
            (
                e_acc + static_errors + execution_report.issues.len(),
                w_acc + warnings,
            )
        },
    );
    let failed = errors > 0 || (args.warnings_as_errors && warnings > 0);
    let reports_json: Vec<_> = reports
        .iter()
        .zip(&execution_reports)
        .map(|(report, execution_report)| lint_report_to_json(report, execution_report))
        .collect();
    let execution_contracts_checked = execution_reports
        .iter()
        .map(|report| report.checked)
        .sum::<usize>();
    let value = serde_json::json!({
        "checked": reports.len(),
        "execution_contracts_checked": execution_contracts_checked,
        "errors": errors,
        "warnings": warnings,
        "failed": failed,
        "reports": reports_json,
    });

    Ok(LintCommandResult { value, failed })
}
