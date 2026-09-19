//! Unified job poller registry (issue #2262).
//!
//! Render and flipbook job APIs lied in four independent ways during the
//! evaluation that produced #2262: an untracked job type made `--wait`
//! impossible, progress counters reported cached zeros while frames were
//! already on disk, and status queries minted fresh jobs for unknown ids.
//!
//! This module owns the pieces that belong to Core:
//!
//! 1. **One registry for every async job type.** A tool that starts a job
//!    registers the contract used to poll it, either explicitly (Rust API, or
//!    a `poll` descriptor the adapter already emits in its result envelope)
//!    or through the legacy `next_tools` heuristic. Registration makes "this
//!    job type is not tracked by the poller" a diagnosable state instead of a
//!    silent early return.
//! 2. **Disk-backed counters.** A job that declares where its outputs land can
//!    be reconciled against authoritative state at read time, so a cached
//!    `current: 0` never wins over frames that exist on disk.
//!
//! Status vocabulary and terminality live here too, so Core, the CLI and
//! adapters agree on what "done waiting" means.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::job::JobProgress;

/// Every status a job report may carry.
pub const JOB_STATUSES: &[&str] = &[
    "pending",
    "running",
    "completed",
    "failed",
    "cancelled",
    "interrupted",
];

/// Statuses after which a poller may stop.
pub const TERMINAL_JOB_STATUSES: &[&str] = &["completed", "failed", "cancelled", "interrupted"];

/// `true` when `status` is part of the canonical job vocabulary.
#[must_use]
pub fn is_known_job_status(status: &str) -> bool {
    JOB_STATUSES.contains(&status)
}

/// `true` when `status` is terminal — the poller's stop condition.
#[must_use]
pub fn is_terminal_job_status(status: &str) -> bool {
    TERMINAL_JOB_STATUSES.contains(&status)
}

/// Walk cap when counting outputs, so a deep render tree cannot turn a status
/// poll into an unbounded descent.
const MAX_WALK_DEPTH: usize = 8;

/// Entry cap when counting outputs, so a directory holding a very large
/// number of files cannot turn one status poll into an unbounded scan. The
/// count is a progress hint, so a saturated walk reports the cap and lets the
/// caller carry on instead of blocking the poll.
const MAX_WALK_ENTRIES: u64 = 50_000;

/// Counter provenance recorded on a reconciled progress payload.
pub const COUNTER_SOURCE_REPORTED: &str = "reported";
/// The counter came from files on disk, not from the handler's own bookkeeping.
pub const COUNTER_SOURCE_DISK: &str = "disk";

/// Which side of the wire owns a job's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PollOwner {
    /// The job is a `JobManager` row owned by Core; poll `jobs_get_status`.
    Core,
    /// The job lives inside the DCC adapter; poll its typed status tool.
    Adapter,
}

impl PollOwner {
    /// Wire identifier.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PollOwner::Core => "core",
            PollOwner::Adapter => "adapter",
        }
    }
}

impl fmt::Display for PollOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How to poll one job type to a terminal state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollContract {
    /// Who answers the poll.
    pub owner: PollOwner,
    /// Tool name to call. Core-owned contracts use `jobs_get_status`.
    pub tool: String,
    /// Argument that carries the job id. Defaults to `job_id`.
    #[serde(default = "default_argument_field")]
    pub argument_field: String,
}

fn default_argument_field() -> String {
    "job_id".to_string()
}

impl PollContract {
    /// Contract for a Core-owned `JobManager` job.
    #[must_use]
    pub fn core(tool: impl Into<String>) -> Self {
        Self {
            owner: PollOwner::Core,
            tool: tool.into(),
            argument_field: default_argument_field(),
        }
    }

    /// Contract for an adapter-owned job polled through `tool`.
    #[must_use]
    pub fn adapter(tool: impl Into<String>) -> Self {
        Self {
            owner: PollOwner::Adapter,
            tool: tool.into(),
            argument_field: default_argument_field(),
        }
    }

    /// Override the argument that carries the job id.
    #[must_use]
    pub fn with_argument_field(mut self, field: impl Into<String>) -> Self {
        self.argument_field = field.into();
        self
    }

    /// Parse a `poll` descriptor from a tool-result envelope.
    ///
    /// Accepts the shape adapters already emit:
    ///
    /// ```json
    /// {"owner": "adapter", "tool": "render__get_job", "arguments": {"job_id": "j-1"}}
    /// ```
    ///
    /// Returns `None` when the descriptor is absent, names no tool, or does not
    /// identify a single job with exactly one argument — an unusable contract
    /// is worse than no contract, because `--wait` would call it and get the
    /// wrong job back.
    ///
    /// Extra arguments are rejected rather than tolerated: a contract carries
    /// one argument field, so replaying `{"job_id": "j", "scene": "s"}` would
    /// drop `scene` and poll the tool with a missing required input. Callers
    /// that need more than one argument must register a contract the poller can
    /// replay in full.
    #[must_use]
    pub fn from_value(value: Option<&Value>) -> Option<Self> {
        let poll = value?.as_object()?;
        let owner = match poll.get("owner").and_then(Value::as_str) {
            Some("core") => PollOwner::Core,
            Some("adapter") => PollOwner::Adapter,
            _ => return None,
        };
        let tool = poll
            .get("tool")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|tool| !tool.is_empty())?;
        let arguments = poll.get("arguments").and_then(Value::as_object)?;
        // Exactly one identifying argument: see the note above on why a
        // second one makes the descriptor unusable instead of merely extra.
        let mut keys = arguments.keys();
        let (Some(argument_field), None) = (keys.next(), keys.next()) else {
            return None;
        };
        Some(Self {
            owner,
            tool: tool.to_string(),
            argument_field: argument_field.clone(),
        })
    }

    /// Render the descriptor form used inside result envelopes.
    #[must_use]
    pub fn to_value(&self, job_id: &str) -> Value {
        json!({
            "owner": self.owner,
            "tool": self.tool,
            "arguments": { self.argument_field.as_str(): job_id },
        })
    }
}

/// Authoritative on-disk progress source.
///
/// Renderers that cache their own counter can report `0` while frames are
/// already written (#2262). When a job declares where its outputs land, the
/// poller counts them instead of trusting the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputCounter {
    /// Directory the job writes into.
    pub dir: PathBuf,
    /// Extensions that count as outputs. Empty means "every file".
    pub extensions: Vec<String>,
}

impl OutputCounter {
    /// Counter over every file below `dir`.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            extensions: Vec::new(),
        }
    }

    /// Restrict the count to `extensions` (dot optional, matched
    /// case-insensitively).
    #[must_use]
    pub fn with_extensions<I, S>(mut self, extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extensions = extensions
            .into_iter()
            .map(Into::into)
            .filter(|extension| !extension.trim().is_empty())
            .collect();
        self
    }

    /// Count matching files below [`Self::dir`].
    ///
    /// `None` means "unknown" — the directory is not there (yet), so a caller
    /// must fall back to the reported counter rather than read this as zero.
    #[must_use]
    pub fn count(&self) -> Option<u64> {
        if !self.dir.is_dir() {
            return None;
        }
        let mut stack = vec![(self.dir.clone(), 0usize)];
        let mut total = 0u64;
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                // `DirEntry::file_type` reports the entry itself, so a symlink
                // to a directory is `is_symlink()` and is never descended
                // into: the walk cannot loop, and symlinked outputs are simply
                // not counted.
                if file_type.is_dir() {
                    if depth + 1 < MAX_WALK_DEPTH {
                        stack.push((entry.path(), depth + 1));
                    }
                } else if file_type.is_file()
                    && self.matches(entry.file_name().to_string_lossy().as_ref())
                {
                    total += 1;
                }
            }
            if total >= MAX_WALK_ENTRIES {
                return Some(MAX_WALK_ENTRIES);
            }
        }
        Some(total)
    }

    fn matches(&self, file_name: &str) -> bool {
        if self.extensions.is_empty() {
            return true;
        }
        let extension = match file_name.rsplit_once('.') {
            Some((_, extension)) => extension,
            None => return false,
        };
        self.extensions.iter().any(|candidate| {
            candidate
                .trim_start_matches('.')
                .eq_ignore_ascii_case(extension)
        })
    }
}

/// One async job type's registration with the unified poller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobPollRegistration {
    /// Tool that starts the job (e.g. `houdini_render__render_rop`).
    pub job_type: String,
    /// How to poll it to a terminal state.
    pub poll: PollContract,
    /// Where its outputs land, when the job declares one.
    pub output: Option<OutputCounter>,
}

impl JobPollRegistration {
    /// Registration without an output counter.
    #[must_use]
    pub fn new(job_type: impl Into<String>, poll: PollContract) -> Self {
        Self {
            job_type: job_type.into(),
            poll,
            output: None,
        }
    }

    /// Attach the output directory used for disk-backed counters.
    #[must_use]
    pub fn with_output(mut self, output: OutputCounter) -> Self {
        self.output = Some(output);
        self
    }
}

/// Why a registration was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollRegistryError {
    /// The job type (launching tool name) was empty.
    EmptyJobType,
    /// The poll contract named no tool.
    EmptyPollTool,
}

impl fmt::Display for PollRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PollRegistryError::EmptyJobType => {
                f.write_str("job poll registration needs a job type")
            }
            PollRegistryError::EmptyPollTool => {
                f.write_str("job poll registration needs a poll tool name")
            }
        }
    }
}

impl std::error::Error for PollRegistryError {}

/// Why a job type cannot be polled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnregisteredReason {
    /// Nothing ever registered this job type.
    NotRegistered,
    /// No job type was supplied in the query.
    MissingJobType,
}

impl UnregisteredReason {
    /// Stable wire identifier surfaced in envelopes and diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            UnregisteredReason::NotRegistered => "job_type_not_registered",
            UnregisteredReason::MissingJobType => "job_type_missing",
        }
    }
}

/// The unified poller's registry: job type → poll contract.
///
/// Send + Sync and cheap to clone behind an `Arc`; every mutation is a short
/// critical section so a status poll never blocks on a registration.
#[derive(Debug, Default)]
pub struct JobPollRegistry {
    registrations: RwLock<HashMap<String, JobPollRegistration>>,
}

impl JobPollRegistry {
    /// Empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the poll contract for a job type.
    ///
    /// # Errors
    ///
    /// Returns [`PollRegistryError`] when the job type or poll tool is empty.
    pub fn register(&self, registration: JobPollRegistration) -> Result<(), PollRegistryError> {
        // Normalize once so lookups stay exact: `resolve`, `poll_tool`,
        // `unregistered_reason` and `contract_value` all trim their query, so
        // storing an untrimmed key would hide the registration from every one
        // of them.
        let job_type = registration.job_type.trim().to_string();
        if job_type.is_empty() {
            return Err(PollRegistryError::EmptyJobType);
        }
        if registration.poll.tool.trim().is_empty() {
            return Err(PollRegistryError::EmptyPollTool);
        }
        let registration = JobPollRegistration {
            job_type: job_type.clone(),
            ..registration
        };
        self.registrations.write().insert(job_type, registration);
        Ok(())
    }

    /// Look up a job type's registration.
    #[must_use]
    pub fn resolve(&self, job_type: &str) -> Option<JobPollRegistration> {
        self.registrations.read().get(job_type).cloned()
    }

    /// Poll tool for a job type, if one is registered.
    #[must_use]
    pub fn poll_tool(&self, job_type: &str) -> Option<String> {
        self.registrations
            .read()
            .get(job_type)
            .map(|registration| registration.poll.tool.clone())
    }

    /// Number of registered job types.
    #[must_use]
    pub fn len(&self) -> usize {
        self.registrations.read().len()
    }

    /// `true` when nothing has registered yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registrations.read().is_empty()
    }

    /// Every registration, ordered by job type.
    #[must_use]
    pub fn list(&self) -> Vec<JobPollRegistration> {
        let mut registrations: Vec<JobPollRegistration> =
            self.registrations.read().values().cloned().collect();
        registrations.sort_by(|left, right| left.job_type.cmp(&right.job_type));
        registrations
    }

    /// Why `job_type` cannot be polled; `None` when it can.
    #[must_use]
    pub fn unregistered_reason(&self, job_type: Option<&str>) -> Option<UnregisteredReason> {
        // A missing job type is itself a reason the job cannot be polled; it
        // must not read as "nothing wrong", which is what `None` means here.
        let Some(job_type) = job_type.map(str::trim).filter(|value| !value.is_empty()) else {
            return Some(UnregisteredReason::MissingJobType);
        };
        if self.resolve(job_type).is_some() {
            return None;
        }
        Some(UnregisteredReason::NotRegistered)
    }

    /// Envelope fragment describing one job type's registration.
    #[must_use]
    pub fn contract_value(&self, job_type: Option<&str>) -> Value {
        let Some(job_type) = job_type.map(str::trim).filter(|value| !value.is_empty()) else {
            return json!({
                "registered": false,
                "reason": UnregisteredReason::MissingJobType.as_str(),
            });
        };
        match self.resolve(job_type) {
            Some(registration) => {
                let mut contract = json!({
                    "registered": true,
                    "job_type": registration.job_type,
                    "poll": {
                        "owner": registration.poll.owner,
                        "tool": registration.poll.tool,
                        "argument_field": registration.poll.argument_field,
                    },
                });
                if let Some(output) = registration.output.as_ref() {
                    contract["output"] = json!({
                        "dir": output.dir,
                        "extensions": output.extensions,
                    });
                }
                contract
            }
            None => json!({
                "registered": false,
                "job_type": job_type,
                "reason": UnregisteredReason::NotRegistered.as_str(),
                "hint": "Register the job type with the unified poller (declare a poll descriptor \
                         on the launching tool) so --wait can reach a terminal state.",
            }),
        }
    }
}

/// Reconcile a handler-reported counter against authoritative on-disk state.
///
/// The observed count wins whenever it is ahead of the report — that is the
/// #2262 shape, where a renderer reports `0` while frames are already written.
/// The payload records `counter_source` (and `reported_current` when the two
/// disagree) so a caller can tell which signal it got, and `total` is never
/// left below `current` so progress renders instead of dividing by zero.
///
/// Returns `None` only when neither side knows anything.
#[must_use]
pub fn reconcile_progress(reported: Option<&JobProgress>, observed: Option<u64>) -> Option<Value> {
    let (current, total, message) = match reported {
        Some(progress) => (
            Some(progress.current),
            progress.total,
            progress.message.as_deref(),
        ),
        None => (None, 0, None),
    };
    reconcile(current, total, message, observed)
}

/// Same as [`reconcile_progress`] for a progress payload that arrives as raw
/// JSON — the shape adapters put in their result envelopes.
#[must_use]
pub fn reconcile_progress_value(reported: Option<&Value>, observed: Option<u64>) -> Option<Value> {
    let (current, total, message) = match reported.filter(|value| value.is_object()) {
        Some(reported) => (
            reported.get("current").and_then(Value::as_u64),
            reported.get("total").and_then(Value::as_u64).unwrap_or(0),
            reported.get("message").and_then(Value::as_str),
        ),
        None => (None, 0, None),
    };
    reconcile(current, total, message, observed)
}

fn reconcile(
    current: Option<u64>,
    total: u64,
    message: Option<&str>,
    observed: Option<u64>,
) -> Option<Value> {
    if current.is_none() && observed.is_none() {
        return None;
    }
    let mut payload = json!({
        "current": current.unwrap_or(0),
        "total": total,
    });
    if let Some(message) = message {
        payload["message"] = Value::String(message.to_string());
    }
    let reported_current = current.unwrap_or(0);
    match observed {
        Some(observed) if observed > reported_current => {
            if current.is_some() {
                payload["reported_current"] = json!(reported_current);
            }
            payload["current"] = json!(observed);
            payload["counter_source"] = Value::String(COUNTER_SOURCE_DISK.to_string());
        }
        _ => {
            payload["counter_source"] = Value::String(COUNTER_SOURCE_REPORTED.to_string());
        }
    }
    let total = payload.get("total").and_then(Value::as_u64).unwrap_or(0);
    let current = payload.get("current").and_then(Value::as_u64).unwrap_or(0);
    if total < current {
        payload["total"] = json!(current);
    }
    Some(payload)
}

/// Extract an output directory + extensions from a job result envelope.
///
/// Looks in the places adapters already write them (`output_dir` /
/// `output.dir`, optionally `output_extensions` / `output.extensions`) so a
/// job becomes disk-counted without a schema change.
#[must_use]
pub fn output_counter_from_result(result: &Value) -> Option<OutputCounter> {
    let candidates = [
        result.pointer("/output_dir"),
        result.pointer("/output/dir"),
        result.pointer("/context/output_dir"),
        result.pointer("/context/output/dir"),
        result.pointer("/result/output_dir"),
    ];
    let dir = candidates
        .into_iter()
        .flatten()
        .find_map(Value::as_str)
        .map(str::trim)
        .filter(|dir| !dir.is_empty())?;
    let extensions = [
        result.pointer("/output_extensions"),
        result.pointer("/output/extensions"),
        result.pointer("/context/output_extensions"),
        result.pointer("/context/output/extensions"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_array)
    .map(|values| {
        values
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    Some(OutputCounter::new(dir).with_extensions(extensions))
}

impl From<OutputCounter> for crate::job::JobOutputTarget {
    fn from(counter: OutputCounter) -> Self {
        Self {
            dir: counter.dir.to_string_lossy().into_owned(),
            extensions: counter.extensions,
        }
    }
}

/// Rebuild a counter from the output target captured at launch.
///
/// Lets a job that has not reached a terminal state — and so has no result to
/// read — still reconcile its counters against disk (issue #2262).
#[must_use]
pub fn output_counter_from_target(target: &crate::job::JobOutputTarget) -> OutputCounter {
    OutputCounter::new(&target.dir).with_extensions(target.extensions.iter().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dcc-mcp-job-poller-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn terminal_vocabulary_is_shared() {
        assert!(is_known_job_status("running"));
        assert!(!is_known_job_status("done"));
        assert!(is_terminal_job_status("completed"));
        assert!(is_terminal_job_status("interrupted"));
        assert!(!is_terminal_job_status("running"));
    }

    #[test]
    fn poll_contract_parses_declared_descriptor() {
        let contract = PollContract::from_value(Some(&json!({
            "owner": "adapter",
            "tool": "render__get_render_job",
            "arguments": {"job_id": "job-42"},
        })))
        .expect("declared contract");

        assert_eq!(contract.owner, PollOwner::Adapter);
        assert_eq!(contract.tool, "render__get_render_job");
        assert_eq!(contract.argument_field, "job_id");
        assert_eq!(
            contract.to_value("job-42"),
            json!({
                "owner": "adapter",
                "tool": "render__get_render_job",
                "arguments": {"job_id": "job-42"},
            })
        );
    }

    #[test]
    fn poll_contract_rejects_unusable_descriptors() {
        assert!(PollContract::from_value(None).is_none());
        assert!(
            PollContract::from_value(Some(&json!({"tool": "t", "arguments": {"job_id": "j"}})))
                .is_none()
        );
        assert!(
            PollContract::from_value(Some(
                &json!({"owner": "adapter", "arguments": {"job_id": "j"}})
            ))
            .is_none()
        );
        // Arguments must identify exactly one job and nothing else: a second
        // argument cannot be replayed, so the descriptor is unusable rather
        // than silently lossy.
        assert!(
            PollContract::from_value(Some(&json!({
                "owner": "adapter",
                "tool": "t",
                "arguments": {"job_id": "j", "frame": 1},
            })))
            .is_none()
        );
        assert_eq!(
            PollContract::from_value(Some(&json!({
                "owner": "adapter",
                "tool": "t",
                "arguments": {"render_id": "j"},
            })))
            .map(|contract| contract.argument_field),
            Some("render_id".to_string())
        );
        assert!(
            PollContract::from_value(Some(&json!({
                "owner": "adapter",
                "tool": "t",
                "arguments": {},
            })))
            .is_none()
        );
        assert!(
            PollContract::from_value(Some(&json!({
                "owner": "adapter",
                "tool": "t",
                "arguments": {"a": "1", "b": "2"},
            })))
            .is_none()
        );
    }

    #[test]
    fn output_counter_counts_files_on_disk() {
        let dir = tempdir("count");
        fs::write(dir.join("frame.0001.exr"), b"a").unwrap();
        fs::write(dir.join("frame.0002.exr"), b"a").unwrap();
        fs::write(dir.join("notes.txt"), b"a").unwrap();
        let nested = dir.join("seq");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("frame.0003.exr"), b"a").unwrap();

        let counter = OutputCounter::new(&dir).with_extensions(["exr"]);
        assert_eq!(counter.count(), Some(3));
        assert_eq!(OutputCounter::new(&dir).count(), Some(4));
        assert_eq!(
            OutputCounter::new(dir.join("missing")).count(),
            None,
            "a missing directory is unknown, not zero"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_counter_walk_is_bounded_by_entry_count() {
        let dir = tempdir("cap");
        for index in 0..(MAX_WALK_ENTRIES + 500) {
            fs::write(dir.join(format!("frame.{index:06}.exr")), b"a").unwrap();
        }

        let counter = OutputCounter::new(&dir);
        assert_eq!(
            counter.count(),
            Some(MAX_WALK_ENTRIES),
            "a saturated walk reports the cap instead of scanning forever"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_reports_unregistered_job_types_explicitly() {
        let registry = JobPollRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(
            registry.unregistered_reason(Some("render__render_rop")),
            Some(UnregisteredReason::NotRegistered)
        );
        assert_eq!(
            registry.contract_value(Some("render__render_rop"))["reason"],
            "job_type_not_registered"
        );
        assert_eq!(registry.contract_value(None)["reason"], "job_type_missing");
        // A missing job type is a reason the job cannot be polled, not
        // "nothing wrong" — `None` must mean exactly that.
        assert_eq!(
            registry.unregistered_reason(None),
            Some(UnregisteredReason::MissingJobType)
        );
        assert_eq!(
            registry.unregistered_reason(Some("   ")),
            Some(UnregisteredReason::MissingJobType)
        );

        registry
            .register(JobPollRegistration::new(
                "render__render_rop",
                PollContract::adapter("render__get_render_job"),
            ))
            .expect("register");

        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.poll_tool("render__render_rop").as_deref(),
            Some("render__get_render_job"),
            "an untracked job type becomes pollable once registered"
        );
        assert_eq!(
            registry.unregistered_reason(Some("render__render_rop")),
            None
        );
        assert_eq!(
            registry.contract_value(Some("render__render_rop"))["registered"],
            true
        );
    }

    #[test]
    fn registry_normalizes_the_job_type_key_on_registration() {
        let registry = JobPollRegistry::new();
        registry
            .register(JobPollRegistration::new(
                "  render__render_rop  ",
                PollContract::adapter("render__get_render_job"),
            ))
            .expect("register");

        assert_eq!(
            registry.len(),
            1,
            "a padded job type is stored once, under its trimmed key"
        );
        assert_eq!(
            registry.poll_tool("render__render_rop").as_deref(),
            Some("render__get_render_job"),
            "a registration that validated is also resolvable"
        );
        assert_eq!(
            registry.unregistered_reason(Some("render__render_rop")),
            None
        );
        assert_eq!(
            registry.resolve("render__render_rop").map(|r| r.job_type),
            Some("render__render_rop".to_string())
        );
    }

    #[test]
    fn registry_rejects_empty_registrations() {
        let registry = JobPollRegistry::new();
        assert_eq!(
            registry.register(JobPollRegistration::new("  ", PollContract::adapter("t"))),
            Err(PollRegistryError::EmptyJobType)
        );
        assert_eq!(
            registry.register(JobPollRegistration::new("t", PollContract::adapter(" "))),
            Err(PollRegistryError::EmptyPollTool)
        );
    }

    #[test]
    fn disk_count_beats_a_cached_zero_counter() {
        let progress = JobProgress {
            current: 0,
            total: 96,
            message: Some("rendering".to_string()),
        };

        let payload = reconcile_progress(Some(&progress), Some(12)).expect("reconciled");
        assert_eq!(payload["current"], 12, "frames on disk beat a cached zero");
        assert_eq!(payload["total"], 96);
        assert_eq!(payload["reported_current"], 0);
        assert_eq!(payload["counter_source"], COUNTER_SOURCE_DISK);
    }

    #[test]
    fn reported_counter_wins_when_it_is_ahead_of_disk() {
        let progress = JobProgress {
            current: 90,
            total: 90,
            message: None,
        };

        let payload = reconcile_progress(Some(&progress), Some(12)).expect("reconciled");
        assert_eq!(payload["current"], 90);
        assert_eq!(payload["counter_source"], COUNTER_SOURCE_REPORTED);
        assert!(payload.get("reported_current").is_none());
    }

    #[test]
    fn disk_only_progress_never_divides_by_zero() {
        let payload = reconcile_progress(None, Some(7)).expect("reconciled");
        assert_eq!(payload["current"], 7);
        assert_eq!(payload["total"], 7);
        assert_eq!(payload["counter_source"], COUNTER_SOURCE_DISK);
        assert_eq!(reconcile_progress(None, None), None);
    }

    #[test]
    fn disk_count_beats_a_cached_zero_in_a_raw_result_payload() {
        let payload = reconcile_progress_value(
            Some(&json!({"current": 0, "total": 96, "message": "rendering"})),
            Some(12),
        )
        .expect("reconciled");

        assert_eq!(payload["current"], 12);
        assert_eq!(payload["total"], 96);
        assert_eq!(payload["message"], "rendering");
        assert_eq!(payload["counter_source"], COUNTER_SOURCE_DISK);
        assert!(reconcile_progress_value(None, None).is_none());
        assert_eq!(
            reconcile_progress_value(Some(&json!(null)), Some(3))
                .map(|payload| payload["current"].clone()),
            Some(json!(3)),
            "an unusable report still yields the on-disk count"
        );
    }

    #[test]
    fn output_counter_is_read_from_result_envelopes() {
        let counter = output_counter_from_result(&json!({
            "output_dir": "/tmp/renders",
            "output_extensions": ["exr"],
        }))
        .expect("counter");
        assert_eq!(counter.dir, PathBuf::from("/tmp/renders"));
        assert_eq!(counter.extensions, vec!["exr".to_string()]);

        assert_eq!(
            output_counter_from_result(&json!({"context": {"output_dir": "/tmp/x"}}))
                .expect("nested counter")
                .dir,
            PathBuf::from("/tmp/x")
        );
        assert!(output_counter_from_result(&json!({"frames": 3})).is_none());
    }
}
