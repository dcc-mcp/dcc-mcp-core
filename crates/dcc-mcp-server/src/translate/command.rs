//! Quote-aware tokenization for directly spawned stdio commands.
//!
//! This is not a shell: comments, expansions and operators remain literal.

/// Parse a command into a program and arguments without shell interpretation.
///
/// Unquoted Unicode whitespace separates arguments, just as `split_whitespace`
/// did. Single/double quotes group text, including empty arguments. Outside
/// quotes, a backslash can escape whitespace or a quote; inside double quotes,
/// it can escape a double quote. Other backslashes and `#` stay literal.
///
/// Windows drive, rooted and relative paths (also after `=`) keep backslashes,
/// including trailing separators before whitespace or a closing double quote.
/// A bare `\` stays a root path; quote arguments with leading spaces. Use quotes for
/// spaces in these paths rather than backslash escapes.
pub(super) fn parse_command(cmd: &str) -> anyhow::Result<(String, Vec<String>)> {
    let mut parts = Vec::new();
    let mut token = String::new();
    let mut started = false;
    let mut quote = None;
    let mut chars = cmd.chars().peekable();

    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => token.push(ch),
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, ch) if ch.is_whitespace() => {
                if started {
                    parts.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            (_, '\\') => {
                // Drive-qualified and rooted paths use backslashes as path
                // separators, even when a quoted segment occurs in the token.
                let windows_path = std::iter::once(token.as_str())
                    .chain(token.split_once('=').map(|(_, value)| value))
                    .any(|path| {
                        path.starts_with('\\')
                            || path == "."
                            || path == ".."
                            || path.starts_with(".\\")
                            || path.starts_with("..\\")
                            || (path.as_bytes().get(1) == Some(&b':')
                                && path.as_bytes()[0].is_ascii_alphabetic())
                            || (path.is_empty()
                                && chars.peek().is_some_and(|next| {
                                    *next == '\\' || (quote.is_none() && next.is_whitespace())
                                }))
                    });
                if chars.peek().is_some_and(|next| {
                    (quote.is_none() && *next == '\'')
                        || (!windows_path
                            && ((quote.is_none() && next.is_whitespace()) || *next == '"'))
                }) {
                    if let Some(escaped) = chars.next() {
                        token.push(escaped);
                    }
                } else {
                    token.push(ch);
                }
                started = true;
            }
            _ => {
                token.push(ch);
                started = true;
            }
        }
    }

    anyhow::ensure!(
        quote.is_none(),
        "could not split stdio command into program + args: unterminated quote"
    );
    if started {
        parts.push(token);
    }
    let mut parts = parts.into_iter();
    Ok((parts.next().unwrap_or_default(), parts.collect()))
}

/// Always quote generated test arguments, including non-ASCII whitespace.
#[cfg(test)]
pub(super) fn quote_argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Split `cmd` the way the pre-change `parse_command` did, for comparison.
    ///
    /// Kept as a local oracle so the regression test below stays readable and
    /// keeps asserting against the historic behaviour rather than against a
    /// snapshot of whatever the current implementation happens to return.
    fn legacy_split(cmd: &str) -> (String, Vec<String>) {
        let mut parts = cmd.split_whitespace();
        let program = parts.next().unwrap_or_default().to_string();
        let args = parts.map(String::from).collect();
        (program, args)
    }

    /// `parse_command` must keep producing the historical `split_whitespace()`
    /// result for plain inputs, including Windows paths and shell metacharacters.
    #[test]
    fn parse_command_matches_plain_whitespace_splitting() {
        let plain = [
            "uvx mcp-server-git",
            "npx @modelcontextprotocol/server-filesystem /tmp",
            "python -u /srv/bridge.py",
            "",
            "   ",
            "single",
            "a\tb\nc  d",
            "python\u{00a0}-u\u{2003}bridge.py\u{3000}tail",
            r"C:\Python311\python.exe -u C:\scripts\bridge.py",
            r"python \\server\share\bridge.py C:\cache\ tail",
            r"python C:\cache\",
            r"python --output=C:\cache\ next",
            r"python --output=\\server\share\ next",
            r"python .\cache\ ..\cache\ next",
            r"python .\ ..\ next",
            r"python C:\cache=old\ --output=C:\cache=old\ next",
            r"python \ next --output=\ next",
            "python #tag keep # another",
            "#executable arg",
            "python $HOME ~/*.py | tail ; echo",
            "cmd --flag=value --other -x",
            "/srv/bridge.py --port 0 --host 127.0.0.1",
        ];
        for cmd in plain {
            assert_eq!(
                parse_command(cmd).expect("plain command must split"),
                legacy_split(cmd),
                "plain command mismatch for {cmd:?}"
            );
        }
    }

    #[test]
    fn parse_command_splits_quoted_program_with_spaces() {
        let (program, args) = parse_command(r#""C:\Program Files\Python311\python.exe" -c foo"#)
            .expect("balanced double quotes must split");
        assert_eq!(program, r"C:\Program Files\Python311\python.exe");
        assert_eq!(args, vec!["-c".to_string(), "foo".to_string()]);
    }

    #[test]
    fn parse_command_keeps_single_quoted_paths_intact() {
        let (program, args) = parse_command(r"'/opt/my tools/python3' '/srv/a b/bridge.py'")
            .expect("balanced single quotes must split");
        assert_eq!(program, "/opt/my tools/python3");
        assert_eq!(args, vec!["/srv/a b/bridge.py".to_string()]);
    }

    /// An unquoted space still separates tokens: quoting is opt-in, and a
    /// command that leaves a path bare splits exactly as it always has.
    #[test]
    fn parse_command_still_splits_unquoted_spaces() {
        let (program, args) = parse_command(r"'/opt/my tools/python3' /srv/a b/bridge.py")
            .expect("balanced single quotes must split");
        assert_eq!(program, "/opt/my tools/python3");
        assert_eq!(args, vec!["/srv/a".to_string(), "b/bridge.py".to_string()]);
    }

    #[test]
    fn parse_command_honours_backslash_escaped_spaces() {
        let (program, args) = parse_command(r"/opt/my\ tools/python3 one\ two")
            .expect("backslash escapes must split");
        assert_eq!(program, "/opt/my tools/python3");
        assert_eq!(args, vec!["one two".to_string()]);
    }

    /// `''` and `""` are preserved as empty arguments rather than dropped,
    /// matching how a shell forwards an explicitly empty argument.
    #[test]
    fn parse_command_preserves_empty_quoted_segments() {
        let (program, args) =
            parse_command("python '' \"\" tail").expect("empty quoted segments must split");
        assert_eq!(program, "python");
        assert_eq!(args, vec![String::new(), String::new(), "tail".to_string()]);
    }

    /// A command with an unterminated quote is an error instead of a silently
    /// truncated spawn.
    #[test]
    fn parse_command_rejects_unterminated_quote() {
        assert!(parse_command(r#""C:\Program Files\python.exe -c foo"#).is_err());
        assert!(parse_command("python 'unterminated").is_err());
    }

    #[test]
    fn parse_command_yields_one_token_for_quoted_space_path() {
        let (program, args) = parse_command(r#""C:\Program Files\Python3xx\python.exe" -c foo"#)
            .expect("balanced double quotes must split");
        assert_eq!(program, r"C:\Program Files\Python3xx\python.exe");
        assert!(
            program.contains(' '),
            "the quoted path must survive splitting with its space intact"
        );
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn parse_command_preserves_mixed_windows_paths_and_hash_arguments() {
        for (cmd, program, args) in [
            (
                r#""C:\Program Files\Python311\python.exe" -u C:\scripts\bridge.py #tag "two words""#,
                r"C:\Program Files\Python311\python.exe",
                vec!["-u", r"C:\scripts\bridge.py", "#tag", "two words"],
            ),
            (
                r#"C:\Python311\python.exe "C:\scene files\bridge.py" #tag tail"#,
                r"C:\Python311\python.exe",
                vec![r"C:\scene files\bridge.py", "#tag", "tail"],
            ),
            (
                r##""\\server\shared tools\python.exe" \\server\scripts\bridge.py "#tag" # keep"##,
                r"\\server\shared tools\python.exe",
                vec![r"\\server\scripts\bridge.py", "#tag", "#", "keep"],
            ),
        ] {
            assert_eq!(
                parse_command(cmd).expect("mixed paths must split"),
                (
                    program.to_string(),
                    args.into_iter().map(String::from).collect()
                ),
                "command mismatch for {cmd:?}"
            );
        }
    }

    #[test]
    fn parse_command_preserves_windows_trailing_directory_separators() {
        let (_, args) =
            parse_command(r#"python C:\cache\ "C:\cache folder\" \\server\share\ tail"#)
                .expect("trailing path separators must stay literal");
        assert_eq!(
            args,
            [
                r"C:\cache\",
                r"C:\cache folder\",
                r"\\server\share\",
                "tail"
            ]
        );
    }

    #[test]
    fn parse_command_handles_unicode_whitespace_with_quoted_and_escaped_text() {
        let (_, args) =
            parse_command("python\u{2003}\"one\u{2003}two\"\u{00a0}three\\\u{3000}four")
                .expect("Unicode whitespace must follow the same quoting rules");
        assert_eq!(args, ["one\u{2003}two", "three\u{3000}four"]);
    }

    #[test]
    fn parse_command_concatenates_segments_and_preserves_escaped_quotes() {
        let (_, args) =
            parse_command(r#"python pre" middle "'post' say\"hi\" 'it'\''s' "say \"hi\"""#)
                .expect("adjacent segments and escaped quotes must split");
        assert_eq!(args, ["pre middle post", "say\"hi\"", "it's", "say \"hi\""]);
    }

    #[test]
    fn parse_command_preserves_literal_backslashes_in_quoted_arguments() {
        let (_, args) =
            parse_command(r#"python "one\ two" 'one\ two' "--output=C:\cache folder\" "C:\cache=old\" "--output=C:\cache=old\" next"#)
                .expect("quoted backslashes and option paths must split");
        assert_eq!(
            args,
            [
                r"one\ two",
                r"one\ two",
                r"--output=C:\cache folder\",
                r"C:\cache=old\",
                r"--output=C:\cache=old\",
                "next"
            ]
        );
    }

    #[test]
    fn parse_command_round_trips_generated_subprocess_paths() {
        // The correlation tests build command lines with quote_argument.
        // Include Windows apostrophes and trailing separators in that contract.
        let values = [
            "",
            "#tag",
            "one two",
            "it\'s",
            r"C:\Python311\python.exe",
            r"C:\Users\Artist's Files\",
            r"\\server\shared tools\python.exe",
            r#"a 'single' and "double" quote"#,
            "line\nnext",
            "one\u{2003}two",
        ];
        for value in values {
            let cmd = format!("python {}", quote_argument(value));
            let (_, args) = parse_command(&cmd).expect("quoted values must split");
            assert_eq!(args, [value], "round-trip mismatch for {cmd:?}");
        }
    }
}
