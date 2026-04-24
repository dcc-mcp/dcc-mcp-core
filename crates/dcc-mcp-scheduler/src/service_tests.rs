//! Unit tests for the scheduler service.
#![cfg(test)]

use super::*;
use crate::sink::RecordingSink;
use chrono::TimeZone;
use std::sync::Arc;

fn cron_spec(id: &str, expr: &str) -> ScheduleSpec {
    ScheduleSpec {
        id: id.into(),
        workflow: "w".into(),
        inputs: serde_json::Value::Null,
        trigger: TriggerSpec::Cron {
            expression: expr.into(),
            timezone: "UTC".into(),
            jitter_secs: 0,
        },
        enabled: true,
        max_concurrent: 1,
    }
}

#[test]
fn rejects_duplicate_ids() {
    let a = cron_spec("same", "* * * * * *");
    let b = cron_spec("same", "*/5 * * * * *");
    let err = SchedulerConfig::from_specs(vec![a, b]).unwrap_err();
    assert!(matches!(err, SchedulerError::DuplicateId { .. }));
}

#[test]
fn accepts_six_field_cron() {
    let s = cron_spec("every_sec", "* * * * * *");
    SchedulerConfig::from_specs(vec![s]).unwrap();
}

#[test]
fn concurrency_tracker_respects_max() {
    let t = ConcurrencyTracker::default();
    assert!(t.try_acquire("s", 2));
    assert!(t.try_acquire("s", 2));
    assert!(!t.try_acquire("s", 2));
    t.release("s");
    assert!(t.try_acquire("s", 2));
}

#[test]
fn concurrency_tracker_unlimited_when_zero() {
    let t = ConcurrencyTracker::default();
    for _ in 0..10 {
        assert!(t.try_acquire("s", 0));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cron_every_second_fires_multiple_times() {
    let cfg = SchedulerConfig::from_specs(vec![cron_spec("tick", "* * * * * *")]).unwrap();
    let sink = Arc::new(RecordingSink::new());
    let (handle, _router) = SchedulerService::new(cfg, sink.clone()).start();
    // Release the concurrency gate each time — the schedule has
    // max_concurrent=1 so we need to mark_terminal to allow subsequent fires.
    let handle_clone = handle.clone();
    let releaser = tokio::spawn(async move {
        for _ in 0..6 {
            tokio::time::sleep(Duration::from_millis(300)).await;
            handle_clone.mark_terminal("tick");
        }
    });
    tokio::time::sleep(Duration::from_millis(2500)).await;
    handle.shutdown();
    let _ = releaser.await;
    let n = sink.len();
    assert!(n >= 2, "expected at least 2 fires in 2.5s window, got {n}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_concurrent_skips_when_not_released() {
    let cfg = SchedulerConfig::from_specs(vec![cron_spec("hold", "* * * * * *")]).unwrap();
    let sink = Arc::new(RecordingSink::new());
    let (handle, _router) = SchedulerService::new(cfg, sink.clone()).start();
    // Do NOT call mark_terminal — in-flight stays at 1 so subsequent
    // fires are skipped.
    tokio::time::sleep(Duration::from_millis(3500)).await;
    handle.shutdown();
    let n = sink.len();
    assert_eq!(n, 1, "expected exactly 1 fire with no release, got {n}");
}

#[test]
fn jitter_seeded_is_deterministic() {
    let tz_date = chrono_tz::UTC
        .with_ymd_and_hms(2030, 1, 1, 0, 0, 0)
        .unwrap();
    let a = jitter_duration(120, Some(42), "id", tz_date);
    let b = jitter_duration(120, Some(42), "id", tz_date);
    assert_eq!(a, b);
    let c = jitter_duration(120, Some(43), "id", tz_date);
    assert_ne!(a, c);
}

#[test]
fn jitter_within_bounds() {
    let tz_date = chrono_tz::UTC
        .with_ymd_and_hms(2030, 1, 1, 0, 0, 0)
        .unwrap();
    for i in 0..20 {
        let d = jitter_duration(5, Some(i), "id", tz_date);
        assert!(d <= Duration::from_secs(5));
    }
}
