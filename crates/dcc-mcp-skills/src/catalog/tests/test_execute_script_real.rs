//! End-to-end tests for the subprocess script-execution path.
//!
//! Unlike the smoke tests in `test_execute_script.rs`, these tests actually
//! invoke a real interpreter, write a real script that processes a real file,
//! and then assert the file was processed correctly. This is the production
//! path that DCC adapters hit when no in-process executor is registered, and
//! the locus of historical breakage (issues #231, dcc-mcp-maya#137/#138).
//!
//! Tests pass `skill_dcc=None` so the host-python check is bypassed and the
//! ambient `python` on PATH is used. Per-language tests are gated on the
//! interpreter being resolvable so CI on either OS can run the rest of the
//! matrix even when one interpreter is missing.

use super::*;
use std::path::PathBuf;
use tempfile::TempDir;

// ── helpers ────────────────────────────────────────────────────────────────

/// `true` when the current process is being ptraced (cargo tarpaulin
/// coverage, valgrind, strace, debugger). Detection only attempts the Linux
/// `/proc/self/status` `TracerPid` field — the only platform where tarpaulin
/// runs in CI. On macOS / Windows the function always returns `false`, which
/// is correct because tarpaulin does not run there. Tarpaulin's ptrace
/// instrumentation interferes with the `Stdio::piped()` / `wait_with_output()`
/// pattern these tests rely on, so when ptraced we skip rather than chase
/// ptrace artefacts in coverage; the non-coverage `cargo test` runs on all
/// three platforms still exercise the same dispatcher code paths.
///
/// Replaces the previous CARGO_TARPAULIN env-var check, which the current
/// cargo-tarpaulin release does not set when invoked via taiki-e/install-action.
fn under_ptrace() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if let Some(rest) = line.strip_prefix("TracerPid:") {
                    return rest.trim().parse::<u32>().map(|p| p != 0).unwrap_or(false);
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// `true` when an executable named `program` is resolvable on PATH (with the
/// usual Windows PATHEXT extensions). Used to skip per-language tests when
/// the interpreter is not installed.
fn have_program(program: &str) -> bool {
    let path_var = match std::env::var_os("PATH") {
        Some(v) => v,
        None => return false,
    };
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path_var) {
        for ext in &exts {
            if dir.join(format!("{program}{ext}")).is_file() {
                return true;
            }
        }
    }
    false
}

/// Materialise `body` as `name` inside a fresh tempdir and return both.
fn write_script(name: &str, body: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join(name);
    std::fs::write(&path, body).expect("write script");
    (dir, path)
}

/// Combined precondition guard for every test in this module: skip when the
/// requested interpreter is missing OR when the test binary is being ptraced
/// (which historically interferes with subprocess Stdio pipes — see
/// `under_ptrace` above). Returns `true` when the test should be skipped.
fn skip_real_exec(interpreter: &str) -> bool {
    under_ptrace() || !have_program(interpreter)
}

// ── Python (.py) — stdin / CLI / file processing ───────────────────────────

const PY_COPY_VIA_STDIN: &str = r#"
import json, sys, pathlib
params = json.loads(sys.stdin.read() or "{}")
src = pathlib.Path(params["input"])
dst = pathlib.Path(params["output"])
dst.write_text(src.read_text(encoding="utf-8") + params.get("suffix", ""), encoding="utf-8")
print(json.dumps({"success": True, "bytes": dst.stat().st_size}))
"#;

#[test]
fn test_real_python_processes_file_via_stdin_params() {
    if skip_real_exec("python") {
        return;
    }
    let (_dir, script) = write_script("copy.py", PY_COPY_VIA_STDIN);
    let work = TempDir::new().unwrap();
    let input = work.path().join("in.txt");
    let output = work.path().join("out.txt");
    std::fs::write(&input, "skill payload").unwrap();

    let result = execute_script(
        script.to_str().unwrap(),
        serde_json::json!({
            "input": input.to_string_lossy(),
            "output": output.to_string_lossy(),
            "suffix": "!",
        }),
        None,
    )
    .expect("execute_script must succeed when the script is well-formed");

    assert_eq!(result["success"], true, "result envelope: {result:?}");
    assert!(output.is_file(), "script must have written the output file");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "skill payload!");
}

const PY_COPY_VIA_ARGPARSE: &str = r#"
import argparse, json, pathlib, sys
# Drain stdin so the writer doesn't block on a SIGPIPE; the script
# intentionally ignores it to exercise the argparse-only path.
sys.stdin.read()
ap = argparse.ArgumentParser()
ap.add_argument("--input", required=True)
ap.add_argument("--output", required=True)
ap.add_argument("--upper", default="false")
args = ap.parse_args()
src = pathlib.Path(args.input).read_text(encoding="utf-8")
out = src.upper() if args.upper.lower() == "true" else src
pathlib.Path(args.output).write_text(out, encoding="utf-8")
print(json.dumps({"success": True, "len": len(out)}))
"#;

#[test]
fn test_real_python_processes_file_via_cli_argparse() {
    if skip_real_exec("python") {
        return;
    }
    let (_dir, script) = write_script("argparse_copy.py", PY_COPY_VIA_ARGPARSE);
    let work = TempDir::new().unwrap();
    let input = work.path().join("in.txt");
    let output = work.path().join("out.txt");
    std::fs::write(&input, "hello world").unwrap();

    let result = execute_script(
        script.to_str().unwrap(),
        serde_json::json!({
            "input": input.to_string_lossy(),
            "output": output.to_string_lossy(),
            "upper": "true",
        }),
        None,
    )
    .expect("argparse path must succeed");

    assert_eq!(result["success"], true, "envelope: {result:?}");
    assert_eq!(result["len"].as_u64(), Some(11));
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "HELLO WORLD");
}

#[test]
fn test_real_python_complex_params_only_via_stdin_not_cli() {
    if skip_real_exec("python") {
        return;
    }
    // Object/array values must NOT be expanded as `--key value` flags;
    // argparse with a bool-only schema would reject them, so use a script
    // that explicitly asserts argv is empty for those keys and reads them
    // from the stdin JSON payload.
    let body = r#"
import json, sys
argv = sys.argv[1:]
params = json.loads(sys.stdin.read() or "{}")
assert "--nested" not in argv, f"objects must not become CLI flags: {argv}"
assert "--items" not in argv, f"arrays must not become CLI flags: {argv}"
assert params["nested"] == {"a": 1}, params
assert params["items"] == [1, 2, 3], params
print(json.dumps({"success": True, "argv": argv}))
"#;
    let (_dir, script) = write_script("complex.py", body);
    let result = execute_script(
        script.to_str().unwrap(),
        serde_json::json!({"nested": {"a": 1}, "items": [1, 2, 3]}),
        None,
    )
    .expect("complex params should pass via stdin only");
    assert_eq!(result["success"], true, "envelope: {result:?}");
}

#[test]
fn test_real_python_nonzero_exit_returns_error_with_stderr() {
    if skip_real_exec("python") {
        return;
    }
    let body = r#"
import sys
sys.stderr.write("the script exploded: bad input\n")
sys.exit(7)
"#;
    let (_dir, script) = write_script("boom.py", body);
    let err = execute_script(script.to_str().unwrap(), serde_json::json!({}), None)
        .expect_err("non-zero exit must surface as Err(...)");
    assert!(err.contains("exited with code 7"), "got: {err}");
    assert!(
        err.contains("the script exploded"),
        "stderr must propagate: {err}"
    );
}

#[test]
fn test_real_python_plain_text_stdout_is_wrapped_as_message() {
    if skip_real_exec("python") {
        return;
    }
    let body = "print('not json, just text')\n";
    let (_dir, script) = write_script("plain.py", body);
    let result = execute_script(script.to_str().unwrap(), serde_json::json!({}), None)
        .expect("plain stdout must succeed");
    assert_eq!(result["success"], true);
    assert_eq!(result["message"], "not json, just text");
}

#[test]
fn test_real_python_empty_stdout_default_success() {
    if skip_real_exec("python") {
        return;
    }
    let body = "pass\n";
    let (_dir, script) = write_script("silent.py", body);
    let result = execute_script(script.to_str().unwrap(), serde_json::json!({}), None)
        .expect("empty stdout must succeed");
    assert_eq!(result["success"], true);
    assert_eq!(result["message"], "");
}

#[test]
fn test_real_python_unicode_params_round_trip_via_stdin() {
    if skip_real_exec("python") {
        return;
    }
    // The Rust dispatcher pins PYTHONIOENCODING=utf-8 and PYTHONUTF8=1 on
    // every Python child so this test does NOT need to wrap sys.stdin /
    // sys.stdout manually. If we ever regress that pinning, this test will
    // fail on Windows (cp1252 host) with classic mojibake — that is the
    // exact production trap we want to keep guarding against.
    let body = r#"
import json, sys
assert (sys.stdin.encoding or "").lower().replace("-", "") in ("utf8", ""), \
    f"stdin encoding must be UTF-8, got {sys.stdin.encoding!r}"
params = json.loads(sys.stdin.read() or "{}")
print(json.dumps({"success": True, "got": params["greeting"]}, ensure_ascii=False))
"#;
    let (_dir, script) = write_script("unicode.py", body);
    let greeting = "你好 🌟 Привет";
    let result = execute_script(
        script.to_str().unwrap(),
        serde_json::json!({"greeting": greeting}),
        None,
    )
    .expect("unicode round-trip");
    assert_eq!(result["success"], true);
    assert_eq!(result["got"].as_str(), Some(greeting));
}

#[test]
fn test_real_python_large_string_param_reaches_via_stdin_not_argv() {
    // Regression: passing a large string param (e.g. a serialised manifest, a
    // base64-encoded image) used to crash on Windows because every scalar
    // string was pushed onto argv as `--key value`, blowing the
    // CreateProcess 32 KiB command-line limit. The dispatcher now skips
    // CLI expansion for strings beyond MAX_CLI_FLAG_VALUE_BYTES (8 KiB)
    // and the value still reaches the script via the stdin JSON payload.
    if skip_real_exec("python") {
        return;
    }
    let body = r#"
import json, sys
params = json.loads(sys.stdin.read() or "{}")
blob = params["blob"]
# Oversized string params must NOT appear on argv, but MUST appear in stdin.
assert "--blob" not in sys.argv[1:], f"oversized string leaked to argv: {sys.argv[1:]!r}"
print(json.dumps({"success": True, "len": len(blob), "head": blob[:8]}))
"#;
    let (_dir, script) = write_script("big.py", body);
    let blob: String = "x".repeat(200_000);
    let result = execute_script(
        script.to_str().unwrap(),
        serde_json::json!({"blob": blob}),
        None,
    )
    .expect("large string param must reach the script via stdin without crashing the spawn");
    assert_eq!(result["len"].as_u64(), Some(200_000));
    assert_eq!(result["head"], "xxxxxxxx");
}

#[test]
fn test_real_python_short_string_param_reaches_via_argv() {
    // Counterpart to the oversized test above: short scalar strings must
    // still be expanded as `--key value` so argparse-based scripts keep
    // working. The threshold lives at MAX_CLI_FLAG_VALUE_BYTES (8 KiB).
    if skip_real_exec("python") {
        return;
    }
    let body = r#"
import json, sys
assert "--name" in sys.argv[1:], f"short string must arrive on argv: {sys.argv[1:]!r}"
i = sys.argv.index("--name")
print(json.dumps({"success": True, "from_argv": sys.argv[i + 1]}))
"#;
    let (_dir, script) = write_script("short.py", body);
    let result = execute_script(
        script.to_str().unwrap(),
        serde_json::json!({"name": "Alice"}),
        None,
    )
    .expect("short string param");
    assert_eq!(result["from_argv"], "Alice");
}

// ── Shell (.sh) — Linux/macOS only, requires bash on PATH ──────────────────

#[cfg(unix)]
#[test]
fn test_real_bash_processes_file_via_cli_flags() {
    if skip_real_exec("bash") {
        return;
    }
    // Bash receives the same scalar params as `--key value` flags. The
    // script reads its own argv (stdin JSON is ignored here on purpose) and
    // copies a file from --input to --output.
    let body = r#"#!/usr/bin/env bash
set -euo pipefail
# Drain stdin so the parent does not block on a closed pipe.
cat > /dev/null
input=""
output=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --input) input="$2"; shift 2 ;;
        --output) output="$2"; shift 2 ;;
        *) shift ;;
    esac
done
cp "$input" "$output"
printf '{"success": true, "wrote": "%s"}\n' "$output"
"#;
    let (_dir, script) = write_script("copy.sh", body);
    // Mark executable on Unix.
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(&script).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&script, perm).unwrap();

    let work = TempDir::new().unwrap();
    let input = work.path().join("in.txt");
    let output = work.path().join("out.txt");
    std::fs::write(&input, "bash payload").unwrap();

    let result = execute_script(
        script.to_str().unwrap(),
        serde_json::json!({
            "input": input.to_string_lossy(),
            "output": output.to_string_lossy(),
        }),
        None,
    )
    .expect("bash script must succeed");
    assert_eq!(result["success"], true, "envelope: {result:?}");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "bash payload");
}

#[cfg(unix)]
#[test]
fn test_real_bash_nonzero_exit_returns_error_with_stderr() {
    if skip_real_exec("bash") {
        return;
    }
    let body = r#"#!/usr/bin/env bash
echo 'bash failed deliberately' 1>&2
exit 3
"#;
    let (_dir, script) = write_script("fail.sh", body);
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(&script).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&script, perm).unwrap();

    let err = execute_script(script.to_str().unwrap(), serde_json::json!({}), None)
        .expect_err("non-zero bash exit must surface as Err(...)");
    assert!(err.contains("exited with code 3"), "got: {err}");
    assert!(
        err.contains("bash failed deliberately"),
        "stderr must propagate: {err}"
    );
}

// ── Batch (.bat) — Windows only ────────────────────────────────────────────

#[cfg(windows)]
#[test]
fn test_real_bat_processes_file_via_cli_flags() {
    // CMD batch script — copy --input to --output and emit JSON to stdout.
    // %1 / %2 / %3 / %4 are the four CLI args. Order matches how
    // `execute_script` serialises scalar params (sorted by HashMap iteration
    // is unstable, so we tolerate either ordering).
    let body = "@echo off\r\n\
        setlocal\r\n\
        set INPUT=\r\n\
        set OUTPUT=\r\n\
        :loop\r\n\
        if \"%~1\"==\"\" goto done\r\n\
        if /I \"%~1\"==\"--input\" ( set INPUT=%~2 & shift & shift & goto loop )\r\n\
        if /I \"%~1\"==\"--output\" ( set OUTPUT=%~2 & shift & shift & goto loop )\r\n\
        shift\r\n\
        goto loop\r\n\
        :done\r\n\
        copy /Y \"%INPUT%\" \"%OUTPUT%\" >nul\r\n\
        echo {\"success\": true, \"wrote\": \"%OUTPUT:\\=/%\"}\r\n";
    let (_dir, script) = write_script("copy.bat", body);
    let work = TempDir::new().unwrap();
    let input = work.path().join("in.txt");
    let output = work.path().join("out.txt");
    std::fs::write(&input, "bat payload").unwrap();

    let result = execute_script(
        script.to_str().unwrap(),
        serde_json::json!({
            "input": input.to_string_lossy(),
            "output": output.to_string_lossy(),
        }),
        None,
    )
    .expect("bat script must succeed");
    assert_eq!(result["success"], true, "envelope: {result:?}");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "bat payload");
}

// ── PowerShell (.ps1) — Windows only ───────────────────────────────────────

#[cfg(windows)]
#[test]
fn test_real_powershell_processes_file_via_cli_flags() {
    if !have_program("pwsh") && !have_program("powershell") {
        return;
    }
    // PowerShell receives `--input <p>` etc as positional args; PowerShell
    // treats `--input` as a literal token (not a parameter name) so we walk
    // $args manually rather than using `param()`.
    let body = "$ErrorActionPreference = 'Stop'\r\n\
        # Drain stdin so the parent does not block on a closed pipe.\r\n\
        $null = [Console]::In.ReadToEnd()\r\n\
        $inputPath = $null\r\n\
        $outputPath = $null\r\n\
        for ($i = 0; $i -lt $args.Count; $i++) {\r\n\
            switch ($args[$i]) {\r\n\
                '--input' { $inputPath = $args[$i + 1]; $i++ }\r\n\
                '--output' { $outputPath = $args[$i + 1]; $i++ }\r\n\
            }\r\n\
        }\r\n\
        Copy-Item -LiteralPath $inputPath -Destination $outputPath -Force\r\n\
        $obj = @{ success = $true; wrote = $outputPath }\r\n\
        Write-Output ($obj | ConvertTo-Json -Compress)\r\n";
    let (_dir, script) = write_script("copy.ps1", body);
    let work = TempDir::new().unwrap();
    let input = work.path().join("in.txt");
    let output = work.path().join("out.txt");
    std::fs::write(&input, "ps1 payload").unwrap();

    let result = execute_script(
        script.to_str().unwrap(),
        serde_json::json!({
            "input": input.to_string_lossy(),
            "output": output.to_string_lossy(),
        }),
        None,
    )
    .expect("ps1 script must succeed");
    assert_eq!(result["success"], true, "envelope: {result:?}");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "ps1 payload");
}

#[cfg(windows)]
#[test]
fn test_real_powershell_nonzero_exit_returns_error_with_stderr() {
    if !have_program("pwsh") && !have_program("powershell") {
        return;
    }
    // `throw` produces a non-zero exit and writes the message to stderr.
    let body = "[Console]::Error.WriteLine('powershell exploded')\r\nexit 5\r\n";
    let (_dir, script) = write_script("fail.ps1", body);
    let err = execute_script(script.to_str().unwrap(), serde_json::json!({}), None)
        .expect_err("non-zero ps1 exit must surface as Err(...)");
    assert!(err.contains("exited with code 5"), "got: {err}");
    assert!(
        err.contains("powershell exploded"),
        "stderr must propagate: {err}"
    );
}
