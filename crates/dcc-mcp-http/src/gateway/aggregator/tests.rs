use super::*;

#[test]
fn skill_management_tool_defs_cover_all_six_tools() {
    let defs = skill_management_tool_defs();
    let names: Vec<&str> = defs
        .iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
        .collect();
    for expected in [
        "list_skills",
        "find_skills",
        "search_skills",
        "get_skill_info",
        "load_skill",
        "unload_skill",
    ] {
        assert!(names.contains(&expected), "missing tool def {expected}");
    }
    assert_eq!(defs.len(), 6, "expected exactly 6 skill-management tools");
}

#[test]
fn skill_management_tool_defs_all_declare_input_schema() {
    for def in skill_management_tool_defs() {
        let schema = def.get("inputSchema").expect("inputSchema present");
        assert_eq!(
            schema.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "schema for {} is not an object",
            def.get("name").unwrap()
        );
    }
}

#[test]
fn inject_instance_metadata_adds_annotations_to_object() {
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    let mut value = json!({"existing": "field"});
    inject_instance_metadata(&mut value, &id, "maya");

    let obj = value.as_object().unwrap();
    assert_eq!(obj.get("existing").unwrap(), &json!("field"));
    assert_eq!(obj.get("_instance_id").unwrap(), &json!(id.to_string()));
    assert_eq!(obj.get("_instance_short").unwrap(), &json!("abcdef01"));
    assert_eq!(obj.get("_dcc_type").unwrap(), &json!("maya"));
}

#[test]
fn inject_instance_metadata_is_noop_for_non_objects() {
    let id = Uuid::new_v4();
    // Arrays and scalars cannot receive annotations — the helper must
    // silently skip them rather than panic.
    let mut arr = json!([1, 2, 3]);
    inject_instance_metadata(&mut arr, &id, "blender");
    assert_eq!(arr, json!([1, 2, 3]));

    let mut s = json!("scalar");
    inject_instance_metadata(&mut s, &id, "blender");
    assert_eq!(s, json!("scalar"));
}

#[test]
fn to_text_result_maps_ok_to_success() {
    let (text, is_error) = to_text_result(Ok("payload".to_string()));
    assert_eq!(text, "payload");
    assert!(!is_error);
}

#[test]
fn to_text_result_maps_err_to_error() {
    let (text, is_error) = to_text_result(Err("boom".to_string()));
    assert_eq!(text, "boom");
    assert!(is_error);
}

// ── #320: extract_job_id covers both sync (None) and async (#318) envelopes.

#[test]
fn extract_job_id_reads_structured_content_first() {
    let v = json!({
        "content": [],
        "structuredContent": {"job_id": "job-42", "status": "pending"},
        "isError": false,
    });
    assert_eq!(extract_job_id(&v).as_deref(), Some("job-42"));
}

#[test]
fn extract_job_id_falls_back_to_meta_dcc_jobid() {
    let v = json!({
        "content": [],
        "_meta": {"dcc": {"jobId": "job-99", "parentJobId": null}},
        "isError": false,
    });
    assert_eq!(extract_job_id(&v).as_deref(), Some("job-99"));
}

#[test]
fn extract_job_id_returns_none_for_sync_reply() {
    let v = json!({"content": [{"type": "text", "text": "ok"}], "isError": false});
    assert!(extract_job_id(&v).is_none());
}

// ── #321: async opt-in detection + envelope merging ────────────────

#[test]
fn meta_signals_async_dispatch_picks_up_async_flag() {
    let meta = json!({"dcc": {"async": true}});
    assert!(meta_signals_async_dispatch(Some(&meta)));
}

#[test]
fn meta_signals_async_dispatch_picks_up_progress_token() {
    let meta = json!({"progressToken": "tok"});
    assert!(meta_signals_async_dispatch(Some(&meta)));
}

#[test]
fn meta_signals_async_dispatch_is_false_for_sync_requests() {
    assert!(!meta_signals_async_dispatch(None));
    let meta = json!({"dcc": {"parentJobId": "abc"}});
    assert!(!meta_signals_async_dispatch(Some(&meta)));
}

#[test]
fn meta_wants_wait_for_terminal_reads_dcc_flag() {
    let meta = json!({"dcc": {"async": true, "wait_for_terminal": true}});
    assert!(meta_wants_wait_for_terminal(Some(&meta)));

    let meta = json!({"dcc": {"async": true}});
    assert!(!meta_wants_wait_for_terminal(Some(&meta)));
}

#[test]
fn strip_gateway_meta_flags_removes_wait_for_terminal_only() {
    let meta = json!({"dcc": {"async": true, "wait_for_terminal": true, "parentJobId": "p"}});
    let stripped = strip_gateway_meta_flags(meta);
    assert_eq!(stripped["dcc"]["async"], true);
    assert_eq!(stripped["dcc"]["parentJobId"], "p");
    assert!(stripped["dcc"].get("wait_for_terminal").is_none());
}

#[test]
fn merge_job_update_into_envelope_completed_sets_status_and_result() {
    let pending = json!({
        "content": [{"type": "text", "text": "Job x queued"}],
        "structuredContent": {"job_id": "x", "status": "pending", "_meta": {"dcc": {"jobId": "x"}}},
        "isError": false,
    });
    let update = json!({
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "x", "status": "completed", "result": {"rows": 42}}
    });
    let merged = merge_job_update_into_envelope(pending, &update, false);
    assert_eq!(merged["structuredContent"]["status"], "completed");
    assert_eq!(merged["structuredContent"]["result"]["rows"], 42);
    assert_eq!(
        merged["structuredContent"]["_meta"]["dcc"]["status"],
        "completed"
    );
    assert_eq!(merged["isError"], false);
}

#[test]
fn merge_job_update_into_envelope_failed_marks_is_error() {
    let pending = json!({
        "content": [{"type": "text", "text": "Job x queued"}],
        "structuredContent": {"job_id": "x", "status": "pending"},
        "isError": false,
    });
    let update = json!({
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "x", "status": "failed", "error": "boom"}
    });
    let merged = merge_job_update_into_envelope(pending, &update, false);
    assert_eq!(merged["structuredContent"]["status"], "failed");
    assert_eq!(merged["structuredContent"]["error"], "boom");
    assert_eq!(merged["isError"], true);
}

#[test]
fn merge_job_update_into_envelope_timeout_sets_timed_out_flag() {
    let pending = json!({
        "content": [{"type": "text", "text": "Job x queued"}],
        "structuredContent": {"job_id": "x", "status": "pending"},
        "isError": false,
    });
    let merged = merge_job_update_into_envelope(pending, &Value::Null, true);
    assert_eq!(
        merged["structuredContent"]["_meta"]["dcc"]["timed_out"],
        true
    );
    assert_eq!(merged["isError"], true);
}
