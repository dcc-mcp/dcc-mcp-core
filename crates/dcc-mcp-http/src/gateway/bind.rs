/// Attempt to bind `host:port` with `SO_REUSEADDR = false`.
///
/// Returns a bound listener on success, or a detailed `io::Error` on failure.
/// Used by both the initial gateway competition and the challenger retry loop.
///
/// Unlike earlier revisions that returned `Option<TcpListener>` via `.ok()?`,
/// this surface preserves the real cause — `EADDRINUSE`, `EACCES`, a Windows
/// overlapped-I/O registration error from `TcpListener::from_std`, etc. —
/// so callers can log it and distinguish "port in use" from "socket setup
/// failed" (issue #303, suggestion D).
pub(crate) async fn try_bind_port(
    host: &str,
    port: u16,
) -> std::io::Result<tokio::net::TcpListener> {
    use socket2::{Domain, Socket, Type};

    let addr: std::net::SocketAddr =
        format!("{host}:{port}")
            .parse()
            .map_err(|e: std::net::AddrParseError| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
            })?;
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, None)?;
    socket.set_reuse_address(false)?;
    #[cfg(unix)]
    socket.set_reuse_port(false)?;
    socket.bind(&addr.into())?;
    socket.listen(128)?;
    socket.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(std::net::TcpListener::from(socket))
}

/// `Option`-returning wrapper kept for the call sites that only care about
/// win/lose semantics. Non-`AddrInUse` errors are still logged so they are
/// never silently discarded (fixes the "silent bind error" leg of #303).
pub(crate) async fn try_bind_port_opt(host: &str, port: u16) -> Option<tokio::net::TcpListener> {
    match try_bind_port(host, port).await {
        Ok(l) => Some(l),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => None,
        Err(e) => {
            tracing::warn!(
                host = %host,
                port = port,
                error = %e,
                kind = ?e.kind(),
                "gateway bind failed (non-AddrInUse) — treating as lost election"
            );
            None
        }
    }
}
