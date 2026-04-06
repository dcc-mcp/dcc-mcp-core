//! Mock DCC adapter — a fully functional mock for development and testing.
//!
//! Provides [`MockDccAdapter`], a configurable mock that implements all DCC adapter
//! traits ([`DccConnection`], [`DccScriptEngine`], [`DccSceneInfo`], [`DccSnapshot`]).
//! Designed for:
//!
//! - **Unit testing** without a real DCC application
//! - **Integration testing** of the MCP server → core → adapter pipeline
//! - **Development** of new DCC integrations (use as a reference implementation)
//! - **CI/CD** environments where no DCC is available
//!
//! # Quick Start
//!
//! ```rust
//! use dcc_mcp_protocols::mock::{MockDccAdapter, MockConfig};
//!
//! // Create with defaults (a "maya" mock)
//! let mut adapter = MockDccAdapter::new();
//!
//! // Or customize
//! let config = MockConfig::builder()
//!     .dcc_type("blender")
//!     .version("4.1.0")
//!     .python_version("3.11.0")
//!     .platform("linux")
//!     .build();
//! let mut adapter = MockDccAdapter::with_config(config);
//! ```
//!
//! # Script Execution
//!
//! The mock adapter executes scripts by returning the script source as output.
//! You can inject custom behavior via [`MockConfig::script_handler`]:
//!
//! ```rust
//! use dcc_mcp_protocols::mock::MockConfig;
//! use dcc_mcp_protocols::adapters::ScriptLanguage;
//!
//! let config = MockConfig::builder()
//!     .script_handler(|code, lang, _timeout| {
//!         if code.contains("error") {
//!             Err("Simulated error".to_string())
//!         } else {
//!             Ok(format!("[{}] {}", lang, code))
//!         }
//!     })
//!     .build();
//! ```

mod adapter;
mod config;
mod helpers;

#[cfg(test)]
mod tests;

pub use adapter::MockDccAdapter;
pub use config::{MockConfig, MockConfigBuilder, ScriptHandler};
