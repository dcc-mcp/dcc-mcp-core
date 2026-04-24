//! [`SchedulerService`] — the runtime that owns cron tasks and webhook
//! routes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule as CronSchedule;
use dashmap::DashMap;
use parking_lot::Mutex;
use rand::RngExt;
use serde_json::json;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::error::SchedulerError;
use crate::sink::{SharedJobSink, TriggerFire, TriggerKind};
use crate::spec::{ScheduleFile, ScheduleSpec, TriggerSpec};
use crate::template::{RenderCtx, render_value};
use crate::webhook::{HMAC_HEADER, verify_hub_signature_256};

/// Parse a cron expression, returning a structural [`CronSchedule`].
///
/// # Errors
///
/// Returns [`SchedulerError::InvalidCron`] on parse failure.
pub fn parse_cron(expression: &str) -> Result<CronSchedule, SchedulerError> {
    expression
        .parse::<CronSchedule>()
        .map_err(|e| SchedulerError::InvalidCron {
            expression: expression.to_string(),
            message: e.to_string(),
        })
}

/// Parse a `chrono_tz` timezone name.
///
/// # Errors
///
/// Returns [`SchedulerError::InvalidTimezone`] on parse failure.
pub fn parse_timezone(name: &str) -> Result<Tz, SchedulerError> {
    name.parse::<Tz>()
        .map_err(|_| SchedulerError::InvalidTimezone {
            timezone: name.to_string(),
        })
}

// ── Configuration ───────────────────────────────────────────────────────

/// Configuration loaded from a directory of `*.schedules.yaml` files.
#[derive(Debug, Clone, Default)]
pub struct SchedulerConfig {
    /// Parsed, validated schedules.
    pub schedules: Vec<ScheduleSpec>,
}

impl SchedulerConfig {
    /// Collect schedules from every `*.schedules.yaml` file in `dir`
    /// (non-recursive).
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::Load`] on IO or parse failure,
    /// [`SchedulerError::DuplicateId`] when two schedules share an id,
    /// or [`SchedulerError::Validation`] on structural problems.
    pub fn from_dir(dir: impl AsRef<Path>) -> Result<Self, SchedulerError> {
        let dir = dir.as_ref();
        let entries = std::fs::read_dir(dir).map_err(|e| SchedulerError::Load {
            path: dir.display().to_string(),
            message: e.to_string(),
        })?;
        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file()
                && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n.ends_with(".schedules.yaml") || n.ends_with(".schedules.yml")
                })
            {
                paths.push(p);
            }
        }
        paths.sort();
        Self::from_paths(&paths)
    }

    /// Build a [`SchedulerConfig`] from explicit paths.
    ///
    /// # Errors
    ///
    /// See [`Self::from_dir`].
    pub fn from_paths(paths: &[PathBuf]) -> Result<Self, SchedulerError> {
        let mut all: Vec<ScheduleSpec> = Vec::new();
        for p in paths {
            let file = ScheduleFile::load(p)?;
            all.extend(file.schedules);
        }
        Self::from_specs(all)
    }

    /// Validate + deduplicate a raw list of schedules.
    ///
    /// # Errors
    ///
    /// See [`Self::from_dir`].
    pub fn from_specs(schedules: Vec<ScheduleSpec>) -> Result<Self, SchedulerError> {
        let mut seen: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(schedules.len());
        for s in &schedules {
            s.validate()?;
            if !seen.insert(s.id.clone()) {
                return Err(SchedulerError::DuplicateId { id: s.id.clone() });
            }
        }
        Ok(Self { schedules })
    }
}

// ── Concurrency tracking ────────────────────────────────────────────────

#[derive(Debug, Default)]
struct ConcurrencyTracker {
    // schedule id -> currently in-flight count
    counts: DashMap<String, u32>,
}

impl ConcurrencyTracker {
    fn try_acquire(&self, id: &str, max: u32) -> bool {
        if max == 0 {
            // Unlimited — still record for visibility.
            *self.counts.entry(id.to_string()).or_insert(0) += 1;
            return true;
        }
        let mut entry = self.counts.entry(id.to_string()).or_insert(0);
        if *entry >= max {
            return false;
        }
        *entry += 1;
        true
    }

    fn release(&self, id: &str) {
        if let Some(mut entry) = self.counts.get_mut(id) {
            *entry = entry.saturating_sub(1);
        }
    }

    fn in_flight(&self, id: &str) -> u32 {
        self.counts.get(id).map_or(0, |e| *e)
    }
}

// ── Handle ──────────────────────────────────────────────────────────────

/// Opaque handle returned by [`SchedulerService::start`].
///
/// Dropping the handle cancels every cron task (via [`Notify`]) and frees
/// the concurrency tracker; the webhook routes die with the `Router`
/// they are attached to.
#[derive(Clone)]
pub struct SchedulerHandle {
    inner: Arc<SchedulerInner>,
}

struct SchedulerInner {
    tracker: ConcurrencyTracker,
    shutdown: Notify,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    specs_by_id: DashMap<String, ScheduleSpec>,
}

impl std::fmt::Debug for SchedulerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchedulerHandle")
            .field("schedules", &self.inner.specs_by_id.len())
            .finish()
    }
}

impl SchedulerHandle {
    /// Called by the host when it observes a terminal workflow status for
    /// a fire that was enqueued by this schedule id. Decrements the
    /// in-flight counter so `max_concurrent` admits future fires.
    pub fn mark_terminal(&self, schedule_id: &str) {
        self.inner.tracker.release(schedule_id);
    }

    /// Current in-flight count for a schedule.
    #[must_use]
    pub fn in_flight(&self, schedule_id: &str) -> u32 {
        self.inner.tracker.in_flight(schedule_id)
    }

    /// Signal cron tasks to stop. Running tasks observe the notification
    /// at their next wake-up.
    pub fn shutdown(&self) {
        self.inner.shutdown.notify_waiters();
        let mut tasks = self.inner.tasks.lock();
        for t in tasks.drain(..) {
            t.abort();
        }
    }

    /// Number of registered schedules.
    #[must_use]
    pub fn schedule_count(&self) -> usize {
        self.inner.specs_by_id.len()
    }
}

impl Drop for SchedulerInner {
    fn drop(&mut self) {
        self.shutdown.notify_waiters();
        let mut tasks = self.tasks.lock();
        for t in tasks.drain(..) {
            t.abort();
        }
    }
}

// ── Service ─────────────────────────────────────────────────────────────

/// Runtime that owns cron tasks and produces the webhook router.
pub struct SchedulerService {
    config: SchedulerConfig,
    sink: SharedJobSink,
    // seed for jitter; None → real rng. Exposed for tests.
    jitter_seed: Option<u64>,
}

impl SchedulerService {
    /// Create a service over a prepared config and sink.
    #[must_use]
    pub fn new(config: SchedulerConfig, sink: SharedJobSink) -> Self {
        Self {
            config,
            sink,
            jitter_seed: None,
        }
    }

    /// Seed the jitter PRNG (tests only — for deterministic behaviour).
    #[must_use]
    pub fn with_jitter_seed(mut self, seed: u64) -> Self {
        self.jitter_seed = Some(seed);
        self
    }

    /// Spawn cron tasks and return `(handle, webhook_router)`.
    ///
    /// The router carries routes for every enabled webhook schedule;
    /// callers merge it into their axum app with `Router::merge`.
    pub fn start(self) -> (SchedulerHandle, Router) {
        let inner = Arc::new(SchedulerInner {
            tracker: ConcurrencyTracker::default(),
            shutdown: Notify::new(),
            tasks: Mutex::new(Vec::new()),
            specs_by_id: DashMap::new(),
        });
        let handle = SchedulerHandle {
            inner: inner.clone(),
        };

        for spec in &self.config.schedules {
            inner.specs_by_id.insert(spec.id.clone(), spec.clone());
        }

        // Spawn cron drivers.
        for spec in self.config.schedules.iter().cloned() {
            if !spec.enabled {
                continue;
            }
            if let TriggerSpec::Cron { .. } = spec.trigger {
                let task =
                    spawn_cron_task(spec, inner.clone(), self.sink.clone(), self.jitter_seed);
                inner.tasks.lock().push(task);
            }
        }

        // Build webhook router.
        let router = build_webhook_router(&self.config, self.sink.clone(), inner.clone());

        (handle, router)
    }
}

// ── Cron driver ─────────────────────────────────────────────────────────

fn spawn_cron_task(
    spec: ScheduleSpec,
    inner: Arc<SchedulerInner>,
    sink: SharedJobSink,
    jitter_seed: Option<u64>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let (expression, tz_name, jitter) = match &spec.trigger {
            TriggerSpec::Cron {
                expression,
                timezone,
                jitter_secs,
            } => (expression.clone(), timezone.clone(), *jitter_secs),
            _ => return,
        };
        let schedule = match parse_cron(&expression) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("scheduler[{}]: {e}", spec.id);
                return;
            }
        };
        let tz = parse_timezone(&tz_name).unwrap_or(chrono_tz::UTC);

        loop {
            let now_tz = Utc::now().with_timezone(&tz);
            let next = match schedule.upcoming(tz).next() {
                Some(t) => t,
                None => {
                    tracing::warn!(
                        "scheduler[{}]: cron expression {:?} produced no upcoming times",
                        spec.id,
                        expression
                    );
                    return;
                }
            };
            let mut delay = (next - now_tz).to_std().unwrap_or(Duration::from_millis(0));
            if jitter > 0 {
                let extra = jitter_duration(jitter, jitter_seed, &spec.id, next);
                delay += extra;
            }
            // Minimum 1ms sleep to avoid tight loops if delay collapses.
            if delay.is_zero() {
                delay = Duration::from_millis(1);
            }
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = inner.shutdown.notified() => {
                    tracing::debug!("scheduler[{}]: shutdown", spec.id);
                    return;
                }
            }

            // Fire (unless concurrency gate rejects).
            if !inner.tracker.try_acquire(&spec.id, spec.max_concurrent) {
                tracing::info!(
                    "scheduler[{}]: skipped (in-flight={}, max_concurrent={})",
                    spec.id,
                    inner.tracker.in_flight(&spec.id),
                    spec.max_concurrent
                );
                // Busy-wait: poll the cron schedule for its *next* fire time
                // rather than immediately retrying. We simply loop — the
                // next iteration picks up a fresh `upcoming`.
                // Prevent a tight loop if next is immediate by a brief
                // grace period.
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                    _ = inner.shutdown.notified() => return,
                }
                continue;
            }

            let fired_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let payload = serde_json::Value::Object(serde_json::Map::new());
            let ctx = RenderCtx {
                payload: &payload,
                schedule_id: &spec.id,
                workflow: &spec.workflow,
            };
            let inputs = render_value(&spec.inputs, &ctx);
            let fire = TriggerFire {
                kind: TriggerKind::Cron,
                schedule_id: spec.id.clone(),
                workflow: spec.workflow.clone(),
                inputs,
                payload,
                fired_at,
            };

            if let Err(err) = sink.enqueue(fire) {
                tracing::warn!("scheduler[{}]: sink error: {err}", spec.id);
                inner.tracker.release(&spec.id);
            }
        }
    })
}

fn jitter_duration(
    max_secs: u32,
    seed: Option<u64>,
    schedule_id: &str,
    next: DateTime<Tz>,
) -> Duration {
    use rand::SeedableRng;
    if max_secs == 0 {
        return Duration::ZERO;
    }
    let max_ms = (u64::from(max_secs)).saturating_mul(1000);
    let secs = match seed {
        Some(s) => {
            // Mix in schedule id + next fire time so each fire gets a
            // different draw while remaining deterministic for a given
            // (seed, schedule, fire-time) triple.
            let mix = s ^ hash64(schedule_id) ^ (next.timestamp() as u64);
            rand::rngs::StdRng::seed_from_u64(mix).random_range(0..=max_ms)
        }
        None => rand::rng().random_range(0..=max_ms),
    };
    Duration::from_millis(secs)
}

fn hash64(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

// ── Webhook router ──────────────────────────────────────────────────────

#[derive(Clone)]
struct WebhookState {
    spec: ScheduleSpec,
    sink: SharedJobSink,
    inner: Arc<SchedulerInner>,
    secret: Option<Vec<u8>>,
}

fn build_webhook_router(
    cfg: &SchedulerConfig,
    sink: SharedJobSink,
    inner: Arc<SchedulerInner>,
) -> Router {
    let mut router = Router::new();
    for spec in &cfg.schedules {
        if !spec.enabled {
            continue;
        }
        let (path, secret_env) = match &spec.trigger {
            TriggerSpec::Webhook { path, secret_env } => (path.clone(), secret_env.clone()),
            _ => continue,
        };
        let secret = match secret_env.as_deref() {
            Some(var) => match std::env::var(var) {
                Ok(v) => Some(v.into_bytes()),
                Err(_) => {
                    tracing::warn!(
                        "scheduler[{}]: webhook secret env var {:?} not set at startup",
                        spec.id,
                        var
                    );
                    // Keep the env name so we can fail-loud on request.
                    None
                }
            },
            None => None,
        };
        let state = WebhookState {
            spec: spec.clone(),
            sink: sink.clone(),
            inner: inner.clone(),
            secret,
        };
        router = router.route(&path, post(handle_webhook).with_state(state));
    }
    router
}

async fn handle_webhook(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let WebhookState {
        spec,
        sink,
        inner,
        secret,
    } = state;

    // HMAC validation.
    if let TriggerSpec::Webhook {
        secret_env: Some(var),
        ..
    } = &spec.trigger
    {
        let Some(key) = secret.as_deref() else {
            tracing::error!(
                "scheduler[{}]: refusing request, secret env {:?} not set",
                spec.id,
                var
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "webhook_secret_missing", "env": var})),
            )
                .into_response();
        };
        let header_val = headers.get(HMAC_HEADER).and_then(|h| h.to_str().ok());
        if !verify_hub_signature_256(key, body.as_ref(), header_val) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "invalid_signature"})),
            )
                .into_response();
        }
    }

    let payload: serde_json::Value = if body.is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_json", "message": e.to_string()})),
                )
                    .into_response();
            }
        }
    };

    if !inner.tracker.try_acquire(&spec.id, spec.max_concurrent) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "max_concurrent_exceeded",
                "schedule_id": spec.id,
                "in_flight": inner.tracker.in_flight(&spec.id),
                "max_concurrent": spec.max_concurrent,
            })),
        )
            .into_response();
    }

    let fired_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ctx = RenderCtx {
        payload: &payload,
        schedule_id: &spec.id,
        workflow: &spec.workflow,
    };
    let inputs = render_value(&spec.inputs, &ctx);
    let fire = TriggerFire {
        kind: TriggerKind::Webhook,
        schedule_id: spec.id.clone(),
        workflow: spec.workflow.clone(),
        inputs: inputs.clone(),
        payload,
        fired_at,
    };

    match sink.enqueue(fire) {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({
                "status": "enqueued",
                "schedule_id": spec.id,
                "workflow": spec.workflow,
                "inputs": inputs,
            })),
        )
            .into_response(),
        Err(err) => {
            inner.tracker.release(&spec.id);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "sink_failed", "message": err})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
