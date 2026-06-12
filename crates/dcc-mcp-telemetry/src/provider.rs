//! Global telemetry provider — initialise once, use everywhere.
//!
//! Call [`TelemetryProvider::init`] early in your application (e.g. `main`).
//! Afterwards use [`tracer`] / [`meter`] helper functions anywhere in the crate.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use opentelemetry::metrics::Meter;
use opentelemetry::trace::{Tracer, TracerProvider as _};
use opentelemetry::{KeyValue, global};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::{SdkTracerProvider, TracerProviderBuilder};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::error::TelemetryError;
use crate::types::{ExporterBackend, LogFormat, TelemetryConfig};

#[cfg(feature = "otlp-exporter")]
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};

#[cfg(feature = "otlp-exporter")]
use opentelemetry_otlp::tonic_types::metadata::MetadataMap;

#[cfg(feature = "otlp-exporter")]
use tonic::metadata::{Ascii, MetadataKey, MetadataValue};

// ── Global handle ─────────────────────────────────────────────────────────────

/// Holds live provider handles so we can shut them down cleanly.
pub struct TelemetryHandle {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
}

impl TelemetryHandle {
    /// Flush all pending spans/metrics and shut down the exporters gracefully.
    pub fn shutdown(&self) {
        if let Some(ref tp) = self.tracer_provider
            && let Err(e) = tp.shutdown()
        {
            tracing::warn!("tracer provider shutdown error: {e}");
        }
        if let Some(ref mp) = self.meter_provider
            && let Err(e) = mp.shutdown()
        {
            tracing::warn!("meter provider shutdown error: {e}");
        }
    }
}

static HANDLE: OnceLock<TelemetryHandle> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
static DIRECT_SPAN_FALLBACK: AtomicBool = AtomicBool::new(false);

// ── Init ──────────────────────────────────────────────────────────────────────

/// Initialise the global telemetry provider from a [`TelemetryConfig`].
///
/// May be called at most once per process. Returns `Err(AlreadyInitialized)`
/// if called a second time.
pub fn init(cfg: &TelemetryConfig) -> Result<(), TelemetryError> {
    if HANDLE.get().is_some() {
        return Err(TelemetryError::AlreadyInitialized);
    }

    // Build OpenTelemetry Resource.
    // OTEL_SERVICE_NAME and OTEL_RESOURCE_ATTRIBUTES env vars take priority
    // over the values in TelemetryConfig (standard OTel precedence rules).
    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.service_name.clone());

    let mut kv = vec![
        KeyValue::new("service.name", service_name),
        KeyValue::new("service.version", cfg.service_version.clone()),
    ];

    // Parse OTEL_RESOURCE_ATTRIBUTES: key=value,key=value,...
    if let Ok(raw) = std::env::var("OTEL_RESOURCE_ATTRIBUTES") {
        for pair in raw.split(',') {
            if let Some((k, v)) = pair.split_once('=') {
                let k = k.trim().to_string();
                let v = v.trim().to_string();
                if !k.is_empty() {
                    kv.push(KeyValue::new(k, v));
                }
            }
        }
    }

    for (k, v) in &cfg.extra_attributes {
        kv.push(KeyValue::new(k.clone(), v.clone()));
    }
    let resource = Resource::builder_empty().with_attributes(kv).build();

    // Build tracer provider
    let tracer_provider = if cfg.enable_tracing {
        Some(build_tracer_provider(cfg, &cfg.exporter, resource.clone())?)
    } else {
        None
    };

    // Build meter provider
    let meter_provider = if cfg.enable_metrics {
        Some(build_meter_provider(cfg, &cfg.exporter, resource)?)
    } else {
        None
    };

    if let Some(ref tp) = tracer_provider {
        global::set_tracer_provider(tp.clone());
    }

    // Install tracing subscriber. Some hosts initialise the shared logging
    // subscriber before telemetry so file logging can be attached later. In
    // that case direct OpenTelemetry spans still export through the global
    // provider above, even though a tracing-opentelemetry layer cannot be
    // installed retroactively.
    let mut direct_span_fallback = false;
    if let Err(err) = install_subscriber(cfg, tracer_provider.as_ref()) {
        if is_global_subscriber_already_set(&err) {
            direct_span_fallback = tracer_provider.is_some();
        } else {
            return Err(err);
        }
    }

    let handle = TelemetryHandle {
        tracer_provider,
        meter_provider: meter_provider.clone(),
    };

    // Register meter provider globally before storing handle
    if let Some(ref mp) = handle.meter_provider {
        global::set_meter_provider(mp.clone());
    }

    HANDLE
        .set(handle)
        .map_err(|_| TelemetryError::AlreadyInitialized)?;
    ACTIVE.store(true, Ordering::Release);
    DIRECT_SPAN_FALLBACK.store(direct_span_fallback, Ordering::Release);

    Ok(())
}

/// Shut down the global telemetry provider, flushing all pending data.
pub fn shutdown() {
    if let Some(h) = HANDLE.get() {
        h.shutdown();
    }
    ACTIVE.store(false, Ordering::Release);
    DIRECT_SPAN_FALLBACK.store(false, Ordering::Release);
}

/// Returns `true` if the global telemetry provider has been initialised.
pub fn is_initialized() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Returns true when telemetry was initialised after another tracing subscriber
/// had already been installed.
///
/// In that mode a `tracing-opentelemetry` layer cannot be added retroactively,
/// so callers that own high-value semantic spans can emit them directly through
/// the global OpenTelemetry tracer provider.
pub fn direct_span_fallback_enabled() -> bool {
    ACTIVE.load(Ordering::Acquire) && DIRECT_SPAN_FALLBACK.load(Ordering::Acquire)
}

/// Initialise a minimal no-op telemetry provider if one has not been set yet.
///
/// This silences the `NoopMeterProvider` / `NoopTracerProvider` warnings that
/// OpenTelemetry emits when `global::meter()` or `global::tracer()` is called
/// before any provider has been registered (issue #467).
///
/// - If the global provider is already set (by a prior `init()` call or by the
///   application configuring OTLP / Stdout exporters), this function does
///   nothing and returns `Ok(())`.
/// - Otherwise it installs a minimal `SdkMeterProvider` with no exporters so
///   metrics are silently discarded but the warning is suppressed.
///
/// Call this early in your server startup if you don't need full telemetry:
///
/// ```no_run
/// dcc_mcp_telemetry::provider::try_init_default().ok();
/// ```
pub fn try_init_default() -> Result<(), TelemetryError> {
    if HANDLE.get().is_some() {
        return Ok(());
    }
    let cfg = TelemetryConfig {
        enable_metrics: true,
        enable_tracing: false,
        exporter: crate::types::ExporterBackend::Noop,
        ..TelemetryConfig::default()
    };
    init(&cfg)
}

// ── Named tracer / meter accessors ────────────────────────────────────────────

/// Get a named [`Tracer`] from the global provider.
pub fn tracer(name: &'static str) -> impl Tracer {
    global::tracer(name)
}

/// Get a named [`Meter`] from the global provider.
pub fn meter(name: &'static str) -> Meter {
    global::meter(name)
}

// ── Internal builders ─────────────────────────────────────────────────────────

fn build_tracer_provider(
    _cfg: &TelemetryConfig,
    backend: &ExporterBackend,
    resource: Resource,
) -> Result<SdkTracerProvider, TelemetryError> {
    let builder: TracerProviderBuilder = match backend {
        ExporterBackend::Stdout => {
            let exporter = opentelemetry_stdout::SpanExporter::default();
            SdkTracerProvider::builder()
                .with_resource(resource)
                .with_simple_exporter(exporter)
        }
        ExporterBackend::Noop => {
            // No exporter — spans are created but immediately dropped.
            SdkTracerProvider::builder().with_resource(resource)
        }
        ExporterBackend::Otlp => {
            #[cfg(feature = "otlp-exporter")]
            {
                let exporter = opentelemetry_otlp::SpanExporter::builder()
                    .with_tonic()
                    .with_endpoint(_cfg.otlp_endpoint())
                    .with_metadata(otlp_metadata(_cfg).map_err(TelemetryError::OtlpConfig)?)
                    .with_timeout(_cfg.otlp_timeout())
                    .build()
                    .map_err(|e| TelemetryError::OtlpConfig(e.to_string()))?;
                return Ok(SdkTracerProvider::builder()
                    .with_resource(resource)
                    .with_batch_exporter(exporter)
                    .build());
            }
            #[cfg(not(feature = "otlp-exporter"))]
            return Err(TelemetryError::OtlpConfig(
                "OTLP exporter requires the 'otlp-exporter' feature to be enabled".to_string(),
            ));
        }
    };

    Ok(builder.build())
}

fn build_meter_provider(
    cfg: &TelemetryConfig,
    backend: &ExporterBackend,
    resource: Resource,
) -> Result<SdkMeterProvider, TelemetryError> {
    #[cfg(not(feature = "otlp-exporter"))]
    let _ = cfg;
    let provider = match backend {
        ExporterBackend::Stdout => {
            let exporter = opentelemetry_stdout::MetricExporter::default();
            SdkMeterProvider::builder()
                .with_resource(resource)
                .with_periodic_exporter(exporter)
                .build()
        }
        ExporterBackend::Noop => {
            // Noop: build with no exporter (metrics are discarded).
            SdkMeterProvider::builder().with_resource(resource).build()
        }
        ExporterBackend::Otlp => {
            #[cfg(feature = "otlp-exporter")]
            {
                let exporter = opentelemetry_otlp::MetricExporter::builder()
                    .with_tonic()
                    .with_endpoint(cfg.otlp_endpoint())
                    .with_metadata(otlp_metadata(cfg).map_err(TelemetryError::MeterProviderSetup)?)
                    .with_timeout(cfg.otlp_timeout())
                    .build()
                    .map_err(|e| TelemetryError::MeterProviderSetup(e.to_string()))?;
                SdkMeterProvider::builder()
                    .with_resource(resource)
                    .with_periodic_exporter(exporter)
                    .build()
            }
            #[cfg(not(feature = "otlp-exporter"))]
            {
                // Fall back to no-op metrics when feature is disabled.
                SdkMeterProvider::builder().with_resource(resource).build()
            }
        }
    };
    Ok(provider)
}

#[cfg(feature = "otlp-exporter")]
fn otlp_metadata(cfg: &TelemetryConfig) -> Result<MetadataMap, String> {
    let headers = cfg.otlp_headers();
    let mut metadata = MetadataMap::with_capacity(headers.len());
    for (key, value) in headers {
        let metadata_key: MetadataKey<Ascii> = key
            .parse()
            .map_err(|err| format!("invalid OTLP header key '{key}': {err}"))?;
        let metadata_value: MetadataValue<Ascii> = value
            .parse()
            .map_err(|err| format!("invalid OTLP header value for '{key}': {err}"))?;
        metadata.insert(metadata_key, metadata_value);
    }
    Ok(metadata)
}

fn install_subscriber(
    cfg: &TelemetryConfig,
    tracer_provider: Option<&SdkTracerProvider>,
) -> Result<(), TelemetryError> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = tracing_subscriber::registry().with(env_filter);

    match cfg.log_format {
        LogFormat::Json => {
            let fmt_layer = tracing_subscriber::fmt::layer().json();
            if let Some(tp) = tracer_provider {
                let otel_layer = OpenTelemetryLayer::new(tp.tracer(cfg.service_name.clone()));
                subscriber
                    .with(fmt_layer)
                    .with(otel_layer)
                    .try_init()
                    .map_err(|e| TelemetryError::TracerProviderSetup(e.to_string()))?;
            } else {
                subscriber
                    .with(fmt_layer)
                    .try_init()
                    .map_err(|e| TelemetryError::TracerProviderSetup(e.to_string()))?;
            }
        }
        LogFormat::Text => {
            let fmt_layer = tracing_subscriber::fmt::layer();
            if let Some(tp) = tracer_provider {
                let otel_layer = OpenTelemetryLayer::new(tp.tracer(cfg.service_name.clone()));
                subscriber
                    .with(fmt_layer)
                    .with(otel_layer)
                    .try_init()
                    .map_err(|e| TelemetryError::TracerProviderSetup(e.to_string()))?;
            } else {
                subscriber
                    .with(fmt_layer)
                    .try_init()
                    .map_err(|e| TelemetryError::TracerProviderSetup(e.to_string()))?;
            }
        }
    }

    Ok(())
}

fn is_global_subscriber_already_set(err: &TelemetryError) -> bool {
    matches!(err, TelemetryError::TracerProviderSetup(message) if message.contains("global default") && message.contains("already"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TelemetryConfig;

    mod test_is_initialized {
        use super::*;

        #[test]
        fn returns_bool_without_panic() {
            // We can't guarantee order of test execution, so just assert it
            // returns a bool without panicking.
            let _ = is_initialized();
        }

        #[test]
        fn direct_span_fallback_returns_bool_without_panic() {
            let _ = direct_span_fallback_enabled();
        }
    }

    mod test_shutdown {
        use super::*;

        #[test]
        fn shutdown_before_init_is_safe() {
            // If not initialized, shutdown should be a no-op.
            shutdown(); // must not panic
        }

        #[test]
        fn shutdown_marks_provider_inactive() {
            ACTIVE.store(true, Ordering::Release);
            DIRECT_SPAN_FALLBACK.store(true, Ordering::Release);

            shutdown();

            assert!(!is_initialized());
            assert!(!direct_span_fallback_enabled());
        }
    }

    mod test_build_tracer_provider {
        use super::*;

        #[test]
        fn noop_backend_builds_successfully() {
            let cfg = TelemetryConfig::default();
            let resource = Resource::builder_empty().build();
            let result = build_tracer_provider(&cfg, &ExporterBackend::Noop, resource);
            assert!(result.is_ok());
        }

        #[test]
        fn stdout_backend_builds_successfully() {
            let cfg = TelemetryConfig::default();
            let resource = Resource::builder_empty().build();
            let result = build_tracer_provider(&cfg, &ExporterBackend::Stdout, resource);
            assert!(result.is_ok());
        }

        #[cfg(not(feature = "otlp-exporter"))]
        #[test]
        fn otlp_backend_without_feature_returns_error() {
            let cfg = TelemetryConfig::default();
            let resource = Resource::builder_empty().build();
            let result = build_tracer_provider(&cfg, &ExporterBackend::Otlp, resource);
            assert!(matches!(result, Err(TelemetryError::OtlpConfig(_))));
        }

        #[cfg(feature = "otlp-exporter")]
        #[tokio::test]
        async fn otlp_backend_with_feature_builds_inside_runtime() {
            let cfg = TelemetryConfig::default();
            let resource = Resource::builder_empty().build();
            let result = build_tracer_provider(&cfg, &ExporterBackend::Otlp, resource);
            assert!(result.is_ok());
        }
    }

    mod test_subscriber_error_classification {
        use super::*;

        #[test]
        fn detects_existing_global_subscriber_error() {
            let err = TelemetryError::TracerProviderSetup(
                "a global default trace dispatcher has already been set".to_string(),
            );
            assert!(is_global_subscriber_already_set(&err));
        }

        #[test]
        fn rejects_unrelated_subscriber_errors() {
            let err = TelemetryError::TracerProviderSetup("invalid layer".to_string());
            assert!(!is_global_subscriber_already_set(&err));
        }
    }

    mod test_build_meter_provider {
        use super::*;

        #[test]
        fn noop_backend_builds_successfully() {
            let cfg = TelemetryConfig::default();
            let resource = Resource::builder_empty().build();
            let result = build_meter_provider(&cfg, &ExporterBackend::Noop, resource);
            assert!(result.is_ok());
        }
    }

    mod test_config_defaults {
        use super::*;

        #[test]
        fn default_config_has_expected_values() {
            let cfg = TelemetryConfig::default();
            assert_eq!(cfg.service_name, "dcc-mcp-core");
            assert!(cfg.enable_tracing);
            assert!(cfg.enable_metrics);
        }
    }
}
