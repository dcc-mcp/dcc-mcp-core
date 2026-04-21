//! dcc-mcp-core: Python bindings entry point.
//!
//! This root crate serves as the `dcc_mcp_core._core` Python extension module.
//! All logic lives in workspace sub-crates; this crate only re-exports and registers.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

// Re-export sub-crates for Rust consumers
pub use dcc_mcp_actions as actions;
pub use dcc_mcp_capture as capture;
pub use dcc_mcp_http as http;
pub use dcc_mcp_models as models;
pub use dcc_mcp_naming as naming;
pub use dcc_mcp_process as process;
pub use dcc_mcp_protocols as protocols;
pub use dcc_mcp_sandbox as sandbox;
pub use dcc_mcp_shm as shm;
pub use dcc_mcp_skills as skills;
pub use dcc_mcp_telemetry as telemetry;
pub use dcc_mcp_transport as transport;
pub use dcc_mcp_usd as usd;
pub use dcc_mcp_utils as utils;

#[cfg(feature = "workflow")]
pub use dcc_mcp_workflow as workflow;

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
    register_process(m)?;
    register_telemetry(m)?;
    register_sandbox(m)?;
    register_shm(m)?;
    register_capture(m)?;
    register_usd(m)?;
    register_utils(m)?;
    register_http(m)?;
    register_naming(m)?;
    register_constants(m)?;
    #[cfg(feature = "workflow")]
    register_workflow(m)?;

    // ── Metadata ──
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("__author__", env!("CARGO_PKG_AUTHORS"))?;

    Ok(())
}

// ── Type-stub generation (opt-in, PoC) ──
//
// Enabled by the `stub-gen` feature — powers the `stub_gen` binary target
// which regenerates `python/dcc_mcp_core/_core.pyi` from annotated Rust code.
#[cfg(feature = "stub-gen")]
pyo3_stub_gen::define_stub_info_gatherer!(stub_info);

#[cfg(feature = "python-bindings")]
fn register_models(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(
        m,
        dcc_mcp_models::ActionResultModel,
        dcc_mcp_models::SerializeFormat,
        dcc_mcp_models::SkillGroup,
        dcc_mcp_models::SkillMetadata,
    );
    add_functions!(
        m,
        dcc_mcp_models::py_success_result,
        dcc_mcp_models::py_error_result,
        dcc_mcp_models::py_from_exception,
        dcc_mcp_models::py_validate_action_result,
        dcc_mcp_models::py_serialize_result,
        dcc_mcp_models::py_deserialize_result,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_actions(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(
        m,
        dcc_mcp_actions::ActionRegistry,
        dcc_mcp_actions::EventBus,
        dcc_mcp_actions::SemVer,
        dcc_mcp_actions::versioned::PyVersionConstraint,
        dcc_mcp_actions::VersionedRegistry,
    );
    dcc_mcp_actions::python::register_classes(m)?;
    dcc_mcp_actions::pipeline::python::register_classes(m)?;
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_protocols(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(
        m,
        // MCP protocol types
        dcc_mcp_protocols::ToolDefinition,
        dcc_mcp_protocols::ToolAnnotations,
        dcc_mcp_protocols::ResourceAnnotations,
        dcc_mcp_protocols::ResourceDefinition,
        dcc_mcp_protocols::ResourceTemplateDefinition,
        dcc_mcp_protocols::PromptArgument,
        dcc_mcp_protocols::PromptDefinition,
        // DCC adapter types
        dcc_mcp_protocols::PyDccInfo,
        dcc_mcp_protocols::PyScriptResult,
        dcc_mcp_protocols::PyScriptLanguage,
        dcc_mcp_protocols::PySceneInfo,
        dcc_mcp_protocols::PySceneStatistics,
        dcc_mcp_protocols::PyDccCapabilities,
        dcc_mcp_protocols::PyDccError,
        dcc_mcp_protocols::PyDccErrorCode,
        dcc_mcp_protocols::PyCaptureResult,
        // Cross-DCC protocol data models
        dcc_mcp_protocols::PyObjectTransform,
        dcc_mcp_protocols::PyBoundingBox,
        dcc_mcp_protocols::PySceneObject,
        dcc_mcp_protocols::PySceneNode,
        dcc_mcp_protocols::PyFrameRange,
        dcc_mcp_protocols::PyRenderOutput,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_skills(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(
        m,
        dcc_mcp_skills::SkillScanner,
        dcc_mcp_skills::PySkillWatcher,
        dcc_mcp_skills::SkillCatalog,
        dcc_mcp_skills::SkillSummary,
        dcc_mcp_models::ToolDeclaration,
    );
    add_functions!(
        m,
        dcc_mcp_skills::py_parse_skill_md,
        dcc_mcp_skills::py_scan_skill_paths,
        dcc_mcp_skills::py_resolve_dependencies,
        dcc_mcp_skills::py_validate_dependencies,
        dcc_mcp_skills::py_expand_transitive_dependencies,
        dcc_mcp_skills::py_scan_and_load,
        dcc_mcp_skills::py_scan_and_load_lenient,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_transport(m: &Bound<'_, PyModule>) -> PyResult<()> {
    add_classes!(
        m,
        dcc_mcp_transport::PyServiceEntry,
        dcc_mcp_transport::PyServiceStatus,
        dcc_mcp_transport::PyTransportAddress,
        dcc_mcp_transport::PyTransportScheme,
        dcc_mcp_transport::PyDccLinkFrame,
        dcc_mcp_transport::PyIpcChannelAdapter,
        dcc_mcp_transport::PyGracefulIpcChannelAdapter,
        dcc_mcp_transport::PySocketServerAdapter,
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
        dcc_mcp_utils::filesystem::py_get_tools_dir,
        dcc_mcp_utils::filesystem::py_get_skills_dir,
        dcc_mcp_utils::filesystem::py_get_skill_paths_from_env,
        dcc_mcp_utils::filesystem::py_get_app_skill_paths_from_env,
        dcc_mcp_utils::type_wrappers::py_unwrap_value,
        dcc_mcp_utils::type_wrappers::py_unwrap_parameters,
        dcc_mcp_utils::type_wrappers::py_wrap_value,
        dcc_mcp_utils::file_logging::python::py_init_file_logging,
        dcc_mcp_utils::file_logging::python::py_shutdown_file_logging,
        dcc_mcp_utils::file_logging::python::py_default_settings,
    );
    add_classes!(
        m,
        dcc_mcp_utils::type_wrappers::BooleanWrapper,
        dcc_mcp_utils::type_wrappers::IntWrapper,
        dcc_mcp_utils::type_wrappers::FloatWrapper,
        dcc_mcp_utils::type_wrappers::StringWrapper,
        dcc_mcp_utils::file_logging::python::PyFileLoggingConfig,
    );
    Ok(())
}

#[cfg(feature = "python-bindings")]
fn register_process(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_process::python::register_classes(m)
}

#[cfg(feature = "python-bindings")]
fn register_telemetry(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_telemetry::python::register_classes(m)
}

#[cfg(feature = "python-bindings")]
fn register_sandbox(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_sandbox::python::register_classes(m)
}

#[cfg(feature = "python-bindings")]
fn register_shm(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_shm::python::register_classes(m)
}

#[cfg(feature = "python-bindings")]
fn register_capture(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_capture::python::register_classes(m)
}

#[cfg(feature = "python-bindings")]
fn register_usd(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_usd::python::register_classes(m)
}

#[cfg(feature = "python-bindings")]
fn register_http(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_http::python::register_classes(m)
}

#[cfg(feature = "python-bindings")]
fn register_naming(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_naming::python::register(m)
}

#[cfg(all(feature = "python-bindings", feature = "workflow"))]
fn register_workflow(m: &Bound<'_, PyModule>) -> PyResult<()> {
    dcc_mcp_workflow::python::register_classes(m)
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
        "ENV_LOG_FILE"       => constants::ENV_LOG_FILE,
        "ENV_LOG_DIR"        => constants::ENV_LOG_DIR,
        "ENV_LOG_MAX_SIZE"   => constants::ENV_LOG_MAX_SIZE,
        "ENV_LOG_MAX_FILES"  => constants::ENV_LOG_MAX_FILES,
        "ENV_LOG_ROTATION"   => constants::ENV_LOG_ROTATION,
        "ENV_LOG_FILE_PREFIX"=> constants::ENV_LOG_FILE_PREFIX,
        "DEFAULT_LOG_FILE_PREFIX" => constants::DEFAULT_LOG_FILE_PREFIX,
        "DEFAULT_LOG_ROTATION"   => constants::DEFAULT_LOG_ROTATION,
    );
    m.add("DEFAULT_LOG_MAX_SIZE", constants::DEFAULT_LOG_MAX_SIZE)?;
    m.add("DEFAULT_LOG_MAX_FILES", constants::DEFAULT_LOG_MAX_FILES)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_workspace_crates_accessible() {
        let _ = dcc_mcp_models::ActionResultModelData::default();
        let reg = dcc_mcp_actions::ActionRegistry::new();
        assert!(reg.is_empty());
        let monitor = dcc_mcp_process::ProcessMonitor::new();
        assert_eq!(monitor.tracked_count(), 0);
        // Verify telemetry types are accessible
        let cfg = dcc_mcp_telemetry::TelemetryConfig::builder("test").build();
        assert_eq!(cfg.service_name, "test");
        // Verify sandbox types are accessible
        let policy = dcc_mcp_sandbox::SandboxPolicy::default();
        assert!(policy.check_action("anything").is_ok());
        // Verify shm types are accessible
        let buf = dcc_mcp_shm::SharedBuffer::create(256).unwrap();
        assert_eq!(buf.capacity(), 256);
        // Verify capture types are accessible
        let capturer = dcc_mcp_capture::Capturer::new_auto();
        let (count, _, _) = capturer.stats().snapshot();
        assert_eq!(count, 0);
        // Verify USD types are accessible
        let mut stage = dcc_mcp_usd::UsdStage::new("test");
        stage.define_prim(dcc_mcp_usd::SdfPath::new("/World").unwrap(), "Xform");
        assert!(stage.has_prim("/World"));
        let m = stage.metrics();
        assert_eq!(m.xform_count, 1);
    }
}
