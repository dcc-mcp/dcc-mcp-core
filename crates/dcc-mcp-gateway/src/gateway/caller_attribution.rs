//! Server-derived caller-attribution helpers for gateway ingress.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use serde_json::Value;
use tokio::sync::RwLock;

use super::resilience::gateway_limits;
use crate::gateway::admin::trace::AgentContext;

pub(crate) const INTERNAL_SOURCE_IP_HEADER: &str = "x-dcc-mcp-internal-source-ip";
pub(crate) const INTERNAL_FORWARDED_FOR_HEADER: &str = "x-dcc-mcp-internal-forwarded-for";
pub(crate) const INTERNAL_AUTH_SUBJECT_HEADER: &str = "x-dcc-mcp-internal-auth-subject";
const MAX_MCP_CLIENT_SESSIONS: usize = 512;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ClientNetworkAttribution {
    pub(crate) source_ip: Option<String>,
    pub(crate) forwarded_for: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ClientAttributionStore {
    mcp_sessions: RwLock<ClientAttributionSessions>,
}

#[derive(Debug, Default)]
struct ClientAttributionSessions {
    order: VecDeque<String>,
    sessions: HashMap<String, AgentContext>,
}

impl ClientAttributionStore {
    pub(crate) async fn record_mcp_initialize(&self, session_id: &str, params: Option<&Value>) {
        let Some(context) = AgentContext::from_mcp_client_info(session_id, params) else {
            return;
        };
        let mut sessions = self.mcp_sessions.write().await;
        if !sessions.sessions.contains_key(session_id) {
            sessions.order.push_back(session_id.to_string());
        }
        sessions.sessions.insert(session_id.to_string(), context);
        while sessions.sessions.len() > MAX_MCP_CLIENT_SESSIONS {
            let Some(oldest) = sessions.order.pop_front() else {
                break;
            };
            sessions.sessions.remove(&oldest);
        }
    }

    pub(crate) async fn augment_mcp_context(
        &self,
        session_id: &str,
        mut context: Option<AgentContext>,
    ) -> Option<AgentContext> {
        let session_context = {
            let sessions = self.mcp_sessions.read().await;
            sessions.sessions.get(session_id).cloned()
        };
        let Some(session_context) = session_context else {
            return context;
        };
        match context.as_mut() {
            Some(ctx) => ctx.merge_missing_client_identity_from(&session_context),
            None => context = Some(session_context),
        }
        context
    }
}

/// Attach server-derived network attribution for downstream handlers.
///
/// These internal headers are overwritten at the gateway boundary so external
/// clients cannot set `source_ip` or `forwarded_for` through ordinary request
/// metadata. Handlers convert them into `AgentContext` server fields.
pub(crate) async fn caller_attribution_middleware(mut req: Request<Body>, next: Next) -> Response {
    {
        let headers = req.headers_mut();
        headers.remove(INTERNAL_SOURCE_IP_HEADER);
        headers.remove(INTERNAL_FORWARDED_FOR_HEADER);
        headers.remove(INTERNAL_AUTH_SUBJECT_HEADER);
    }

    let addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| *addr)
        .unwrap_or_else(loopback_socket_addr);
    let attribution = derive_client_network_attribution(&addr, req.headers());
    if let Some(source_ip) = attribution.source_ip
        && let Ok(value) = HeaderValue::from_str(&source_ip)
    {
        req.headers_mut().insert(INTERNAL_SOURCE_IP_HEADER, value);
    }
    if !attribution.forwarded_for.is_empty() {
        let value = attribution.forwarded_for.join(", ");
        if let Ok(value) = HeaderValue::from_str(&value) {
            req.headers_mut()
                .insert(INTERNAL_FORWARDED_FOR_HEADER, value);
        }
    }

    next.run(req).await
}

fn loopback_socket_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}

#[must_use]
pub(crate) fn derive_client_network_attribution(
    connect: &SocketAddr,
    headers: &HeaderMap,
) -> ClientNetworkAttribution {
    derive_client_network_attribution_with_depth(
        connect,
        headers,
        gateway_limits().xff_trusted_depth as usize,
    )
}

#[must_use]
pub(crate) fn derive_client_network_attribution_with_depth(
    connect: &SocketAddr,
    headers: &HeaderMap,
    trusted_depth: usize,
) -> ClientNetworkAttribution {
    let forwarded_for = forwarded_for_chain(headers);
    let source = effective_client_ip_from_chain(connect.ip(), &forwarded_for, trusted_depth);
    let forwarded_for = if trusted_depth > 0 {
        forwarded_for
    } else {
        Vec::new()
    };
    ClientNetworkAttribution {
        source_ip: Some(source.to_string()),
        forwarded_for,
    }
}

#[must_use]
pub(crate) fn effective_client_ip(connect: &SocketAddr, headers: &HeaderMap) -> IpAddr {
    derive_client_network_attribution(connect, headers)
        .source_ip
        .and_then(|value| value.parse::<IpAddr>().ok())
        .unwrap_or_else(|| connect.ip())
}

#[must_use]
pub(crate) fn internal_network_attribution(headers: &HeaderMap) -> ClientNetworkAttribution {
    ClientNetworkAttribution {
        source_ip: header_str(headers, INTERNAL_SOURCE_IP_HEADER),
        forwarded_for: headers
            .get(INTERNAL_FORWARDED_FOR_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(parse_ip_list)
            .unwrap_or_default(),
    }
}

fn effective_client_ip_from_chain(
    peer_ip: IpAddr,
    forwarded_for: &[String],
    trusted_depth: usize,
) -> IpAddr {
    if trusted_depth == 0 || forwarded_for.len() <= trusted_depth {
        return peer_ip;
    }
    let idx = forwarded_for.len() - 1 - trusted_depth;
    forwarded_for[idx].parse().unwrap_or(peer_ip)
}

fn forwarded_for_chain(headers: &HeaderMap) -> Vec<String> {
    if let Some(raw) = header_str(headers, "x-forwarded-for") {
        let parsed = parse_ip_list(&raw);
        if !parsed.is_empty() {
            return parsed;
        }
    }
    headers
        .get("forwarded")
        .and_then(|value| value.to_str().ok())
        .map(parse_forwarded_header)
        .unwrap_or_default()
}

fn parse_ip_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .filter_map(|segment| normalise_forwarded_for_value(segment.trim()))
        .collect()
}

fn parse_forwarded_header(raw: &str) -> Vec<String> {
    raw.split(',')
        .filter_map(|entry| {
            entry.split(';').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                key.trim()
                    .eq_ignore_ascii_case("for")
                    .then(|| normalise_forwarded_for_value(value.trim()))
                    .flatten()
            })
        })
        .collect()
}

fn normalise_forwarded_for_value(raw: &str) -> Option<String> {
    let value = raw.trim_matches('"').trim();
    if value.is_empty() || value.eq_ignore_ascii_case("unknown") {
        return None;
    }
    let value = if let Some(rest) = value.strip_prefix('[') {
        rest.split_once(']').map(|(ip, _)| ip).unwrap_or(rest)
    } else if let Some((host, _port)) = value.rsplit_once(':') {
        if host.parse::<IpAddr>().is_ok() {
            host
        } else {
            value
        }
    } else {
        value
    };
    value.parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> SocketAddr {
        SocketAddr::new("10.0.0.10".parse().unwrap(), 1234)
    }

    #[test]
    fn source_ip_ignores_forwarded_headers_without_trusted_depth() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "198.51.100.1, 10.0.0.20".parse().unwrap(),
        );

        let attribution = derive_client_network_attribution_with_depth(&peer(), &headers, 0);

        assert_eq!(attribution.source_ip.as_deref(), Some("10.0.0.10"));
        assert!(attribution.forwarded_for.is_empty());
    }

    #[test]
    fn source_ip_uses_xff_left_of_trusted_proxy_hops() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "198.51.100.1, 203.0.113.9, 10.0.0.20".parse().unwrap(),
        );

        let attribution = derive_client_network_attribution_with_depth(&peer(), &headers, 1);

        assert_eq!(attribution.source_ip.as_deref(), Some("203.0.113.9"));
        assert_eq!(
            attribution.forwarded_for,
            vec![
                "198.51.100.1".to_string(),
                "203.0.113.9".to_string(),
                "10.0.0.20".to_string()
            ]
        );
    }

    #[test]
    fn source_ip_can_use_forwarded_header_when_xff_is_absent() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            "for=198.51.100.7;proto=https, for=\"[2001:db8::1]\""
                .parse()
                .unwrap(),
        );

        let attribution = derive_client_network_attribution_with_depth(&peer(), &headers, 1);

        assert_eq!(attribution.source_ip.as_deref(), Some("198.51.100.7"));
        assert_eq!(
            attribution.forwarded_for,
            vec!["198.51.100.7".to_string(), "2001:db8::1".to_string()]
        );
    }
}
