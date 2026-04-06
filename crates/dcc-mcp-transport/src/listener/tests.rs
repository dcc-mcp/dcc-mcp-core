//! Tests for IpcListener and ListenerHandle.

// ── TCP listener tests ──

mod tcp_listener {
    use std::time::Duration;

    use crate::connector::connect;
    use crate::framed::FramedIo;
    use crate::ipc::TransportAddress;
    use crate::listener::IpcListener;
    use crate::message::{Request, Response};

    #[tokio::test]
    async fn test_bind_tcp_ephemeral_port() {
        let addr = TransportAddress::tcp("127.0.0.1", 0);
        let listener = IpcListener::bind(&addr).await.unwrap();

        assert_eq!(listener.transport_name(), "tcp");

        let local = listener.local_address().unwrap();
        if let TransportAddress::Tcp { host, port } = local {
            assert_eq!(host, "127.0.0.1");
            assert_ne!(port, 0, "should have been assigned a real port");
        } else {
            panic!("expected Tcp address");
        }
    }

    #[tokio::test]
    async fn test_accept_tcp_connection() {
        let addr = TransportAddress::tcp("127.0.0.1", 0);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let local = listener.local_address().unwrap();

        // Connect from client side.
        let accept_fut = listener.accept();
        let connect_fut = connect(&local, Duration::from_secs(5));

        let (server_result, client_result) = tokio::join!(accept_fut, connect_fut);

        let server_stream = server_result.unwrap();
        let _client_stream = client_result.unwrap();

        assert_eq!(server_stream.transport_name(), "tcp");
    }

    #[tokio::test]
    async fn test_tcp_framed_roundtrip() {
        let addr = TransportAddress::tcp("127.0.0.1", 0);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let local = listener.local_address().unwrap();

        let server_fut = async {
            let stream = listener.accept().await.unwrap();
            let mut framed = FramedIo::new(stream);
            let req: Request = framed.recv().await.unwrap();
            let resp = Response {
                id: req.id,
                success: true,
                payload: b"hello back".to_vec(),
                error: None,
            };
            framed.send(&resp).await.unwrap();
        };

        let client_fut = async {
            let stream = connect(&local, Duration::from_secs(5)).await.unwrap();
            let mut framed = FramedIo::new(stream);

            let req = Request {
                id: uuid::Uuid::new_v4(),
                method: "ping".to_string(),
                params: b"hello".to_vec(),
            };
            framed.send(&req).await.unwrap();

            let resp: Response = framed.recv().await.unwrap();
            assert!(resp.success);
            assert_eq!(resp.payload, b"hello back");
        };

        tokio::join!(server_fut, client_fut);
    }

    #[tokio::test]
    async fn test_tcp_multiple_accepts() {
        let addr = TransportAddress::tcp("127.0.0.1", 0);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let local = listener.local_address().unwrap();

        // Accept 3 connections sequentially.
        for i in 0..3 {
            let accept_fut = listener.accept();
            let connect_fut = connect(&local, Duration::from_secs(5));
            let (server_result, _client_result) = tokio::join!(accept_fut, connect_fut);
            let stream = server_result.unwrap();
            assert_eq!(stream.transport_name(), "tcp", "connection {i}");
        }
    }

    #[tokio::test]
    async fn test_bind_tcp_invalid_address() {
        // Binding to an invalid address should fail.
        let addr = TransportAddress::tcp("999.999.999.999", 0);
        let result = IpcListener::bind(&addr).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::TransportError::IpcConnectionFailed { address, .. } => {
                assert!(address.starts_with("tcp://"));
            }
            other => panic!("expected IpcConnectionFailed, got: {other:?}"),
        }
    }
}

// ── ListenerHandle tests ──

mod listener_handle_tests {
    use std::time::Duration;

    use crate::connector::connect;
    use crate::ipc::TransportAddress;
    use crate::listener::{IpcListener, ListenerHandle};

    #[tokio::test]
    async fn test_handle_accept_and_count() {
        let addr = TransportAddress::tcp("127.0.0.1", 0);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let local = listener.local_address().unwrap();
        let handle = ListenerHandle::new(listener);

        assert_eq!(handle.accept_count(), 0);
        assert!(!handle.is_shutdown());

        let accept_fut = handle.accept();
        let connect_fut = connect(&local, Duration::from_secs(5));
        let (server_result, _client_result) = tokio::join!(accept_fut, connect_fut);

        assert!(server_result.unwrap().is_ok());
        assert_eq!(handle.accept_count(), 1);
    }

    #[tokio::test]
    async fn test_handle_shutdown() {
        let addr = TransportAddress::tcp("127.0.0.1", 0);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let handle = ListenerHandle::new(listener);

        handle.shutdown();
        assert!(handle.is_shutdown());

        let result = handle.accept().await;
        assert!(result.is_none(), "should return None after shutdown");
    }

    #[tokio::test]
    async fn test_handle_transport_name() {
        let addr = TransportAddress::tcp("127.0.0.1", 0);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let handle = ListenerHandle::new(listener);
        assert_eq!(handle.transport_name(), "tcp");
    }

    #[tokio::test]
    async fn test_handle_local_address() {
        let addr = TransportAddress::tcp("127.0.0.1", 0);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let handle = ListenerHandle::new(listener);

        let local = handle.local_address().unwrap();
        if let TransportAddress::Tcp { port, .. } = local {
            assert_ne!(port, 0);
        } else {
            panic!("expected Tcp address");
        }
    }
}

// ── Windows Named Pipe listener tests ──

#[cfg(windows)]
mod named_pipe_listener {
    use std::time::Duration;

    use crate::connector::connect;
    use crate::framed::FramedIo;
    use crate::ipc::TransportAddress;
    use crate::listener::IpcListener;
    use crate::message::{Request, Response};

    #[tokio::test]
    async fn test_bind_named_pipe() {
        let pipe_name = format!("dcc-mcp-test-listener-{}", uuid::Uuid::new_v4());
        let addr = TransportAddress::named_pipe(&pipe_name);
        let listener = IpcListener::bind(&addr).await.unwrap();

        assert_eq!(listener.transport_name(), "named_pipe");

        let local = listener.local_address().unwrap();
        if let TransportAddress::NamedPipe { path } = local {
            assert!(path.contains(&pipe_name));
        } else {
            panic!("expected NamedPipe address");
        }
    }

    #[tokio::test]
    async fn test_named_pipe_accept() {
        let pipe_name = format!("dcc-mcp-test-accept-{}", uuid::Uuid::new_v4());
        let addr = TransportAddress::named_pipe(&pipe_name);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let local = listener.local_address().unwrap();

        let accept_fut = listener.accept();
        let connect_fut = connect(&local, Duration::from_secs(5));

        let (server_result, client_result) = tokio::join!(accept_fut, connect_fut);
        assert!(server_result.is_ok());
        assert!(client_result.is_ok());
    }

    #[tokio::test]
    async fn test_named_pipe_framed_roundtrip() {
        let pipe_name = format!("dcc-mcp-test-framed-{}", uuid::Uuid::new_v4());
        let addr = TransportAddress::named_pipe(&pipe_name);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let local = listener.local_address().unwrap();

        let server_fut = async {
            let stream = listener.accept().await.unwrap();
            let mut framed = FramedIo::new(stream);
            let req: Request = framed.recv().await.unwrap();
            let resp = Response {
                id: req.id,
                success: true,
                payload: b"pipe-response".to_vec(),
                error: None,
            };
            framed.send(&resp).await.unwrap();
        };

        let client_fut = async {
            let stream = connect(&local, Duration::from_secs(5)).await.unwrap();
            let mut framed = FramedIo::new(stream);
            let req = Request {
                id: uuid::Uuid::new_v4(),
                method: "test".to_string(),
                params: b"pipe-request".to_vec(),
            };
            framed.send(&req).await.unwrap();
            let resp: Response = framed.recv().await.unwrap();
            assert!(resp.success);
            assert_eq!(resp.payload, b"pipe-response");
        };

        tokio::join!(server_fut, client_fut);
    }
}

// ── Unix Socket listener tests ──

#[cfg(unix)]
mod unix_socket_listener {
    use std::time::Duration;

    use crate::connector::connect;
    use crate::ipc::TransportAddress;
    use crate::listener::IpcListener;

    #[tokio::test]
    async fn test_bind_unix_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sock");
        let addr = TransportAddress::unix_socket(&path);
        let listener = IpcListener::bind(&addr).await.unwrap();

        assert_eq!(listener.transport_name(), "unix_socket");
    }

    #[tokio::test]
    async fn test_unix_socket_accept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-accept.sock");
        let addr = TransportAddress::unix_socket(&path);
        let listener = IpcListener::bind(&addr).await.unwrap();
        let local = listener.local_address().unwrap();

        let accept_fut = listener.accept();
        let connect_fut = connect(&local, Duration::from_secs(5));

        let (server_result, client_result) = tokio::join!(accept_fut, connect_fut);
        assert!(server_result.is_ok());
        assert!(client_result.is_ok());
    }

    #[tokio::test]
    async fn test_unix_socket_removes_stale_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-stale.sock");

        // Create a stale file at the socket path.
        std::fs::write(&path, "stale").unwrap();
        assert!(path.exists());

        // Binding should succeed (removes stale file).
        let addr = TransportAddress::unix_socket(&path);
        let listener = IpcListener::bind(&addr).await.unwrap();
        assert_eq!(listener.transport_name(), "unix_socket");
    }
}

// ── Platform-unsupported listener tests ──

#[cfg(not(windows))]
mod not_windows {
    use crate::ipc::TransportAddress;
    use crate::listener::IpcListener;

    #[tokio::test]
    async fn test_bind_named_pipe_unsupported() {
        let addr = TransportAddress::named_pipe("dcc-mcp-test");
        let result = IpcListener::bind(&addr).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::TransportError::IpcNotSupported { transport, .. } => {
                assert_eq!(transport, "named_pipe");
            }
            other => panic!("expected IpcNotSupported, got: {other:?}"),
        }
    }
}

#[cfg(not(unix))]
mod not_unix {
    use crate::ipc::TransportAddress;
    use crate::listener::IpcListener;

    #[tokio::test]
    async fn test_bind_unix_socket_unsupported() {
        let addr = TransportAddress::unix_socket("/tmp/dcc-mcp-test.sock");
        let result = IpcListener::bind(&addr).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::TransportError::IpcNotSupported { transport, .. } => {
                assert_eq!(transport, "unix_socket");
            }
            other => panic!("expected IpcNotSupported, got: {other:?}"),
        }
    }
}
