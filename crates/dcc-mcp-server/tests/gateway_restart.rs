//! Exercise the real server's restart/daemon subprocess boundary with a stale
//! pidfile. The round-trip parser tests cover every setting; this test proves
//! the production spawn path uses that argv and the live gateway keeps it.

#![cfg(feature = "gateway-daemon")]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

struct GatewayProcess {
    pidfile: PathBuf,
}

impl GatewayProcess {
    fn pid(&self) -> Option<Pid> {
        std::fs::read_to_string(&self.pidfile)
            .ok()?
            .trim()
            .parse::<u32>()
            .ok()
            .map(Pid::from_u32)
    }

    fn stop(&self) -> Result<(), String> {
        let Some(pid) = self.pid() else {
            return Ok(());
        };
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::everything(),
        );
        let Some(process) = system.process(pid) else {
            return Ok(());
        };
        if matches!(
            process.status(),
            ProcessStatus::Dead | ProcessStatus::Zombie
        ) {
            return Ok(());
        }
        // Only kill the test's daemon, never a PID reused by another process.
        if !process
            .cmd()
            .iter()
            .any(|arg| arg == self.pidfile.as_os_str())
        {
            return Err(format!("PID {pid} is not the test daemon"));
        }
        if !process.kill() {
            return Err(format!("could not stop test daemon {pid}"));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing(),
            );
            if system.process(pid).is_none_or(|process| {
                matches!(
                    process.status(),
                    ProcessStatus::Dead | ProcessStatus::Zombie
                )
            }) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "test daemon {pid} did not exit within five seconds"
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for GatewayProcess {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("gateway test cleanup failed: {error}");
        }
    }
}

fn isolated_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-server"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("DCC_MCP_") {
            command.env_remove(key);
        }
    }
    command
}

#[tokio::test]
async fn restart_preserves_explicit_boolean_values_in_the_live_gateway() {
    for enabled in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let gateway = GatewayProcess {
            pidfile: temp.path().join("gateway with spaces.pid"),
        };
        // Produce a reaped PID rather than guessing a number or targeting any
        // existing process. Restart's stale-pidfile path must spawn a real child.
        let mut exited = isolated_command()
            .arg("--version")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let stale_pid = exited.id();
        assert!(exited.wait().unwrap().success());
        std::fs::write(&gateway.pidfile, stale_pid.to_string()).unwrap();

        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let mut command = isolated_command();
        let value = enabled.to_string();
        command.args([
            "gateway",
            "--restart",
            "--daemon",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--remote-port",
            "0",
            "--gateway-idle-timeout-secs",
            "30",
        ]);
        command.arg("--pidfile").arg(&gateway.pidfile);
        command
            .arg("--registry-dir")
            .arg(temp.path().join("registry"));
        command.env("HOME", temp.path());
        command.env("USERPROFILE", temp.path());
        command.env("DCC_MCP_GATEWAY_ENSURE_TIMEOUT_SECS", "15");
        for (flag, key) in [
            ("gateway-persist", "DCC_MCP_GATEWAY_PERSIST"),
            ("no-admin", "DCC_MCP_NO_ADMIN"),
            ("semantic-search-enabled", "DCC_MCP_SEMANTIC_SEARCH_ENABLED"),
        ] {
            command.arg(format!("--{flag}={value}"));
            command.env(key, if enabled { "0" } else { "1" });
        }
        // Keep all subprocess output bounded even if readiness never arrives.
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(30), command.output())
            .await
            .expect("restart must finish within 30 seconds")
            .unwrap();
        assert!(
            output.status.success(),
            "restart failed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let base = format!("http://127.0.0.1:{port}");
        let ready: Value = client
            .get(format!("{base}/v1/readyz"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(ready["gateway_lifecycle"]["persist"], enabled);
        assert_eq!(ready["gateway_lifecycle"]["idle_timeout_secs"], 30);

        let pid = gateway.pid().expect("daemon must write its pidfile");
        assert_ne!(pid.as_u32(), stale_pid);
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::everything(),
        );
        let process = system
            .process(pid)
            .expect("replacement daemon must be alive");
        for flag in ["--no-admin", "--semantic-search-enabled"] {
            assert!(
                process
                    .cmd()
                    .windows(2)
                    .any(|pair| pair[0] == flag && pair[1] == value.as_str()),
                "replacement argv must preserve {flag}={value}: {:?}",
                process.cmd(),
            );
        }
        #[cfg(feature = "admin")]
        {
            // Axum nests `/` at the exact prefix, without a trailing slash.
            let admin = client.get(format!("{base}/admin")).send().await.unwrap();
            assert_eq!(
                admin.status(),
                if enabled {
                    reqwest::StatusCode::NOT_FOUND
                } else {
                    reqwest::StatusCode::OK
                },
            );
        }
        // Wait for SQLite/log handles to close before removing the temporary
        // directory, especially on Windows. Drop is the panic-path fallback.
        gateway.stop().expect("test daemon must exit cleanly");
        drop(gateway);
    }
}
