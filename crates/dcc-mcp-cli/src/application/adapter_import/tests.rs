use super::*;
use std::io::Write;

const PROBE_TEST_WRITER_PID_ENV: &str = "DCC_MCP_TEST_PROBE_WRITER_PID";
const PROBE_TEST_WRITER_READY_ENV: &str = "DCC_MCP_TEST_PROBE_WRITER_READY";
const PROBE_TEST_WRITER_MODE_ENV: &str = "DCC_MCP_TEST_PROBE_WRITER_MODE";
const PROBE_TEST_SEARCH_PATH_ENV: &str = "PYTHONPATH";

/// Write a stand-in interpreter path. Probe output drives the assertions,
/// so the file only has to exist and be spawnable as a script.
fn fake_python(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(file, "# stand-in interpreter for probe tests").unwrap();
    path
}

#[test]
fn module_name_normalises_distribution_names() {
    assert_eq!(module_name("dcc-mcp-maya"), "dcc_mcp_maya");
    assert_eq!(module_name("dcc_mcp_3dsmax"), "dcc_mcp_3dsmax");
    assert_eq!(module_name("dcc-mcp-PowerPoint"), "dcc_mcp_powerpoint");
}

#[test]
fn import_module_keeps_a_dotted_catalog_entry_point() {
    let entry: CatalogEntry = serde_json::from_value(json!({
        "name": "dcc-mcp-fixture",
        "description": "Fixture",
        "dcc": ["fixture"],
        "tags": ["adapter"],
        "version": "1.0.0",
        "install": {
            "type": "pip",
            "pip_package": "dcc-mcp-fixture",
            "entry_point": "dcc_mcp_fixture.cli:create_server",
        },
    }))
    .unwrap();
    assert_eq!(
        import_module(&entry, "dcc-mcp-fixture"),
        "dcc_mcp_fixture.cli"
    );
}

#[test]
fn target_without_interpreter_is_unavailable_not_failure() {
    let target = AdapterImportTarget {
        dcc_type: "maya".to_string(),
        module: "dcc_mcp_maya".to_string(),
        distribution: "dcc-mcp-maya".to_string(),
        python: None,
        python_source: None,
    };
    let probe = probe_target(&target);
    assert_eq!(probe["status"], "unavailable");
    assert_eq!(probe["reason"], "no_interpreter");
    assert!(!probe_status(&probe).is_failure());
    assert!(
        probe["hint"]
            .as_str()
            .unwrap()
            .contains(PYTHON_EXECUTABLE_ENV)
    );
}

#[test]
fn unlaunchable_interpreter_reports_unavailable() {
    let target = AdapterImportTarget {
        dcc_type: "maya".to_string(),
        module: "dcc_mcp_maya".to_string(),
        distribution: "dcc-mcp-maya".to_string(),
        python: Some(PathBuf::from("/__definitely_missing__/python")),
        python_source: Some("cli_override".to_string()),
    };
    let probe = probe_target(&target);
    assert_eq!(probe["status"], "unavailable");
    assert!(
        probe["reason"]
            .as_str()
            .unwrap()
            .starts_with("interpreter_not_launchable")
    );
    assert!(!probe_status(&probe).is_failure());
}

#[test]
fn observed_dcc_types_deduplicates_by_normalised_key() {
    let inventory = json!({
        "instances": [
            {"dcc_type": "maya"},
            {"dcc_type": "Maya"},
            {"dcc_type": "3dsmax"},
            {"dcc_type": ""},
            {"instance_id": "no-dcc-type"},
        ]
    });
    assert_eq!(observed_dcc_types(Some(&inventory)), vec!["maya", "3dsmax"]);
}

#[test]
fn probe_reports_missing_when_dist_and_module_are_absent() {
    let python = std::env::var_os("PYTHON").unwrap_or_else(|| "python3".into());
    let target = AdapterImportTarget {
        dcc_type: "maya".to_string(),
        module: "dcc_mcp_maya_definitely_absent".to_string(),
        distribution: "dcc-mcp-maya-definitely-absent".to_string(),
        python: Some(PathBuf::from(python)),
        python_source: Some("cli_override".to_string()),
    };
    let probe = probe_target(&target);
    if probe["status"] == "unavailable" {
        // No interpreter on this machine; the contract under test is that a
        // missing interpreter is never graded as an adapter failure.
        assert!(!probe_status(&probe).is_failure());
        return;
    }
    assert_eq!(probe["status"], "missing");
    assert_eq!(probe["imported"], false);
    assert_eq!(probe["error_type"], "ModuleNotFoundError");
}

#[test]
fn grade_classifies_dist_present_but_import_failed() {
    let mut report = Map::new();
    report.insert("imported".to_string(), json!(false));
    report.insert("dist_version".to_string(), json!("0.9.2"));
    assert_eq!(grade(&report, None), AdapterImportStatus::Unimportable);

    let mut absent = Map::new();
    absent.insert("imported".to_string(), json!(false));
    assert_eq!(grade(&absent, None), AdapterImportStatus::Missing);

    let mut mismatch = Map::new();
    mismatch.insert("imported".to_string(), json!(true));
    mismatch.insert("dist_version".to_string(), json!("0.9.2"));
    mismatch.insert("module_version".to_string(), json!("0.9.1"));
    assert_eq!(grade(&mismatch, None), AdapterImportStatus::VersionMismatch);

    let mut ok = Map::new();
    ok.insert("imported".to_string(), json!(true));
    ok.insert("dist_version".to_string(), json!("0.9.2"));
    ok.insert("module_version".to_string(), json!("0.9.2"));
    assert_eq!(grade(&ok, None), AdapterImportStatus::Ok);
}

#[test]
fn core_bounds_grade_the_target_version_without_guessing_unknown_versions() {
    for (running, verdict, status) in [
        (
            Some("0.20.48"),
            RuntimeVerdict::Supported,
            AdapterImportStatus::Ok,
        ),
        (
            Some("0.20.0"),
            RuntimeVerdict::CoreOlderThanDeclared,
            AdapterImportStatus::IncompatibleCore,
        ),
        (
            Some("0.21.0"),
            RuntimeVerdict::CoreNewerThanDeclared,
            AdapterImportStatus::IncompatibleCore,
        ),
        (
            None,
            RuntimeVerdict::UnknownCoreVersion,
            AdapterImportStatus::Ok,
        ),
    ] {
        let report = json!({
            "imported": true, "dist_version": "1.0.0", "module_version": "1.0.0",
            "core_requirement": "dcc-mcp-core>=0.20.24,<0.21.0", "core_version": running,
        });
        let report = report.as_object().unwrap();
        let core = core_compatibility(report).unwrap();
        assert_eq!(core.verdict, verdict);
        assert_eq!(grade(report, Some(&core)), status);
    }
    assert!(probe_status(&json!({"status": "incompatible_core"})).is_failure());
    assert!(core_compatibility(json!({"core_version": "0.21.0"}).as_object().unwrap()).is_none());

    let prerelease = json!({
        "imported": true, "dist_version": "1.0.0", "module_version": "1.0.0",
        "core_requirement": "dcc-mcp-core>=0.20.24,<0.21.0", "core_version": "0.20.24rc1",
    });
    let prerelease = prerelease.as_object().unwrap();
    let core = core_compatibility(prerelease).unwrap();
    assert_eq!(core.verdict, RuntimeVerdict::UnknownCoreVersion);
    assert_eq!(grade(prerelease, Some(&core)), AdapterImportStatus::Ok);
    assert_ne!(
        grade(prerelease, Some(&core)),
        AdapterImportStatus::VersionMismatch
    );
}

#[test]
fn interpreter_probe_uses_imported_core_and_actual_distribution_requirement() {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in [
        (
            "fixture_adapter.py",
            "import dcc_mcp_core\n__version__ = '1.0.0'\n",
        ),
        ("dcc_mcp_core.py", "__version__ = '0.21.0'\n"),
    ] {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    for (folder, metadata) in [
        (
            "dcc_mcp_fixture-1.0.0.dist-info",
            "Metadata-Version: 2.1\nName: dcc-mcp-fixture\nVersion: 1.0.0\nRequires-Dist: dcc-mcp-core>=0.20.24,<0.21.0\n\n",
        ),
        (
            "dcc_mcp_core-0.20.48.dist-info",
            "Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: 0.20.48\n\n",
        ),
    ] {
        let folder = dir.path().join(folder);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("METADATA"), metadata).unwrap();
    }
    let python = real_python().expect("Python is required for the host-free probe regression");
    let probe = |script: &str| {
        let output = Command::new(&python)
            .args([
                "-S",
                "-c",
                "import sys;sys.path.insert(0,sys.argv.pop(1));exec(sys.argv.pop(1))",
            ])
            .arg(dir.path())
            .args([script, "fixture_adapter", "dcc-mcp-fixture"])
            .output()
            .expect("Python is required for the host-free probe fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Map<String, Value>>(&output.stdout).unwrap()
    };
    let report = probe(PROBE_SCRIPT);
    assert_eq!(report["core_version"], "0.21.0");
    assert_eq!(report["core_dist_version"], "0.20.48");
    assert_eq!(report["core_version_source"], "imported_module");
    let core = core_compatibility(&report).unwrap();
    assert_eq!(
        grade(&report, Some(&core)),
        AdapterImportStatus::IncompatibleCore
    );
    std::fs::write(dir.path().join("dcc_mcp_fixture-1.0.0.dist-info/METADATA"),
        "Name: dcc-mcp-fixture\nVersion: 1.0.0\nRequires-Dist: dcc-mcp-core>=0.20.24,<0.21.0\nRequires-Dist: dcc-mcp-core>=0.20.40,<0.21.0; python_version >= '3.7'\n\n").unwrap();
    let report = probe(PROBE_SCRIPT);
    assert_eq!(report["core_requirement"], "dcc-mcp-core>=0.20.24,<0.21.0");
    let core = core_compatibility(&report).unwrap();
    assert_eq!(core.verdict, RuntimeVerdict::CoreNewerThanDeclared);
    assert_eq!(
        grade(&report, Some(&core)),
        AdapterImportStatus::IncompatibleCore
    );

    std::fs::write(
        dir.path().join("dcc_mcp_fixture-1.0.0.dist-info/METADATA"),
        concat!(
            "Name: dcc-mcp-fixture\nVersion: 1.0.0\n",
            "Requires-Dist: dcc-mcp-core>=0.20.24,<0.21.0\n",
            "Requires-Dist: dcc-mcp-core>=0.20.40,<0.21.0\n\n",
        ),
    )
    .unwrap();
    let report = probe(PROBE_SCRIPT);
    assert!(report["core_requirement"].is_null());
    assert!(core_compatibility(&report).is_none());
    let metadata_dir = dir.path().join("dcc_mcp_fixture-1.0.0.dist-info");
    let legacy = dir.path().join("dcc_mcp_fixture.egg-info");
    std::fs::rename(&metadata_dir, &legacy).unwrap();
    std::fs::rename(legacy.join("METADATA"), legacy.join("PKG-INFO")).unwrap();
    let metadata =
        "Name: dcc-mcp-fixture\nVersion: 1.0.0\nRequires-Dist: dcc-mcp-core>=0.20.0,<1.0.0\n\n";
    std::fs::write(legacy.join("PKG-INFO"), metadata).unwrap();
    let report = probe(PROBE_SCRIPT);
    assert_eq!(report["dist_version"], "1.0.0");
    assert_eq!(report["core_requirement"], "dcc-mcp-core>=0.20.0,<1.0.0");
    let mut older = report.clone();
    older.insert("core_version".into(), json!("0.19.9"));
    let core = core_compatibility(&older).unwrap();
    assert!(!core.bound.is_ok());
    assert_eq!(
        grade(&older, Some(&core)),
        AdapterImportStatus::IncompatibleCore
    );
    std::fs::remove_file(legacy.join("PKG-INFO")).unwrap();
    std::fs::remove_dir(&legacy).unwrap();
    std::fs::write(&legacy, metadata).unwrap();
    assert_eq!(
        probe(PROBE_SCRIPT)["core_requirement"],
        "dcc-mcp-core>=0.20.0,<1.0.0"
    );
    let normalized = metadata.replace(
        "dcc-mcp-core>=0.20.0,<1.0.0",
        "DCC__mCp..CoRe>=0.20.24,<0.21.0",
    );
    std::fs::write(&legacy, normalized).unwrap();
    let report = probe(PROBE_SCRIPT);
    assert_eq!(
        report["core_requirement"],
        "DCC__mCp..CoRe>=0.20.24,<0.21.0"
    );
    let core = core_compatibility(&report).unwrap();
    assert_eq!(
        grade(&report, Some(&core)),
        AdapterImportStatus::IncompatibleCore
    );
    std::fs::remove_file(&legacy).unwrap();
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(
        legacy.join("PKG-INFO"),
        "Name: dcc-mcp-fixture\nVersion: 1.0.0\n\n",
    )
    .unwrap();
    let fallback = format!(
        "import sys\nsys.modules['importlib.metadata'] = None\nsys.modules['pkg_resources'] = None\n{PROBE_SCRIPT_BODY}"
    );
    for script in [PROBE_SCRIPT, fallback.as_str()] {
        std::fs::write(
            legacy.join("requires.txt"),
            "dcc-mcp-core>=0.20.24,<0.21.0\n",
        )
        .unwrap();
        let report = probe(script);
        let core = core_compatibility(&report).unwrap();
        assert_eq!(
            grade(&report, Some(&core)),
            AdapterImportStatus::IncompatibleCore
        );
        std::fs::write(
            legacy.join("requires.txt"),
            "[feature]\ndcc-mcp-core>=0.20.24,<0.21.0\n",
        )
        .unwrap();
        assert!(core_compatibility(&probe(script)).is_none());
        std::fs::write(
            legacy.join("requires.txt"),
            concat!(
                "dcc-mcp-core>=0.20.24,<0.21.0\n",
                "[:python_version >= '3.7']\n",
                "dcc-mcp-core>=0.20.40,<0.21.0\n",
            ),
        )
        .unwrap();
        let report = probe(script);
        assert_eq!(report["core_requirement"], "dcc-mcp-core>=0.20.24,<0.21.0");
        let core = core_compatibility(&report).unwrap();
        assert_eq!(core.verdict, RuntimeVerdict::CoreNewerThanDeclared);
        assert_eq!(
            grade(&report, Some(&core)),
            AdapterImportStatus::IncompatibleCore
        );
    }
}

/// Locate a real interpreter for tests that need one.
fn real_python() -> Option<PathBuf> {
    for var in ["PYTHON", "PYTHON3"] {
        if let Some(path) = std::env::var_os(var).filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(path));
        }
    }
    for candidate in ["python3", "python"] {
        let Ok(out) = Command::new(candidate)
            .arg("-c")
            .arg("import sys; sys.stdout.write(sys.executable)")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    None
}

#[test]
fn probe_script_runs_on_a_real_interpreter() {
    // Guards the py37-compatible probe script itself against the standard
    // library `json` module, which every supported interpreter can import.
    let Some(python) = real_python() else {
        eprintln!("skipping: no python interpreter available");
        return;
    };
    let report = run_probe_with_env(&python, "json", "json", PROBE_TIMEOUT, Vec::new())
        .expect("a real interpreter must produce a readable probe report");
    assert_eq!(report["imported"], true);
    assert!(report["python_version"].as_str().unwrap().starts_with("3."));
}

/// The probe must read the declared leaf of a dotted entry point, not the
/// parent package that bare `__import__` returns.
#[test]
fn probe_reads_dotted_leaf_version_and_origin() {
    let Some(python) = real_python() else {
        eprintln!("skipping: no python interpreter available");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("dcc_mcp_fixture");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(package.join("__init__.py"), "__version__ = '0.1.0'\n").unwrap();
    std::fs::write(package.join("leaf.py"), "__version__ = '1.0.0'\n").unwrap();
    let metadata = dir.path().join("dcc_mcp_fixture-1.0.0.dist-info");
    std::fs::create_dir(&metadata).unwrap();
    std::fs::write(
        metadata.join("METADATA"),
        "Name: dcc-mcp-fixture\nVersion: 1.0.0\n\n",
    )
    .unwrap();

    let mut command = Command::new(&python);
    command
        .arg("-c")
        .arg(format!(
            "import sys\nsys.path.insert(0, sys.argv.pop(1))\n{PROBE_SCRIPT_BODY}"
        ))
        .arg(dir.path())
        .args(["dcc_mcp_fixture.leaf", "dcc-mcp-fixture"]);
    let output = command.output().expect("probe helper should run");
    assert!(output.status.success());
    let report: Map<String, Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["imported"], true);
    assert_eq!(report["dist_version"], "1.0.0");
    assert_eq!(report["module_version"], "1.0.0");
    assert!(
        report["module_origin"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("dcc_mcp_fixture/leaf.py")
    );
    assert_eq!(grade(&report, None), AdapterImportStatus::Ok);
}

/// The production probe must accept the same fixed (module, distribution)
/// argument contract used by probe_target. This guards the argument-level
/// defect where a path-pop statement consumed the module name.
#[test]
fn probe_target_reads_dotted_leaf_from_a_real_interpreter() {
    let Some(python) = real_python() else {
        eprintln!("skipping: no python interpreter available");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("dcc_mcp_prod_leaf_fixture");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(package.join("__init__.py"), "__version__ = 'parent'\n").unwrap();
    std::fs::write(package.join("leaf.py"), "__version__ = '4.2.0'\n").unwrap();
    let metadata = dir.path().join("dcc_mcp_prod_leaf_fixture-4.2.0.dist-info");
    std::fs::create_dir(&metadata).unwrap();
    std::fs::write(
        metadata.join("METADATA"),
        "Name: dcc-mcp-prod-leaf-fixture\nVersion: 4.2.0\n\n",
    )
    .unwrap();

    let target = AdapterImportTarget {
        dcc_type: "fixture".to_string(),
        module: "dcc_mcp_prod_leaf_fixture.leaf".to_string(),
        distribution: "dcc-mcp-prod-leaf-fixture".to_string(),
        python: Some(python),
        python_source: Some("test_fixture".to_string()),
    };
    let env_overrides = vec![(
        PROBE_TEST_SEARCH_PATH_ENV.to_string(),
        dir.path().to_path_buf(),
    )];
    let probe = probe_target_with_env(&target, env_overrides);

    assert_eq!(probe["status"], "ok");
    assert_eq!(probe["module"], "dcc_mcp_prod_leaf_fixture.leaf");
    assert_eq!(probe["distribution"], "dcc-mcp-prod-leaf-fixture");
    assert_eq!(probe["module_version"], "4.2.0");
    assert_eq!(probe["dist_version"], "4.2.0");
    assert!(
        probe["module_origin"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("dcc_mcp_prod_leaf_fixture/leaf.py")
    );
}

/// Regression guard for a chatty adapter: output larger than the OS pipe
/// buffer must still be collected in full by the production probe.
#[test]
fn probe_reads_a_child_that_floods_stderr() {
    let Some(python) = real_python() else {
        eprintln!("skipping: no python interpreter available");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("dcc_mcp_noisy_fixture");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(
        package.join("__init__.py"),
        format!(
            "import sys\nsys.stderr.write('x' * ({}))\nsys.stderr.flush()\n__version__ = '7.0.0'\n",
            1024 * 1024
        ),
    )
    .unwrap();

    let report = run_probe_with_env(
        &python,
        "dcc_mcp_noisy_fixture",
        "dcc-mcp-noisy-fixture",
        PROBE_TIMEOUT,
        vec![(
            PROBE_TEST_SEARCH_PATH_ENV.to_string(),
            dir.path().to_path_buf(),
        )],
    )
    .expect("noisy adapter output must not deadlock the production probe");
    assert_eq!(report["imported"], true);
    assert_eq!(report["module_version"], "7.0.0");
}

/// A stream that crosses the sampled capture threshold is stopped through the
/// production probe's owned cleanup path before its spool is read.
#[test]
fn probe_rejects_output_above_the_capture_threshold() {
    let Some(python) = real_python() else {
        eprintln!("skipping: no python interpreter available");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("dcc_mcp_flood_fixture");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(
        package.join("__init__.py"),
        format!(
            "import sys\nsys.stderr.write('x' * ({}))\nsys.stderr.flush()\n__version__ = '7.0.0'\n",
            PROBE_OUTPUT_LIMIT as usize + 1024 * 1024
        ),
    )
    .unwrap();

    let error = run_probe_with_env(
        &python,
        "dcc_mcp_flood_fixture",
        "dcc-mcp-flood-fixture",
        PROBE_TIMEOUT,
        vec![(
            PROBE_TEST_SEARCH_PATH_ENV.to_string(),
            dir.path().to_path_buf(),
        )],
    )
    .unwrap_err();
    assert_eq!(error, "probe_output_limit_exceeded");
}

#[test]
fn overrides_opt_in_dcc_types_without_live_instances() {
    let mut overrides = BTreeMap::new();
    overrides.insert("maya".to_string(), PathBuf::from("/opt/maya/bin/mayapy"));
    let request = AdapterImportRequest {
        inventory: None,
        python_overrides: overrides,
        catalog: None,
    };
    let (targets, catalog_error) = resolve_targets(&request);
    assert_eq!(catalog_error, None);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].dcc_type, "maya");
    assert_eq!(targets[0].module, "dcc_mcp_maya");
    assert_eq!(targets[0].python_source.as_deref(), Some("cli_override"));
}

/// An explicit catalog that cannot be loaded must be reported, not silently
/// downgraded to the `dcc-mcp-<dcc_type>` naming convention.
#[test]
fn explicit_catalog_load_failure_is_reported() {
    let mut overrides = BTreeMap::new();
    overrides.insert("maya".to_string(), PathBuf::from("/opt/maya/bin/mayapy"));
    let request = AdapterImportRequest {
        inventory: None,
        python_overrides: overrides,
        catalog: Some(PathBuf::from("/__definitely_missing__/catalog.yml")),
    };
    let (targets, catalog_error) = resolve_targets(&request);
    assert!(catalog_error.is_some(), "load failure should be surfaced");
    assert_eq!(targets.len(), 1);
    // Falls back to the naming convention so the probe still runs.
    assert_eq!(targets[0].module, "dcc_mcp_maya");
}

/// The normal fixture's writer emits the final JSON line after the probe
/// interpreter exits. That proves the implementation waits for a finite
/// inherited writer before reading the spool; killing the parent's tree on
/// direct-child exit would lose this line and fail the test.
fn write_probe_fixture(dir: &Path, mode: &str) {
    let package = dir.join("dcc_mcp_inherited_fixture");
    std::fs::create_dir_all(&package).unwrap();
    let writer_code = match mode {
        "exit" => {
            "import json,sys,time; time.sleep(0.4); report = dict(module='dcc_mcp_inherited_fixture.leaf', imported=True, module_version='writer-final'); print(json.dumps(report))"
        }
        _ => {
            "import sys,time; print('writer-out', flush=True); print('writer-err', file=sys.stderr, flush=True); time.sleep(30)"
        }
    };
    let leaf = format!(
        r#"import os, json, subprocess, sys, time
child_code = {WRITER_CODE:?}
writer = subprocess.Popen(
[sys.executable, "-c", child_code],
stdin=subprocess.DEVNULL,
stdout=None,
stderr=None,
)
with open(os.environ[{PID_ENV:?}], "w") as handle:
handle.write(str(writer.pid))
with open(os.environ[{READY_ENV:?}], "w") as handle:
handle.write("ready")
sys.exit(0)
"#,
        PID_ENV = PROBE_TEST_WRITER_PID_ENV,
        READY_ENV = PROBE_TEST_WRITER_READY_ENV,
        WRITER_CODE = writer_code,
    );
    std::fs::write(package.join("__init__.py"), "__version__ = 'parent'\n").unwrap();
    std::fs::write(package.join("leaf.py"), leaf).unwrap();
    let metadata = dir.join("dcc_mcp_inherited_fixture-1.0.0.dist-info");
    std::fs::create_dir_all(&metadata).unwrap();
    std::fs::write(
        metadata.join("METADATA"),
        "Name: dcc-mcp-inherited-fixture\nVersion: 1.0.0\n\n",
    )
    .unwrap();
}

fn stop_process(pid: u32) {
    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
            panic!(
                "owned fixture cleanup failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE};
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
        if !handle.is_null() {
            unsafe {
                windows_sys::Win32::System::Threading::TerminateProcess(handle as HANDLE, 1);
                windows_sys::Win32::Foundation::CloseHandle(handle);
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    let _ = pid;
}

fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid as i32, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle as HANDLE) };
        true
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

fn wait_for_file(path: &Path, deadline: Instant) -> bool {
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

/// A writer PID alone cannot prove that Windows reaped the child. A caller
/// that only has a PID, or whose handle query fails, must treat exit as
/// unknown and fail the assertion instead of silently passing.
fn assert_writer_exited(pid: u32, context: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        };
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        assert!(
            !handle.is_null(),
            "{context}: writer exit status is unknown"
        );
        let outcome =
            unsafe { WaitForSingleObject(handle as HANDLE, WRITER_EXIT_WAIT.as_millis() as u32) };
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle as HANDLE) };
        assert_eq!(outcome, WAIT_OBJECT_0, "{context}: writer exit unconfirmed");
    }
    #[cfg(not(windows))]
    {
        assert!(!process_is_alive(pid), "{context}: writer is still alive");
    }
}

const WRITER_EXIT_WAIT: Duration = Duration::from_secs(2);

struct ProbeFixtureGuard {
    pid_path: PathBuf,
}

impl Drop for ProbeFixtureGuard {
    fn drop(&mut self) {
        if let Ok(pid) = std::fs::read_to_string(&self.pid_path) {
            if let Ok(pid) = pid.trim().parse::<u32>() {
                stop_process(pid);
            }
        }
        let _ = std::fs::remove_file(&self.pid_path);
    }
}

struct WriterFixture {
    // Fields drop in declaration order, so the guard must run before TempDir.
    _cleanup: ProbeFixtureGuard,
    _dir: tempfile::TempDir,
    python: PathBuf,
    pid_path: PathBuf,
    ready_path: PathBuf,
    dir_path: PathBuf,
}

fn writer_test_setup(mode: &str) -> Option<WriterFixture> {
    let python = real_python()?;
    let dir = tempfile::tempdir().unwrap();
    write_probe_fixture(dir.path(), mode);
    let dir_path = dir.path().to_path_buf();
    let pid_path = dir.path().join("writer.pid");
    let ready_path = dir.path().join("writer.ready");
    let fixture_guard = ProbeFixtureGuard {
        pid_path: pid_path.clone(),
    };
    Some(WriterFixture {
        _cleanup: fixture_guard,
        _dir: dir,
        python,
        pid_path,
        ready_path,
        dir_path,
    })
}

#[test]
fn probe_normal_parent_exit_drains_inherited_writer() {
    let Some(fixture) = writer_test_setup("exit") else {
        eprintln!("skipping: no python interpreter available");
        return;
    };
    let started = Instant::now();
    let WriterFixture {
        python,
        pid_path,
        ready_path,
        dir_path,
        ..
    } = fixture;
    let env_overrides = vec![
        (PROBE_TEST_WRITER_PID_ENV.to_string(), pid_path.clone()),
        (PROBE_TEST_WRITER_READY_ENV.to_string(), ready_path.clone()),
        (
            PROBE_TEST_WRITER_MODE_ENV.to_string(),
            PathBuf::from("exit"),
        ),
        (
            PROBE_TEST_SEARCH_PATH_ENV.to_string(),
            dir_path.to_path_buf(),
        ),
    ];
    let report = run_probe_with_env(
        &python,
        "dcc_mcp_inherited_fixture.leaf",
        "dcc-mcp-inherited-fixture",
        Duration::from_secs(2),
        env_overrides,
    )
    .expect("production probe must finish after its parent exits");
    let elapsed = started.elapsed();

    assert_eq!(report["module"], "dcc_mcp_inherited_fixture.leaf");
    assert_eq!(report["imported"], true);
    assert_eq!(report["module_version"], "writer-final");
    assert!(wait_for_file(
        &ready_path,
        Instant::now() + Duration::from_secs(1)
    ));
    let pid = std::fs::read_to_string(&pid_path)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    assert_writer_exited(pid, "finite writer");
    assert!(elapsed < Duration::from_secs(4), "elapsed: {elapsed:?}");
}

#[test]
fn probe_timeout_terminates_inherited_writer() {
    let Some(fixture) = writer_test_setup("timeout") else {
        eprintln!("skipping: no python interpreter available");
        return;
    };
    let WriterFixture {
        python,
        pid_path,
        ready_path,
        dir_path,
        ..
    } = fixture;
    let started = Instant::now();
    let env_overrides = vec![
        (PROBE_TEST_WRITER_PID_ENV.to_string(), pid_path.clone()),
        (PROBE_TEST_WRITER_READY_ENV.to_string(), ready_path.clone()),
        (
            PROBE_TEST_WRITER_MODE_ENV.to_string(),
            PathBuf::from("timeout"),
        ),
        (
            PROBE_TEST_SEARCH_PATH_ENV.to_string(),
            dir_path.to_path_buf(),
        ),
    ];
    let error = run_probe_with_env(
        &python,
        "dcc_mcp_inherited_fixture.leaf",
        "dcc-mcp-inherited-fixture",
        Duration::from_secs(2),
        env_overrides,
    )
    .unwrap_err();
    let elapsed = started.elapsed();

    assert_eq!(error, "probe_timed_out_after_2s", "error: {error}");
    assert!(wait_for_file(
        &ready_path,
        Instant::now() + Duration::from_secs(1)
    ));
    let pid = std::fs::read_to_string(&pid_path)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    assert_writer_exited(pid, "timed-out writer");
    assert!(elapsed < Duration::from_secs(4), "elapsed: {elapsed:?}");
}

#[test]
fn summary_counts_failures() {
    let mut overrides = BTreeMap::new();
    let dir = tempfile::tempdir().unwrap();
    let python = fake_python(dir.path(), "fake-python");
    overrides.insert("maya".to_string(), python);
    let request = AdapterImportRequest {
        inventory: None,
        python_overrides: overrides,
        catalog: None,
    };
    let summary = probe_adapters(&request);
    assert_eq!(summary["total"], 1);
    assert_eq!(summary["unavailable"], 1);
    assert_eq!(summary["failures"], 0);
    assert!(!has_failures(&summary));
}
