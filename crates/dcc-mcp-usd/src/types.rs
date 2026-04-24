//! Core USD data types.
//!
//! This module provides Rust-native representations of the fundamental USD
//! concepts: `SdfPath`, `VtValue`, `UsdAttribute`, `UsdPrim`, `UsdLayer`,
//! and `UsdStageMetrics`.
//!
//! # Why pure Rust instead of OpenUSD C++ bindings?
//!
//! OpenUSD C++ bindings (`usd-rs`) are still experimental and require a full
//! OpenUSD build, which is a 30-minute compile that is impractical for a
//! lightweight core library.  Instead we provide:
//!
//! - A **pure-Rust USD data model** sufficient for scene description exchange.
//! - **USDA (text) serialization**: every `UsdStage` can be written to / read
//!   from the human-readable `.usda` format.
//! - **JSON interop**: stages can be serialized to/from JSON for transport
//!   over the MCP IPC layer.
//! - A **`DccSceneInfo` bridge** so any DCC's scene info can be converted to
//!   a USD-compatible representation.
//!
//! Once `usd-rs` stabilizes, this module can be extended with C++ bridging
//! while keeping the same public API surface.
//!
//! ## Maintainer layout
//!
//! `types.rs` is a thin facade; every type lives in a focused sibling file:
//!
//! - [`sdf_path`] — [`SdfPath`]
//! - [`vt_value`] — [`VtValue`]
//! - [`attribute`] — [`UsdAttribute`]
//! - [`prim`] — [`UsdPrim`]
//! - [`layer`] — [`UsdLayer`]
//! - [`metrics`] — [`UsdStageMetrics`]
//! - `types_tests.rs` — unit tests (compiled only under `#[cfg(test)]`)
//!
//! All public symbols are re-exported so `dcc_mcp_usd::types::{SdfPath,
//! VtValue, UsdAttribute, UsdPrim, UsdLayer, UsdStageMetrics}` keep working
//! unchanged for downstream callers.

#[path = "types_sdf_path.rs"]
mod sdf_path;

#[path = "types_vt_value.rs"]
mod vt_value;

#[path = "types_attribute.rs"]
mod attribute;

#[path = "types_prim.rs"]
mod prim;

#[path = "types_layer.rs"]
mod layer;

#[path = "types_metrics.rs"]
mod metrics;

pub use attribute::UsdAttribute;
pub use layer::UsdLayer;
pub use metrics::UsdStageMetrics;
pub use prim::UsdPrim;
pub use sdf_path::SdfPath;
pub use vt_value::VtValue;

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
