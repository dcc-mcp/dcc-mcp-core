//! Pure recording projections used by the admin recording compiler.

use serde_json::{Map, Value};

/// Resolve the logical UI session used to pair semantic find and act events.
#[must_use]
pub fn recording_ui_session(arguments: &Value) -> String {
    arguments
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_owned()
}

/// Retain only backend-neutral semantic locator fields from a UI find event.
#[must_use]
pub fn recording_semantic_query(arguments: &Value) -> Value {
    let mut query = Map::new();
    for key in ["query", "role", "label", "object_name"] {
        if let Some(value) = arguments.get(key) {
            query.insert(key.to_owned(), value.clone());
        }
    }
    Value::Object(query)
}

/// Build the default replay postcondition for a semantic UI action.
#[must_use]
pub fn recording_default_postcondition(query: &Value) -> Value {
    let mut condition = Map::from_iter([(
        "kind".to_owned(),
        Value::String("control_exists".to_owned()),
    )]);
    if let Some(fields) = query.as_object() {
        condition.extend(fields.clone());
    }
    Value::Object(condition)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn semantic_projection_preserves_supported_locator_fields() {
        let arguments = json!({
            "session_id": "maya-main",
            "query": "Render Settings",
            "role": "dialog",
            "label": "Render Settings",
            "object_name": "renderSettingsWindow",
            "coordinates": [120, 240],
        });

        let query = recording_semantic_query(&arguments);

        assert_eq!(recording_ui_session(&arguments), "maya-main");
        assert_eq!(
            query,
            json!({
                "query": "Render Settings",
                "role": "dialog",
                "label": "Render Settings",
                "object_name": "renderSettingsWindow",
            })
        );
        assert_eq!(
            recording_default_postcondition(&query),
            json!({
                "kind": "control_exists",
                "query": "Render Settings",
                "role": "dialog",
                "label": "Render Settings",
                "object_name": "renderSettingsWindow",
            })
        );
    }

    #[test]
    fn semantic_projection_is_backend_neutral_and_defaults_the_session() {
        let arguments = json!({
            "query": "Layers panel",
            "role": "panel",
            "dcc_type": "photoshop",
        });

        assert_eq!(recording_ui_session(&arguments), "default");
        assert_eq!(
            recording_semantic_query(&arguments),
            json!({"query": "Layers panel", "role": "panel"})
        );
        assert_eq!(
            recording_default_postcondition(&Value::Null),
            json!({"kind": "control_exists"})
        );
    }
}
