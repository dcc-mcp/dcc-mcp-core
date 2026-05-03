use super::*;

use dcc_mcp_transport::error::TransportResult;

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
    adapter_version: Option<String>,
    adapter_dcc: Option<String>,
    tool_exposure: super::config::GatewayToolExposure,
    cursor_safe_tool_names: bool,
) -> Result<GatewayTasks, Box<dyn std::error::Error + Send + Sync>> {
    // ── Yield channel ─────────────────────────────────────────────────────
    let (yield_tx, mut yield_rx) = watch::channel(false);
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
    let sse_http_client = reqwest::Client::builder().build()?;

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
    let cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            let r = reg_cleanup.read().await;

            // Keep the sentinel fresh first — it's what `has_newer_sentinel`
            // and every consumer of `list_instances("__gateway__")` rely on.
            let _ = r.heartbeat(&sentinel_key_cleanup);

            match r.cleanup_stale(stale_timeout) {
                Ok(n) if n > 0 => tracing::info!("Gateway: evicted {} stale instance(s)", n),
                Err(e) => tracing::warn!("Gateway: stale cleanup error: {e}"),
                _ => {}
            }

            match r.prune_dead_pids() {
                Ok(n) if n > 0 => tracing::info!("Gateway: reaped {} ghost entry/entries", n),
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
                let _ = yield_tx_cleanup.send(true);
                break;
            }
        }
    });

    // ── Instance-change watcher (every 2 s) ───────────────────────────────
    // Detects when DCC instances join or leave and broadcasts
    // `notifications/resources/list_changed` to all connected SSE clients.
    let reg_watch = registry.clone();
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
                let mut keys: Vec<String> = r
                    .list_all()
                    .into_iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                            && !e.is_stale(stale_timeout)
                            && !is_own_instance(e, &watch_own_host, watch_own_port)
                    })
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
            Ok(n) if n > 0 => {
                tracing::info!(
                    evicted = n,
                    "Gateway: startup port probe evicted unreachable instance(s)"
                );
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
    let backend_sub_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let urls: Vec<String> = {
                let r = reg_sub.read().await;
                r.list_all()
                    .into_iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                            && !e.is_stale(stale_timeout)
                            && !is_own_instance(e, &sub_own_host, sub_own_port)
                    })
                    .map(|e| format!("http://{}:{}/mcp", e.host, e.port))
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
    let gw_state = GatewayState {
        registry: registry.clone(),
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
        pending_calls: Arc::new(RwLock::new(HashMap::new())),
        subscriber,
        allow_unknown_tools,
        adapter_version,
        adapter_dcc,
        tool_exposure,
        cursor_safe_tool_names,
        capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
    };
    let gw_router = build_gateway_router(gw_state);

    #[cfg(feature = "prometheus")]
    let gw_router = super::metrics::attach_gateway_metrics_route(gw_router);

    let actual = listener.local_addr()?;
    tracing::info!(
        "Gateway listening on http://{}  (GET /mcp = SSE stream, POST /mcp = MCP endpoint)",
        actual
    );

    let gw_handle = tokio::spawn(async move {
        axum::serve(listener, gw_router)
            .with_graceful_shutdown(async move {
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

    // ── Periodic health-check task (issue #556) ───────────────────────────
    // Every 30 s, attempt a lightweight `GET /health` against every registered
    // instance. TCP reachability alone is not enough: Maya commandPort also
    // accepts TCP bytes, but it is not an MCP HTTP backend and JSON-RPC POSTs to
    // it can trigger a modal security dialog (#592). After 2 consecutive
    // failures mark the row Unreachable; after 3 stale rounds auto-deregister.
    let reg_health = registry.clone();
    let health_http_client = http_client.clone();
    let health_own_host = own_host.clone();
    let health_own_port = own_port;
    let health_check_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // failure_count keyed by "dcc_type:instance_id"
        let mut failure_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        loop {
            interval.tick().await;
            let entries = {
                let r = reg_health.read().await;
                r.list_all()
                    .into_iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                            && !is_own_instance(e, &health_own_host, health_own_port)
                    })
                    .collect::<Vec<_>>()
            };

            for entry in entries {
                let mcp_url = format!("http://{}:{}/mcp", entry.host, entry.port);
                // #713: three-state probe — distinguishes a booting
                // backend (process alive, `/v1/readyz` returns 503) from
                // a process-dead one. Booting backends keep their row
                // but get status Booting until readiness flips green.
                let outcome = crate::gateway::backend_client::probe_mcp_readiness(
                    &health_http_client,
                    &mcp_url,
                    Duration::from_secs(5),
                )
                .await;

                let key = format!("{}:{}", entry.dcc_type, entry.instance_id);

                // ── Ready ──────────────────────────────────────────
                if outcome.is_ready() {
                    let recovered_from_failure = failure_counts.remove(&key).is_some();
                    let was_not_available = !matches!(
                        entry.status,
                        dcc_mcp_transport::discovery::types::ServiceStatus::Available,
                    );
                    if recovered_from_failure || was_not_available {
                        let r = reg_health.read().await;
                        let _ = r.update_status(
                            &entry.key(),
                            dcc_mcp_transport::discovery::types::ServiceStatus::Available,
                        );
                        tracing::info!(
                            dcc_type = %entry.dcc_type,
                            instance_id = %entry.instance_id,
                            previous_status = %entry.status,
                            "Readiness probe green — marking Available"
                        );
                    }
                    continue;
                }

                // ── Booting (alive but red) ────────────────────────
                // Issue #713: don't count a booting backend as a
                // consecutive failure and don't deregister it. We
                // just flip its status to Booting so `list_dcc_instances`
                // and the aggregator can filter traffic away from it.
                if outcome.is_alive() {
                    if !matches!(
                        entry.status,
                        dcc_mcp_transport::discovery::types::ServiceStatus::Booting,
                    ) {
                        let r = reg_health.read().await;
                        let _ = r.update_status(
                            &entry.key(),
                            dcc_mcp_transport::discovery::types::ServiceStatus::Booting,
                        );
                        tracing::info!(
                            dcc_type = %entry.dcc_type,
                            instance_id = %entry.instance_id,
                            previous_status = %entry.status,
                            "Backend booting (GET /v1/readyz red) — marking Booting without deregister"
                        );
                    }
                    // Clear any prior consecutive-failure tally: the
                    // backend is alive, just not ready yet.
                    failure_counts.remove(&key);
                    continue;
                }

                // ── Unreachable ────────────────────────────────────
                let count = failure_counts.entry(key.clone()).or_insert(0);
                *count += 1;
                tracing::warn!(
                    dcc_type = %entry.dcc_type,
                    instance_id = %entry.instance_id,
                    consecutive_failures = *count,
                    "Health check failed"
                );

                if *count >= 2 {
                    // Mark Unreachable so live_instances filters it out.
                    let r = reg_health.read().await;
                    let _ = r.update_status(
                        &entry.key(),
                        dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable,
                    );
                }

                if *count >= 3 {
                    // Auto-deregister after persistent unreachability.
                    let r = reg_health.read().await;
                    let _ = r.deregister(&entry.key());
                    failure_counts.remove(&key);
                    tracing::info!(
                        dcc_type = %entry.dcc_type,
                        instance_id = %entry.instance_id,
                        "Auto-deregistered after 3 consecutive health-check failures"
                    );
                }
            }
        }
    });

    // ── Prometheus metrics updater (issue #559) ───────────────────────────
    #[cfg(feature = "prometheus")]
    let metrics_handle = {
        let reg_metrics = registry.clone();
        let stale_timeout_metrics = stale_timeout;
        tokio::spawn(async move {
            let exporter = dcc_mcp_telemetry::PrometheusExporter::new();
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                let r = reg_metrics.read().await;
                let all = r.list_all();
                let active = all
                    .iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                            && !e.is_stale(stale_timeout_metrics)
                            && !matches!(
                                e.status,
                                dcc_mcp_transport::discovery::types::ServiceStatus::ShuttingDown
                                    | dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable
                            )
                    })
                    .count() as i64;
                let stale = all
                    .iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE && e.is_stale(stale_timeout_metrics)
                    })
                    .count() as i64;
                exporter.set_instances_total("active", active);
                exporter.set_instances_total("stale", stale);
            }
        })
    };

    // Combine all tasks under one abort handle.
    #[cfg(feature = "prometheus")]
    let combined = tokio::spawn(async move {
        let _ = tokio::join!(
            cleanup_handle,
            watcher_handle,
            tools_watcher_handle,
            prompts_watcher_handle,
            resources_watcher_handle,
            backend_sub_handle,
            route_gc_handle,
            health_check_handle,
            gw_handle,
            metrics_handle
        );
    });
    #[cfg(not(feature = "prometheus"))]
    let combined = tokio::spawn(async move {
        let _ = tokio::join!(
            cleanup_handle,
            watcher_handle,
            tools_watcher_handle,
            prompts_watcher_handle,
            resources_watcher_handle,
            backend_sub_handle,
            route_gc_handle,
            health_check_handle,
            gw_handle
        );
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

    Ok(GatewayTasks {
        abort: combined.abort_handle(),
        supervisor: combined,
        yield_tx,
    })
}

/// Verify that the gateway accept-loop is actually running by connecting to it.
///
/// Retries a small number of times with short back-off to give the Tokio
/// runtime a chance to schedule the `axum::serve` task — necessary under
/// PyO3-embedded hosts where workers are slow to pick up newly spawned tasks
/// (issue #303).
/// On gateway startup, probe every registered instance's TCP port and
/// deregister any that are unreachable. Complements `prune_dead_pids`
/// which only checks PID liveness — a process may be alive but its MCP
/// listener already shut down (issue #556).
pub(crate) async fn probe_and_evict_dead_instances(
    registry: &FileRegistry,
    stale_timeout: Duration,
    own_host: &str,
    own_port: u16,
) -> TransportResult<usize> {
    let entries: Vec<_> = registry
        .list_all()
        .into_iter()
        .filter(|e| {
            e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                && !e.is_stale(stale_timeout)
                && !is_own_instance(e, own_host, own_port)
        })
        .collect();

    let mut evicted = 0usize;
    for entry in entries {
        let addr = format!("{}:{}", entry.host, entry.port);
        let reachable = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        .is_ok_and(|r| r.is_ok());

        if !reachable {
            registry.deregister(&entry.key())?;
            evicted += 1;
            tracing::info!(
                dcc_type = %entry.dcc_type,
                instance_id = %entry.instance_id,
                addr = %addr,
                "Startup probe: instance unreachable — deregistered"
            );
        }
    }
    Ok(evicted)
}

pub(crate) async fn self_probe_listener(addr: std::net::SocketAddr) -> Result<(), std::io::Error> {
    const MAX_ATTEMPTS: u32 = 10;
    const ATTEMPT_TIMEOUT: Duration = Duration::from_millis(200);
    const BACKOFF: Duration = Duration::from_millis(100);

    let mut last_err: Option<std::io::Error> = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match tokio::time::timeout(ATTEMPT_TIMEOUT, tokio::net::TcpStream::connect(addr)).await {
            Ok(Ok(_stream)) => {
                tracing::debug!(addr = %addr, attempt, "Gateway self-probe succeeded");
                return Ok(());
            }
            Ok(Err(e)) => {
                tracing::debug!(addr = %addr, attempt, error = %e, "Gateway self-probe: connect error");
                last_err = Some(e);
            }
            Err(_) => {
                tracing::debug!(addr = %addr, attempt, "Gateway self-probe: connect timed out");
                last_err = Some(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "self-probe connect timed out",
                ));
            }
        }
        tokio::time::sleep(BACKOFF).await;
    }

    Err(last_err.unwrap_or_else(|| std::io::Error::other("self-probe failed with no error")))
}
