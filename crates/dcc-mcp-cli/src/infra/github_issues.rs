use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
    let mut child = command.spawn().map_err(|_| {
        tracker_error(
            "GitHub CLI could not be started; install gh and authenticate before filing feedback",
        )
    })?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_and_wait(&mut child);
            return Err(tracker_error("GitHub CLI stdout was unavailable"));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate_and_wait(&mut child);
            return Err(tracker_error("GitHub CLI stderr was unavailable"));
        }
    };
    let stdin = match stdin_body {
        Some(_) => match child.stdin.take() {
            Some(stdin) => Some(stdin),
            None => {
                terminate_and_wait(&mut child);
                return Err(tracker_error("GitHub CLI stdin was unavailable"));
            }
        },
        None => None,
    };
    let stdout_reader = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stdin_writer = stdin.map(|mut stdin| {
        let bytes = stdin_body
            .expect("stdin exists only for a body")
            .as_bytes()
            .to_vec();
        thread::spawn(move || stdin.write_all(&bytes))
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let reaped = terminate_and_wait(&mut child);
                join_after_termination(stdin_writer, stdout_reader, stderr_reader);
                return Err(tracker_error(if reaped {
                    "GitHub CLI timed out and was terminated"
                } else {
                    "GitHub CLI timed out and child cleanup could not be confirmed"
                }));
            }
            Err(_) => {
                let reaped = terminate_and_wait(&mut child);
                join_after_termination(stdin_writer, stdout_reader, stderr_reader);
                return Err(tracker_error(if reaped {
                    "GitHub CLI did not complete"
                } else {
                    "GitHub CLI failed and child cleanup could not be confirmed"
                }));
            }
        }
    };

    if let Some(writer) = stdin_writer {
        match writer.join() {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => {
                return Err(tracker_error("GitHub CLI could not receive the issue body"));
            }
        }
    }
    let stdout = stdout_reader
        .join()
        .ok()
        .and_then(Result::ok)
        .ok_or_else(|| tracker_error("GitHub CLI stdout was unavailable"))?;
    let stderr = stderr_reader
        .join()
        .ok()
        .and_then(Result::ok)
        .ok_or_else(|| tracker_error("GitHub CLI stderr was unavailable"))?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn terminate_and_wait(child: &mut std::process::Child) -> bool {
    let _ = child.kill();
    child.wait().is_ok()
}

fn join_after_termination(
    stdin_writer: Option<thread::JoinHandle<std::io::Result<()>>>,
    stdout_reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stderr_reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
) {
    if let Some(writer) = stdin_writer {
        let _ = writer.join();
    }
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
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
}
