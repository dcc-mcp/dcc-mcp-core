//! Native JSON Schema validation used by public Python contracts.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

const MAX_VALIDATION_ERRORS: usize = 32;

fn validate_draft_2020_12(schema_json: &str, instance_json: &str) -> Result<Vec<String>, String> {
    let schema: serde_json::Value = serde_json::from_str(schema_json)
        .map_err(|error| format!("invalid JSON schema document: {error}"))?;
    let instance: serde_json::Value = serde_json::from_str(instance_json)
        .map_err(|error| format!("invalid JSON instance document: {error}"))?;
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&schema)
        .map_err(|error| format!("failed to compile Draft 2020-12 schema: {error}"))?;

    Ok(validator
        .iter_errors(&instance)
        .take(MAX_VALIDATION_ERRORS)
        .map(|error| {
            format!(
                "instance={} schema={}: {}",
                error.instance_path, error.schema_path, error
            )
        })
        .collect())
}

/// Validate a JSON document against a JSON Schema Draft 2020-12 document.
///
/// This intentionally remains a private native primitive. Public callers use
/// `dcc_mcp_core.deployment.validate_install_sop_report`, which owns the
/// packaged schema and stable exception contract.
#[pyfunction]
fn _validate_json_schema_draft_2020_12(
    schema_json: &str,
    instance_json: &str,
) -> PyResult<Vec<String>> {
    validate_draft_2020_12(schema_json, instance_json).map_err(PyValueError::new_err)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(_validate_json_schema_draft_2020_12, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_draft_2020_12;

    #[test]
    fn reports_instance_and_schema_paths() {
        let schema = r#"{
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"name": {"type": "string", "minLength": 1}},
            "required": ["name"]
        }"#;
        let errors = validate_draft_2020_12(schema, r#"{"name":""}"#).unwrap();

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("instance=/name"));
        assert!(errors[0].contains("schema=/properties/name/minLength"));
    }
}
