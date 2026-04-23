use super::*;

#[test]
fn test_maya_preset() {
    let config = MockConfig::maya("2024.2");
    let adapter = MockDccAdapter::with_config(config);
    assert_eq!(adapter.info().dcc_type, "maya");
    assert_eq!(adapter.info().version, "2024.2");
    let caps = adapter.capabilities();
    assert!(caps.script_languages.contains(&ScriptLanguage::Python));
    assert!(caps.script_languages.contains(&ScriptLanguage::Mel));
}

#[test]
fn test_blender_preset() {
    let config = MockConfig::blender("4.1.0");
    let adapter = MockDccAdapter::with_config(config);
    assert_eq!(adapter.info().dcc_type, "blender");
    assert_eq!(adapter.capabilities().script_languages.len(), 1);
}

#[test]
fn test_houdini_preset() {
    let config = MockConfig::houdini("20.0.547");
    let adapter = MockDccAdapter::with_config(config);
    assert_eq!(adapter.info().dcc_type, "houdini");
    let caps = adapter.capabilities();
    assert_eq!(caps.script_languages.len(), 3);
    assert!(caps.script_languages.contains(&ScriptLanguage::Vex));
}

#[test]
fn test_max_3ds_preset() {
    let config = MockConfig::max_3ds("2025");
    let adapter = MockDccAdapter::with_config(config);
    assert_eq!(adapter.info().dcc_type, "3dsmax");
    assert!(
        adapter
            .capabilities()
            .script_languages
            .contains(&ScriptLanguage::MaxScript)
    );
}

#[test]
fn test_unreal_preset() {
    let config = MockConfig::unreal("5.4");
    let adapter = MockDccAdapter::with_config(config);
    assert_eq!(adapter.info().dcc_type, "unreal");
    assert!(
        adapter
            .capabilities()
            .script_languages
            .contains(&ScriptLanguage::Blueprint)
    );
}

#[test]
fn test_unity_preset() {
    let config = MockConfig::unity("2022.3");
    let adapter = MockDccAdapter::with_config(config);
    assert_eq!(adapter.info().dcc_type, "unity");
    assert!(adapter.info().python_version.is_none());
    assert!(
        adapter
            .capabilities()
            .script_languages
            .contains(&ScriptLanguage::CSharp)
    );
}
