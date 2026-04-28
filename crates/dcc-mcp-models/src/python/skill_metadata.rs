//! PyO3 bindings for `SkillMetadata`.
//!
//! Trivial String / `Vec<T>` getters and setters are emitted by
//! `#[derive(PyWrapper)]` on the struct itself
//! (`crate::skill_metadata::SkillMetadata`); see issue #528 M3.3. This
//! module retains only the methods that need custom logic: the `#[new]`
//! constructor, the curated `__repr__` / `__str__` / `__eq__`, the
//! JSON-bridging `metadata` accessor pair, the serde-round-tripping
//! `policy` / `external_deps` accessor pairs, and every `py_*` method.

use pyo3::prelude::*;

use crate::skill_metadata::{SkillDependencies, SkillMetadata, SkillPolicy, ToolDeclaration};

use dcc_mcp_naming::{DEFAULT_DCC, DEFAULT_VERSION};

#[pymethods]
impl SkillMetadata {
    #[new]
    #[pyo3(signature = (
        name,
        description = "".to_string(),
        tools = vec![],
        dcc = DEFAULT_DCC.to_string(),
        tags = vec![],
        search_hint = "".to_string(),
        scripts = vec![],
        skill_path = "".to_string(),
        version = DEFAULT_VERSION.to_string(),
        depends = vec![],
        metadata_files = vec![],
        license = "".to_string(),
        compatibility = "".to_string(),
        allowed_tools = vec![],
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        description: String,
        tools: Vec<ToolDeclaration>,
        dcc: String,
        tags: Vec<String>,
        search_hint: String,
        scripts: Vec<String>,
        skill_path: String,
        version: String,
        depends: Vec<String>,
        metadata_files: Vec<String>,
        license: String,
        compatibility: String,
        allowed_tools: Vec<String>,
    ) -> Self {
        Self {
            name,
            description,
            tools,
            dcc,
            tags,
            search_hint,
            scripts,
            skill_path,
            version,
            depends,
            metadata_files,
            license,
            compatibility,
            allowed_tools,
            metadata: serde_json::Value::Null,
            policy: None,
            external_deps: None,
            groups: Vec::new(),
            legacy_extension_fields: Vec::new(),
            prompts_file: None,
            layer: None,
            recipes_file: None,
            introspection_file: None,
        }
    }

    fn __repr__(&self) -> String {
        format!("SkillMetadata(name={:?}, dcc={:?})", self.name, self.dcc)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __eq__(&self, other: &SkillMetadata) -> bool {
        self == other
    }

    // ── Trivial accessors emitted by `#[derive(PyWrapper)]` (#528 M3.3) ─
    // `name`, `description`, `dcc`, `version`, `license`, `compatibility`,
    // `skill_path`, `search_hint` (String → &str + setter), `tags`,
    // `scripts`, `depends`, `metadata_files`, `allowed_tools`, `tools`,
    // `groups`, `legacy_extension_fields`, `layer` (Vec<T> / Option<T>
    // clone + setter, except `legacy_extension_fields` which is read-only).
    // See `crate::skill_metadata::SkillMetadata`'s `#[py_wrapper(...)]`
    // table for the canonical list.

    #[getter]
    fn metadata(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<Py<PyAny>> {
        use dcc_mcp_pybridge::py_json::json_value_to_pyobject;
        let value = if self.metadata.is_null() {
            serde_json::json!({})
        } else {
            self.metadata.clone()
        };
        json_value_to_pyobject(py, &value)
    }

    #[setter]
    fn set_metadata(&mut self, py: pyo3::Python<'_>, value: Py<PyAny>) -> pyo3::PyResult<()> {
        use dcc_mcp_pybridge::py_json::py_any_to_json_value;
        self.metadata = py_any_to_json_value(value.bind(py))
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(())
    }

    #[getter]
    fn policy(&self) -> Option<String> {
        self.policy
            .as_ref()
            .and_then(|policy| serde_json::to_string(policy).ok())
    }

    #[setter]
    fn set_policy(&mut self, value: Option<String>) {
        self.policy = value.and_then(|json| serde_json::from_str::<SkillPolicy>(&json).ok());
    }

    #[pyo3(name = "is_implicit_invocation_allowed")]
    fn py_is_implicit_invocation_allowed(&self) -> bool {
        self.policy
            .as_ref()
            .map(|policy| policy.is_implicit_invocation_allowed())
            .unwrap_or(true)
    }

    #[pyo3(name = "matches_product")]
    fn py_matches_product(&self, product: String) -> bool {
        self.policy
            .as_ref()
            .map(|policy| policy.matches_product(&product))
            .unwrap_or(true)
    }

    #[getter]
    fn external_deps(&self) -> Option<String> {
        self.external_deps
            .as_ref()
            .and_then(|deps| serde_json::to_string(deps).ok())
    }

    #[setter]
    fn set_external_deps(&mut self, value: Option<String>) {
        self.external_deps =
            value.and_then(|json| serde_json::from_str::<SkillDependencies>(&json).ok());
    }

    #[pyo3(name = "required_env_vars")]
    fn py_required_env_vars(&self) -> Vec<String> {
        SkillMetadata::required_env_vars(self)
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[pyo3(name = "required_bins")]
    fn py_required_bins(&self) -> Vec<String> {
        SkillMetadata::required_bins(self)
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[pyo3(name = "primary_env")]
    fn py_primary_env(&self) -> Option<String> {
        SkillMetadata::primary_env(self).map(String::from)
    }

    #[pyo3(name = "emoji")]
    fn py_emoji(&self) -> Option<String> {
        SkillMetadata::emoji(self).map(String::from)
    }

    #[pyo3(name = "homepage")]
    fn py_homepage(&self) -> Option<String> {
        SkillMetadata::homepage(self).map(String::from)
    }

    #[pyo3(name = "validate")]
    fn py_validate(&self) -> Vec<String> {
        SkillMetadata::validate(self)
    }

    #[pyo3(name = "is_spec_compliant")]
    fn py_is_spec_compliant(&self) -> bool {
        SkillMetadata::is_spec_compliant(self)
    }

    #[pyo3(name = "required_capabilities")]
    fn py_required_capabilities(&self) -> Vec<String> {
        SkillMetadata::required_capabilities(self)
    }
}
