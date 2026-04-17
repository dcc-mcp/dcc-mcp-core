//! Type-stub generator — PoC for `pyo3-stub-gen` integration.
//!
//! Walks every `#[gen_stub_pyclass]` / `#[gen_stub_pymethods]` annotation
//! registered via `inventory::submit!` at link time and emits a `.pyi`
//! package for the `dcc_mcp_core._core` module.
//!
//! Run with:
//!     cargo run --bin stub_gen --features stub-gen
//!     # or: just stubgen
//!
//! PoC scope: only the `dcc-mcp-capture` crate is annotated, so only its
//! types appear in the generated output.
//!
//! ## How this plays with the hand-maintained `python/dcc_mcp_core/_core.pyi`
//!
//! `pyo3-stub-gen` v0.20 detects our mixed layout (`python-source =
//! "python"`, `module-name = "dcc_mcp_core._core"`) and writes a *package-
//! style* stub:
//!
//!   python/dcc_mcp_core/_core/__init__.pyi    ← generated (5.7 KB for 6 classes)
//!   python/dcc_mcp_core/__init__.pyi          ← generated umbrella
//!
//! This collides with (and would eventually replace) the single-file
//! `python/dcc_mcp_core/_core.pyi` we currently maintain by hand. For the
//! PoC we move the generated files out of `python/` into `target/stubgen/`
//! so the hand-written stub stays authoritative.

use std::path::{Path, PathBuf};

use pyo3_stub_gen::Result;

const GEN_PKG_STUB: &str = "python/dcc_mcp_core/_core/__init__.pyi";
const GEN_PARENT_STUB: &str = "python/dcc_mcp_core/__init__.pyi";
const GEN_PKG_DIR: &str = "python/dcc_mcp_core/_core";

const POC_PKG_STUB: &str = "target/stubgen/_core/__init__.pyi";
const POC_PARENT_STUB: &str = "target/stubgen/__init__.pyi";

fn main() -> Result<()> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // 1. Run the generator — this writes into python/dcc_mcp_core/...
    let stub = _core::stub_info()?;
    stub.generate()?;

    // 2. Relocate the output so we don't clobber the hand-written stub.
    let gen_pkg = manifest_dir.join(GEN_PKG_STUB);
    let gen_parent = manifest_dir.join(GEN_PARENT_STUB);
    let gen_pkg_dir = manifest_dir.join(GEN_PKG_DIR);
    let poc_pkg = manifest_dir.join(POC_PKG_STUB);
    let poc_parent = manifest_dir.join(POC_PARENT_STUB);

    if let Some(dir) = poc_pkg.parent() {
        std::fs::create_dir_all(dir)?;
    }
    move_replace(&gen_pkg, &poc_pkg)?;
    move_replace(&gen_parent, &poc_parent)?;
    // Clean up the now-empty _core/ directory pyo3-stub-gen created next to _core.pyi.
    let _ = std::fs::remove_dir(&gen_pkg_dir);

    println!();
    println!("=== pyo3-stub-gen PoC — generation complete ===");
    println!(
        "Hand-written stub  : {}",
        manifest_dir.join("python/dcc_mcp_core/_core.pyi").display()
    );
    println!("Generated package  : {}", poc_pkg.display());
    println!("Generated umbrella : {}", poc_parent.display());
    println!();
    println!("View with:");
    println!("    Get-Content {}", poc_pkg.display());
    Ok(())
}

/// Rename `src` to `dst`, overwriting `dst` if it exists. No-op if `src` missing.
fn move_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    if dst.exists() {
        std::fs::remove_file(dst)?;
    }
    std::fs::rename(src, dst)?;
    Ok(())
}
