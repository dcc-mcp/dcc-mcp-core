//! Gateway admin SQLite — path resolution and location policy (DIP for embedders).

use std::path::PathBuf;

use crate::domain::env::{ENV_GATEWAY_ADMIN_DB, ENV_REGISTRY_DIR, GATEWAY_ADMIN_SQLITE_FILENAME};

/// Default path: the platform-local persistent DCC-MCP state directory.
///
/// An explicit registry hint is still honored for embedders that intentionally
/// colocate state. The registry environment variable is only a last-resort
/// fallback when the platform state directory cannot be resolved.
#[must_use]
pub fn default_gateway_admin_sqlite_path(registry_dir: Option<&PathBuf>) -> PathBuf {
    let base = registry_dir
        .cloned()
        .or_else(|| {
            dcc_mcp_paths::get_platform_dir("state")
                .ok()
                .map(PathBuf::from)
        })
        .or_else(|| std::env::var_os(ENV_REGISTRY_DIR).map(PathBuf::from))
        .unwrap_or_else(|| std::env::temp_dir().join("dcc-mcp-registry"));
    base.join(GATEWAY_ADMIN_SQLITE_FILENAME)
}

/// Resolve path: explicit config → `DCC_MCP_GATEWAY_ADMIN_DB` → [`default_gateway_admin_sqlite_path`].
#[must_use]
pub fn resolve_gateway_admin_sqlite_path(
    explicit: Option<&PathBuf>,
    registry_dir: Option<&PathBuf>,
) -> PathBuf {
    if let Some(p) = explicit {
        return p.clone();
    }
    if let Some(p) = std::env::var_os(ENV_GATEWAY_ADMIN_DB) {
        return PathBuf::from(p);
    }
    default_gateway_admin_sqlite_path(registry_dir)
}

/// Strategy interface for **where** the gateway admin database lives (tests / embedders).
pub trait GatewayAdminDbLocationPolicy: Send + Sync {
    /// Return the absolute or process-relative SQLite path.
    fn resolve_gateway_admin_sqlite(&self) -> PathBuf;
}

/// Policy used by `dcc-mcp-server` / gateway: optional explicit path + registry hint.
#[derive(Clone, Default, Debug)]
pub struct EnvAndRegistryGatewayAdminPolicy {
    pub explicit: Option<PathBuf>,
    pub registry_dir: Option<PathBuf>,
}

impl GatewayAdminDbLocationPolicy for EnvAndRegistryGatewayAdminPolicy {
    fn resolve_gateway_admin_sqlite(&self) -> PathBuf {
        resolve_gateway_admin_sqlite_path(self.explicit.as_ref(), self.registry_dir.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_wins_over_everything() {
        let explicit = PathBuf::from("/tmp/explicit.sqlite");
        let got = resolve_gateway_admin_sqlite_path(Some(&explicit), None);
        assert_eq!(got, explicit);
    }

    #[test]
    fn explicit_registry_hint_is_still_honored() {
        let reg = PathBuf::from("/tmp/reg-test-only");
        let got = resolve_gateway_admin_sqlite_path(None, Some(&reg));
        assert!(got.ends_with(GATEWAY_ADMIN_SQLITE_FILENAME));
        assert!(got.starts_with(&reg));
    }

    #[test]
    fn default_without_registry_uses_persistent_application_state() {
        let state_dir = PathBuf::from(
            dcc_mcp_paths::get_platform_dir("state").expect("resolve application state directory"),
        );

        assert_eq!(
            default_gateway_admin_sqlite_path(None),
            state_dir.join(GATEWAY_ADMIN_SQLITE_FILENAME)
        );
    }
}
