//! Infrastructure: DDL and (future) drivers. Domain must not import from here.

pub mod file_log_merge;
pub mod gateway_admin_schema;
#[cfg(feature = "gateway-admin-sqlite")]
pub mod gateway_admin_sqlite;
