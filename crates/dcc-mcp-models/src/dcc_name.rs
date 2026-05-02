//! Typed DCC name enum (#491).
//!
//! Replaces stringly-typed `&str` / `String` DCC identifiers across the
//! workspace. Validates and normalises at the API boundary so internal
//! code can rely on case-stable, exhaustively-matchable variants.
//!
//! Unknown DCCs are tolerated via [`DccName::Other`] so the enum can
//! evolve without breaking external integrations.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Canonical DCC application identifier.
///
/// Wire form is the lowercase canonical name (`"maya"`, `"blender"`, …).
/// Round-trips through serde and `parse` / `to_string` losslessly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum DccName {
    /// Autodesk Maya
    Maya,
    /// Blender Foundation Blender
    Blender,
    /// SideFX Houdini
    Houdini,
    /// Autodesk 3ds Max
    ThreedsMax,
    /// Maxon Cinema 4D
    Cinema4d,
    /// Adobe Photoshop
    Photoshop,
    /// Pixologic ZBrush
    Zbrush,
    /// Epic Unreal Engine
    Unreal,
    /// Unity Editor
    Unity,
    /// Figma
    Figma,
    /// Foundry Nuke
    Nuke,
    /// Any DCC not covered by the canonical variants — preserves the
    /// caller-supplied lowercase string for forward-compat.
    Other(String),
}

impl DccName {
    /// Parse a DCC name with case-insensitive matching against the
    /// canonical aliases. Unknown values become [`DccName::Other`] with
    /// the lowercased input preserved.
    pub fn parse(s: &str) -> Self {
        let lower = s.trim().to_ascii_lowercase();
        match lower.as_str() {
            "maya" => Self::Maya,
            "blender" => Self::Blender,
            "houdini" => Self::Houdini,
            "3dsmax" | "max" | "threedsmax" => Self::ThreedsMax,
            "c4d" | "cinema4d" => Self::Cinema4d,
            "photoshop" | "ps" => Self::Photoshop,
            "zbrush" => Self::Zbrush,
            "unreal" | "ue" | "ue5" => Self::Unreal,
            "unity" => Self::Unity,
            "figma" => Self::Figma,
            "nuke" => Self::Nuke,
            _ => Self::Other(lower),
        }
    }

    /// Canonical wire/string representation (always lowercase, no spaces).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Maya => "maya",
            Self::Blender => "blender",
            Self::Houdini => "houdini",
            Self::ThreedsMax => "3dsmax",
            Self::Cinema4d => "c4d",
            Self::Photoshop => "photoshop",
            Self::Zbrush => "zbrush",
            Self::Unreal => "unreal",
            Self::Unity => "unity",
            Self::Figma => "figma",
            Self::Nuke => "nuke",
            Self::Other(s) => s.as_str(),
        }
    }

    /// `true` if this is one of the canonical (well-known) variants.
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Other(_))
    }
}

impl fmt::Display for DccName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DccName {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

impl From<&str> for DccName {
    fn from(s: &str) -> Self {
        Self::parse(s)
    }
}

impl From<String> for DccName {
    fn from(s: String) -> Self {
        Self::parse(&s)
    }
}

impl From<DccName> for String {
    fn from(d: DccName) -> Self {
        d.as_str().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_lowercase() {
        assert_eq!(DccName::parse("maya"), DccName::Maya);
        assert_eq!(DccName::parse("blender"), DccName::Blender);
        assert_eq!(DccName::parse("houdini"), DccName::Houdini);
        assert_eq!(DccName::parse("c4d"), DccName::Cinema4d);
        assert_eq!(DccName::parse("photoshop"), DccName::Photoshop);
        assert_eq!(DccName::parse("zbrush"), DccName::Zbrush);
        assert_eq!(DccName::parse("unreal"), DccName::Unreal);
        assert_eq!(DccName::parse("unity"), DccName::Unity);
        assert_eq!(DccName::parse("figma"), DccName::Figma);
        assert_eq!(DccName::parse("nuke"), DccName::Nuke);
    }

    #[test]
    fn parse_is_case_insensitive_and_trims() {
        assert_eq!(DccName::parse("MAYA"), DccName::Maya);
        assert_eq!(DccName::parse("  Blender  "), DccName::Blender);
        assert_eq!(DccName::parse("3DsMax"), DccName::ThreedsMax);
        assert_eq!(DccName::parse("PS"), DccName::Photoshop);
        assert_eq!(DccName::parse("UE5"), DccName::Unreal);
    }

    #[test]
    fn unknown_value_becomes_other_with_lowercase() {
        assert_eq!(DccName::parse("Krita"), DccName::Other("krita".into()));
        assert!(!DccName::parse("Krita").is_known());
    }

    #[test]
    fn round_trip_serde_known_values() {
        for known in [DccName::Maya, DccName::Blender, DccName::Cinema4d] {
            let json = serde_json::to_string(&known).unwrap();
            let back: DccName = serde_json::from_str(&json).unwrap();
            assert_eq!(known, back);
            assert_eq!(json, format!("\"{}\"", known.as_str()));
        }
    }

    #[test]
    fn round_trip_serde_other() {
        let unk = DccName::Other("modo".into());
        let json = serde_json::to_string(&unk).unwrap();
        let back: DccName = serde_json::from_str(&json).unwrap();
        assert_eq!(unk, back);
    }

    #[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
    struct GatewayRegistration {
        dcc_type: DccName,
        app_name: String,
        tools: Vec<String>,
    }

    #[test]
    fn serde_preserves_real_gateway_registration_dcc_types() {
        let payloads = [
            (
                r#"{"dcc_type":"photoshop","app_name":"Photoshop 2026","tools":["photoshop.layers.list"]}"#,
                DccName::Photoshop,
            ),
            (
                r#"{"dcc_type":"zbrush","app_name":"ZBrush 2026","tools":["zbrush.subtool.list"]}"#,
                DccName::Zbrush,
            ),
            (
                r#"{"dcc_type":"krita","app_name":"Krita Studio","tools":["krita.document.active"]}"#,
                DccName::Other("krita".into()),
            ),
        ];

        for (payload, expected_dcc) in payloads {
            let registration: GatewayRegistration = serde_json::from_str(payload).unwrap();
            assert_eq!(registration.dcc_type, expected_dcc);
            assert_eq!(
                serde_json::to_value(&registration).unwrap()["dcc_type"],
                serde_json::Value::String(registration.dcc_type.to_string())
            );
        }
    }

    #[test]
    fn display_uses_as_str() {
        assert_eq!(DccName::Maya.to_string(), "maya");
        assert_eq!(DccName::Zbrush.to_string(), "zbrush");
        assert_eq!(DccName::Unreal.to_string(), "unreal");
        assert_eq!(DccName::Unity.to_string(), "unity");
        assert_eq!(DccName::Figma.to_string(), "figma");
        assert_eq!(DccName::Other("krita".into()).to_string(), "krita");
    }

    #[test]
    fn from_str_infallible_returns_other_for_unknown() {
        let d: DccName = "wibble".parse().unwrap();
        assert_eq!(d, DccName::Other("wibble".into()));
    }
}
