//! Error types for dcc-mcp-telemetry.

use thiserror::Error;

/// Errors that can occur during telemetry operations.
#[derive(Debug, Error)]
pub enum TelemetryError {
    /// The telemetry provider has not been initialized.
    #[error("telemetry provider not initialized; call TelemetryConfig::init() first")]
    NotInitialized,

    /// A telemetry provider is already initialized globally.
    #[error("telemetry provider already initialized")]
    AlreadyInitialized,

    /// Tracer provider setup failed.
    #[error("failed to install tracer provider: {0}")]
    TracerProviderSetup(String),

    /// Meter provider setup failed.
    #[error("failed to install meter provider: {0}")]
    MeterProviderSetup(String),

    /// OTLP exporter configuration error.
    #[error("OTLP exporter configuration error: {0}")]
    OtlpConfig(String),

    /// An invalid span name or attribute key was provided.
    #[error("invalid telemetry attribute: {0}")]
    InvalidAttribute(String),

    /// Internal telemetry error.
    #[error("internal telemetry error: {0}")]
    Internal(String),
}

impl From<opentelemetry_sdk::trace::TraceError> for TelemetryError {
    fn from(e: opentelemetry_sdk::trace::TraceError) -> Self {
        TelemetryError::TracerProviderSetup(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_error_display {
        use super::*;

        #[test]
        fn not_initialized_message() {
            let e = TelemetryError::NotInitialized;
            assert!(e.to_string().contains("not initialized"));
        }

        #[test]
        fn already_initialized_message() {
            let e = TelemetryError::AlreadyInitialized;
            assert!(e.to_string().contains("already initialized"));
        }

        #[test]
        fn tracer_provider_setup_wraps_message() {
            let e = TelemetryError::TracerProviderSetup("bad config".into());
            assert!(e.to_string().contains("bad config"));
        }

        #[test]
        fn meter_provider_setup_wraps_message() {
            let e = TelemetryError::MeterProviderSetup("no endpoint".into());
            assert!(e.to_string().contains("no endpoint"));
        }

        #[test]
        fn otlp_config_wraps_message() {
            let e = TelemetryError::OtlpConfig("bad url".into());
            assert!(e.to_string().contains("bad url"));
        }

        #[test]
        fn invalid_attribute_wraps_message() {
            let e = TelemetryError::InvalidAttribute("empty key".into());
            assert!(e.to_string().contains("empty key"));
        }

        #[test]
        fn internal_wraps_message() {
            let e = TelemetryError::Internal("unexpected state".into());
            assert!(e.to_string().contains("unexpected state"));
        }
    }

    mod test_error_from {
        use super::*;

        #[test]
        fn from_trace_error() {
            let trace_err = opentelemetry_sdk::trace::TraceError::Other("test error".into());
            let tel_err = TelemetryError::from(trace_err);
            assert!(matches!(tel_err, TelemetryError::TracerProviderSetup(_)));
        }
    }
}
