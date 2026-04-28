//! PyO3 bindings for `SkillGroup` / `ToolDeclaration` / `ToolAnnotations`.

use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDictMethods};
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;

use crate::skill_metadata::{
    ExecutionMode, NextTools, SkillGroup, ThreadAffinity, ToolAnnotations, ToolDeclaration,
};

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl SkillGroup {
    #[new]
    #[pyo3(signature = (name, description="".to_string(), tools=Vec::<String>::new(), default_active=false))]
    fn new(name: String, description: String, tools: Vec<String>, default_active: bool) -> Self {
        Self {
            name,
            description,
            tools,
            default_active,
        }
    }

    // Trivial getters (`name`, `description`, `tools`, `default_active`)
    // are emitted by `#[derive(PyWrapper)]` (#528 M3.4) on the struct;
    // see `crate::skill_metadata::SkillGroup`'s `#[py_wrapper(...)]` table.

    fn __repr__(&self) -> String {
        format!(
            "SkillGroup(name={:?}, tools={}, default_active={})",
            self.name,
            self.tools.len(),
            self.default_active
        )
    }
}

#[pymethods]
impl ToolDeclaration {
    #[new]
    #[pyo3(signature = (name, description="".to_string(), input_schema=None, output_schema=None, read_only=false, destructive=false, idempotent=false, defer_loading=false, source_file="".to_string(), group="".to_string(), execution="sync".to_string(), timeout_hint_secs=None, thread_affinity="any".to_string()))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        description: String,
        input_schema: Option<String>,
        output_schema: Option<String>,
        read_only: bool,
        destructive: bool,
        idempotent: bool,
        defer_loading: bool,
        source_file: String,
        group: String,
        execution: String,
        timeout_hint_secs: Option<u32>,
        thread_affinity: String,
    ) -> pyo3::PyResult<Self> {
        let input_schema = input_schema
            .and_then(|schema| serde_json::from_str(&schema).ok())
            .unwrap_or(serde_json::json!({"type": "object"}));
        let output_schema = output_schema
            .and_then(|schema| serde_json::from_str(&schema).ok())
            .unwrap_or(serde_json::Value::Null);
        let execution = parse_execution_mode(&execution)?;
        let thread_affinity = parse_thread_affinity(&thread_affinity)?;
        Ok(Self {
            name,
            description,
            input_schema,
            output_schema,
            read_only,
            destructive,
            idempotent,
            defer_loading,
            source_file,
            next_tools: NextTools::default(),
            group,
            execution,
            timeout_hint_secs,
            thread_affinity,
            _deferred_guard: None,
            annotations: ToolAnnotations::default(),
            required_capabilities: Vec::new(),
        })
    }

    // ── Trivial accessors emitted by `#[derive(PyWrapper)]` (#528 M3.4) ─
    // `name`, `description`, `read_only`, `destructive`, `idempotent`,
    // `defer_loading`, `source_file`, `group` (read+write), plus
    // `timeout_hint_secs` (Option<u32>) and `required_capabilities`
    // (Vec<String>). See `crate::skill_metadata::ToolDeclaration`'s
    // `#[py_wrapper(...)]` table for the canonical list.

    #[getter]
    fn execution(&self) -> &'static str {
        match self.execution {
            ExecutionMode::Sync => "sync",
            ExecutionMode::Async => "async",
        }
    }

    #[setter]
    fn set_execution(&mut self, value: String) -> pyo3::PyResult<()> {
        self.execution = parse_execution_mode(&value)?;
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!("ToolDeclaration(name={:?})", self.name)
    }

    #[getter]
    fn input_schema(&self) -> String {
        self.input_schema.to_string()
    }

    #[setter]
    fn set_input_schema(&mut self, value: String) {
        self.input_schema =
            serde_json::from_str(&value).unwrap_or(serde_json::json!({"type": "object"}));
    }

    #[getter]
    fn output_schema(&self) -> String {
        if self.output_schema.is_null() {
            String::new()
        } else {
            self.output_schema.to_string()
        }
    }

    #[setter]
    fn set_output_schema(&mut self, value: String) {
        self.output_schema = if value.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&value).unwrap_or(serde_json::Value::Null)
        };
    }

    #[getter]
    fn annotations(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<Py<PyAny>> {
        use pyo3::types::PyDict;
        let dict = PyDict::new(py);
        if let Some(value) = &self.annotations.title {
            dict.set_item("title", value)?;
        }
        if let Some(value) = self.annotations.read_only_hint {
            dict.set_item("readOnlyHint", value)?;
        }
        if let Some(value) = self.annotations.destructive_hint {
            dict.set_item("destructiveHint", value)?;
        }
        if let Some(value) = self.annotations.idempotent_hint {
            dict.set_item("idempotentHint", value)?;
        }
        if let Some(value) = self.annotations.open_world_hint {
            dict.set_item("openWorldHint", value)?;
        }
        if let Some(value) = self.annotations.deferred_hint {
            dict.set_item("deferredHint", value)?;
        }
        Ok(dict.into_any().unbind())
    }

    #[setter]
    fn set_annotations(
        &mut self,
        py: pyo3::Python<'_>,
        value: Option<Py<PyAny>>,
    ) -> pyo3::PyResult<()> {
        use pyo3::types::PyDict;
        let Some(obj) = value else {
            self.annotations = ToolAnnotations::default();
            return Ok(());
        };
        let bound = obj.bind(py);
        if bound.is_none() {
            self.annotations = ToolAnnotations::default();
            return Ok(());
        }
        let dict = bound.cast::<PyDict>().map_err(|_| {
            pyo3::exceptions::PyTypeError::new_err("annotations must be a dict or None")
        })?;

        self.annotations = ToolAnnotations {
            title: get_dict_string(dict, &["title"])?,
            read_only_hint: get_dict_bool(dict, &["read_only_hint", "readOnlyHint"])?,
            destructive_hint: get_dict_bool(dict, &["destructive_hint", "destructiveHint"])?,
            idempotent_hint: get_dict_bool(dict, &["idempotent_hint", "idempotentHint"])?,
            open_world_hint: get_dict_bool(dict, &["open_world_hint", "openWorldHint"])?,
            deferred_hint: get_dict_bool(dict, &["deferred_hint", "deferredHint"])?,
        };
        Ok(())
    }

    #[getter]
    fn next_tools<'py>(
        &self,
        py: Python<'py>,
    ) -> pyo3::PyResult<Option<pyo3::Bound<'py, pyo3::types::PyDict>>> {
        if self.next_tools.on_success.is_empty() && self.next_tools.on_failure.is_empty() {
            return Ok(None);
        }
        let dict = pyo3::types::PyDict::new(py);
        dict.set_item("on_success", self.next_tools.on_success.clone())?;
        dict.set_item("on_failure", self.next_tools.on_failure.clone())?;
        Ok(Some(dict))
    }

    #[setter]
    fn set_next_tools(
        &mut self,
        value: Option<&pyo3::Bound<'_, pyo3::types::PyAny>>,
    ) -> pyo3::PyResult<()> {
        use pyo3::types::PyDict;
        let Some(value) = value else {
            self.next_tools = NextTools::default();
            return Ok(());
        };
        if value.is_none() {
            self.next_tools = NextTools::default();
            return Ok(());
        }
        let dict = value.cast::<PyDict>().map_err(|_| {
            pyo3::exceptions::PyTypeError::new_err(
                "next_tools must be a dict with optional on_success/on_failure list keys, or None",
            )
        })?;
        let on_success: Vec<String> = dict
            .get_item("on_success")
            .ok()
            .flatten()
            .map(|value| value.extract())
            .transpose()?
            .unwrap_or_default();
        let on_failure: Vec<String> = dict
            .get_item("on_failure")
            .ok()
            .flatten()
            .map(|value| value.extract())
            .transpose()?
            .unwrap_or_default();
        self.next_tools = NextTools {
            on_success,
            on_failure,
        };
        Ok(())
    }
}

fn parse_execution_mode(value: &str) -> pyo3::PyResult<ExecutionMode> {
    match value {
        "sync" => Ok(ExecutionMode::Sync),
        "async" => Ok(ExecutionMode::Async),
        other => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "execution must be 'sync' or 'async' (got {other:?})",
        ))),
    }
}

fn parse_thread_affinity(value: &str) -> pyo3::PyResult<ThreadAffinity> {
    ThreadAffinity::parse(value).ok_or_else(|| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "thread_affinity must be 'any' or 'main' (got {value:?})"
        ))
    })
}

fn get_dict_bool(
    dict: &pyo3::Bound<'_, pyo3::types::PyDict>,
    keys: &[&str],
) -> pyo3::PyResult<Option<bool>> {
    for key in keys {
        if let Some(value) = dict.get_item(key)? {
            if value.is_none() {
                return Ok(None);
            }
            return Ok(Some(value.extract::<bool>()?));
        }
    }
    Ok(None)
}

fn get_dict_string(
    dict: &pyo3::Bound<'_, pyo3::types::PyDict>,
    keys: &[&str],
) -> pyo3::PyResult<Option<String>> {
    for key in keys {
        if let Some(value) = dict.get_item(key)? {
            if value.is_none() {
                return Ok(None);
            }
            return Ok(Some(value.extract::<String>()?));
        }
    }
    Ok(None)
}
