//! Pure data types — no I/O, no HTTP, no file access.
//!
//! Re-exports all trace types for backward compatibility.

pub mod agent_context;
pub mod trace;

pub use trace::*;
