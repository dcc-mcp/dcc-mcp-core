use std::collections::HashMap;

use serde_json::Value;

use super::TrafficCaptureError;
use super::config::TrafficFilterDocument;

#[derive(Debug, Clone, Default)]
pub(super) struct TrafficFilter {
    include: Vec<TrafficMatchRule>,
    exclude: Vec<TrafficMatchRule>,
}

impl TrafficFilter {
    pub(super) fn from_document(
        document: Option<TrafficFilterDocument>,
    ) -> Result<Self, TrafficCaptureError> {
        let Some(document) = document else {
            return Ok(Self::default());
        };
        Ok(Self {
            include: parse_rules(document.include.unwrap_or_default())?,
            exclude: parse_rules(document.exclude.unwrap_or_default())?,
        })
    }

    pub(super) fn allows(&self, attributes: &Value) -> bool {
        let included =
            self.include.is_empty() || self.include.iter().any(|rule| rule.matches(attributes));
        let excluded = self.exclude.iter().any(|rule| rule.matches(attributes));
        included && !excluded
    }
}

#[derive(Debug, Clone)]
struct TrafficMatchRule {
    path: Vec<String>,
    pattern: String,
}

impl TrafficMatchRule {
    fn matches(&self, attributes: &Value) -> bool {
        lookup_path(attributes, &self.path)
            .and_then(value_to_match_string)
            .is_some_and(|value| wildcard_matches(&self.pattern, &value))
    }
}

fn parse_rules(
    documents: Vec<HashMap<String, String>>,
) -> Result<Vec<TrafficMatchRule>, TrafficCaptureError> {
    documents
        .into_iter()
        .map(|rule| {
            if rule.len() != 1 {
                return Err(TrafficCaptureError::InvalidRule(format!("{rule:?}")));
            }
            let (path, pattern) = rule.into_iter().next().expect("length checked");
            Ok(TrafficMatchRule {
                path: parse_path(&path),
                pattern,
            })
        })
        .collect()
}

fn parse_path(path: &str) -> Vec<String> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn lookup_path<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor.as_object()?.get(segment)?;
    }
    Some(cursor)
}

fn value_to_match_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }

    let mut remaining = value;
    let mut first = true;
    for part in pattern.split('*').filter(|part| !part.is_empty()) {
        if first && !pattern.starts_with('*') {
            let Some(next) = remaining.strip_prefix(part) else {
                return false;
            };
            remaining = next;
        } else if let Some(index) = remaining.find(part) {
            remaining = &remaining[index + part.len()..];
        } else {
            return false;
        }
        first = false;
    }

    pattern.ends_with('*') || remaining.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wildcard_rules_match_prefix_and_suffix() {
        let attrs = json!({"http": {"url": "/foo/v1/readyz"}});
        let rule = TrafficMatchRule {
            path: parse_path("http.url"),
            pattern: "*/v1/readyz".to_string(),
        };

        assert!(rule.matches(&attrs));
    }
}
