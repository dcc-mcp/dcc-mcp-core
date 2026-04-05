//! `dcc-mcp-usd` — USD (Universal Scene Description) support for dcc-mcp-core.
//!
//! This crate provides:
//!
//! - **Core USD types**: `SdfPath`, `VtValue`, `UsdAttribute`, `UsdPrim`,
//!   `UsdLayer`, `UsdStage` — pure Rust representations of the fundamental
//!   USD data model.
//! - **USDA serialization**: every `UsdStage` can be exported as human-readable
//!   `.usda` text that any OpenUSD-compatible tool can read.
//! - **JSON transport**: stages serialize to/from compact JSON for efficient
//!   transfer over the MCP IPC layer.
//! - **DCC bridge**: convert between `dcc-mcp-protocols` `SceneInfo` and
//!   `UsdStage`, enabling cross-DCC scene exchange.
//! - **`DccSceneInfo` impl**: `UsdStage` implements the standard scene info
//!   trait so it can be queried through the unified adapter interface.
//! - **PyO3 bindings**: `UsdStage`, `UsdPrim`, `SdfPath`, `VtValue` exposed
//!   to Python.
//!
//! # Relationship to OpenUSD
//!
//! This crate does **not** link against the OpenUSD C++ library.  It provides
//! a compatible data model and serialization format suitable for lightweight
//! scene description exchange in the DCC-MCP ecosystem.  When `usd-rs`
//! stabilizes (tracking: https://github.com/vfx-rs/usd-rs), this crate can be
//! extended with direct C++ bridging while keeping the same Rust API.
//!
//! # Quick start
//!
//! ```rust
//! use dcc_mcp_usd::stage::UsdStage;
//! use dcc_mcp_usd::types::{SdfPath, VtValue};
//!
//! let mut stage = UsdStage::new("my_scene");
//! stage.define_prim(SdfPath::new("/World").unwrap(), "Xform");
//! stage.define_prim(SdfPath::new("/World/Cube").unwrap(), "Mesh");
//! stage.set_attribute("/World/Cube", "extent", VtValue::Vec3f(1.0, 1.0, 1.0)).unwrap();
//!
//! // Export as USDA text
//! let usda = stage.export_usda();
//! assert!(usda.contains("#usda 1.0"));
//!
//! // Round-trip via JSON
//! let json = stage.to_json().unwrap();
//! let back = UsdStage::from_json(&json).unwrap();
//! assert!(back.has_prim("/World/Cube"));
//! ```

pub mod bridge;
pub mod error;
pub mod scene_info_impl;
pub mod stage;
pub mod types;

#[cfg(feature = "python-bindings")]
pub mod python;

pub use error::{UsdError, UsdResult};
pub use stage::UsdStage;
pub use types::{SdfPath, UsdAttribute, UsdLayer, UsdPrim, UsdStageMetrics, VtValue};
