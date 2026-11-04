use super::*;
use serde_json::json;

#[test]
fn canonical_view_keeps_errors_metrics_artifacts_and_non_mirror_content() {
    let payload = json!({"success": false, "error": "host_error", "context": {
        "cached": true, "fracture": true, "float": false,
        "metrics": [{"frame": 1, "error": "TypeError"}],
        "artifacts": [{"path": "metrics.json", "sha256": "digest"}]
    }});
    let annotated =
        json!({"type":"text", "text":payload.to_string(), "annotations":{"audience":["user"]}});
    let mut value = json!({"success":false,"request_id":"r1","result":{
        "structuredContent":payload, "isError":true, "_meta":{"trace":"keep"},
        "content":[{"type":"text","text":payload.to_string()},
            {"type":"text","text":"different diagnostic"},
            {"type":"image","data":"image-data","mimeType":"image/png"},annotated]
    }});
    let raw = value.clone();
    canonical_result(&mut value);
    assert_eq!(value["result"]["structuredContent"], payload);
    assert_eq!(value["result"]["isError"], true);
    assert_eq!(value["result"]["_meta"], raw["result"]["_meta"]);
    assert_eq!(
        value["result"]["content"],
        json!([
        {"type":"text","text":"different diagnostic"},
        {"type":"image","data":"image-data","mimeType":"image/png"},annotated])
    );
    assert_eq!(raw["result"]["content"].as_array().unwrap().len(), 4);
}

#[test]
fn text_only_unknown_and_distinct_results_remain_lossless() {
    for mut value in [
        json!({"result":{"content":[{"type":"text","text":"{\"value\":1}"}]}}),
        json!({"result":{"structuredContent":{"value":2},"content":[{"type":"text","text":"{\"value\":1}"}]}}),
        json!({"result":{"structuredContent":null,"content":[{"type":"text","text":"bad JSON"}]}}),
        json!({"instances":[],"gateway":{}}),
    ] {
        let original = value.clone();
        canonical_result(&mut value);
        assert_eq!(value, original);
    }
}

#[test]
fn batch_views_keep_step_status_and_opaque_user_data() {
    let payload = json!({"result":{"content":[{"type":"text","text":"user data"}]}});
    let mut value = json!({"ok":false,"results":[{"id":"failed-step","ok":false,
        "result":{"isError":true,"structuredContent":payload,
            "content":[{"type":"text","text":payload.to_string()}]}}]});
    canonical_result(&mut value);
    assert_eq!(value["results"][0]["result"]["structuredContent"], payload);
    assert_eq!(value["results"][0]["result"]["content"], json!([]));
    assert_eq!(value["results"][0]["result"]["isError"], true);
    assert_eq!(value["results"][0]["ok"], false);
    assert_eq!(value["results"][0]["id"], "failed-step");
}

#[test]
fn acceptance_states_are_preserved_without_interpreting_default_false() {
    for (state, verified) in [("not_evaluated", false), ("pass", true), ("fail", false)] {
        let payload = json!({"context":{"flotation_verified":verified,
            "flotation_status":state,"cached":true,"fracture_verified":true,
            "metrics":(1..=96).map(|frame| json!({"frame":frame})).collect::<Vec<_>>()}});
        let mut value = json!({"success":true,"result":{"structuredContent":payload,
            "content":[{"type":"text","text":payload.to_string()}]}});
        canonical_result(&mut value);
        assert_eq!(value["result"]["structuredContent"], payload);
        assert_eq!(
            value["result"]["structuredContent"]["context"]["flotation_status"],
            state
        );
    }
    let mut value = json!({"result":{"structuredContent":{"flotation_verified":false},
        "content":[{"type":"text","text":"{\"flotation_verified\":false}"}]}});
    canonical_result(&mut value);
    assert_eq!(
        value["result"]["structuredContent"],
        json!({"flotation_verified":false})
    );
}

#[test]
fn pointer_selection_is_explicit_and_keeps_call_identity_and_failure() {
    let value = json!({"success":false,"request_id":"r1","tool_slug":"tool",
        "result":{"structuredContent":{"float":false,"a/b":{"~key":[42]}}}});
    let pointers = vec![
        "/result/structuredContent/float".into(),
        "/result/structuredContent/a~1b/~0key/0".into(),
        "/missing".into(),
    ];
    let selected = select_fields(value, &pointers);
    assert_eq!(selected["success"], false);
    assert_eq!(selected["request_id"], "r1");
    assert_eq!(selected["selected"][&pointers[0]], false);
    assert_eq!(selected["selected"][&pointers[1]], 42);
    assert_eq!(selected["selected"][&pointers[2]], Value::Null);
    for pointer in ["", "/", "/a~0b", "/a~1b/0"] {
        assert!(parse_pointer(pointer).is_ok());
    }
    for pointer in ["field", "/a~", "/a~2"] {
        assert!(parse_pointer(pointer).is_err());
    }
}
