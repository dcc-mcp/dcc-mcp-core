use std::time::Duration;

use dcc_mcp_skill_rest::ReadinessReport;

use super::urls::{health_url_from_mcp_url, healthz_url_from_mcp_url, readyz_url_from_mcp_url};

/// Outcome of the gateway's readiness probe (#713).
///
/// * [`Ready`] — `/v1/readyz` answered `200` with base routing bits
///   green, or a pre-#660 backend answered `/health` or `/healthz`.
///   Safe to forward `tools/call`.
/// * [`Booting`] — `/v1/readyz` answered (typically `503`) with at
///   least one bit red. The process is alive, just not done
///   initialising — keep the registry row, but do **not** route
///   traffic to it.
/// * [`Unreachable`] — Neither `/v1/readyz` nor a legacy health
///   endpoint answered.
///   Eligible for the existing stale-cleanup pipeline.
///
/// [`Ready`]: ProbeOutcome::Ready
/// [`Booting`]: ProbeOutcome::Booting
/// [`Unreachable`]: ProbeOutcome::Unreachable
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeOutcome {
    /// Backend is fully ready.
    Ready,
    /// Backend is alive but some readiness bit is red (still booting).
    Booting,
    /// Backend answered neither `/v1/readyz` nor a legacy health endpoint.
    Unreachable,
}

impl ProbeOutcome {
    /// True when the backend may service `tools/call` right now.
    pub(crate) fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// True when the backend process is alive (ready or booting).
    ///
    /// Callers use this to keep a registry row instead of marking it
    /// [`ServiceStatus::Unreachable`](dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable).
    pub(crate) fn is_alive(self) -> bool {
        matches!(self, Self::Ready | Self::Booting)
    }
}

/// Probe a backend's `/v1/readyz` readiness surface (#713 / #660).
///
/// Returns a [`ReadinessReport`] when the backend answered `/v1/readyz`
/// with a parseable JSON body (on either `200` or `503`), and `None`
/// when the REST surface is absent — callers should then fall back to
/// the legacy `/health` / `/healthz` checks.
pub(crate) async fn probe_readiness(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Option<ReadinessReport> {
    let url = readyz_url_from_mcp_url(mcp_url);
    let resp = client
        .get(&url)
        .timeout(timeout)
        .header("accept", "application/json, text/event-stream")
        .send()
        .await
        .ok()?;

    // `/v1/readyz` returns 200 when base routing bits are green and 503
    // when any base bit is red — in **both** cases the body is a full
    // `ReadinessReport` (see `dcc-mcp-skill-rest/src/router.rs::handle_readyz`).
    // Any other status (404, 500 without body, …) means "no readiness
    // surface", not "backend is red".
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 503 {
        return None;
    }
    resp.json::<ReadinessReport>().await.ok()
}

/// Map a parsed `/v1/readyz` body to a [`ProbeOutcome`] without another HTTP hop.
#[must_use]
pub(crate) fn probe_outcome_from_report(report: &ReadinessReport) -> ProbeOutcome {
    if report.is_ready() {
        ProbeOutcome::Ready
    } else {
        ProbeOutcome::Booting
    }
}

/// Classify liveness with at most one `/v1/readyz` request.
///
/// When readyz is present, both the cached [`ReadinessReport`] and the
/// [`ProbeOutcome`] are derived from that single response. Legacy backends
/// without readyz fall back to legacy health endpoints only.
pub(crate) async fn probe_mcp_readiness_once(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> (Option<ReadinessReport>, ProbeOutcome) {
    if let Some(report) = probe_readiness(client, mcp_url, timeout).await {
        let outcome = probe_outcome_from_report(&report);
        return (Some(report), outcome);
    }

    let ok = legacy_health_ok(client, &health_url_from_mcp_url(mcp_url), timeout).await
        || legacy_health_ok(client, &healthz_url_from_mcp_url(mcp_url), timeout).await;
    let outcome = if ok {
        ProbeOutcome::Ready
    } else {
        ProbeOutcome::Unreachable
    };
    (None, outcome)
}

async fn legacy_health_ok(client: &reqwest::Client, url: &str, timeout: Duration) -> bool {
    client
        .get(url)
        .timeout(timeout)
        .header("accept", "application/json, text/event-stream")
        .send()
        .await
        .is_ok_and(|resp| resp.status().is_success())
}

/// Classify a backend as [`Ready`] / [`Booting`] / [`Unreachable`] using
/// the readiness probe introduced in #713.
///
/// Order of checks:
/// 1. `GET /v1/readyz` — if the backend answered (200 *or* 503 with a
///    parseable body) we trust it:
///    * `is_ready() == true`  ⇒ [`Ready`]
///    * `is_ready() == false` ⇒ [`Booting`]
/// 2. Otherwise fall back to `GET /health`, then `GET /healthz`, for
///    pre-#660 backends that never mounted the REST surface:
///    * `200 OK`  ⇒ [`Ready`]
///    * otherwise ⇒ [`Unreachable`]
///
/// [`Ready`]: ProbeOutcome::Ready
/// [`Booting`]: ProbeOutcome::Booting
/// [`Unreachable`]: ProbeOutcome::Unreachable
pub(crate) async fn probe_mcp_readiness(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> ProbeOutcome {
    probe_mcp_readiness_once(client, mcp_url, timeout).await.1
}

/// Return true when the target looks like a DCC MCP HTTP server.
///
/// This is the legacy boolean wrapper kept for callers that only need a
/// live/dead classification — notably [`call_backend`] below. #713 gave
/// us three states; prefer [`probe_mcp_readiness`] in new code so
/// "alive but booting" can be distinguished from "gone".
///
/// Behaviour change under #713: the underlying check first tries
/// `/v1/readyz` and treats a non-ready (`503`) report as *not* healthy,
/// falling back to legacy health endpoints only when the readiness surface
/// is missing.
/// A backend whose host DCC is still initialising now reports `false`
/// instead of silently routing traffic.
#[cfg(test)]
pub(crate) async fn probe_mcp_health(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> bool {
    probe_mcp_readiness(client, mcp_url, timeout)
        .await
        .is_ready()
}
