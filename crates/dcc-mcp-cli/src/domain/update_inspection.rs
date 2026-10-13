//! Read-only update observations. Release evidence never proves a runtime fix.

use semver::Version;
use serde::Serialize;

pub const CORE_REPOSITORY: &str = "https://github.com/dcc-mcp/dcc-mcp-core";
pub const RELEASE_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize)]
pub struct VersionObservation {
    pub version: Option<String>,
    pub source: &'static str,
    pub reason: Option<&'static str>,
}

impl VersionObservation {
    pub fn known(version: impl Into<String>, source: &'static str) -> Self {
        Self {
            version: Some(version.into()),
            source,
            reason: None,
        }
    }

    pub fn unknown(reason: &'static str) -> Self {
        Self {
            version: None,
            source: "unavailable",
            reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Versions {
    /// The package manager's record, which can differ after a self-update.
    pub installed: VersionObservation,
    /// The executing CLI build, not an arbitrary host or server runtime.
    pub runtime: VersionObservation,
    /// Core compiled into this CLI. This is not a Python distribution observation.
    pub bundled_core: VersionObservation,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceObservation {
    pub provider: &'static str,
    pub url: Option<String>,
    pub checked_at_unix_secs: Option<u64>,
    pub cache_age_secs: Option<u64>,
    pub ttl_secs: u64,
    pub offline: bool,
    pub reason: Option<&'static str>,
    /// Live refresh failure, retained separately from a fallback's limitations.
    pub refresh_error: Option<&'static str>,
}

impl SourceObservation {
    pub fn is_fresh(&self) -> bool {
        self.checked_at_unix_secs.is_some()
            && self.cache_age_secs.is_some_and(|age| age < self.ttl_secs)
            && self.reason.is_none()
            && self.refresh_error.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct ReleaseObservation {
    pub version: String,
    pub url: Option<String>,
    pub published_at: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Constraint {
    pub status: &'static str,
    pub reason: &'static str,
}

impl Constraint {
    pub fn unknown(reason: &'static str) -> Self {
        Self {
            status: "unknown",
            reason,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Constraints {
    pub pins: Constraint,
    pub compatibility: Constraint,
}

impl Default for Constraints {
    fn default() -> Self {
        Self {
            pins: Constraint::unknown("package_manager_and_studio_pins_not_observed"),
            compatibility: Constraint::unknown("release_does_not_declare_runtime_compatibility"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FixEvidence {
    pub status: &'static str,
    pub reason: &'static str,
    pub issue_url: Option<String>,
    pub release_url: Option<String>,
    pub runtime_verification_required: bool,
}

#[derive(Debug, Serialize)]
pub struct UpdateInspection {
    pub schema_version: &'static str,
    pub component: &'static str,
    pub read_only: bool,
    pub dry_run: bool,
    pub channel: String,
    pub supported_channels: Vec<&'static str>,
    pub versions: Versions,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    pub published_at: Option<String>,
    pub version_status: &'static str,
    pub version_reason: &'static str,
    pub source: SourceObservation,
    pub constraints: Constraints,
    pub fix: FixEvidence,
    pub plan: UpdatePlan,
}

#[derive(Debug, Serialize)]
pub struct UpdatePlan {
    pub status: &'static str,
    pub requires_user_confirmation: bool,
    pub upgrade_guidance: &'static str,
    pub rollback_guidance: &'static str,
    pub automatic_actions: Vec<&'static str>,
}

/// Parse only canonical issues in the component's official repository.
pub fn official_issue_number(url: &str) -> Option<u64> {
    let number = url.strip_prefix(&format!("{CORE_REPOSITORY}/issues/"))?;
    let parsed = number.parse::<u64>().ok()?;
    (parsed > 0 && parsed.to_string() == number).then_some(parsed)
}

pub fn inspect(
    versions: Versions,
    channel: &str,
    source: SourceObservation,
    release: Option<ReleaseObservation>,
    constraints: Constraints,
    issue_url: Option<&str>,
) -> UpdateInspection {
    let (version_status, version_reason) = compare_versions(
        versions.runtime.version.as_deref(),
        channel,
        &source,
        release.as_ref(),
    );
    let fix = fix_evidence(issue_url, release.as_ref(), &source, channel);
    let plan_status = if constraints.pins.status == "blocked"
        || constraints.compatibility.status == "incompatible"
    {
        "blocked"
    } else if version_status != "update_available" {
        "no_upgrade_recommended"
    } else if constraints.pins.status != "allows"
        || constraints.compatibility.status != "compatible"
    {
        "review_required"
    } else {
        "approval_required"
    };
    UpdateInspection {
        schema_version: "dcc-mcp.update-inspection.v1",
        component: "dcc-mcp-cli",
        read_only: true,
        dry_run: true,
        channel: channel.to_owned(),
        supported_channels: vec!["stable"],
        versions,
        latest_version: release.as_ref().map(|value| value.version.clone()),
        release_url: release.as_ref().and_then(|value| value.url.clone()),
        published_at: release
            .as_ref()
            .and_then(|value| value.published_at.clone()),
        version_status,
        version_reason,
        source,
        constraints,
        fix,
        plan: UpdatePlan {
            status: plan_status,
            requires_user_confirmation: true,
            upgrade_guidance: "Review the exact release, compatibility, and existing pins. After explicit approval, use the installation's supported update procedure; the existing `dcc-mcp-cli update apply --yes` stages the latest CLI, does not honor an exact version pin, and does not restart a host or server.",
            rollback_guidance: "No automatic rollback is provided. Preserve the current version and use the original installation method to reinstall that exact official release after explicit approval; verify the runtime again. Do not remove or override pins.",
            automatic_actions: Vec::new(),
        },
    }
}

fn parse_version(raw: &str) -> Option<Version> {
    Version::parse(raw.strip_prefix('v').unwrap_or(raw)).ok()
}

fn compare_versions(
    runtime: Option<&str>,
    channel: &str,
    source: &SourceObservation,
    release: Option<&ReleaseObservation>,
) -> (&'static str, &'static str) {
    if channel != "stable" {
        return ("unknown", "unsupported_channel");
    }
    if !source.is_fresh() {
        return ("unknown", "release_source_unavailable_stale_or_unverified");
    }
    let Some(candidate) = release.and_then(|value| parse_version(&value.version)) else {
        return ("unknown", "release_version_unavailable_or_invalid");
    };
    let Some(current) = runtime.and_then(parse_version) else {
        return ("unknown", "runtime_version_unavailable_or_invalid");
    };
    if !candidate.pre.is_empty() || !current.pre.is_empty() {
        return (
            "unknown",
            "prerelease_comparison_not_supported_by_stable_provider",
        );
    }
    match candidate.cmp_precedence(&current) {
        std::cmp::Ordering::Greater => ("update_available", "newer_stable_release_observed"),
        std::cmp::Ordering::Equal => ("up_to_date", "runtime_matches_checked_stable_release"),
        std::cmp::Ordering::Less => (
            "ahead_of_channel",
            "runtime_newer_than_checked_stable_release",
        ),
    }
}

fn fix_evidence(
    issue_url: Option<&str>,
    release: Option<&ReleaseObservation>,
    source: &SourceObservation,
    channel: &str,
) -> FixEvidence {
    let mut evidence = FixEvidence {
        status: "unknown",
        reason: "no_issue_requested",
        issue_url: None,
        release_url: None,
        runtime_verification_required: true,
    };
    let Some(issue_url) = issue_url else {
        return evidence;
    };
    let Some(number) = official_issue_number(issue_url) else {
        evidence.reason = "issue_is_not_a_canonical_official_issue";
        return evidence;
    };
    evidence.issue_url = Some(issue_url.to_owned());
    let Some(release) = release else {
        evidence.reason = "release_unavailable";
        return evidence;
    };
    let expected_url = format!(
        "{CORE_REPOSITORY}/releases/tag/v{}",
        release.version.trim_start_matches('v')
    );
    if channel != "stable"
        || !source.is_fresh()
        || source.provider != "official_github_release"
        || release.url.as_deref() != Some(expected_url.as_str())
    {
        evidence.reason = "fresh_official_release_evidence_unavailable";
        return evidence;
    }
    evidence.release_url = release.url.clone();
    let Some(notes) = release.notes.as_deref() else {
        evidence.reason = "release_notes_unavailable";
        return evidence;
    };
    // This deliberately accepts a small prose subset rather than attempting a
    // partial Markdown parser. A closing statement anywhere in an example,
    // code block, quotation, or comment is not a release fix declaration.
    // Reject the entire note when its context is ambiguous, even if another
    // line looks like a closing statement.
    if ambiguous_fix_context(notes) {
        evidence.reason = "release_note_context_requires_manual_review";
        return evidence;
    }
    // Accept a standalone closing statement only. Keywords elsewhere, issue
    // mentions, PR numbers, and "may fix" language are not fix evidence.
    let references = [format!("#{number}"), issue_url.to_owned()];
    let declares_fix = notes.lines().any(|line| {
        let line = line.trim_end();
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("+ "))
            .unwrap_or(line)
            .trim_end_matches('.');
        let Some((verb, reference)) = line.split_once(' ') else {
            return false;
        };
        matches!(
            verb.to_ascii_lowercase().as_str(),
            "fixes" | "fixed" | "closes" | "closed" | "resolves" | "resolved"
        ) && references
            .iter()
            .any(|candidate| candidate == reference.trim())
    });
    if declares_fix {
        evidence.status = "release_declares_fix";
        evidence.reason = "official_release_has_exact_closing_reference";
    } else {
        evidence.status = "no_matching_evidence";
        evidence.reason = "no_exact_fix_reference_in_checked_release_only";
    }
    evidence
}

fn ambiguous_fix_context(notes: &str) -> bool {
    let lower = notes.to_ascii_lowercase();
    // Raw HTML can contain non-prose content under arbitrary element names.
    // The accepted prose subset therefore excludes all angle-bracket context.
    if notes.contains(['<', '>']) {
        return true;
    }
    if ["`", "~~~", "<!--", "-->", "<pre", "<code", "<blockquote"]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return true;
    }
    if [
        "example",
        "sample",
        "template",
        "syntax",
        "demonstrat",
        "illustrat",
        "hypothet",
    ]
    .iter()
    .any(|context| lower.contains(context))
    {
        return true;
    }
    notes.lines().any(|line| {
        !line.trim().is_empty() && (line.starts_with(char::is_whitespace) || line.starts_with('>'))
    })
}

#[cfg(test)]
#[path = "update_inspection_tests.rs"]
mod tests;
