use super::*;

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

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }
    #[setter]
    fn set_name(&mut self, value: String) {
        self.name = value;
    }

    #[getter]
    fn description(&self) -> &str {
        &self.description
    }
    #[setter]
    fn set_description(&mut self, value: String) {
        self.description = value;
    }

    #[getter]
    fn dcc(&self) -> &str {
        &self.dcc
    }
    #[setter]
    fn set_dcc(&mut self, value: String) {
        self.dcc = value;
    }

    #[getter]
    fn version(&self) -> &str {
        &self.version
    }
    #[setter]
    fn set_version(&mut self, value: String) {
        self.version = value;
    }

    #[getter]
    fn license(&self) -> &str {
        &self.license
    }
    #[setter]
    fn set_license(&mut self, value: String) {
        self.license = value;
    }

    #[getter]
    fn compatibility(&self) -> &str {
        &self.compatibility
    }
    #[setter]
    fn set_compatibility(&mut self, value: String) {
        self.compatibility = value;
    }

    #[getter]
    fn skill_path(&self) -> &str {
        &self.skill_path
    }
    #[setter]
    fn set_skill_path(&mut self, value: String) {
        self.skill_path = value;
    }

    #[getter]
    fn tags(&self) -> Vec<String> {
        self.tags.clone()
    }
    #[setter]
    fn set_tags(&mut self, value: Vec<String>) {
        self.tags = value;
    }

    #[getter]
    fn search_hint(&self) -> &str {
        &self.search_hint
    }
    #[setter]
    fn set_search_hint(&mut self, value: String) {
        self.search_hint = value;
    }

    #[getter]
    fn scripts(&self) -> Vec<String> {
        self.scripts.clone()
    }
    #[setter]
    fn set_scripts(&mut self, value: Vec<String>) {
        self.scripts = value;
    }

    #[getter]
    fn depends(&self) -> Vec<String> {
        self.depends.clone()
    }
    #[setter]
    fn set_depends(&mut self, value: Vec<String>) {
        self.depends = value;
    }

    #[getter]
    fn metadata_files(&self) -> Vec<String> {
        self.metadata_files.clone()
    }
    #[setter]
    fn set_metadata_files(&mut self, value: Vec<String>) {
        self.metadata_files = value;
    }

    #[getter]
    fn allowed_tools(&self) -> Vec<String> {
        self.allowed_tools.clone()
    }
    #[setter]
    fn set_allowed_tools(&mut self, value: Vec<String>) {
        self.allowed_tools = value;
    }

    #[getter]
    fn tools(&self) -> Vec<ToolDeclaration> {
        self.tools.clone()
    }
    #[setter]
    fn set_tools(&mut self, value: Vec<ToolDeclaration>) {
        self.tools = value;
    }

    #[getter]
    fn groups(&self) -> Vec<SkillGroup> {
        self.groups.clone()
    }
    #[setter]
    fn set_groups(&mut self, value: Vec<SkillGroup>) {
        self.groups = value;
    }

    #[getter]
    fn metadata(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<Py<PyAny>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        let value = if self.metadata.is_null() {
            serde_json::json!({})
        } else {
            self.metadata.clone()
        };
        json_value_to_pyobject(py, &value)
    }

    #[setter]
    fn set_metadata(&mut self, py: pyo3::Python<'_>, value: Py<PyAny>) -> pyo3::PyResult<()> {
        use dcc_mcp_utils::py_json::py_any_to_json_value;
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

    #[getter]
    fn legacy_extension_fields(&self) -> Vec<String> {
        self.legacy_extension_fields.clone()
    }

    #[getter]
    fn layer(&self) -> Option<String> {
        self.layer.clone()
    }

    #[setter]
    fn set_layer(&mut self, value: Option<String>) {
        self.layer = value;
    }

    #[pyo3(name = "required_capabilities")]
    fn py_required_capabilities(&self) -> Vec<String> {
        SkillMetadata::required_capabilities(self)
    }
}
