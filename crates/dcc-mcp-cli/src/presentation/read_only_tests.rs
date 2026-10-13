use super::*;

fn parse(arguments: &[&str]) -> Result<Option<(ReadOnlyCommand, OutputFormat)>, clap::Error> {
    read_only_args(arguments.iter().map(OsString::from))
}

#[test]
fn canonical_parser_accepts_global_options_before_and_after_inspection() {
    for arguments in [
        vec![
            "dcc-mcp-cli",
            "--output",
            "json",
            "--base-url",
            "http://127.0.0.1:1",
            "update",
            "inspect",
            "--offline",
        ],
        vec![
            "dcc-mcp-cli",
            "update",
            "inspect",
            "--offline",
            "--output",
            "json",
            "--base-url",
            "http://127.0.0.1:1",
        ],
    ] {
        let (args, output) = parse(&arguments).unwrap().unwrap();
        let ReadOnlyCommand::Update(args) = args else {
            panic!("expected update inspection");
        };
        assert!(args.offline);
        assert_eq!(args.channel, "stable");
        assert_eq!(output, OutputFormat::Json);
    }
}

#[test]
fn malformed_inspection_and_help_never_return_legacy_fallback() {
    for arguments in [
        vec!["dcc-mcp-cli", "update", "inspect", "--unknown"],
        vec!["dcc-mcp-cli", "update", "inspect", "--channel"],
        vec![
            "dcc-mcp-cli",
            "update",
            "inspect",
            "--issue",
            "https://github.com/other/repo/issues/1",
        ],
        vec!["dcc-mcp-cli", "--output", "invalid", "update", "inspect"],
    ] {
        assert_eq!(parse(&arguments).unwrap_err().exit_code(), 2);
    }
    for arguments in [
        vec!["dcc-mcp-cli", "update", "inspect", "--help"],
        vec!["dcc-mcp-cli", "--help"],
        vec!["dcc-mcp-cli", "--version"],
    ] {
        assert_eq!(parse(&arguments).unwrap_err().exit_code(), 0);
    }
}

#[test]
fn only_valid_other_commands_return_legacy_fallback() {
    assert!(
        parse(&["dcc-mcp-cli", "update", "check"])
            .unwrap()
            .is_none()
    );
    assert!(
        parse(&["dcc-mcp-cli", "update", "apply", "--yes"])
            .unwrap()
            .is_none()
    );
    assert!(parse(&["dcc-mcp-cli", "health"]).unwrap().is_none());
    assert!(parse(&["dcc-mcp-cli", "health", "--invalid"]).is_err());
}

#[test]
fn feedback_previews_use_read_only_gate_and_canonical_global_options() {
    for arguments in [
        vec![
            "dcc-mcp-cli",
            "--require-gateway",
            "feedback",
            "bundle",
            "finding.json",
            "--json",
        ],
        vec![
            "dcc-mcp-cli",
            "feedback",
            "bundle",
            "finding.json",
            "--output",
            "json",
        ],
    ] {
        let (command, output) = parse(&arguments).unwrap().unwrap();
        assert!(matches!(command, ReadOnlyCommand::Bundle(_)));
        assert_eq!(output, OutputFormat::Json);
    }
    let (command, output) = parse(&["dcc-mcp-cli", "feedback", "file", "finding.json", "--json"])
        .unwrap()
        .unwrap();
    assert!(matches!(command, ReadOnlyCommand::FeedbackPlan(_)));
    assert_eq!(output, OutputFormat::Json);
    for arguments in [
        vec![
            "dcc-mcp-cli",
            "feedback",
            "file",
            "finding.json",
            "--yes",
            "--create",
        ],
        vec![
            "dcc-mcp-cli",
            "feedback",
            "bundle",
            "finding.json",
            "--unknown",
        ],
    ] {
        assert_eq!(parse(&arguments).unwrap_err().exit_code(), 2);
    }
    assert_eq!(
        parse(&["dcc-mcp-cli", "feedback", "bundle", "--help"])
            .unwrap_err()
            .exit_code(),
        0
    );
}
