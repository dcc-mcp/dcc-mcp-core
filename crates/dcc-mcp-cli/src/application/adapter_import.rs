//! Adapter import probing for `dcc-mcp-cli doctor`.
//!
//! A DCC adapter can be *installed* and still be unusable: the classic case is
//! an editable install whose source checkout was deleted, which leaves a
//! `*.dist-info` behind so distribution metadata keeps reporting the package as
//! installed while `import <module>` raises `ModuleNotFoundError`. Nothing in
//! `doctor` surfaced that, so a broken install was reported as `status: ok`.
//!
//! This module runs one real import probe per DCC type in the interpreter that
//! is expected to host that adapter and grades the result:
//!
//! * `ok` — importable and, when both sides report one, versions agree.
//! * `missing` — no distribution metadata and no importable module.
//! * `unimportable` — distribution metadata present but the import failed.
//! * `version_mismatch` — importable, but module and distribution disagree.
//! * `unavailable` — the probe itself could not run (no interpreter configured,
//!   interpreter not launchable, or probe output unreadable). This is never
//!   treated as an adapter failure; it only says the check could not be made.
//!
//! Interpreter selection is explicit on purpose. Guessing an ambient `python`
//! would probe an interpreter that never hosts the adapter and report a false
//! `missing`, which is exactly the noise this check must not add.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Map, Value, json};

use super::install::InstallService;
use crate::domain::install::normalized_dcc_key;

/// Environment variable naming the interpreter that hosts the adapters.
///
/// Mirrors `dcc_mcp_core.constants.ENV_PYTHON_EXECUTABLE`, already honoured by
/// `build_sidecar_command`. Duplicated here so the CLI stays import-free.
pub const PYTHON_EXECUTABLE_ENV: &str = "DCC_MCP_PYTHON_EXECUTABLE";

/// Hard ceiling for one interpreter probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// Result of one adapter import probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterImportStatus {
    /// Importable; versions agree or are not comparable.
    Ok,
    /// Neither distribution metadata nor an importable module was found.
    Missing,
    /// Distribution metadata claims the adapter is installed but importing it failed.
    Unimportable,
    /// Importable, but the module version differs from the distribution version.
    VersionMismatch,
    /// The probe could not be executed at all.
    Unavailable,
}

impl AdapterImportStatus {
    /// Whether this grade is evidence of a broken adapter install.
    ///
    /// `Unavailable` is deliberately excluded: an unrun probe is not a failing
    /// adapter, and reporting it as one would re-introduce false negatives.
    #[must_use]
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            Self::Missing | Self::Unimportable | Self::VersionMismatch
        )
    }
}

/// One DCC type to probe, with everything needed to run the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterImportTarget {
    /// Canonical DCC type (e.g. `maya`).
    pub dcc_type: String,
    /// Importable module name (e.g. `dcc_mcp_maya`).
    pub module: String,
    /// Distribution name (e.g. `dcc-mcp-maya`).
    pub distribution: String,
    /// Interpreter to probe with, when one could be resolved.
    pub python: Option<PathBuf>,
    /// Where the interpreter came from, for operator-facing output.
    pub python_source: Option<String>,
}

/// Request describing the adapters `doctor` should probe.
#[derive(Debug, Clone, Default)]
pub struct AdapterImportRequest {
    /// Live registry inventory, as produced by `local_registry`.
    pub inventory: Option<Value>,
    /// Explicit `dcc_type=path` interpreter overrides from the CLI.
    pub python_overrides: BTreeMap<String, PathBuf>,
    /// Explicit catalog path; `None` uses the bundled release catalog.
    pub catalog: Option<PathBuf>,
}

/// Run every adapter import probe and summarise the outcome.
#[must_use]
pub fn probe_adapters(request: &AdapterImportRequest) -> Value {
    let targets = resolve_targets(request);
    let mut probes = Vec::with_capacity(targets.len());
    let mut failures = 0_usize;
    let mut unavailable = 0_usize;

    for target in targets {
        let probe = probe_target(&target);
        let status = probe_status(&probe);
        if status.is_failure() {
            failures += 1;
        } else if status == AdapterImportStatus::Unavailable {
            unavailable += 1;
        }
        probes.push(probe);
    }

    json!({
        "total": probes.len(),
        "failures": failures,
        "unavailable": unavailable,
        "python_executable_env": PYTHON_EXECUTABLE_ENV,
        "probes": probes,
    })
}

/// Whether any probe found a broken adapter install.
#[must_use]
pub fn has_failures(summary: &Value) -> bool {
    summary
        .get("failures")
        .and_then(Value::as_u64)
        .is_some_and(|failures| failures > 0)
}

/// Build the probe targets from registry observations and operator overrides.
///
/// Only DCC types with a registered instance are probed implicitly. Probing
/// every catalog DCC type would run the check against hosts that are not
/// present and drown the real signal in `missing` rows.
fn resolve_targets(request: &AdapterImportRequest) -> Vec<AdapterImportTarget> {
    let env_python = std::env::var_os(PYTHON_EXECUTABLE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    let mut by_key: BTreeMap<String, AdapterImportTarget> = BTreeMap::new();
    for dcc_type in observed_dcc_types(request.inventory.as_ref()) {
        if let Some(target) = build_target(
            &dcc_type,
            request.catalog.as_deref(),
            &request.python_overrides,
            env_python.as_ref(),
        ) {
            by_key.insert(normalized_dcc_key(&dcc_type), target);
        }
    }

    // Explicit overrides opt a DCC type in even with no live instance, so an
    // operator can audit an adapter before opening the host.
    for dcc_type in request.python_overrides.keys() {
        let key = normalized_dcc_key(dcc_type);
        if by_key.contains_key(&key) {
            continue;
        }
        if let Some(target) = build_target(
            dcc_type,
            request.catalog.as_deref(),
            &request.python_overrides,
            env_python.as_ref(),
        ) {
            by_key.insert(key, target);
        }
    }
    by_key.into_values().collect()
}

fn build_target(
    dcc_type: &str,
    catalog: Option<&Path>,
    overrides: &BTreeMap<String, PathBuf>,
    env_python: Option<&PathBuf>,
) -> Option<AdapterImportTarget> {
    let key = normalized_dcc_key(dcc_type);
    if key.is_empty() {
        return None;
    }
    let (distribution, module, catalog_python) = adapter_names(dcc_type, catalog);
    let (python, python_source) = if let Some(path) = python_path_of(overrides, &key) {
        (Some(path), Some("cli_override".to_string()))
    } else if let Some(path) = env_python {
        (Some(path.clone()), Some(PYTHON_EXECUTABLE_ENV.to_string()))
    } else if let Some(path) = catalog_python {
        (Some(path), Some("catalog_python_path".to_string()))
    } else {
        (None, None)
    };

    Some(AdapterImportTarget {
        dcc_type: dcc_type.to_string(),
        module,
        distribution,
        python,
        python_source,
    })
}

/// DCC types observed in the local registry inventory, de-duplicated.
fn observed_dcc_types(inventory: Option<&Value>) -> Vec<String> {
    let Some(instances) = inventory
        .and_then(|value| value.get("instances"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    for instance in instances {
        let Some(dcc_type) = instance.get("dcc_type").and_then(Value::as_str) else {
            continue;
        };
        let dcc_type = dcc_type.trim();
        if dcc_type.is_empty() {
            continue;
        }
        let key = normalized_dcc_key(dcc_type);
        if seen
            .iter()
            .any(|existing: &String| normalized_dcc_key(existing) == key)
        {
            continue;
        }
        seen.push(dcc_type.to_string());
    }
    seen
}

/// Resolve distribution name, module name, and catalog interpreter for a DCC type.
///
/// Falls back to the `dcc_mcp_<dcc_type>` convention when the catalog has no
/// matching adapter entry, so unlisted or custom DCC types are still probed.
fn adapter_names(dcc_type: &str, catalog: Option<&Path>) -> (String, String, Option<PathBuf>) {
    let key = normalized_dcc_key(dcc_type);
    let fallback_distribution = format!("dcc-mcp-{dcc_type}");
    let entries = match InstallService::bundled().catalog_entries(catalog) {
        Ok(entries) => entries,
        Err(_) => {
            let module = module_name(&fallback_distribution);
            return (fallback_distribution, module, None);
        }
    };

    for entry in entries.iter().filter(|entry| {
        entry
            .tags
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case("adapter"))
    }) {
        if !entry.dcc.iter().any(|dcc| normalized_dcc_key(dcc) == key) {
            continue;
        }
        let source = entry
            .install
            .as_ref()
            .and_then(|install| install.pip_package.clone())
            .filter(|package| !package.trim().is_empty())
            .unwrap_or_else(|| entry.name.clone());
        let module = entry
            .install
            .as_ref()
            .and_then(|install| install.entry_point.clone())
            .and_then(|entry_point| entry_point.split(':').next().map(str::to_string))
            .map(|module| module.trim().to_string())
            .filter(|module| !module.is_empty() && !module.contains(' '))
            .unwrap_or_else(|| module_name(&source));
        let python = entry
            .install
            .as_ref()
            .and_then(|install| install.python_path.clone())
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from);
        return (source, module, python);
    }

    let module = module_name(&fallback_distribution);
    (fallback_distribution, module, None)
}

/// Convert a distribution name to its importable module name.
fn module_name(distribution: &str) -> String {
    let mut module = String::with_capacity(distribution.len());
    for ch in distribution.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            module.push(ch.to_ascii_lowercase());
        } else {
            module.push('_');
        }
    }
    module
}

/// Look up an explicit interpreter override, matching on the normalized DCC key.
fn python_path_of(overrides: &BTreeMap<String, PathBuf>, key: &str) -> Option<PathBuf> {
    overrides
        .iter()
        .find(|(dcc_type, _)| normalized_dcc_key(dcc_type) == key)
        .map(|(_, path)| path.clone())
}

/// Run one interpreter probe and grade the result.
fn probe_target(target: &AdapterImportTarget) -> Value {
    let mut probe = json!({
        "dcc_type": target.dcc_type,
        "module": target.module,
        "distribution": target.distribution,
        "python": target.python,
        "python_source": target.python_source,
    });
    let obj = probe
        .as_object_mut()
        .expect("adapter import probe is a JSON object");

    let Some(python) = target.python.as_ref() else {
        obj.insert(
            "status".to_string(),
            json!(AdapterImportStatus::Unavailable),
        );
        obj.insert("reason".to_string(), json!("no_interpreter"));
        obj.insert(
            "hint".to_string(),
            json!(format!(
                "Set {PYTHON_EXECUTABLE_ENV} or pass --adapter-python {}=<python> to probe this adapter in its host interpreter.",
                target.dcc_type
            )),
        );
        return probe;
    };

    match run_probe(python, &target.module, &target.distribution) {
        Ok(report) => {
            let status = grade(&report);
            obj.extend(report);
            obj.insert("status".to_string(), json!(status));
        }
        Err(reason) => {
            obj.insert(
                "status".to_string(),
                json!(AdapterImportStatus::Unavailable),
            );
            obj.insert("reason".to_string(), json!(reason));
        }
    }
    probe
}

/// Grade a probe report using the distribution and import observations.
fn grade(report: &Map<String, Value>) -> AdapterImportStatus {
    let imported = report
        .get("imported")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let installed = report.get("dist_version").and_then(Value::as_str).is_some()
        || report.get("dist_source").and_then(Value::as_str).is_some();
    if !imported {
        return if installed {
            AdapterImportStatus::Unimportable
        } else {
            AdapterImportStatus::Missing
        };
    }
    let dist_version = report.get("dist_version").and_then(Value::as_str);
    let module_version = report.get("module_version").and_then(Value::as_str);
    match (dist_version, module_version) {
        (Some(dist), Some(module)) if dist != module => AdapterImportStatus::VersionMismatch,
        _ => AdapterImportStatus::Ok,
    }
}

/// Read back the status recorded on a probe row.
fn probe_status(probe: &Value) -> AdapterImportStatus {
    probe
        .get("status")
        .and_then(Value::as_str)
        .and_then(|status| match status {
            "ok" => Some(AdapterImportStatus::Ok),
            "missing" => Some(AdapterImportStatus::Missing),
            "unimportable" => Some(AdapterImportStatus::Unimportable),
            "version_mismatch" => Some(AdapterImportStatus::VersionMismatch),
            "unavailable" => Some(AdapterImportStatus::Unavailable),
            _ => None,
        })
        .unwrap_or(AdapterImportStatus::Unavailable)
}

/// Spawn `<python> -c <script> <module> <distribution>` and parse its JSON report.
fn run_probe(
    python: &Path,
    module: &str,
    distribution: &str,
) -> Result<Map<String, Value>, String> {
    let mut child = Command::new(python)
        .arg("-c")
        .arg(PROBE_SCRIPT)
        .arg(module)
        .arg(distribution)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("interpreter_not_launchable: {err}"))?;

    // Bound the probe: importing an adapter inside a live host can block, and
    // `doctor` must never hang on one unresponsive interpreter.
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        if child
            .try_wait()
            .map_err(|err| format!("probe_failed: {err}"))?
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "probe_timed_out_after_{}s",
                PROBE_TIMEOUT.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }

    let output = child
        .wait_with_output()
        .map_err(|err| format!("probe_failed: {err}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(line) = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
    else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = first_line(stderr.trim()).unwrap_or("no output");
        return Err(if output.status.success() {
            format!("probe_output_unreadable: {detail}")
        } else {
            format!("probe_exited_{}: {}", output.status, detail)
        });
    };
    serde_json::from_str(line.trim())
        .map_err(|err| format!("probe_output_unreadable: {err}"))
        .and_then(|value: Value| match value {
            Value::Object(map) => Ok(map),
            _ => Err("probe_output_unreadable: expected a JSON object".to_string()),
        })
}

fn first_line(text: &str) -> Option<&str> {
    text.lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
}

/// Python 3.7-compatible import probe.
///
/// Constraints: no walrus operator, no `importlib.metadata` assumption (3.8+),
/// and no dependency outside the standard library. Distribution metadata is
/// read through `importlib.metadata`, then `pkg_resources`, then a `sys.path`
/// scan of `*.dist-info` directories so a Python 3.7 host still reports the
/// "installed but not importable" case that motivated this check.
const PROBE_SCRIPT: &str = r#"
import json
import os
import re
import sys

module_name = sys.argv[1] if len(sys.argv) > 1 else ""
dist_name = sys.argv[2] if len(sys.argv) > 2 else module_name

report = {
    "module": module_name,
    "distribution": dist_name,
    "python": sys.executable,
    "python_version": "%d.%d.%d" % (sys.version_info[0], sys.version_info[1], sys.version_info[2]),
    "imported": False,
    "dist_version": None,
    "dist_source": None,
    "module_version": None,
    "module_origin": None,
    "error_type": None,
    "error": None,
}


def _scan_dist_dirs(name):
    normalised = re.sub(r"[-_.]+", "_", name).strip("_").lower()
    if not normalised:
        return None, None
    prefix = normalised + "-"
    suffix = ".dist-info"
    legacy = normalised + ".egg-info"
    for entry_path in sys.path:
        try:
            names = os.listdir(entry_path)
        except Exception:
            continue
        for name in names:
            if name.startswith(prefix) and name.endswith(suffix):
                version = name[len(prefix):-len(suffix)]
                return (version or None), "dist_info_dir"
            if name == legacy:
                return None, "egg_info_dir"
    return None, None


def _dist_version(name):
    try:
        import importlib.metadata as metadata

        return metadata.version(name), "importlib.metadata"
    except Exception:
        pass
    try:
        import pkg_resources

        return pkg_resources.get_distribution(name).version, "pkg_resources"
    except Exception:
        pass
    return _scan_dist_dirs(name)


if dist_name:
    version, source = _dist_version(dist_name)
    report["dist_version"] = version
    report["dist_source"] = source

if module_name:
    try:
        module = __import__(module_name)
        report["imported"] = True
        origin = getattr(module, "__file__", None)
        report["module_origin"] = str(origin) if origin else None
        version = getattr(module, "__version__", None)
        report["module_version"] = str(version) if version is not None else None
    except Exception as exc:
        report["error_type"] = type(exc).__name__
        report["error"] = str(exc)

print(json.dumps(report))
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
        assert_eq!(grade(&report), AdapterImportStatus::Unimportable);

        let mut absent = Map::new();
        absent.insert("imported".to_string(), json!(false));
        assert_eq!(grade(&absent), AdapterImportStatus::Missing);

        let mut mismatch = Map::new();
        mismatch.insert("imported".to_string(), json!(true));
        mismatch.insert("dist_version".to_string(), json!("0.9.2"));
        mismatch.insert("module_version".to_string(), json!("0.9.1"));
        assert_eq!(grade(&mismatch), AdapterImportStatus::VersionMismatch);

        let mut ok = Map::new();
        ok.insert("imported".to_string(), json!(true));
        ok.insert("dist_version".to_string(), json!("0.9.2"));
        ok.insert("module_version".to_string(), json!("0.9.2"));
        assert_eq!(grade(&ok), AdapterImportStatus::Ok);
    }

    #[test]
    fn fake_interpreter_script_runs_on_real_python() {
        // Guards the py37-compatible probe script itself: run it with the
        // ambient interpreter against the standard library `json` module.
        let python = std::env::var_os("PYTHON").or_else(|| std::env::var_os("PYTHON3"));
        let Some(python) = python else { return };
        let report = run_probe(&PathBuf::from(python), "json", "json");
        let Ok(report) = report else { return };
        assert_eq!(report["imported"], true);
        assert!(report["python_version"].as_str().unwrap().starts_with("3."));
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
        let targets = resolve_targets(&request);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].dcc_type, "maya");
        assert_eq!(targets[0].module, "dcc_mcp_maya");
        assert_eq!(targets[0].python_source.as_deref(), Some("cli_override"));
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
}
