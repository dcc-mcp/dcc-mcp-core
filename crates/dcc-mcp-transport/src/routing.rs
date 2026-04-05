//! Instance routing — smart selection of DCC instances for multi-instance environments.
//!
//! When multiple instances of the same DCC type are running (e.g. 3 Maya instances
//! working on different shots), the router decides which instance to send a request to.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::discovery::types::{ServiceEntry, ServiceStatus};
use crate::error::{TransportError, TransportResult};

/// Strategy for selecting a DCC instance when multiple are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoutingStrategy {
    /// Route to the first available (healthy) instance. Default behavior.
    #[default]
    FirstAvailable,
    /// Distribute requests evenly across instances (round-robin).
    RoundRobin,
    /// Route to the instance with the fewest active requests.
    LeastBusy,
    /// Route to a specific instance identified by hint (instance_id, scene name, etc.).
    Specific,
    /// Route to the instance whose scene matches the given hint.
    SceneMatch,
    /// Route to a random available instance.
    Random,
}

impl std::fmt::Display for RoutingStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FirstAvailable => write!(f, "first_available"),
            Self::RoundRobin => write!(f, "round_robin"),
            Self::LeastBusy => write!(f, "least_busy"),
            Self::Specific => write!(f, "specific"),
            Self::SceneMatch => write!(f, "scene_match"),
            Self::Random => write!(f, "random"),
        }
    }
}

/// Instance router that selects DCC instances based on a routing strategy.
///
/// Thread-safe: uses atomic counters for round-robin state.
pub struct InstanceRouter {
    /// Default routing strategy.
    default_strategy: RoutingStrategy,
    /// Round-robin counter per DCC type.
    round_robin_counters: Arc<dashmap::DashMap<String, AtomicUsize>>,
}

impl Default for InstanceRouter {
    fn default() -> Self {
        Self::new(RoutingStrategy::FirstAvailable)
    }
}

impl InstanceRouter {
    /// Create a new router with the given default strategy.
    pub fn new(default_strategy: RoutingStrategy) -> Self {
        Self {
            default_strategy,
            round_robin_counters: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Get the default routing strategy.
    pub fn default_strategy(&self) -> RoutingStrategy {
        self.default_strategy
    }

    /// Set the default routing strategy.
    pub fn set_default_strategy(&mut self, strategy: RoutingStrategy) {
        self.default_strategy = strategy;
    }

    /// Select an instance from the given list using the specified strategy and hint.
    ///
    /// # Arguments
    /// * `instances` — All registered instances for a DCC type.
    /// * `strategy` — Which routing strategy to use (or `None` for default).
    /// * `hint` — Optional hint string (instance_id for Specific, scene name for SceneMatch).
    pub fn select(
        &self,
        instances: &[ServiceEntry],
        strategy: Option<RoutingStrategy>,
        hint: Option<&str>,
    ) -> TransportResult<ServiceEntry> {
        let strategy = strategy.unwrap_or(self.default_strategy);

        // Filter to healthy (Available) instances for most strategies
        let available: Vec<&ServiceEntry> = instances
            .iter()
            .filter(|e| e.status == ServiceStatus::Available)
            .collect();

        match strategy {
            RoutingStrategy::FirstAvailable => self.select_first_available(&available),
            RoutingStrategy::RoundRobin => self.select_round_robin(&available),
            RoutingStrategy::LeastBusy => self.select_least_busy(instances),
            RoutingStrategy::Specific => self.select_specific(instances, hint),
            RoutingStrategy::SceneMatch => self.select_scene_match(&available, hint),
            RoutingStrategy::Random => self.select_random(&available),
        }
    }

    /// First available: return the first healthy instance.
    fn select_first_available(&self, available: &[&ServiceEntry]) -> TransportResult<ServiceEntry> {
        available
            .first()
            .cloned()
            .cloned()
            .ok_or_else(|| TransportError::ServiceNotFound {
                dcc_type: "unknown".to_string(),
                instance_id: "any".to_string(),
            })
    }

    /// Round-robin: distribute across available instances.
    fn select_round_robin(&self, available: &[&ServiceEntry]) -> TransportResult<ServiceEntry> {
        if available.is_empty() {
            return Err(TransportError::ServiceNotFound {
                dcc_type: "unknown".to_string(),
                instance_id: "any (round_robin)".to_string(),
            });
        }

        let dcc_type = &available[0].dcc_type;
        let counter = self
            .round_robin_counters
            .entry(dcc_type.clone())
            .or_insert_with(|| AtomicUsize::new(0));
        let idx = counter.fetch_add(1, Ordering::Relaxed) % available.len();
        Ok(available[idx].clone())
    }

    /// Least busy: prefer Available over Busy, skip Unreachable/ShuttingDown.
    fn select_least_busy(&self, instances: &[ServiceEntry]) -> TransportResult<ServiceEntry> {
        // First try Available instances
        let available: Vec<&ServiceEntry> = instances
            .iter()
            .filter(|e| e.status == ServiceStatus::Available)
            .collect();

        if let Some(entry) = available.first() {
            return Ok((*entry).clone());
        }

        // Fall back to Busy instances (they're at least alive)
        instances
            .iter()
            .find(|e| e.status == ServiceStatus::Busy)
            .cloned()
            .ok_or_else(|| TransportError::ServiceNotFound {
                dcc_type: instances
                    .first()
                    .map(|e| e.dcc_type.clone())
                    .unwrap_or_default(),
                instance_id: "any (least_busy)".to_string(),
            })
    }

    /// Specific: match by instance_id or partial instance_id hint.
    fn select_specific(
        &self,
        instances: &[ServiceEntry],
        hint: Option<&str>,
    ) -> TransportResult<ServiceEntry> {
        let hint = hint.ok_or_else(|| {
            TransportError::Internal(
                "RoutingStrategy::Specific requires a hint (instance_id)".to_string(),
            )
        })?;

        // Try exact UUID match first
        if let Ok(uuid) = uuid::Uuid::parse_str(hint) {
            if let Some(entry) = instances.iter().find(|e| e.instance_id == uuid) {
                return Ok(entry.clone());
            }
        }

        // Try partial match on instance_id string
        instances
            .iter()
            .find(|e| e.instance_id.to_string().starts_with(hint))
            .cloned()
            .ok_or_else(|| TransportError::ServiceNotFound {
                dcc_type: instances
                    .first()
                    .map(|e| e.dcc_type.clone())
                    .unwrap_or_default(),
                instance_id: hint.to_string(),
            })
    }

    /// Scene match: find an instance whose scene field matches the hint.
    fn select_scene_match(
        &self,
        available: &[&ServiceEntry],
        hint: Option<&str>,
    ) -> TransportResult<ServiceEntry> {
        let hint = hint.ok_or_else(|| {
            TransportError::Internal(
                "RoutingStrategy::SceneMatch requires a hint (scene name)".to_string(),
            )
        })?;

        // Case-insensitive substring match on scene
        let hint_lower = hint.to_lowercase();
        available
            .iter()
            .find(|e| {
                e.scene
                    .as_ref()
                    .is_some_and(|s| s.to_lowercase().contains(&hint_lower))
            })
            .cloned()
            .cloned()
            .ok_or_else(|| TransportError::ServiceNotFound {
                dcc_type: available
                    .first()
                    .map(|e| e.dcc_type.clone())
                    .unwrap_or_default(),
                instance_id: format!("scene={hint}"),
            })
    }

    /// Random: select a random available instance.
    fn select_random(&self, available: &[&ServiceEntry]) -> TransportResult<ServiceEntry> {
        if available.is_empty() {
            return Err(TransportError::ServiceNotFound {
                dcc_type: "unknown".to_string(),
                instance_id: "any (random)".to_string(),
            });
        }

        // Use a simple deterministic "random" based on current time nanoseconds
        // (avoids pulling in a full RNG crate for this simple use case)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let idx = (now.subsec_nanos() as usize) % available.len();
        Ok(available[idx].clone())
    }

    /// Reset round-robin counters (useful for testing or when instances change).
    pub fn reset_counters(&self) {
        self.round_robin_counters.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_instances(count: usize) -> Vec<ServiceEntry> {
        (0..count)
            .map(|i| {
                let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812 + i as u16);
                entry.scene = Some(format!("shot_{:03}.ma", i + 1));
                entry
            })
            .collect()
    }

    fn make_mixed_status_instances() -> Vec<ServiceEntry> {
        let mut instances = vec![];

        let mut e1 = ServiceEntry::new("maya", "127.0.0.1", 18812);
        e1.status = ServiceStatus::Unreachable;
        instances.push(e1);

        let mut e2 = ServiceEntry::new("maya", "127.0.0.1", 18813);
        e2.status = ServiceStatus::Busy;
        instances.push(e2);

        let e3 = ServiceEntry::new("maya", "127.0.0.1", 18814);
        // e3 is Available (default)
        instances.push(e3);

        instances
    }

    #[test]
    fn test_routing_strategy_display() {
        assert_eq!(
            RoutingStrategy::FirstAvailable.to_string(),
            "first_available"
        );
        assert_eq!(RoutingStrategy::RoundRobin.to_string(), "round_robin");
        assert_eq!(RoutingStrategy::LeastBusy.to_string(), "least_busy");
        assert_eq!(RoutingStrategy::Specific.to_string(), "specific");
        assert_eq!(RoutingStrategy::SceneMatch.to_string(), "scene_match");
        assert_eq!(RoutingStrategy::Random.to_string(), "random");
    }

    #[test]
    fn test_routing_strategy_default() {
        assert_eq!(RoutingStrategy::default(), RoutingStrategy::FirstAvailable);
    }

    #[test]
    fn test_router_default() {
        let router = InstanceRouter::default();
        assert_eq!(router.default_strategy(), RoutingStrategy::FirstAvailable);
    }

    #[test]
    fn test_first_available_returns_first_healthy() {
        let router = InstanceRouter::default();
        let instances = make_mixed_status_instances();

        let selected = router
            .select(&instances, Some(RoutingStrategy::FirstAvailable), None)
            .unwrap();
        assert_eq!(selected.port, 18814); // Only the third is Available
    }

    #[test]
    fn test_first_available_no_instances() {
        let router = InstanceRouter::default();
        let result = router.select(&[], Some(RoutingStrategy::FirstAvailable), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_round_robin_distributes() {
        let router = InstanceRouter::new(RoutingStrategy::RoundRobin);
        let instances = make_instances(3);

        let r1 = router.select(&instances, None, None).unwrap();
        let r2 = router.select(&instances, None, None).unwrap();
        let r3 = router.select(&instances, None, None).unwrap();
        let r4 = router.select(&instances, None, None).unwrap();

        // Should cycle through all three, then wrap around
        assert_eq!(r1.port, 18812);
        assert_eq!(r2.port, 18813);
        assert_eq!(r3.port, 18814);
        assert_eq!(r4.port, 18812); // Wraps around
    }

    #[test]
    fn test_round_robin_empty() {
        let router = InstanceRouter::new(RoutingStrategy::RoundRobin);
        let result = router.select(&[], None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_least_busy_prefers_available() {
        let router = InstanceRouter::default();
        let instances = make_mixed_status_instances();

        let selected = router
            .select(&instances, Some(RoutingStrategy::LeastBusy), None)
            .unwrap();
        assert_eq!(selected.port, 18814); // Available instance
    }

    #[test]
    fn test_least_busy_falls_back_to_busy() {
        let router = InstanceRouter::default();
        let mut instances = vec![];

        let mut e1 = ServiceEntry::new("maya", "127.0.0.1", 18812);
        e1.status = ServiceStatus::Unreachable;
        instances.push(e1);

        let mut e2 = ServiceEntry::new("maya", "127.0.0.1", 18813);
        e2.status = ServiceStatus::Busy;
        instances.push(e2);

        let selected = router
            .select(&instances, Some(RoutingStrategy::LeastBusy), None)
            .unwrap();
        assert_eq!(selected.port, 18813); // Falls back to Busy
    }

    #[test]
    fn test_least_busy_all_unreachable() {
        let router = InstanceRouter::default();
        let mut e1 = ServiceEntry::new("maya", "127.0.0.1", 18812);
        e1.status = ServiceStatus::Unreachable;

        let result = router.select(&[e1], Some(RoutingStrategy::LeastBusy), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_specific_by_uuid() {
        let router = InstanceRouter::default();
        let instances = make_instances(3);
        let target_id = instances[1].instance_id.to_string();

        let selected = router
            .select(
                &instances,
                Some(RoutingStrategy::Specific),
                Some(&target_id),
            )
            .unwrap();
        assert_eq!(selected.instance_id, instances[1].instance_id);
    }

    #[test]
    fn test_specific_by_partial_id() {
        let router = InstanceRouter::default();
        let instances = make_instances(3);
        // Use first 8 chars of the UUID
        let partial_id = instances[2].instance_id.to_string()[..8].to_string();

        let selected = router
            .select(
                &instances,
                Some(RoutingStrategy::Specific),
                Some(&partial_id),
            )
            .unwrap();
        assert_eq!(selected.instance_id, instances[2].instance_id);
    }

    #[test]
    fn test_specific_no_hint() {
        let router = InstanceRouter::default();
        let instances = make_instances(1);

        let result = router.select(&instances, Some(RoutingStrategy::Specific), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_specific_not_found() {
        let router = InstanceRouter::default();
        let instances = make_instances(1);

        let result = router.select(
            &instances,
            Some(RoutingStrategy::Specific),
            Some("nonexistent-id"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_scene_match_exact() {
        let router = InstanceRouter::default();
        let instances = make_instances(3);

        let selected = router
            .select(
                &instances,
                Some(RoutingStrategy::SceneMatch),
                Some("shot_002"),
            )
            .unwrap();
        assert_eq!(selected.scene.as_deref(), Some("shot_002.ma"));
    }

    #[test]
    fn test_scene_match_case_insensitive() {
        let router = InstanceRouter::default();
        let instances = make_instances(3);

        let selected = router
            .select(
                &instances,
                Some(RoutingStrategy::SceneMatch),
                Some("SHOT_003"),
            )
            .unwrap();
        assert_eq!(selected.scene.as_deref(), Some("shot_003.ma"));
    }

    #[test]
    fn test_scene_match_no_hint() {
        let router = InstanceRouter::default();
        let instances = make_instances(1);

        let result = router.select(&instances, Some(RoutingStrategy::SceneMatch), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_scene_match_not_found() {
        let router = InstanceRouter::default();
        let instances = make_instances(2);

        let result = router.select(
            &instances,
            Some(RoutingStrategy::SceneMatch),
            Some("nonexistent_scene"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_random_returns_valid_instance() {
        let router = InstanceRouter::default();
        let instances = make_instances(5);

        let selected = router
            .select(&instances, Some(RoutingStrategy::Random), None)
            .unwrap();
        // Just verify it's one of the instances
        assert!(
            instances
                .iter()
                .any(|e| e.instance_id == selected.instance_id)
        );
    }

    #[test]
    fn test_random_empty() {
        let router = InstanceRouter::default();
        let result = router.select(&[], Some(RoutingStrategy::Random), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_reset_counters() {
        let router = InstanceRouter::new(RoutingStrategy::RoundRobin);
        let instances = make_instances(3);

        // Advance the counter
        let _ = router.select(&instances, None, None);
        let _ = router.select(&instances, None, None);

        router.reset_counters();

        // After reset, should start from 0 again
        let selected = router.select(&instances, None, None).unwrap();
        assert_eq!(selected.port, 18812);
    }

    #[test]
    fn test_set_default_strategy() {
        let mut router = InstanceRouter::new(RoutingStrategy::FirstAvailable);
        assert_eq!(router.default_strategy(), RoutingStrategy::FirstAvailable);

        router.set_default_strategy(RoutingStrategy::RoundRobin);
        assert_eq!(router.default_strategy(), RoutingStrategy::RoundRobin);
    }

    #[test]
    fn test_select_uses_default_strategy() {
        let router = InstanceRouter::new(RoutingStrategy::RoundRobin);
        let instances = make_instances(3);

        // Should use RoundRobin since strategy=None
        let r1 = router.select(&instances, None, None).unwrap();
        let r2 = router.select(&instances, None, None).unwrap();
        assert_ne!(r1.instance_id, r2.instance_id);
    }

    #[test]
    fn test_select_override_strategy() {
        let router = InstanceRouter::new(RoutingStrategy::RoundRobin);
        let instances = make_instances(3);
        let target_id = instances[2].instance_id.to_string();

        // Override with Specific, should ignore default RoundRobin
        let selected = router
            .select(
                &instances,
                Some(RoutingStrategy::Specific),
                Some(&target_id),
            )
            .unwrap();
        assert_eq!(selected.instance_id, instances[2].instance_id);
    }
}
