//! dcc-mcp-core: Python bindings entry point.
//!
//! This root crate serves as the `dcc_mcp_core._core` Python extension module.
//! All logic lives in workspace sub-crates; this crate only re-exports and registers.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

// Re-export sub-crates for Rust consumers
pub use dcc_mcp_actions as actions;
pub use dcc_mcp_models as models;
pub use dcc_mcp_protocols as protocols;
pub use dcc_mcp_skills as skills;
pub use dcc_mcp_transport as transport;
pub use dcc_mcp_utils as utils;

// ── Helper macros (defined before use for readability) ──

/// Batch-register `#[pyfunction]`s on a module.
#[cfg(feature = "python-bindings")]
macro_rules! add_functions {
    ($m:expr, $($func:path),+ $(,)?) => {
        $( $m.add_function(wrap_pyfunction!($func, $m)?)?; )+
    };
}

/// Batch-register `#[pyclass]` types on a module.
#[cfg(feature = "python-bindings")]
macro_rules! add_classes {
    ($m:expr, $($cls:path),+ $(,)?) => {
        $( $m.add_class::<$cls>()?; )+
    };
}

/// Batch-register string constants on a module.
#[cfg(feature = "python-bindings")]
macro_rules! add_constants {
    ($m:expr, $($name:literal => $val:expr),+ $(,)?) => {
        $( $m.add($name, $val)?; )+
    };
}

// ── Module initialization ──

/// Python module initialization — `dcc_mcp_core._core`
#[cfg(feature = "python-bindings")]
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_utils::log_config::init_logging();

    register_models(m)?;
    register_actions(m)?;
    register_protocols(m)?;
    register_skills(m)?;
    register_transport(m)?;
    register_utils(m)?;
    register_constants(m)?;

    // ── Metadata ──
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("__author__", env!("CARGO_PKG_AUTHORS"))?;

    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_models(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(
        m,
        dcc_mcp_models::ActionResultModel,
        dcc_mcp_models::SkillMetadata,
    );
    add_functions!(
        m,
        dcc_mcp_models::py_success_result,
        dcc_mcp_models::py_error_result,
        dcc_mcp_models::py_from_exception,
        dcc_mcp_models::py_validate_action_result,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_actions(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(
        m,
        dcc_mcp_actions::ActionRegistry,
        dcc_mcp_actions::EventBus,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_protocols(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(
        m,
        dcc_mcp_protocols::ToolDefinition,
        dcc_mcp_protocols::ToolAnnotations,
        dcc_mcp_protocols::ResourceDefinition,
        dcc_mcp_protocols::ResourceTemplateDefinition,
        dcc_mcp_protocols::PromptArgument,
        dcc_mcp_protocols::PromptDefinition,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_skills(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(m, dcc_mcp_skills::SkillScanner);
    add_functions!(
        m,
        dcc_mcp_skills::py_parse_skill_md,
        dcc_mcp_skills::py_scan_skill_paths,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_transport(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(
        m,
        dcc_mcp_transport::PyTransportManager,
        dcc_mcp_transport::PyServiceEntry,
        dcc_mcp_transport::PyServiceStatus,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_utils(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_functions!(
        m,
        dcc_mcp_utils::filesystem::py_get_platform_dir,
        dcc_mcp_utils::filesystem::py_get_config_dir,
        dcc_mcp_utils::filesystem::py_get_data_dir,
        dcc_mcp_utils::filesystem::py_get_log_dir,
        dcc_mcp_utils::filesystem::py_get_actions_dir,
        dcc_mcp_utils::filesystem::py_get_skills_dir,
        dcc_mcp_utils::filesystem::py_get_skill_paths_from_env,
        dcc_mcp_utils::type_wrappers::py_unwrap_value,
        dcc_mcp_utils::type_wrappers::py_unwrap_parameters,
        dcc_mcp_utils::type_wrappers::py_wrap_value,
    );
    add_classes!(
        m,
        dcc_mcp_utils::type_wrappers::BooleanWrapper,
        dcc_mcp_utils::type_wrappers::IntWrapper,
        dcc_mcp_utils::type_wrappers::FloatWrapper,
        dcc_mcp_utils::type_wrappers::StringWrapper,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_constants(m: &Bound<'_, PyModule>) -> PyResult<()> {
    use dcc_mcp_utils::constants;
    add_constants!(
        m,
        "APP_NAME"           => constants::APP_NAME,
        "APP_AUTHOR"         => constants::APP_AUTHOR,
        "DEFAULT_DCC"        => constants::DEFAULT_DCC,
        "DEFAULT_VERSION"    => constants::DEFAULT_VERSION,
        "DEFAULT_MIME_TYPE"  => constants::DEFAULT_MIME_TYPE,
        "SKILL_METADATA_FILE"=> constants::SKILL_METADATA_FILE,
        "SKILL_SCRIPTS_DIR"  => constants::SKILL_SCRIPTS_DIR,
        "SKILL_METADATA_DIR" => constants::SKILL_METADATA_DIR,
        "ENV_SKILL_PATHS"    => constants::ENV_SKILL_PATHS,
        "ENV_LOG_LEVEL"      => constants::ENV_LOG_LEVEL,
        "DEFAULT_LOG_LEVEL"  => constants::DEFAULT_LOG_LEVEL,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_workspace_crates_accessible() {
        let _ = dcc_mcp_models::ActionResultModelData::default();
        let reg = dcc_mcp_actions::ActionRegistry::new();
        assert!(reg.is_empty());
    }
}
