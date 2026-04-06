//! IPC transport unit tests.

use super::*;

// ── TransportAddress tests ──

mod test_transport_address {
    use super::*;

    #[test]
    fn test_tcp_address() {
        let addr = TransportAddress::tcp("127.0.0.1", 18812);
        assert!(addr.is_tcp());
        assert!(!addr.is_named_pipe());
        assert!(!addr.is_unix_socket());
        assert!(addr.is_local());
        assert_eq!(addr.scheme(), "tcp");
        assert_eq!(addr.tcp_parts(), Some(("127.0.0.1", 18812)));
        assert!(addr.ipc_path().is_none());
    }

    #[test]
    fn test_tcp_remote_not_local() {
        let addr = TransportAddress::tcp("192.168.1.100", 18812);
        assert!(!addr.is_local());
    }

    #[test]
    fn test_tcp_localhost_is_local() {
        let addr = TransportAddress::tcp("localhost", 18812);
        assert!(addr.is_local());
    }

    #[test]
    fn test_tcp_ipv6_loopback_is_local() {
        let addr = TransportAddress::tcp("::1", 18812);
        assert!(addr.is_local());
    }

    #[test]
    fn test_named_pipe_address() {
        let addr = TransportAddress::named_pipe("dcc-mcp-maya-12345");
        assert!(addr.is_named_pipe());
        assert!(!addr.is_tcp());
        assert!(addr.is_local());
        assert_eq!(addr.scheme(), "pipe");

        if let TransportAddress::NamedPipe { path } = &addr {
            assert_eq!(path, r"\\.\pipe\dcc-mcp-maya-12345");
        } else {
            panic!("expected NamedPipe");
        }
    }

    #[test]
    fn test_named_pipe_full_path() {
        let addr = TransportAddress::named_pipe(r"\\.\pipe\my-custom-pipe");
        if let TransportAddress::NamedPipe { path } = &addr {
            assert_eq!(path, r"\\.\pipe\my-custom-pipe");
        } else {
            panic!("expected NamedPipe");
        }
    }

    #[test]
    fn test_unix_socket_address() {
        let addr = TransportAddress::unix_socket("/tmp/dcc-mcp-maya-12345.sock");
        assert!(addr.is_unix_socket());
        assert!(!addr.is_tcp());
        assert!(addr.is_local());
        assert_eq!(addr.scheme(), "unix");
    }

    #[test]
    fn test_default_pipe_name() {
        let addr = TransportAddress::default_pipe_name("maya", 12345);
        if let TransportAddress::NamedPipe { path } = &addr {
            assert_eq!(path, r"\\.\pipe\dcc-mcp-maya-12345");
        } else {
            panic!("expected NamedPipe");
        }
    }

    #[test]
    fn test_default_unix_socket() {
        let addr = TransportAddress::default_unix_socket("blender", 6789);
        if let TransportAddress::UnixSocket { path } = &addr {
            let expected = std::env::temp_dir().join("dcc-mcp-blender-6789.sock");
            assert_eq!(path, &expected);
        } else {
            panic!("expected UnixSocket");
        }
    }

    #[test]
    fn test_default_local_platform() {
        let addr = TransportAddress::default_local("houdini", 9999);
        if cfg!(windows) {
            assert!(addr.is_named_pipe());
        } else {
            assert!(addr.is_unix_socket());
        }
    }

    #[test]
    fn test_display_tcp() {
        let addr = TransportAddress::tcp("192.168.1.10", 8080);
        assert_eq!(addr.to_string(), "tcp://192.168.1.10:8080");
    }

    #[test]
    fn test_display_named_pipe() {
        let addr = TransportAddress::named_pipe("test-pipe");
        assert_eq!(addr.to_string(), r"pipe://\\.\pipe\test-pipe");
    }

    #[test]
    fn test_display_unix_socket() {
        let addr = TransportAddress::unix_socket("/tmp/test.sock");
        assert_eq!(addr.to_string(), "unix:///tmp/test.sock");
    }

    #[test]
    fn test_serialization_tcp() {
        let addr = TransportAddress::tcp("127.0.0.1", 18812);
        let json = serde_json::to_string(&addr).unwrap();
        let deserialized: TransportAddress = serde_json::from_str(&json).unwrap();
        assert_eq!(addr, deserialized);
    }

    #[test]
    fn test_serialization_named_pipe() {
        let addr = TransportAddress::named_pipe("test");
        let json = serde_json::to_string(&addr).unwrap();
        let deserialized: TransportAddress = serde_json::from_str(&json).unwrap();
        assert_eq!(addr, deserialized);
    }

    #[test]
    fn test_serialization_unix_socket() {
        let addr = TransportAddress::unix_socket("/tmp/test.sock");
        let json = serde_json::to_string(&addr).unwrap();
        let deserialized: TransportAddress = serde_json::from_str(&json).unwrap();
        assert_eq!(addr, deserialized);
    }

    #[test]
    fn test_connection_string_tcp() {
        let addr = TransportAddress::tcp("10.0.0.1", 9090);
        assert_eq!(addr.to_connection_string(), "tcp://10.0.0.1:9090");
    }

    #[test]
    fn test_ipc_path_for_pipe() {
        let addr = TransportAddress::named_pipe("my-pipe");
        let path = addr.ipc_path().unwrap();
        assert!(path.to_str().unwrap().contains("my-pipe"));
    }

    #[test]
    fn test_ipc_path_for_unix() {
        let addr = TransportAddress::unix_socket("/tmp/my.sock");
        let path = addr.ipc_path().unwrap();
        assert_eq!(path, Path::new("/tmp/my.sock"));
    }

    // ── parse tests ──

    #[test]
    fn test_parse_tcp() {
        let addr = TransportAddress::parse("tcp://127.0.0.1:9000").unwrap();
        assert!(addr.is_tcp());
        assert_eq!(addr.tcp_parts(), Some(("127.0.0.1", 9000)));
    }

    #[test]
    fn test_parse_tcp_ipv6() {
        let addr = TransportAddress::parse("tcp://::1:8080").unwrap();
        assert!(addr.is_tcp());
    }

    #[test]
    fn test_parse_named_pipe() {
        let addr = TransportAddress::parse("pipe://my-dcc-pipe").unwrap();
        assert!(addr.is_named_pipe());
    }

    #[test]
    fn test_parse_unix_socket() {
        let addr = TransportAddress::parse("unix:///tmp/dcc.sock").unwrap();
        assert!(addr.is_unix_socket());
    }

    #[test]
    fn test_parse_invalid_scheme() {
        let result = TransportAddress::parse("http://localhost:80");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown scheme"));
    }

    #[test]
    fn test_parse_tcp_missing_port() {
        let result = TransportAddress::parse("tcp://127.0.0.1");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tcp_invalid_port() {
        let result = TransportAddress::parse("tcp://127.0.0.1:999999");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_roundtrip_tcp() {
        let original = TransportAddress::tcp("10.0.0.1", 7777);
        let parsed = TransportAddress::parse(&original.to_connection_string()).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_from_str_tcp() {
        let addr: TransportAddress = "tcp://127.0.0.1:9000".parse().unwrap();
        assert!(addr.is_tcp());
        assert_eq!(addr.tcp_parts(), Some(("127.0.0.1", 9000)));
    }

    #[test]
    fn test_from_str_named_pipe() {
        let addr: TransportAddress = "pipe://dcc-mcp-maya".parse().unwrap();
        assert!(addr.is_named_pipe());
        // On Windows, named pipe paths are expanded to \\.\pipe\<name>
        // On other platforms, the raw name is kept as-is.
        let path = addr.ipc_path().expect("should have ipc_path");
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("dcc-mcp-maya"),
            "expected path to contain 'dcc-mcp-maya', got: {path_str}"
        );
    }

    #[test]
    fn test_from_str_unix_socket() {
        let addr: TransportAddress = "unix:///tmp/dcc-mcp.sock".parse().unwrap();
        assert!(addr.is_unix_socket());
        assert_eq!(
            addr.ipc_path(),
            Some(std::path::Path::new("/tmp/dcc-mcp.sock"))
        );
    }

    #[test]
    fn test_from_str_invalid_returns_err() {
        let result: Result<TransportAddress, _> = "http://localhost:8080".parse();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("unknown scheme"));
    }

    #[test]
    fn test_from_str_roundtrip_all_variants() {
        let tcp = TransportAddress::tcp("192.168.1.1", 1234);
        let parsed_tcp: TransportAddress = tcp.to_string().parse().unwrap();
        assert_eq!(tcp, parsed_tcp);

        let pipe = TransportAddress::named_pipe("test-pipe-name");
        let parsed_pipe: TransportAddress = pipe.to_string().parse().unwrap();
        assert_eq!(pipe, parsed_pipe);

        let unix = TransportAddress::unix_socket("/tmp/test.sock");
        let parsed_unix: TransportAddress = unix.to_string().parse().unwrap();
        assert_eq!(unix, parsed_unix);
    }
}

// ── TransportScheme tests ──

mod test_transport_scheme {
    use super::*;

    #[test]
    fn test_default_is_auto() {
        assert_eq!(TransportScheme::default(), TransportScheme::Auto);
    }

    #[test]
    fn test_tcp_only_always_tcp() {
        let scheme = TransportScheme::TcpOnly;
        let addr = scheme.select_address("maya", "127.0.0.1", 18812, Some(12345));
        assert!(addr.is_tcp());
    }

    #[test]
    fn test_tcp_only_remote() {
        let scheme = TransportScheme::TcpOnly;
        let addr = scheme.select_address("maya", "192.168.1.100", 18812, Some(12345));
        assert!(addr.is_tcp());
        assert_eq!(addr.tcp_parts(), Some(("192.168.1.100", 18812)));
    }

    #[test]
    fn test_auto_local_with_pid() {
        let scheme = TransportScheme::Auto;
        let addr = scheme.select_address("maya", "127.0.0.1", 18812, Some(12345));
        // On local with PID, should prefer IPC
        if cfg!(windows) {
            assert!(addr.is_named_pipe());
        } else if cfg!(unix) {
            assert!(addr.is_unix_socket());
        }
    }

    #[test]
    fn test_auto_local_without_pid() {
        let scheme = TransportScheme::Auto;
        let addr = scheme.select_address("maya", "127.0.0.1", 18812, None);
        // No PID → falls back to TCP
        assert!(addr.is_tcp());
    }

    #[test]
    fn test_auto_remote() {
        let scheme = TransportScheme::Auto;
        let addr = scheme.select_address("maya", "192.168.1.100", 18812, Some(12345));
        // Remote → TCP
        assert!(addr.is_tcp());
    }

    #[test]
    fn test_prefer_ipc_local() {
        let scheme = TransportScheme::PreferIpc;
        let addr = scheme.select_address("blender", "localhost", 9090, Some(54321));
        if cfg!(windows) {
            assert!(addr.is_named_pipe());
        } else if cfg!(unix) {
            assert!(addr.is_unix_socket());
        }
    }

    #[test]
    fn test_prefer_ipc_remote_fallback() {
        let scheme = TransportScheme::PreferIpc;
        let addr = scheme.select_address("blender", "10.0.0.5", 9090, Some(54321));
        assert!(addr.is_tcp());
    }

    #[test]
    fn test_display() {
        assert_eq!(TransportScheme::Auto.to_string(), "auto");
        assert_eq!(TransportScheme::TcpOnly.to_string(), "tcp_only");
        assert_eq!(
            TransportScheme::PreferNamedPipe.to_string(),
            "prefer_named_pipe"
        );
        assert_eq!(
            TransportScheme::PreferUnixSocket.to_string(),
            "prefer_unix_socket"
        );
        assert_eq!(TransportScheme::PreferIpc.to_string(), "prefer_ipc");
    }

    #[test]
    fn test_serialization() {
        let scheme = TransportScheme::PreferIpc;
        let json = serde_json::to_string(&scheme).unwrap();
        let deserialized: TransportScheme = serde_json::from_str(&json).unwrap();
        assert_eq!(scheme, deserialized);
    }
}

// ── IpcConfig tests ──

mod test_ipc_config {
    use super::*;

    #[test]
    fn test_default() {
        let config = IpcConfig::default();
        assert_eq!(config.pipe_prefix, "dcc-mcp");
        assert_eq!(config.buffer_size, 64 * 1024);
        assert_eq!(config.scheme, TransportScheme::Auto);
        assert_eq!(config.connect_timeout, std::time::Duration::from_secs(5));
    }

    #[test]
    fn test_with_scheme() {
        let config = IpcConfig::with_scheme(TransportScheme::TcpOnly);
        assert_eq!(config.scheme, TransportScheme::TcpOnly);
        // Other fields should be defaults
        assert_eq!(config.pipe_prefix, "dcc-mcp");
    }

    #[test]
    fn test_pipe_path() {
        let config = IpcConfig::default();
        let path = config.pipe_path("maya", 12345);
        assert_eq!(path, r"\\.\pipe\dcc-mcp-maya-12345");
    }

    #[test]
    fn test_socket_path() {
        let config = IpcConfig::default();
        let path = config.socket_path("houdini", 9999);
        let expected = std::env::temp_dir().join("dcc-mcp-houdini-9999.sock");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_custom_prefix() {
        let config = IpcConfig {
            pipe_prefix: "my-app".to_string(),
            ..Default::default()
        };
        let path = config.pipe_path("blender", 777);
        assert_eq!(path, r"\\.\pipe\my-app-blender-777");
    }

    #[test]
    fn test_custom_socket_dir() {
        let config = IpcConfig {
            socket_dir: PathBuf::from("/var/run"),
            ..Default::default()
        };
        let path = config.socket_path("maya", 42);
        assert_eq!(path, PathBuf::from("/var/run/dcc-mcp-maya-42.sock"));
    }

    #[test]
    fn test_address_for() {
        let config = IpcConfig::default();
        let addr = config.address_for("maya", 12345);
        if cfg!(windows) {
            assert!(addr.is_named_pipe());
        } else {
            assert!(addr.is_unix_socket());
        }
    }
}

// ── PlatformCapabilities tests ──

mod test_platform_capabilities {
    use super::*;

    #[test]
    fn test_detect() {
        let caps = PlatformCapabilities::detect();
        assert!(caps.tcp); // TCP is always available

        if cfg!(windows) {
            assert!(caps.named_pipe);
            assert!(!caps.unix_socket);
        }
        if cfg!(unix) {
            assert!(!caps.named_pipe);
            assert!(caps.unix_socket);
        }
    }

    #[test]
    fn test_has_ipc() {
        let caps = PlatformCapabilities::detect();
        // At least one IPC should be available on any platform
        assert!(caps.has_ipc());
    }

    #[test]
    fn test_preferred_ipc() {
        let caps = PlatformCapabilities::detect();
        let preferred = caps.preferred_ipc();
        assert!(preferred.is_some());

        if cfg!(windows) {
            assert_eq!(preferred, Some("named_pipe"));
        }
        if cfg!(unix) {
            assert_eq!(preferred, Some("unix_socket"));
        }
    }

    #[test]
    fn test_display() {
        let caps = PlatformCapabilities::detect();
        let display = caps.to_string();
        assert!(display.contains("tcp"));
    }

    #[test]
    fn test_no_ipc_platform() {
        // Simulate a platform with no IPC
        let caps = PlatformCapabilities {
            tcp: true,
            named_pipe: false,
            unix_socket: false,
        };
        assert!(!caps.has_ipc());
        assert!(caps.preferred_ipc().is_none());
    }
}
