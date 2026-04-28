//! End-to-end smoke test for the relay MVP (issue #504).
//!
//! Spins up:
//!   1. an in-process echo TCP server (the "local DCC")
//!   2. a `RelayServer` on `127.0.0.1:0` for both agent and frontend
//!   3. one `dcc-mcp-tunnel-agent` registration that bridges to the echo
//!   4. one frontend client that selects the tunnel and round-trips bytes
//!
//! Asserts that bytes round-trip end-to-end through the relay.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

use dcc_mcp_tunnel_agent::{AgentConfig, run_once};
use dcc_mcp_tunnel_protocol::{TunnelClaims, auth};
use dcc_mcp_tunnel_relay::{RelayConfig, RelayServer, data::write_select_tunnel};

const SECRET: &[u8] = b"e2e-test-secret-must-exceed-32-bytes";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn spawn_echo_server() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => return,
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = sock.read(&mut buf).await {
                    if n == 0 {
                        return;
                    }
                    if sock.write_all(&buf[..n]).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn frontend_round_trips_through_relay_to_local_echo() {
    let echo = spawn_echo_server().await;

    let cfg = RelayConfig {
        jwt_secret: SECRET.to_vec(),
        public_host: "localhost".into(),
        base_url: "tcp://localhost:0".into(),
        stale_timeout: Duration::from_secs(60),
        max_tunnels: 0,
    };
    let relay = RelayServer::start(
        cfg,
        "127.0.0.1:0".parse().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    let agent_addr = relay.agent_addr;
    let frontend_addr = relay.frontend_addr;
    let registry = Arc::clone(&relay.registry);

    let claims = TunnelClaims {
        sub: "e2e-test".into(),
        iat: now_secs(),
        exp: now_secs() + 600,
        iss: "e2e".into(),
        allowed_dcc: vec!["maya".into()],
    };
    let token = auth::issue(&claims, SECRET).unwrap();

    let agent_cfg = AgentConfig {
        relay_url: agent_addr.to_string(),
        token,
        dcc: "maya".into(),
        capabilities: vec!["scene.read".into()],
        agent_version: "e2e/0.0".into(),
        local_target: echo.to_string(),
        heartbeat_interval: Duration::from_secs(5),
        reconnect: dcc_mcp_tunnel_agent::ReconnectPolicy::default(),
    };
    let agent_task = tokio::spawn(async move { run_once(agent_cfg).await });

    // Wait for the registry to see the tunnel — much faster than polling
    // the agent task itself, and gives us the assigned tunnel id.
    let tunnel_id = wait_for_tunnel(&registry).await;

    let mut frontend = TcpStream::connect(frontend_addr).await.unwrap();
    write_select_tunnel(&mut frontend, &tunnel_id)
        .await
        .unwrap();

    let payload = b"hello-relay-mvp";
    frontend.write_all(payload).await.unwrap();

    let mut reply = vec![0u8; payload.len()];
    timeout(Duration::from_secs(3), frontend.read_exact(&mut reply))
        .await
        .expect("echo round-trip timed out")
        .expect("echo round-trip failed");
    assert_eq!(&reply[..], payload);

    drop(frontend);
    relay.shutdown();
    agent_task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_invalid_token() {
    let cfg = RelayConfig {
        jwt_secret: SECRET.to_vec(),
        ..RelayConfig::default()
    };
    let relay = RelayServer::start(
        cfg,
        "127.0.0.1:0".parse().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    let agent_cfg = AgentConfig::new(
        relay.agent_addr.to_string(),
        "garbage-token",
        "maya",
        "127.0.0.1:1",
    );
    let outcome = run_once(agent_cfg).await;
    assert!(
        matches!(
            outcome,
            Err(dcc_mcp_tunnel_agent::ClientError::Rejected(ref ack))
                if !ack.ok
        ),
        "expected Rejected, got {outcome:?}"
    );
    relay.shutdown();
}

async fn wait_for_tunnel(registry: &dcc_mcp_tunnel_relay::TunnelRegistry) -> String {
    timeout(Duration::from_secs(3), async {
        loop {
            if let Some(entry) = registry.iter().next() {
                return entry.key().clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("registry never saw the tunnel")
}
