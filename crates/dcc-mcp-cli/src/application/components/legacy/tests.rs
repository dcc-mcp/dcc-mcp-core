use super::*;

fn manifest(version: &str, target: &str, bytes: &[u8]) -> InstallManifest {
    let asset_name = format!(
        "dcc-cua-{version}-{target}.{}",
        archive_extension(target).unwrap()
    );
    let path = if target.ends_with("windows-msvc") {
        "dcc-cua.exe"
    } else {
        "dcc-cua"
    };
    let temporary = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temporary.path(), bytes).unwrap();
    InstallManifest {
        schema_version: 1,
        name: COMPONENT_NAME.into(),
        version: version.into(),
        target: target.into(),
        asset: InstallAsset {
            url: format!("{OFFICIAL_RELEASES}/download/v{version}/{asset_name}"),
            name: asset_name,
            sha256: "a".repeat(64),
        },
        install: InstallPlan {
            directories: vec![],
            files: vec![InstallFile {
                path: path.into(),
                sha256: dcc_mcp_updater::sha256_file(temporary.path()).unwrap(),
            }],
        },
    }
}

fn target() -> &'static str {
    if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

#[test]
fn version_hint_is_bounded_unambiguous_and_never_executes_the_file() {
    let file = tempfile::NamedTempFile::new().unwrap();
    for (bytes, expected) in [
        (
            b"usage: dcc-cua --help\0dcc-cua 1.9.0\0".as_slice(),
            Some("1.9.0"),
        ),
        (b"dcc-cua 1.9.0", Some("1.9.0")),
        (b"dcc-cua 1.9.0 dcc-cua 1.9.0", Some("1.9.0")),
        (b"no banner", None),
        (b"dcc-cua 1.9.0 dcc-cua 1.8.0", None),
        (b"dcc-cua 1.9.0-rc1", None),
        (b"dcc-cua 1.9.0+build", None),
        (b"dcc-cua 01.9.0", None),
        (b"dcc-cua 1.9.0/../../evil", Some("1.9.0")),
    ] {
        std::fs::write(file.path(), bytes).unwrap();
        assert_eq!(read_version_hint(file.path()).ok().as_deref(), expected);
    }
    let mut split = vec![0; 8190];
    split.extend_from_slice(b"dcc-cua 1.9.0\0");
    std::fs::write(file.path(), split).unwrap();
    assert_eq!(read_version_hint(file.path()).unwrap(), "1.9.0");
    for terminated in [false, true] {
        let mut split_version = vec![0; 8180];
        split_version.extend_from_slice(b"dcc-cua 123.456.789");
        if terminated {
            split_version.push(0);
        }
        std::fs::write(file.path(), split_version).unwrap();
        assert_eq!(read_version_hint(file.path()).unwrap(), "123.456.789");
    }
    let mut oversized = b"dcc-cua 1.9.0\0".to_vec();
    oversized.resize(8140, 0);
    oversized.extend_from_slice(b"dcc-cua ");
    oversized.extend_from_slice(&[b'1'; 70]);
    std::fs::write(file.path(), oversized).unwrap();
    assert!(read_version_hint(file.path()).is_err());
    file.as_file().set_len(MAX_LEGACY_BINARY_BYTES + 1).unwrap();
    assert!(read_version_hint(file.path()).is_err());
}

#[tokio::test]
async fn only_a_fixed_official_manifest_and_matching_foreground_hash_authorize_migration() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"dcc-cua 1.9.0\0official legacy bytes";
    std::fs::write(root.path().join(component_file_name()), bytes).unwrap();
    let old = manifest("1.9.0", target(), bytes);
    let requested = manifest("1.10.0", target(), b"new runtime");
    let expected_hash = old.install.files[0].sha256.clone();
    let digest =
        verified_foreground_digest_with(root.path(), target(), &requested, |url| async move {
            assert_eq!(url, manifest_url(target(), Some("1.9.0")).unwrap());
            Ok(serde_json::to_vec(&old).unwrap())
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(digest, expected_hash);
    assert_eq!(
        std::fs::read(root.path().join(component_file_name())).unwrap(),
        bytes
    );
    assert!(!root.path().join(".dcc-cua-install-receipt.json").exists());
}

#[tokio::test]
async fn tampering_wrong_platform_and_network_failure_do_not_write_any_installation_file() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"dcc-cua 1.9.0\0untrusted bytes";
    let path = root.path().join(component_file_name());
    std::fs::write(&path, bytes).unwrap();
    let requested = manifest("1.10.0", target(), b"new runtime");
    let wrong_hash = manifest("1.9.0", target(), b"dcc-cua 1.9.0\0official bytes");
    let wrong_target = if cfg!(windows) {
        "x86_64-unknown-linux-gnu"
    } else {
        "x86_64-pc-windows-msvc"
    };
    let wrong_platform = manifest("1.9.0", wrong_target, bytes);
    for invalid in [wrong_hash, wrong_platform] {
        assert!(
            verified_foreground_digest_with(root.path(), target(), &requested, |_| async move {
                Ok(serde_json::to_vec(&invalid).unwrap())
            })
            .await
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!root.path().join(".dcc-cua-install-receipt.json").exists());
    }
    assert!(
        verified_foreground_digest_with(root.path(), target(), &requested, |_| async {
            bail!("network unavailable");
        })
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn any_existing_receipt_disables_legacy_proofs_even_when_malformed() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(component_file_name()), b"dcc-cua 1.9.0\0").unwrap();
    std::fs::write(
        root.path().join(".dcc-cua-install-receipt.json"),
        b"invalid",
    )
    .unwrap();
    let requested = manifest("1.10.0", target(), b"new runtime");
    let result = verified_foreground_digest_with(root.path(), target(), &requested, |_| {
        std::future::ready(Err(anyhow::anyhow!("unexpected manifest fetch")))
    })
    .await
    .unwrap();
    assert!(result.is_none());
}
