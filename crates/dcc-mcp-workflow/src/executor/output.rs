use super::*;

/// Turn a raw tool output into a [`StepOutput`], persisting any inline
/// artefacts into the configured store when present.
pub fn ingest_output(state: &RunState, step_id: &StepId, mut output: Value) -> StepOutput {
    // Promote `file_refs` from raw output to artefact store when possible.
    // We consult `output.file_refs` and `output.context.file_refs`; any
    // entry that has `inline_bytes` (base64) is re-put into the store.
    if let Some(store) = &state.artefacts {
        maybe_upload_inline_refs(store.as_ref(), &mut output, state.root_job_id);
    }
    let mut step_out = StepOutput::from_value(output);
    // Ensure each FileRef picks up the producer_job_id for downstream filters.
    for fr in step_out.file_refs.iter_mut() {
        if fr.producer_job_id.is_none() {
            fr.producer_job_id = Some(state.root_job_id);
        }
    }
    let _ = step_id; // reserved for future step-level artefact tagging
    step_out
}

/// Scan `output` for inline artefact entries (`inline_b64` or `path`) and upload them to `store`.
pub fn maybe_upload_inline_refs(
    store: &dyn dcc_mcp_artefact::ArtefactStore,
    output: &mut Value,
    producer_job: Uuid,
) {
    let upload_one = |entry: &mut Value| {
        if let Some(obj) = entry.as_object_mut() {
            // If the entry already has a `uri`, leave it alone.
            if obj.get("uri").and_then(Value::as_str).is_some() {
                return;
            }
            if let Some(b64) = obj.get("inline_b64").and_then(Value::as_str) {
                use base64::Engine as _;
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                    match store.put(ArtefactBody::Inline(bytes)) {
                        Ok(mut fr) => {
                            if fr.producer_job_id.is_none() {
                                fr.producer_job_id = Some(producer_job);
                            }
                            if let Ok(v) = serde_json::to_value(&fr) {
                                *entry = v;
                            }
                        }
                        Err(e) => warn!(error = %e, "inline artefact upload failed"),
                    }
                }
            } else if let Some(path) = obj.get("path").and_then(Value::as_str) {
                let p = std::path::PathBuf::from(path);
                match store.put(ArtefactBody::Path(p)) {
                    Ok(mut fr) => {
                        if fr.producer_job_id.is_none() {
                            fr.producer_job_id = Some(producer_job);
                        }
                        if let Ok(v) = serde_json::to_value(&fr) {
                            *entry = v;
                        }
                    }
                    Err(e) => warn!(error = %e, "path artefact upload failed"),
                }
            }
        }
    };

    let uploaders = |arr_key: &str, root: &mut Value| {
        if let Some(arr) = root.get_mut(arr_key).and_then(Value::as_array_mut) {
            for entry in arr.iter_mut() {
                upload_one(entry);
            }
        }
    };

    uploaders("file_refs", output);
    if let Some(ctx) = output.get_mut("context")
        && let Some(arr) = ctx.get_mut("file_refs").and_then(Value::as_array_mut)
    {
        for entry in arr.iter_mut() {
            upload_one(entry);
        }
    }
    // Squash unused warning when `root_job_id` not needed.
    let _ = producer_job;
}
