//! `VtValue` — variant value type for USD attribute values.

use serde::{Deserialize, Serialize};

/// A variant value type representing USD attribute values.
///
/// This is a simplified version of USD's `VtValue` that covers the most
/// common attribute types encountered in DCC workflows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum VtValue {
    /// Boolean
    Bool(bool),
    /// 32-bit integer
    Int(i32),
    /// 64-bit integer
    Int64(i64),
    /// Single-precision float
    Float(f32),
    /// Double-precision float
    Double(f64),
    /// UTF-8 string
    String(String),
    /// USD asset path (a string referencing an external file)
    Asset(String),
    /// Token (enumeration-like identifier in USD)
    Token(String),
    /// 2D vector (x, y)
    Vec2f(f32, f32),
    /// 3D vector (x, y, z)
    Vec3f(f32, f32, f32),
    /// 4D vector (x, y, z, w)
    Vec4f(f32, f32, f32, f32),
    /// 4×4 matrix (row-major, 16 elements)
    Matrix4d([f64; 16]),
    /// Array of floats
    FloatArray(Vec<f32>),
    /// Array of integers
    IntArray(Vec<i32>),
    /// Array of 3D vectors
    Vec3fArray(Vec<[f32; 3]>),
    /// Array of strings
    StringArray(Vec<String>),
}

impl VtValue {
    /// Return the USD type name string (e.g. `"float3"`, `"token"`).
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Int64(_) => "int64",
            Self::Float(_) => "float",
            Self::Double(_) => "double",
            Self::String(_) => "string",
            Self::Asset(_) => "asset",
            Self::Token(_) => "token",
            Self::Vec2f(..) => "float2",
            Self::Vec3f(..) => "float3",
            Self::Vec4f(..) => "float4",
            Self::Matrix4d(_) => "matrix4d",
            Self::FloatArray(_) => "float[]",
            Self::IntArray(_) => "int[]",
            Self::Vec3fArray(_) => "float3[]",
            Self::StringArray(_) => "string[]",
        }
    }

    /// Try to extract a float value, promoting `Int` and `Double` if needed.
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Double(v) => Some(*v as f32),
            Self::Int(v) => Some(*v as f32),
            _ => None,
        }
    }

    /// Try to extract a string value (for `String`, `Token`, and `Asset`).
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::Token(s) | Self::Asset(s) => Some(s.as_str()),
            _ => None,
        }
    }
}
