//! Resolution of transport-internal in-process split-phase continuations.

use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub const SPLIT_PHASE_ERROR_PREFIX: &str = "SPLIT_PHASE_";

fn split_phase_error(code: &str, message: &str) -> String {
    format!("{SPLIT_PHASE_ERROR_PREFIX}{code}: {message}")
}

/// Project a split-phase failure into the transport-neutral job envelope.
/// Legacy non-split errors remain plain strings for wire compatibility.
#[must_use]
pub fn project_error_for_job(message: &str) -> String {
    let message = if message == "CANCELLED" {
        split_phase_error("CANCELLED", "continuation cancelled")
    } else {
        message.to_owned()
    };
    let Some((code, detail)) = message.split_once(": ") else {
        return message;
    };
    if !code.starts_with(SPLIT_PHASE_ERROR_PREFIX) {
        return message;
    }
    serde_json::json!({
        "layer": "instance",
        "code": code,
        "message": detail,
    })
    .to_string()
}

/// Resolve a continuation marker after the main-affinity dispatch closure has
/// returned. The callback is consumed before execution, enforcing ownership
/// and one-shot replay protection.
pub async fn resolve_output(
    output: Value,
    cancellation: Option<CancellationToken>,
) -> Result<Value, String> {
    let marker = dcc_mcp_skills::catalog::execute::split_phase_marker(&output);
    let Some((owner, id, generation)) =
        marker.map(|(owner, id, generation)| (owner.to_owned(), id.to_owned(), generation))
    else {
        if dcc_mcp_skills::catalog::execute::has_split_phase_marker(&output) {
            return Err(split_phase_error(
                "MALFORMED_MARKER",
                "reserved split-phase marker is malformed",
            ));
        }
        return Ok(output);
    };

    if cancellation
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        let _ = dcc_mcp_skills::catalog::execute::take_split_phase_continuation(&owner, &id);
        return Err(split_phase_error("CANCELLED", "continuation cancelled"));
    }
    let Some(registration) =
        dcc_mcp_skills::catalog::execute::take_split_phase_continuation_if_generation(
            &owner, &id, generation,
        )
    else {
        return Err(split_phase_error(
            "MISSING",
            "continuation is missing or already consumed",
        ));
    };
    let timeout = registration.timeout;
    let control = registration.control();
    // Re-check the lifecycle after ownership transfer and immediately before
    // submitting the blocking callback. Shutdown may race with marker take;
    // never start a callback that cannot commit durably.
    if !registration.commit_allowed() {
        registration.cancel();
        return Err(split_phase_error(
            "INVALIDATED",
            "continuation invalidated before execution",
        ));
    }
    let worker = tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking({
            let callback = registration.callback.clone();
            let control = control.clone();
            move || (callback)(control)
        }),
    );
    tokio::pin!(worker);
    let result = if let Some(token) = cancellation.as_ref() {
        tokio::select! {
            result = &mut worker => result,
            _ = token.cancelled() => {
                registration.cancel();
                return Err(split_phase_error("CANCELLED", "continuation cancelled"));
            }
        }
    } else {
        worker.await
    };
    let result = match result {
        Err(_) => {
            registration.cancel();
            return Err(split_phase_error("TIMEOUT", "continuation timed out"));
        }
        Ok(result) => result
            .map_err(|err| {
                split_phase_error(
                    "WORKER_FAILED",
                    &format!("continuation worker failed: {err}"),
                )
            })?
            .map_err(|err| split_phase_error("CALLBACK_FAILED", &err))?,
    };

    if dcc_mcp_skills::catalog::execute::has_split_phase_marker(&result) {
        return Err(split_phase_error(
            "NESTED",
            "nested split-phase continuation rejected",
        ));
    }
    if cancellation
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        return Err(split_phase_error("CANCELLED", "continuation cancelled"));
    }
    if !registration.commit_allowed() {
        return Err(split_phase_error(
            "SHUTDOWN",
            "continuation invalidated by shutdown",
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn resolves_once_and_rejects_replay() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let id = store.register(
            Arc::new(|_| Ok(serde_json::json!({"ok": true}))),
            std::time::Duration::from_secs(1),
        );
        let marker = json!({"_dcc_mcp_split_phase": {"kind": "continuation.v1", "owner": store.owner(), "generation": store.generation(), "continuation_id": id}});
        assert_eq!(
            resolve_output(marker.clone(), None).await.unwrap(),
            json!({"ok": true})
        );
        assert!(resolve_output(marker, None).await.is_err());
    }

    #[tokio::test]
    async fn malformed_reserved_marker_fails_closed() {
        let malformed = json!({"_dcc_mcp_split_phase": {"kind": "continuation.v1"}});
        assert!(resolve_output(malformed, None).await.is_err());
    }

    #[tokio::test]
    async fn cancellation_is_checked_before_continuation_and_consumes_ownership() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let id = store.register(
            Arc::new(|_| Ok(serde_json::json!({"published": true}))),
            std::time::Duration::from_secs(1),
        );
        let marker = serde_json::json!({"_dcc_mcp_split_phase": {"kind": "continuation.v1", "owner": store.owner(), "generation": store.generation(), "continuation_id": id}});
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(
            resolve_output(marker.clone(), Some(token)).await,
            Err("SPLIT_PHASE_CANCELLED: continuation cancelled".into())
        );
        assert!(resolve_output(marker, None).await.is_err());
    }

    #[test]
    fn project_error_for_job_preserves_machine_readable_fields() {
        let projected = project_error_for_job("SPLIT_PHASE_TIMEOUT: continuation timed out");
        let value: serde_json::Value = serde_json::from_str(&projected).expect("JSON envelope");
        assert_eq!(value["layer"], "instance");
        assert_eq!(value["code"], "SPLIT_PHASE_TIMEOUT");
        assert_eq!(value["message"], "continuation timed out");
    }

    #[tokio::test]
    async fn timeout_fails_closed() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let id = store.register(
            Arc::new(|_| {
                std::thread::sleep(std::time::Duration::from_millis(30));
                Ok(serde_json::json!({"ok": true}))
            }),
            std::time::Duration::from_millis(1),
        );
        let marker = serde_json::json!({"_dcc_mcp_split_phase": {"kind": "continuation.v1", "owner": store.owner(), "generation": store.generation(), "continuation_id": id}});
        let err = resolve_output(marker, None).await.unwrap_err();
        assert!(err.contains("timed out"));
    }

    #[tokio::test]
    async fn timeout_control_blocks_late_durable_side_effect() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let published = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let published_callback = published.clone();
        let id = store.register(
            Arc::new(move |control| {
                std::thread::sleep(std::time::Duration::from_millis(30));
                if control.check().is_ok() {
                    published_callback.store(true, std::sync::atomic::Ordering::Release);
                }
                Ok(serde_json::json!({"published": true}))
            }),
            std::time::Duration::from_millis(1),
        );
        let marker = json!({"_dcc_mcp_split_phase": {"kind": "continuation.v1", "owner": store.owner(), "generation": store.generation(), "continuation_id": id}});
        assert!(resolve_output(marker, None).await.is_err());
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(!published.load(std::sync::atomic::Ordering::Acquire));
    }

    #[tokio::test]
    async fn shutdown_invalidates_running_continuation_before_commit() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started_callback = started.clone();
        let published = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let published_callback = published.clone();
        let id = store.register(
            Arc::new(move |control| {
                started_callback.store(true, std::sync::atomic::Ordering::Release);
                std::thread::sleep(std::time::Duration::from_millis(30));
                if control.check().is_ok() {
                    published_callback.store(true, std::sync::atomic::Ordering::Release);
                }
                Ok(serde_json::json!({"published": true}))
            }),
            std::time::Duration::from_secs(1),
        );
        let marker = json!({"_dcc_mcp_split_phase": {"kind": "continuation.v1", "owner": store.owner(), "generation": store.generation(), "continuation_id": id}});
        let task = tokio::spawn(resolve_output(marker, None));
        while !started.load(std::sync::atomic::Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        store.drain();
        let err = task.await.unwrap().unwrap_err();
        assert!(err.contains("invalidated"));
        assert!(!published.load(std::sync::atomic::Ordering::Acquire));
    }

    #[tokio::test]
    async fn shutdown_rejects_new_registration_and_resume_recovers() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        store.drain();
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_in_callback = called.clone();
        let id = store.register(
            Arc::new(move |_| {
                called_in_callback.store(true, std::sync::atomic::Ordering::Release);
                Ok(json!({"published": true}))
            }),
            std::time::Duration::from_secs(1),
        );
        let marker = json!({"_dcc_mcp_split_phase": {"kind": "continuation.v1", "owner": store.owner(), "generation": store.generation(), "continuation_id": id}});
        assert!(resolve_output(marker, None).await.is_err());
        assert!(!called.load(std::sync::atomic::Ordering::Acquire));

        store.resume();
        let id = store.register(
            Arc::new(|control| {
                control.check()?;
                Ok(json!({"published": true}))
            }),
            std::time::Duration::from_secs(1),
        );
        let marker = json!({"_dcc_mcp_split_phase": {"kind": "continuation.v1", "owner": store.owner(), "generation": store.generation(), "continuation_id": id}});
        assert_eq!(
            resolve_output(marker, None).await.unwrap(),
            json!({"published": true})
        );
    }

    #[test]
    fn shutdown_generation_drops_old_entries_and_isolates_owners() {
        let first = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let second = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let first_id = first.register(
            Arc::new(|_| Ok(serde_json::json!({"owner": "first"}))),
            std::time::Duration::from_secs(1),
        );
        let second_id = second.register(
            Arc::new(|_| Ok(serde_json::json!({"owner": "second"}))),
            std::time::Duration::from_secs(1),
        );
        let old_generation = first.generation();
        first.drain();
        assert!(
            dcc_mcp_skills::catalog::execute::take_split_phase_continuation_if_generation(
                first.owner(),
                &first_id,
                old_generation,
            )
            .is_none()
        );
        assert!(
            dcc_mcp_skills::catalog::execute::take_split_phase_continuation(
                second.owner(),
                &second_id,
            )
            .is_some()
        );
    }

    #[test]
    fn orphaned_marker_is_reaped_after_ttl_without_take() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let id = store.register(
            Arc::new(|_| Ok(serde_json::json!({"orphan": true}))),
            std::time::Duration::from_millis(10),
        );
        std::thread::sleep(std::time::Duration::from_millis(1_100));
        assert!(
            dcc_mcp_skills::catalog::execute::take_split_phase_continuation(store.owner(), &id,)
                .is_none()
        );
    }
}
