//! Row patching for [`FileRegistry`] — applying a change set to one service row.
//!
//! Split out of [`super::file_registry`] (issue #842 file-size policy): every
//! function here answers a single question — *given a row and a patch, what
//! does the row become?* — so the registry itself keeps only the transaction,
//! locking and persistence concerns.
//!
//! The merge rules are shared by both writable collections on a row:
//!
//! - [`ServiceEntry::metadata`] — strings only; an empty value removes the key.
//! - [`ServiceEntry::extras`] — JSON-typed; [`serde_json::Value::Null`] removes
//!   the key. See `apply_extras_patch` for why a `Null` is a tombstone rather
//!   than a value.

use std::collections::HashMap;

use super::file_registry::FileRegistry;
use super::types::{ServiceEntry, ServiceKey, ServiceSnapshot};
use crate::error::TransportResult;

/// Apply a borrowed [`ServiceSnapshot`] patch to one row, then touch its heartbeat.
///
/// `None` leaves a field unchanged. Empty strings clear optional string fields,
/// `Some(&[])` clears the document list, an empty `metadata` value removes that
/// key, and a [`serde_json::Value::Null`] `extras` value removes that key.
///
/// This is a pure in-memory mutation: callers own the write transaction and the
/// decision to persist.
pub fn apply_snapshot(entry: &mut ServiceEntry, snapshot: &ServiceSnapshot<'_>) {
    if let Some(scene) = snapshot.scene {
        entry.scene = (!scene.is_empty()).then(|| scene.to_string());
    }
    if let Some(version) = snapshot.version {
        entry.version = (!version.is_empty()).then(|| version.to_string());
    }
    if let Some(documents) = snapshot.documents {
        entry.documents = documents
            .iter()
            .filter(|document| !document.is_empty())
            .cloned()
            .collect();
    }
    if let Some(display_name) = snapshot.display_name {
        entry.display_name = (!display_name.is_empty()).then(|| display_name.to_string());
    }
    if let Some(metadata) = snapshot.metadata {
        apply_metadata_patch(entry, metadata);
    }
    if let Some(extras) = snapshot.extras {
        apply_extras_patch(entry, extras);
    }
    entry.touch();
}

/// Merge string metadata into a row; an empty value removes the matching key.
///
/// Keys absent from `patch` are left untouched, so an adapter can update one
/// field without resending metadata owned by another component.
pub fn apply_metadata_patch(entry: &mut ServiceEntry, patch: &HashMap<String, String>) {
    for (name, value) in patch {
        if value.is_empty() {
            entry.metadata.remove(name);
        } else {
            entry.metadata.insert(name.clone(), value.clone());
        }
    }
}

/// Merge JSON-typed extras into a row; a [`serde_json::Value::Null`] removes the
/// matching key.
///
/// Unlike [`apply_metadata_patch`], values keep their JSON type: integers,
/// floats, booleans, nested objects and arrays all survive the
/// `services.json` round-trip instead of being coerced to strings (issue
/// #2500). That fidelity is the entire reason `extras` exists alongside
/// `metadata`.
///
/// A `Null` is treated as a **removal tombstone**, not as a stored value.
/// Callers publish extras as a *merge patch*, so a key that was dropped from
/// the patch before it reached this function would carry no instruction to
/// delete it and the row would keep serving a stale value. Keeping the `Null`
/// in the patch is what lets a delete propagate; readers filter the tombstones
/// back out.
pub fn apply_extras_patch(entry: &mut ServiceEntry, patch: &HashMap<String, serde_json::Value>) {
    for (name, value) in patch {
        if value.is_null() {
            entry.extras.remove(name);
        } else {
            entry.extras.insert(name.clone(), value.clone());
        }
    }
}

/// Row-patching surface of [`FileRegistry`].
///
/// These live beside the pure helpers above because they are thin adapters:
/// each one builds a [`ServiceSnapshot`] and hands it to
/// [`FileRegistry::update_snapshot`], which owns the transaction.
///
/// `update_snapshot` itself deliberately stays in `file_registry.rs`: it is the
/// one member that reaches into the registry's private lock and entry map, and
/// widening that visibility purely to split the file would leak the locking
/// internals to the whole crate.
impl FileRegistry {
    /// Update scene and/or version metadata for a service, and refresh heartbeat.
    ///
    /// This is the primary way for a running instance to report that the user
    /// has opened a different scene (e.g. switched documents in Photoshop) or
    /// that the DCC version has changed.
    pub fn update_metadata(
        &self,
        key: &ServiceKey,
        scene: Option<&str>,
        version: Option<&str>,
    ) -> TransportResult<bool> {
        self.update_snapshot(
            key,
            ServiceSnapshot {
                scene,
                version,
                ..ServiceSnapshot::default()
            },
        )
    }

    /// Update the active document, full document list, and optional display name.
    ///
    /// Designed for multi-document DCC applications (e.g. Photoshop, After Effects)
    /// that can have several files open simultaneously. For single-document DCCs
    /// (Maya, Blender, Houdini) it is equivalent to [`Self::update_metadata`] with
    /// the `scene` field, but also stores `display_name` when provided.
    ///
    /// # Parameters
    /// - `active_document` — the currently focused file; stored in `scene`.
    ///   Pass `Some("")` to clear.
    /// - `documents` — full list of open documents; replaces the previous list.
    ///   Pass `&[]` to clear.
    /// - `display_name` — human-readable instance label (e.g. `"PS-Marketing"`).
    ///   Pass `Some("")` to clear.  `None` leaves the existing value unchanged.
    ///
    /// Always refreshes the heartbeat so the gateway does not mark the instance stale.
    pub fn update_documents(
        &self,
        key: &ServiceKey,
        active_document: Option<&str>,
        documents: &[String],
        display_name: Option<&str>,
    ) -> TransportResult<bool> {
        self.update_snapshot(
            key,
            ServiceSnapshot {
                scene: active_document,
                documents: Some(documents),
                display_name,
                ..ServiceSnapshot::default()
            },
        )
    }

    /// Merge arbitrary string metadata for a service and refresh heartbeat.
    ///
    /// Values are merged into [`ServiceEntry::metadata`]. Passing an empty value
    /// removes that key, which gives embedders a small clearing mechanism
    /// without replacing unrelated adapter metadata.
    pub fn update_instance_metadata(
        &self,
        key: &ServiceKey,
        metadata: &HashMap<String, String>,
    ) -> TransportResult<bool> {
        self.update_snapshot(
            key,
            ServiceSnapshot {
                metadata: Some(metadata),
                ..ServiceSnapshot::default()
            },
        )
    }

    /// Merge arbitrary JSON-typed extras for a service and refresh heartbeat.
    ///
    /// This is the [`ServiceEntry::extras`] counterpart of
    /// [`Self::update_instance_metadata`]. Unlike `metadata`, values keep their
    /// JSON type — numbers, booleans, nested objects and arrays all survive the
    /// `services.json` round-trip (issue #2500).
    ///
    /// Values are merged into [`ServiceEntry::extras`]. Passing
    /// [`serde_json::Value::Null`] removes that key, which gives embedders a
    /// small clearing mechanism without replacing unrelated adapter extras.
    pub fn update_instance_extras(
        &self,
        key: &ServiceKey,
        extras: &HashMap<String, serde_json::Value>,
    ) -> TransportResult<bool> {
        self.update_snapshot(
            key,
            ServiceSnapshot {
                extras: Some(extras),
                ..ServiceSnapshot::default()
            },
        )
    }
}
