use super::*;

mod health;
#[cfg(feature = "prometheus")]
mod metrics;
mod probe;

use probe::probe_and_evict_dead_instances;
pub(crate) use probe::self_probe_listener;

/// Outcome of [`start_gateway_tasks`] for the ambient (shared-runtime) path.
pub(crate) struct GatewayTasks {
    /// AbortHandle for the combined supervisor task (cleanup + watcher +
    /// tools watcher + serve).
    pub(crate) abort: AbortHandle,
    /// JoinHandle for the combined supervisor task. Retained by
    /// `GatewayHandle` so the task is not silently detached — this is the
    /// fix for the "Run A: TIMEOUT" leg of issue #303.
    pub(crate) supervisor: tokio::task::JoinHandle<()>,
    /// Yield signal used by the caller to trigger graceful shutdown.
    #[allow(dead_code)]
    pub(crate) yield_tx: Arc<watch::Sender<bool>>,
}

struct GatewayTaskGroup {
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl GatewayTaskGroup {
    fn new(handles: Vec<tokio::task::JoinHandle<()>>) -> Self {
        Self { handles }
    }

    async fn wait_all(mut self) {
        while let Some(handle) = self.handles.pop() {
            let _ = handle.await;
        }
    }
}

impl Drop for GatewayTaskGroup {
    fn drop(&mut self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

async fn wait_for_gateway_yield(mut yield_rx: watch::Receiver<bool>) {
    loop {
        if yield_rx.changed().await.is_err() {
            break;
        }
        if *yield_rx.borrow() {
            break;
        }
    }
}

/// Capacity of the in-memory audit ring buffer and SQLite merge limit.
#[cfg(feature = "admin")]
const ADMIN_AUDIT_RING_CAPACITY: usize = 512;

/// Build and run the gateway HTTP server with graceful-yield and live-push support.
///
/// Returns a [`GatewayTasks`] handle holding both the `AbortHandle` and the
/// supervisor task's `JoinHandle`, so the caller (typically a
/// [`GatewayHandle`]) can keep the task alive for its own lifetime.
///
/// `sentinel_key` is the registry key of the `__gateway__` sentinel row that
/// the caller registered; the cleanup loop heartbeats it (issue #229).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn start_gateway_tasks(
    listener: tokio::net::TcpListener,
    remote_listener: Option<tokio::net::TcpListener>,
    registry: Arc<RwLock<FileRegistry>>,
    stale_timeout: Duration,
    backend_timeout: Duration,
    async_dispatch_timeout: Duration,
    wait_terminal_timeout: Duration,
    route_ttl: Duration,
    max_routes_per_session: usize,
    server_name: String,
    server_version: String,
    sentinel_key: ServiceKey,
    own_host: String,
    own_port: u16,
    allow_unknown_tools: bool,
    policy: dcc_mcp_gateway_core::policy::GatewayPolicy,
    adapter_version: Option<String>,
    adapter_dcc: Option<String>,
    middleware_chain: crate::gateway::middleware::MiddlewareChain,
    #[cfg(feature = "admin")] admin_enabled: bool,
    #[cfg(feature = "admin")] admin_path: String,
    #[cfg(feature = "admin")] admin_persist: crate::gateway::config::AdminPersistConfig,
    health_check_interval_secs: u64,
    health_check_failures: u32,
    mdns_discovery_enabled: bool,
    mdns_ttl: Duration,
    mdns_probe_timeout: Duration,
) -> Result<GatewayTasks, Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(not(feature = "mdns"))]
    let _ = (mdns_discovery_enabled, mdns_ttl, mdns_probe_timeout);

    // ── Yield channel ─────────────────────────────────────────────────────
    let (yield_tx, yield_rx) = watch::channel(false);
    let yield_tx = Arc::new(yield_tx);

    // ── SSE broadcast channel ──────────────────────────────────────────────
    // All MCP notifications (resources/list_changed, tools/list_changed) are
    // sent here. Capacity 128 is generous; watchers fire at most every 2-3 s.
    let (events_tx, _) = broadcast::channel::<String>(128);
    let events_tx = Arc::new(events_tx);

    // ── Shared HTTP client for backend fan-out (JSON-RPC calls) ───────────
    // Reused by both the tools-list watcher task and the facade /mcp handler
    // via GatewayState so connection pooling is shared across all consumers.
    // A 30-second timeout is appropriate for regular request/response calls.
    let http_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()?;

    // ── Separate HTTP client for the backend SSE subscriber (issue #TODO) ──
    // MUST NOT have a client-level timeout. reqwest's `.timeout()` applies to
    // the *entire* request including the streaming response body, so a 30-second
    // client timeout would kill every long-lived SSE connection exactly 30 s
    // after it was established — producing the recurring "error decoding response
    // body / stream closed — reconnecting" log storm seen in production.
    //
    // The per-chunk idle timeout is enforced inside `pump_stream` via
    // `tokio::time::timeout(STREAM_IDLE_TIMEOUT, ...)` on each chunk read
    // (currently 60 s), which correctly keeps the stream alive through
    // normal server-side SSE keep-alive heartbeats while still failing fast
    // when the backend goes genuinely silent.
    let sse_http_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()?;

    let http_instance_registry = Arc::new(parking_lot::RwLock::new(
        crate::gateway::http_registration::HttpInstanceRegistry::default(),
    ));
    let mdns_instance_registry = Arc::new(parking_lot::RwLock::new(
        crate::gateway::mdns_discovery::MdnsInstanceRegistry::default(),
    ));

    // ── Contention event log + Prometheus counters (issue #766) ───────────
    let contention_log = Arc::new(crate::gateway::event_log::EventLog::new());
    #[cfg(feature = "prometheus")]
    let gateway_metrics = Arc::new(crate::gateway::event_log::GatewayMetrics::new());

    // ── Stale cleanup + sentinel heartbeat + dead-PID pruning (every 15 s) ─
    //
    // Issue #229: the sentinel row is heartbeated here — without this, it
    // would be considered stale 30 s after startup and challengers could not
    // distinguish a live gateway from a crashed one.
    //
    // Issue #227: dead-PID pruning reaps ghost rows left behind when a DCC
    // plugin crashes after registering but before its own heartbeat starts.
    let reg_cleanup = registry.clone();
    let own_version = server_version.clone();
    let own_adapter_version = adapter_version.clone();
    let own_adapter_dcc = adapter_dcc.clone();
    let yield_tx_cleanup = yield_tx.clone();
    let sentinel_key_cleanup = sentinel_key.clone();
    let cleanup_event_log = contention_log.clone();
    #[cfg(feature = "prometheus")]
    let cleanup_metrics = gateway_metrics.clone();
    let cleanup_own_version = server_version.clone();
    let cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            let r = reg_cleanup.read().await;

            // Keep the sentinel fresh first — it's what `has_newer_sentinel`
            // and every consumer of `list_instances("__gateway__")` rely on.
            let _ = r.heartbeat(&sentinel_key_cleanup);

            match r.cleanup_stale(stale_timeout) {
                Ok(n) if n > 0 => {
                    tracing::info!("Gateway: evicted {} stale instance(s)", n);
                    // Record one synthetic stale-eviction event per batch.
                    crate::gateway::event_log::record_event(
                        &cleanup_event_log,
                        #[cfg(feature = "prometheus")]
                        &cleanup_metrics,
                        crate::gateway::event_log::EventKind::GhostReaped,
                        "gateway",
                        "cleanup",
                        Some(format!("stale cleanup evicted {n} instance(s)")),
                    );
                }
                Err(e) => tracing::warn!("Gateway: stale cleanup error: {e}"),
                _ => {}
            }

            match r.prune_dead_pids() {
                Ok(n) if n > 0 => {
                    tracing::info!("Gateway: reaped {} ghost entry/entries", n);
                    crate::gateway::event_log::record_event(
                        &cleanup_event_log,
                        #[cfg(feature = "prometheus")]
                        &cleanup_metrics,
                        crate::gateway::event_log::EventKind::GhostReaped,
                        "gateway",
                        "cleanup",
                        Some(format!("dead-PID sweep reaped {n} ghost entry/entries")),
                    );
                }
                Err(e) => tracing::warn!("Gateway: ghost-entry reap error: {e}"),
                _ => {}
            }

            // Issue maya#137: include adapter_version + adapter_dcc in the
            // self-yield decision so a freshly-installed Maya plugin (real
            // DCC) can preempt an older standalone (`unknown`) gateway.
            let own_info = ElectionInfo::new(
                &own_version,
                own_adapter_version.as_deref(),
                own_adapter_dcc.as_deref(),
            );
            if has_newer_sentinel(&r, own_info, stale_timeout) {
                tracing::info!(
                    current = %own_version,
                    adapter_version = ?own_adapter_version,
                    adapter_dcc = ?own_adapter_dcc,
                    "Gateway: newer-version sentinel detected — initiating voluntary yield"
                );
                crate::gateway::event_log::record_event(
                    &cleanup_event_log,
                    #[cfg(feature = "prometheus")]
                    &cleanup_metrics,
                    crate::gateway::event_log::EventKind::VoluntaryYield,
                    "gateway",
                    "self",
                    Some(format!(
                        "yielded to newer challenger; own={cleanup_own_version}"
                    )),
                );
                let _ = yield_tx_cleanup.send(true);
                break;
            }
        }
    });

    // ── Instance-change watcher (every 2 s) ───────────────────────────────
    // Detects when DCC instances join or leave and broadcasts
    // `notifications/resources/list_changed` to all connected SSE clients.
    let reg_watch = registry.clone();
    let watch_http_registry = http_instance_registry.clone();
    let watch_mdns_registry = mdns_instance_registry.clone();
    let events_tx_watch = events_tx.clone();
    let watch_own_host = own_host.clone();
    let watch_own_port = own_port;
    let watcher_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        // Fingerprint = sorted "dcc_type:instance_id" strings of live instances.
        let mut last_fingerprint = String::new();

        loop {
            interval.tick().await;

            let fingerprint = {
                let r = reg_watch.read().await;
                let mut entries: Vec<_> = r
                    .list_all()
                    .into_iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                            && !e.is_stale(stale_timeout)
                            && !is_own_instance(e, &watch_own_host, watch_own_port)
                    })
                    .collect();
                let http_entries = watch_http_registry
                    .read()
                    .live_entries(std::time::SystemTime::now());
                let mdns_entries = watch_mdns_registry
                    .read()
                    .live_entries(std::time::SystemTime::now());
                let http_ids: std::collections::HashSet<_> =
                    http_entries.iter().map(|entry| entry.instance_id).collect();
                let mdns_ids: std::collections::HashSet<_> =
                    mdns_entries.iter().map(|entry| entry.instance_id).collect();
                entries.retain(|entry| {
                    !http_ids.contains(&entry.instance_id) && !mdns_ids.contains(&entry.instance_id)
                });
                entries.extend(mdns_entries);
                entries.extend(http_entries);
                let mut keys: Vec<String> = entries
                    .into_iter()
                    .map(|e| format!("{}:{}", e.dcc_type, e.instance_id))
                    .collect();
                keys.sort_unstable();
                keys.join("|")
            };

            if fingerprint != last_fingerprint {
                tracing::debug!(
                    "Gateway: instance set changed — broadcasting resources/list_changed"
                );
                // Only send if there are active SSE subscribers.
                if events_tx_watch.receiver_count() > 0 {
                    let notif = serde_json::to_string(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/resources/list_changed",
                        "params": {}
                    }))
                    .unwrap_or_default();
                    let _ = events_tx_watch.send(notif);
                }
                last_fingerprint = fingerprint;
            }
        }
    });

    // ── Aggregated tools/list_changed watcher (every 3 s) ─────────────────
    // Polls every live backend's `tools/list`, computes a set-fingerprint of
    // "{instance_id}:{tool_name}" tuples, and broadcasts one
    // `notifications/tools/list_changed` to gateway SSE subscribers when the
    // aggregated set changes (skill loaded / unloaded on any DCC).
    //
    // Polling (vs. real SSE subscription to each backend) keeps the gateway
    // decoupled from backend session lifecycles and works uniformly even when
    // instances come and go. 3-second granularity is well within the latency
    // budget for interactive skill loading.
    let reg_tools = registry.clone();
    let events_tx_tools = events_tx.clone();
    let http_client_tools = http_client.clone();
    let tools_own_host = own_host.clone();
    let tools_own_port = own_port;
    let tools_watcher_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_fingerprint = String::new();

        loop {
            interval.tick().await;
            // Always compute the fingerprint so a subscriber that connects after
            // startup does not inherit a stale empty baseline. Only the broadcast
            // itself is gated on receivers below.
            let fingerprint = aggregator::compute_tools_fingerprint_with_own(
                &reg_tools,
                stale_timeout,
                &http_client_tools,
                backend_timeout,
                Some(tools_own_host.as_str()),
                tools_own_port,
            )
            .await;

            if fingerprint != last_fingerprint {
                // First tick always "changes" from empty-string → don't push
                // on initial startup unless there are actually tools.
                if (!last_fingerprint.is_empty() || !fingerprint.is_empty())
                    && events_tx_tools.receiver_count() > 0
                {
                    tracing::debug!(
                        "Gateway: aggregated tool set changed — broadcasting tools/list_changed"
                    );
                    let notif = serde_json::to_string(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/tools/list_changed",
                        "params": {}
                    }))
                    .unwrap_or_default();
                    let _ = events_tx_tools.send(notif);
                }
                last_fingerprint = fingerprint;
            }
        }
    });

    // ── Aggregated prompts/list_changed watcher (every 3 s) ────────────
    // Mirror of the tools watcher — polls every live backend's
    // `prompts/list`, fingerprints the `{instance_id}:{prompt_name}` set,
    // and broadcasts one `notifications/prompts/list_changed` to gateway
    // SSE subscribers when the aggregated set changes.
    //
    // Skills opt into prompts by dropping a sibling `prompts.yaml`
    // (issues #351 / #355), so the cadence here matches the tools
    // watcher: skill load/unload is the same workflow trigger.
    let reg_prompts = registry.clone();
    let events_tx_prompts = events_tx.clone();
    let http_client_prompts = http_client.clone();
    let prompts_own_host = own_host.clone();
    let prompts_own_port = own_port;
    let prompts_watcher_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_fingerprint = String::new();

        loop {
            interval.tick().await;
            let fingerprint = aggregator::compute_prompts_fingerprint_with_own(
                &reg_prompts,
                stale_timeout,
                &http_client_prompts,
                backend_timeout,
                Some(prompts_own_host.as_str()),
                prompts_own_port,
            )
            .await;

            if fingerprint != last_fingerprint {
                if (!last_fingerprint.is_empty() || !fingerprint.is_empty())
                    && events_tx_prompts.receiver_count() > 0
                {
                    tracing::debug!(
                        "Gateway: aggregated prompt set changed — broadcasting prompts/list_changed"
                    );
                    let notif = serde_json::to_string(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/prompts/list_changed",
                        "params": {}
                    }))
                    .unwrap_or_default();
                    let _ = events_tx_prompts.send(notif);
                }
                last_fingerprint = fingerprint;
            }
        }
    });

    // ── Aggregated resources/list_changed watcher (every 3 s) ─────────────
    // Polls every live backend's `resources/list`, computes a set-fingerprint
    // of "{instance_id}:{backend_uri}" tuples, and broadcasts one
    // `notifications/resources/list_changed` to gateway SSE subscribers when
    // the aggregated set changes (resource added / removed on any DCC).
    //
    // Parallel to the tools watcher above. Same 3-second cadence, same
    // hysteresis (no broadcast on the empty→empty first tick), same
    // fail-soft semantics (unreachable backends contribute zero resources).
    //
    // #732: the instance-change watcher above already emits
    // `resources/list_changed` when the set of live DCC instances changes;
    // this watcher adds the second, finer signal — a resource added on an
    // existing backend, without any membership change.
    let reg_resources = registry.clone();
    let events_tx_resources = events_tx.clone();
    let http_client_resources = http_client.clone();
    let resources_own_host = own_host.clone();
    let resources_own_port = own_port;
    let resources_watcher_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_fingerprint = String::new();

        loop {
            interval.tick().await;
            let fingerprint = aggregator::compute_resources_fingerprint_with_own(
                &reg_resources,
                stale_timeout,
                &http_client_resources,
                backend_timeout,
                Some(resources_own_host.as_str()),
                resources_own_port,
            )
            .await;

            if fingerprint != last_fingerprint {
                if (!last_fingerprint.is_empty() || !fingerprint.is_empty())
                    && events_tx_resources.receiver_count() > 0
                {
                    tracing::debug!(
                        "Gateway: aggregated resource set changed — broadcasting resources/list_changed"
                    );
                    let notif = serde_json::to_string(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/resources/list_changed",
                        "params": {}
                    }))
                    .unwrap_or_default();
                    let _ = events_tx_resources.send(notif);
                }
                last_fingerprint = fingerprint;
            }
        }
    });

    // ── Backend SSE subscriber manager (#320) ─────────────────────────────
    // Multiplexes per-backend SSE notifications back to originating client
    // sessions. Each `ensure_subscribed` spawns a reconnecting task.
    // Uses `sse_http_client` (no client-level timeout) so the long-lived
    // SSE streams are not killed by a 30-second request timeout.
    let subscriber = sse_subscriber::SubscriberManager::with_limits(
        sse_http_client,
        route_ttl,
        max_routes_per_session,
    );
    // #322: GC loop — evicts stale (non-terminal) routes that outlive
    // their TTL. Terminal jobs are auto-evicted in `deliver`.
    let route_gc_handle = subscriber.spawn_route_gc();

    // ── Pre-subscribe registry hygiene (issue #419 + #556) ────────────────
    //
    // Before the backend subscriber loop starts fanning SSE connections at
    // everything in the registry, do a one-shot synchronous cleanup so we
    // don't waste reconnect budget on ghost rows left behind by a previous
    // crash. The periodic `cleanup_handle` above runs on a 15-second
    // cadence; without this synchronous pass, the subscriber would see
    // stale / dead-PID entries during the first ~15 s and pay the full
    // exponential-backoff retry cost trying to reach them.
    //
    // Issue #556: also probe every registered port and immediately deregister
    // instances whose TCP port is closed, even if the PID still appears alive.
    #[cfg(feature = "admin")]
    let mut startup_probe_evictions = Vec::new();
    {
        let r = registry.read().await;
        match r.prune_dead_pids() {
            Ok(n) if n > 0 => {
                tracing::info!(
                    reaped = n,
                    "Gateway: pre-subscribe dead-PID sweep reaped ghost entry/entries"
                );
            }
            Err(e) => tracing::warn!("Gateway: pre-subscribe dead-PID sweep error: {e}"),
            _ => {}
        }
        match r.cleanup_stale(stale_timeout) {
            Ok(n) if n > 0 => {
                tracing::info!(
                    evicted = n,
                    "Gateway: pre-subscribe stale sweep evicted instance(s)"
                );
            }
            Err(e) => tracing::warn!("Gateway: pre-subscribe stale sweep error: {e}"),
            _ => {}
        }
        // Startup port probe: evict any instance whose port is unreachable.
        match probe_and_evict_dead_instances(&r, stale_timeout, &own_host, own_port).await {
            Ok(evicted) if !evicted.is_empty() => {
                tracing::info!(
                    evicted = evicted.len(),
                    "Gateway: startup port probe evicted unreachable instance(s)"
                );
                #[cfg(feature = "admin")]
                {
                    startup_probe_evictions = evicted;
                }
            }
            Err(e) => tracing::warn!("Gateway: startup port probe error: {e}"),
            _ => {}
        }
    }

    // Periodically ensure every live backend has an active subscription.
    // The subscriber's internal DashMap makes repeat calls cheap, so we
    // just poll the instance registry at the same cadence as the
    // instance-change watcher.
    let reg_sub = registry.clone();
    let sub_for_task = subscriber.clone();
    let sub_own_host = own_host.clone();
    let sub_own_port = own_port;
    let sub_http_registry = http_instance_registry.clone();
    let sub_mdns_registry = mdns_instance_registry.clone();
    let backend_sub_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let urls: Vec<String> = {
                let r = reg_sub.read().await;
                let mut entries: Vec<_> = r
                    .list_all()
                    .into_iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                            && e.port != 0
                            && !e.is_stale(stale_timeout)
                            && !is_own_instance(e, &sub_own_host, sub_own_port)
                    })
                    .collect();
                let http_entries = sub_http_registry
                    .read()
                    .live_entries(std::time::SystemTime::now());
                let mdns_entries = sub_mdns_registry
                    .read()
                    .live_entries(std::time::SystemTime::now());
                let http_ids: std::collections::HashSet<_> =
                    http_entries.iter().map(|entry| entry.instance_id).collect();
                let mdns_ids: std::collections::HashSet<_> =
                    mdns_entries.iter().map(|entry| entry.instance_id).collect();
                entries.retain(|entry| {
                    !http_ids.contains(&entry.instance_id) && !mdns_ids.contains(&entry.instance_id)
                });
                entries.extend(mdns_entries);
                entries.extend(http_entries);
                entries
                    .into_iter()
                    .map(|e| crate::gateway::http_registration::entry_mcp_url(&e))
                    .collect()
            };
            // Add subscriptions for newly-appeared backends.
            for url in &urls {
                sub_for_task.ensure_subscribed(url);
            }
            // Remove subscriptions for backends that have disappeared from the
            // registry (stale / dead). Without this the reconnect loop runs
            // indefinitely for ports that no longer exist. Issue #402.
            sub_for_task.prune_unlisted(&urls);
        }
    });

    // ── Gateway HTTP server ────────────────────────────────────────────────
    // When `admin` feature is disabled the AuditMiddleware block is compiled
    // out, so `gw_state` is never mutated.  With `admin` enabled the block
    // below reassigns `gw_state.middleware_chain`, so `mut` is required.
    let instance_diagnostics =
        Arc::new(crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new());
    let traffic_capture = Arc::new(crate::gateway::traffic::TrafficCapture::from_env()?);
    let capability_index = Arc::new(crate::gateway::capability::CapabilityIndex::new());
    #[cfg(feature = "mdns")]
    let mdns_browser_handle = if mdns_discovery_enabled {
        crate::gateway::mdns_discovery::spawn_mdns_browser(
            mdns_instance_registry.clone(),
            http_client.clone(),
            events_tx.clone(),
            capability_index.clone(),
            mdns_ttl,
            mdns_probe_timeout,
        )
    } else {
        tokio::spawn(async {})
    };
    if traffic_capture.is_enabled() {
        tracing::info!("Gateway traffic capture enabled");
    }

    #[cfg_attr(not(feature = "admin"), allow(unused_mut))]
    let mut gw_state = GatewayState {
        registry: registry.clone(),
        http_instance_registry: http_instance_registry.clone(),
        mdns_instance_registry: mdns_instance_registry.clone(),
        stale_timeout,
        backend_timeout,
        async_dispatch_timeout,
        wait_terminal_timeout,
        server_name,
        server_version,
        own_host: own_host.clone(),
        own_port,
        http_client: http_client.clone(),
        yield_tx: yield_tx.clone(),
        events_tx,
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(HashMap::new())),
        client_attribution: Arc::new(
            crate::gateway::caller_attribution::ClientAttributionStore::default(),
        ),
        pending_calls: Arc::new(RwLock::new(HashMap::new())),
        subscriber,
        allow_unknown_tools,
        policy: Arc::new(policy),
        adapter_version,
        adapter_dcc,
        capability_index: capability_index.clone(),
        event_log: contention_log.clone(),
        #[cfg(feature = "prometheus")]
        gateway_metrics: gateway_metrics.clone(),
        middleware_chain: Arc::new(middleware_chain),
        instance_diagnostics: instance_diagnostics.clone(),
        traffic_capture,
        search_telemetry: Arc::new(crate::gateway::search_telemetry::SearchTelemetryStore::new()),
        debug_routes_enabled: false,
    };

    // ── Admin UI state (#772, #864) ────────────────────────────────────────
    // Wire AuditMiddleware into the default chain so /admin/api/calls is
    // populated. We prepend one AdminAuditSink-backed AuditMiddleware only
    // when admin is enabled AND the caller has not already registered their
    // own AuditMiddleware (detected by checking if the chain already has
    // before-call hooks — a heuristic that avoids double-recording for
    // operators who supply a custom SIEM sink).
    #[cfg(feature = "admin")]
    let sqlite_lane = if admin_enabled {
        let db_path =
            dcc_mcp_db::resolve_gateway_admin_sqlite_path(admin_persist.sqlite_path.as_ref(), None);
        match crate::gateway::admin::sqlite_lane::AdminSqliteLane::spawn(
            db_path,
            admin_persist.sqlite_retention_days.clamp(1, 3650),
        ) {
            Ok(l) => Some(l),
            Err(e) => {
                tracing::warn!(error = %e, "gateway admin SQLite unavailable");
                None
            }
        }
    } else {
        None
    };

    #[cfg(feature = "admin")]
    persist_startup_probe_evictions(&sqlite_lane, &startup_probe_evictions);

    #[cfg(feature = "admin")]
    let gw_router = {
        let admin_state_opt = if admin_enabled {
            let durable_store = crate::gateway::admin::state::DurableAuditStore::from_env();

            // 1. Shared ring buffer — the middleware writes here; the handler reads it.
            let audit_log: std::sync::Arc<crate::gateway::admin::state::AuditLog> =
                std::sync::Arc::new(parking_lot::Mutex::new(Vec::with_capacity(
                    ADMIN_AUDIT_RING_CAPACITY,
                )));
            if let Some(store) = &durable_store {
                audit_log.lock().extend(
                    store
                        .load_audit()
                        .into_iter()
                        .rev()
                        .take(ADMIN_AUDIT_RING_CAPACITY)
                        .rev(),
                );
            }

            // 2. Phase 2 trace log — ring buffer for per-call dispatch traces.
            let trace_log: std::sync::Arc<crate::gateway::admin::trace::TraceLog> =
                std::sync::Arc::new(crate::gateway::admin::trace::TraceLog::new(
                    crate::gateway::admin::trace::TraceLog::DEFAULT_CAPACITY,
                ));
            if let Some(store) = &durable_store {
                trace_log.extend(store.load_traces());
            }

            if let Some(ref lane) = sqlite_lane {
                let r = lane.reader();
                trace_log.extend(r.list_traces_since(None, 10_000));
                let from_sqlite = r.list_audits_recent(ADMIN_AUDIT_RING_CAPACITY);
                if !from_sqlite.is_empty() {
                    let mut buf = audit_log.lock();
                    let mut merged: Vec<crate::gateway::admin::state::AdminAuditRecord> =
                        from_sqlite;
                    merged.extend(buf.drain(..));
                    merged.sort_by_key(|a| a.timestamp);
                    let overflow = merged.len().saturating_sub(ADMIN_AUDIT_RING_CAPACITY);
                    if overflow > 0 {
                        merged.drain(0..overflow);
                    }
                    *buf = merged;
                }
            }

            // 3. Build the sink that feeds the audit ring buffer and the trace log.
            let mut admin_sink = crate::gateway::admin::state::AdminAuditSink::new(
                audit_log.clone(),
                ADMIN_AUDIT_RING_CAPACITY,
            )
            .with_trace_log(trace_log.clone());
            if let Some(ref lane) = sqlite_lane {
                admin_sink = admin_sink.with_sqlite_lane(lane.clone());
            }
            if let Some(store) = durable_store.clone() {
                admin_sink = admin_sink.with_durable_store(store);
            }
            let admin_sink: std::sync::Arc<dyn crate::gateway::middleware::AuditSink> =
                std::sync::Arc::new(admin_sink);

            // 4. Prepend AuditMiddleware to the chain so every tools/call
            //    passes through it.
            {
                let audit_mw = std::sync::Arc::new(
                    crate::gateway::middleware::AuditMiddleware::new(admin_sink),
                );
                let mut chain = (*gw_state.middleware_chain).clone();
                chain.prepend_before(audit_mw.clone());
                chain.prepend_after(audit_mw);
                gw_state.middleware_chain = std::sync::Arc::new(chain);
            }

            // 5. Build AdminState with audit log and trace log attached.
            let sqlite_reader = sqlite_lane.as_ref().map(|l| l.reader());
            Some(
                crate::gateway::admin::state::AdminState::new(gw_state.clone())
                    .with_audit_log(audit_log)
                    .with_trace_log(trace_log, sqlite_reader)
                    .with_skill_paths_snapshot(admin_persist.skill_paths_snapshot)
                    .with_admin_sqlite_lane(sqlite_lane.clone())
                    .with_skill_paths_reload(admin_persist.skill_paths_reload.clone()),
            )
        } else {
            None
        };
        build_gateway_router_with_admin(gw_state, admin_state_opt, &admin_path)
    };
    #[cfg(not(feature = "admin"))]
    let gw_router = build_gateway_router(gw_state);

    #[cfg(feature = "prometheus")]
    let gw_router = super::metrics::attach_gateway_metrics_route(gw_router);

    let actual = listener.local_addr()?;
    let remote_actual = remote_listener
        .as_ref()
        .and_then(|listener| listener.local_addr().ok());
    tracing::info!(
        "Gateway listening on http://{}  (GET /mcp = SSE stream, POST /mcp = MCP endpoint)",
        actual
    );
    if let Some(addr) = remote_actual {
        tracing::info!(
            "Gateway remote listener on http://{}  (LAN clients should use the machine IP, not 0.0.0.0)",
            addr
        );
    }

    let local_yield_rx = yield_rx.clone();
    let local_router = gw_router.clone();
    let gw_handle = tokio::spawn(async move {
        use std::net::SocketAddr;

        axum::serve(
            listener,
            local_router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let mut yield_rx = local_yield_rx;
            loop {
                if yield_rx.changed().await.is_err() {
                    break;
                }
                if *yield_rx.borrow() {
                    tracing::info!("Gateway: graceful shutdown triggered — releasing port");
                    break;
                }
            }
        })
        .await
        .ok();
    });

    let remote_handle = remote_listener
        .map(|remote_listener| {
            let mut remote_yield_rx = yield_rx.clone();
            let remote_router = gw_router.clone();
            tokio::spawn(async move {
                use std::net::SocketAddr;

                axum::serve(
                    remote_listener,
                    remote_router.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .with_graceful_shutdown(async move {
                    loop {
                        if remote_yield_rx.changed().await.is_err() {
                            break;
                        }
                        if *remote_yield_rx.borrow() {
                            tracing::info!(
                                "Gateway remote listener: graceful shutdown triggered — releasing port"
                            );
                            break;
                        }
                    }
                })
                .await
                .ok();
            })
        })
        .unwrap_or_else(|| tokio::spawn(async {}));

    // Periodic health-check task (issue #556)
    let health_cfg = health::HealthCheckConfig {
        own_host: own_host.clone(),
        own_port,
        health_check_interval_secs,
        health_check_failures,
        #[cfg(feature = "admin")]
        admin_sqlite_lane: sqlite_lane.clone(),
        #[cfg(feature = "prometheus")]
        metrics: gateway_metrics.clone(),
    };
    let health_check_handle = health::spawn_health_check_task(
        registry.clone(),
        http_client.clone(),
        contention_log.clone(),
        instance_diagnostics,
        health_cfg,
    );

    // ── Prometheus metrics updater (issue #559) ───────────────────────────
    #[cfg(feature = "prometheus")]
    let metrics_handle = metrics::spawn_metrics_updater(registry.clone(), stale_timeout);

    // Combine all tasks under one abort handle. The task group owns every
    // spawned gateway child; when the supervisor is aborted or a cooperative
    // yield is requested, dropping the group aborts the children instead of
    // detaching them as leaked background work.
    let supervisor_yield_rx = yield_rx.clone();
    let task_handles = vec![
        cleanup_handle,
        watcher_handle,
        tools_watcher_handle,
        prompts_watcher_handle,
        resources_watcher_handle,
        backend_sub_handle,
        route_gc_handle,
        health_check_handle,
        gw_handle,
        remote_handle,
    ];
    #[cfg(feature = "mdns")]
    let mut task_handles = task_handles;
    #[cfg(feature = "mdns")]
    task_handles.push(mdns_browser_handle);
    #[cfg(feature = "prometheus")]
    let mut task_handles = task_handles;
    #[cfg(feature = "prometheus")]
    task_handles.push(metrics_handle);

    let combined = tokio::spawn(async move {
        let task_group = GatewayTaskGroup::new(task_handles);
        tokio::select! {
            _ = wait_for_gateway_yield(supervisor_yield_rx) => {}
            _ = task_group.wait_all() => {}
        }
    });

    // ── Post-spawn self-probe (issue #303) ────────────────────────────────
    //
    // `bind()` succeeding does not guarantee the accept-loop is actually
    // running — under PyO3-embedded hosts (e.g. mayapy on Windows) a freshly
    // spawned Tokio task can be starved long enough that the caller is told
    // `is_gateway = true` while clients see `CONNECTION REFUSED` or
    // `CONNECTION TIMED OUT` on the gateway port.
    //
    // Connecting to our own address forces the runtime to drive the accept
    // loop at least once; if that fails within the budget we trigger a yield
    // so the listener is dropped, then propagate an error so the caller can
    // fall back to plain-instance mode.
    if let Err(e) = self_probe_listener(actual).await {
        tracing::warn!(
            addr = %actual,
            error = %e,
            "Gateway self-probe failed — aborting gateway role and releasing port"
        );
        // Trigger graceful shutdown of the listener task.
        let _ = yield_tx.send(true);
        // Give the shutdown a brief moment to run so the port is released
        // before the caller decides what to do next. We do NOT await the
        // task's JoinHandle here because the runtime may be starved — we
        // rely on `combined.abort_handle()` / `yield_tx` for cleanup.
        tokio::time::sleep(Duration::from_millis(50)).await;
        return Err(format!("gateway listener self-probe failed at {actual}: {e}").into());
    }
    if let Some(addr) = remote_actual
        && let Err(e) = self_probe_listener(addr).await
    {
        tracing::warn!(
            addr = %addr,
            error = %e,
            "Gateway remote listener self-probe failed — local gateway remains active"
        );
    }

    Ok(GatewayTasks {
        abort: combined.abort_handle(),
        supervisor: combined,
        yield_tx,
    })
}

#[cfg(feature = "admin")]
fn persist_startup_probe_evictions(
    lane: &Option<crate::gateway::admin::sqlite_lane::AdminSqliteLane>,
    evictions: &[dcc_mcp_transport::discovery::types::ServiceEntry],
) {
    if let Some(lane) = lane {
        for entry in evictions {
            lane.try_persist_deregistered_instance(entry, "startup port probe unreachable");
        }
    }
}

#[cfg(all(test, feature = "admin-persist-sqlite"))]
mod tests {
    use super::*;
    use dcc_mcp_transport::discovery::types::ServiceEntry;

    #[test]
    fn startup_probe_evictions_are_persisted_to_admin_sqlite() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("startup-deregistered.sqlite");
        let lane = crate::gateway::admin::sqlite_lane::AdminSqliteLane::spawn(db_path.clone(), 30)
            .expect("spawn lane");
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18815);
        let instance_id = entry.instance_id.to_string();

        persist_startup_probe_evictions(&Some(lane.clone()), &[entry]);
        drop(lane);

        let rows = crate::gateway::admin::sqlite_lane::AdminSqliteReader::new(db_path)
            .list_deregistered_instances(10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["reason"], "startup port probe unreachable");
        assert_eq!(rows[0]["dcc_type"], "maya");
        assert_eq!(rows[0]["instance_id"], instance_id);
    }
}
