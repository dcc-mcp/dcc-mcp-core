use axum::http::StatusCode;
use serde_json::{Map, Value};

pub(super) fn summarize(
    response_text: &str,
    http_status: StatusCode,
) -> (Option<i64>, String, Value) {
    let parsed = serde_json::from_str::<Value>(response_text).ok();
    let errcode = parsed
        .as_ref()
        .and_then(|value| value.get("errcode"))
        .and_then(Value::as_i64);
    let errmsg = parsed
        .as_ref()
        .and_then(|value| value.get("errmsg"))
        .and_then(Value::as_str)
        .unwrap_or(if http_status.is_success() {
            "ok"
        } else {
            "failed"
        })
        .to_string();

    let mut summary = Map::new();
    if let Some(code) = errcode {
        summary.insert("errcode".into(), Value::from(code));
    }
    summary.insert("errmsg".into(), Value::String(errmsg.clone()));
    (errcode, errmsg, Value::Object(summary))
}
