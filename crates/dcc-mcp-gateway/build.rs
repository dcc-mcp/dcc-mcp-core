//! Materialise `src/gateway/admin/generated/index.html` via the Vite admin bundle.
//!
//! When the `admin` feature is enabled, this script runs `vx npm …` from the
//! `admin-ui` directory (Node is resolved via the parent `vx.toml`) or falls
//! back to plain `npm` with `current_dir(admin-ui)` when `vx` is not on `PATH`.
//! Build-only installs pass `--ignore-scripts` so the Playwright browser
//! postinstall is kept out of Cargo/CI compilation paths.
//!
//! **manylinux / PyO3 maturin-action**: the Linux wheel build runs inside a
//! Docker image that does not ship `vx`. CI must pre-build the bundle on the
//! host (see `.github/actions/build-wheel/action.yml`) and set
//! `DCC_MCP_ADMIN_UI_PREBUILT=1` so this script reuses the generated
//! `index.html` from the bind-mounted workspace (typically passed through
//! `maturin-action` `docker-options`).

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn workspace_root(manifest_dir: &Path) -> Result<PathBuf, String> {
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "expected Cargo.toml under crates/dcc-mcp-gateway".to_string())
}

/// Run `vx npm …` from `admin-ui/`, or `npm …` from `admin-ui/` if `vx`
/// is missing (e.g. manylinux Docker without vx — use `DCC_MCP_ADMIN_UI_PREBUILT`
/// instead).
fn run_npm_or_vx(admin_ui: &Path, npm_args: &[&str]) -> Result<(), String> {
    let mut vx_argv: Vec<&str> = vec!["npm"];
    vx_argv.extend_from_slice(npm_args);

    let vx_result = Command::new("vx")
        .args(&vx_argv)
        .current_dir(admin_ui)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match vx_result {
        Ok(status) if status.success() => return Ok(()),
        Ok(status) => {
            println!(
                "cargo:warning=`vx {}` exited with {status}; trying `npm {}` in {}",
                vx_argv.join(" "),
                npm_args.join(" "),
                admin_ui.display()
            );
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            println!(
                "cargo:warning=vx not on PATH; trying `npm {}` in {}",
                npm_args.join(" "),
                admin_ui.display()
            );
        }
        Err(e) => {
            return Err(format!("failed to spawn `vx {}`: {e}", vx_argv.join(" ")));
        }
    }

    let status = Command::new("npm")
        .args(npm_args)
        .current_dir(admin_ui)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                format!(
                    "failed to spawn `npm`: {e}. Install vx (see vx.toml `[tools].node`) \
                     or set DCC_MCP_ADMIN_UI_PREBUILT=1 after pre-building admin-ui on the host."
                )
            } else {
                format!("failed to spawn `npm`: {e}")
            }
        })?;

    if !status.success() {
        return Err(format!("`npm {}` exited with {status}", npm_args.join(" ")));
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

    if std::env::var_os("DCC_MCP_ADMIN_UI_PREBUILT").is_some() {
        if out.is_file() {
            println!("cargo:warning=reusing pre-built admin UI (DCC_MCP_ADMIN_UI_PREBUILT=1)");
            return Ok(());
        }
        // Fall back to building locally when the pre-built file is missing
        // (e.g. maturin sdist unpacked to a temp directory).
        println!(
            "cargo:warning=DCC_MCP_ADMIN_UI_PREBUILT=1 but {} is missing; falling back to local build",
            out.display()
        );
    }

    if !admin_ui.join("package.json").is_file() {
        return Err(format!(
            "admin-ui/package.json missing (expected {}). Clone the full workspace.",
            admin_ui.display()
        ));
    }

    if !admin_ui.join("node_modules").exists() {
        println!(
            "cargo:warning=Installing admin-ui dependencies (vx npm ci --ignore-scripts or npm ci --ignore-scripts)"
        );
        run_npm_or_vx(&admin_ui, &["ci", "--ignore-scripts"])?;
    }

    println!("cargo:warning=Building admin-ui bundle (vx npm run build or npm run build)");
    run_npm_or_vx(&admin_ui, &["run", "build"])?;

    if !out.is_file() {
        return Err(format!("admin build did not write {}", out.display()));
    }

    Ok(())
}
