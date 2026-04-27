//! Built-in [`ResourceProducer`] implementations: `scene://`,
//! `capture://`, `audit://`, and `artefact://`.

use std::sync::Arc;

use dcc_mcp_artefact::SharedArtefactStore;
use parking_lot::RwLock;
use serde_json::{Value, json};

use super::types::{ProducerContent, ResourceError, ResourceProducer, ResourceResult};
use crate::protocol::McpResource;

pub(crate) struct SceneProducer {
    pub(crate) snapshot: Arc<RwLock<Option<Value>>>,
}

impl ResourceProducer for SceneProducer {
    fn scheme(&self) -> &str {
        "scene"
    }

    fn list(&self) -> Vec<McpResource> {
        vec![McpResource {
            uri: "scene://current".to_string(),
            name: "Current Scene".to_string(),
            description: Some(
                "JSON summary of the current DCC scene (nodes, counts, metadata). \
                 Updated by the embedding adapter via ResourceRegistry::set_scene()."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
        }]
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        if uri != "scene://current" {
            return Err(ResourceError::NotFound(uri.to_string()));
        }
        let snapshot = self.snapshot.read();
        let text = match snapshot.as_ref() {
            Some(v) => serde_json::to_string(v).map_err(|e| ResourceError::Read(e.to_string()))?,
            None => serde_json::to_string(&json!({
                "status": "no_scene_published",
                "hint": "embedding adapter should call ResourceRegistry::set_scene"
            }))
            .map_err(|e| ResourceError::Read(e.to_string()))?,
        };
        Ok(ProducerContent::Text {
            uri: uri.to_string(),
            mime_type: "application/json".to_string(),
            text,
        })
    }
}

pub(crate) struct CaptureProducer;

impl ResourceProducer for CaptureProducer {
    fn scheme(&self) -> &str {
        "capture"
    }

    fn list(&self) -> Vec<McpResource> {
        // Only surface capture://current_window when a real window backend
        // is available. Mock backend indicates no DCC window is present.
        let capturer = dcc_mcp_capture::Capturer::new_window_auto();
        if matches!(
            capturer.backend_kind(),
            dcc_mcp_capture::CaptureBackendKind::Mock
        ) {
            return Vec::new();
        }
        vec![McpResource {
            uri: "capture://current_window".to_string(),
            name: "DCC Window Snapshot".to_string(),
            description: Some(
                "PNG snapshot of the active DCC window. Read-only; each \
                 resources/read triggers a fresh capture."
                    .to_string(),
            ),
            mime_type: Some("image/png".to_string()),
        }]
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        if uri != "capture://current_window" {
            return Err(ResourceError::NotFound(uri.to_string()));
        }
        let capturer = dcc_mcp_capture::Capturer::new_window_auto();
        let cfg = dcc_mcp_capture::CaptureConfig::builder()
            .format(dcc_mcp_capture::CaptureFormat::Png)
            .build();
        let frame = capturer
            .capture(&cfg)
            .map_err(|e| ResourceError::Read(e.to_string()))?;
        Ok(ProducerContent::Blob {
            uri: uri.to_string(),
            mime_type: "image/png".to_string(),
            bytes: frame.data,
        })
    }
}

pub(crate) struct AuditProducer {
    log: Option<Arc<dcc_mcp_sandbox::AuditLog>>,
}

impl AuditProducer {
    pub(crate) fn new(log: Arc<dcc_mcp_sandbox::AuditLog>) -> Self {
        Self { log: Some(log) }
    }

    /// Stub producer used until `ResourceRegistry::wire_audit_log` is
    /// called. Returns an empty tail so callers can still list and read
    /// the resource without blowing up.
    pub(crate) fn disabled() -> Self {
        Self { log: None }
    }
}

impl ResourceProducer for AuditProducer {
    fn scheme(&self) -> &str {
        "audit"
    }

    fn list(&self) -> Vec<McpResource> {
        vec![McpResource {
            uri: "audit://recent".to_string(),
            name: "Recent Audit Entries".to_string(),
            description: Some(
                "Tail of the sandbox AuditLog. Supports ?limit=N (default \
                 100). Fires notifications/resources/updated on append."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
        }]
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        let limit = parse_audit_limit(uri).unwrap_or(100);
        let entries = match &self.log {
            Some(log) => {
                let all = log.entries();
                let start = all.len().saturating_sub(limit);
                all[start..].to_vec()
            }
            None => Vec::new(),
        };
        let payload = json!({
            "limit": limit,
            "count": entries.len(),
            "entries": entries,
        });
        let text =
            serde_json::to_string(&payload).map_err(|e| ResourceError::Read(e.to_string()))?;
        Ok(ProducerContent::Text {
            uri: uri.to_string(),
            mime_type: "application/json".to_string(),
            text,
        })
    }
}

fn parse_audit_limit(uri: &str) -> Option<usize> {
    let q = uri.split_once('?')?.1;
    for kv in q.split('&') {
        if let Some(("limit", value)) = kv.split_once('=') {
            return value.parse().ok();
        }
    }
    None
}

/// Producer for `artefact://` URIs, backed by a
/// [`dcc_mcp_artefact::ArtefactStore`] (issue #349).
///
/// When `enabled` is `false`, `list` hides entries and `read` returns
/// `ResourceError::NotEnabled` so clients can distinguish "scheme unknown"
/// from "scheme recognized but disabled".
pub(crate) struct ArtefactProducer {
    pub(crate) enabled: bool,
    pub(crate) store: Option<SharedArtefactStore>,
}

impl ResourceProducer for ArtefactProducer {
    fn scheme(&self) -> &str {
        "artefact"
    }

    fn list(&self) -> Vec<McpResource> {
        if !self.enabled {
            return Vec::new();
        }
        let Some(store) = self.store.as_ref() else {
            return Vec::new();
        };
        let refs = store
            .list(dcc_mcp_artefact::ArtefactFilter::default())
            .unwrap_or_default();
        refs.into_iter()
            .map(|fr| McpResource {
                uri: fr.uri.clone(),
                name: fr.digest.clone().unwrap_or_else(|| fr.uri.clone()),
                description: Some(format!(
                    "Artefact ({} bytes, digest {})",
                    fr.size_bytes.unwrap_or(0),
                    fr.digest.as_deref().unwrap_or("unknown"),
                )),
                mime_type: fr.mime.clone(),
            })
            .collect()
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        if !self.enabled {
            return Err(ResourceError::NotEnabled(format!(
                "artefact resources not enabled: {uri}"
            )));
        }
        let Some(store) = self.store.as_ref() else {
            return Err(ResourceError::NotEnabled(format!(
                "artefact store not configured: {uri}"
            )));
        };
        let head = store
            .head(uri)
            .map_err(|e| ResourceError::Read(e.to_string()))?;
        let Some(head) = head else {
            return Err(ResourceError::NotFound(uri.to_string()));
        };
        let body = store
            .get(uri)
            .map_err(|e| ResourceError::Read(e.to_string()))?;
        let bytes = body
            .ok_or_else(|| ResourceError::NotFound(uri.to_string()))?
            .into_bytes()
            .map_err(|e| ResourceError::Read(e.to_string()))?;
        let mime = head
            .mime
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());
        Ok(ProducerContent::Blob {
            uri: uri.to_string(),
            mime_type: mime,
            bytes,
        })
    }
}
