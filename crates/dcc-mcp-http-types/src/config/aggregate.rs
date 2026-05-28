//! McpHttpConfig aggregate and compatibility accessors/builders (issue #852).

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use dcc_mcp_gateway_core::policy::GatewayPolicy;

use super::{
    FeatureFlags, GatewayConfig, InstanceConfig, JobConfig, JobRecoveryPolicy, QueueConfig,
    RelaySourceConfig, ServerConfig, ServerSpawnMode, SessionConfig, TelemetryConfig,
    WorkflowConfig,
};

// ── McpHttpConfig aggregate ────────────────────────────────────────────────

/// Configuration for the MCP HTTP server.
///
/// This is a **thin aggregate** of 9 cohesive sub-config structs,
/// each owning one orthogonal concern:
///
/// | Field | Struct | Concern |
/// |-------|--------|----------|
/// | `server` | [`ServerConfig`] | Core server identity & transport |
/// | `instance` | [`InstanceConfig`] | DCC registration metadata |
/// | `session` | [`SessionConfig`] | Session lifecycle & tool-cache |
/// | `gateway` | [`GatewayConfig`] | Gateway election, routing, discovery |
/// | `queue` | [`QueueConfig`] | Queue depth & backpressure |
/// | `telemetry` | [`TelemetryConfig`] | Prometheus metrics |
/// | `features` | [`FeatureFlags`] | Opt-in capability switches |
/// | `workflow` | [`WorkflowConfig`] | Workflow & scheduler |
/// | `job` | [`JobConfig`] | Job persistence & recovery |
///
/// Use [`McpHttpConfig::default()`] for sensible defaults, or the
/// builder-pattern methods (`.with_*()`) for customization.
///
/// `#[deprecated]` `::new(port)` is kept for one minor release to avoid
/// breaking existing callers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpHttpConfig {
    /// Core server identity & transport.
    pub server: ServerConfig,

    /// DCC instance registration metadata.
    pub instance: InstanceConfig,

    /// Session lifecycle & tool-cache.
    pub session: SessionConfig,

    /// Gateway election, routing, and discovery.
    pub gateway: GatewayConfig,

    /// Queue depth & backpressure.
    pub queue: QueueConfig,

    /// Prometheus metrics.
    pub telemetry: TelemetryConfig,

    /// Opt-in capability switches.
    pub features: FeatureFlags,

    /// Workflow & scheduler.
    pub workflow: WorkflowConfig,

    /// Job persistence & recovery.
    pub job: JobConfig,
}

// ── McpHttpConfig accessors ────────────────────────────────────────────────

#[allow(missing_docs, clippy::must_use_candidate)]
impl McpHttpConfig {
    pub fn port(&self) -> u16 {
        self.server.port
    }
    pub fn set_port(&mut self, v: u16) {
        self.server.port = v;
    }
    pub fn host(&self) -> String {
        self.server.host.to_string()
    }
    pub fn set_host(&mut self, v: &str) -> Result<&mut Self, std::net::AddrParseError> {
        self.server.host = v.parse()?;
        Ok(self)
    }
    pub fn endpoint_path(&self) -> String {
        self.server.endpoint_path.clone()
    }
    pub fn set_endpoint_path(&mut self, v: String) {
        self.server.endpoint_path = v;
    }
    pub fn server_name(&self) -> String {
        self.server.server_name.clone()
    }
    pub fn set_server_name(&mut self, v: String) {
        self.server.server_name = v;
    }
    pub fn server_version(&self) -> String {
        self.server.server_version.clone()
    }
    pub fn set_server_version(&mut self, v: String) {
        self.server.server_version = v;
    }
    pub fn max_sessions(&self) -> usize {
        self.server.max_sessions
    }
    pub fn set_max_sessions(&mut self, v: usize) {
        self.server.max_sessions = v;
    }
    pub fn request_timeout_ms(&self) -> u64 {
        self.server.request_timeout_ms
    }
    pub fn set_request_timeout_ms(&mut self, v: u64) {
        self.server.request_timeout_ms = v;
    }
    pub fn enable_cors(&self) -> bool {
        self.server.enable_cors
    }
    pub fn set_enable_cors(&mut self, v: bool) {
        self.server.enable_cors = v;
    }
    pub fn self_probe_timeout_ms(&self) -> u64 {
        self.server.self_probe_timeout_ms
    }
    pub fn set_self_probe_timeout_ms(&mut self, v: u64) {
        self.server.self_probe_timeout_ms = v;
    }
    pub fn spawn_mode(&self) -> ServerSpawnMode {
        self.server.spawn_mode
    }
    pub fn set_spawn_mode(&mut self, v: ServerSpawnMode) {
        self.server.spawn_mode = v;
    }
    pub fn session_ttl_secs(&self) -> u64 {
        self.session.session_ttl_secs
    }
    pub fn set_session_ttl_secs(&mut self, v: u64) {
        self.session.session_ttl_secs = v;
    }
    pub fn enable_tool_cache(&self) -> bool {
        self.session.enable_tool_cache
    }
    pub fn set_enable_tool_cache(&mut self, v: bool) {
        self.session.enable_tool_cache = v;
    }
    pub fn gateway_port(&self) -> u16 {
        self.gateway.gateway_port
    }
    pub fn set_gateway_port(&mut self, v: u16) {
        self.gateway.gateway_port = v;
    }
    pub fn remote_host(&self) -> Option<String> {
        self.gateway.remote_host.clone()
    }
    pub fn set_remote_host(&mut self, v: Option<String>) {
        self.gateway.remote_host = v;
    }
    pub fn remote_gateway_port(&self) -> u16 {
        self.gateway.remote_gateway_port
    }
    pub fn set_remote_gateway_port(&mut self, v: u16) {
        self.gateway.remote_gateway_port = v;
    }
    pub fn admin_enabled(&self) -> bool {
        self.gateway.admin_enabled
    }
    pub fn set_admin_enabled(&mut self, v: bool) {
        self.gateway.admin_enabled = v;
    }
    pub fn admin_path(&self) -> String {
        self.gateway.admin_path.clone()
    }
    pub fn set_admin_path(&mut self, v: String) {
        self.gateway.admin_path = v;
    }
    pub fn stale_timeout_secs(&self) -> u64 {
        self.gateway.stale_timeout_secs
    }
    pub fn set_stale_timeout_secs(&mut self, v: u64) {
        self.gateway.stale_timeout_secs = v;
    }
    pub fn heartbeat_secs(&self) -> u64 {
        self.gateway.heartbeat_secs
    }
    pub fn set_heartbeat_secs(&mut self, v: u64) {
        self.gateway.heartbeat_secs = v;
    }
    pub fn backend_timeout_ms(&self) -> u64 {
        self.gateway.backend_timeout_ms
    }
    pub fn set_backend_timeout_ms(&mut self, v: u64) {
        self.gateway.backend_timeout_ms = v;
    }
    pub fn gateway_async_dispatch_timeout_ms(&self) -> u64 {
        self.gateway.gateway_async_dispatch_timeout_ms
    }
    pub fn set_gateway_async_dispatch_timeout_ms(&mut self, v: u64) {
        self.gateway.gateway_async_dispatch_timeout_ms = v;
    }
    pub fn gateway_wait_terminal_timeout_ms(&self) -> u64 {
        self.gateway.gateway_wait_terminal_timeout_ms
    }
    pub fn set_gateway_wait_terminal_timeout_ms(&mut self, v: u64) {
        self.gateway.gateway_wait_terminal_timeout_ms = v;
    }
    pub fn gateway_route_ttl_secs(&self) -> u64 {
        self.gateway.gateway_route_ttl_secs
    }
    pub fn set_gateway_route_ttl_secs(&mut self, v: u64) {
        self.gateway.gateway_route_ttl_secs = v;
    }
    pub fn gateway_max_routes_per_session(&self) -> u64 {
        self.gateway.gateway_max_routes_per_session
    }
    pub fn set_gateway_max_routes_per_session(&mut self, v: u64) {
        self.gateway.gateway_max_routes_per_session = v;
    }
    pub fn adapter_version(&self) -> Option<String> {
        self.gateway.adapter_version.clone()
    }
    pub fn set_adapter_version(&mut self, v: Option<String>) {
        self.gateway.adapter_version = v;
    }
    pub fn adapter_dcc(&self) -> Option<String> {
        self.gateway.adapter_dcc.clone()
    }
    pub fn set_adapter_dcc(&mut self, v: Option<String>) {
        self.gateway.adapter_dcc = v;
    }
    pub fn gateway_name(&self) -> Option<String> {
        self.gateway.gateway_name.clone()
    }
    pub fn set_gateway_name(&mut self, v: Option<String>) {
        self.gateway.gateway_name = v;
    }
    pub fn allow_unknown_tools(&self) -> bool {
        self.gateway.allow_unknown_tools
    }
    pub fn set_allow_unknown_tools(&mut self, v: bool) {
        self.gateway.allow_unknown_tools = v;
    }
    pub fn discover_mdns(&self) -> bool {
        self.gateway.discover_mdns
    }
    pub fn set_discover_mdns(&mut self, v: bool) {
        self.gateway.discover_mdns = v;
    }
    pub fn relay_sources(&self) -> Vec<RelaySourceConfig> {
        self.gateway.relay_sources.clone()
    }
    pub fn set_relay_sources(&mut self, v: Vec<RelaySourceConfig>) {
        self.gateway.relay_sources = v;
    }
    pub fn gateway_policy(&self) -> GatewayPolicy {
        self.gateway.policy.clone()
    }
    pub fn set_gateway_policy(&mut self, policy: GatewayPolicy) {
        self.gateway.policy = policy;
    }
    pub fn deferred_queue_depth(&self) -> usize {
        self.queue.deferred_queue_depth
    }
    pub fn set_deferred_queue_depth(&mut self, v: usize) {
        self.queue.deferred_queue_depth = v;
    }
    pub fn bridge_queue_depth(&self) -> usize {
        self.queue.bridge_queue_depth
    }
    pub fn set_bridge_queue_depth(&mut self, v: usize) {
        self.queue.bridge_queue_depth = v;
    }
    pub fn host_queue_depth(&self) -> usize {
        self.queue.host_queue_depth
    }
    pub fn set_host_queue_depth(&mut self, v: usize) {
        self.queue.host_queue_depth = v;
    }
    pub fn queue_send_timeout_ms(&self) -> u64 {
        self.queue.queue_send_timeout_ms
    }
    pub fn set_queue_send_timeout_ms(&mut self, v: u64) {
        self.queue.queue_send_timeout_ms = v;
    }
    pub fn enable_prometheus(&self) -> bool {
        self.telemetry.enable_prometheus
    }
    pub fn set_enable_prometheus(&mut self, v: bool) {
        self.telemetry.enable_prometheus = v;
    }
    pub fn prometheus_basic_auth(&self) -> Option<(String, String)> {
        self.telemetry.prometheus_basic_auth.clone()
    }
    pub fn set_prometheus_basic_auth(&mut self, v: Option<(String, String)>) {
        self.telemetry.prometheus_basic_auth = v;
    }
    pub fn lazy_actions(&self) -> bool {
        self.features.lazy_actions
    }
    pub fn set_lazy_actions(&mut self, v: bool) {
        self.features.lazy_actions = v;
    }
    pub fn enable_workflows(&self) -> bool {
        self.workflow.enable_workflows
    }
    pub fn set_enable_workflows(&mut self, v: bool) {
        self.workflow.enable_workflows = v;
    }
    pub fn shutdown_on_drop(&self) -> bool {
        self.features.shutdown_on_drop
    }
    pub fn set_shutdown_on_drop(&mut self, v: bool) {
        self.features.shutdown_on_drop = v;
    }
    pub fn enable_job_notifications(&self) -> bool {
        self.features.enable_job_notifications
    }
    pub fn set_enable_job_notifications(&mut self, v: bool) {
        self.features.enable_job_notifications = v;
    }
    pub fn enable_resources(&self) -> bool {
        self.features.enable_resources
    }
    pub fn set_enable_resources(&mut self, v: bool) {
        self.features.enable_resources = v;
    }
    pub fn enable_prompts(&self) -> bool {
        self.features.enable_prompts
    }
    pub fn set_enable_prompts(&mut self, v: bool) {
        self.features.enable_prompts = v;
    }
    pub fn exclude_skill_stubs_from_tools_list(&self) -> bool {
        self.features.exclude_skill_stubs_from_tools_list
    }
    pub fn set_exclude_skill_stubs_from_tools_list(&mut self, v: bool) {
        self.features.exclude_skill_stubs_from_tools_list = v;
    }
    pub fn exclude_group_stubs_from_tools_list(&self) -> bool {
        self.features.exclude_group_stubs_from_tools_list
    }
    pub fn set_exclude_group_stubs_from_tools_list(&mut self, v: bool) {
        self.features.exclude_group_stubs_from_tools_list = v;
    }
    pub fn standalone_main_thread_execution(&self) -> bool {
        self.features.standalone_main_thread_execution
    }
    pub fn set_standalone_main_thread_execution(&mut self, v: bool) {
        self.features.standalone_main_thread_execution = v;
    }
    /// Set both skill and group stub exclusion flags together (issue #174 / #238).
    pub fn set_exclude_progressive_stubs_from_tools_list(&mut self, v: bool) {
        self.features.exclude_skill_stubs_from_tools_list = v;
        self.features.exclude_group_stubs_from_tools_list = v;
    }
    pub fn enable_artefact_resources(&self) -> bool {
        self.features.enable_artefact_resources
    }
    pub fn set_enable_artefact_resources(&mut self, v: bool) {
        self.features.enable_artefact_resources = v;
    }
    pub fn dcc_type(&self) -> Option<String> {
        self.instance.dcc_type.clone()
    }
    pub fn set_dcc_type(&mut self, v: Option<String>) {
        self.instance.dcc_type = v;
    }
    pub fn dcc_version(&self) -> Option<String> {
        self.instance.dcc_version.clone()
    }
    pub fn set_dcc_version(&mut self, v: Option<String>) {
        self.instance.dcc_version = v;
    }
    pub fn scene(&self) -> Option<String> {
        self.instance.scene.clone()
    }
    pub fn set_scene(&mut self, v: Option<String>) {
        self.instance.scene = v;
    }
    pub fn instance_metadata(&self) -> HashMap<String, String> {
        self.instance.instance_metadata.clone()
    }
    pub fn set_instance_metadata(&mut self, v: HashMap<String, String>) {
        self.instance.instance_metadata = v;
    }
    pub fn declared_capabilities(&self) -> Vec<String> {
        self.instance.declared_capabilities.clone()
    }
    pub fn set_declared_capabilities(&mut self, v: Vec<String>) {
        self.instance.declared_capabilities = v;
    }
    pub fn job_storage_path(&self) -> Option<PathBuf> {
        self.job.job_storage_path.clone()
    }
    pub fn set_job_storage_path(&mut self, v: Option<PathBuf>) {
        self.job.job_storage_path = v;
    }
    pub fn job_recovery(&self) -> JobRecoveryPolicy {
        self.job.job_recovery
    }
    pub fn set_job_recovery(&mut self, v: JobRecoveryPolicy) {
        self.job.job_recovery = v;
    }
    pub fn enable_scheduler(&self) -> bool {
        self.workflow.enable_scheduler
    }
    pub fn set_enable_scheduler(&mut self, v: bool) {
        self.workflow.enable_scheduler = v;
    }
    pub fn schedules_dir(&self) -> Option<PathBuf> {
        self.workflow.schedules_dir.clone()
    }
    pub fn set_schedules_dir(&mut self, v: Option<PathBuf>) {
        self.workflow.schedules_dir = v;
    }
}

// ── McpHttpConfig builders ─────────────────────────────────────────────────

#[allow(missing_docs, clippy::must_use_candidate)]
impl McpHttpConfig {
    /// Create a config with the given port and sensible defaults.
    ///
    /// # Deprecated
    ///
    /// Use [`McpHttpConfig::default()`] or the builder pattern instead.
    /// This method will be removed in a future minor release.
    #[deprecated(note = "use McpHttpConfig::default() or the builder pattern")]
    pub fn new(port: u16) -> Self {
        let mut cfg = Self::default();
        cfg.server.port = port;
        cfg
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.server.port = port;
        self
    }

    /// Builder: stamp the adapter package version onto the gateway
    /// sentinel for version-aware election (issue maya#137).
    pub fn with_adapter_version(mut self, version: impl Into<String>) -> Self {
        self.gateway.adapter_version = Some(version.into());
        self
    }

    /// Builder: declare the DCC type this adapter is bound to so the
    /// gateway election can prefer real DCCs over generic standalone
    /// servers (issue maya#137).
    pub fn with_adapter_dcc(mut self, dcc: impl Into<String>) -> Self {
        self.gateway.adapter_dcc = Some(dcc.into());
        self
    }

    /// Builder: give this gateway candidate a human-readable identity for
    /// admin diagnostics and the `__gateway__` sentinel row.
    pub fn with_gateway_name(mut self, name: impl Into<String>) -> Self {
        self.gateway.gateway_name = Some(name.into());
        self
    }

    /// Builder: apply a gateway capability policy to MCP and REST surfaces.
    pub fn with_gateway_policy(mut self, policy: GatewayPolicy) -> Self {
        self.gateway.policy = policy;
        self
    }

    /// Builder: enable or disable gateway read-only mode.
    pub fn with_gateway_read_only(mut self, read_only: bool) -> Self {
        self.gateway.policy.read_only = read_only;
        self
    }

    /// Builder: attach context/provenance metadata to the FileRegistry row.
    pub fn with_instance_metadata<I, K, V>(mut self, metadata: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.instance.instance_metadata = metadata
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        self
    }

    /// Builder: enable the scheduler subsystem and point at a directory
    /// of `*.schedules.yaml` files (issue #352).
    pub fn with_scheduler(mut self, dir: impl Into<PathBuf>) -> Self {
        self.workflow.enable_scheduler = true;
        self.workflow.schedules_dir = Some(dir.into());
        self
    }

    /// Builder: persist tracked jobs in a SQLite database at `path`
    /// (issue #328).
    pub fn with_job_storage_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.job.job_storage_path = Some(path.into());
        self
    }

    /// Builder: choose how the next server start reacts to in-flight rows
    /// persisted by a previous run (issue #567).
    pub fn with_job_recovery(mut self, policy: JobRecoveryPolicy) -> Self {
        self.job.job_recovery = policy;
        self
    }

    /// Builder: declare the DCC capabilities this host provides (issue #354).
    pub fn with_declared_capabilities<I, S>(mut self, caps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.instance.declared_capabilities = caps.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: enable the built-in `workflows_*` tools (issue #348).
    pub fn with_workflows(mut self) -> Self {
        self.workflow.enable_workflows = true;
        self
    }

    /// Builder: enable the lazy-actions fast-path (#254).
    pub fn with_lazy_actions(mut self) -> Self {
        self.features.lazy_actions = true;
        self
    }

    /// Builder: force the `<skill>__<action>` form even when bare names
    /// would be unique (#307).
    pub fn without_bare_tool_names(mut self) -> Self {
        self.features.bare_tool_names = false;
        self
    }

    /// Builder: allow main-affinity tools without a GUI dispatcher in a
    /// standalone interpreter where the adapter has verified the current
    /// in-process execution lane is DCC-safe.
    pub fn with_standalone_main_thread_execution(mut self) -> Self {
        self.features.standalone_main_thread_execution = true;
        self
    }

    /// Returns the full socket address string, e.g. `127.0.0.1:8765`.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }

    /// Builder: set server name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.server.server_name = name.into();
        self
    }

    /// Builder: set server version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.server.server_version = version.into();
        self
    }

    /// Builder: allow all interfaces (0.0.0.0). Use with caution.
    pub fn with_all_interfaces(mut self) -> Self {
        self.server.host = IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);
        self
    }

    /// Builder: enable CORS (for browser clients).
    pub fn with_cors(mut self) -> Self {
        self.server.enable_cors = true;
        self
    }

    /// Builder: set request timeout.
    pub fn with_timeout_ms(mut self, ms: u64) -> Self {
        self.server.request_timeout_ms = ms;
        self
    }

    /// Builder: set the idle session TTL. 0 disables background eviction.
    pub fn with_session_ttl_secs(mut self, secs: u64) -> Self {
        self.session.session_ttl_secs = secs;
        self
    }

    /// Builder: enable gateway competition on the given port.
    pub fn with_gateway(mut self, port: u16) -> Self {
        self.gateway.gateway_port = port;
        self
    }

    /// Builder: set the shared FileRegistry directory.
    pub fn with_registry_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.gateway.registry_dir = Some(dir.into());
        self
    }

    /// Builder: set the DCC application type (e.g. `"maya"`).
    pub fn with_dcc_type(mut self, dcc_type: impl Into<String>) -> Self {
        self.instance.dcc_type = Some(dcc_type.into());
        self
    }

    /// Builder: select the listener spawn strategy (issue #303).
    pub fn with_spawn_mode(mut self, mode: ServerSpawnMode) -> Self {
        self.server.spawn_mode = mode;
        self
    }

    /// Builder: override the per-backend gateway fan-out timeout (issue #314).
    pub fn with_backend_timeout_ms(mut self, ms: u64) -> Self {
        self.gateway.backend_timeout_ms = ms;
        self
    }

    /// Builder: override the gateway's async-dispatch timeout (issue #321).
    pub fn with_gateway_async_dispatch_timeout_ms(mut self, ms: u64) -> Self {
        self.gateway.gateway_async_dispatch_timeout_ms = ms;
        self
    }

    /// Builder: override the gateway wait-for-terminal timeout (issue #321).
    pub fn with_gateway_wait_terminal_timeout_ms(mut self, ms: u64) -> Self {
        self.gateway.gateway_wait_terminal_timeout_ms = ms;
        self
    }

    /// Builder: override the gateway's routing-cache TTL (issue #322).
    pub fn with_gateway_route_ttl_secs(mut self, secs: u64) -> Self {
        self.gateway.gateway_route_ttl_secs = secs;
        self
    }

    /// Builder: override the gateway's per-session route cap (issue #322).
    pub fn with_gateway_max_routes_per_session(mut self, cap: u64) -> Self {
        self.gateway.gateway_max_routes_per_session = cap;
        self
    }

    /// Builder: enable or disable LAN-local mDNS/DNS-SD discovery.
    pub fn with_mdns_discovery(mut self, enabled: bool) -> Self {
        self.gateway.discover_mdns = enabled;
        self
    }

    /// Builder: configure tunnel relay sources for gateway discovery.
    pub fn with_relay_sources(mut self, sources: Vec<RelaySourceConfig>) -> Self {
        self.gateway.relay_sources = sources;
        self
    }

    /// Builder: disable the connection-scoped tool-list cache (issue #438).
    pub fn without_tool_cache(mut self) -> Self {
        self.session.enable_tool_cache = false;
        self
    }

    /// Builder: set the deferred-executor queue capacity (issue #715).
    pub fn with_deferred_queue_depth(mut self, depth: usize) -> Self {
        self.queue.deferred_queue_depth = depth;
        self
    }

    /// Builder: set the host-bridge queue capacity (issue #715).
    pub fn with_bridge_queue_depth(mut self, depth: usize) -> Self {
        self.queue.bridge_queue_depth = depth;
        self
    }

    /// Builder: set the host-side `QueueDispatcher` capacity (issue #715).
    pub fn with_host_queue_depth(mut self, depth: usize) -> Self {
        self.queue.host_queue_depth = depth;
        self
    }

    /// Builder: set the send-timeout applied to the bounded channels
    /// (issue #715).
    pub fn with_queue_send_timeout_ms(mut self, ms: u64) -> Self {
        self.queue.queue_send_timeout_ms = ms;
        self
    }

    /// Load the queue-stack knobs from the environment (issue #715).
    ///
    /// Delegates to [`QueueConfig::apply_env_overrides`].
    pub fn apply_queue_env_overrides(mut self) -> Self {
        self.queue = self.queue.apply_env_overrides();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── McpHttpConfig aggregate ────────────────────────────────────────

    #[test]
    fn mcp_http_config_default_composes_all_sub_configs() {
        let cfg = McpHttpConfig::default();
        assert_eq!(cfg.server.port, 8765);
        assert_eq!(cfg.session.session_ttl_secs, 3_600);
        assert_eq!(cfg.gateway.gateway_port, 0);
        assert_eq!(cfg.queue.deferred_queue_depth, 16);
        assert_eq!(cfg.job.job_recovery, JobRecoveryPolicy::Drop);
    }

    #[test]
    fn mcp_http_config_round_trips_json() {
        let cfg = McpHttpConfig::default()
            .with_port(9876)
            .with_name("maya-mcp")
            .with_gateway(9765)
            .with_dcc_type("maya")
            .with_job_recovery(JobRecoveryPolicy::Requeue);
        let s = serde_json::to_string(&cfg).unwrap();
        let back: McpHttpConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.server.port, 9876);
        assert_eq!(back.server.server_name, "maya-mcp");
        assert_eq!(back.gateway.gateway_port, 9765);
        assert_eq!(back.instance.dcc_type.as_deref(), Some("maya"));
        assert_eq!(back.job.job_recovery, JobRecoveryPolicy::Requeue);
    }

    #[test]
    fn mcp_http_config_job_recovery_builder_round_trips() {
        let cfg = McpHttpConfig::default().with_job_recovery(JobRecoveryPolicy::Requeue);
        assert_eq!(cfg.job.job_recovery, JobRecoveryPolicy::Requeue);
        assert_eq!(cfg.job.job_recovery.as_str(), "requeue");
    }

    #[test]
    fn mcp_http_config_mdns_discovery_builder_round_trips() {
        let cfg = McpHttpConfig::default().with_mdns_discovery(true);
        let s = serde_json::to_string(&cfg).unwrap();
        let back: McpHttpConfig = serde_json::from_str(&s).unwrap();
        assert!(back.gateway.discover_mdns);
    }

    #[test]
    fn mcp_http_config_relay_sources_builder_round_trips() {
        let cfg = McpHttpConfig::default().with_relay_sources(vec![RelaySourceConfig {
            admin_url: "http://127.0.0.1:9872".into(),
            public_base_url: "http://127.0.0.1:9873".into(),
            poll_interval_secs: Some(2),
        }]);
        let s = serde_json::to_string(&cfg).unwrap();
        let back: McpHttpConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.gateway.relay_sources.len(), 1);
        assert_eq!(back.gateway.relay_sources[0].poll_interval_secs, Some(2));
    }

    #[test]
    fn mcp_http_config_queue_caps_default_to_pre_715_values() {
        let cfg = McpHttpConfig::default();
        assert_eq!(cfg.queue.deferred_queue_depth, 16);
        assert_eq!(cfg.queue.bridge_queue_depth, 16);
        assert_eq!(cfg.queue.host_queue_depth, 0);
        assert_eq!(cfg.queue.queue_send_timeout_ms, 2_000);
    }

    #[test]
    fn mcp_http_config_payload_limits_default_values() {
        let cfg = McpHttpConfig::default();
        assert_eq!(cfg.queue.max_request_body_bytes, 4 * 1024 * 1024);
        assert_eq!(cfg.queue.max_response_content_bytes, 1024 * 1024);
        assert_eq!(cfg.queue.sse_chunk_size_bytes, 64 * 1024);
    }

    #[test]
    fn mcp_http_config_set_host_returns_err_on_invalid_input() {
        let mut cfg = McpHttpConfig::default();
        let original = cfg.server.host;
        let err = cfg.set_host("not.an.ip.address").unwrap_err();
        assert!(!err.to_string().is_empty());
        assert_eq!(cfg.server.host, original);
    }

    #[test]
    fn mcp_http_config_set_host_accepts_valid_inputs() {
        let mut cfg = McpHttpConfig::default();
        cfg.set_host("10.0.0.5").expect("valid IPv4");
        assert_eq!(cfg.server.host.to_string(), "10.0.0.5");
        cfg.set_host("::1").expect("valid IPv6");
        assert_eq!(cfg.server.host.to_string(), "::1");
    }
}
