//! WeCom webhook validation and public-safe response projection.

use axum::http::StatusCode;
use serde_json::{Map, Value};

const WECOM_WEBHOOK_HOST: &str = "qyapi.weixin.qq.com";
const WECOM_WEBHOOK_PATH: &str = "/cgi-bin/webhook/send";

/// Public configuration hint without a real webhook credential.
pub const WECOM_WEBHOOK_URL_HINT: &str = "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=...";

/// Return whether `value` is a production WeCom robot webhook URL.
#[must_use]
pub fn strict_wecom_webhook_url_looks_valid(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host.eq_ignore_ascii_case(WECOM_WEBHOOK_HOST))
            && matches!(url.port(), None | Some(443))
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && has_robot_shape(&url)
    })
}

fn has_robot_shape(url: &reqwest::Url) -> bool {
    url.path() == WECOM_WEBHOOK_PATH
        && url.query_pairs().any(|(key, value)| {
            key == "key" && !value.trim().is_empty() && value.as_ref() != "********"
        })
}

/// Reduce a WeCom response to the fields safe for admin API clients.
pub fn summarize_wecom_response(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_url_validation_requires_the_wecom_robot_endpoint() {
        assert!(strict_wecom_webhook_url_looks_valid(
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123"
        ));
        assert!(strict_wecom_webhook_url_looks_valid(
            "https://qyapi.weixin.qq.com:443/cgi-bin/webhook/send?key=abc123"
        ));
        for value in [
            "http://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123",
            "https://example.com/cgi-bin/webhook/send?key=abc123",
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send",
            "https://qyapi.weixin.qq.com/other?key=abc123",
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=********",
        ] {
            assert!(!strict_wecom_webhook_url_looks_valid(value), "{value}");
        }
    }

    #[test]
    fn response_summary_does_not_echo_unknown_fields() {
        let (errcode, errmsg, summary) = summarize_wecom_response(
            r#"{"errcode":93000,"errmsg":"invalid webhook","secret_echo":"leak"}"#,
            StatusCode::OK,
        );

        assert_eq!(errcode, Some(93000));
        assert_eq!(errmsg, "invalid webhook");
        assert_eq!(summary["errcode"], 93000);
        assert_eq!(summary["errmsg"], "invalid webhook");
        assert!(summary.get("secret_echo").is_none());
    }

    #[test]
    fn response_summary_falls_back_to_http_status() {
        let (_, success, _) = summarize_wecom_response("not json", StatusCode::OK);
        let (_, failure, _) = summarize_wecom_response("not json", StatusCode::BAD_GATEWAY);
        assert_eq!(success, "ok");
        assert_eq!(failure, "failed");
    }
}
