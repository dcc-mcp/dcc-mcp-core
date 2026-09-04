use std::collections::BTreeMap;
use std::path::Path;

use dcc_mcp_models::DccName;
use serde::Serialize;
use serde_json::Value;

use super::{BUNDLED_CATALOG, InstallError, InstallService};
use crate::domain::install::normalized_dcc_key;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DccTypesCatalog {
    pub total: usize,
    pub dcc_types: Vec<DccTypeSummary>,
    pub custom_types_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DccTypeSummary {
    pub dcc_type: String,
    pub adapters: Vec<DccAdapterSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DccAdapterSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub catalog_install_available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Presence {
    Present,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CatalogPresence {
    Present,
    Absent,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PackageInstallation {
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AdapterImport {
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProjectBootstrap {
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RegistryRegistration {
    Present,
    Absent,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DirectReadiness {
    Ready,
    NotReady,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ExactInstanceCall {
    NotRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RealHostEffect {
    NotVerified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Uncertainty {
    Version,
    CustomFork,
    RealHost,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DiscoveryNextAction {
    id: String,
    command: Vec<String>,
    requires_consent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DccDiscoveryDecision {
    schema_version: u8,
    dcc_type: String,
    live_instances: Option<usize>,
    public_adapter: Presence,
    released_catalog: CatalogPresence,
    package_installation: PackageInstallation,
    adapter_import: AdapterImport,
    project_bootstrap: ProjectBootstrap,
    registry_registration: RegistryRegistration,
    direct_readiness: DirectReadiness,
    gateway_capability_index: Presence,
    search_hit: Presence,
    exact_instance_call: ExactInstanceCall,
    real_host_effect: RealHostEffect,
    uncertainties: Vec<Uncertainty>,
    failure_stage: Option<String>,
    failure_reason: Option<String>,
    next_action: DiscoveryNextAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeObservation {
    live_instances: usize,
    registry_registration: RegistryRegistration,
    direct_readiness: DirectReadiness,
}

impl InstallService {
    /// List adapter-backed DCC types from the bundled or explicitly supplied catalog.
    pub fn dcc_types(&self, catalog_path: Option<&Path>) -> Result<DccTypesCatalog, InstallError> {
        let entries = if let Some(path) = catalog_path {
            self.load_entries(Some(path))?
        } else {
            dcc_mcp_catalog::load_from_str(BUNDLED_CATALOG)?
        };
        let mut grouped: BTreeMap<String, BTreeMap<String, DccAdapterSummary>> = BTreeMap::new();
        let mut canonical_by_normalized: BTreeMap<String, String> = BTreeMap::new();

        for entry in entries.iter().filter(|entry| {
            entry
                .tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case("adapter"))
        }) {
            let adapter = DccAdapterSummary {
                name: entry.name.clone(),
                version: entry.version.clone(),
                url: entry.url.clone(),
                catalog_install_available: entry.install.is_some(),
            };
            for dcc_type in &entry.dcc {
                let parsed = DccName::parse(dcc_type).to_string();
                let normalized = normalized_dcc_key(&parsed);
                if normalized.is_empty() {
                    continue;
                }
                let canonical = canonical_by_normalized
                    .entry(normalized)
                    .or_insert_with(|| parsed.clone())
                    .clone();
                grouped
                    .entry(canonical)
                    .or_default()
                    .insert(adapter.name.clone(), adapter.clone());
            }
        }

        let dcc_types = grouped
            .into_iter()
            .map(|(dcc_type, adapters)| DccTypeSummary {
                dcc_type,
                adapters: adapters.into_values().collect(),
            })
            .collect::<Vec<_>>();

        Ok(DccTypesCatalog {
            total: dcc_types.len(),
            dcc_types,
            custom_types_supported: true,
        })
    }

    pub(crate) fn discovery_decision(
        &self,
        catalog_path: Option<&std::path::Path>,
        requested_dcc_type: &str,
        inventory: Option<&Value>,
    ) -> DccDiscoveryDecision {
        let Some(canonical_dcc) = validated_dcc_type(requested_dcc_type) else {
            return failure_decision(
                "unknown",
                Presence::Unknown,
                CatalogPresence::Unknown,
                None,
                "input_validation",
                "INVALID_DCC_TYPE",
                inspect_catalog_action(),
            );
        };
        let requested_key = normalized_dcc_key(&canonical_dcc);
        let runtime = inventory.map(|value| observe_runtime(value, &requested_key));

        let entries = if let Some(path) = catalog_path {
            self.load_entries(Some(path))
        } else {
            dcc_mcp_catalog::load_from_str(BUNDLED_CATALOG).map_err(Into::into)
        };
        let entries = match entries {
            Ok(entries) => entries,
            Err(_) => {
                return failure_decision(
                    &canonical_dcc,
                    Presence::Unknown,
                    CatalogPresence::Unknown,
                    runtime,
                    "catalog_load",
                    "CATALOG_LOAD_FAILED",
                    inspect_catalog_action(),
                );
            }
        };
        let adapter = entries.iter().find(|entry| {
            entry
                .tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case("adapter"))
                && entry.dcc.iter().any(|dcc| {
                    let parsed = DccName::parse(dcc).to_string();
                    normalized_dcc_key(&parsed) == requested_key
                })
        });
        let public_adapter = if catalog_path.is_none() && adapter.is_some() {
            Presence::Present
        } else {
            Presence::Unknown
        };
        let released_catalog = if catalog_path.is_some() {
            CatalogPresence::Unknown
        } else if adapter.is_some() {
            CatalogPresence::Present
        } else {
            CatalogPresence::Absent
        };
        let Some(runtime) = runtime else {
            return failure_decision(
                &canonical_dcc,
                public_adapter,
                released_catalog,
                None,
                "registry_read",
                "REGISTRY_READ_FAILED",
                doctor_action(),
            );
        };
        let live_instances = runtime.live_instances;
        let direct_readiness = runtime.direct_readiness;
        let instructions_url = if catalog_path.is_none() {
            adapter
                .and_then(|entry| entry.install.as_ref())
                .and_then(|install| install.instructions_url.clone())
        } else {
            None
        };
        let next_action = if direct_readiness == DirectReadiness::Ready {
            search_action(&canonical_dcc)
        } else if live_instances > 0 {
            wait_ready_action(&canonical_dcc)
        } else if catalog_path.is_none()
            && adapter.and_then(|entry| entry.install.as_ref()).is_some()
        {
            install_plan_action(&canonical_dcc, instructions_url)
        } else {
            inspect_catalog_action()
        };
        let (failure_stage, failure_reason) = if adapter.is_none() {
            (
                Some("catalog_lookup".to_string()),
                Some("CATALOG_ENTRY_NOT_FOUND".to_string()),
            )
        } else if direct_readiness == DirectReadiness::NotReady {
            (
                Some("direct_readiness".to_string()),
                Some("INSTANCE_NOT_READY".to_string()),
            )
        } else {
            (None, None)
        };

        DccDiscoveryDecision {
            schema_version: 1,
            dcc_type: canonical_dcc,
            live_instances: Some(live_instances),
            public_adapter,
            released_catalog,
            package_installation: PackageInstallation::Unknown,
            adapter_import: AdapterImport::Unknown,
            project_bootstrap: ProjectBootstrap::Unknown,
            registry_registration: runtime.registry_registration,
            direct_readiness,
            gateway_capability_index: Presence::Unknown,
            search_hit: Presence::Unknown,
            exact_instance_call: ExactInstanceCall::NotRun,
            real_host_effect: RealHostEffect::NotVerified,
            uncertainties: vec![
                Uncertainty::Version,
                Uncertainty::CustomFork,
                Uncertainty::RealHost,
            ],
            failure_stage,
            failure_reason,
            next_action,
        }
    }
}

fn validated_dcc_type(requested: &str) -> Option<String> {
    if requested.trim() != requested
        || requested.is_empty()
        || requested.chars().count() > 64
        || requested
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':'))
    {
        return None;
    }

    let canonical = DccName::parse(requested).to_string();
    (!canonical.is_empty() && canonical.chars().count() <= 64).then_some(canonical)
}

fn observe_runtime(inventory: &Value, requested_key: &str) -> RuntimeObservation {
    let matching_instances = inventory
        .get("instances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|instance| {
            instance
                .get("dcc_type")
                .and_then(Value::as_str)
                .map(|dcc| {
                    let parsed = DccName::parse(dcc).to_string();
                    normalized_dcc_key(&parsed) == requested_key
                })
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let live_instances = matching_instances.len();
    let ready_instances = matching_instances
        .iter()
        .filter(|instance| {
            instance
                .pointer("/direct_control/ready")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();

    RuntimeObservation {
        live_instances,
        registry_registration: if live_instances > 0 {
            RegistryRegistration::Present
        } else {
            RegistryRegistration::Absent
        },
        direct_readiness: if live_instances == 0 {
            DirectReadiness::Unknown
        } else if ready_instances > 0 {
            DirectReadiness::Ready
        } else {
            DirectReadiness::NotReady
        },
    }
}

fn failure_decision(
    dcc_type: &str,
    public_adapter: Presence,
    released_catalog: CatalogPresence,
    runtime: Option<RuntimeObservation>,
    failure_stage: &str,
    failure_reason: &str,
    next_action: DiscoveryNextAction,
) -> DccDiscoveryDecision {
    DccDiscoveryDecision {
        schema_version: 1,
        dcc_type: dcc_type.to_string(),
        live_instances: runtime.map(|observation| observation.live_instances),
        public_adapter,
        released_catalog,
        package_installation: PackageInstallation::Unknown,
        adapter_import: AdapterImport::Unknown,
        project_bootstrap: ProjectBootstrap::Unknown,
        registry_registration: runtime
            .map(|observation| observation.registry_registration)
            .unwrap_or(RegistryRegistration::Unknown),
        direct_readiness: runtime
            .map(|observation| observation.direct_readiness)
            .unwrap_or(DirectReadiness::Unknown),
        gateway_capability_index: Presence::Unknown,
        search_hit: Presence::Unknown,
        exact_instance_call: ExactInstanceCall::NotRun,
        real_host_effect: RealHostEffect::NotVerified,
        uncertainties: vec![
            Uncertainty::Version,
            Uncertainty::CustomFork,
            Uncertainty::RealHost,
        ],
        failure_stage: Some(failure_stage.to_string()),
        failure_reason: Some(failure_reason.to_string()),
        next_action,
    }
}

fn install_plan_action(dcc_type: &str, instructions_url: Option<String>) -> DiscoveryNextAction {
    DiscoveryNextAction {
        id: "plan_install".to_string(),
        command: vec![
            "dcc-mcp-cli".to_string(),
            "--output".to_string(),
            "json".to_string(),
            "--non-interactive".to_string(),
            "install".to_string(),
            "--dcc-type".to_string(),
            dcc_type.to_string(),
        ],
        requires_consent: false,
        instructions_url,
    }
}

fn search_action(dcc_type: &str) -> DiscoveryNextAction {
    DiscoveryNextAction {
        id: "search_capabilities".to_string(),
        command: vec![
            "dcc-mcp-cli".to_string(),
            "--output".to_string(),
            "json".to_string(),
            "search".to_string(),
            "--dcc-type".to_string(),
            dcc_type.to_string(),
        ],
        requires_consent: false,
        instructions_url: None,
    }
}

fn wait_ready_action(dcc_type: &str) -> DiscoveryNextAction {
    DiscoveryNextAction {
        id: "wait_ready".to_string(),
        command: vec![
            "dcc-mcp-cli".to_string(),
            "--output".to_string(),
            "json".to_string(),
            "wait-ready".to_string(),
            "--dcc-type".to_string(),
            dcc_type.to_string(),
        ],
        requires_consent: false,
        instructions_url: None,
    }
}

fn inspect_catalog_action() -> DiscoveryNextAction {
    DiscoveryNextAction {
        id: "inspect_catalog".to_string(),
        command: vec![
            "dcc-mcp-cli".to_string(),
            "--output".to_string(),
            "json".to_string(),
            "--non-interactive".to_string(),
            "dcc-types".to_string(),
        ],
        requires_consent: false,
        instructions_url: None,
    }
}

fn doctor_action() -> DiscoveryNextAction {
    DiscoveryNextAction {
        id: "inspect_registry".to_string(),
        command: vec![
            "dcc-mcp-cli".to_string(),
            "--output".to_string(),
            "json".to_string(),
            "doctor".to_string(),
        ],
        requires_consent: false,
        instructions_url: None,
    }
}
