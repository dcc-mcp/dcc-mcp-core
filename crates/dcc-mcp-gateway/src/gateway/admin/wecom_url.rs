const WECOM_WEBHOOK_HOST: &str = "qyapi.weixin.qq.com";
const WECOM_WEBHOOK_PATH: &str = "/cgi-bin/webhook/send";

pub(super) const WECOM_WEBHOOK_URL_HINT: &str =
    "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=...";

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

pub(super) fn strict_looks_valid(value: &str) -> bool {
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

fn has_robot_shape(url: &reqwest::Url) -> bool {
    url.path() == WECOM_WEBHOOK_PATH
        && url.query_pairs().any(|(key, value)| {
            key == "key" && !value.trim().is_empty() && value.as_ref() != "********"
        })
}
