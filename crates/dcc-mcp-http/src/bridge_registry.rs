//! Bridge connection registry for gateway mode.
//!
//! In gateway mode, external bridge plugins (Photoshop UXP, ZBrush, etc.) run in separate
//! processes. This registry allows them to register bridge connection info that skill scripts
//! can access via `get_bridge_context()`.

use dashmap::DashMap;
use dcc_mcp_protocols::bridge::{BridgeHeartbeat, BridgeHello};
use serde_json::{Map, Value};
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
    /// Stable identifier for one concrete DCC process or editor session.
    pub instance_id: Option<String>,
    /// Identifier for one plugin peer inside the host process.
    pub peer_id: Option<String>,
    /// Host application version reported by the plugin.
    pub host_version: Option<String>,
    /// Project or experience identifier reported by the plugin.
    pub project_id: Option<String>,
    /// Active document, place, or scene identifier reported by the plugin.
    pub document_id: Option<String>,
    /// Whether the host can accept calls at the time of the last update.
    pub ready: bool,
    /// Unix epoch timestamp in milliseconds for the last heartbeat.
    pub last_heartbeat_ms: Option<u64>,
    /// Capability metadata advertised by the plugin.
    pub capabilities: Map<String, Value>,
    /// Additional plugin-owned metadata.
    pub metadata: Map<String, Value>,
}

impl BridgeContext {
    fn legacy(dcc_type: String, bridge_url: String) -> Self {
        Self {
            dcc_type,
            bridge_url,
            connected: true,
            instance_id: None,
            peer_id: None,
            host_version: None,
            project_id: None,
            document_id: None,
            ready: true,
            last_heartbeat_ms: None,
            capabilities: Map::new(),
            metadata: Map::new(),
        }
    }

    /// Create a context for a concrete plugin instance.
    #[must_use]
    pub fn instance(
        dcc_type: String,
        bridge_url: String,
        instance_id: String,
        peer_id: Option<String>,
    ) -> Self {
        Self {
            dcc_type,
            bridge_url,
            connected: true,
            instance_id: Some(instance_id),
            peer_id,
            host_version: None,
            project_id: None,
            document_id: None,
            ready: false,
            last_heartbeat_ms: None,
            capabilities: Map::new(),
            metadata: Map::new(),
        }
    }
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

        self.register_context(BridgeContext::legacy(dcc_type, url))
    }

    /// Register or update a concrete bridge instance while retaining other
    /// instances of the same DCC type.
    pub fn register_context(&self, context: BridgeContext) -> Result<(), String> {
        validate_context(&context)?;
        let key = context
            .instance_id
            .as_deref()
            .map(instance_key)
            .unwrap_or_else(|| dcc_key(&context.dcc_type));
        self.bridges.insert(key, context);
        Ok(())
    }

    /// Register a concrete instance with the common metadata fields.
    #[allow(clippy::too_many_arguments)]
    pub fn register_instance(
        &self,
        dcc_type: String,
        url: String,
        instance_id: String,
        peer_id: Option<String>,
        host_version: Option<String>,
        project_id: Option<String>,
        document_id: Option<String>,
        ready: bool,
    ) -> Result<(), String> {
        let mut context = BridgeContext::instance(dcc_type, url, instance_id, peer_id);
        context.host_version = host_version;
        context.project_id = project_id;
        context.document_id = document_id;
        context.ready = ready;
        self.register_context(context)
    }

    /// Register a protocol hello without making each adapter copy metadata
    /// extraction logic.
    pub fn register_hello(
        &self,
        dcc_type: String,
        url: String,
        hello: &BridgeHello,
        ready: bool,
    ) -> Result<(), String> {
        let mut context = match hello.instance_id.clone() {
            Some(instance_id) => {
                BridgeContext::instance(dcc_type, url, instance_id, hello.peer_id.clone())
            }
            None => BridgeContext::legacy(dcc_type, url),
        };
        context.host_version = hello
            .host_version
            .clone()
            .or_else(|| hello.dcc_version.clone());
        context.project_id = hello.project.clone();
        context.document_id = hello.document.clone();
        context.ready = ready;
        context.capabilities = hello.capabilities.clone();
        context.metadata = hello.metadata.clone();
        self.register_context(context)
    }

    /// Get bridge context for a specific DCC type.
    pub fn get(&self, dcc_type: &str) -> Option<BridgeContext> {
        self.select_for_dcc_type(dcc_type)
    }

    /// Get a bridge context for a concrete instance identifier.
    pub fn get_instance(&self, instance_id: &str) -> Option<BridgeContext> {
        self.bridges
            .get(&instance_key(instance_id))
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

    /// Update liveness and host metadata for a concrete instance.
    pub fn update_instance(
        &self,
        instance_id: &str,
        connected: bool,
        ready: bool,
        last_heartbeat_ms: Option<u64>,
        document_id: Option<String>,
    ) -> Result<(), String> {
        if let Some(mut entry) = self.bridges.get_mut(&instance_key(instance_id)) {
            entry.connected = connected;
            entry.ready = ready;
            entry.last_heartbeat_ms = last_heartbeat_ms;
            if document_id.is_some() {
                entry.document_id = document_id;
            }
            Ok(())
        } else {
            Err(format!("Bridge instance not found: {instance_id}"))
        }
    }

    /// Apply a protocol heartbeat to a registered instance.
    pub fn apply_heartbeat(
        &self,
        instance_id: &str,
        heartbeat: &BridgeHeartbeat,
        last_heartbeat_ms: Option<u64>,
    ) -> Result<(), String> {
        let ready = heartbeat.ready.unwrap_or(true);
        self.update_instance(instance_id, true, ready, last_heartbeat_ms, None)
    }

    /// Mark a bridge as disconnected without removing it from registry.
    pub fn set_disconnected(&self, dcc_type: &str) -> Result<(), String> {
        let keys: Vec<String> = self
            .bridges
            .iter()
            .filter(|entry| entry.value().dcc_type == dcc_type)
            .map(|entry| entry.key().clone())
            .collect();
        if keys.is_empty() {
            Err(format!("Bridge not found: {dcc_type}"))
        } else {
            for key in keys {
                if let Some(mut entry) = self.bridges.get_mut(&key) {
                    entry.connected = false;
                    entry.ready = false;
                }
            }
            Ok(())
        }
    }

    /// Mark one concrete instance as disconnected.
    pub fn set_instance_disconnected(&self, instance_id: &str) -> Result<(), String> {
        if let Some(mut entry) = self.bridges.get_mut(&instance_key(instance_id)) {
            entry.connected = false;
            entry.ready = false;
            Ok(())
        } else {
            Err(format!("Bridge instance not found: {instance_id}"))
        }
    }

    /// Remove a bridge from the registry.
    pub fn unregister(&self, dcc_type: &str) -> Result<(), String> {
        let keys: Vec<String> = self
            .bridges
            .iter()
            .filter(|entry| entry.value().dcc_type == dcc_type)
            .map(|entry| entry.key().clone())
            .collect();
        if keys.is_empty() {
            return Err(format!("Bridge not found: {dcc_type}"));
        }
        for key in keys {
            self.bridges.remove(&key);
        }
        Ok(())
    }

    /// Remove one concrete bridge instance.
    pub fn unregister_instance(&self, instance_id: &str) -> Result<(), String> {
        self.bridges
            .remove(&instance_key(instance_id))
            .map(|_| ())
            .ok_or_else(|| format!("Bridge instance not found: {instance_id}"))
    }

    /// Clear all registered bridges.
    pub fn clear(&self) {
        self.bridges.clear();
    }

    /// Check if a bridge is registered.
    pub fn contains(&self, dcc_type: &str) -> bool {
        self.bridges.contains_key(&dcc_key(dcc_type))
            || self
                .bridges
                .iter()
                .any(|entry| entry.value().dcc_type == dcc_type)
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

fn validate_context(context: &BridgeContext) -> Result<(), String> {
    if context.dcc_type.is_empty() {
        return Err("dcc_type cannot be empty".to_string());
    }
    if context.bridge_url.is_empty() {
        return Err("url cannot be empty".to_string());
    }
    if context.instance_id.as_deref() == Some("") {
        return Err("instance_id cannot be empty".to_string());
    }
    Ok(())
}

fn dcc_key(dcc_type: &str) -> String {
    format!("dcc:{dcc_type}")
}

fn instance_key(instance_id: &str) -> String {
    format!("instance:{instance_id}")
}

impl BridgeRegistry {
    fn select_for_dcc_type(&self, dcc_type: &str) -> Option<BridgeContext> {
        self.bridges
            .iter()
            .filter(|entry| entry.value().dcc_type == dcc_type)
            .max_by_key(|entry| {
                (
                    entry.value().connected,
                    entry.value().ready,
                    entry.value().last_heartbeat_ms.unwrap_or_default(),
                )
            })
            .map(|entry| entry.value().clone())
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

    #[test]
    fn concrete_instances_share_a_dcc_type_without_overwriting() {
        let registry = BridgeRegistry::new();
        registry
            .register_instance(
                "roblox-studio".to_string(),
                "ws://localhost:9001".to_string(),
                "studio-1".to_string(),
                Some("plugin-1".to_string()),
                Some("0.632".to_string()),
                Some("experience-1".to_string()),
                Some("place-1".to_string()),
                true,
            )
            .unwrap();
        registry
            .register_instance(
                "roblox-studio".to_string(),
                "ws://localhost:9002".to_string(),
                "studio-2".to_string(),
                Some("plugin-2".to_string()),
                Some("0.633".to_string()),
                Some("experience-2".to_string()),
                Some("place-2".to_string()),
                true,
            )
            .unwrap();

        assert_eq!(registry.len(), 2);
        assert_eq!(
            registry
                .get_instance("studio-1")
                .unwrap()
                .document_id
                .as_deref(),
            Some("place-1")
        );
        assert_eq!(registry.list_all().len(), 2);
        assert!(registry.get("roblox-studio").is_some());
    }

    #[test]
    fn dcc_type_lifecycle_operations_cover_all_concrete_instances() {
        let registry = BridgeRegistry::new();
        registry
            .register_instance(
                "roblox-studio".to_string(),
                "ws://localhost:9001".to_string(),
                "studio-1".to_string(),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        registry
            .register_instance(
                "roblox-studio".to_string(),
                "ws://localhost:9002".to_string(),
                "studio-2".to_string(),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();

        registry.set_disconnected("roblox-studio").unwrap();
        assert!(!registry.get_instance("studio-1").unwrap().connected);
        assert!(!registry.get_instance("studio-2").unwrap().connected);

        registry.unregister("roblox-studio").unwrap();
        assert!(registry.is_empty());
    }

    #[test]
    fn instance_heartbeat_updates_readiness_and_document() {
        let registry = BridgeRegistry::new();
        registry
            .register_instance(
                "photoshop".to_string(),
                "ws://localhost:9001".to_string(),
                "ps-1".to_string(),
                None,
                None,
                None,
                Some("old.psd".to_string()),
                false,
            )
            .unwrap();
        registry
            .update_instance("ps-1", true, true, Some(123), Some("new.psd".to_string()))
            .unwrap();
        let context = registry.get_instance("ps-1").unwrap();
        assert!(context.ready);
        assert_eq!(context.last_heartbeat_ms, Some(123));
        assert_eq!(context.document_id.as_deref(), Some("new.psd"));
    }

    #[test]
    fn concrete_instance_disconnect_clears_readiness() {
        let registry = BridgeRegistry::new();
        registry
            .register_instance(
                "roblox-studio".to_string(),
                "ws://localhost:9001".to_string(),
                "studio-1".to_string(),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();

        registry.set_instance_disconnected("studio-1").unwrap();
        let context = registry.get_instance("studio-1").unwrap();
        assert!(!context.connected);
        assert!(!context.ready);
    }

    #[test]
    fn protocol_hello_and_heartbeat_update_one_instance() {
        let registry = BridgeRegistry::new();
        let mut hello = BridgeHello {
            protocol_version: Some("1".to_string()),
            client: "roblox-studio-plugin".to_string(),
            version: "0.1.0".to_string(),
            dcc_version: Some("0.632".to_string()),
            instance_id: Some("studio-1".to_string()),
            peer_id: Some("plugin".to_string()),
            host_name: Some("Roblox Studio".to_string()),
            host_version: Some("0.632".to_string()),
            document: Some("place-1".to_string()),
            project: Some("experience-1".to_string()),
            data_model: Some("DataModel".to_string()),
            capabilities: Map::new(),
            metadata: Map::new(),
        };
        hello
            .capabilities
            .insert("search_game_tree".to_string(), Value::Bool(true));
        registry
            .register_hello(
                "roblox-studio".to_string(),
                "ws://localhost:9001".to_string(),
                &hello,
                true,
            )
            .unwrap();
        registry
            .apply_heartbeat(
                "studio-1",
                &BridgeHeartbeat {
                    sequence: 3,
                    ready: Some(true),
                    data: None,
                },
                Some(456),
            )
            .unwrap();

        let context = registry.get_instance("studio-1").unwrap();
        assert_eq!(context.project_id.as_deref(), Some("experience-1"));
        assert_eq!(context.capabilities["search_game_tree"], Value::Bool(true));
        assert_eq!(context.last_heartbeat_ms, Some(456));
    }
}
