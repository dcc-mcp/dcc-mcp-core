//! Agent-readable experiment and governance projections.

use serde_json::{Value, json};

use super::super::admin::experiments::{experiment_detail_payload, experiment_list_payload};
use super::super::admin::governance::build_governance_payload;
use super::super::admin::state::AdminState;
use super::super::state::GatewayState;
use super::util::{parse_query, split_uri};

pub const EXPERIMENTS_URI: &str = "gateway://experiments";
pub const EXPERIMENTS_PREFIX: &str = "gateway://experiments/";
pub const GOVERNANCE_URI: &str = "gateway://governance";

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1_000;

pub fn pointers() -> [Value; 2] {
    [
        json!({
            "uri": EXPERIMENTS_URI,
            "name": "Reproducible experiments",
            "description": "Read experiment runs, Session DAG links, metrics, and evidence-only judge results. Use gateway://experiments/{experiment_id} for detail.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": GOVERNANCE_URI,
            "name": "Effective gateway governance",
            "description": "Read the effective policy, traffic capture, redaction, quota, and recent enforcement decisions.",
            "mimeType": "application/json"
        }),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    Experiments { limit: usize },
    Experiment { experiment_id: String },
    Governance { limit: usize },
}

pub fn parse(uri: &str) -> Option<Query> {
    if let Some(rest) = uri.strip_prefix(EXPERIMENTS_PREFIX) {
        let experiment_id = rest.split('?').next().unwrap_or_default().trim();
        if !experiment_id.is_empty() {
            return Some(Query::Experiment {
                experiment_id: experiment_id.to_owned(),
            });
        }
    }

    let (path, query) = split_uri(uri);
    let limit = query
        .map(parse_query)
        .and_then(|params| {
            params
                .get("limit")
                .and_then(|value| value.parse::<usize>().ok())
        })
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    match path {
        EXPERIMENTS_URI => Some(Query::Experiments { limit }),
        GOVERNANCE_URI => Some(Query::Governance { limit }),
        _ => None,
    }
}

pub async fn build_payload(gs: &GatewayState, query: &Query) -> Result<Value, String> {
    let state = admin_state(gs);
    match query {
        Query::Experiments { limit } => experiment_list_payload(&state, *limit),
        Query::Experiment { experiment_id } => experiment_detail_payload(&state, experiment_id),
        Query::Governance { limit } => return Ok(build_governance_payload(&state, *limit).await),
    }
    .map_err(|error| error.message().to_owned())
}

fn admin_state(gs: &GatewayState) -> AdminState {
    let state = AdminState::new(gs.clone());
    #[cfg(feature = "admin-persist-sqlite")]
    {
        state.with_admin_sqlite_lane(gs.admin_sqlite_lane.clone())
    }
    #[cfg(not(feature = "admin-persist-sqlite"))]
    {
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_observability_resources() {
        assert_eq!(
            parse("gateway://experiments?limit=25"),
            Some(Query::Experiments { limit: 25 })
        );
        assert_eq!(
            parse("gateway://experiments/exp-42"),
            Some(Query::Experiment {
                experiment_id: "exp-42".into()
            })
        );
        assert_eq!(
            parse("gateway://governance?limit=50"),
            Some(Query::Governance { limit: 50 })
        );
        assert!(parse("gateway://memory").is_none());
    }
}
