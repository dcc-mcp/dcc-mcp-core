//! `dcc-mcp-cli host` command surface.
//!
//! `list` and `doctor` are the phase-1 read-only half: they report whether a
//! host binary exists and whether its version clears the manifest baseline, and
//! they never write to disk or spawn an installer. `install` and `pin` are the
//! phase-2 self-provisioning half and are declared here so the surface is one
//! coherent tree.

use std::path::PathBuf;

use anyhow::Context;
use serde_json::Value;

use crate::application::host::detect::HostEnv;
use crate::application::host::manifest::{self, HostLock, HostManifest};
use crate::application::host::{doctor, doctor_value, has_unavailable, manifest as host_manifest};

#[derive(Debug, clap::Subcommand)]
pub(crate) enum HostAction {
    /// List every host in the manifest with its licence class and pinned version.
    List,
    /// Report host availability. Read-only: never installs anything.
    Doctor {
        /// Host specs such as `blender`, `blender==5.1`, `blender>=5.1`.
        /// Omit to probe every known host.
        #[arg(value_name = "SPEC")]
        specs: Vec<String>,
    },
    /// Provision an open-source host at the manifest-pinned version.
    Install {
        /// Host spec, e.g. `blender==5.1.1`. Defaults to the pinned version.
        #[arg(value_name = "SPEC")]
        spec: String,
        /// Skip the consent prompt.
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Record a pinned host version in the user-level lock file.
    Pin {
        /// Host spec carrying the version to pin, e.g. `blender==5.1.1`.
        #[arg(value_name = "SPEC")]
        spec: String,
    },
}

/// Default user-level lock location, overridable for tests and studios.
const LOCK_ENV: &str = "DCC_MCP_HOSTS_LOCK";

/// Run the `host` command tree.
pub(crate) fn run(action: &HostAction) -> anyhow::Result<HostRun> {
    match action {
        HostAction::List => Ok(HostRun {
            value: load_manifest()?.to_list_value(),
            failed: false,
        }),
        HostAction::Doctor { specs } => {
            let manifest = load_manifest()?;
            let probes = doctor(specs, &manifest, &HostEnv::from_process());
            let failed = has_unavailable(&probes);
            Ok(HostRun {
                value: doctor_value(&probes),
                failed,
            })
        }
        HostAction::Install { .. } | HostAction::Pin { .. } => {
            // Phase 2 lands in its own PR. Refusing here keeps the surface
            // honest instead of silently doing nothing behind a parsed flag.
            anyhow::bail!(
                "host install / host pin are not wired yet; use `host doctor` to report availability"
            )
        }
    }
}

/// Result of one `host` invocation.
pub(crate) struct HostRun {
    pub value: Value,
    /// Whether any probed host is unusable, which drives the exit code.
    pub failed: bool,
}

/// Load the manifest with the user-level lock applied.
fn load_manifest() -> anyhow::Result<HostManifest> {
    let lock = load_lock()?;
    host_manifest::load_with_lock(&lock).context("failed to load the host manifest")
}

/// Read the lock file named by `DCC_MCP_HOSTS_LOCK`, or the default location.
fn load_lock() -> anyhow::Result<HostLock> {
    let path = std::env::var_os(LOCK_ENV)
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|dir| dir.join("dcc-mcp").join("hosts.lock")));
    manifest::load_lock(path.as_deref()).context("failed to read the host version lock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_reports_the_manifest_without_probing() {
        let HostRun { value, failed } = run(&HostAction::List).unwrap();
        assert!(!failed);
        let hosts = value["hosts"].as_array().expect("hosts array");
        assert!(!hosts.is_empty());
        assert!(hosts.iter().any(|host| host["id"] == "blender"));
    }

    #[test]
    fn doctor_of_an_absent_host_is_missing() {
        // The test environment has no `definitely-not-a-host` binary, and the
        // probe must not create one.
        let HostRun { value, failed } = run(&HostAction::Doctor {
            specs: vec!["definitely-not-a-host".to_string()],
        })
        .unwrap();
        assert!(failed);
        assert_eq!(value["hosts"][0]["status"], "unknown_host");
        assert_eq!(value["read_only"], true);
    }

    #[test]
    fn install_and_pin_are_explicitly_unwired() {
        for action in [
            HostAction::Install {
                spec: "blender".to_string(),
                yes: false,
            },
            HostAction::Pin {
                spec: "blender==5.1.1".to_string(),
            },
        ] {
            assert!(run(&action).is_err(), "{action:?} must not silently no-op");
        }
    }
}
