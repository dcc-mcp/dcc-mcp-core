//! Loopback-only bootstrap regressions; downloaded fixture bytes are never run.

use super::*;
use axum::{Json, Router, routing::get};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const VERSION: &str = "0.20.46";
const BYTES: &[u8] = b"fixture gateway, not an executable";

struct Fixture {
    base: String,
    downloads: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(version: &str, sha: Option<&str>, serve_manifest: bool) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let manifest = serde_json::json!({"dcc-mcp-server": {
        "version": version,
        "url": format!("{base}/dcc-mcp-server-windows-x86_64.exe"),
        "sha256": sha,
    }});
    let downloads = Arc::new(AtomicUsize::new(0));
    let count = downloads.clone();
    let mut app = Router::new().route(
        "/dcc-mcp-server-windows-x86_64.exe",
        get(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { BYTES }
        }),
    );
    if serve_manifest {
        let path = format!(
            "/v{VERSION}/dcc-mcp-update-manifest-{}.json",
            dcc_mcp_gateway::release_platform().unwrap()
        );
        app = app.route(
            &path,
            get(move || {
                let manifest = manifest.clone();
                async move { Json(manifest) }
            }),
        );
    }
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Fixture {
        base,
        downloads,
        task,
    }
}

#[tokio::test]
async fn bootstrap_uses_manifest_asset_url_and_verified_download_without_launching() {
    let sha = hex::encode(Sha256::digest(BYTES));
    let fixture = fixture(VERSION, Some(&sha), true).await;
    let cache = tempfile::tempdir().unwrap();
    let path = download_release_binary(VERSION, &fixture.base, cache.path())
        .await
        .unwrap();
    assert_eq!(
        path,
        cache
            .path()
            .join(VERSION)
            .join(GATEWAY_BINARY_NAME_WITH_EXT)
    );
    assert_eq!(std::fs::read(path).unwrap(), BYTES);
    assert_eq!(fixture.downloads.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read_dir(cache.path().join(VERSION))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn bootstrap_rejects_wrong_version_missing_digest_and_checksum_mismatch() {
    let wrong_sha = "0".repeat(64);
    for (version, sha, expected_downloads) in [
        ("0.20.45", Some(wrong_sha.as_str()), 0),
        (VERSION, None, 0),
        (VERSION, Some(wrong_sha.as_str()), 1),
    ] {
        let fixture = fixture(version, sha, true).await;
        let cache = tempfile::tempdir().unwrap();
        assert!(
            download_release_binary(VERSION, &fixture.base, cache.path())
                .await
                .is_err()
        );
        let version_dir = cache.path().join(VERSION);
        assert!(!version_dir.join(GATEWAY_BINARY_NAME_WITH_EXT).exists());
        if version_dir.exists() {
            assert_eq!(std::fs::read_dir(version_dir).unwrap().count(), 0);
        }
        assert_eq!(fixture.downloads.load(Ordering::SeqCst), expected_downloads);
    }
}

#[tokio::test]
async fn bootstrap_404_preserves_the_error_and_explicit_cli_fallback_evidence() {
    let fixture = fixture(VERSION, None, false).await;
    let cache = tempfile::tempdir().unwrap();
    let error = download_release_binary(VERSION, &fixture.base, cache.path())
        .await
        .unwrap_err()
        .context("x".repeat(4096));
    let message = bootstrap_fallback_message(&error, std::path::Path::new("fixture-cli"));
    assert!(message.contains("404"), "{message}");
    assert!(message.contains("resolving gateway bootstrap"));
    assert!(message.contains("current CLI sub-command fallback fixture-cli"));
    assert!(message.chars().count() <= 2048);
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
    assert_eq!(fixture.downloads.load(Ordering::SeqCst), 0);
}
