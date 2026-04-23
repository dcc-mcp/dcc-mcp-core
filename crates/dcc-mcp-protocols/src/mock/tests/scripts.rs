use super::*;

#[test]
fn test_execute_script_echo() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    let result = adapter
        .execute_script("print('hello')", ScriptLanguage::Python, None)
        .unwrap();

    assert!(result.success);
    assert_eq!(result.output.as_deref(), Some("print('hello')"));
    assert_eq!(adapter.script_count(), 1);
}

#[test]
fn test_execute_script_unsupported_language() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    // Default mock only supports Python
    let result = adapter
        .execute_script("some mel code", ScriptLanguage::Mel, None)
        .unwrap();

    assert!(!result.success);
    assert!(result.error.as_deref().unwrap().contains("Unsupported"));
}

#[test]
fn test_execute_script_not_connected() {
    let adapter = MockDccAdapter::new();
    let result = adapter.execute_script("code", ScriptLanguage::Python, None);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, DccErrorCode::ConnectionFailed);
}

#[test]
fn test_custom_script_handler() {
    let config = MockConfig::builder()
        .script_handler(|code, _lang, _timeout| {
            if code.contains("error") {
                Err("Simulated error".to_string())
            } else {
                Ok(format!("result: {code}"))
            }
        })
        .build();
    let mut adapter = MockDccAdapter::with_config(config);
    adapter.connect().unwrap();

    let ok_result = adapter
        .execute_script("hello", ScriptLanguage::Python, None)
        .unwrap();
    assert!(ok_result.success);
    assert_eq!(ok_result.output.as_deref(), Some("result: hello"));

    let err_result = adapter
        .execute_script("trigger error", ScriptLanguage::Python, None)
        .unwrap();
    assert!(!err_result.success);
    assert!(err_result.error.as_deref().unwrap().contains("Simulated"));
}

#[test]
fn test_supported_languages() {
    let config = MockConfig::maya("2024");
    let adapter = MockDccAdapter::with_config(config);

    let langs = adapter.supported_languages();
    assert_eq!(langs.len(), 2);
    assert!(langs.contains(&ScriptLanguage::Python));
    assert!(langs.contains(&ScriptLanguage::Mel));
}
