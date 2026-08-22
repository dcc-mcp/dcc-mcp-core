//! Shared test utilities for the dcc-mcp-core workspace.
//!
//! # RAII environment variable guards
//!
//! Tests that touch process-global [`std::env::set_var`] / [`std::env::remove_var`]
//! must use [`EnvVarGuard`] or [`EnvVarsGuard`] to safely save/restore the
//! previous value and to serialise concurrent access within the same crate.
//!
//! # Feature-gated domain fixtures
//!
//! Enable `skill-rest` for the canonical in-memory catalog, recording
//! invoker, deterministic auth gate, audit sink, and real-router harness:
//!
//! ```ignore
//! use dcc_mcp_test_utils::skill_rest::{
//!     InMemorySkillCatalog, RecordingToolInvoker, SkillRestTestHarness,
//! };
//! ```
//!
//! Trait-specific implementations remain physically owned by
//! `dcc-mcp-skill-rest`; this crate re-exports them so workspace tests have one
//! fixture entry point without introducing a production dependency cycle.
//!
//! ```ignore
//! use dcc_mcp_test_utils::{EnvVarGuard, EnvVarsGuard};
//!
//! // Single var
//! let _g = EnvVarGuard::set("MY_VAR", Some("value"));
//!
//! // Multiple vars
//! let _g = EnvVarsGuard::set(&[("A", Some("1")), ("B", None)]);
//! ```

use std::sync::{Mutex, MutexGuard};

/// Per-DCC REST fixtures, re-exported from their trait owner to keep the
/// production dependency graph acyclic.
#[cfg(feature = "skill-rest")]
pub mod skill_rest {
    pub use dcc_mcp_skill_rest::testing::*;
}

/// Global lock that serialises all env-var mutations within this crate's
/// test binary.  Without this, concurrent tests that call `set_var` /
/// `remove_var` race on the process-global environment table.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard that sets an environment variable and restores the previous
/// value (or removes it) when dropped.
///
/// All mutations are serialised via [`ENV_LOCK`] so that concurrent tests
/// in the same binary do not race.
pub struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvVarGuard {
    /// Set `key` to `value` (or remove it when `value` is `None`), saving
    /// the previous value so it can be restored on drop.
    pub fn set(key: &'static str, value: Option<&str>) -> Self {
        let lock = ENV_LOCK.lock().expect("env lock poisoned");
        let previous = std::env::var(key).ok();
        // SAFETY: serialised by ENV_LOCK; the previous value is restored on
        // drop so no other test can observe the side-effect past this guard's
        // lifetime.
        unsafe {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        Self {
            key,
            previous,
            _lock: lock,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: we still hold ENV_LOCK (via _lock), so this is serialised.
        unsafe {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// RAII guard that sets multiple environment variables atomically and
/// restores all previous values when dropped.
pub struct EnvVarsGuard {
    previous: Vec<(&'static str, Option<String>)>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvVarsGuard {
    /// Set each var in `vars` to its corresponding value (or remove it
    /// when the value is `None`), saving all previous values.
    pub fn set(vars: &[(&'static str, Option<&str>)]) -> Self {
        let lock = ENV_LOCK.lock().expect("env lock poisoned");
        let previous = vars
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect::<Vec<_>>();
        // SAFETY: serialised by ENV_LOCK; previous values are restored on drop.
        unsafe {
            for (key, value) in vars {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for EnvVarsGuard {
    fn drop(&mut self) {
        // SAFETY: we still hold ENV_LOCK (via _lock), so this is serialised.
        unsafe {
            for (key, value) in &self.previous {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_var_set_and_restore() {
        let _g = EnvVarGuard::set("DCC_MCP_TEST_UTILS_SINGLE", Some("hello"));
        assert_eq!(std::env::var("DCC_MCP_TEST_UTILS_SINGLE").unwrap(), "hello");
    }

    #[test]
    fn single_var_remove_and_restore() {
        // Set it first so there's something to remove.
        // SAFETY: test-only env mutation; the guard restores on drop.
        unsafe { std::env::set_var("DCC_MCP_TEST_UTILS_REMOVE", "before") };
        {
            let _g = EnvVarGuard::set("DCC_MCP_TEST_UTILS_REMOVE", None);
            assert!(std::env::var("DCC_MCP_TEST_UTILS_REMOVE").is_err());
        }
        assert_eq!(
            std::env::var("DCC_MCP_TEST_UTILS_REMOVE").unwrap(),
            "before"
        );
        // SAFETY: test cleanup.
        unsafe { std::env::remove_var("DCC_MCP_TEST_UTILS_REMOVE") };
    }

    #[test]
    fn multi_var_set_and_restore() {
        let _g = EnvVarsGuard::set(&[
            ("DCC_MCP_TEST_UTILS_A", Some("1")),
            ("DCC_MCP_TEST_UTILS_B", Some("2")),
        ]);
        assert_eq!(std::env::var("DCC_MCP_TEST_UTILS_A").unwrap(), "1");
        assert_eq!(std::env::var("DCC_MCP_TEST_UTILS_B").unwrap(), "2");
    }
}

#[cfg(all(test, feature = "skill-rest"))]
mod skill_rest_fixture_tests {
    use std::sync::Arc;

    use dcc_mcp_skill_rest::SkillRestService;
    use serde_json::{Value, json};

    use crate::skill_rest::{
        InMemorySkillCatalog, RecordingToolInvoker, SkillRestTestHarness, catalog_action,
    };

    #[tokio::test]
    async fn shared_harness_exercises_real_router_and_records_invocation() {
        let mut action = catalog_action("create_sphere", "geometry", "maya", true);
        action.description = "Create a polygon sphere".to_string();
        let catalog = Arc::new(InMemorySkillCatalog::new([action]));
        let invoker = Arc::new(RecordingToolInvoker::default());
        invoker.set_next(Ok(json!({"name": "pSphere1"})));
        let service = SkillRestService::new(catalog, invoker.clone());
        let harness = SkillRestTestHarness::new(service);

        let search = harness
            .server
            .post("/v1/search")
            .json(&json!({"query": "sphere"}))
            .await;
        search.assert_status_ok();
        let body: Value = search.json();
        let slug = body["hits"][0]["slug"].as_str().unwrap();

        let call = harness
            .server
            .post("/v1/call")
            .json(&json!({"tool_slug": slug, "params": {"radius": 2.0}}))
            .await;
        call.assert_status_ok();
        assert_eq!(invoker.calls()[0].action_name, "create_sphere");
        assert_eq!(harness.audit.len(), 2);
    }
}
