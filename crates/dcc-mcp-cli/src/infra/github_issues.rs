use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde::Deserialize;

use crate::application::feedback_file::{
    FeedbackIssueRecord, FeedbackIssueSearchField, FeedbackIssueState, FeedbackIssueTracker,
    FeedbackIssueTrackerError,
};
use crate::domain::feedback_file::FeedbackIssueCandidate;

pub struct GhFeedbackIssueTracker {
    program: OsString,
}

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
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if stdin_body.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        let mut child = command.spawn().map_err(|_| tracker_error(
            "GitHub CLI could not be started; install gh and authenticate before filing feedback",
        ))?;
        if let Some(body) = stdin_body {
            let write_result = child
                .stdin
                .take()
                .ok_or_else(|| tracker_error("GitHub CLI stdin was unavailable"))?
                .write_all(body.as_bytes());
            if write_result.is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(tracker_error("GitHub CLI could not receive the issue body"));
            }
        }
        let output = child
            .wait_with_output()
            .map_err(|_| tracker_error("GitHub CLI did not complete"))?;
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
        repo.to_string(),
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
        repo.to_string(),
        "--body-file".to_string(),
        "-".to_string(),
    ]
}

fn create_args(repo: &str, title: &str) -> Vec<String> {
    vec![
        "issue".to_string(),
        "create".to_string(),
        "--repo".to_string(),
        repo.to_string(),
        "--title".to_string(),
        title.to_string(),
        "--body-file".to_string(),
        "-".to_string(),
    ]
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
                "dcc-mcp/dcc-mcp-godot",
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
                "dcc-mcp/dcc-mcp-godot",
                "--title",
                "agent report: startup failed",
                "--body-file",
                "-"
            ]
        );
    }
}
