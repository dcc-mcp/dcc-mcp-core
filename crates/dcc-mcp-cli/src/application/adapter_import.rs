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
use std::fs::File;
use std::io::{self, Read, Write};
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
/// One quarter of the total budget is reserved for writers, termination, and
/// reaping (two seconds at the production default).
const PROBE_CLEANUP_RESERVE_DIVISOR: u32 = 4;
/// Maximum bytes collected from each output spool.
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

    match run_probe(python, &target.module, &target.distribution, PROBE_TIMEOUT) {
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
    let running = report
        .get("core_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    Some(check_runtime(
        requirement,
        running,
        &CoreBoundPolicy::default(),
    ))
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
/// Both pipes are drained from the moment the child starts, and the interpreter
/// owns a separate process group or job. The shared deadline covers the
/// interpreter, inherited writers, and cleanup. Normal completion collects
/// complete output; timeout and unconfirmed cleanup are `unavailable` probes.
fn run_probe(
    python: &Path,
    module: &str,
    distribution: &str,
    timeout: Duration,
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
    // returns. Adoption happens before the child is resumed on Windows.
    let mut tree = ProbeChildTree::adopt(child, Instant::now() + cleanup_window)?;
    let status = loop {
        if tree
            .child
            .try_wait()
            .map_err(|err| format!("probe_failed: {err}"))?
            .is_some()
        {
            break tree
                .child
                .wait()
                .map_err(|err| format!("probe_failed: {err}"))?;
        }
        if Instant::now() >= operation_deadline {
            let reaped = tree.terminate_and_reap(Instant::now() + cleanup_window);
            if !reaped.confirmed {
                return Err("probe_cleanup_unconfirmed".to_string());
            }
            return Err(format!("probe_timed_out_after_{}s", timeout.as_secs()));
        }
        thread::sleep(PROBE_POLL_INTERVAL);
    };

    // A finite writer may legitimately outlive the interpreter and append the
    // final report. Give the owned tree the reserved window to finish before
    // terminating it; an unconfirmed tree state is never treated as clean.
    let cleanup_deadline = started + timeout;
    let reaped = if tree.wait_for_descendants(cleanup_deadline) {
        ProbeCleanupReceipt {
            terminated: true,
            reaped: true,
        }
    } else {
        tree.terminate_and_reap(cleanup_deadline)
    };
    if !reaped.confirmed {
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

/// A probe interpreter and every helper it starts are cleaned up as one tree.
struct ProbeChildTree {
    child: Child,
    #[cfg(unix)]
    process_group: i32,
    #[cfg(windows)]
    job: OwnedHandle,
}

impl ProbeChildTree {
    fn adopt(mut child: Child, deadline: Instant) -> Result<Self, String> {
        #[cfg(unix)]
        {
            let process_group = child.id() as i32;
            return Ok(Self {
                child,
                process_group,
            });
        }
        #[cfg(windows)]
        {
            let job = match assign_probe_job(&child) {
                Ok(job) => job,
                Err(err) => {
                    let _ = terminate_and_reap_suspended(&mut child, deadline);
                    return Err(err);
                }
            };
            if let Err(err) = resume_probe_process(&child) {
                let owned = Self { child, job };
                let _ = owned.terminate_and_reap(deadline);
                return Err(err);
            }
            return Ok(Self { child, job });
        }
        #[cfg(not(any(unix, windows)))]
        Ok(Self { child })
    }

    fn terminate_and_reap(&mut self, deadline: Instant) -> ProbeCleanupReceipt {
        let terminated = self.terminate_descendants();
        let _ = self.child.kill();
        let reaped = loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break true,
                Ok(None) if Instant::now() < deadline => thread::sleep(PROBE_POLL_INTERVAL),
                Ok(None) | Err(_) => break false,
            }
        };
        ProbeCleanupReceipt { terminated, reaped }
    }

    /// Poll the owned process group or job until all descendants exit. A
    /// failed platform query is a cleanup failure, never a clean receipt.
    fn wait_for_descendants(&self, deadline: Instant) -> bool {
        loop {
            match self.descendants_present() {
                Ok(false) => return true,
                Ok(true) if Instant::now() < deadline => {
                    thread::sleep(PROBE_POLL_INTERVAL);
                }
                Ok(true) | Err(_) => return false,
            }
        }
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
            JobObjectBasicProcessIdList, QueryInformationJobObject,
        };
        let mut count: u32 = 0;
        // SAFETY: a zero-size query reports the required list size.
        let queried = unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle() as HANDLE,
                JobObjectBasicProcessIdList,
                std::ptr::null_mut(),
                0,
                &mut count,
            )
        };
        if !queried {
            return Err(std::io::Error::last_os_error());
        }
        Ok(count > 0)
    }

    #[cfg(not(any(unix, windows)))]
    fn descendants_present(&self) -> std::io::Result<bool> {
        let _ = self;
        Ok(false)
    }
}

#[derive(Debug, Clone, Copy)]
struct ProbeCleanupReceipt {
    terminated: bool,
    reaped: bool,
}

impl ProbeCleanupReceipt {
    fn confirmed(self) -> bool {
        self.terminated && self.reaped
    }
}

#[cfg(windows)]
fn terminate_and_reap_suspended(child: &mut Child, deadline: Instant) -> bool {
    let _ = child.kill();
    let reaped = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) if Instant::now() < deadline => thread::sleep(PROBE_POLL_INTERVAL),
            Ok(None) | Err(_) => break false,
        }
    };
    reaped
}

#[cfg(windows)]
fn assign_probe_job(child: &Child) -> Result<OwnedHandle, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
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
    let process = child.as_raw_handle().ok_or_else(|| {
        "interpreter_not_launchable: child exited before job assignment".to_string()
    })?;
    // SAFETY: the standard-library child handle remains live for assignment.
    let assigned =
        configured != 0 && unsafe { AssignProcessToJobObject(job, process as HANDLE) } != 0;
    if !assigned {
        let error = std::io::Error::last_os_error();
        // SAFETY: `job` is live and not yet owned.
        unsafe { CloseHandle(job) };
        return Err(format!(
            "interpreter_not_launchable: job assignment failed: {error}"
        ));
    }
    // SAFETY: CreateJobObjectW returned a valid Win32 handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(job) })
}

#[cfg(windows)]
fn resume_probe_process(child: &Child) -> Result<(), String> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Threading::ResumeThread;
    let handle = child
        .as_raw_handle()
        .ok_or_else(|| "interpreter_not_launchable: child exited before resume".to_string())?;
    // SAFETY: the child is live and suspended until this call.
    let result = unsafe { ResumeThread(handle as HANDLE) };
    if result == u32::MAX {
        return Err(format!(
            "interpreter_not_launchable: resume failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
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

/// Python 3.7-compatible import probe.
///
/// Constraints: no walrus operator, no `importlib.metadata` assumption (3.8+),
/// and no dependency outside the standard library. Distribution metadata is
/// read through `importlib.metadata`, then `pkg_resources`, then a `sys.path`
/// scan of `*.dist-info` directories so a Python 3.7 host still reports the
/// "installed but not importable" case that motivated this check.
const PROBE_SCRIPT_BODY: &str = r#"
from email.parser import Parser
import json
import os
import re
import sys

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
core_module = sys.modules.get("dcc_mcp_core")
observed = getattr(core_module, "__version__", None) if core_module is not None else None
report["core_version"] = str(observed) if observed is not None else version
report["core_version_source"] = "imported_module" if observed is not None else source

print(json.dumps(report))
"#;

const PROBE_SCRIPT: &str = concat!(
    "import sys\n",
    "sys.path.insert(0, sys.argv.pop(1))\n",
    PROBE_SCRIPT_BODY
);

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
        assert!(
            core_compatibility(json!({"core_version": "0.21.0"}).as_object().unwrap()).is_none()
        );

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
        let report = run_probe(&python, "json", "json", PROBE_TIMEOUT)
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
            .arg(PROBE_SCRIPT)
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

        let report = run_probe(
            &python,
            "dcc_mcp_noisy_fixture",
            "dcc-mcp-noisy-fixture",
            PROBE_TIMEOUT,
        )
        .expect("noisy adapter output must not deadlock the production probe");
        assert_eq!(report["imported"], true);
        assert_eq!(report["module_version"], "7.0.0");
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
        let leaf = format!(
            r#"import os, json, subprocess, sys, time
if os.environ[{MODE_ENV:?}] == "exit":
    child_code = "import json,sys,time; time.sleep(0.4); print(json.dumps({'module': 'dcc_mcp_inherited_fixture.leaf', 'imported': True, 'module_version': 'writer-final'}))"
else:
    child_code = "import sys,time; print('writer-out', flush=True); print('writer-err', file=sys.stderr, flush=True); time.sleep(30)"
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
if os.environ[{MODE_ENV:?}] == "exit":
    sys.exit(0)
while True:
    time.sleep(0.2)
"#,
            PID_ENV = PROBE_TEST_WRITER_PID_ENV,
            READY_ENV = PROBE_TEST_WRITER_READY_ENV,
            MODE_ENV = PROBE_TEST_WRITER_MODE_ENV,
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

    struct ProbeFixtureGuard {
        pid_path: PathBuf,
        env_keys: [&'static str; 3],
    }

    impl Drop for ProbeFixtureGuard {
        fn drop(&mut self) {
            if let Ok(pid) = std::fs::read_to_string(&self.pid_path) {
                if let Ok(pid) = pid.trim().parse::<u32>() {
                    stop_process(pid);
                }
            }
            let _ = std::fs::remove_file(&self.pid_path);
            for key in self.env_keys {
                std::env::remove_var(key);
            }
        }
    }

    struct WriterFixture {
        _dir: tempfile::TempDir,
        _cleanup: ProbeFixtureGuard,
        python: PathBuf,
        pid_path: PathBuf,
        ready_path: PathBuf,
    }

    fn writer_test_setup(mode: &str) -> Option<WriterFixture> {
        let python = real_python()?;
        let dir = tempfile::tempdir().unwrap();
        write_probe_fixture(dir.path(), mode);
        let pid_path = dir.path().join("writer.pid");
        let ready_path = dir.path().join("writer.ready");
        let fixture_guard = ProbeFixtureGuard {
            pid_path: pid_path.clone(),
            env_keys: [
                PROBE_TEST_WRITER_PID_ENV,
                PROBE_TEST_WRITER_READY_ENV,
                PROBE_TEST_WRITER_MODE_ENV,
            ],
        };
        Some(WriterFixture {
            _dir: dir,
            _cleanup: fixture_guard,
            python,
            pid_path,
            ready_path,
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
            ..
        } = fixture;
        std::env::set_var(PROBE_TEST_WRITER_PID_ENV, &pid_path);
        std::env::set_var(PROBE_TEST_WRITER_READY_ENV, &ready_path);
        std::env::set_var(PROBE_TEST_WRITER_MODE_ENV, "exit");
        let report = run_probe(
            &python,
            "dcc_mcp_inherited_fixture.leaf",
            "dcc-mcp-inherited-fixture",
            Duration::from_secs(2),
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
        assert!(
            !process_is_alive(pid),
            "finite writer must be fully drained"
        );
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
            ..
        } = fixture;
        std::env::set_var(PROBE_TEST_WRITER_PID_ENV, &pid_path);
        std::env::set_var(PROBE_TEST_WRITER_READY_ENV, &ready_path);
        std::env::set_var(PROBE_TEST_WRITER_MODE_ENV, "timeout");
        let started = Instant::now();
        let error = run_probe(
            &python,
            "dcc_mcp_inherited_fixture.leaf",
            "dcc-mcp-inherited-fixture",
            Duration::from_secs(2),
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
        assert!(!process_is_alive(pid), "writer must be gone after cleanup");
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
}
