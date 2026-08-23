#[cfg(test)]
const WECOM_WEBHOOK_PATH: &str = "/cgi-bin/webhook/send";

pub(super) use dcc_mcp_gateway_admin::{
    WECOM_WEBHOOK_URL_HINT, strict_wecom_webhook_url_looks_valid as strict_looks_valid,
};

pub(super) fn looks_valid(value: &str) -> bool {
    strict_looks_valid(value) || {
        #[cfg(test)]
        {
            test_looks_valid(value)
        }
        #[cfg(not(test))]
        {
            false
        }
    }
}

#[cfg(test)]
fn test_looks_valid(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "http"
            && url.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost") || {
                    host.parse::<std::net::IpAddr>()
                        .is_ok_and(|addr| addr.is_loopback())
                }
            })
            && url.fragment().is_none()
            && has_robot_shape(&url)
    })
}

#[cfg(test)]
fn has_robot_shape(url: &reqwest::Url) -> bool {
    url.path() == WECOM_WEBHOOK_PATH
        && url.query_pairs().any(|(key, value)| {
            key == "key" && !value.trim().is_empty() && value.as_ref() != "********"
        })
}
