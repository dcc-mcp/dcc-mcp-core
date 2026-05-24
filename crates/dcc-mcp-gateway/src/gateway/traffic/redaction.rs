use std::collections::HashMap;

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
        redacted_paths
    }
}

#[derive(Debug, Clone)]
struct RedactRule {
    path: Vec<String>,
    display_path: String,
    replacement: String,
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
}
