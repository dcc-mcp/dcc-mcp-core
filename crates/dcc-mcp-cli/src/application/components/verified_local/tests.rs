use super::*;

fn digest(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

#[test]
fn manifest_requires_exact_bytes_and_bounded_regular_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("manifest.json");
    std::fs::write(&path, b"{}").unwrap();
    assert!(
        read_manifest(&path, &digest(b"different"))
            .unwrap_err()
            .to_string()
            .contains("SHA-256")
    );
    std::fs::write(&path, vec![b' '; MAX_MANIFEST_BYTES + 1]).unwrap();
    assert!(
        read_manifest(&path, &digest(b"{}"))
            .unwrap_err()
            .to_string()
            .contains("size limit")
    );
    assert!(read_manifest(root.path(), &digest(b"{}")).is_err());
}

#[test]
fn local_staging_includes_only_declared_hash_verified_resources() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join(component_file_name()), b"runtime").unwrap();
    std::fs::write(source.path().join("unlisted.dll"), b"untrusted sibling").unwrap();
    std::fs::create_dir(source.path().join("assets")).unwrap();
    std::fs::write(source.path().join("assets/theme.json"), b"theme").unwrap();
    let plan = InstallPlan {
        directories: vec!["assets".into()],
        files: vec![
            InstallFile {
                path: component_file_name().into(),
                sha256: digest(b"runtime"),
            },
            InstallFile {
                path: "assets/theme.json".into(),
                sha256: digest(b"theme"),
            },
        ],
    };
    let staged = stage_local_package(source.path(), &plan).unwrap();
    assert!(!staged.path().join("unlisted.dll").exists());
    assert_eq!(
        std::fs::read(staged.path().join("assets/theme.json")).unwrap(),
        b"theme"
    );
    std::fs::write(source.path().join("assets/theme.json"), b"changed").unwrap();
    assert!(stage_local_package(source.path(), &plan).is_err());
    assert_eq!(
        std::fs::read(staged.path().join("assets/theme.json")).unwrap(),
        b"theme"
    );
}

#[cfg(unix)]
#[test]
fn fixed_install_rejects_changed_declared_file_before_executing_metadata() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let install_root = root.path().join("install");
    let source = root.path().join("source");
    std::fs::create_dir(&install_root).unwrap();
    std::fs::create_dir(&source).unwrap();
    let anchor = install_root.join("dcc-mcp-cli");
    std::fs::write(&anchor, b"core").unwrap();
    std::fs::write(install_root.join("dcc-cua"), b"old").unwrap();
    let marker = root.path().join("executed");
    let script = format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display());
    std::fs::write(source.join("dcc-cua"), &script).unwrap();
    std::fs::set_permissions(
        source.join("dcc-cua"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(source.join("theme"), b"changed").unwrap();
    let build_manifest = |version: &str, foreground: &str| InstallManifest {
        schema_version: 1,
        name: "dcc-cua".into(),
        version: version.into(),
        target: current_target().unwrap().into(),
        asset: InstallAsset {
            name: format!("dcc-cua-{version}-{}.tar.gz", current_target().unwrap()),
            url: format!(
                "{OFFICIAL_RELEASES}/download/v{version}/dcc-cua-{version}-{}.tar.gz",
                current_target().unwrap()
            ),
            sha256: "a".repeat(64),
        },
        install: InstallPlan {
            directories: vec![],
            files: vec![
                InstallFile {
                    path: "dcc-cua".into(),
                    sha256: foreground.into(),
                },
                InstallFile {
                    path: "theme".into(),
                    sha256: digest(b"original"),
                },
            ],
        },
    };
    let manifest =
        serde_json::to_vec(&build_manifest("1.1.0", &digest(script.as_bytes()))).unwrap();
    let previous = serde_json::to_vec(&build_manifest("1.0.0", &digest(b"old"))).unwrap();
    let manifest_path = root.path().join("new.json");
    let previous_path = root.path().join("old.json");
    std::fs::write(&manifest_path, &manifest).unwrap();
    std::fs::write(&previous_path, &previous).unwrap();
    let result = install(VerifiedInstallRequest {
        target_core_cli: anchor,
        expected_core_cli_sha256: digest(b"core"),
        manifest: manifest_path,
        manifest_sha256: digest(&manifest),
        previous_manifest: previous_path,
        previous_manifest_sha256: digest(&previous),
        source_root: source,
        version: "1.1.0".into(),
        expected_current_sha256: digest(b"old"),
        expected_new_sha256: digest(script.as_bytes()),
    });
    assert!(result.unwrap_err().to_string().contains("SHA-256"));
    assert!(!marker.exists());
    assert_eq!(std::fs::read(install_root.join("dcc-cua")).unwrap(), b"old");
}
