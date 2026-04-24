//! `UsdAttribute` — typed, optionally time-sampled attribute on a prim.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::vt_value::VtValue;

/// A USD attribute on a prim.
///
/// Attributes hold typed values and can optionally be time-varying.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsdAttribute {
    /// Attribute name (e.g. `"xformOp:translate"`).
    pub name: String,
    /// The attribute's value at the default time.
    pub default_value: Option<VtValue>,
    /// Time-sampled values: frame → value.
    #[serde(default)]
    pub time_samples: HashMap<String, VtValue>,
    /// Whether this attribute is custom (not part of the schema).
    #[serde(default)]
    pub custom: bool,
    /// Metadata on the attribute.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl UsdAttribute {
    /// Create a new attribute with a default value.
    pub fn new(name: impl Into<String>, value: VtValue) -> Self {
        Self {
            name: name.into(),
            default_value: Some(value),
            time_samples: HashMap::new(),
            custom: false,
            metadata: HashMap::new(),
        }
    }

    /// Create a custom attribute (not in the schema).
    pub fn custom(name: impl Into<String>, value: VtValue) -> Self {
        Self {
            name: name.into(),
            default_value: Some(value),
            time_samples: HashMap::new(),
            custom: true,
            metadata: HashMap::new(),
        }
    }

    /// Get the value at a given time code.  Falls back to the default value.
    ///
    /// Keys are stored as decimal strings.  Both `"24"` and `"24.0"` are tried.
    pub fn get_at(&self, time_code: f64) -> Option<&VtValue> {
        // Try both "24.0" and "24" formats (Rust Display for f64 can vary)
        let key1 = format!("{time_code}");
        let key2 = if time_code.fract() == 0.0 {
            format!("{}", time_code as i64)
        } else {
            key1.clone()
        };
        self.time_samples
            .get(&key1)
            .or_else(|| self.time_samples.get(&key2))
            .or(self.default_value.as_ref())
    }
}
