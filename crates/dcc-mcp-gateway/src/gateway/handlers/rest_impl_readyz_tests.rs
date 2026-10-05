//! `GET /v1/readyz` — the readiness counters must not stand alone (#4209).

use super::rest_impl_tests::{response_json, test_gateway_state};
use super::*;

#[tokio::test]
async fn gateway_readyz_reports_why_an_instance_is_not_ready() {
    // `live=1 / ready=0` with no per-instance reason forced operators to
    // reverse-engineer the probe. The reason must ride along in the same
    // payload as the counters.
    let gs = test_gateway_state("1.2.3");
    let mut entry = ServiceEntry::new("blender", "127.0.0.1", 18777);
    entry.status = ServiceStatus::Available;
    let instance_id = entry.instance_id;
    {
        let registry = &gs.registry;
        registry.register(entry).unwrap();
    }
    gs.instance_diagnostics.record_probe_failure(
        instance_id,
        crate::gateway::backend_client::probe::ProbeFailure {
            kind: "no-readiness-surface".to_string(),
            message: "http://127.0.0.1:18777/v1/readyz answered HTTP 404".to_string(),
            probed_url: "http://127.0.0.1:18777/v1/readyz".to_string(),
        },
    );

    let (status, body) = response_json(handle_v1_readyz(State(gs)).await.into_response()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["live_instance_count"], 1);
    assert_eq!(body["ready_instance_count"], 0);
    assert_eq!(body["not_ready_instance_count"], 1);

    let instance = &body["instances"][0];
    assert_eq!(
        instance["probe_failure"]["kind"], "no-readiness-surface",
        "the counter alone is not enough — the reason must be readable: {body:#}"
    );
    assert_eq!(
        instance["probe_failure"]["probed_url"],
        "http://127.0.0.1:18777/v1/readyz"
    );
    assert!(
        instance["probe_failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("404")),
        "the message must carry the HTTP status: {body:#}"
    );
}
