//! Resolution of transport-internal in-process split-phase continuations.

use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// Resolve a continuation marker after the main-affinity dispatch closure has
/// returned. The callback is consumed before execution, enforcing ownership
/// and one-shot replay protection.
pub async fn resolve_output(
    output: Value,
    cancellation: Option<CancellationToken>,
) -> Result<Value, String> {
    let Some((owner, id, generation)) =
        dcc_mcp_skills::catalog::execute::split_phase_marker(&output)
            .map(|(owner, id, generation)| (owner.to_owned(), id.to_owned(), generation))
    else {
        return Ok(output);
    };

    if cancellation
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        let _ = dcc_mcp_skills::catalog::execute::take_split_phase_continuation(&owner, &id);
        return Err("CANCELLED".to_string());
    }
    let Some(registration) =
        dcc_mcp_skills::catalog::execute::take_split_phase_continuation_if_generation(
            &owner, &id, generation,
        )
    else {
        return Err("split-phase continuation is missing or already consumed".to_string());
    };
    let timeout = registration.timeout;
    let result = tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking({
            let callback = registration.callback.clone();
            move || (callback)()
        }),
    )
    .await
    .map_err(|_| "split-phase continuation timed out".to_string())?
    .map_err(|err| format!("split-phase continuation worker failed: {err}"))??;

    if dcc_mcp_skills::catalog::execute::split_phase_marker(&result).is_some() {
        return Err("nested split-phase continuation rejected".to_string());
    }
    if cancellation
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        return Err("CANCELLED".to_string());
    }
    if !registration.commit_allowed() {
        return Err("split-phase continuation invalidated by shutdown".to_string());
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
            Arc::new(|| Ok(serde_json::json!({"ok": true}))),
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
    async fn cancellation_is_checked_before_continuation_and_consumes_ownership() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let id = store.register(
            Arc::new(|| Ok(serde_json::json!({"published": true}))),
            std::time::Duration::from_secs(1),
        );
        let marker = serde_json::json!({"_dcc_mcp_split_phase": {"kind": "continuation.v1", "owner": store.owner(), "generation": store.generation(), "continuation_id": id}});
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(
            resolve_output(marker.clone(), Some(token)).await,
            Err("CANCELLED".into())
        );
        assert!(resolve_output(marker, None).await.is_err());
    }

    #[tokio::test]
    async fn timeout_fails_closed() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let id = store.register(
            Arc::new(|| {
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
    async fn shutdown_invalidates_running_continuation_before_commit() {
        let store = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started_callback = started.clone();
        let id = store.register(
            Arc::new(move || {
                started_callback.store(true, std::sync::atomic::Ordering::Release);
                std::thread::sleep(std::time::Duration::from_millis(30));
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
    }

    #[test]
    fn shutdown_generation_drops_old_entries_and_isolates_owners() {
        let first = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let second = dcc_mcp_skills::catalog::execute::SplitPhaseStore::new();
        let first_id = first.register(
            Arc::new(|| Ok(serde_json::json!({"owner": "first"}))),
            std::time::Duration::from_secs(1),
        );
        let second_id = second.register(
            Arc::new(|| Ok(serde_json::json!({"owner": "second"}))),
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
            Arc::new(|| Ok(serde_json::json!({"orphan": true}))),
            std::time::Duration::from_millis(10),
        );
        std::thread::sleep(std::time::Duration::from_millis(1_100));
        assert!(
            dcc_mcp_skills::catalog::execute::take_split_phase_continuation(store.owner(), &id,)
                .is_none()
        );
    }
}
