//! Shared gateway inventory instance selection helpers.
//!
//! Remote CLI commands consume `/v1/instances` and `/v1/readyz` rows. Keep the
//! DCC filter, UUID-prefix matching, and ambiguity errors in one place so
//! `wait-ready`, `reload-skills`, and future direct instance commands agree.

use std::fmt;

use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InstanceSelectionError {
    PrefixTooShort { instance_id: String },
    Ambiguous { candidates: Vec<Value> },
}

impl InstanceSelectionError {
    pub(crate) fn to_json(&self) -> Value {
        match self {
            Self::PrefixTooShort { instance_id } => json!({
                "kind": "instance-id-prefix-too-short",
                "instance_id": instance_id,
                "min_len": 4,
            }),
            Self::Ambiguous { candidates } => json!({
                "kind": "ambiguous-instance",
                "candidates": candidates.iter().map(instance_summary).collect::<Vec<_>>(),
            }),
        }
    }
}

impl fmt::Display for InstanceSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrefixTooShort { instance_id } => {
                write!(
                    f,
                    "instance-id prefix '{instance_id}' must be at least 4 characters"
                )
            }
            Self::Ambiguous { candidates } => write!(
                f,
                "remote instance selection is ambiguous; candidates: {}",
                candidates
                    .iter()
                    .map(instance_label)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl std::error::Error for InstanceSelectionError {}

pub(crate) fn select_instances(
    payload: &Value,
    dcc_type: Option<&str>,
    instance_hint: Option<&str>,
) -> Result<Vec<Value>, InstanceSelectionError> {
    let hint = normalize_instance_hint(instance_hint)?;
    Ok(payload
        .get("instances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|instance| {
            dcc_type.is_none_or(|expected| {
                instance_field(instance, "dcc_type")
                    .or_else(|| instance_field(instance, "dcc"))
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
            })
        })
        .filter(|instance| {
            hint.as_deref()
                .is_none_or(|expected| instance_matches(instance, expected))
        })
        .cloned()
        .collect())
}

pub(crate) fn select_one_instance(
    payload: &Value,
    dcc_type: Option<&str>,
    instance_hint: Option<&str>,
) -> Result<Option<Value>, InstanceSelectionError> {
    let matches = select_instances(payload, dcc_type, instance_hint)?;
    match matches.as_slice() {
        [] => Ok(None),
        [instance] => Ok(Some(instance.clone())),
        _ => Err(InstanceSelectionError::Ambiguous {
            candidates: matches,
        }),
    }
}

pub(crate) fn instance_field<'a>(instance: &'a Value, key: &str) -> Option<&'a str> {
    instance
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn instance_label(instance: &Value) -> String {
    let dcc = instance_field(instance, "dcc_type")
        .or_else(|| instance_field(instance, "dcc"))
        .unwrap_or("-");
    let id = instance
        .get("instance_short")
        .and_then(Value::as_str)
        .or_else(|| instance_field(instance, "instance_id"))
        .unwrap_or("-");
    format!("{dcc}:{id}")
}

fn normalize_instance_hint(
    instance_hint: Option<&str>,
) -> Result<Option<String>, InstanceSelectionError> {
    let Some(hint) = instance_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if hint.len() < 4 {
        return Err(InstanceSelectionError::PrefixTooShort {
            instance_id: hint.to_ascii_lowercase(),
        });
    }
    Ok(Some(hint.to_ascii_lowercase()))
}

fn instance_matches(instance: &Value, expected: &str) -> bool {
    instance
        .get("instance_short")
        .and_then(Value::as_str)
        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        || instance_field(instance, "instance_id").is_some_and(|actual| {
            let actual = actual.to_ascii_lowercase();
            let simple = actual.replace('-', "");
            actual == expected || simple.starts_with(expected)
        })
}

fn instance_summary(instance: &Value) -> Value {
    json!({
        "dcc_type": instance.get("dcc_type").or_else(|| instance.get("dcc")).cloned().unwrap_or(Value::Null),
        "instance_id": instance.get("instance_id").cloned().unwrap_or(Value::Null),
        "instance_short": instance.get("instance_short").cloned().unwrap_or(Value::Null),
        "status": instance.get("status").cloned().unwrap_or(Value::Null),
        "mcp_url": instance.get("mcp_url").cloned().unwrap_or(Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_one_matches_short_id_or_uuid_prefix() {
        let payload = json!({
            "instances": [{
                "dcc_type": "maya",
                "instance_id": "abc12345-0000-0000-0000-000000000000",
                "instance_short": "abc12345"
            }]
        });

        assert_eq!(
            select_one_instance(&payload, Some("maya"), Some("abc12345"))
                .unwrap()
                .unwrap()["instance_short"],
            "abc12345"
        );
        assert!(
            select_one_instance(&payload, Some("maya"), Some("abc123450000"))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn select_instances_accepts_legacy_dcc_field() {
        let payload = json!({
            "instances": [{
                "dcc": "photoshop",
                "instance_id": "def67890-0000-0000-0000-000000000000"
            }]
        });

        let matches = select_instances(&payload, Some("photoshop"), None).unwrap();

        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn short_prefix_reports_structured_error() {
        let payload = json!({"instances": []});

        let error = select_one_instance(&payload, None, Some("abc")).unwrap_err();

        assert_eq!(error.to_json()["kind"], "instance-id-prefix-too-short");
        assert_eq!(error.to_json()["min_len"], 4);
    }

    #[test]
    fn ambiguous_selection_reports_candidates() {
        let payload = json!({
            "instances": [
                {"dcc_type": "maya", "instance_id": "abc10000-0000-0000-0000-000000000000", "instance_short": "abc10000"},
                {"dcc_type": "maya", "instance_id": "abc20000-0000-0000-0000-000000000000", "instance_short": "abc20000"}
            ]
        });

        let error = select_one_instance(&payload, Some("maya"), None).unwrap_err();

        assert_eq!(error.to_json()["kind"], "ambiguous-instance");
        assert_eq!(error.to_json()["candidates"].as_array().unwrap().len(), 2);
    }
}
