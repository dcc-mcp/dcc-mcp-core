use super::*;

use futures::FutureExt;

/// Extract a human-readable message from a panic payload.
fn panic_message(info: &dyn std::any::Any) -> String {
    if let Some(s) = info.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Orchestrates FileRegistry registration, heartbeat, stale cleanup, and the
/// optional gateway HTTP server.
pub struct GatewayRunner {
    /// Gateway configuration.
    pub config: GatewayConfig,
    /// Shared file-based service registry.
    pub registry: Arc<RwLock<FileRegistry>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResidentGatewayHealth {
    Missing,
    Healthy,
    Unhealthy,
}

impl GatewayRunner {
    /// Create a new runner, initializing (or loading) the `FileRegistry` from
    /// `config.registry_dir`, the `DCC_MCP_REGISTRY_DIR` environment variable,
    /// or a system temp dir (in that order of precedence).
    pub fn new(config: GatewayConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let dir = config
            .registry_dir
            .clone()
            .or_else(|| {
                std::env::var("DCC_MCP_REGISTRY_DIR")
                    .ok()
                    .map(std::path::PathBuf::from)
            })
            .unwrap_or_else(|| std::env::temp_dir().join("dcc-mcp-registry"));
        let registry = FileRegistry::new(&dir)?;
        Ok(Self {
            config,
            registry: Arc::new(RwLock::new(registry)),
        })
    }

    /// Register `entry`, start heartbeat, and run the liveness-aware gateway election.
    ///
    /// ## Election algorithm
    ///
    /// 1. **Win**: binds the gateway port → becomes gateway immediately.
    ///    - Registers a `__gateway__` sentinel with its own version in FileRegistry.
    ///    - Periodically checks whether any live instance has a *newer* version;
    ///      if so, initiates voluntary yield (graceful shutdown of its listener).
    ///
    /// 2. **Lose + healthy resident**: registers as a plain DCC instance
    ///    even when this process has a newer version. Healthy co-existing
    ///    DCCs must not drop active MCP clients just because another adapter
    ///    started.
    ///
    /// 3. **Lose + missing or unhealthy resident**:
    ///    - First tries a cooperative [`POST /gateway/yield`] when this
    ///      process is newer and the incumbent supports it.
    ///    - Regardless of the response, enters a **challenger retry loop** that
    ///      polls the port every `challenger_poll_interval_secs` for up to
    ///      `challenger_timeout_secs`.
    ///    - When the port becomes free (old gateway yielded or crashed),
    ///      the challenger binds it and becomes the new gateway.
    ///
    /// ## Live scene/version updates
    ///
    /// Pass `metadata_provider` to keep the `scene` and `version` fields in the
    /// `FileRegistry` in sync with the running DCC application.  The closure is
    /// called on every heartbeat tick and the returned `(scene, version)` pair is
    /// written via `FileRegistry::update_metadata`.  This ensures that
    /// `list_dcc_instances` always shows the currently open scene — even when the
    /// user opens a different file after the server was started.
    pub async fn start(
        &self,
        entry: ServiceEntry,
        metadata_provider: Option<MetadataProvider>,
    ) -> Result<GatewayHandle, Box<dyn std::error::Error + Send + Sync>> {
        let service_key = entry.key();

        // ── Register in FileRegistry ─────────────────────────────────────
        {
            let reg = self.registry.read().await;
            reg.register(entry)?;
        }
        tracing::info!(instance = %service_key.instance_id, "Registered in FileRegistry");

        // ── Heartbeat task ────────────────────────────────────────────────
        //
        // Besides touching the timestamp, every tick also calls update_metadata
        // when a metadata_provider is present.  This keeps the `scene` field
        // in FileRegistry current so that list_dcc_instances always reflects
        // the currently open DCC scene without requiring a server restart.
        //
        // The task is wrapped in a restart loop so that a panic does not silently
        // abort heartbeats (issue #554).
        let heartbeat_abort = if self.config.heartbeat_secs > 0 {
            let reg = self.registry.clone();
            let key = service_key.clone();
            let secs = self.config.heartbeat_secs;
            let provider = metadata_provider;
            let h = tokio::spawn(async move {
                loop {
                    let reg = reg.clone();
                    let key_inner = key.clone();
                    let provider = provider.clone();
                    let result = std::panic::AssertUnwindSafe(async move {
                        let mut tick = tokio::time::interval(Duration::from_secs(secs));
                        loop {
                            tick.tick().await;
                            let r = reg.read().await;
                            if let Some(ref prov) = provider {
                                let snap = prov();
                                if !snap.documents.is_empty() {
                                    // Multi-document DCC (Photoshop, After Effects…):
                                    // update active document + full open-document list + label.
                                    let _ = r.update_documents(
                                        &key_inner,
                                        snap.scene.as_deref(),
                                        &snap.documents,
                                        snap.display_name.as_deref(),
                                    );
                                } else {
                                    // Single-document DCC (Maya, Blender, Houdini…):
                                    // update scene path and version only.
                                    let _ = r.update_metadata(
                                        &key_inner,
                                        snap.scene.as_deref(),
                                        snap.version.as_deref(),
                                    );
                                }
                            } else {
                                let _ = r.heartbeat(&key_inner);
                            }
                        }
                    })
                    .catch_unwind()
                    .await;

                    let msg = match result {
                        Err(panic_info) => panic_message(&*panic_info),
                        Ok(()) => {
                            // Normal loop exit (should not happen)
                            break;
                        }
                    };
                    tracing::error!(
                        instance = %key.instance_id,
                        panic = %msg,
                        "Heartbeat task panicked — restarting in 5s"
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            });
            Some(h.abort_handle())
        } else {
            None
        };

        // ── Gateway election ──────────────────────────────────────────────
        let (
            is_gateway,
            gateway_abort,
            challenger_abort,
            gateway_supervisor,
            gateway_thread,
            sentinel_key,
        ) = if self.config.gateway_port > 0 {
            let outcome = self.run_election().await?;
            (
                outcome.is_gateway,
                outcome.gateway_abort,
                outcome.challenger_abort,
                outcome.gateway_supervisor,
                outcome.gateway_thread,
                outcome.sentinel_key,
            )
        } else {
            (false, None, None, None, None, None)
        };

        // Issue #718: on clean shutdown the `Drop` impl deregisters every
        // key we own (the instance row, plus the gateway sentinel if we
        // won the election), so `services.json` no longer carries zombie
        // "available" rows for the full `stale_timeout_secs` window.
        let mut pending_deregister = Vec::with_capacity(2);
        pending_deregister.push(service_key.clone());
        if let Some(k) = sentinel_key {
            pending_deregister.push(k);
        }

        Ok(GatewayHandle {
            is_gateway,
            service_key,
            heartbeat_abort,
            gateway_abort,
            gateway_supervisor,
            gateway_thread,
            challenger_abort,
            registry: self.registry.clone(),
            pending_deregister,
        })
    }

    /// Core liveness-aware election logic, extracted for clarity.
    ///
    /// Public so out-of-process sidecars can re-run election after the
    /// incumbent gateway dies without restarting the whole DCC host.
    pub async fn run_election(
        &self,
    ) -> Result<ElectionOutcome, Box<dyn std::error::Error + Send + Sync>> {
        let stale_timeout = Duration::from_secs(self.config.stale_timeout_secs);
        let backend_timeout = Duration::from_millis(self.config.backend_timeout_ms);
        let async_dispatch_timeout = Duration::from_millis(self.config.async_dispatch_timeout_ms);
        let wait_terminal_timeout = Duration::from_millis(self.config.wait_terminal_timeout_ms);
        let route_ttl = Duration::from_secs(self.config.route_ttl_secs);
        let max_routes_per_session = self.config.max_routes_per_session as usize;
        let own_version = self.config.server_version.clone();
        let own_adapter_version = self.config.adapter_version.clone();
        let own_adapter_dcc = self.config.adapter_dcc.clone();
        let gateway_name = self.effective_gateway_name();

        // Prune dead FileRegistry entries BEFORE election so:
        //
        //   1. The win path writes its sentinel into a clean registry —
        //      no zombie ``__gateway__`` row from a previously-crashed
        //      gateway lingering alongside ours. Otherwise peers'
        //      ``list_instances(__gateway__).next()`` could pick up the
        //      ghost and run challenger logic against a phantom version.
        //
        //   2. The loss path's resident-sentinel lookup is honest: when
        //      bind fails because the kernel still holds the port in
        //      TIME_WAIT (Windows: up to 2 min after a gateway crash),
        //      ``resident`` correctly reports "no live gateway" instead
        //      of returning the dead one. Without this prune, peers
        //      running the same crate version as the dead gateway would
        //      stay as plain instances forever — never even spawning
        //      the challenger loop that would eventually take over.
        //
        // The prune is cheap on a healthy registry (no rows to evict,
        // no I/O). The flush only happens when at least one row is
        // dropped. RFC #998 follow-up (2026-05-16).
        let pruned = {
            let reg = self.registry.read().await;
            reg.prune_dead_entries().unwrap_or(0)
        };
        if pruned > 0 {
            tracing::info!(
                port = self.config.gateway_port,
                pruned,
                "Pruned dead FileRegistry entries before election"
            );
        }

        match try_bind_port_opt(&self.config.host, self.config.gateway_port).await {
            // ── We won the port ───────────────────────────────────────────
            Some(listener) => {
                // Sweep any leftover ``__gateway__`` sentinels before
                // writing our own. Only one process can actually own the
                // port we just bound — the OS guarantees that — so any
                // pre-existing sentinel row is stale. Rows belonging to
                // dead PIDs would already have been removed by the
                // ``prune_dead_entries`` call above, but rows owned by
                // LIVE peers that previously held the gateway role
                // (then lost it in a restart / failover) survive that
                // prune. Leaving them in the registry would let peers'
                // resident-sentinel lookups pick up a phantom version
                // and run challenger logic against it, or worse fall
                // into the "same-or-stronger" plain-instance branch
                // when our own sentinel happens to sort second.
                //
                // We are about to register the authoritative sentinel
                // for this port — replacement, not co-existence. RFC
                // #998 follow-up (sentinel-rotation pollution observed
                // in three-Maya live session, 2026-05-16).
                {
                    let reg = self.registry.read().await;
                    let existing = reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE);
                    for entry in &existing {
                        let _ = reg.deregister(&entry.key());
                    }
                    if !existing.is_empty() {
                        tracing::info!(
                            cleared = existing.len(),
                            "Cleared stale __gateway__ sentinels before writing our own"
                        );
                    }
                }

                // Write a sentinel entry so challengers can read our version.
                // `ServiceEntry::new` auto-populates `pid` with our process id,
                // so a crash of *this* process makes the sentinel prunable by
                // `prune_dead_pids` on other peers (issue #227).
                //
                // Issue maya#137: stamp adapter_version + adapter_dcc on the
                // sentinel so peers can apply the three-tier election
                // comparison (crate version → adapter version → real-DCC
                // tiebreaker).
                let mut sentinel = ServiceEntry::new(
                    GATEWAY_SENTINEL_DCC_TYPE,
                    &self.config.host,
                    self.config.gateway_port,
                );
                sentinel.version = Some(own_version.clone());
                sentinel.adapter_version = own_adapter_version.clone();
                sentinel.adapter_dcc = own_adapter_dcc.clone();
                stamp_gateway_sentinel(&mut sentinel, &gateway_name, "active");
                let sentinel_key = sentinel.key();
                {
                    let reg = self.registry.read().await;
                    let _ = reg.register(sentinel);
                }

                let remote_listener = self.bind_remote_gateway_listener().await;

                match start_gateway_tasks(
                    listener,
                    remote_listener,
                    self.registry.clone(),
                    stale_timeout,
                    backend_timeout,
                    async_dispatch_timeout,
                    wait_terminal_timeout,
                    route_ttl,
                    max_routes_per_session,
                    format!("{} ({gateway_name})", self.config.server_name),
                    own_version.clone(),
                    sentinel_key.clone(),
                    self.config.host.clone(),
                    self.config.gateway_port,
                    self.config.allow_unknown_tools,
                    #[cfg(feature = "mdns")]
                    self.config.discover_mdns,
                    self.config.relay_sources.clone(),
                    self.config.policy.clone(),
                    own_adapter_version.clone(),
                    own_adapter_dcc.clone(),
                    self.config.middleware_chain.clone(),
                    #[cfg(feature = "admin")]
                    self.config.admin_enabled,
                    #[cfg(feature = "admin")]
                    self.config.admin_path.clone(),
                    #[cfg(feature = "admin")]
                    self.config.admin_persist.clone(),
                    self.config.health_check_interval_secs,
                    self.config.health_check_failures,
                    self.config.auth.clone(),
                )
                .await
                {
                    Ok(tasks) => {
                        tracing::info!(version = %own_version, "Won gateway election");
                        Ok(ElectionOutcome {
                            is_gateway: true,
                            gateway_abort: Some(tasks.abort),
                            challenger_abort: None,
                            gateway_supervisor: Some(tasks.supervisor),
                            gateway_thread: None,
                            // Issue #718: winners must also deregister the
                            // `__gateway__` sentinel on clean shutdown.
                            sentinel_key: Some(sentinel_key),
                        })
                    }
                    // Issue #303: bind() succeeded but the accept-loop never
                    // came up (or the self-probe timed out). Fall back to
                    // plain-instance mode instead of failing the whole
                    // server start — the instance listener is unaffected.
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            version = %own_version,
                            "Gateway tasks failed to become healthy — falling back to plain-instance mode"
                        );
                        // Issue #718: the sentinel was written before
                        // `start_gateway_tasks` failed. Clean it up now
                        // so peers don't see a phantom gateway.
                        {
                            let reg = self.registry.read().await;
                            let _ = reg.deregister(&sentinel_key);
                        }
                        Ok(ElectionOutcome {
                            is_gateway: false,
                            gateway_abort: None,
                            challenger_abort: None,
                            gateway_supervisor: None,
                            gateway_thread: None,
                            sentinel_key: None,
                        })
                    }
                }
            }

            // ── Port is taken — liveness-aware challenger logic ───────────
            None => {
                // Read the sentinel so logs and optional cooperative-yield
                // requests can identify the resident gateway profile.
                let resident = {
                    let reg = self.registry.read().await;
                    reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE)
                        .into_iter()
                        .next()
                };

                let gw_version = resident
                    .as_ref()
                    .and_then(|e| e.version.clone())
                    .unwrap_or_default();
                let gw_adapter_version = resident.as_ref().and_then(|e| e.adapter_version.clone());
                let gw_adapter_dcc = resident.as_ref().and_then(|e| e.adapter_dcc.clone());

                let resident_health = if resident.is_none() {
                    ResidentGatewayHealth::Missing
                } else {
                    probe_resident_gateway_health(
                        &self.config.host,
                        self.config.gateway_port,
                        Duration::from_secs(self.config.challenger_poll_interval_secs.clamp(1, 5)),
                    )
                    .await
                };

                // Three cases reach this branch:
                //   A. Resident exists and /health passes    -> plain
                //   B. Resident exists but /health fails     -> challenger
                //   C. Resident is gone (TIME_WAIT / race    -> challenger
                //      with no live sentinel; the OS still
                //      holds the address but no peer is the
                //      authoritative owner)
                //
                // (C) is the post-crash recovery case: the previous
                // gateway died, ``prune_dead_entries`` cleared the stale
                // sentinel, but the kernel still keeps the port in
                // TIME_WAIT so our first bind attempt failed. Without
                // spawning a challenger here, peers running the same
                // crate version as the dead gateway would never poll
                // for the port to free up — they would stay as plain
                // instances forever, leaving 9765 dark until someone
                // restarts a DCC. The challenger loop polls up to
                // ``challenger_timeout_secs`` and wins the bind the
                // moment TIME_WAIT releases (#893 follow-up).
                let challenger_reason = challenger_reason(resident_health);

                if let Some(challenger_reason) = challenger_reason {
                    tracing::info!(
                        own = %own_version,
                        own_adapter_version = ?own_adapter_version,
                        own_adapter_dcc = ?own_adapter_dcc,
                        gateway = %gw_version,
                        gateway_adapter_version = ?gw_adapter_version,
                        gateway_adapter_dcc = ?gw_adapter_dcc,
                        resident_health = ?resident_health,
                        "{}",
                        challenger_reason,
                    );
                    let challenger_abort = self.spawn_challenger_loop(&own_version, &gw_version);
                    // Return as non-gateway for now; challenger loop will promote us later.
                    Ok(ElectionOutcome {
                        is_gateway: false,
                        gateway_abort: None,
                        challenger_abort: Some(challenger_abort),
                        gateway_supervisor: None,
                        gateway_thread: None,
                        sentinel_key: None,
                    })
                } else {
                    tracing::info!(
                        port = self.config.gateway_port,
                        gateway_version = %gw_version,
                        gateway_adapter_version = ?gw_adapter_version,
                        gateway_adapter_dcc = ?gw_adapter_dcc,
                        own_version = %own_version,
                        own_adapter_version = ?own_adapter_version,
                        own_adapter_dcc = ?own_adapter_dcc,
                        resident_health = ?resident_health,
                        "Gateway port held by healthy resident — running as plain DCC instance"
                    );
                    Ok(ElectionOutcome {
                        is_gateway: false,
                        gateway_abort: None,
                        challenger_abort: None,
                        gateway_supervisor: None,
                        gateway_thread: None,
                        sentinel_key: None,
                    })
                }
            }
        }
    }

    /// Spawn the background challenger loop.
    ///
    /// 1. Sends a cooperative [`POST /gateway/yield`] to ask the old gateway
    ///    nicely (works if it runs `≥ 0.12.29`; ignored otherwise).
    /// 2. Polls the port every 10 s until it becomes free or the timeout fires.
    /// 3. When the port frees up, calls [`start_gateway_tasks`] to fully take over.
    fn spawn_challenger_loop(&self, own_version: &str, gw_version: &str) -> AbortHandle {
        let host = self.config.host.clone();
        let port = self.config.gateway_port;
        let own_ver = own_version.to_owned();
        let gw_ver = gw_version.to_owned();
        let registry = self.registry.clone();
        let stale_timeout = Duration::from_secs(self.config.stale_timeout_secs);
        let backend_timeout = Duration::from_millis(self.config.backend_timeout_ms);
        let async_dispatch_timeout = Duration::from_millis(self.config.async_dispatch_timeout_ms);
        let wait_terminal_timeout = Duration::from_millis(self.config.wait_terminal_timeout_ms);
        let route_ttl = Duration::from_secs(self.config.route_ttl_secs);
        let max_routes_per_session = self.config.max_routes_per_session as usize;
        let server_name = self.config.server_name.clone();
        let gateway_name = self.effective_gateway_name();
        let timeout_secs = self.config.challenger_timeout_secs;
        let poll_interval_secs = self.config.challenger_poll_interval_secs.max(1);
        let allow_unknown_tools = self.config.allow_unknown_tools;
        let policy = self.config.policy.clone();
        let remote_host = self.config.remote_host.clone();
        let remote_gateway_port = self.config.remote_gateway_port;
        let adapter_version = self.config.adapter_version.clone();
        let adapter_dcc = self.config.adapter_dcc.clone();
        let middleware_chain = self.config.middleware_chain.clone();
        #[cfg(feature = "mdns")]
        let discover_mdns = self.config.discover_mdns;
        let relay_sources = self.config.relay_sources.clone();
        #[cfg(feature = "admin")]
        let admin_enabled = self.config.admin_enabled;
        #[cfg(feature = "admin")]
        let admin_path = self.config.admin_path.clone();
        #[cfg(feature = "admin")]
        let admin_persist = self.config.admin_persist.clone();
        let health_check_interval_secs = self.config.health_check_interval_secs;
        let health_check_failures = self.config.health_check_failures;
        let auth = self.config.auth.clone();

        let handle = tokio::spawn(async move {
            // Publish a short-lived challenger sentinel before asking the
            // resident gateway to yield. This gives newer gateways a second
            // takeover path: the cooperative HTTP yield is the fast path,
            // while the resident gateway's existing newer-sentinel sweep is
            // the fallback if the HTTP request is delayed or dropped under
            // heavy scheduler load.
            let mut challenge_sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, &host, port);
            challenge_sentinel.version = Some(own_ver.clone());
            challenge_sentinel.adapter_version = adapter_version.clone();
            challenge_sentinel.adapter_dcc = adapter_dcc.clone();
            stamp_gateway_sentinel(&mut challenge_sentinel, &gateway_name, "challenger");
            let challenge_sentinel_key = challenge_sentinel.key();
            {
                let reg = registry.read().await;
                let _ = reg.register(challenge_sentinel);
            }
            let _challenge_guard = PromotedGatewayGuard {
                abort: None,
                registry: registry.clone(),
                sentinel_key: Some(challenge_sentinel_key),
            };

            // ── Cooperative yield request ─────────────────────────────────
            // If the old gateway also speaks our protocol it will shut down
            // gracefully; if not (e.g. v0.12.6) this is a no-op 404 — fine.
            let yield_url = format!("http://{}:{}/gateway/yield", host, port);
            let yield_timeout = Duration::from_secs(poll_interval_secs.clamp(1, 5));
            let yield_client = reqwest::Client::builder()
                .connect_timeout(yield_timeout)
                .timeout(yield_timeout)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            request_cooperative_yield(&yield_client, &yield_url, &own_ver, &gw_ver).await;

            // ── Retry loop ────────────────────────────────────────────────
            let max_retries = (timeout_secs / poll_interval_secs).max(1);
            for attempt in 1..=max_retries {
                tokio::time::sleep(Duration::from_secs(poll_interval_secs)).await;

                if let Some(listener) = try_bind_port_opt(&host, port).await {
                    tracing::info!(
                        attempt = attempt,
                        version = %own_ver,
                        "Challenger: won gateway port — starting gateway tasks"
                    );

                    // Sweep leftover ``__gateway__`` sentinels before
                    // writing ours. The old gateway's Drop should have
                    // deregistered its sentinel on clean shutdown, but
                    // unclean exits (crash, SIGKILL, Task Manager kill)
                    // leave a row whose owning PID is alive again on a
                    // subsequent process start with a recycled PID, or
                    // simply outlives the bind contest because the
                    // peer-side ``prune_dead_entries`` won't drop a row
                    // whose PID is alive. The challenger path used to
                    // ``register`` next to those rows, leaving N stale
                    // sentinels per port. RFC #998 follow-up.
                    {
                        let reg = registry.read().await;
                        let existing = reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE);
                        for entry in &existing {
                            let _ = reg.deregister(&entry.key());
                        }
                        if !existing.is_empty() {
                            tracing::info!(
                                cleared = existing.len(),
                                "Challenger: cleared stale __gateway__ sentinels before writing our own"
                            );
                        }
                    }

                    // Update sentinel with our version + adapter info so
                    // peers see the same election profile we used to win.
                    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, &host, port);
                    sentinel.version = Some(own_ver.clone());
                    sentinel.adapter_version = adapter_version.clone();
                    sentinel.adapter_dcc = adapter_dcc.clone();
                    stamp_gateway_sentinel(&mut sentinel, &gateway_name, "active");
                    let sentinel_key = sentinel.key();
                    {
                        let reg = registry.read().await;
                        let _ = reg.register(sentinel);
                    }

                    let remote_listener =
                        bind_remote_gateway_listener(remote_host.clone(), remote_gateway_port)
                            .await;

                    match start_gateway_tasks(
                        listener,
                        remote_listener,
                        registry.clone(),
                        stale_timeout,
                        backend_timeout,
                        async_dispatch_timeout,
                        wait_terminal_timeout,
                        route_ttl,
                        max_routes_per_session,
                        format!("{server_name} ({gateway_name})"),
                        own_ver.clone(),
                        sentinel_key.clone(),
                        host.clone(),
                        port,
                        allow_unknown_tools,
                        #[cfg(feature = "mdns")]
                        discover_mdns,
                        relay_sources.clone(),
                        policy.clone(),
                        adapter_version.clone(),
                        adapter_dcc.clone(),
                        middleware_chain.clone(),
                        #[cfg(feature = "admin")]
                        admin_enabled,
                        #[cfg(feature = "admin")]
                        admin_path.clone(),
                        #[cfg(feature = "admin")]
                        admin_persist.clone(),
                        health_check_interval_secs,
                        health_check_failures,
                        auth.clone(),
                    )
                    .await
                    {
                        Ok(tasks) => {
                            tracing::info!(
                                version = %own_ver,
                                "Challenger: promoted to gateway"
                            );
                            let _guard = PromotedGatewayGuard {
                                abort: Some(tasks.abort),
                                registry: registry.clone(),
                                sentinel_key: Some(sentinel_key),
                            };
                            let _ = tasks.supervisor.await;
                        }
                        Err(e) => {
                            tracing::error!("Challenger: failed to start gateway tasks: {e}");
                            let reg = registry.read().await;
                            let _ = reg.deregister(&sentinel_key);
                        }
                    }
                    return;
                }

                tracing::debug!("Challenger: port still taken (attempt {attempt}/{max_retries})");
                request_cooperative_yield(&yield_client, &yield_url, &own_ver, &gw_ver).await;
            }

            tracing::warn!(
                own = %own_ver,
                gateway = %gw_ver,
                "Challenger: gave up after {max_retries} retries — staying as plain instance"
            );
        });

        handle.abort_handle()
    }

    async fn bind_remote_gateway_listener(&self) -> Option<tokio::net::TcpListener> {
        bind_remote_gateway_listener(
            self.config.remote_host.clone(),
            self.config.remote_gateway_port,
        )
        .await
    }

    fn effective_gateway_name(&self) -> String {
        self.config
            .gateway_name
            .as_ref()
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| {
                let dcc = self.config.adapter_dcc.as_deref().unwrap_or("standalone");
                format!("{dcc}-gateway-pid{}", std::process::id())
            })
    }
}

fn stamp_gateway_sentinel(entry: &mut ServiceEntry, gateway_name: &str, role: &str) {
    entry.display_name = Some(gateway_name.to_string());
    entry
        .metadata
        .insert("dcc_mcp_role".to_string(), "gateway-sentinel".to_string());
    entry
        .metadata
        .insert("gateway_name".to_string(), gateway_name.to_string());
    entry
        .metadata
        .insert("gateway_role".to_string(), role.to_string());
    entry.metadata.insert(
        "gateway_process_pid".to_string(),
        std::process::id().to_string(),
    );
}

fn challenger_reason(resident_health: ResidentGatewayHealth) -> Option<&'static str> {
    match resident_health {
        ResidentGatewayHealth::Missing => Some(
            "Bind failed but no resident sentinel (TIME_WAIT / race) — entering challenger mode",
        ),
        ResidentGatewayHealth::Unhealthy => {
            Some("Resident gateway failed /health probe — entering challenger mode")
        }
        ResidentGatewayHealth::Healthy => None,
    }
}

async fn probe_resident_gateway_health(
    host: &str,
    port: u16,
    timeout: Duration,
) -> ResidentGatewayHealth {
    let url = format!("http://{host}:{port}/health");
    let client = reqwest::Client::builder()
        .connect_timeout(timeout)
        .timeout(timeout)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => ResidentGatewayHealth::Healthy,
        Ok(resp) => {
            tracing::warn!(
                status = %resp.status(),
                url = %url,
                "Resident gateway /health probe failed"
            );
            ResidentGatewayHealth::Unhealthy
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                url = %url,
                "Resident gateway /health probe failed"
            );
            ResidentGatewayHealth::Unhealthy
        }
    }
}

struct PromotedGatewayGuard {
    abort: Option<AbortHandle>,
    registry: Arc<RwLock<FileRegistry>>,
    sentinel_key: Option<ServiceKey>,
}

impl Drop for PromotedGatewayGuard {
    fn drop(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
        if let Some(key) = self.sentinel_key.take()
            && let Ok(registry) = self.registry.try_read()
        {
            let _ = registry.deregister(&key);
        }
    }
}

async fn bind_remote_gateway_listener(
    remote_host: Option<String>,
    remote_gateway_port: u16,
) -> Option<tokio::net::TcpListener> {
    if remote_gateway_port == 0 {
        return None;
    }
    let host = remote_host.unwrap_or_else(|| "0.0.0.0".to_string());
    match try_bind_port_opt(&host, remote_gateway_port).await {
        Some(listener) => {
            tracing::info!(
                host = %host,
                port = remote_gateway_port,
                "Gateway remote listener enabled"
            );
            Some(listener)
        }
        None => {
            tracing::warn!(
                host = %host,
                port = remote_gateway_port,
                "Gateway remote listener unavailable; continuing with local gateway only"
            );
            None
        }
    }
}

struct CooperativeYieldFallbackDetail {
    error_kind: Option<String>,
    message: String,
    optional_capability_miss: bool,
}

async fn request_cooperative_yield(
    client: &reqwest::Client,
    yield_url: &str,
    own_ver: &str,
    gw_ver: &str,
) {
    if !should_probe_cooperative_yield(own_ver, gw_ver) {
        tracing::debug!(
            own = %own_ver,
            gateway = %gw_ver,
            fallback = "polling",
            "Skipping cooperative yield probe because challenger is not newer than the current gateway"
        );
        return;
    }

    let body = serde_json::json!({ "challenger_version": own_ver }).to_string();
    match client
        .post(yield_url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!(
                gateway = %gw_ver,
                "Cooperative yield accepted — waiting for port to free up"
            );
        }
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            let detail = cooperative_yield_fallback_detail(status, &body_text);
            if detail.optional_capability_miss {
                tracing::debug!(
                    status = %status,
                    gateway = %gw_ver,
                    error_kind = detail.error_kind.as_deref().unwrap_or("unknown"),
                    fallback = "polling",
                    "Cooperative yield optional capability unavailable ({}) — polling for port",
                    detail.message
                );
            } else {
                tracing::info!(
                    status = %status,
                    gateway = %gw_ver,
                    error_kind = detail.error_kind.as_deref().unwrap_or("unknown"),
                    fallback = "polling",
                    "Cooperative yield unavailable or refused ({}) — polling for port",
                    detail.message
                );
            }
        }
        Err(err) => {
            tracing::debug!(
                gateway = %gw_ver,
                error = %err,
                fallback = "polling",
                "Cooperative yield request failed — polling for port"
            );
        }
    }
}

fn should_probe_cooperative_yield(own_ver: &str, gw_ver: &str) -> bool {
    gw_ver.trim().is_empty() || is_newer_version(own_ver, gw_ver)
}

fn cooperative_yield_fallback_detail(
    status: reqwest::StatusCode,
    body: &str,
) -> CooperativeYieldFallbackDetail {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let error_kind = parsed
        .as_ref()
        .and_then(|value| value.pointer("/error/kind"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let optional_capability_miss = matches!(
        error_kind.as_deref(),
        Some("optional-capability-unsupported")
    ) || matches!(
        status,
        reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
    );
    let message = parsed
        .as_ref()
        .and_then(|value| value.pointer("/error/message"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            parsed
                .as_ref()
                .and_then(|value| value.get("error"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| match status {
            reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED => {
                "gateway does not expose /gateway/yield; this is a non-fatal optional capability miss".to_string()
            }
            _ => {
                "gateway returned a non-success response to the optional yield probe".to_string()
            }
        });

    CooperativeYieldFallbackDetail {
        error_kind,
        message,
        optional_capability_miss,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooperative_yield_fallback_reads_structured_optional_capability() {
        let detail = cooperative_yield_fallback_detail(
            reqwest::StatusCode::CONFLICT,
            r#"{"error":{"kind":"optional-capability-unsupported","message":"poll instead"}}"#,
        );

        assert_eq!(
            detail.error_kind.as_deref(),
            Some("optional-capability-unsupported")
        );
        assert_eq!(detail.message, "poll instead");
        assert!(detail.optional_capability_miss);
    }

    #[test]
    fn cooperative_yield_fallback_legacy_404_is_non_fatal() {
        let detail = cooperative_yield_fallback_detail(reqwest::StatusCode::NOT_FOUND, "");

        assert_eq!(detail.error_kind, None);
        assert!(detail.optional_capability_miss);
        assert!(
            detail.message.contains("optional capability miss"),
            "legacy detail should mark the fallback as optional: {}",
            detail.message
        );
    }

    #[test]
    fn cooperative_yield_probe_skips_known_same_or_newer_gateway() {
        assert!(should_probe_cooperative_yield("0.17.8", ""));
        assert!(should_probe_cooperative_yield("0.17.9", "0.17.8"));
        assert!(!should_probe_cooperative_yield("0.17.8", "0.17.8"));
        assert!(!should_probe_cooperative_yield("0.17.7", "0.17.8"));
    }

    #[test]
    fn healthy_resident_suppresses_version_preemption() {
        assert_eq!(challenger_reason(ResidentGatewayHealth::Healthy), None);
    }

    #[test]
    fn unhealthy_resident_enters_challenger_mode_even_without_version_advantage() {
        assert_eq!(
            challenger_reason(ResidentGatewayHealth::Unhealthy),
            Some("Resident gateway failed /health probe — entering challenger mode")
        );
    }

    #[test]
    fn missing_resident_still_recovers_time_wait_race() {
        assert_eq!(
            challenger_reason(ResidentGatewayHealth::Missing),
            Some(
                "Bind failed but no resident sentinel (TIME_WAIT / race) — entering challenger mode"
            )
        );
    }
}
