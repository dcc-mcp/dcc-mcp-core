use super::*;

// ── Default ─────────────────────────────────────────────────────────────────

#[test]
fn test_action_result_default() {
    let data = ActionResultModelData::default();
    assert!(data.success); // Consistent with Python __new__ default (success=true)
    assert!(data.message.is_empty());
    assert!(data.prompt.is_none());
    assert!(data.error.is_none());
    assert!(data.context.is_empty());
}

#[test]
fn test_action_result_model_default_success_true() {
    let model = ActionResultModel::default();
    assert!(model.data().success);
}

// ── Factory methods ─────────────────────────────────────────────────────────

#[test]
fn test_success_factory() {
    let data = ActionResultModelData::success(
        "All done".to_string(),
        Some("Next: verify mesh".to_string()),
        HashMap::new(),
    );
    assert!(data.success);
    assert_eq!(data.message, "All done");
    assert_eq!(data.prompt.as_deref(), Some("Next: verify mesh"));
    assert!(data.error.is_none());
}

#[test]
fn test_failure_factory() {
    let data = ActionResultModelData::failure(
        "Operation failed".to_string(),
        Some("FileNotFoundError: missing.py".to_string()),
        Some("Check file path".to_string()),
        HashMap::new(),
    );
    assert!(!data.success);
    assert_eq!(data.message, "Operation failed");
    assert_eq!(data.error.as_deref(), Some("FileNotFoundError: missing.py"));
    assert_eq!(data.prompt.as_deref(), Some("Check file path"));
}

#[test]
fn test_failure_factory_no_error() {
    let data = ActionResultModelData::failure("Cancelled".to_string(), None, None, HashMap::new());
    assert!(!data.success);
    assert!(data.error.is_none());
    assert!(data.prompt.is_none());
}

// ── from_data / data accessor ───────────────────────────────────────────────

#[test]
fn test_from_data_round_trip() {
    let data = ActionResultModelData {
        success: false,
        message: "bad".to_string(),
        prompt: None,
        error: Some("oops".to_string()),
        context: HashMap::from([("key".to_string(), serde_json::Value::Bool(true))]),
    };
    let model = ActionResultModel::from_data(data.clone());
    assert_eq!(model.data(), &data);
}

// ── Display ─────────────────────────────────────────────────────────────────

#[test]
fn test_display_success() {
    let model = ActionResultModel::from_data(ActionResultModelData {
        success: true,
        message: "render complete".to_string(),
        ..Default::default()
    });
    let s = model.to_string();
    assert!(s.contains("Success"));
    assert!(s.contains("render complete"));
}

#[test]
fn test_display_failure_with_error() {
    let model = ActionResultModel::from_data(ActionResultModelData {
        success: false,
        message: "operation failed".to_string(),
        error: Some("TypeError: bad arg".to_string()),
        ..Default::default()
    });
    let s = model.to_string();
    assert!(s.contains("Error"));
    assert!(s.contains("TypeError: bad arg"));
}

#[test]
fn test_display_failure_uses_message_when_no_error() {
    let model = ActionResultModel::from_data(ActionResultModelData {
        success: false,
        message: "something went wrong".to_string(),
        error: None,
        ..Default::default()
    });
    let s = model.to_string();
    assert!(s.contains("something went wrong"));
}

// ── Serialization ───────────────────────────────────────────────────────────

#[test]
fn test_action_result_serialization() {
    let data = ActionResultModelData {
        success: true,
        message: "test".to_string(),
        prompt: Some("next step".to_string()),
        error: None,
        context: HashMap::new(),
    };
    let json = serde_json::to_string(&data).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"message\":\"test\""));
    assert!(json.contains("\"prompt\":\"next step\""));
    // Optional None fields are skipped
    assert!(!json.contains("\"error\""));
}

#[test]
fn test_action_result_deserialization_round_trip() {
    let data = ActionResultModelData {
        success: false,
        message: "failed operation".to_string(),
        prompt: None,
        error: Some("IOError: file not found".to_string()),
        context: HashMap::from([("retry".to_string(), serde_json::json!(true))]),
    };
    let json = serde_json::to_string(&data).unwrap();
    let back: ActionResultModelData = serde_json::from_str(&json).unwrap();
    assert_eq!(data, back);
}

#[test]
fn test_action_result_deserialize_minimal_json() {
    // Only 'message' provided, everything else defaults
    let json = r#"{"message": "hello"}"#;
    let data: ActionResultModelData = serde_json::from_str(json).unwrap();
    assert!(data.success); // default = true
    assert_eq!(data.message, "hello");
    assert!(data.error.is_none());
}

// ── Context ─────────────────────────────────────────────────────────────────

#[test]
fn test_context_with_nested_values() {
    let mut ctx = HashMap::new();
    ctx.insert("count".to_string(), serde_json::json!(42));
    ctx.insert("nested".to_string(), serde_json::json!({"a": 1}));
    ctx.insert("list".to_string(), serde_json::json!([1, 2, 3]));

    let data = ActionResultModelData::success("ctx test".to_string(), None, ctx.clone());
    assert_eq!(data.context["count"], serde_json::json!(42));
    assert_eq!(data.context["list"], serde_json::json!([1, 2, 3]));
}

// ── PartialEq / Clone ────────────────────────────────────────────────────────

#[test]
fn test_action_result_clone_eq() {
    let data = ActionResultModelData {
        success: true,
        message: "ok".to_string(),
        ..Default::default()
    };
    let cloned = data.clone();
    assert_eq!(data, cloned);
}

#[test]
fn test_action_result_model_clone_eq() {
    let model = ActionResultModel::from_data(ActionResultModelData {
        success: false,
        message: "err".to_string(),
        ..Default::default()
    });
    let cloned = model.clone();
    assert_eq!(model, cloned);
}
