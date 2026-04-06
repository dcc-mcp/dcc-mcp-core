//! Transport, pool, and session configuration.

use std::time::Duration;

/// Configuration for the transport layer.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Pool configuration.
    pub pool: PoolConfig,
    /// Session configuration.
    pub session: SessionConfig,
    /// Connect timeout for new TCP connections.
    pub connect_timeout: Duration,
    /// Heartbeat interval for health checks.
    pub heartbeat_interval: Duration,
    /// Optional listen address for server-side IPC.
    ///
    /// When set, callers can use [`TransportManager::listen`] to bind an
    /// [`IpcListener`](crate::listener::IpcListener) without specifying the address again.
    ///
    /// Encoded as `"tcp://host:port"`, `"pipe://name"`, or `"unix:///path"`.
    pub listen_address: Option<String>,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            pool: PoolConfig::default(),
            session: SessionConfig::default(),
            connect_timeout: Duration::from_secs(10),
            heartbeat_interval: Duration::from_secs(5),
            listen_address: None,
        }
    }
}

/// Configuration for the connection pool.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum connections per DCC type.
    pub max_connections_per_type: usize,
    /// Maximum idle time before eviction.
    pub max_idle_time: Duration,
    /// Maximum lifetime for a connection.
    pub max_lifetime: Duration,
    /// Timeout when acquiring a connection from the pool.
    pub acquire_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections_per_type: 10,
            max_idle_time: Duration::from_secs(300),
            max_lifetime: Duration::from_secs(3600),
            acquire_timeout: Duration::from_secs(30),
        }
    }
}

/// Configuration for session management.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Time before an idle session is eligible for cleanup.
    pub idle_timeout: Duration,
    /// Maximum retries for automatic reconnection.
    pub reconnect_max_retries: u32,
    /// Base backoff duration for exponential reconnection.
    pub reconnect_backoff_base: Duration,
    /// Maximum session lifetime.
    pub max_session_lifetime: Duration,
    /// Interval for heartbeat / health-check pings.
    pub heartbeat_interval: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(300),
            reconnect_max_retries: 3,
            reconnect_backoff_base: Duration::from_secs(1),
            max_session_lifetime: Duration::from_secs(3600),
            heartbeat_interval: Duration::from_secs(5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert_eq!(config.heartbeat_interval, Duration::from_secs(5));
        assert!(config.listen_address.is_none());
    }

    #[test]
    fn test_transport_config_with_listen_address() {
        let config = TransportConfig {
            listen_address: Some("tcp://127.0.0.1:9000".to_string()),
            ..Default::default()
        };
        assert_eq!(
            config.listen_address.as_deref(),
            Some("tcp://127.0.0.1:9000")
        );
    }

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.max_connections_per_type, 10);
        assert_eq!(config.max_idle_time, Duration::from_secs(300));
        assert_eq!(config.max_lifetime, Duration::from_secs(3600));
        assert_eq!(config.acquire_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_session_config_default() {
        let config = SessionConfig::default();
        assert_eq!(config.idle_timeout, Duration::from_secs(300));
        assert_eq!(config.reconnect_max_retries, 3);
        assert_eq!(config.reconnect_backoff_base, Duration::from_secs(1));
        assert_eq!(config.max_session_lifetime, Duration::from_secs(3600));
        assert_eq!(config.heartbeat_interval, Duration::from_secs(5));
    }
}
