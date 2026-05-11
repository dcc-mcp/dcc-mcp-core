use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

/// Drive `poll_pending_bounded(max)` on the calling thread until either
/// `max_iterations` ticks have elapsed or `should_stop` returns true.
/// Mirrors what a DCC idle pump would do.
async fn pump_until<F: Fn() -> bool>(
    exec: &mut DeferredExecutor,
    should_stop: F,
    max_iterations: usize,
) {
    for _ in 0..max_iterations {
        let _ = exec.poll_pending_bounded(8);
        if should_stop() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn submit_deferred_runs_closure_on_pumped_thread() {
    let mut exec = DeferredExecutor::new(16);
    let handle = exec.handle();
    let ct = CancellationToken::new();

    let invoked = Arc::new(AtomicBool::new(false));
    let invoked_clone = invoked.clone();
    let rx = handle.submit_deferred(
        "test.tool",
        ct.clone(),
        Box::new(move || {
            invoked_clone.store(true, Ordering::SeqCst);
            "\"ok\"".to_string()
        }),
    );

    let done = Arc::new(AtomicBool::new(false));
    let done_clone = done.clone();
    tokio::spawn(async move {
        let out = rx.await.expect("receiver");
        assert_eq!(out, "\"ok\"");
        done_clone.store(true, Ordering::SeqCst);
    });

    pump_until(&mut exec, || done.load(Ordering::SeqCst), 40).await;
    assert!(invoked.load(Ordering::SeqCst), "closure ran");
    assert!(done.load(Ordering::SeqCst), "oneshot resolved");
}

#[tokio::test]
async fn submit_deferred_skips_closure_when_cancelled_before_pump() {
    let mut exec = DeferredExecutor::new(16);
    let handle = exec.handle();
    let ct = CancellationToken::new();

    // Cancel BEFORE the pump ever runs — the wrapper's pre-check must
    // short-circuit and the user closure must NOT be invoked.
    ct.cancel();

    let user_closure_ran = Arc::new(AtomicBool::new(false));
    let flag = user_closure_ran.clone();
    let rx = handle.submit_deferred(
        "test.tool",
        ct.clone(),
        Box::new(move || {
            flag.store(true, Ordering::SeqCst);
            "\"should-not-run\"".to_string()
        }),
    );

    // Give the submitter task a chance to observe the cancellation and
    // decide between (a) not enqueuing at all or (b) enqueuing a wrapper
    // that short-circuits. Either outcome is correct — what matters is
    // that the user closure never runs.
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _ = exec.poll_pending_bounded(8);

    // The oneshot either resolves with CANCELLED or is dropped.
    let res = tokio::time::timeout(Duration::from_millis(100), rx).await;
    assert!(
        !user_closure_ran.load(Ordering::SeqCst),
        "user closure must not run after cancel-before-pump"
    );
    match res {
        Ok(Ok(json_str)) => {
            assert!(
                json_str.contains("CANCELLED"),
                "wrapper must surface CANCELLED, got {json_str}"
            );
        }
        Ok(Err(_)) | Err(_) => {
            // Sender dropped — caller would observe this as a cancelled
            // oneshot and translate to CANCELLED externally.
        }
    }
}

#[tokio::test]
async fn submit_deferred_runs_on_pump_thread_not_tokio_worker() {
    // Issue #332 acceptance — main-affined jobs must land on the thread
    // that drives the pump, never on a Tokio worker.
    let mut exec = DeferredExecutor::new(16);
    let handle = exec.handle();
    let ct = CancellationToken::new();

    let pump_thread_id = std::thread::current().id();
    let captured = Arc::new(parking_lot::Mutex::new(None::<std::thread::ThreadId>));
    let captured_clone = captured.clone();
    let rx = handle.submit_deferred(
        "test.main_only",
        ct.clone(),
        Box::new(move || {
            *captured_clone.lock() = Some(std::thread::current().id());
            "\"ok\"".to_string()
        }),
    );

    // Pump on THIS (test) thread; Tokio workers never touch the queue.
    let (done_tx, mut done_rx) = oneshot::channel();
    tokio::spawn(async move {
        let out = rx.await.expect("oneshot");
        let _ = done_tx.send(out);
    });
    let mut out = None;
    for _ in 0..50 {
        let _ = exec.poll_pending_bounded(8);
        if let Ok(o) = done_rx.try_recv() {
            out = Some(o);
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    assert!(out.is_some(), "submit_deferred returned");
    let observed = captured.lock().expect("closure recorded its thread id");
    assert_eq!(
        observed, pump_thread_id,
        "main-affined closure must execute on the pump thread"
    );
}

#[tokio::test]
async fn yield_frame_returns_after_pump_tick() {
    let mut exec = DeferredExecutor::new(16);
    let handle = exec.handle();

    let tick_count = Arc::new(AtomicUsize::new(0));
    let yielded = Arc::new(AtomicBool::new(false));
    let yielded_clone = yielded.clone();

    tokio::spawn(async move {
        handle.yield_frame().await.expect("yield");
        yielded_clone.store(true, Ordering::SeqCst);
    });

    // Simulate the DCC event loop.
    for _ in 0..40 {
        let n = exec.poll_pending_bounded(8);
        tick_count.fetch_add(n, Ordering::SeqCst);
        if yielded.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    assert!(yielded.load(Ordering::SeqCst), "yield_frame completed");
    assert!(
        tick_count.load(Ordering::SeqCst) >= 1,
        "pump processed at least one task"
    );
}

/// Issue #715: saturating the executor channel without pumping
/// surfaces a structured `QueueOverloaded` after the send-timeout
/// — not a silent hang, and not `ExecutorClosed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn execute_returns_queue_overloaded_on_saturation() {
    // Tiny channel + short send-timeout so the test stays fast.
    let _exec = DeferredExecutor::with_send_timeout(2, Duration::from_millis(50));
    let handle = _exec.handle();

    // Fill the channel: neither of these awaits resolves because
    // no one pumps. We don't await them here; we just let them
    // occupy the two slots.
    let h1 = handle.clone();
    let h2 = handle.clone();
    let _t1 = tokio::spawn(async move {
        let _ = h1.execute(Box::new(|| "one".to_string())).await;
    });
    let _t2 = tokio::spawn(async move {
        let _ = h2.execute(Box::new(|| "two".to_string())).await;
    });
    // Let them land in the channel.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // The third submit should hit the send-timeout and bubble
    // QueueOverloaded.
    let err = handle
        .execute(Box::new(|| "three".to_string()))
        .await
        .expect_err("saturation must fail");
    match err {
        HttpError::QueueOverloaded {
            depth,
            capacity,
            retry_after_secs,
        } => {
            assert_eq!(capacity, 2, "capacity reflects config");
            assert!(depth >= 1, "depth reported non-zero");
            assert!(retry_after_secs >= 1, "retry hint is non-zero");
        }
        other => panic!("expected QueueOverloaded, got {other:?}"),
    }
    let stats = handle.queue_stats();
    assert!(stats.total_rejected >= 1, "reject counter bumped");
    assert_eq!(stats.capacity, 2, "capacity reported in snapshot");
}

/// Issue #715: the queue-stats snapshot tracks enqueued/dequeued
/// and reports a non-zero oldest-wait-age while jobs are parked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_stats_track_enqueue_dequeue_and_age() {
    let mut exec = DeferredExecutor::new(8);
    let handle = exec.handle();

    let h = handle.clone();
    let submitter = tokio::spawn(async move { h.execute(Box::new(|| "r".to_string())).await });
    // Let the submit land.
    tokio::time::sleep(Duration::from_millis(20)).await;
    let pre = handle.queue_stats();
    assert_eq!(pre.total_enqueued, 1, "one submit recorded");
    assert_eq!(pre.total_dequeued, 0, "nothing drained yet");
    assert!(pre.oldest_wait_ms.unwrap_or(0) > 0, "wait-age reported");
    assert_eq!(pre.pending, 1);

    // Pump.
    let drained = exec.poll_pending();
    assert_eq!(drained, 1);
    let res = submitter.await.expect("join").expect("execute");
    assert_eq!(res, "r");

    let post = handle.queue_stats();
    assert_eq!(post.total_dequeued, 1, "drain recorded");
    assert_eq!(post.pending, 0);
    assert!(post.oldest_wait_ms.is_none(), "oldest cleared after drain");
    assert!(post.wait_p50_ms.is_some(), "p50 populated from sample");
}
