use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use super::TrafficCaptureError;

#[derive(Debug, Clone, Default)]
pub(super) struct TrafficRedactor {
    rules: Vec<RedactRule>,
}

impl TrafficRedactor {
    pub(super) fn from_document(
        document: Option<Vec<HashMap<String, String>>>,
    ) -> Result<Self, TrafficCaptureError> {
        let Some(document) = document else {
            return Ok(Self::default());
        };

        let rules = document
            .into_iter()
            .map(|rule| {
                if rule.len() != 1 {
                    return Err(TrafficCaptureError::InvalidRule(format!("{rule:?}")));
                }
                let (path, replacement) = rule.into_iter().next().expect("length checked");
                Ok(RedactRule {
                    path: parse_path(&path),
                    display_path: path,
                    replacement,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { rules })
    }

    pub(super) fn redact(&self, attributes: &mut Value) -> Vec<String> {
        let mut redacted_paths = Vec::new();
        for rule in &self.rules {
            if replace_path(attributes, &rule.path, &rule.replacement) {
                redacted_paths.push(rule.display_path.clone());
            }
        }
        redact_default_attribution_fields(attributes, &mut Vec::new(), &mut redacted_paths);
        redacted_paths
    }

    pub(super) fn snapshot(&self) -> TrafficRedactionSnapshot {
        TrafficRedactionSnapshot {
            rule_count: self.rules.len(),
            paths: self
                .rules
                .iter()
                .map(|rule| rule.display_path.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
struct RedactRule {
    path: Vec<String>,
    display_path: String,
    replacement: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrafficRedactionSnapshot {
    pub rule_count: usize,
    pub paths: Vec<String>,
}

fn parse_path(path: &str) -> Vec<String> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn replace_path(value: &mut Value, path: &[String], replacement: &str) -> bool {
    let Some((last, parents)) = path.split_last() else {
        return false;
    };

    let mut cursor = value;
    for segment in parents {
        let Some(next) = cursor.as_object_mut().and_then(|map| map.get_mut(segment)) else {
            return false;
        };
        cursor = next;
    }

    let Some(slot) = cursor.as_object_mut().and_then(|map| map.get_mut(last)) else {
        return false;
    };
    *slot = Value::String(replacement.to_string());
    true
}

fn redact_default_attribution_fields(
    value: &mut Value,
    path: &mut Vec<String>,
    redacted_paths: &mut Vec<String>,
) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                path.push(key.clone());
                if is_sensitive_attribution_key(key) {
                    if !child.is_null() {
                        *child = Value::String("[REDACTED_ATTRIBUTION]".to_string());
                        redacted_paths.push(path.join("."));
                    }
                } else {
                    redact_default_attribution_fields(child, path, redacted_paths);
                }
                path.pop();
            }
        }
        Value::Array(items) => {
            for (idx, child) in items.iter_mut().enumerate() {
                path.push(idx.to_string());
                redact_default_attribution_fields(child, path, redacted_paths);
                path.pop();
            }
        }
        _ => {}
    }
}

fn is_sensitive_attribution_key(key: &str) -> bool {
    let normalised = key
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-' && *ch != ' ')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalised.as_str(),
        "actorid"
            | "actorname"
            | "actoremail"
            | "actoremailhash"
            | "authsubject"
            | "clienthost"
            | "forwardedfor"
            | "sourceip"
            | "userinputhash"
            | "agentreplyhash"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_exact_json_path() {
        let redactor = TrafficRedactor::from_document(Some(vec![HashMap::from([(
            "body.data.params.arguments.api_key".to_string(),
            "[REDACTED]".to_string(),
        )])]))
        .unwrap();
        let mut attrs = json!({
            "body": {
                "data": {
                    "params": {
                        "arguments": {
                            "api_key": "secret"
                        }
                    }
                }
            }
        });

        let redacted = redactor.redact(&mut attrs);

        assert_eq!(redacted, vec!["body.data.params.arguments.api_key"]);
        assert_eq!(
            attrs["body"]["data"]["params"]["arguments"]["api_key"],
            "[REDACTED]"
        );
    }

    #[test]
    fn redacts_attribution_fields_by_default() {
        let redactor = TrafficRedactor::default();
        let mut attrs = json!({
            "body": {
                "data": {
                    "params": {
                        "_meta": {
                            "agent_context": {
                                "actor_id": "artist-1",
                                "actor_name": "Morgan Artist",
                                "client_platform": "cursor",
                                "client_host": "workstation-7",
                                "auth_subject": "oauth:artist-1",
                                "source_ip": "203.0.113.10"
                            }
                        }
                    }
                }
            }
        });

        let redacted = redactor.redact(&mut attrs);

        assert_eq!(
            attrs["body"]["data"]["params"]["_meta"]["agent_context"]["actor_id"],
            "[REDACTED_ATTRIBUTION]"
        );
        assert_eq!(
            attrs["body"]["data"]["params"]["_meta"]["agent_context"]["client_platform"],
            "cursor"
        );
        assert!(
            redacted
                .iter()
                .any(|path| path.ends_with("agent_context.actor_id"))
        );
        assert!(
            redacted
                .iter()
                .any(|path| path.ends_with("agent_context.auth_subject"))
        );
    }
}
