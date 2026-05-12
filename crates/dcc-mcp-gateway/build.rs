//! Materialise `src/gateway/admin/generated/index.html` via the Vite admin bundle.
//!
//! The output directory is gitignored. When the `admin` feature is enabled, this script
//! runs `vx npm …` so Node/npm are resolved from project `vx.toml` via the `vx` shim (not
//! a standalone system Node install). CI and `just preflight` assume `vx` is on PATH.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn workspace_root(manifest_dir: &Path) -> Result<PathBuf, String> {
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "expected Cargo.toml under crates/dcc-mcp-gateway".to_string())
}

fn run_vx_npm(workspace: &Path, npm_argv: &[&str]) -> Result<(), String> {
    let mut argv: Vec<&str> = vec!["npm"];
    argv.extend_from_slice(npm_argv);

    let status = Command::new("vx")
        .args(&argv)
        .current_dir(workspace)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| {
            format!(
                "failed to spawn `vx {}`: {error} (install vx / see vx.toml `[tools].node`)",
                argv.join(" ")
            )
        })?;

    if !status.success() {
        return Err(format!("`vx {}` exited with {status}", argv.join(" ")));
    }

    Ok(())
}

fn main() -> Result<(), String> {
    if std::env::var_os("CARGO_FEATURE_ADMIN").is_none() {
        return Ok(());
    }

    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "CARGO_MANIFEST_DIR is not set".to_string())?;
    let ws = workspace_root(&manifest_dir)?;
    let admin_ui = ws.join("admin-ui");
    let out = manifest_dir.join("src/gateway/admin/generated/index.html");

    println!(
        "cargo:rerun-if-changed={}",
        admin_ui.join("package.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        admin_ui.join("package-lock.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        admin_ui.join("vite.config.ts").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        admin_ui.join("tsconfig.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        admin_ui.join("tsconfig.app.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        admin_ui.join("index.html").display()
    );
    println!("cargo:rerun-if-changed={}", admin_ui.join("src").display());

    if !admin_ui.join("package.json").is_file() {
        return Err(format!(
            "admin-ui/package.json missing (expected {}). Clone the full workspace.",
            admin_ui.display()
        ));
    }

    if !admin_ui.join("node_modules").exists() {
        println!("cargo:warning=Installing admin-ui dependencies (vx npm ci)");
        run_vx_npm(&ws, &["--prefix", "admin-ui", "ci"])?;
    }

    run_vx_npm(&ws, &["--prefix", "admin-ui", "run", "build"])?;

    if !out.is_file() {
        return Err(format!("admin build did not write {}", out.display()));
    }

    Ok(())
}
