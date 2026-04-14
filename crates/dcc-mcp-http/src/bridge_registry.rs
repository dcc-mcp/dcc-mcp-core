//! Bridge connection registry for gateway mode.
//!
//! In gateway mode, external bridge plugins (Photoshop UXP, ZBrush, etc.) run in separate
//! processes. This registry allows them to register bridge connection info that skill scripts
//! can access via `get_bridge_context()`.

use dashmap::DashMap;
use std::sync::Arc;

/// Information about a bridge connection.
#[derive(Debug, Clone)]
pub struct BridgeContext {
    /// DCC type (e.g., "photoshop", "zbrush")
    pub dcc_type: String,
    /// Bridge endpoint URL (e.g., "ws://localhost:9001")
    pub bridge_url: String,
    /// Whether the bridge is currently connected
    pub connected: bool,
}

/// Registry for bridge connections available in gateway mode.
///
/// Thread-safe registry backed by DashMap for concurrent access from multiple
/// skill script processes.
#[derive(Debug, Clone)]
pub struct BridgeRegistry {
    bridges: Arc<DashMap<String, BridgeContext>>,
}

impl BridgeRegistry {
    /// Create a new empty BridgeRegistry.
    pub fn new() -> Self {
        Self {
            bridges: Arc::new(DashMap::new()),
        }
    }

    /// Register or update a bridge connection.
    ///
    /// # Arguments
    /// * `dcc_type` - DCC type identifier (e.g., "photoshop")
    /// * `url` - Bridge endpoint URL (e.g., "ws://localhost:9001")
    pub fn register(&self, dcc_type: String, url: String) -> Result<(), String> {
        if dcc_type.is_empty() {
            return Err("dcc_type cannot be empty".to_string());
        }
        if url.is_empty() {
            return Err("url cannot be empty".to_string());
        }

        let context = BridgeContext {
            dcc_type: dcc_type.clone(),
            bridge_url: url,
            connected: true,
        };
        self.bridges.insert(dcc_type, context);
        Ok(())
    }

    /// Get bridge context for a specific DCC type.
    pub fn get(&self, dcc_type: &str) -> Option<BridgeContext> {
        self.bridges
            .get(dcc_type)
            .map(|entry| entry.value().clone())
    }

    /// Get bridge URL for a specific DCC type.
    ///
    /// Convenience method that extracts just the URL from bridge context.
    pub fn get_url(&self, dcc_type: &str) -> Option<String> {
        self.get(dcc_type).map(|ctx| ctx.bridge_url)
    }

    /// List all registered bridges.
    pub fn list_all(&self) -> Vec<BridgeContext> {
        self.bridges
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Mark a bridge as disconnected without removing it from registry.
    pub fn set_disconnected(&self, dcc_type: &str) -> Result<(), String> {
        if let Some(mut entry) = self.bridges.get_mut(dcc_type) {
            entry.connected = false;
            Ok(())
        } else {
            Err(format!("Bridge not found: {}", dcc_type))
        }
    }

    /// Remove a bridge from the registry.
    pub fn unregister(&self, dcc_type: &str) -> Result<(), String> {
        self.bridges
            .remove(dcc_type)
            .map(|_| ())
            .ok_or_else(|| format!("Bridge not found: {}", dcc_type))
    }

    /// Clear all registered bridges.
    pub fn clear(&self) {
        self.bridges.clear();
    }

    /// Check if a bridge is registered.
    pub fn contains(&self, dcc_type: &str) -> bool {
        self.bridges.contains_key(dcc_type)
    }

    /// Get the number of registered bridges.
    pub fn len(&self) -> usize {
        self.bridges.len()
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.bridges.is_empty()
    }
}

impl Default for BridgeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_get() {
        let registry = BridgeRegistry::new();
        registry
            .register("photoshop".to_string(), "ws://localhost:9001".to_string())
            .unwrap();

        let ctx = registry.get("photoshop").unwrap();
        assert_eq!(ctx.dcc_type, "photoshop");
        assert_eq!(ctx.bridge_url, "ws://localhost:9001");
        assert!(ctx.connected);
    }

    #[test]
    fn test_get_url() {
        let registry = BridgeRegistry::new();
        registry
            .register("zbrush".to_string(), "http://localhost:9002".to_string())
            .unwrap();

        let url = registry.get_url("zbrush").unwrap();
        assert_eq!(url, "http://localhost:9002");
    }

    #[test]
    fn test_multiple_bridges() {
        let registry = BridgeRegistry::new();
        registry
            .register("photoshop".to_string(), "ws://localhost:9001".to_string())
            .unwrap();
        registry
            .register("zbrush".to_string(), "http://localhost:9002".to_string())
            .unwrap();

        assert_eq!(registry.len(), 2);
        assert!(registry.contains("photoshop"));
        assert!(registry.contains("zbrush"));
    }

    #[test]
    fn test_unregister() {
        let registry = BridgeRegistry::new();
        registry
            .register("photoshop".to_string(), "ws://localhost:9001".to_string())
            .unwrap();

        assert!(registry.contains("photoshop"));
        registry.unregister("photoshop").unwrap();
        assert!(!registry.contains("photoshop"));
    }

    #[test]
    fn test_set_disconnected() {
        let registry = BridgeRegistry::new();
        registry
            .register("photoshop".to_string(), "ws://localhost:9001".to_string())
            .unwrap();

        let ctx = registry.get("photoshop").unwrap();
        assert!(ctx.connected);

        registry.set_disconnected("photoshop").unwrap();
        let ctx = registry.get("photoshop").unwrap();
        assert!(!ctx.connected);
    }

    #[test]
    fn test_invalid_registration() {
        let registry = BridgeRegistry::new();

        assert!(
            registry
                .register("".to_string(), "ws://localhost:9001".to_string())
                .is_err()
        );
        assert!(
            registry
                .register("photoshop".to_string(), "".to_string())
                .is_err()
        );
    }

    #[test]
    fn test_clear() {
        let registry = BridgeRegistry::new();
        registry
            .register("photoshop".to_string(), "ws://localhost:9001".to_string())
            .unwrap();
        registry
            .register("zbrush".to_string(), "http://localhost:9002".to_string())
            .unwrap();

        assert_eq!(registry.len(), 2);
        registry.clear();
        assert!(registry.is_empty());
    }
}
