use std::time::Duration;

use dcc_mcp_skill_rest::ReadinessReport;

use super::urls::{health_url_from_mcp_url, readyz_url_from_mcp_url};

/// Outcome of the gateway's three-state readiness probe (#713).
///
/// * [`Ready`] — `/v1/readyz` answered `200` with all three bits
///   green, or a pre-#660 backend answered `/health`.
///   Safe to forward `tools/call`.
/// * [`Booting`] — `/v1/readyz` answered (typically `503`) with at
///   least one bit red. The process is alive, just not done
///   initialising — keep the registry row, but do **not** route
///   traffic to it.
/// * [`Unreachable`] — Neither `/v1/readyz` nor `/health` answered.
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
    /// Backend answered neither `/v1/readyz` nor `/health`.
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

/// Three-state probe of a backend's `/v1/readyz` surface (#713 / #660).
///
/// Returns a [`ReadinessReport`] when the backend answered `/v1/readyz`
/// with a parseable JSON body (on either `200` or `503`), and `None`
/// when the REST surface is absent — callers should then fall back to
/// the legacy `/health` check.
pub(crate) async fn probe_readiness(
    client: &reqwest::Client,
    mcp_url: &str,
    timeout: Duration,
) -> Option<ReadinessReport> {
    let url = readyz_url_from_mcp_url(mcp_url);
    let resp = client
        .get(&url)
        .timeout(timeout)
        .header("accept", "application/json")
        .send()
        .await
        .ok()?;

    // `/v1/readyz` returns 200 when all three bits are green and 503 when
    // any bit is red — in **both** cases the body is a full
    // `ReadinessReport` (see `dcc-mcp-skill-rest/src/router.rs::handle_readyz`).
    // Any other status (404, 500 without body, …) means "no readiness
    // surface", not "backend is red".
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 503 {
        return None;
    }
    resp.json::<ReadinessReport>().await.ok()
}

/// Classify a backend as [`Ready`] / [`Booting`] / [`Unreachable`] using
/// the three-state probe introduced in #713.
///
/// Order of checks:
/// 1. `GET /v1/readyz` — if the backend answered (200 *or* 503 with a
///    parseable body) we trust it:
///    * `is_ready() == true`  ⇒ [`Ready`]
///    * `is_ready() == false` ⇒ [`Booting`]
/// 2. Otherwise fall back to `GET /health` for pre-#660 backends that
///    never mounted the REST surface:
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
    if let Some(report) = probe_readiness(client, mcp_url, timeout).await {
        return if report.is_ready() {
            ProbeOutcome::Ready
        } else {
            ProbeOutcome::Booting
        };
    }

    let health_url = health_url_from_mcp_url(mcp_url);
    let ok = client
        .get(&health_url)
        .timeout(timeout)
        .header("accept", "application/json")
        .send()
        .await
        .is_ok_and(|resp| resp.status().is_success());
    if ok {
        ProbeOutcome::Ready
    } else {
        ProbeOutcome::Unreachable
    }
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
/// falling back to `/health` only when the readiness surface is missing.
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
