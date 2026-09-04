use std::path::Path;

use serde_json::Value;

use super::to_json;
use crate::application::gateway_ensure;
use crate::application::install::InstallService;
use crate::application::local_registry::list_local_instances;

pub(super) fn run(catalog: Option<&Path>, dcc_type: Option<&str>) -> anyhow::Result<Value> {
    let service = InstallService::bundled();
    if let Some(dcc_type) = dcc_type {
        let inventory = list_local_instances(gateway_ensure::default_registry_dir()).ok();
        to_json(service.discovery_decision(catalog, dcc_type, inventory.as_ref()))
    } else {
        to_json(service.dcc_types(catalog)?)
    }
}
