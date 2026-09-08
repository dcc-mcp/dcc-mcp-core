//! Native validation for the canonical Install SOP v1 report contract.

use std::fmt;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};

const INSTALL_SOP_SCHEMA_ID: &str =
    "https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json";
const INSTALL_SOP_SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
const INSTALL_SOP_SCHEMA_SHA256: &str =
    "3ca25788439917b4d4c0617230a762f9797756b5b54f45c8c4149f975b90f904";
const DUPLICATE_JSON_KEY: &str = "duplicate_json_object_key";
const MAX_VALIDATION_ERRORS: usize = 32;
const MAX_DIAGNOSTIC_BYTES: usize = 512;
const MAX_DIAGNOSTICS_BYTES: usize = 8192;
const MAX_PATH_BYTES: usize = 192;
const MAX_KEYWORD_BYTES: usize = 48;
const TRUNCATED_DIAGNOSTIC: &str =
    "code=validation_truncated keyword=truncated instance=/ schema=/";

struct UniqueJson(serde_json::Value);

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

struct UniqueJsonVisitor;

impl<'de> Visitor<'de> for UniqueJsonVisitor {
    type Value = UniqueJson;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .map(UniqueJson)
            .ok_or_else(|| E::custom("non_finite_json_number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        UniqueJson::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueJson>()? {
            values.push(value.0);
        }
        Ok(UniqueJson(serde_json::Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some((key, value)) = object.next_entry::<String, UniqueJson>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(DUPLICATE_JSON_KEY));
            }
            values.insert(key, value.0);
        }
        Ok(UniqueJson(serde_json::Value::Object(values)))
    }
}

fn parse_unique_json(document: &str, prefix: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str::<UniqueJson>(document)
        .map(|value| value.0)
        .map_err(|error| {
            if error.to_string().contains(DUPLICATE_JSON_KEY) {
                format!("{prefix}_duplicate_key")
            } else {
                format!("{prefix}_invalid_json")
            }
        })
}

fn contains_external_ref(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values.iter().any(contains_external_ref),
        serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
            if matches!(key.as_str(), "$ref" | "$dynamicRef" | "$recursiveRef") {
                return value
                    .as_str()
                    .map(|reference| reference != "#" && !reference.starts_with("#/"))
                    .unwrap_or(true);
            }
            contains_external_ref(value)
        }),
        _ => false,
    }
}

fn validate_schema_identity(schema_json: &str, schema: &serde_json::Value) -> Result<(), String> {
    if schema.get("$id").is_some()
        && schema.get("$id").and_then(serde_json::Value::as_str) != Some(INSTALL_SOP_SCHEMA_ID)
    {
        return Err("install_sop_schema_identity_mismatch".to_owned());
    }
    if schema.get("$schema").is_some()
        && schema.get("$schema").and_then(serde_json::Value::as_str)
            != Some(INSTALL_SOP_SCHEMA_DIALECT)
    {
        return Err("install_sop_schema_dialect_mismatch".to_owned());
    }
    if contains_external_ref(schema) {
        return Err("install_sop_schema_external_ref".to_owned());
    }

    let digest: String = Sha256::digest(schema_json.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != INSTALL_SOP_SCHEMA_SHA256 {
        return Err("install_sop_schema_digest_mismatch".to_owned());
    }
    if schema.get("$id").and_then(serde_json::Value::as_str) != Some(INSTALL_SOP_SCHEMA_ID) {
        return Err("install_sop_schema_identity_mismatch".to_owned());
    }
    if schema.get("$schema").and_then(serde_json::Value::as_str) != Some(INSTALL_SOP_SCHEMA_DIALECT)
    {
        return Err("install_sop_schema_dialect_mismatch".to_owned());
    }
    Ok(())
}

fn bounded_text(value: &str, limit: usize) -> String {
    let mut output = String::new();
    for character in value.chars() {
        let character = if character.is_control() {
            '?'
        } else {
            character
        };
        if output.len() + character.len_utf8() > limit.saturating_sub(1) {
            output.push('~');
            return output;
        }
        output.push(character);
    }
    output
}

fn bounded_pointer(value: &str) -> String {
    if value.is_empty() {
        "/".to_owned()
    } else {
        bounded_text(value, MAX_PATH_BYTES)
    }
}

fn bounded_keyword(schema_path: &str) -> String {
    let keyword = schema_path.rsplit('/').next().unwrap_or("unknown");
    let keyword: String = keyword
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '$') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let keyword = bounded_text(&keyword, MAX_KEYWORD_BYTES);
    if keyword.is_empty() {
        "unknown".to_owned()
    } else {
        keyword
    }
}

fn validation_diagnostics(
    validator: &jsonschema::Validator,
    instance: &serde_json::Value,
) -> Vec<String> {
    let mut diagnostics = Vec::new();
    let mut total_bytes = 0;

    for error in validator.iter_errors(instance) {
        let instance_path = bounded_pointer(&error.instance_path.to_string());
        let schema_path = bounded_pointer(&error.schema_path.to_string());
        let keyword = bounded_keyword(&error.schema_path.to_string());
        let diagnostic = format!(
            "code=schema_validation keyword={keyword} instance={instance_path} schema={schema_path}"
        );
        debug_assert!(diagnostic.len() <= MAX_DIAGNOSTIC_BYTES);

        if diagnostics.len() >= MAX_VALIDATION_ERRORS.saturating_sub(1)
            || total_bytes + diagnostic.len() + TRUNCATED_DIAGNOSTIC.len() > MAX_DIAGNOSTICS_BYTES
        {
            diagnostics.push(TRUNCATED_DIAGNOSTIC.to_owned());
            break;
        }
        total_bytes += diagnostic.len();
        diagnostics.push(diagnostic);
    }
    diagnostics
}

fn validate_install_sop_report_json(
    schema_json: &str,
    instance_json: &str,
) -> Result<Vec<String>, String> {
    let schema = parse_unique_json(schema_json, "install_sop_schema")?;
    validate_schema_identity(schema_json, &schema)?;
    let instance = parse_unique_json(instance_json, "install_sop_report")?;
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&schema)
        .map_err(|_| "install_sop_schema_compile_failed".to_owned())?;

    Ok(validation_diagnostics(&validator, &instance))
}

/// Validate report JSON against the byte-pinned Install SOP v1 schema.
///
/// This private native primitive accepts schema bytes so the Python package and
/// native wheel must agree on one canonical document before validation runs.
#[pyfunction]
fn _validate_install_sop_report_json(
    schema_json: &str,
    instance_json: &str,
) -> PyResult<Vec<String>> {
    validate_install_sop_report_json(schema_json, instance_json).map_err(PyValueError::new_err)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(_validate_install_sop_report_json, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_DIAGNOSTIC_BYTES, MAX_DIAGNOSTICS_BYTES, TRUNCATED_DIAGNOSTIC,
        validate_install_sop_report_json,
    };

    const INSTALL_SOP_SCHEMA: &str =
        include_str!("../../../python/dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json");

    #[test]
    fn reports_instance_and_schema_paths() {
        let report = r#"{
            "schema_version": 2,
            "status": "planned",
            "dcc_type": "example",
            "adapter_version": "1.2.3",
            "core_version": "0.20.23",
            "steps": [],
            "next_steps": [],
            "receipt_path": null,
            "verify": {
                "directly_usable": false,
                "failure_stage": null,
                "failure_reason": null
            }
        }"#;
        let errors = validate_install_sop_report_json(INSTALL_SOP_SCHEMA, report).unwrap();

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("instance=/schema_version"));
        assert!(errors[0].contains("schema=/properties/schema_version/const"));
        assert!(errors[0].contains("keyword=const"));
    }

    #[test]
    fn rejects_untrusted_install_sop_schema_documents() {
        assert_eq!(
            validate_install_sop_report_json(r#"{"type":"object"}"#, "{}"),
            Err("install_sop_schema_digest_mismatch".to_owned())
        );
        assert_eq!(
            validate_install_sop_report_json(r#"{"$id":"x","$id":"y"}"#, "{}"),
            Err("install_sop_schema_duplicate_key".to_owned())
        );
        assert_eq!(
            validate_install_sop_report_json(
                r##"{"$id":"https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json","$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"https://attacker.invalid/schema"}"##,
                "{}",
            ),
            Err("install_sop_schema_external_ref".to_owned())
        );
    }

    #[test]
    fn rejects_duplicate_report_object_keys() {
        assert_eq!(
            validate_install_sop_report_json(
                INSTALL_SOP_SCHEMA,
                r#"{"schema_version":1,"schema_version":2}"#,
            ),
            Err("install_sop_report_duplicate_key".to_owned())
        );
    }

    #[test]
    fn diagnostics_are_deterministic_bounded_and_value_free() {
        let private_value = format!("PRIVATE-VALUE-{}", "x".repeat(4096));
        let next_steps: Vec<_> = (0..48)
            .map(|index| {
                serde_json::json!({
                    "id": format!("step-{index}"),
                    "description": "Execute the install plan.",
                    "why": "Planning does not mutate the host.",
                    "command": ["dcc-mcp-example", " ".repeat(4096)]
                })
            })
            .collect();
        let report = serde_json::json!({
            "schema_version": 1,
            "status": private_value,
            "dcc_type": "example",
            "adapter_version": "1.2.3",
            "core_version": "0.20.23",
            "steps": [],
            "next_steps": next_steps,
            "receipt_path": null,
            "verify": {
                "directly_usable": false,
                "failure_stage": null,
                "failure_reason": null
            }
        })
        .to_string();

        let first = validate_install_sop_report_json(INSTALL_SOP_SCHEMA, &report).unwrap();
        let second = validate_install_sop_report_json(INSTALL_SOP_SCHEMA, &report).unwrap();

        assert_eq!(first, second);
        assert!(!first.is_empty());
        assert!(first.iter().all(|entry| !entry.contains("PRIVATE-VALUE")));
        assert!(
            first
                .iter()
                .all(|entry| entry.len() <= MAX_DIAGNOSTIC_BYTES)
        );
        assert!(first.iter().map(String::len).sum::<usize>() <= MAX_DIAGNOSTICS_BYTES);
        assert!(first.iter().all(|entry| {
            entry.contains("code=") && entry.contains("instance=") && entry.contains("schema=")
        }));
        assert_eq!(first.last().map(String::as_str), Some(TRUNCATED_DIAGNOSTIC));
    }
}
