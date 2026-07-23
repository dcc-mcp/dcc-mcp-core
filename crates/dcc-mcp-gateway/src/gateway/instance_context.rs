//! Live context for one or more gateway instances.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Value, json};
use sysinfo::{Pid, ProcessesToUpdate, System};
use uuid::Uuid;

use super::http_registration::entry_mcp_url;
use super::state::GatewayState;
use dcc_mcp_transport::discovery::types::ServiceEntry;

#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessMetrics {
    pub cpu_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
    pub virtual_memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MachineMetrics {
    pub cpu_percent: f32,
    pub total_memory_bytes: u64,
    pub used_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub total_swap_bytes: u64,
    pub used_swap_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct InstanceContext {
    pub scene: Option<String>,
    pub documents: Vec<String>,
    pub loaded_skills: Vec<String>,
    pub action_count: usize,
    pub process: ProcessMetrics,
    pub machine: MachineMetrics,
    pub backend_context_error: Option<String>,
}

pub(crate) async fn collect(
    gs: &GatewayState,
    entries: &[ServiceEntry],
) -> HashMap<Uuid, InstanceContext> {
    let pids = entries.iter().filter_map(|entry| entry.pid).collect();
    let metrics = tokio::task::spawn_blocking(move || sample_metrics(pids));
    let backend_contexts = futures::future::join_all(
        entries
            .iter()
            .cloned()
            .map(|entry| fetch_backend_context(gs, entry)),
    );
    let (metrics, backend_contexts) = tokio::join!(metrics, backend_contexts);
    let (process_metrics, machine_metrics) = metrics.unwrap_or_default();

    let records = gs.capability_index.snapshot().records;
    let mut loaded_skills: HashMap<Uuid, BTreeSet<String>> = HashMap::new();
    let mut action_counts: HashMap<Uuid, usize> = HashMap::new();
    for record in records.iter().filter(|record| record.loaded) {
        *action_counts.entry(record.instance_id).or_default() += 1;
        if let Some(skill) = record.skill_name.as_ref() {
            loaded_skills
                .entry(record.instance_id)
                .or_default()
                .insert(skill.clone());
        }
    }

    backend_contexts
        .into_iter()
        .map(|(entry, backend)| {
            let mut context = InstanceContext {
                scene: entry.scene.clone(),
                documents: entry.documents.clone(),
                loaded_skills: loaded_skills
                    .remove(&entry.instance_id)
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
                action_count: action_counts.remove(&entry.instance_id).unwrap_or_default(),
                process: entry
                    .pid
                    .and_then(|pid| process_metrics.get(&pid).cloned())
                    .unwrap_or_default(),
                machine: machine_metrics.clone(),
                backend_context_error: None,
            };
            match backend {
                Ok(value) => {
                    context.scene = value
                        .get("scene")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .or(context.scene);
                    if let Some(documents) = value.get("documents").and_then(Value::as_array) {
                        context.documents = documents
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect();
                    }
                }
                Err(error) => context.backend_context_error = Some(error),
            }
            (entry.instance_id, context)
        })
        .collect()
}

pub(crate) async fn build_payload(gs: &GatewayState, entry: ServiceEntry) -> Value {
    let context = collect(gs, std::slice::from_ref(&entry))
        .await
        .remove(&entry.instance_id)
        .unwrap_or_default();
    let resource_uri = format!("gateway://instances/{}", entry.instance_id);
    let mut payload = gs.instance_json(&entry);
    payload["scene"] = json!(context.scene);
    payload["documents"] = json!(context.documents);
    payload["performance"] = performance_json(&context);
    payload["skills"] = skills_json(&context);
    payload["routes"] = json!({
        "resource_uri": resource_uri,
        "context_url": format!("/v1/instances/{}/context", entry.instance_id),
        "call_url": format!(
            "/v1/dcc/{}/instances/{}/call",
            entry.dcc_type, entry.instance_id
        ),
    });
    payload["agent_hint"] = json!(format!(
        "Use {resource_uri} as the target DCC instance context for subsequent discovery and calls."
    ));
    payload["backend_context_error"] = json!(context.backend_context_error);
    payload
}

pub(crate) fn performance_json(context: &InstanceContext) -> Value {
    json!({
        "process": {
            "cpu_percent": context.process.cpu_percent,
            "memory_bytes": context.process.memory_bytes,
            "virtual_memory_bytes": context.process.virtual_memory_bytes,
        },
        "machine": {
            "cpu_percent": context.machine.cpu_percent,
            "total_memory_bytes": context.machine.total_memory_bytes,
            "used_memory_bytes": context.machine.used_memory_bytes,
            "available_memory_bytes": context.machine.available_memory_bytes,
            "total_swap_bytes": context.machine.total_swap_bytes,
            "used_swap_bytes": context.machine.used_swap_bytes,
        },
    })
}

pub(crate) fn skills_json(context: &InstanceContext) -> Value {
    json!({
        "loaded": context.loaded_skills,
        "loaded_count": context.loaded_skills.len(),
        "action_count": context.action_count,
    })
}

async fn fetch_backend_context(
    gs: &GatewayState,
    entry: ServiceEntry,
) -> (ServiceEntry, Result<Value, String>) {
    let result = async {
        let mut url = reqwest::Url::parse(&entry_mcp_url(&entry)).map_err(|e| e.to_string())?;
        let path = url.path().trim_end_matches('/');
        let base = path.strip_suffix("/mcp").unwrap_or(path);
        url.set_path(&format!("{base}/v1/context"));
        url.set_query(None);
        let response = gs
            .http_client
            .get(url)
            .timeout(gs.backend_timeout.min(std::time::Duration::from_secs(2)))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        response.json::<Value>().await.map_err(|e| e.to_string())
    }
    .await;
    (entry, result)
}

fn sample_metrics(pids: Vec<u32>) -> (HashMap<u32, ProcessMetrics>, MachineMetrics) {
    let sysinfo_pids: Vec<Pid> = pids.iter().copied().map(Pid::from_u32).collect();
    let mut system = System::new_all();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    system.refresh_cpu_usage();
    system.refresh_memory();
    system.refresh_processes(ProcessesToUpdate::Some(&sysinfo_pids), true);

    let processes = pids
        .into_iter()
        .filter_map(|pid| {
            system.process(Pid::from_u32(pid)).map(|process| {
                (
                    pid,
                    ProcessMetrics {
                        cpu_percent: Some(process.cpu_usage()),
                        memory_bytes: Some(process.memory()),
                        virtual_memory_bytes: Some(process.virtual_memory()),
                    },
                )
            })
        })
        .collect();
    let machine = MachineMetrics {
        cpu_percent: system.global_cpu_usage(),
        total_memory_bytes: system.total_memory(),
        used_memory_bytes: system.used_memory(),
        available_memory_bytes: system.available_memory(),
        total_swap_bytes: system.total_swap(),
        used_swap_bytes: system.used_swap(),
    };
    (processes, machine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_reports_current_process_and_machine_memory() {
        let (processes, machine) = sample_metrics(vec![std::process::id()]);
        let current = processes.get(&std::process::id()).unwrap();
        assert!(current.memory_bytes.unwrap_or_default() > 0);
        assert!(machine.total_memory_bytes >= machine.available_memory_bytes);
    }
}
