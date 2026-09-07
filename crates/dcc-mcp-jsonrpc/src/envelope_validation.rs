//! Validation of optional final-revision envelope fields; no session state.

use crate::envelope::{
    CLIENT_CAPABILITIES_META_KEY, CLIENT_INFO_META_KEY, EnvelopeIssue, LOG_LEVEL_META_KEY,
};
use serde_json::{Map, Value};

pub(crate) fn is_safe_integer(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|v| v.fract() == 0.0 && v.abs() <= 9_007_199_254_740_991.0)
}

fn object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, EnvelopeIssue> {
    value
        .as_object()
        .ok_or_else(|| EnvelopeIssue::new(path, "expected an object"))
}

fn optional_field(
    map: &Map<String, Value>,
    key: &str,
    path: &str,
    valid: fn(&Value) -> bool,
) -> Result<(), EnvelopeIssue> {
    if map.get(key).is_some_and(|value| !valid(value)) {
        return Err(EnvelopeIssue::new(
            format!("{path}.{key}"),
            "invalid field type",
        ));
    }
    Ok(())
}

fn validate_identity(value: &Value) -> Result<(), EnvelopeIssue> {
    let path = CLIENT_INFO_META_KEY;
    let info = object(value, path)?;
    for key in ["name", "version"] {
        if !info.get(key).is_some_and(Value::is_string) {
            return Err(EnvelopeIssue::new(
                format!("{path}.{key}"),
                "expected a string",
            ));
        }
    }
    for key in ["title", "description", "websiteUrl"] {
        optional_field(info, key, path, Value::is_string)?;
    }
    if let Some(icons) = info.get("icons") {
        let icons = icons
            .as_array()
            .ok_or_else(|| EnvelopeIssue::new(format!("{path}.icons"), "expected an array"))?;
        for (index, icon) in icons.iter().enumerate() {
            let path = format!("{path}.icons.{index}");
            let icon = object(icon, &path)?;
            if !icon.get("src").is_some_and(Value::is_string) {
                return Err(EnvelopeIssue::new(
                    format!("{path}.src"),
                    "expected a string",
                ));
            }
            optional_field(icon, "mimeType", &path, Value::is_string)?;
            optional_field(icon, "sizes", &path, |value| {
                value
                    .as_array()
                    .is_some_and(|a| a.iter().all(Value::is_string))
            })?;
            optional_field(icon, "theme", &path, |value| {
                matches!(value.as_str(), Some("light" | "dark"))
            })?;
        }
    }
    Ok(())
}

fn validate_capabilities(caps: &Map<String, Value>) -> Result<(), EnvelopeIssue> {
    let path = CLIENT_CAPABILITIES_META_KEY;
    for key in ["experimental", "extensions"] {
        if let Some(value) = caps.get(key) {
            let path = format!("{path}.{key}");
            for (name, value) in object(value, &path)? {
                object(value, &format!("{path}.{name}"))?;
            }
        }
    }
    for key in ["sampling", "elicitation", "roots"] {
        if let Some(value) = caps.get(key) {
            let path = format!("{path}.{key}");
            let value = object(value, &path)?;
            match key {
                "sampling" => {
                    for field in ["context", "tools"] {
                        optional_field(value, field, &path, Value::is_object)?;
                    }
                }
                "elicitation" => {
                    optional_field(value, "url", &path, Value::is_object)?;
                    if let Some(form) = value.get("form") {
                        let path = format!("{path}.form");
                        optional_field(
                            object(form, &path)?,
                            "applyDefaults",
                            &path,
                            Value::is_boolean,
                        )?;
                    }
                }
                "roots" => optional_field(value, "listChanged", &path, Value::is_boolean)?,
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_optional_fields(
    meta: &Map<String, Value>,
    caps: &Map<String, Value>,
) -> Result<(), EnvelopeIssue> {
    if let Some(info) = meta.get(CLIENT_INFO_META_KEY) {
        validate_identity(info)?;
    }
    validate_capabilities(caps)?;
    if let Some(token) = meta.get("progressToken")
        && !token.is_string()
        && !is_safe_integer(token)
    {
        return Err(EnvelopeIssue::new(
            "progressToken",
            "expected a string or safe integer",
        ));
    }
    if let Some(level) = meta.get(LOG_LEVEL_META_KEY)
        && !matches!(
            level.as_str(),
            Some(
                "debug"
                    | "info"
                    | "notice"
                    | "warning"
                    | "error"
                    | "critical"
                    | "alert"
                    | "emergency"
            )
        )
    {
        return Err(EnvelopeIssue::new(LOG_LEVEL_META_KEY, "invalid log level"));
    }
    Ok(())
}
