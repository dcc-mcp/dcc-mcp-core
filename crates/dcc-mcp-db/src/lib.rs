//! # dcc-mcp-db
//!
//! Single place for **persistence contracts** and **shared primitives** across
//! DCC-MCP (gateway admin SQLite, future log/stats materialisation, job stores).
//!
//! ## Architecture
//!
//! - **domain** — value objects, env key names, and `DbError` (no I/O).
//! - **application** — pure policies (path resolution) and `GatewayAdminDbLocationPolicy`.
//! - **infra** — versioned DDL strings, SQLite adapters, and file-log merge helpers.
//!
//! Gateway and server crates depend on this package instead of duplicating
//! environment variable names or schema text. The admin SQLite writer thread
//! lives behind the `gateway-admin-sqlite` feature.

pub mod application;
pub mod domain;
pub mod infra;

pub use application::gateway_admin::{
    EnvAndRegistryGatewayAdminPolicy, GatewayAdminDbLocationPolicy,
    default_gateway_admin_sqlite_path, resolve_gateway_admin_sqlite_path,
};
pub use domain::env;
pub use domain::error::DbError;
pub use domain::gateway_admin_audit::GatewayAdminAuditPersistedJson;
pub use domain::gateway_admin_deregistered::GatewayDeregisteredInstanceJson;
pub use infra::file_log_merge::{
    default_gateway_log_dir, parse_gateway_file_log_line, read_gateway_log_dir_rows_recent,
};
pub use infra::gateway_admin_schema::GATEWAY_ADMIN_SQLITE_DDL;

#[cfg(feature = "gateway-admin-sqlite")]
pub use infra::gateway_admin_sqlite::{
    GatewayAdminSqliteLane, GatewayAdminSqliteReader, read_custom_skill_paths_for_startup,
};
