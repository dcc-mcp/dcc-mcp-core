//! Backend-neutral worker projections for the admin dashboard.

use serde_json::{Value, json};

/// Health bucket used by the worker summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerHealth {
    Live,
    Stale,
    Unhealthy,
}

/// Gateway-independent input for one admin worker card.
#[derive(Debug, Clone)]
pub struct WorkerSnapshot {
    pub instance_id: String,
    pub dcc_type: String,
    pub display_name: Option<String>,
    pub pid: Option<u32>,
    pub host_pid: Option<u32>,
    pub mcp_url: String,
    pub host: String,
    pub port: u16,
    pub status: String,
    pub stale: bool,
    pub uptime_secs: u64,
    pub registered_at_unix: Option<u64>,
    pub last_heartbeat_unix: Option<u64>,
    pub version: Option<String>,
    pub server_version: Option<String>,
    pub adapter_version: Option<String>,
    pub instance_type: String,
    pub adapter_dcc: Option<String>,
    pub scene: Option<String>,
    pub documents: Vec<String>,
    pub loaded_skills: Vec<String>,
    pub loaded_skill_count: usize,
    pub action_count: usize,
    pub performance: Value,
    pub failure_reason: Option<String>,
    pub failure_stage: Option<String>,
    pub dispatch_status: Option<String>,
    pub dispatch_ready: bool,
    pub dispatch_ready_at_unix: Option<String>,
    pub host_rpc_uri: Option<String>,
    pub host_rpc_scheme: Option<String>,
    pub gateway_runtime_mode: Option<String>,
    pub gateway_guardian_enabled: bool,
    pub gateway_recovery_driver: String,
    pub registration_refresh_mode: String,
    pub metadata: Value,
    pub cpu_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
    pub virtual_memory_bytes: Option<u64>,
    pub backend_context_error: Option<String>,
}

impl WorkerSnapshot {
    fn into_json(self) -> Value {
        json!({
            "instance_id": self.instance_id,
            "dcc_type": self.dcc_type,
            "display_name": self.display_name,
            "pid": self.pid,
            "host_pid": self.host_pid,
            "mcp_url": self.mcp_url,
            "host": self.host,
            "port": self.port,
            "status": self.status,
            "stale": self.stale,
            "uptime_secs": self.uptime_secs,
            "registered_at_unix": self.registered_at_unix,
            "last_heartbeat_unix": self.last_heartbeat_unix,
            "version": self.version,
            "server_version": self.server_version,
            "adapter_version": self.adapter_version,
            "instance_type": self.instance_type,
            "adapter_dcc": self.adapter_dcc,
            "scene": self.scene,
            "documents": self.documents,
            "loaded_skills": self.loaded_skills,
            "loaded_skill_count": self.loaded_skill_count,
            "action_count": self.action_count,
            "performance": self.performance,
            "failure_reason": self.failure_reason,
            "failure_stage": self.failure_stage,
            "dispatch_status": self.dispatch_status,
            "dispatch_ready": self.dispatch_ready,
            "dispatch_ready_at_unix": self.dispatch_ready_at_unix,
            "host_rpc_uri": self.host_rpc_uri,
            "host_rpc_scheme": self.host_rpc_scheme,
            "gateway_runtime_mode": self.gateway_runtime_mode,
            "gateway_guardian_enabled": self.gateway_guardian_enabled,
            "gateway_recovery_driver": self.gateway_recovery_driver,
            "registration_refresh_mode": self.registration_refresh_mode,
            "metadata": self.metadata,
            "cpu_percent": self.cpu_percent,
            "memory_bytes": self.memory_bytes,
            "virtual_memory_bytes": self.virtual_memory_bytes,
            "backend_context_error": self.backend_context_error,
        })
    }
}

/// Project worker snapshots into the stable admin API response.
#[must_use]
pub fn workers_payload(workers: Vec<(WorkerSnapshot, WorkerHealth)>) -> Value {
    let mut live = 0usize;
    let mut stale = 0usize;
    let mut unhealthy = 0usize;
    let workers: Vec<Value> = workers
        .into_iter()
        .map(|(worker, health)| {
            match health {
                WorkerHealth::Live => live += 1,
                WorkerHealth::Stale => stale += 1,
                WorkerHealth::Unhealthy => unhealthy += 1,
            }
            worker.into_json()
        })
        .collect();

    json!({
        "total": workers.len(),
        "summary": {
            "live": live,
            "stale": stale,
            "unhealthy": unhealthy,
        },
        "workers": workers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(instance_id: &str, status: &str, stale: bool) -> WorkerSnapshot {
        WorkerSnapshot {
            instance_id: instance_id.to_string(),
            dcc_type: "maya".to_string(),
            display_name: Some("Maya 2026".to_string()),
            pid: Some(42),
            host_pid: Some(41),
            mcp_url: "http://127.0.0.1:18812/mcp".to_string(),
            host: "127.0.0.1".to_string(),
            port: 18812,
            status: status.to_string(),
            stale,
            uptime_secs: 30,
            registered_at_unix: Some(10),
            last_heartbeat_unix: Some(40),
            version: Some("0.20.8".to_string()),
            server_version: Some("0.20.8".to_string()),
            adapter_version: Some("0.8.0".to_string()),
            instance_type: "gui".to_string(),
            adapter_dcc: Some("maya".to_string()),
            scene: Some("turntable.ma".to_string()),
            documents: vec!["turntable.ma".to_string()],
            loaded_skills: vec!["maya-modeling".to_string()],
            loaded_skill_count: 1,
            action_count: 3,
            performance: json!({"process": {"cpu_percent": 2.5}}),
            failure_reason: None,
            failure_stage: None,
            dispatch_status: Some("ready".to_string()),
            dispatch_ready: true,
            dispatch_ready_at_unix: Some("40".to_string()),
            host_rpc_uri: Some("commandport://maya".to_string()),
            host_rpc_scheme: Some("commandport".to_string()),
            gateway_runtime_mode: Some("daemon".to_string()),
            gateway_guardian_enabled: true,
            gateway_recovery_driver: "daemon_guardian".to_string(),
            registration_refresh_mode: "file_registry_heartbeat".to_string(),
            metadata: json!({"dispatch_status": "ready"}),
            cpu_percent: Some(2.5),
            memory_bytes: Some(1024),
            virtual_memory_bytes: Some(2048),
            backend_context_error: None,
        }
    }

    #[test]
    fn preserves_worker_contract_and_counts_health_buckets() {
        let payload = workers_payload(vec![
            (worker("live", "available", false), WorkerHealth::Live),
            (worker("stale", "stale", true), WorkerHealth::Stale),
            (worker("booting", "booting", false), WorkerHealth::Unhealthy),
        ]);

        assert_eq!(payload["total"], 3);
        assert_eq!(
            payload["summary"],
            json!({"live": 1, "stale": 1, "unhealthy": 1})
        );
        assert_eq!(payload["workers"][0]["scene"], "turntable.ma");
        assert_eq!(payload["workers"][0]["dispatch_ready"], true);
        assert_eq!(payload["workers"][1]["stale"], true);
        assert_eq!(payload["workers"][2]["status"], "booting");
    }
}
