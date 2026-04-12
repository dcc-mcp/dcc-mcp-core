//! Script execution helpers for the skill catalog.
//!
//! Provides subprocess-based script dispatch used by [`SkillCatalog::load_skill`]
//! when auto-registering handlers in the Skills-First workflow.

use dcc_mcp_models::ToolDeclaration;

/// Resolve which script file backs a tool declaration.
///
/// Priority:
/// 1. `tool_decl.source_file` — explicit path set in ToolDeclaration
/// 2. A script whose stem matches the tool name in the skill's scripts list
/// 3. The only script in the skill (if exactly one exists)
pub(crate) fn resolve_tool_script(
    tool_decl: &ToolDeclaration,
    scripts: &[String],
    skill_path: &std::path::Path,
) -> Option<String> {
    // 1. Explicit source_file on the tool declaration
    if !tool_decl.source_file.is_empty() {
        let p = std::path::Path::new(&tool_decl.source_file);
        // If relative, resolve against the skill root directory so that
        // the subprocess always receives an absolute path regardless of CWD.
        if p.is_relative() {
            let abs = skill_path.join(p);
            return Some(abs.to_string_lossy().into_owned());
        }
        return Some(tool_decl.source_file.clone());
    }

    // Extract bare tool name (after __ if present)
    let tool_name = if tool_decl.name.contains("__") {
        tool_decl.name.split("__").last().unwrap_or(&tool_decl.name)
    } else {
        &tool_decl.name
    };
    let tool_name_lower = tool_name.to_lowercase().replace('-', "_");

    // 2. Script whose stem matches the tool name
    for script in scripts {
        let stem = std::path::Path::new(script)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .replace('-', "_");
        if stem == tool_name_lower {
            // Resolve against skill_path if relative
            let p = std::path::Path::new(script);
            if p.is_relative() {
                let abs = skill_path.join(p);
                return Some(abs.to_string_lossy().into_owned());
            }
            return Some(script.clone());
        }
    }

    // 3. Single-script skill — the one script backs all tools
    if scripts.len() == 1 {
        let p = std::path::Path::new(&scripts[0]);
        if p.is_relative() {
            let abs = skill_path.join(p);
            return Some(abs.to_string_lossy().into_owned());
        }
        return Some(scripts[0].clone());
    }

    None
}

/// Execute a skill script as a subprocess, passing params as JSON via stdin.
///
/// The script is expected to:
/// - Read JSON params from stdin (or use sys.argv for simple cases)
/// - Write a JSON result to stdout
/// - Exit with code 0 on success, non-zero on failure
///
/// Returns `Ok(Value)` on success, `Err(String)` on failure.
pub(crate) fn execute_script(
    script_path: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let params_json = serde_json::to_string(&params).unwrap_or_else(|_| "{}".to_string());

    let path = std::path::Path::new(script_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Resolve the Python interpreter:
    // 1. DCC_MCP_PYTHON_EXECUTABLE env var (explicit override, e.g. mayapy)
    // 2. Fall back to the Python that shipped the `python` command on PATH
    let python_exe =
        std::env::var("DCC_MCP_PYTHON_EXECUTABLE").unwrap_or_else(|_| "python".to_string());

    // Optional: prepend a Python init snippet before running the skill script.
    // DCC_MCP_PYTHON_INIT_SNIPPET can contain a one-liner (semicolon separated)
    // to run before the script, e.g. "import maya.standalone; maya.standalone.initialize(name='python')"
    let init_snippet = std::env::var("DCC_MCP_PYTHON_INIT_SNIPPET").ok();

    // Choose interpreter based on extension
    let (program, args): (String, Vec<String>) = match ext.as_str() {
        "py" => {
            if let Some(ref snippet) = init_snippet {
                // Wrap: python -c "exec(open(...).read())" with init prepended
                let wrapper = format!(
                    "exec(compile(open(r'{path}','r').read(), r'{path}', 'exec'), {{'__file__': r'{path}', '__name__': '__main__'}})",
                    path = script_path
                );
                let code = format!("{}; {}", snippet, wrapper);
                (python_exe, vec!["-c".to_string(), code])
            } else {
                (python_exe, vec![script_path.to_string()])
            }
        }
        "sh" | "bash" => ("bash".to_string(), vec![script_path.to_string()]),
        "bat" | "cmd" => (
            "cmd".to_string(),
            vec!["/C".to_string(), script_path.to_string()],
        ),
        "mel" | "lua" | "hscript" | "maxscript" => (python_exe, vec![script_path.to_string()]),
        _ => (python_exe, vec![script_path.to_string()]),
    };

    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn '{script_path}': {e}"))?;

    // Write params to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(params_json.as_bytes());
        // stdin closes when dropped, signalling EOF to the script
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Script '{script_path}' execution failed: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let detail = if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(format!(
            "Script '{script_path}' exited with code {code}: {detail}"
        ));
    }

    // Try to parse stdout as JSON; fall back to plain text result
    let result_str = stdout.trim();
    if result_str.is_empty() {
        return Ok(serde_json::json!({"success": true, "message": ""}));
    }

    match serde_json::from_str::<serde_json::Value>(result_str) {
        Ok(v) => Ok(v),
        Err(_) => {
            // Plain text output — wrap it
            Ok(serde_json::json!({"success": true, "message": result_str}))
        }
    }
}
