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

impl GatewayRunner {
    /// Create a new runner, initializing (or loading) the `FileRegistry` from
    /// `config.registry_dir` (or a system temp dir if `None`).
    pub fn new(config: GatewayConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let dir = config
            .registry_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("dcc-mcp-registry"));
        let registry = FileRegistry::new(&dir)?;
        Ok(Self {
            config,
            registry: Arc::new(RwLock::new(registry)),
        })
    }

    /// Register `entry`, start heartbeat, and run the **version-aware gateway election**.
    ///
    /// ## Election algorithm
    ///
    /// 1. **Win**: binds the gateway port → becomes gateway immediately.
    ///    - Registers a `__gateway__` sentinel with its own version in FileRegistry.
    ///    - Periodically checks whether any live instance has a *newer* version;
    ///      if so, initiates voluntary yield (graceful shutdown of its listener).
    ///
    /// 2. **Lose + same-or-older version**: registers as a plain DCC instance
    ///    (current `is_gateway = false` behaviour).
    ///
    /// 3. **Lose + newer version** (e.g. `0.12.29` vs `0.12.6` gateway):
    ///    - First tries a cooperative [`POST /gateway/yield`] to the existing
    ///      gateway (works if the gateway supports it, i.e. is also `≥ 0.12.29`).
    ///    - Regardless of the response, enters a **challenger retry loop** that
    ///      polls the port every 10 s for up to `challenger_timeout_secs`.
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
        let (is_gateway, gateway_abort, challenger_abort, gateway_supervisor, gateway_thread) =
            if self.config.gateway_port > 0 {
                let outcome = self.run_election().await?;
                (
                    outcome.is_gateway,
                    outcome.gateway_abort,
                    outcome.challenger_abort,
                    outcome.gateway_supervisor,
                    outcome.gateway_thread,
                )
            } else {
                (false, None, None, None, None)
            };

        Ok(GatewayHandle {
            is_gateway,
            service_key,
            heartbeat_abort,
            gateway_abort,
            gateway_supervisor,
            gateway_thread,
            challenger_abort,
        })
    }

    /// Core version-aware election logic, extracted for clarity.
    pub(crate) async fn run_election(
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

        match try_bind_port_opt(&self.config.host, self.config.gateway_port).await {
            // ── We won the port ───────────────────────────────────────────
            Some(listener) => {
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
                let sentinel_key = sentinel.key();
                {
                    let reg = self.registry.read().await;
                    let _ = reg.register(sentinel);
                }

                match start_gateway_tasks(
                    listener,
                    self.registry.clone(),
                    stale_timeout,
                    backend_timeout,
                    async_dispatch_timeout,
                    wait_terminal_timeout,
                    route_ttl,
                    max_routes_per_session,
                    format!("{} (gateway)", self.config.server_name),
                    own_version.clone(),
                    sentinel_key,
                    self.config.host.clone(),
                    self.config.gateway_port,
                    self.config.allow_unknown_tools,
                    own_adapter_version.clone(),
                    own_adapter_dcc.clone(),
                    self.config.tool_exposure,
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
                        Ok(ElectionOutcome {
                            is_gateway: false,
                            gateway_abort: None,
                            challenger_abort: None,
                            gateway_supervisor: None,
                            gateway_thread: None,
                        })
                    }
                }
            }

            // ── Port is taken — version-aware challenger logic ────────────
            None => {
                // Read the sentinel to discover the current gateway's full
                // election profile (crate version + adapter metadata).
                // Issue maya#137: the previous lookup only fetched `version`,
                // so a freshly-released DCC adapter could never preempt an
                // older standalone server pinned to a newer crate version.
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

                let own_info = ElectionInfo::new(
                    &own_version,
                    own_adapter_version.as_deref(),
                    own_adapter_dcc.as_deref(),
                );
                let gw_info = ElectionInfo::new(
                    if gw_version.is_empty() {
                        "0.0.0"
                    } else {
                        &gw_version
                    },
                    gw_adapter_version.as_deref(),
                    gw_adapter_dcc.as_deref(),
                );

                if !gw_version.is_empty() && is_newer_election(own_info, gw_info) {
                    tracing::info!(
                        own = %own_version,
                        own_adapter_version = ?own_adapter_version,
                        own_adapter_dcc = ?own_adapter_dcc,
                        gateway = %gw_version,
                        gateway_adapter_version = ?gw_adapter_version,
                        gateway_adapter_dcc = ?gw_adapter_dcc,
                        "We outrank the current gateway — entering challenger mode"
                    );
                    let challenger_abort = self.spawn_challenger_loop(&own_version, &gw_version);
                    // Return as non-gateway for now; challenger loop will promote us later.
                    Ok(ElectionOutcome {
                        is_gateway: false,
                        gateway_abort: None,
                        challenger_abort: Some(challenger_abort),
                        gateway_supervisor: None,
                        gateway_thread: None,
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
                        "Gateway port held by same-or-stronger candidate — running as plain DCC instance"
                    );
                    Ok(ElectionOutcome {
                        is_gateway: false,
                        gateway_abort: None,
                        challenger_abort: None,
                        gateway_supervisor: None,
                        gateway_thread: None,
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
        let timeout_secs = self.config.challenger_timeout_secs;
        let allow_unknown_tools = self.config.allow_unknown_tools;
        let adapter_version = self.config.adapter_version.clone();
        let adapter_dcc = self.config.adapter_dcc.clone();
        let tool_exposure = self.config.tool_exposure;

        let handle = tokio::spawn(async move {
            // ── Cooperative yield request ─────────────────────────────────
            // If the old gateway also speaks our protocol it will shut down
            // gracefully; if not (e.g. v0.12.6) this is a no-op 404 — fine.
            let yield_url = format!("http://{}:{}/gateway/yield", host, port);
            let body = serde_json::json!({ "challenger_version": own_ver }).to_string();
            if let Ok(resp) = reqwest::Client::new()
                .post(&yield_url)
                .header("content-type", "application/json")
                .body(body)
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                if resp.status().is_success() {
                    tracing::info!(
                        gateway = %gw_ver,
                        "Cooperative yield accepted — waiting for port to free up"
                    );
                } else {
                    tracing::info!(
                        status = %resp.status(),
                        "Cooperative yield not supported by gateway v{gw_ver} \
                         (normal for older versions) — polling for port"
                    );
                }
            }

            // ── Retry loop ────────────────────────────────────────────────
            let max_retries = (timeout_secs / 10).max(1);
            for attempt in 1..=max_retries {
                tokio::time::sleep(Duration::from_secs(10)).await;

                if let Some(listener) = try_bind_port_opt(&host, port).await {
                    tracing::info!(
                        attempt = attempt,
                        version = %own_ver,
                        "Challenger: won gateway port — starting gateway tasks"
                    );

                    // Update sentinel with our version + adapter info so
                    // peers see the same election profile we used to win.
                    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, &host, port);
                    sentinel.version = Some(own_ver.clone());
                    sentinel.adapter_version = adapter_version.clone();
                    sentinel.adapter_dcc = adapter_dcc.clone();
                    let sentinel_key = sentinel.key();
                    {
                        let reg = registry.read().await;
                        let _ = reg.register(sentinel);
                    }

                    if let Err(e) = start_gateway_tasks(
                        listener,
                        registry.clone(),
                        stale_timeout,
                        backend_timeout,
                        async_dispatch_timeout,
                        wait_terminal_timeout,
                        route_ttl,
                        max_routes_per_session,
                        format!("{server_name} (gateway)"),
                        own_ver.clone(),
                        sentinel_key,
                        host.clone(),
                        port,
                        allow_unknown_tools,
                        adapter_version.clone(),
                        adapter_dcc.clone(),
                        tool_exposure,
                    )
                    .await
                    {
                        tracing::error!("Challenger: failed to start gateway tasks: {e}");
                    }
                    return;
                }

                tracing::debug!("Challenger: port still taken (attempt {attempt}/{max_retries})");
            }

            tracing::warn!(
                own = %own_ver,
                gateway = %gw_ver,
                "Challenger: gave up after {max_retries} retries — staying as plain instance"
            );
        });

        handle.abort_handle()
    }
}
