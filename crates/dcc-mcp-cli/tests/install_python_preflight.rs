//! Host-free coverage for install interpreter selection and fail-closed GUI paths.

use std::path::PathBuf;

use dcc_mcp_catalog::CatalogEntry;
use dcc_mcp_cli::domain::install::{InstallPlanError, InstallPlanner, InstallRequest};
use serde_json::json;

fn plan(
    dcc: &str,
    dcc_path: Option<PathBuf>,
    python: Option<&str>,
    catalog_python: Option<&str>,
) -> Result<serde_json::Value, InstallPlanError> {
    let package = format!("dcc-mcp-{dcc}");
    let wheel = package.replace('-', "_");
    let entry: CatalogEntry = serde_json::from_value(json!({
        "name": package,
        "description": "Host-free interpreter fixture",
        "dcc": [dcc],
        "tags": ["adapter"],
        "version": "0.3.0",
        "install": {
            "type": "pip",
            "pip_package": package,
            "python_path": catalog_python,
            "url": format!("https://files.pythonhosted.org/{wheel}-0.3.0-py3-none-any.whl"),
            "sha256": "a".repeat(64),
        },
    }))
    .unwrap();
    let plan = InstallPlanner::plan(
        &[entry],
        InstallRequest {
            dcc_type: dcc.into(),
            version: None,
            catalog_path: None,
            python: python.map(str::to_owned),
            dcc_path,
            plugin_source: None,
            adobe_debug_root: None,
        },
    )?;
    Ok(serde_json::to_value(plan).unwrap())
}

#[test]
fn records_existing_embedded_python_in_the_executable_plan() {
    for (dcc, gui, embedded) in [
        ("maya", "maya.exe", "mayapy.exe"),
        ("maya", "MAYA.EXE", "Mayapy.EXE"),
        ("maya", "maya", "mayapy"),
        ("maya", "maya.bin", "mayapy"),
        ("maya", "MAYA.BIN", "Mayapy"),
        ("houdini", "Houdini", "hython"),
        ("houdini", "houdinifx.exe", "hython.exe"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let host = dir.path().join(gui);
        let python = dir.path().join(embedded);
        std::fs::write(&host, b"").unwrap();
        std::fs::write(&python, b"").unwrap();
        let result = plan(dcc, Some(host), None, None).unwrap();
        assert_eq!(
            result["steps"][0]["action"]["python"],
            python.to_str().unwrap()
        );
    }
}

#[test]
fn operator_then_catalog_precede_automatic_discovery() {
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("maya.exe");
    std::fs::write(&host, b"").unwrap();
    std::fs::write(dir.path().join("mayapy.exe"), b"").unwrap();
    let explicit = plan(
        "maya",
        Some(host.clone()),
        Some("operator-python"),
        Some("catalog-python"),
    )
    .unwrap();
    assert_eq!(explicit["steps"][0]["action"]["python"], "operator-python");
    let catalog = plan("maya", Some(host), None, Some("catalog-python")).unwrap();
    assert_eq!(catalog["steps"][0]["action"]["python"], "catalog-python");
}

#[test]
fn gui_interpreters_and_missing_embedded_siblings_fail_preflight() {
    for (python, catalog) in [(Some("maya.exe"), None), (None, Some("houdini.exe"))] {
        assert!(matches!(
            plan("maya", None, python, catalog),
            Err(InstallPlanError::InvalidPythonExecutable { .. })
        ));
    }
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("maya.exe");
    std::fs::write(&host, b"").unwrap();
    assert!(matches!(
        plan("maya", Some(host.clone()), None, None),
        Err(InstallPlanError::InvalidPythonExecutable { .. })
    ));
    std::fs::create_dir(dir.path().join("mayapy.exe")).unwrap();
    assert!(matches!(
        plan("maya", Some(host), None, None),
        Err(InstallPlanError::InvalidPythonExecutable { .. })
    ));
}

#[test]
fn non_python_replacements_and_custom_hosts_keep_existing_policy() {
    for (dcc, gui, sibling) in [
        ("unreal", "UnrealEditor.exe", "UnrealEditor-Cmd.exe"),
        ("blender", "blender.exe", "python.exe"),
        ("custom-editor", "maya.exe", "mayapy.exe"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let host = dir.path().join(gui);
        std::fs::write(&host, b"").unwrap();
        std::fs::write(dir.path().join(sibling), b"").unwrap();
        let result = plan(dcc, Some(host), None, None).unwrap();
        assert!(result["steps"][0]["action"].get("python").is_none());
    }
}

#[test]
fn missing_or_directory_selected_embedded_hosts_fail_preflight() {
    for (dcc, gui, embedded) in [
        ("maya", "maya.exe", "mayapy.exe"),
        ("maya", "MAYA.BIN", "mayapy"),
        ("houdini", "houdinifx", "hython"),
        ("houdini", "HOUDINI.EXE", "hython.exe"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let host = dir.path().join(gui);
        std::fs::write(dir.path().join(embedded), b"").unwrap();
        for directory in [false, true] {
            if directory {
                std::fs::create_dir(&host).unwrap();
            }
            assert!(matches!(
                plan(dcc, Some(host.clone()), None, None),
                Err(InstallPlanError::InvalidPythonExecutable { .. })
            ));
            let explicit = plan(dcc, Some(host.clone()), Some("operator-python"), None).unwrap();
            assert_eq!(explicit["steps"][0]["action"]["python"], "operator-python");
            let catalog = plan(dcc, Some(host.clone()), None, Some("catalog-python")).unwrap();
            assert_eq!(catalog["steps"][0]["action"]["python"], "catalog-python");
            let unrelated = plan("custom-editor", Some(host.clone()), None, None).unwrap();
            assert!(unrelated["steps"][0]["action"].get("python").is_none());
        }
    }
}
