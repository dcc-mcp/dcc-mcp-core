//! Host-free coverage for install interpreter selection and fail-closed GUI paths.

use std::collections::BTreeMap;
use std::path::PathBuf;

use dcc_mcp_catalog::CatalogEntry;
use dcc_mcp_cli::domain::install::python::{INSTALL_PYTHON_ENV, InstallEnv};
use dcc_mcp_cli::domain::install::{InstallPlanError, InstallPlanner, InstallRequest};
use serde_json::json;

fn write_executable(path: impl AsRef<std::path::Path>) {
    let path = path.as_ref();
    std::fs::write(path, b"").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn plan(
    dcc: &str,
    dcc_path: Option<PathBuf>,
    python: Option<&str>,
    catalog_python: Option<&str>,
) -> Result<serde_json::Value, InstallPlanError> {
    plan_with_env(dcc, dcc_path, python, catalog_python, None)
}

fn plan_with_env(
    dcc: &str,
    dcc_path: Option<PathBuf>,
    python: Option<&str>,
    catalog_python: Option<&str>,
    env_python: Option<&str>,
) -> Result<serde_json::Value, InstallPlanError> {
    let env = env_python.map(|value| {
        let mut vars = BTreeMap::new();
        vars.insert(INSTALL_PYTHON_ENV.to_string(), value.to_string());
        InstallEnv::from_vars(vars)
    });
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
    let environment = env.unwrap_or_default();
    let plan = InstallPlanner::plan_with_env(
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
        &environment,
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
        write_executable(&host);
        write_executable(&python);
        let result = plan(dcc, Some(host), None, None).unwrap();
        assert_eq!(
            result["steps"][0]["action"]["python"],
            python.to_str().unwrap()
        );
        assert_eq!(result["python_source"], "discovered_host");
    }
}

#[test]
fn operator_then_catalog_precede_automatic_discovery() {
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("maya.exe");
    write_executable(&host);
    write_executable(dir.path().join("mayapy.exe"));
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
fn normalized_dcc_aliases_still_discover_host_python() {
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("MAYA.EXE");
    write_executable(&host);
    write_executable(dir.path().join("Mayapy.EXE"));

    let result = plan("Maya", Some(host), None, None).unwrap();

    assert_eq!(result["python_source"], "discovered_host");
    assert!(
        result["steps"][0]["action"]["python"]
            .as_str()
            .is_some_and(|path| path.ends_with("Mayapy.EXE"))
    );
}

#[test]
fn blender_discovers_python_from_its_bundled_runtime_layout() {
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("blender.exe");
    let python_dir = dir.path().join("4.3").join("python").join("bin");
    std::fs::create_dir_all(&python_dir).unwrap();
    let python = python_dir.join(if cfg!(windows) {
        "python.exe"
    } else {
        "python3.11"
    });
    write_executable(&host);
    write_executable(&python);

    let result = plan("blender", Some(host), None, None).unwrap();

    assert_eq!(result["python_source"], "discovered_host");
    assert_eq!(
        result["steps"][0]["action"]["python"],
        python.to_str().unwrap()
    );
}

#[test]
fn blender_discovers_python_from_a_macos_app_resources_layout() {
    let dir = tempfile::tempdir().unwrap();
    let contents = dir.path().join("Blender.app").join("Contents");
    let bundle = dir.path().join("Blender.app");
    let host = contents.join("MacOS").join("Blender");
    let python_dir = contents
        .join("Resources")
        .join("4.3")
        .join("python")
        .join("bin");
    std::fs::create_dir_all(host.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&python_dir).unwrap();
    let python = python_dir.join(if cfg!(windows) {
        "python.exe"
    } else {
        "python3.11"
    });
    write_executable(&host);
    write_executable(&python);

    let result = plan("blender", Some(bundle), None, None).unwrap();

    assert_eq!(result["python_source"], "discovered_host");
    assert_eq!(
        result["steps"][0]["action"]["python"],
        python.to_str().unwrap()
    );
}

#[test]
fn multiple_blender_runtimes_fail_closed_without_an_explicit_python() {
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("blender.exe");
    write_executable(&host);
    for version in ["4.2", "4.3"] {
        let python = dir
            .path()
            .join(version)
            .join("python")
            .join("bin")
            .join(if cfg!(windows) {
                "python.exe"
            } else {
                "python3.11"
            });
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        write_executable(python);
    }

    assert!(matches!(
        plan("blender", Some(host), None, None),
        Err(InstallPlanError::AmbiguousHostInterpreters { .. })
    ));
}

#[cfg(windows)]
#[test]
fn blender_ignores_backup_python_executables() {
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("blender.exe");
    let python_dir = dir.path().join("4.3").join("python").join("bin");
    std::fs::create_dir_all(&python_dir).unwrap();
    let python = python_dir.join("python.exe");
    let backup = python_dir.join("python.exe.bak");
    write_executable(&host);
    write_executable(&python);
    write_executable(&backup);

    let result = plan("blender", Some(host), None, None).unwrap();

    assert_eq!(result["python_source"], "discovered_host");
    assert_eq!(
        result["steps"][0]["action"]["python"],
        python.to_str().unwrap()
    );
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
    write_executable(&host);
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
fn unsupported_hosts_keep_plan_only_behavior() {
    for (dcc, gui, sibling) in [
        ("unreal", "UnrealEditor.exe", "UnrealEditor-Cmd.exe"),
        ("custom-editor", "maya.exe", "mayapy.exe"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let host = dir.path().join(gui);
        write_executable(&host);
        write_executable(dir.path().join(sibling));
        let result = plan(dcc, Some(host.clone()), None, None).unwrap();
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
        write_executable(dir.path().join(embedded));
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

#[test]
fn environment_python_precedes_catalog_and_records_source() {
    let plan = plan_with_env(
        "maya",
        None,
        None,
        Some("catalog-python"),
        Some("environment-python"),
    )
    .unwrap();
    assert_eq!(plan["steps"][0]["action"]["python"], "environment-python");
    assert_eq!(
        plan["python_source"], "environment",
        "the selected interpreter source must be recorded in the plan"
    );
}

#[test]
fn missing_maya_and_houdini_hosts_fail_preflight_without_path_fallback() {
    for dcc in ["maya", "houdini"] {
        let error = plan(dcc, None, None, None).unwrap_err();
        assert!(
            matches!(error, InstallPlanError::MissingHostInterpreter(_)),
            "{dcc}: {error:?}"
        );
    }
}

#[test]
fn custom_dcc_without_native_host_keeps_existing_policy() {
    let plan = plan("custom-editor", None, Some("operator-python"), None).unwrap();
    assert_eq!(plan["steps"][0]["action"]["python"], "operator-python");
    assert_eq!(plan["python_source"], "requested");
}
