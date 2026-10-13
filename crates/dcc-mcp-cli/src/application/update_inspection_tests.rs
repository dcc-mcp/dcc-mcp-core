use std::cell::Cell;

use super::*;

struct CountingProvider {
    calls: Cell<usize>,
    fail: bool,
}

impl ReleaseProvider for CountingProvider {
    async fn stable_release(&self) -> Result<ReleaseObservation, &'static str> {
        self.calls.set(self.calls.get() + 1);
        if self.fail {
            return Err("official_release_unreachable");
        }
        Ok(ReleaseObservation {
            version: "0.20.3".into(),
            url: Some(format!("{CORE_REPOSITORY}/releases/tag/v0.20.3")),
            published_at: Some("2026-10-01T12:00:00Z".into()),
            notes: Some("Fixes #42".into()),
        })
    }
}

fn versions() -> Versions {
    Versions {
        installed: VersionObservation::unknown("package_manager_record_missing_or_invalid"),
        runtime: VersionObservation::known("0.20.2", "executing_cli_build"),
        bundled_core: VersionObservation::known("0.20.2", "cli_workspace_build"),
    }
}

fn request(offline: bool) -> InspectRequest {
    InspectRequest {
        offline,
        channel: "stable".into(),
        issue_url: Some(format!("{CORE_REPOSITORY}/issues/42")),
    }
}

#[tokio::test]
async fn offline_never_calls_provider_even_with_cache() {
    let provider = CountingProvider {
        calls: Cell::new(0),
        fail: false,
    };
    let cache = Some(ReadOnlyCliUpdateCache {
        checked_at_unix_secs: 100,
        latest_version: "0.20.3".into(),
    });
    let report = inspect_with_provider(&provider, request(true), versions(), cache, 120).await;
    assert_eq!(provider.calls.get(), 0);
    assert_eq!(report.version_status, "unknown");
    assert_eq!(report.source.cache_age_secs, Some(20));
    assert_eq!(report.source.provider, "gateway_update_cache");
    assert_eq!(report.fix.status, "unknown");
}

#[tokio::test]
async fn unsupported_channel_does_not_call_stable_provider() {
    let provider = CountingProvider {
        calls: Cell::new(0),
        fail: false,
    };
    let mut input = request(false);
    input.channel = "beta".into();
    let report = inspect_with_provider(&provider, input, versions(), None, 120).await;
    assert_eq!(provider.calls.get(), 0);
    assert_eq!(report.version_reason, "unsupported_channel");
}

#[tokio::test]
async fn unavailable_source_has_no_false_current_or_fix_status() {
    let provider = CountingProvider {
        calls: Cell::new(0),
        fail: true,
    };
    let report = inspect_with_provider(&provider, request(false), versions(), None, 120).await;
    assert_eq!(provider.calls.get(), 1);
    assert_eq!(report.version_status, "unknown");
    assert_eq!(report.source.reason, Some("official_release_unreachable"));
    assert_eq!(
        report.source.refresh_error,
        Some("official_release_unreachable")
    );
    assert_eq!(report.fix.status, "unknown");
}

#[tokio::test]
async fn cache_fallback_keeps_refresh_failure_separate_from_provenance() {
    let provider = CountingProvider {
        calls: Cell::new(0),
        fail: true,
    };
    let cache = Some(ReadOnlyCliUpdateCache {
        checked_at_unix_secs: 100,
        latest_version: "0.20.3".into(),
    });
    let report = inspect_with_provider(&provider, request(false), versions(), cache, 120).await;
    assert_eq!(report.source.provider, "gateway_update_cache");
    assert_eq!(
        report.source.reason,
        Some("official_release_provenance_not_recorded")
    );
    assert_eq!(
        report.source.refresh_error,
        Some("official_release_unreachable")
    );
    assert_eq!(report.version_status, "unknown");
}

#[tokio::test]
async fn official_observation_preserves_time_and_conservative_constraints() {
    let provider = CountingProvider {
        calls: Cell::new(0),
        fail: false,
    };
    let report = inspect_with_provider(&provider, request(false), versions(), None, 120).await;
    assert_eq!(provider.calls.get(), 1);
    assert_eq!(report.source.checked_at_unix_secs, Some(120));
    assert_eq!(report.source.cache_age_secs, Some(0));
    assert_eq!(report.version_status, "update_available");
    assert_eq!(report.constraints.pins.status, "unknown");
    assert_eq!(report.constraints.compatibility.status, "unknown");
    assert_eq!(report.fix.status, "release_declares_fix");
}

#[test]
fn official_release_decoder_rejects_foreign_and_prerelease_metadata() {
    let valid = serde_json::json!({ "tag_name": "v0.20.3",
        "html_url": format!("{CORE_REPOSITORY}/releases/tag/v0.20.3"),
        "draft": false, "prerelease": false,
        "published_at": "2026-10-01T12:00:00Z", "body": "Fixes #42" });
    assert_eq!(
        decode_release(&serde_json::to_vec(&valid).unwrap())
            .unwrap()
            .version,
        "0.20.3"
    );
    for (key, value) in [
        (
            "html_url",
            serde_json::json!("https://github.com/other/repo/releases/tag/v0.20.3"),
        ),
        ("draft", serde_json::json!(true)),
        ("prerelease", serde_json::json!(true)),
        ("published_at", serde_json::Value::Null),
        ("tag_name", serde_json::json!("v0.21.0-rc.1")),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert!(
            decode_release(&serde_json::to_vec(&invalid).unwrap()).is_err(),
            "{key}"
        );
    }
}
