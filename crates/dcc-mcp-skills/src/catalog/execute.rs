//! Script execution helpers for the skill catalog.
//!
//! Provides two execution paths used by [`SkillCatalog::load_skill`]:
//!
//! 1. **In-process** (preferred when running inside a DCC application):
//!    The script is executed directly inside the current Python interpreter
//!    via PyO3.  This is correct for Maya, Blender, Houdini, etc. because
//!    the host DCC already provides its own interpreter with all DCC modules
//!    available (`maya.cmds`, `bpy`, `hou`, …).  Spawning a subprocess in
//!    that scenario would start a *second* interpreter (or a whole new DCC
//!    instance when `DCC_MCP_PYTHON_EXECUTABLE` is set to `mayapy`), which
//!    has no access to the live scene.
//!
//! 2. **Subprocess** (fallback for standalone / non-DCC environments):
//!    The original behaviour — spawn `python` (or the executable named by
//!    `DCC_MCP_PYTHON_EXECUTABLE`) as a child process and communicate via
//!    stdin/stdout JSON.  Still required when dcc-mcp-core runs outside of
//!    a DCC process (e.g. a standalone gateway, test harness, or a DCC that
//!    has no embedded Python).
//!
//! The catalog switches between these paths automatically: if a
//! [`ScriptExecutorFn`] has been registered (via
//! [`SkillCatalog::with_in_process_executor`]) it is used; otherwise the
//! subprocess path is taken.

use dcc_mcp_models::ToolDeclaration;

/// A pluggable script executor that runs a skill script inside the **current**
/// process rather than spawning a child process.
///
/// DCC adapters (Maya, Blender, Houdini…) should register one of these via
/// [`SkillCatalog::with_in_process_executor`] so that skill scripts are
/// executed inside the host DCC's own Python interpreter instead of being
/// dispatched to a subprocess.
///
/// The closure receives:
/// - `script_path` — absolute path to the `.py` script to execute.
/// - `params`      — the tool's input parameters as a `serde_json::Value`.
///
/// It must return `Ok(Value)` on success or `Err(String)` on failure.
pub type ScriptExecutorFn =
    dyn Fn(String, serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync;

/// Execute a skill script **in-process** using PyO3.
///
/// This is the preferred execution path when dcc-mcp-core is embedded inside
/// a DCC application (Maya, Blender, Houdini, …).  The script is loaded via
/// `importlib.util` inside the *current* Python interpreter — the one already
/// running inside the DCC — so all host modules (`maya.cmds`, `bpy`, `hou`, …)
/// are available without spawning any external process.
///
/// The script receives the input parameters via a `__mcp_params__` global dict
/// so it can access them with `params = globals().get("__mcp_params__", {})`.
/// The script is expected to set a `__mcp_result__` module-level variable to a
/// JSON-serialisable dict before returning.
///
/// # Fallback
/// If the `python-bindings` Cargo feature is not enabled (i.e. PyO3 is not
/// available) this function is not compiled and the catalog falls back to the
/// subprocess path automatically.
#[cfg(feature = "python-bindings")]
#[allow(dead_code)] // Available for DCC adapters that invoke it directly
pub(crate) fn execute_script_in_process(
    script_path: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use dcc_mcp_utils::py_json::{json_value_to_pyobject, py_any_to_json_value};
    use pyo3::prelude::*;
    use pyo3::types::PyDict;

    Python::try_attach(|py| {
        // Build a glue script that loads the target via importlib.util,
        // injects __mcp_params__, executes the module, and captures
        // __mcp_result__ — all in a single `eval`-friendly expression.
        //
        // We cannot use `PyModule::from_code` here because its signature
        // requires `&CStr` literals which cannot hold runtime strings in
        // PyO3 0.28.  Using `py.run(CStr)` has the same limitation.
        // Instead we delegate fully to Python's own importlib so that the
        // script has a proper `__spec__` and the DCC's import hooks fire.
        let params_obj =
            json_value_to_pyobject(py, &params).map_err(|e| format!("params → Python: {e}"))?;

        let glue = PyDict::new(py);
        glue.set_item("_script_path", script_path)
            .map_err(|e| format!("PyO3 glue dict: {e}"))?;
        glue.set_item("_params", params_obj)
            .map_err(|e| format!("PyO3 glue dict: {e}"))?;

        // Execute via Python eval — the code string is built at runtime so
        // there is no need for `c_str!` macros.
        let run_code = r#"
import importlib.util as _ilu, types as _types
_spec = _ilu.spec_from_file_location("__mcp_skill__", _script_path)
_mod = _ilu.module_from_spec(_spec)
_mod.__mcp_params__ = _params
_spec.loader.exec_module(_mod)
getattr(_mod, "__mcp_result__", {"success": True, "message": ""})
"#;

        // `py.eval_bound` accepts a `&str` in PyO3 0.28
        let result_obj = py
            .eval(
                pyo3::ffi::c_str!(
                    r#"
import importlib.util as _ilu
_spec = _ilu.spec_from_file_location("__mcp_skill__", _script_path)
_mod = _ilu.module_from_spec(_spec)
_mod.__mcp_params__ = _params
_spec.loader.exec_module(_mod)
getattr(_mod, "__mcp_result__", {"success": True, "message": ""})
"#
                ),
                None,
                Some(&glue),
            )
            .map_err(|e| format!("in-process script '{script_path}' failed: {e}"))?;

        let _ = run_code; // silence unused warning
        py_any_to_json_value(&result_obj).map_err(|e| format!("result → JSON: {e}"))
    })
    .ok_or_else(|| {
        // Distinguish between "Python not initialized at all" and
        // "initialized but GIL not held / wrong thread".
        let initialized = unsafe { pyo3::ffi::Py_IsInitialized() } != 0;
        if initialized {
            format!(
                "Python interpreter is initialized but the GIL is not held; \
                 cannot execute '{script_path}' in-process. \
                 Hint: ensure you are calling this from the main Python thread \
                 or that the in-process executor is registered via \
                 SkillCatalog::with_in_process_executor."
            )
        } else {
            format!(
                "Python interpreter is not initialized in this process; \
                 cannot execute '{script_path}' in-process. \
                 Hint: when running inside a DCC, register an in-process executor \
                 via SkillCatalog::with_in_process_executor."
            )
        }
    })
    .and_then(|r| r)
}

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

/// Execute a skill script as a subprocess.
///
/// Parameters are passed in **two complementary ways** so that scripts can use
/// whichever convention they prefer:
///
/// 1. **stdin (preferred)** — the full params JSON object is written to the
///    child's stdin.  Scripts read it with `json.load(sys.stdin)`.
///
/// 2. **CLI flags (argparse-compatible)** — each top-level string/number/bool
///    key in `params` is also appended as `--<key> <value>` so that scripts
///    that use `argparse` work without any modification.
///
/// The script is expected to write a JSON result to stdout and exit 0 on
/// success, or exit non-zero on failure (stderr is captured for the error
/// message).
///
/// Returns `Ok(Value)` on success, `Err(String)` on failure.
/// `dcc` values that require a DCC-specific Python interpreter (mayapy, blender --python,
/// hython, 3dsmaxpy…). When a skill declares one of these and neither
/// `DCC_MCP_PYTHON_EXECUTABLE` nor `DCC_MCP_PYTHON_INIT_SNIPPET` is exported, the
/// worker would silently fall back to the ambient `python` on PATH — where
/// `import maya.cmds` either fails outright or resolves to an unusable stub.
/// Returning a structured error in that case is far better than the previous
/// behaviour where commands like `cmds.polySphere(...)` raised
/// `AttributeError` mid-skill (see issue #231).
const DCC_NAMES_REQUIRING_HOST_PYTHON: &[&str] = &[
    "maya",
    "blender",
    "houdini",
    "3dsmax",
    "max",
    "nuke",
    "katana",
    "cinema4d",
    "c4d",
    "modo",
    "motionbuilder",
];

/// Known DCC GUI executable names (case-insensitive).  Used to detect
/// when `DCC_MCP_PYTHON_EXECUTABLE` has been mistakenly pointed at a
/// GUI binary instead of the headless interpreter.
const DCC_GUI_EXECUTABLES: &[&str] = &[
    "maya",
    "maya.exe",
    "maya.bin",
    "blender",
    "blender.exe",
    "houdini",
    "houdini.exe",
    "houdinifx",
    "houdinicore",
    "3dsmax",
    "3dsmax.exe",
    "nuke",
    "nuke.exe",
    "nukestudio",
    "modo",
    "modo.exe",
    "motionbuilder",
    "motionbuilder.exe",
    "cinema4d",
    "cinema4d.exe",
    "c4d",
    "katana",
    "katana.exe",
];

pub(crate) fn execute_script(
    script_path: &str,
    params: serde_json::Value,
    skill_dcc: Option<&str>,
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
    let python_exe_override = std::env::var("DCC_MCP_PYTHON_EXECUTABLE").ok();
    let python_exe = python_exe_override
        .clone()
        .unwrap_or_else(|| "python".to_string());

    // Optional: prepend a Python init snippet before running the skill script.
    // DCC_MCP_PYTHON_INIT_SNIPPET can contain a one-liner (semicolon separated)
    // to run before the script, e.g. "import maya.standalone; maya.standalone.initialize(name='python')"
    let init_snippet = std::env::var("DCC_MCP_PYTHON_INIT_SNIPPET").ok();

    // Fail-loud when a skill targeting a DCC host Python is about to be launched
    // through the ambient `python` on PATH (issue #231). A skill can opt out of
    // the check by setting `DCC_MCP_ALLOW_AMBIENT_PYTHON=1` (intended for test
    // harnesses and the stub-based `python` DCC).
    if (ext == "py" || ext == "pyw")
        && python_exe_override.is_none()
        && init_snippet.is_none()
        && std::env::var("DCC_MCP_ALLOW_AMBIENT_PYTHON")
            .ok()
            .as_deref()
            != Some("1")
    {
        if let Some(dcc) = skill_dcc {
            let dcc_lc = dcc.to_ascii_lowercase();
            if DCC_NAMES_REQUIRING_HOST_PYTHON.contains(&dcc_lc.as_str()) {
                let msg = format!(
                    "Skill for DCC '{dcc}' cannot run with the ambient Python on PATH: \
                     `import {dcc_lc}.cmds` (or the equivalent) is either missing or a stub. \
                     Export DCC_MCP_PYTHON_EXECUTABLE to the DCC's host interpreter \
                     (e.g. mayapy, hython, 'blender --python') and DCC_MCP_PYTHON_INIT_SNIPPET \
                     to the per-DCC bootstrap code before starting the MCP server. \
                     Set DCC_MCP_ALLOW_AMBIENT_PYTHON=1 only for tests / stubs. \
                     See issue #231 for the contract."
                );
                tracing::error!(target: "dcc_mcp_skills::execute", %dcc, "{}", msg);
                return Err(msg);
            }
        }
    }

    // Guard against accidentally pointing DCC_MCP_PYTHON_EXECUTABLE at a GUI
    // executable (e.g. maya.exe, Maya, blender.exe).  Spawning a GUI as a
    // Python interpreter will open a second DCC window instead of running the
    // skill script.
    if let Some(ref exe) = python_exe_override {
        if let Some(stem) = std::path::Path::new(exe)
            .file_stem()
            .and_then(|s| s.to_str())
        {
            let stem_lc = stem.to_lowercase();
            if DCC_GUI_EXECUTABLES.iter().any(|g| g == &stem_lc.as_str()) {
                let msg = format!(
                    "DCC_MCP_PYTHON_EXECUTABLE points to a GUI executable '{exe}' ({stem}). \
                     This will spawn a new DCC window instead of running the skill script. \
                     Use the command-line interpreter (e.g. mayapy, blender --python, hython) \
                     or leave DCC_MCP_PYTHON_EXECUTABLE unset to use the in-process executor."
                );
                tracing::error!(target: "dcc_mcp_skills::execute", exe, "{}", msg);
                return Err(msg);
            }
        }
    }

    // Build CLI args that argparse-based scripts can consume.
    // Only scalar values (string, number, bool) are expanded; objects/arrays
    // are left for the stdin JSON path.
    let mut cli_extra: Vec<String> = Vec::new();
    if let Some(obj) = params.as_object() {
        for (key, val) in obj {
            let flag = format!("--{}", key.replace('_', "-"));
            match val {
                serde_json::Value::String(s) => {
                    cli_extra.push(flag);
                    cli_extra.push(s.clone());
                }
                serde_json::Value::Number(n) => {
                    cli_extra.push(flag);
                    cli_extra.push(n.to_string());
                }
                serde_json::Value::Bool(b) => {
                    // Boolean flags: --flag true / --flag false
                    cli_extra.push(flag);
                    cli_extra.push(b.to_string());
                }
                // Skip null / object / array — too complex for CLI args
                _ => {}
            }
        }
    }

    // Choose interpreter based on extension, appending the CLI extra args
    let (program, mut args): (String, Vec<String>) = match ext.as_str() {
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
    // Append CLI flags after the script path (or after the -c "..." snippet)
    args.extend(cli_extra);

    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn '{script_path}': {e}"))?;

    // Write full params JSON to stdin so scripts can also do json.load(sys.stdin)
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
