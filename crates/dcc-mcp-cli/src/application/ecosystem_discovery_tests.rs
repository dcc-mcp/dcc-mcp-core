use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct InjectedProvider {
    id: &'static str,
    calls: AtomicUsize,
    network_calls: AtomicUsize,
    fail: bool,
}

impl DiscoveryProvider for InjectedProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    fn observe(
        &self,
        offline: bool,
        _now: u64,
    ) -> Pin<Box<dyn Future<Output = CatalogObservation> + Send + '_>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !offline {
                self.network_calls.fetch_add(1, Ordering::SeqCst);
            }
            if self.fail {
                return unavailable(self.id, "https://example.invalid", "network_error");
            }
            let mut result = unavailable(self.id, "https://example.invalid", "unused");
            result.source.state = "available";
            result.source.reason = None;
            result.entries = serde_json::from_value(serde_json::json!([{
                "name": "just-added-tool", "description": "Dynamic provider entry", "targets": [{"kind":"web","id":"new-web-tool"}]
            }])).unwrap();
            result
        })
    }
}

fn provider(id: &'static str, fail: bool) -> InjectedProvider {
    InjectedProvider {
        id,
        calls: AtomicUsize::new(0),
        network_calls: AtomicUsize::new(0),
        fail,
    }
}

fn request(offline: bool) -> DiscoveryRequest {
    DiscoveryRequest {
        offline,
        query: String::new(),
        target: None,
        provider: None,
        limit: 50,
    }
}

#[tokio::test]
async fn provider_seam_preserves_dynamic_entries_and_partial_failure() {
    let available = provider("future_official_provider", false);
    let failed = provider("unreachable_official_provider", true);
    let report = discover_with_providers(&[&available, &failed], request(false), 1000).await;
    assert_eq!(report.status, "partial");
    assert_eq!(report.components[0].identity.name, "just-added-tool");
    assert_eq!(
        report.components[0].identity.provider_id,
        "future_official_provider"
    );
    assert_eq!(
        report.components[0].targets[0].to_string(),
        "web:new-web-tool"
    );
    assert_eq!(failed.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn provider_selection_skips_other_providers_and_passes_offline() {
    let selected = provider(INSTALL_PROVIDER, false);
    let excluded = provider(MARKETPLACE_PROVIDER, true);
    let mut options = request(true);
    options.provider = Some(INSTALL_PROVIDER.into());
    let report = discover_with_providers(&[&selected, &excluded], options, 1000).await;
    assert_eq!(report.status, "complete");
    assert_eq!(selected.calls.load(Ordering::SeqCst), 1);
    assert_eq!(selected.network_calls.load(Ordering::SeqCst), 0);
    assert_eq!(excluded.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn official_marketplace_offline_is_explicitly_unavailable_without_a_fetch() {
    let snapshot = OfficialMarketplace.observe(true, 1000).await;
    assert_eq!(
        snapshot.source.reason,
        Some("offline_no_verified_marketplace_snapshot")
    );
    assert!(!snapshot.source.latest_checked);
    assert!(snapshot.entries.is_empty());
}
