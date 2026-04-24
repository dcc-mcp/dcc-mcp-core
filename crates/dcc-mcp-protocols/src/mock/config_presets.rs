use crate::adapters::{SceneInfo, ScriptLanguage};

use super::MockConfig;

/// Preset mock configurations for common DCC types.
impl MockConfig {
    /// Create a Maya mock configuration.
    #[must_use]
    pub fn maya(version: &str) -> Self {
        Self {
            dcc_type: "maya".to_string(),
            version: version.to_string(),
            python_version: Some("3.10.11".to_string()),
            supported_languages: vec![ScriptLanguage::Python, ScriptLanguage::Mel],
            scene: SceneInfo {
                format: ".ma".to_string(),
                up_axis: Some("y".to_string()),
                units: Some("cm".to_string()),
                fps: Some(24.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a Blender mock configuration.
    #[must_use]
    pub fn blender(version: &str) -> Self {
        Self {
            dcc_type: "blender".to_string(),
            version: version.to_string(),
            python_version: Some("3.11.0".to_string()),
            supported_languages: vec![ScriptLanguage::Python],
            scene: SceneInfo {
                format: ".blend".to_string(),
                up_axis: Some("z".to_string()),
                units: Some("m".to_string()),
                fps: Some(24.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a Houdini mock configuration.
    #[must_use]
    pub fn houdini(version: &str) -> Self {
        Self {
            dcc_type: "houdini".to_string(),
            version: version.to_string(),
            python_version: Some("3.10.10".to_string()),
            supported_languages: vec![
                ScriptLanguage::Python,
                ScriptLanguage::HScript,
                ScriptLanguage::Vex,
            ],
            scene: SceneInfo {
                format: ".hip".to_string(),
                up_axis: Some("y".to_string()),
                units: Some("m".to_string()),
                fps: Some(24.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a 3ds Max mock configuration.
    #[must_use]
    pub fn max_3ds(version: &str) -> Self {
        Self {
            dcc_type: "3dsmax".to_string(),
            version: version.to_string(),
            python_version: Some("3.11.0".to_string()),
            supported_languages: vec![ScriptLanguage::Python, ScriptLanguage::MaxScript],
            scene: SceneInfo {
                format: ".max".to_string(),
                up_axis: Some("z".to_string()),
                units: Some("cm".to_string()),
                fps: Some(30.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create an Unreal Engine mock configuration (HTTP-based, no Python on DCC side).
    #[must_use]
    pub fn unreal(version: &str) -> Self {
        Self {
            dcc_type: "unreal".to_string(),
            version: version.to_string(),
            python_version: Some("3.11.0".to_string()),
            supported_languages: vec![ScriptLanguage::Python, ScriptLanguage::Blueprint],
            scene: SceneInfo {
                format: ".umap".to_string(),
                up_axis: Some("z".to_string()),
                units: Some("cm".to_string()),
                fps: Some(30.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a Unity mock configuration (no Python, C# only).
    #[must_use]
    pub fn unity(version: &str) -> Self {
        Self {
            dcc_type: "unity".to_string(),
            version: version.to_string(),
            python_version: None,
            supported_languages: vec![ScriptLanguage::CSharp],
            scene: SceneInfo {
                format: ".unity".to_string(),
                up_axis: Some("y".to_string()),
                units: Some("m".to_string()),
                fps: Some(60.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a Photoshop mock configuration (2D, image layers).
    #[must_use]
    pub fn photoshop(version: &str) -> Self {
        Self {
            dcc_type: "photoshop".to_string(),
            version: version.to_string(),
            python_version: Some("3.11.0".to_string()),
            supported_languages: vec![ScriptLanguage::Python],
            scene: SceneInfo {
                format: ".psd".to_string(),
                up_axis: None,
                units: Some("px".to_string()),
                fps: None,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
