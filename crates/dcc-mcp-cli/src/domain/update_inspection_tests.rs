use super::*;

fn versions() -> Versions {
    Versions {
        installed: VersionObservation::known("0.20.1", "package_manager_record"),
        runtime: VersionObservation::known("0.20.2", "executing_cli_build"),
        bundled_core: VersionObservation::known("0.20.2", "cli_workspace_build"),
    }
}

fn source() -> SourceObservation {
    SourceObservation {
        provider: "official_github_release",
        url: Some(format!("{CORE_REPOSITORY}/releases/tag/v0.20.3")),
        checked_at_unix_secs: Some(100),
        cache_age_secs: Some(0),
        ttl_secs: RELEASE_TTL_SECS,
        offline: false,
        reason: None,
        refresh_error: None,
    }
}

fn release(notes: &str) -> ReleaseObservation {
    ReleaseObservation {
        version: "0.20.3".to_owned(),
        url: Some(format!("{CORE_REPOSITORY}/releases/tag/v0.20.3")),
        published_at: Some("2026-10-01T12:00:00Z".to_owned()),
        notes: Some(notes.to_owned()),
    }
}

fn report(notes: &str) -> UpdateInspection {
    inspect(
        versions(),
        "stable",
        source(),
        Some(release(notes)),
        Constraints::default(),
        Some(&format!("{CORE_REPOSITORY}/issues/42")),
    )
}

#[test]
fn exact_official_fix_reference_does_not_claim_runtime_fixed() {
    let result = report("Changes\n- Fixes #42.\n");
    assert_eq!(result.version_status, "update_available");
    assert_eq!(result.fix.status, "release_declares_fix");
    assert!(result.fix.runtime_verification_required);
    assert_eq!(result.plan.status, "review_required");
    assert!(result.plan.automatic_actions.is_empty());
}

#[test]
fn vague_issue_mentions_and_similar_numbers_are_not_fix_evidence() {
    for notes in [
        "Fixes #420",
        "May fix #42",
        "Known issue #42",
        "fix: improve stability (#42)",
        "Fixes #42 and might improve something else",
        "Fixes https://github.com/other/repo/issues/42",
    ] {
        assert_eq!(report(notes).fix.status, "no_matching_evidence", "{notes}");
    }
}

#[test]
fn code_comments_quotations_and_examples_never_declare_a_fix() {
    for notes in [
        "New release tooling example:\n```text\nFixes #42\n```\n",
        "~~~text\nFixes #42\n~~~",
        "~~~\nFixes #42",
        "    Fixes #42",
        "\tFixes #42",
        "<!--\nFixes #42\n-->",
        "<!--\nFixes #42",
        "> Fixes #42",
        "<pre>\nFixes #42\n</pre>",
        "<code>\nFixes #42\n</code>",
        "Example closing statement:\n\nFixes #42",
        "## Release template\nFixes #42",
        "## Changes\n- Fixes #42\n\n```text\nother code\n```",
    ] {
        let result = report(notes);
        assert_eq!(result.fix.status, "unknown", "{notes}");
        assert_eq!(
            result.fix.reason,
            "release_note_context_requires_manual_review"
        );
    }
}

#[test]
fn unambiguous_standalone_prose_and_bullets_are_supported() {
    for notes in [
        "Fixes #42",
        "## Changes\n\n* Fixes #42.",
        "+ Resolves https://github.com/dcc-mcp/dcc-mcp-core/issues/42",
    ] {
        assert_eq!(report(notes).fix.status, "release_declares_fix", "{notes}");
    }
}

#[test]
fn missing_notes_and_foreign_release_are_unknown() {
    let mut observed = release("Fixes #42");
    observed.notes = None;
    let result = inspect(
        versions(),
        "stable",
        source(),
        Some(observed.clone()),
        Constraints::default(),
        Some(&format!("{CORE_REPOSITORY}/issues/42")),
    );
    assert_eq!(result.fix.status, "unknown");
    observed.notes = Some("Fixes #42".into());
    observed.url = Some("https://github.com/other/repo/releases/tag/v0.20.3".into());
    let result = inspect(
        versions(),
        "stable",
        source(),
        Some(observed),
        Constraints::default(),
        Some(&format!("{CORE_REPOSITORY}/issues/42")),
    );
    assert_eq!(result.fix.status, "unknown");
}

#[test]
fn explicit_pin_and_incompatibility_block_upgrade_without_changing_them() {
    for constraints in [
        Constraints {
            pins: Constraint {
                status: "blocked",
                reason: "explicit_version_pin",
            },
            compatibility: Constraint {
                status: "compatible",
                reason: "declared_compatible",
            },
        },
        Constraints {
            pins: Constraint {
                status: "allows",
                reason: "policy_allows",
            },
            compatibility: Constraint {
                status: "incompatible",
                reason: "core_requirement_not_met",
            },
        },
    ] {
        let result = inspect(
            versions(),
            "stable",
            source(),
            Some(release("")),
            constraints,
            None,
        );
        assert_eq!(result.version_status, "update_available");
        assert_eq!(result.plan.status, "blocked");
        assert!(result.plan.automatic_actions.is_empty());
    }
}

#[test]
fn offline_unavailable_and_expired_cache_do_not_report_current() {
    for age in [None, Some(RELEASE_TTL_SECS), Some(RELEASE_TTL_SECS + 1)] {
        let mut observation = source();
        observation.offline = true;
        observation.cache_age_secs = age;
        let result = inspect(
            versions(),
            "stable",
            observation,
            Some(release("Fixes #42")),
            Constraints::default(),
            Some(&format!("{CORE_REPOSITORY}/issues/42")),
        );
        assert_eq!(result.version_status, "unknown");
        assert_eq!(result.fix.status, "unknown");
    }
}

#[test]
fn cached_gateway_version_is_not_official_release_evidence() {
    let mut observation = source();
    observation.provider = "gateway_update_cache";
    observation.offline = true;
    observation.reason = Some("official_release_provenance_not_recorded");
    let result = inspect(
        versions(),
        "stable",
        observation,
        Some(release("Fixes #42")),
        Constraints::default(),
        Some(&format!("{CORE_REPOSITORY}/issues/42")),
    );
    assert_eq!(result.version_status, "unknown");
    assert_eq!(result.fix.status, "unknown");
    assert_eq!(result.latest_version.as_deref(), Some("0.20.3"));
}

#[test]
fn unsupported_channels_and_prereleases_are_explicit_unknowns() {
    let result = inspect(
        versions(),
        "nightly",
        source(),
        Some(release("")),
        Constraints::default(),
        None,
    );
    assert_eq!(result.version_reason, "unsupported_channel");
    let mut observed = release("");
    observed.version = "0.21.0-rc.1".into();
    let result = inspect(
        versions(),
        "stable",
        source(),
        Some(observed),
        Constraints::default(),
        None,
    );
    assert_eq!(
        result.version_reason,
        "prerelease_comparison_not_supported_by_stable_provider"
    );
}

#[test]
fn version_observations_remain_distinct_in_json() {
    let mut observed = versions();
    observed.bundled_core = VersionObservation::unknown("not_declared_by_this_build");
    let result = inspect(
        observed,
        "stable",
        source(),
        Some(release("")),
        Constraints::default(),
        None,
    );
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["versions"]["installed"]["version"], "0.20.1");
    assert_eq!(json["versions"]["runtime"]["version"], "0.20.2");
    assert!(json["versions"]["bundled_core"]["version"].is_null());
    assert_eq!(
        json["versions"]["bundled_core"]["reason"],
        "not_declared_by_this_build"
    );
    assert_eq!(json["schema_version"], "dcc-mcp.update-inspection.v1");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["dry_run"], true);
}

#[test]
fn semantic_versions_ignore_build_metadata_and_never_offer_downgrade() {
    let mut observed = release("");
    observed.version = "0.20.2+release.1".into();
    let result = inspect(
        versions(),
        "stable",
        source(),
        Some(observed.clone()),
        Constraints::default(),
        None,
    );
    assert_eq!(result.version_status, "up_to_date");
    observed.version = "0.20.1".into();
    let result = inspect(
        versions(),
        "stable",
        source(),
        Some(observed),
        Constraints::default(),
        None,
    );
    assert_eq!(result.version_status, "ahead_of_channel");
    assert_eq!(result.plan.status, "no_upgrade_recommended");
}

#[test]
fn official_issue_urls_are_canonical_and_repository_scoped() {
    assert_eq!(
        official_issue_number(&format!("{CORE_REPOSITORY}/issues/42")),
        Some(42)
    );
    for suffix in ["0", "042", "42/", "42?token=secret", "42#comment", "../42"] {
        assert!(official_issue_number(&format!("{CORE_REPOSITORY}/issues/{suffix}")).is_none());
    }
    assert!(official_issue_number("https://github.com/other/repo/issues/42").is_none());
}
