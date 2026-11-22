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
//! * `incompatible_core` — importable, but the target Core is outside the
//!   adapter's declared dependency range.
//! * `unavailable` — the probe itself could not run (no interpreter configured,
//!   interpreter not launchable, or probe output unreadable). This is never
//!   treated as an adapter failure; it only says the check could not be made.
//!
//! Interpreter selection is explicit on purpose. Guessing an ambient `python`
//! would probe an interpreter that never hosts the adapter and report a false
//! `missing`, which is exactly the noise this check must not add.

use std::collections::BTreeMap;
use std::fs::{File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::mem::size_of;
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

use dcc_mcp_catalog::{
    CatalogEntry, CoreBoundPolicy, CoreRuntimeReport, RuntimeVerdict, check_runtime,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::install::InstallService;
use crate::domain::install::normalized_dcc_key;

/// Environment variable naming the interpreter that hosts the adapters.
///
/// Mirrors `dcc_mcp_core.constants.ENV_PYTHON_EXECUTABLE`, already honoured by
/// `build_sidecar_command`. Duplicated here so the CLI stays import-free.
pub const PYTHON_EXECUTABLE_ENV: &str = "DCC_MCP_PYTHON_EXECUTABLE";

/// Total wall-clock ceiling for one probe, including process-tree cleanup.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
/// Half of the total budget is reserved for writers, termination, and
/// reaping (thirty seconds at the production default).
const PROBE_CLEANUP_RESERVE_DIVISOR: u32 = 2;
/// Maximum bytes collected from each output spool after cleanup. Capture
/// stops early when a sampled spool exceeds this threshold; sampling happens
/// every `PROBE_POLL_INTERVAL`, so a fast writer may briefly write farther.
/// This is an early-stop threshold, not a kernel-enforced disk quota.
const PROBE_OUTPUT_LIMIT: u64 = 4 * 1024 * 1024;
/// Poll interval for parent exit and bounded cleanup loops.
const PROBE_POLL_INTERVAL: Duration = Duration::from_millis(25);

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
    /// Importable, but the target Core is outside the adapter's declared range.
    IncompatibleCore,
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
            Self::Missing | Self::Unimportable | Self::VersionMismatch | Self::IncompatibleCore
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
    let (targets, catalog_error) = resolve_targets(request);
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

    let mut summary = json!({
        "total": probes.len(),
        "failures": failures,
        "unavailable": unavailable,
        "python_executable_env": PYTHON_EXECUTABLE_ENV,
        "probes": probes,
    });
    if let Some(error) = catalog_error {
        summary["catalog_error"] = json!(error);
    }
    summary
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
fn resolve_targets(request: &AdapterImportRequest) -> (Vec<AdapterImportTarget>, Option<String>) {
    let env_python = std::env::var_os(PYTHON_EXECUTABLE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    // Load the catalog once and reuse it: parsing it per target would repeat
    // the work N times and hide the load error behind each individual target.
    let (catalog_entries, catalog_error) = load_catalog_entries(request.catalog.as_deref());

    let mut by_key: BTreeMap<String, AdapterImportTarget> = BTreeMap::new();
    for dcc_type in observed_dcc_types(request.inventory.as_ref()) {
        if let Some(target) = build_target(
            &dcc_type,
            catalog_entries.as_deref(),
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
            catalog_entries.as_deref(),
            &request.python_overrides,
            env_python.as_ref(),
        ) {
            by_key.insert(key, target);
        }
    }
    (by_key.into_values().collect(), catalog_error)
}

/// Load adapter catalog entries once, surfacing the error for an explicit path.
///
/// `load_from_file` maps a missing path to an empty entry list (that behaviour
/// is relied on elsewhere), so an operator typo would silently degrade every
/// probe to the `dcc-mcp-<dcc_type>` naming convention. Check the path here so
/// the failure is visible in the summary instead of hiding behind the fallback.
fn load_catalog_entries(catalog: Option<&Path>) -> (Option<Vec<CatalogEntry>>, Option<String>) {
    if let Some(path) = catalog.filter(|path| !path.exists()) {
        return (
            None,
            Some(format!("adapter catalog not found: {}", path.display())),
        );
    }
    match InstallService::bundled().catalog_entries(catalog) {
        Ok(entries) => (Some(entries), None),
        Err(err) => (None, Some(err.to_string())),
    }
}

fn build_target(
    dcc_type: &str,
    catalog_entries: Option<&[CatalogEntry]>,
    overrides: &BTreeMap<String, PathBuf>,
    env_python: Option<&PathBuf>,
) -> Option<AdapterImportTarget> {
    let key = normalized_dcc_key(dcc_type);
    if key.is_empty() {
        return None;
    }
    let (distribution, module, catalog_python) = adapter_names(dcc_type, catalog_entries);
    // A DCC-specific interpreter outranks the global one. `DCC_MCP_PYTHON_EXECUTABLE`
    // names a single interpreter, so using it for every registered DCC type would
    // probe e.g. Blender inside mayapy and report a healthy adapter as `missing`.
    let (python, python_source) = if let Some(path) = python_path_of(overrides, &key) {
        (Some(path), Some("cli_override".to_string()))
    } else if let Some(path) = catalog_python {
        (Some(path), Some("catalog_python_path".to_string()))
    } else if let Some(path) = env_python {
        (Some(path.clone()), Some(PYTHON_EXECUTABLE_ENV.to_string()))
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
fn adapter_names(
    dcc_type: &str,
    catalog_entries: Option<&[CatalogEntry]>,
) -> (String, String, Option<PathBuf>) {
    let key = normalized_dcc_key(dcc_type);
    let fallback_distribution = format!("dcc-mcp-{dcc_type}");
    let Some(entries) = catalog_entries else {
        let module = module_name(&fallback_distribution);
        return (fallback_distribution, module, None);
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
        let module = import_module(entry, &source);
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

/// Resolve the declared import module without invoking its entry-point factory.
pub(crate) fn import_module(entry: &CatalogEntry, distribution: &str) -> String {
    entry
        .install
        .as_ref()
        .and_then(|install| install.entry_point.as_deref())
        .and_then(|entry_point| entry_point.split(':').next())
        .map(str::trim)
        .filter(|module| !module.is_empty() && !module.contains(' '))
        .map(str::to_string)
        .unwrap_or_else(|| module_name(distribution))
}

/// Convert a distribution name to its conventional importable module name.
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
pub(crate) fn probe_target(target: &AdapterImportTarget) -> Value {
    probe_target_with_env(target, Vec::new())
}

/// Check a catalog-declared Core requirement against the version reported by
/// the target interpreter probe.
pub(crate) fn core_compatibility_for_requirement(
    report: &Map<String, Value>,
    requirement: &str,
) -> CoreRuntimeReport {
    // Distribution metadata is not evidence that the target interpreter can
    // actually import Core.  Only grade a runtime version observed from the
    // imported module; otherwise the caller must treat this as unverified.
    if report.get("core_version_source").and_then(Value::as_str) != Some("imported_module") {
        return check_runtime(requirement, "", &CoreBoundPolicy::default());
    }
    let running = report
        .get("core_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    check_runtime(requirement, running, &CoreBoundPolicy::default())
}

#[cfg(test)]
mod tests;

/// Shared implementation with command-local environment overrides used by
/// fixture tests. The production probe path keeps its fixed argument contract.
fn probe_target_with_env(
    target: &AdapterImportTarget,
    env_overrides: Vec<(String, PathBuf)>,
) -> Value {
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

    match run_probe_with_env(
        python,
        &target.module,
        &target.distribution,
        PROBE_TIMEOUT,
        env_overrides,
    ) {
        Ok(report) => {
            let core = core_compatibility(&report);
            let status = grade(&report, core.as_ref());
            obj.extend(report);
            obj.insert("status".to_string(), json!(status));
            if let Some(core) = core {
                obj.insert(
                    "core_compatibility".into(),
                    json!({
                        "requirement": core.declaration,
                        "running": core.running,
                        "status": core.verdict.as_str(),
                        "declaration_valid": core.bound.is_ok(),
                    }),
                );
            } else {
                obj.insert(
                    "core_compatibility".into(),
                    json!({
                        "status": "not_checked",
                        "reason": "no_unambiguous_unconditional_core_requirement",
                    }),
                );
            }
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
fn grade(report: &Map<String, Value>, core: Option<&CoreRuntimeReport>) -> AdapterImportStatus {
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
    if let (Some(dist), Some(module)) = (dist_version, module_version)
        && dist != module
    {
        return AdapterImportStatus::VersionMismatch;
    }
    if core.is_some()
        && report.get("core_version_source").and_then(Value::as_str) != Some("imported_module")
    {
        return AdapterImportStatus::IncompatibleCore;
    }
    let conclusive_core_mismatch = core.is_some_and(|core| {
        matches!(
            core.verdict,
            RuntimeVerdict::CoreOlderThanDeclared | RuntimeVerdict::CoreNewerThanDeclared
        )
    });
    if conclusive_core_mismatch {
        AdapterImportStatus::IncompatibleCore
    } else {
        AdapterImportStatus::Ok
    }
}

fn core_compatibility(report: &Map<String, Value>) -> Option<CoreRuntimeReport> {
    let requirement = report.get("core_requirement")?.as_str()?;
    Some(core_compatibility_for_requirement(report, requirement))
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
            "incompatible_core" => Some(AdapterImportStatus::IncompatibleCore),
            "unavailable" => Some(AdapterImportStatus::Unavailable),
            _ => None,
        })
        .unwrap_or(AdapterImportStatus::Unavailable)
}

/// Spawn the configured interpreter with the probe script and parse its JSON report.
///
/// Output is spooled in a private temporary directory and the interpreter owns
/// a separate process group or job. One wall-clock budget covers the
/// interpreter, inherited writers, and cleanup. Normal completion collects
/// complete output; timeout and unconfirmed cleanup are unavailable probes.
/// Production passes no overrides; tests inject only command-scoped values.
fn run_probe_with_env(
    python: &Path,
    module: &str,
    distribution: &str,
    timeout: Duration,
    env_overrides: Vec<(String, PathBuf)>,
) -> Result<Map<String, Value>, String> {
    run_probe_with_spool_check(
        python,
        module,
        distribution,
        timeout,
        env_overrides,
        |path| std::fs::metadata(path),
    )
}

/// Production implementation. The metadata reader is a parameter so the
/// regression test can inject an I/O failure without racing file deletion;
/// production always passes the standard filesystem metadata function.
fn run_probe_with_spool_check(
    python: &Path,
    module: &str,
    distribution: &str,
    timeout: Duration,
    env_overrides: Vec<(String, PathBuf)>,
    spool_metadata: impl Fn(&Path) -> std::io::Result<Metadata>,
) -> Result<Map<String, Value>, String> {
    let cleanup_window = timeout / PROBE_CLEANUP_RESERVE_DIVISOR;
    let started = Instant::now();
    let operation_deadline = started + timeout.saturating_sub(cleanup_window);
    let spool_dir = tempfile::tempdir()
        .map_err(|err| format!("probe_failed: output spool unavailable: {err}"))?;
    let stdout_path = spool_dir.path().join("stdout");
    let stderr_path = spool_dir.path().join("stderr");
    let stdout_child = File::create(&stdout_path)
        .map_err(|err| format!("probe_failed: stdout spool unavailable: {err}"))?;
    let stderr_child = File::create(&stderr_path)
        .map_err(|err| format!("probe_failed: stderr spool unavailable: {err}"))?;
    let mut command = Command::new(python);
    for (key, value) in env_overrides {
        command.env(key, value);
    }
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    {
        const CREATE_SUSPENDED: u32 = 0x0000_0004;
        command.creation_flags(CREATE_SUSPENDED);
    }
    let child = command
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg("-c")
        .arg(PROBE_SCRIPT)
        .arg(module)
        .arg(distribution)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_child))
        .stderr(Stdio::from(stderr_child))
        .spawn()
        .map_err(|err| format!("interpreter_not_launchable: {err}"))?;

    // Every failure after spawn terminates and reaps the child tree before it
    // returns. On Windows the child stays suspended until job adoption and
    // exactly one owned thread handle has been resumed.
    // Adoption and every post-spawn cleanup path share the same absolute
    // deadline. Recomputing it from `Instant::now()` would extend the total
    // probe budget by another cleanup window.
    let cleanup_deadline = started + timeout;
    let mut tree = ProbeChildTree::adopt(child, cleanup_deadline)?;
    let spool_paths = [stdout_path.as_path(), stderr_path.as_path()];
    let spool_overflow = |tree: &mut ProbeChildTree| -> Result<(), String> {
        for path in spool_paths {
            let check = spool_metadata(path).map(|metadata| metadata.len() > PROBE_OUTPUT_LIMIT);
            if check.as_ref().is_ok_and(|exceeded| *exceeded) {
                let receipt = tree.terminate_and_reap(cleanup_deadline);
                return if receipt.confirmed() {
                    Err("probe_output_limit_exceeded".to_string())
                } else {
                    Err("probe_cleanup_unconfirmed".to_string())
                };
            }
            match check {
                Ok(false) => {}
                Ok(true) => unreachable!("handled by the overflow branch"),
                Err(err) => {
                    let receipt = tree.terminate_and_reap(cleanup_deadline);
                    return if receipt.confirmed() {
                        Err(format!("probe_failed: output spool query failed: {err}"))
                    } else {
                        Err("probe_cleanup_unconfirmed".to_string())
                    };
                }
            }
        }
        Ok(())
    };

    let status = loop {
        if Instant::now() >= operation_deadline {
            let reaped = tree.terminate_and_reap(cleanup_deadline);
            if !reaped.confirmed() {
                return Err("probe_cleanup_unconfirmed".to_string());
            }
            return Err(format!("probe_timed_out_after_{}s", timeout.as_secs()));
        }
        match tree.child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(err) => {
                let reaped = tree.terminate_and_reap(cleanup_deadline);
                return if reaped.confirmed() {
                    Err(format!("probe_failed: {err}"))
                } else {
                    Err("probe_cleanup_unconfirmed".to_string())
                };
            }
        }
        if Instant::now() >= operation_deadline {
            continue;
        }
        spool_overflow(&mut tree)?;
        thread::sleep(PROBE_POLL_INTERVAL);
    };

    // A finite writer may legitimately outlive the interpreter and append the
    // final report. Give the owned group or job the remaining budget to finish
    // before terminating it; an unconfirmed state is never treated as clean.
    let reaped = loop {
        spool_overflow(&mut tree)?;
        // An empty job/group is only evidence while it is observed inside the
        // same absolute cleanup budget. A late observation cannot turn an
        // expired probe into a clean result.
        if Instant::now() >= cleanup_deadline {
            let reaped = tree.terminate_and_reap(cleanup_deadline);
            return if reaped.confirmed() {
                Err(format!("probe_timed_out_after_{}s", timeout.as_secs()))
            } else {
                Err("probe_cleanup_unconfirmed".to_string())
            };
        }
        match tree.descendants_present() {
            Ok(false) => {
                break ProbeCleanupReceipt {
                    reaped: true,
                    exited: true,
                };
            }
            Ok(true) => thread::sleep(PROBE_POLL_INTERVAL),
            Err(_) => break tree.terminate_and_reap(cleanup_deadline),
        }
    };
    if !reaped.confirmed() {
        return Err("probe_cleanup_unconfirmed".to_string());
    }
    let stdout_text = read_spool(&stdout_path)?;
    let stderr_text = read_spool(&stderr_path)?;
    let Some(line) = stdout_text
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
    else {
        let detail = first_line(stderr_text.trim()).unwrap_or("no output");
        return Err(if status.success() {
            format!("probe_output_unreadable: {detail}")
        } else {
            format!("probe_exited_{status}: {detail}")
        });
    };
    serde_json::from_str(line.trim())
        .map_err(|err| format!("probe_output_unreadable: {err}"))
        .and_then(|value: Value| match value {
            Value::Object(map) => Ok(map),
            _ => Err("probe_output_unreadable: expected a JSON object".to_string()),
        })
}

/// A probe interpreter and the helpers it starts in its dedicated process
/// group or Windows job. A helper that deliberately leaves that group is not
/// tracked; this is a group/job ownership guarantee, not a full-tree claim.
struct ProbeChildTree {
    child: Child,
    #[cfg(unix)]
    process_group: i32,
    #[cfg(windows)]
    job: OwnedHandle,
}

impl ProbeChildTree {
    #[cfg(unix)]
    fn adopt(child: Child, _deadline: Instant) -> Result<Self, String> {
        let process_group = child.id() as i32;
        Ok(Self {
            child,
            process_group,
        })
    }

    #[cfg(windows)]
    fn adopt(child: Child, deadline: Instant) -> Result<Self, String> {
        let mut child = child;
        let job = match assign_probe_job(&child) {
            Ok(job) => job,
            Err(err) => {
                return if terminate_and_reap_suspended(&mut child, deadline) {
                    Err(err)
                } else {
                    Err("probe_cleanup_unconfirmed".to_string())
                };
            }
        };
        if let Err(err) = resume_probe_process(&child) {
            let mut owned = Self { child, job };
            let reaped = owned.terminate_and_reap(deadline);
            return if reaped.confirmed() {
                Err(err)
            } else {
                Err("probe_cleanup_unconfirmed".to_string())
            };
        }
        Ok(Self { child, job })
    }

    #[cfg(not(any(unix, windows)))]
    fn adopt(child: Child, _deadline: Instant) -> Result<Self, String> {
        Ok(Self { child })
    }

    fn terminate_and_reap(&mut self, deadline: Instant) -> ProbeCleanupReceipt {
        // Ask the owned group or job to terminate before waiting. Windows
        // returns failure when the job is already empty, so the API result is
        // not the receipt; the bounded probes below are the evidence.
        let _ = self.terminate_descendants();
        let _ = self.child.kill();

        let mut reaped = false;
        let mut exited = false;
        loop {
            if reaped && exited {
                break;
            }
            // Always take one observation after termination. The absolute
            // deadline then decides whether the observed state is evidence.
            let expired = Instant::now() >= deadline;
            if !reaped {
                match self.child.try_wait() {
                    Ok(Some(_)) => reaped = true,
                    Ok(None) => {}
                    Err(_) => {
                        break;
                    }
                }
            }
            if !exited {
                match self.descendants_present() {
                    Ok(false) => exited = true,
                    Ok(true) => {}
                    Err(_) => break,
                }
            }
            if expired || (reaped && exited) {
                break;
            }
            thread::sleep(PROBE_POLL_INTERVAL);
        }
        if Instant::now() >= deadline {
            exited = false;
        }
        ProbeCleanupReceipt { reaped, exited }
    }

    fn terminate_descendants(&self) -> bool {
        #[cfg(unix)]
        {
            let result = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
            result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::HANDLE;
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            // SAFETY: `job` is an owned Job Object handle that outlives the call.
            unsafe { TerminateJobObject(self.job.as_raw_handle() as HANDLE, 1) != 0 }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = self;
            true
        }
    }

    #[cfg(unix)]
    fn descendants_present(&self) -> std::io::Result<bool> {
        // SAFETY: signal 0 probes the dedicated process group without sending
        // a signal; the group was created for this child before exec.
        let result = unsafe { libc::kill(-self.process_group, 0) };
        if result == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(false)
        } else {
            Err(error)
        }
    }

    #[cfg(windows)]
    fn descendants_present(&self) -> std::io::Result<bool> {
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
            QueryInformationJobObject,
        };
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the buffer matches the requested accounting information class.
        let result = unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle() as HANDLE,
                JobObjectBasicAccountingInformation,
                &mut accounting as *mut _ as *mut _,
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(accounting.ActiveProcesses > 0)
    }

    #[cfg(not(any(unix, windows)))]
    fn descendants_present(&self) -> std::io::Result<bool> {
        let _ = self;
        Ok(false)
    }
}

#[derive(Debug, Clone, Copy)]
struct ProbeCleanupReceipt {
    reaped: bool,
    exited: bool,
}

impl ProbeCleanupReceipt {
    fn confirmed(self) -> bool {
        self.reaped && self.exited
    }
}

#[cfg(windows)]
fn terminate_and_reap_suspended(child: &mut Child, deadline: Instant) -> bool {
    let _ = child.kill();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) if Instant::now() < deadline => thread::sleep(PROBE_POLL_INTERVAL),
            Ok(None) | Err(_) => break false,
        }
    }
}

#[cfg(windows)]
fn assign_probe_job(child: &Child) -> Result<OwnedHandle, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    // SAFETY: null name and security attributes request an unnamed private job.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(format!(
            "interpreter_not_launchable: job creation failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: pointer and size match the requested job information class.
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        let error = std::io::Error::last_os_error();
        // SAFETY: `job` is live and not yet owned.
        unsafe { CloseHandle(job) };
        return Err(format!(
            "interpreter_not_launchable: job configuration failed: {error}"
        ));
    }
    let process = child.as_raw_handle();
    // SAFETY: the standard-library child handle remains live for assignment.
    let assigned = unsafe { AssignProcessToJobObject(job, process as HANDLE) } != 0;
    if !assigned {
        let error = std::io::Error::last_os_error();
        // SAFETY: `job` is live and not yet owned.
        unsafe { CloseHandle(job) };
        return Err(format!(
            "interpreter_not_launchable: job assignment failed: {error}"
        ));
    }
    // SAFETY: the standard-library child handle remains live for the query.
    let mut in_job = 0;
    // SAFETY: both raw handles are live and `in_job` is a writable BOOL.
    let queried = unsafe { IsProcessInJob(process as HANDLE, job, &mut in_job) };
    if queried == 0 {
        let error = std::io::Error::last_os_error();
        // SAFETY: `job` is live and not yet owned.
        unsafe { CloseHandle(job) };
        return Err(format!(
            "interpreter_not_launchable: job assignment verification failed: {error}"
        ));
    }
    if in_job == 0 {
        // SAFETY: `job` is live and not yet owned.
        unsafe { CloseHandle(job) };
        return Err(
            "interpreter_not_launchable: job assignment verification failed: child not in job"
                .to_string(),
        );
    }
    // SAFETY: CreateJobObjectW returned a valid Win32 handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(job) })
}

#[cfg(windows)]
fn resume_probe_process(child: &Child) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    let child_id = child.id();
    // SAFETY: this system-wide thread snapshot is a fresh, independent handle.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "interpreter_not_launchable: thread snapshot failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: ownership of the unique snapshot handle transfers to `OwnedHandle`.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot as _) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    // SAFETY: the entry is writable and `dwSize` describes its allocated size.
    let mut found = unsafe { Thread32First(snapshot.as_raw_handle() as _, &mut entry) } != 0;
    let mut thread_id = None;
    while found {
        if entry.th32OwnerProcessID == child_id {
            if thread_id.is_some() {
                return Err(
                    "interpreter_not_launchable: multiple suspended probe threads".to_string(),
                );
            }
            thread_id = Some(entry.th32ThreadID);
        }
        // SAFETY: `entry` remains valid for the next snapshot record.
        found = unsafe { Thread32Next(snapshot.as_raw_handle() as _, &mut entry) } != 0;
    }
    let Some(thread_id) = thread_id else {
        return Err("interpreter_not_launchable: probe thread not found".to_string());
    };
    // SAFETY: the thread ID came from the live snapshot of the owned child.
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if thread.is_null() {
        return Err(format!(
            "interpreter_not_launchable: probe thread open failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `thread` is a unique handle opened with suspend/resume rights.
    let previous_count = unsafe { ResumeThread(thread) };
    let result = if previous_count == u32::MAX {
        Err(format!(
            "interpreter_not_launchable: resume failed: {}",
            std::io::Error::last_os_error()
        ))
    } else if previous_count == 0 {
        Err("interpreter_not_launchable: probe thread was not suspended".to_string())
    } else {
        Ok(())
    };
    // SAFETY: the borrowed thread handle is closed exactly once here.
    unsafe { CloseHandle(thread) };
    result
}

fn first_line(text: &str) -> Option<&str> {
    text.lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
}

/// Read a private output spool after the owning process tree is gone.
fn read_spool(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|err| format!("probe_output_unreadable: {err}"))?;
    let mut bytes = Vec::new();
    file.take(PROBE_OUTPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("probe_output_unreadable: {err}"))?;
    if bytes.len() as u64 > PROBE_OUTPUT_LIMIT {
        return Err("probe_output_unreadable: output limit exceeded".to_string());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Python 3.7-compatible import probe. Python 3.8+ uses the optional
/// `importlib.metadata` fast path when available, while Python 3.7 uses the
/// legacy and directory-scan fallbacks.
///
/// Constraints: no walrus operator and no dependency outside the standard
/// library. Distribution metadata is read through the optional
/// `importlib.metadata` path, then `pkg_resources`, then a `sys.path` scan of
/// `*.dist-info` directories so a Python 3.7 host still reports the "installed
/// but not importable" case that motivated this check.
const PROBE_SCRIPT_BODY: &str = r#"
import sys
from email.parser import Parser
import json
import os
import re

module_name = sys.argv[1] if len(sys.argv) > 1 else ""
dist_name = sys.argv[2] if len(sys.argv) > 2 else module_name


def _unconditional_requirement(values):
    """Keep one distinct unconditional requirement; never evaluate markers."""
    unconditional = []
    for value in values:
        if ";" in value:
            continue
        specifier = value.strip()
        if specifier and specifier not in unconditional:
            unconditional.append(specifier)
    return unconditional[0] if len(unconditional) == 1 else None


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
    "core_requirement": None,
    "core_version": None,
    "core_version_source": None,
    "core_dist_version": None,
    "error_type": None,
    "error": None,
}


def _egg_requirements(contents):
    requirements, marker = [], ""
    for line in contents.splitlines():
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        if line.startswith("[") and line.endswith("]"):
            extra, _, condition = line[1:-1].partition(":")
            terms = (["(" + condition + ")"] if condition else [])
            if extra:
                terms.append('extra == "' + extra + '"')
            marker = "; " + " and ".join(terms) if terms else ""
        else:
            # Preserve legacy section markers without evaluating applicability.
            requirements.append(line + (" " if "@" in line else "") + marker)
    return requirements


def _read_metadata(path):
    try:
        with open(path, encoding="utf-8", errors="replace") as handle:
            metadata = Parser().parsestr(handle.read(1024 * 1024))
        requirements = metadata.get_all("Requires-Dist", [])
        if not requirements and os.path.basename(path) == "PKG-INFO":
            try:
                with open(os.path.join(os.path.dirname(path), "requires.txt"), encoding="utf-8", errors="replace") as handle:
                    requirements = _egg_requirements(handle.read(1024 * 1024))
            except OSError:
                pass
        return metadata.get("Version"), requirements
    except OSError:
        return None, []


def _scan_dist_dirs(name):
    normalised = re.sub(r"[-_.]+", "_", name).strip("_").lower()
    if not normalised:
        return None, None, []
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
                version, requirements = _read_metadata(os.path.join(entry_path, name, "METADATA"))
                return (version or None), "dist_info_dir", requirements
            if name == legacy:
                path = os.path.join(entry_path, name)
                if os.path.isdir(path):
                    path = os.path.join(path, "PKG-INFO")
                version, requirements = _read_metadata(path)
                return version, "egg_info_dir", requirements
    return None, None, []


def _dist_version(name):
    try:
        import importlib.metadata as metadata

        return metadata.version(name), "importlib.metadata", metadata.requires(name) or []
    except Exception:
        pass
    try:
        import pkg_resources

        distribution = pkg_resources.get_distribution(name)
        requirements = []
        for metadata_file in ("METADATA", "PKG-INFO"):
            if distribution.has_metadata(metadata_file):
                contents = distribution.get_metadata(metadata_file)[:1024 * 1024]
                requirements = Parser().parsestr(contents).get_all("Requires-Dist", [])
                break
        if not requirements and distribution.has_metadata("requires.txt"):
            requirements = _egg_requirements(distribution.get_metadata("requires.txt")[:1024 * 1024])
        return distribution.version, "pkg_resources", requirements
    except Exception:
        pass
    return _scan_dist_dirs(name)


if dist_name:
    version, source, requirements = _dist_version(dist_name)
    report["dist_version"] = version
    report["dist_source"] = source
    core_requirements = [value for value in requirements
                         if re.match(r"^dcc[-_.]+mcp[-_.]+core(?=\s|[<=>!~(\[]|$)", value, re.I)]
    report["core_requirement"] = _unconditional_requirement(core_requirements)

if module_name:
    try:
        module = __import__(module_name, fromlist=["__name__"])
        report["imported"] = True
        origin = getattr(module, "__file__", None)
        report["module_origin"] = str(origin) if origin else None
        version = getattr(module, "__version__", None)
        report["module_version"] = str(version) if version is not None else None
    except Exception as exc:
        report["error_type"] = type(exc).__name__
        report["error"] = str(exc)

version, source, _ = _dist_version("dcc-mcp-core")
report["core_dist_version"] = version
try:
    # Import Core explicitly instead of trusting distribution metadata.  An
    # adapter that does not import Core itself still needs a real Core probe.
    core_module = __import__("dcc_mcp_core")
    observed = getattr(core_module, "__version__", None)
    report["core_version"] = str(observed) if observed is not None else None
    report["core_version_source"] = "imported_module"
except Exception as exc:
    report["core_version"] = None
    report["core_version_source"] = None
    report["core_error_type"] = type(exc).__name__
    report["core_error"] = str(exc)

print(json.dumps(report))
"#;

const PROBE_SCRIPT: &str = PROBE_SCRIPT_BODY;
