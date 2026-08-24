use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
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

use serde::Deserialize;

use crate::application::feedback_file::{
    FeedbackIssueRecord, FeedbackIssueSearchField, FeedbackIssueState, FeedbackIssueTracker,
    FeedbackIssueTrackerError,
};
use crate::domain::feedback_file::FeedbackIssueCandidate;

pub struct GhFeedbackIssueTracker {
    program: OsString,
}

const GH_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const GH_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

impl Default for GhFeedbackIssueTracker {
    fn default() -> Self {
        Self {
            program: OsString::from("gh"),
        }
    }
}

impl GhFeedbackIssueTracker {
    #[must_use]
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn run(
        &self,
        args: &[String],
        stdin_body: Option<&str>,
    ) -> Result<Output, FeedbackIssueTrackerError> {
        let command = self.command(args, stdin_body.is_some());
        let output = run_child_bounded(command, stdin_body, GH_OPERATION_TIMEOUT)?;
        if !output.status.success() {
            let code = output
                .status
                .code()
                .map_or_else(|| "terminated".to_string(), |code| code.to_string());
            return Err(tracker_error(format!(
                "GitHub issue operation failed (exit {code}); check gh auth status and repository access"
            )));
        }
        Ok(output)
    }

    fn command(&self, args: &[String], piped_stdin: bool) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .env("GH_HOST", "github.com")
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if piped_stdin {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        command
    }
}

fn run_child_bounded(
    mut command: Command,
    stdin_body: Option<&str>,
    timeout: Duration,
) -> Result<Output, FeedbackIssueTrackerError> {
    let mut tree = OwnedChildTree::spawn(&mut command)?;
    let stdout = match tree.child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            tree.terminate_and_reap(Instant::now() + GH_CLEANUP_TIMEOUT);
            return Err(tracker_error("GitHub CLI stdout was unavailable"));
        }
    };
    let stderr = match tree.child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            tree.terminate_and_reap(Instant::now() + GH_CLEANUP_TIMEOUT);
            return Err(tracker_error("GitHub CLI stderr was unavailable"));
        }
    };
    let stdin = match stdin_body {
        Some(_) => match tree.child.stdin.take() {
            Some(stdin) => Some(stdin),
            None => {
                tree.terminate_and_reap(Instant::now() + GH_CLEANUP_TIMEOUT);
                return Err(tracker_error("GitHub CLI stdin was unavailable"));
            }
        },
        None => None,
    };
    let workers = IoWorkers::spawn(stdout, stderr, stdin, stdin_body);

    let operation_deadline = Instant::now() + timeout;
    let status = loop {
        match tree.child.try_wait() {
            Ok(Some(status)) => {
                tree.terminate_descendants();
                break status;
            }
            Ok(None) if Instant::now() < operation_deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let cleanup_deadline = Instant::now() + GH_CLEANUP_TIMEOUT;
                let reaped = tree.terminate_and_reap(cleanup_deadline);
                let drained = workers.collect_until(cleanup_deadline).is_ok();
                return Err(tracker_error(if reaped && drained {
                    "GitHub CLI timed out and was terminated"
                } else {
                    "GitHub CLI timed out and child cleanup could not be confirmed"
                }));
            }
            Err(_) => {
                let cleanup_deadline = Instant::now() + GH_CLEANUP_TIMEOUT;
                let reaped = tree.terminate_and_reap(cleanup_deadline);
                let drained = workers.collect_until(cleanup_deadline).is_ok();
                return Err(tracker_error(if reaped && drained {
                    "GitHub CLI did not complete"
                } else {
                    "GitHub CLI failed and child cleanup could not be confirmed"
                }));
            }
        }
    };

    let cleanup_deadline = Instant::now() + GH_CLEANUP_TIMEOUT;
    let pipes = workers.collect_until(cleanup_deadline).map_err(|failure| {
        tree.terminate_descendants();
        tracker_error(failure.message())
    })?;
    Ok(Output {
        status,
        stdout: pipes.stdout,
        stderr: pipes.stderr,
    })
}

struct OwnedChildTree {
    child: Child,
    #[cfg(unix)]
    process_group: i32,
    #[cfg(windows)]
    job: OwnedHandle,
}

impl OwnedChildTree {
    fn spawn(command: &mut Command) -> Result<Self, FeedbackIssueTrackerError> {
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(0x0000_0004); // CREATE_SUSPENDED
        let child = command.spawn().map_err(|_| {
            tracker_error(
                "GitHub CLI could not be started; install gh and authenticate before filing feedback",
            )
        })?;
        #[cfg(unix)]
        {
            let process_group = child.id() as i32;
            Ok(Self {
                child,
                process_group,
            })
        }
        #[cfg(windows)]
        {
            match assign_kill_on_close_job(&child) {
                Ok(job) => match resume_suspended_process(&child) {
                    Ok(()) => Ok(Self { child, job }),
                    Err(_) => {
                        let mut tree = Self { child, job };
                        tree.terminate_and_reap(Instant::now() + GH_CLEANUP_TIMEOUT);
                        Err(tracker_error(
                            "GitHub CLI process tree could not be started safely",
                        ))
                    }
                },
                Err(_) => {
                    let mut child = child;
                    let _ = child.kill();
                    let _ = child.wait();
                    Err(tracker_error(
                        "GitHub CLI process tree could not be owned safely",
                    ))
                }
            }
        }
    }

    fn terminate_descendants(&self) -> bool {
        #[cfg(unix)]
        {
            // SAFETY: the negative PID targets only the dedicated process group
            // created for this child before exec.
            let result = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
            if result == 0 {
                return true;
            }
            std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::HANDLE;
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;

            // SAFETY: `job` is an owned, live Job Object handle.
            unsafe { TerminateJobObject(self.job.as_raw_handle() as HANDLE, 1) != 0 }
        }
    }

    fn terminate_and_reap(&mut self, deadline: Instant) -> bool {
        let tree_terminated = self.terminate_descendants();
        let _ = self.child.kill();
        let reaped = loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break true,
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(None) | Err(_) => break false,
            }
        };
        tree_terminated && reaped
    }
}

#[cfg(windows)]
fn assign_kill_on_close_job(child: &Child) -> std::io::Result<OwnedHandle> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    // SAFETY: null name/security pointers request an unnamed Job Object with defaults.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: the information pointer and size match the requested Job Object class.
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    // SAFETY: the standard-library child handle remains live for this assignment call.
    let assigned = configured != 0
        && unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) } != 0;
    if !assigned {
        let error = std::io::Error::last_os_error();
        // SAFETY: `job` was created successfully and is not yet owned by `OwnedHandle`.
        unsafe { CloseHandle(job) };
        return Err(error);
    }
    // SAFETY: ownership of the unique Job Object handle transfers to `OwnedHandle`.
    Ok(unsafe { OwnedHandle::from_raw_handle(job as _) })
}

#[cfg(windows)]
fn resume_suspended_process(child: &Child) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // SAFETY: the snapshot handle is validated before it is wrapped and used.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: ownership of the unique snapshot handle transfers to `OwnedHandle`.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot as _) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    // SAFETY: `entry` has the required size and remains writable for enumeration.
    let mut found = unsafe { Thread32First(snapshot.as_raw_handle() as _, &mut entry) } != 0;
    while found {
        if entry.th32OwnerProcessID == child.id() {
            // SAFETY: the thread ID came from the live system snapshot.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: `thread` is a live handle opened with suspend/resume rights.
            let previous_count = unsafe { ResumeThread(thread) };
            // SAFETY: `thread` is not wrapped elsewhere and must be closed exactly once.
            unsafe { CloseHandle(thread) };
            if previous_count == u32::MAX {
                return Err(std::io::Error::last_os_error());
            }
            return Ok(());
        }
        // SAFETY: `entry` remains valid for the next snapshot record.
        found = unsafe { Thread32Next(snapshot.as_raw_handle() as _, &mut entry) } != 0;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "suspended GitHub CLI thread was not found",
    ))
}

enum IoEvent {
    Stdout(std::io::Result<Vec<u8>>),
    Stderr(std::io::Result<Vec<u8>>),
    Stdin(std::io::Result<()>),
}

struct IoWorkers {
    receiver: Receiver<IoEvent>,
    handles: Vec<thread::JoinHandle<()>>,
    expected_events: usize,
}

struct PipeOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
enum IoFailure {
    Timeout,
    Worker,
    Stdin,
    Stdout,
    Stderr,
}

impl IoFailure {
    fn message(self) -> &'static str {
        match self {
            Self::Timeout | Self::Worker => "GitHub CLI pipe cleanup could not be confirmed",
            Self::Stdin => "GitHub CLI could not receive the issue body",
            Self::Stdout => "GitHub CLI stdout was unavailable",
            Self::Stderr => "GitHub CLI stderr was unavailable",
        }
    }
}

impl IoWorkers {
    fn spawn(
        stdout: std::process::ChildStdout,
        stderr: std::process::ChildStderr,
        stdin: Option<std::process::ChildStdin>,
        stdin_body: Option<&str>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let stdout_sender = sender.clone();
        let stdout_reader = thread::spawn(move || {
            let mut stdout = stdout;
            let mut bytes = Vec::new();
            let result = stdout.read_to_end(&mut bytes).map(|_| bytes);
            let _ = stdout_sender.send(IoEvent::Stdout(result));
        });
        let stderr_sender = sender.clone();
        let stderr_reader = thread::spawn(move || {
            let mut stderr = stderr;
            let mut bytes = Vec::new();
            let result = stderr.read_to_end(&mut bytes).map(|_| bytes);
            let _ = stderr_sender.send(IoEvent::Stderr(result));
        });
        let mut handles = vec![stdout_reader, stderr_reader];
        let mut expected_events = 2;
        if let Some(mut stdin) = stdin {
            let stdin_sender = sender.clone();
            let bytes = stdin_body
                .expect("stdin exists only for a body")
                .as_bytes()
                .to_vec();
            handles.push(thread::spawn(move || {
                let _ = stdin_sender.send(IoEvent::Stdin(stdin.write_all(&bytes)));
            }));
            expected_events += 1;
        }
        drop(sender);
        Self {
            receiver,
            handles,
            expected_events,
        }
    }

    fn collect_until(self, deadline: Instant) -> Result<PipeOutput, IoFailure> {
        let mut stdout = None;
        let mut stderr = None;
        let mut stdin_ok = self.expected_events == 2;
        let mut failure = None;
        for _ in 0..self.expected_events {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                failure.get_or_insert(IoFailure::Timeout);
                break;
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(IoEvent::Stdout(Ok(bytes))) => stdout = Some(bytes),
                Ok(IoEvent::Stdout(Err(_))) => {
                    failure.get_or_insert(IoFailure::Stdout);
                }
                Ok(IoEvent::Stderr(Ok(bytes))) => stderr = Some(bytes),
                Ok(IoEvent::Stderr(Err(_))) => {
                    failure.get_or_insert(IoFailure::Stderr);
                }
                Ok(IoEvent::Stdin(Ok(()))) => stdin_ok = true,
                Ok(IoEvent::Stdin(Err(_))) => {
                    failure.get_or_insert(IoFailure::Stdin);
                }
                Err(_) => {
                    failure.get_or_insert(IoFailure::Timeout);
                    break;
                }
            }
        }
        for handle in self.handles {
            while !handle.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
            if !handle.is_finished() || handle.join().is_err() {
                failure.get_or_insert(IoFailure::Worker);
            }
        }
        if !stdin_ok {
            failure.get_or_insert(IoFailure::Stdin);
        }
        if let Some(failure) = failure {
            return Err(failure);
        }
        Ok(PipeOutput {
            stdout: stdout.ok_or(IoFailure::Stdout)?,
            stderr: stderr.ok_or(IoFailure::Stderr)?,
        })
    }
}

impl FeedbackIssueTracker for GhFeedbackIssueTracker {
    fn search_open(
        &self,
        repo: &str,
        query: &str,
        fields: &[FeedbackIssueSearchField],
        limit: usize,
    ) -> Result<Vec<FeedbackIssueRecord>, FeedbackIssueTrackerError> {
        if !(1..=100).contains(&limit) {
            return Err(tracker_error(
                "GitHub issue search limit must be between 1 and 100",
            ));
        }
        let output = self.run(&search_args(repo, query, fields, limit), None)?;
        parse_issue_records(repo, &output.stdout)
    }

    fn view_issue(
        &self,
        repo: &str,
        number: u64,
    ) -> Result<FeedbackIssueRecord, FeedbackIssueTrackerError> {
        let output = self.run(&view_args(repo, number), None)?;
        let value: GhIssue = serde_json::from_slice(&output.stdout)
            .map_err(|_| tracker_error("GitHub CLI returned invalid issue data"))?;
        parse_issue(repo, value)
    }

    fn comment_issue(
        &self,
        repo: &str,
        number: u64,
        body: &str,
    ) -> Result<(), FeedbackIssueTrackerError> {
        self.run(&comment_args(repo, number), Some(body))?;
        Ok(())
    }

    fn create_issue(
        &self,
        repo: &str,
        title: &str,
        body: &str,
    ) -> Result<FeedbackIssueCandidate, FeedbackIssueTrackerError> {
        let output = self.run(&create_args(repo, title), Some(body))?;
        let url = std::str::from_utf8(&output.stdout)
            .map_err(|_| tracker_error("GitHub CLI returned an invalid issue URL"))?
            .trim();
        let number = parse_canonical_issue_url(repo, url)
            .ok_or_else(|| tracker_error("GitHub CLI returned an unexpected issue URL"))?;
        Ok(FeedbackIssueCandidate {
            number,
            title: title.to_string(),
            url: url.to_string(),
        })
    }
}

fn search_args(
    repo: &str,
    query: &str,
    fields: &[FeedbackIssueSearchField],
    limit: usize,
) -> Vec<String> {
    let mut args = vec![
        "search".to_string(),
        "issues".to_string(),
        query.to_string(),
        "--repo".to_string(),
        repo.to_string(),
        "--state".to_string(),
        "open".to_string(),
        "--limit".to_string(),
        limit.to_string(),
        "--json".to_string(),
        "number,title,url,body,state".to_string(),
    ];
    for field in fields {
        args.push("--match".to_string());
        args.push(
            match field {
                FeedbackIssueSearchField::Title => "title",
                FeedbackIssueSearchField::Body => "body",
            }
            .to_string(),
        );
    }
    args
}

fn view_args(repo: &str, number: u64) -> Vec<String> {
    vec![
        "issue".to_string(),
        "view".to_string(),
        number.to_string(),
        "--repo".to_string(),
        github_repo(repo),
        "--json".to_string(),
        "number,title,url,body,state".to_string(),
    ]
}

fn comment_args(repo: &str, number: u64) -> Vec<String> {
    vec![
        "issue".to_string(),
        "comment".to_string(),
        number.to_string(),
        "--repo".to_string(),
        github_repo(repo),
        "--body-file".to_string(),
        "-".to_string(),
    ]
}

fn create_args(repo: &str, title: &str) -> Vec<String> {
    vec![
        "issue".to_string(),
        "create".to_string(),
        "--repo".to_string(),
        github_repo(repo),
        "--title".to_string(),
        title.to_string(),
        "--body-file".to_string(),
        "-".to_string(),
    ]
}

fn github_repo(repo: &str) -> String {
    format!("github.com/{repo}")
}

fn parse_issue_records(
    repo: &str,
    bytes: &[u8],
) -> Result<Vec<FeedbackIssueRecord>, FeedbackIssueTrackerError> {
    let values: Vec<GhIssue> = serde_json::from_slice(bytes)
        .map_err(|_| tracker_error("GitHub CLI returned invalid issue search data"))?;
    values
        .into_iter()
        .map(|value| parse_issue(repo, value))
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GhIssue {
    number: u64,
    title: String,
    url: String,
    #[serde(default)]
    body: Option<String>,
    state: String,
}

fn parse_issue(
    repo: &str,
    value: GhIssue,
) -> Result<FeedbackIssueRecord, FeedbackIssueTrackerError> {
    let number = parse_canonical_issue_url(repo, &value.url).ok_or_else(|| {
        tracker_error("GitHub CLI returned an issue outside the routed repository")
    })?;
    if number != value.number {
        return Err(tracker_error(
            "GitHub CLI returned an inconsistent issue number",
        ));
    }
    let state = if value.state.eq_ignore_ascii_case("open") {
        FeedbackIssueState::Open
    } else if value.state.eq_ignore_ascii_case("closed") {
        FeedbackIssueState::Closed
    } else {
        return Err(tracker_error(
            "GitHub CLI returned an unsupported issue state",
        ));
    };
    Ok(FeedbackIssueRecord {
        candidate: FeedbackIssueCandidate {
            number,
            title: value.title,
            url: value.url,
        },
        state,
        body: value.body.unwrap_or_default(),
    })
}

fn parse_canonical_issue_url(repo: &str, url: &str) -> Option<u64> {
    let prefix = format!("https://github.com/{repo}/issues/");
    let number = url.strip_prefix(&prefix)?;
    (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| number.parse().ok())
        .flatten()
        .filter(|number| *number > 0)
}

fn tracker_error(message: impl Into<String>) -> FeedbackIssueTrackerError {
    FeedbackIssueTrackerError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn search_argv_scopes_repository_state_fields_and_bound() {
        let args = search_args(
            "dcc-mcp/dcc-mcp-godot",
            "\"sha256:abc\"",
            &[
                FeedbackIssueSearchField::Title,
                FeedbackIssueSearchField::Body,
            ],
            21,
        );

        assert_eq!(&args[..3], ["search", "issues", "\"sha256:abc\""]);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--repo", "dcc-mcp/dcc-mcp-godot"])
        );
        assert!(args.windows(2).any(|pair| pair == ["--state", "open"]));
        assert!(args.windows(2).any(|pair| pair == ["--limit", "21"]));
        assert!(args.windows(2).any(|pair| pair == ["--match", "title"]));
        assert!(args.windows(2).any(|pair| pair == ["--match", "body"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--json", "number,title,url,body,state"])
        );
    }

    #[test]
    fn parser_accepts_only_canonical_issue_urls_for_the_selected_repo() {
        let records = parse_issue_records(
            "dcc-mcp/dcc-mcp-godot",
            br#"[{"number":42,"title":"Startup fails","url":"https://github.com/dcc-mcp/dcc-mcp-godot/issues/42","body":"marker","state":"open"}]"#,
        )
        .unwrap();
        assert_eq!(records[0].candidate.number, 42);
        let bodyless = parse_issue_records(
            "dcc-mcp/dcc-mcp-godot",
            br#"[{"number":43,"title":"Bodyless","url":"https://github.com/dcc-mcp/dcc-mcp-godot/issues/43","body":null,"state":"OPEN"}]"#,
        )
        .unwrap();
        assert_eq!(bodyless[0].body, "");

        for invalid in [
            br#"[{"number":42,"title":"x","url":"https://github.com/another/repo/issues/42","body":"","state":"open"}]"#.as_slice(),
            br#"[{"number":42,"title":"x","url":"https://github.com/dcc-mcp/dcc-mcp-godot/issues/43","body":"","state":"open"}]"#.as_slice(),
            br#"[{"number":42,"title":"x","url":"https://github.com/dcc-mcp/dcc-mcp-godot/pull/42","body":"","state":"open"}]"#.as_slice(),
        ] {
            assert!(parse_issue_records("dcc-mcp/dcc-mcp-godot", invalid).is_err());
        }
    }

    #[test]
    fn mutation_argv_uses_stdin_body_file_instead_of_inline_content() {
        let comment = comment_args("dcc-mcp/dcc-mcp-godot", 42);
        assert_eq!(
            comment,
            [
                "issue",
                "comment",
                "42",
                "--repo",
                "github.com/dcc-mcp/dcc-mcp-godot",
                "--body-file",
                "-"
            ]
        );
        let create = create_args("dcc-mcp/dcc-mcp-godot", "agent report: startup failed");
        assert_eq!(
            create,
            [
                "issue",
                "create",
                "--repo",
                "github.com/dcc-mcp/dcc-mcp-godot",
                "--title",
                "agent report: startup failed",
                "--body-file",
                "-"
            ]
        );
    }

    #[test]
    fn every_operation_pins_github_dot_com_before_io() {
        let repo = "dcc-mcp/dcc-mcp-godot";
        let expected = "github.com/dcc-mcp/dcc-mcp-godot";
        let operations = [
            view_args(repo, 42),
            comment_args(repo, 42),
            create_args(repo, "agent report: startup failed"),
        ];

        for args in operations {
            let repo_index = args.iter().position(|arg| arg == "--repo").unwrap();
            assert_eq!(args.get(repo_index + 1).map(String::as_str), Some(expected));
        }

        let search = search_args(repo, "startup", &[FeedbackIssueSearchField::Title], 1);
        let repo_index = search.iter().position(|arg| arg == "--repo").unwrap();
        assert_eq!(search[repo_index + 1], repo);

        let command = GhFeedbackIssueTracker::new("gh").command(&[], false);
        let host = command
            .get_envs()
            .find(|(name, _)| *name == "GH_HOST")
            .and_then(|(_, value)| value);
        assert_eq!(host, Some(std::ffi::OsStr::new("github.com")));
    }

    #[test]
    fn bounded_child_timeout_kills_waits_and_returns_a_stable_error() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["-n", "6", "127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("5");
            command
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();

        let error = run_child_bounded(command, None, Duration::from_millis(50)).unwrap_err();

        assert_eq!(error.message, "GitHub CLI timed out and was terminated");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn bounded_child_timeout_terminates_descendants_that_inherit_pipes() {
        let temp = tempfile::tempdir().unwrap();
        let pid_path = temp.path().join("descendant.pid");
        let mut command = descendant_pipe_holder_command(&pid_path);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();

        let error = run_child_bounded(command, None, Duration::from_secs(1)).unwrap_err();

        assert_eq!(error.message, "GitHub CLI timed out and was terminated");
        assert!(started.elapsed() < Duration::from_secs(3));
        let pid = std::fs::read_to_string(&pid_path)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert_process_stops(pid);
    }

    #[cfg(windows)]
    fn descendant_pipe_holder_command(pid_path: &Path) -> Command {
        let path = pid_path.to_string_lossy().replace('\'', "''");
        let script = format!(
            "$p = Start-Process ping.exe -ArgumentList '-n','6','127.0.0.1' -NoNewWindow -PassThru; [IO.File]::WriteAllText('{path}', [string]$p.Id); Start-Sleep -Seconds 30"
        );
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-Command", &script]);
        command
    }

    #[cfg(unix)]
    fn descendant_pipe_holder_command(pid_path: &Path) -> Command {
        let path = pid_path.to_string_lossy().replace('\'', "'\\''");
        let script =
            format!("sleep 5 & child=$!; printf '%s' \"$child\" > '{path}'; wait \"$child\"");
        let mut command = Command::new("sh");
        command.args(["-c", &script]);
        command
    }

    fn assert_process_stops(pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while process_is_alive(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_is_alive(pid),
            "descendant process {pid} survived timeout cleanup"
        );
    }

    #[cfg(windows)]
    fn process_is_alive(pid: u32) -> bool {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 1 }}"
                ),
            ])
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(unix)]
    fn process_is_alive(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|status| status.success())
    }
}
