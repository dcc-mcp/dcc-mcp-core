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

/// Python module initialization — `dcc_mcp_core._core`
#[cfg(feature = "python-bindings")]
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Initialize logging
    dcc_mcp_utils::log_config::init_logging();

    // ── Models ──
    m.add_class::<dcc_mcp_models::ActionResultModel>()?;
    m.add_class::<dcc_mcp_models::SkillMetadata>()?;
    m.add_function(wrap_pyfunction!(dcc_mcp_models::py_success_result, m)?)?;
    m.add_function(wrap_pyfunction!(dcc_mcp_models::py_error_result, m)?)?;
    m.add_function(wrap_pyfunction!(dcc_mcp_models::py_from_exception, m)?)?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_models::py_validate_action_result,
        m
    )?)?;

    // ── Actions ──
    m.add_class::<dcc_mcp_actions::ActionRegistry>()?;
    m.add_class::<dcc_mcp_actions::EventBus>()?;

    // ── Protocol types ──
    m.add_class::<dcc_mcp_protocols::ToolDefinition>()?;
    m.add_class::<dcc_mcp_protocols::ToolAnnotations>()?;
    m.add_class::<dcc_mcp_protocols::ResourceDefinition>()?;
    m.add_class::<dcc_mcp_protocols::ResourceTemplateDefinition>()?;
    m.add_class::<dcc_mcp_protocols::PromptArgument>()?;
    m.add_class::<dcc_mcp_protocols::PromptDefinition>()?;

    // ── Skills ──
    m.add_class::<dcc_mcp_skills::SkillScanner>()?;
    m.add_function(wrap_pyfunction!(dcc_mcp_skills::py_parse_skill_md, m)?)?;
    m.add_function(wrap_pyfunction!(dcc_mcp_skills::py_scan_skill_paths, m)?)?;

    // ── Transport ──
    m.add_class::<dcc_mcp_transport::PyTransportManager>()?;

    // ── Utils: filesystem ──
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::filesystem::py_get_platform_dir,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::filesystem::py_get_config_dir,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::filesystem::py_get_data_dir,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::filesystem::py_get_log_dir,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::filesystem::py_get_actions_dir,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::filesystem::py_get_skills_dir,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::filesystem::py_get_skill_paths_from_env,
        m
    )?)?;

    // ── Utils: type wrappers ──
    m.add_class::<dcc_mcp_utils::type_wrappers::BooleanWrapper>()?;
    m.add_class::<dcc_mcp_utils::type_wrappers::IntWrapper>()?;
    m.add_class::<dcc_mcp_utils::type_wrappers::FloatWrapper>()?;
    m.add_class::<dcc_mcp_utils::type_wrappers::StringWrapper>()?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::type_wrappers::py_unwrap_value,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::type_wrappers::py_unwrap_parameters,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        dcc_mcp_utils::type_wrappers::py_wrap_value,
        m
    )?)?;

    // ── Constants ──
    m.add("APP_NAME", dcc_mcp_utils::constants::APP_NAME)?;
    m.add("APP_AUTHOR", dcc_mcp_utils::constants::APP_AUTHOR)?;
    m.add("DEFAULT_DCC", dcc_mcp_utils::constants::DEFAULT_DCC)?;
    m.add(
        "SKILL_METADATA_FILE",
        dcc_mcp_utils::constants::SKILL_METADATA_FILE,
    )?;
    m.add(
        "SKILL_SCRIPTS_DIR",
        dcc_mcp_utils::constants::SKILL_SCRIPTS_DIR,
    )?;
    m.add("ENV_SKILL_PATHS", dcc_mcp_utils::constants::ENV_SKILL_PATHS)?;
    m.add("ENV_LOG_LEVEL", dcc_mcp_utils::constants::ENV_LOG_LEVEL)?;
    m.add(
        "DEFAULT_LOG_LEVEL",
        dcc_mcp_utils::constants::DEFAULT_LOG_LEVEL,
    )?;

    // ── Metadata ──
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("__author__", "Hal Long <hal.long@outlook.com>")?;

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
