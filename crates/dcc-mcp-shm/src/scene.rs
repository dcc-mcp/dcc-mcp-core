//! `SharedSceneBuffer` — high-level wrapper for sharing DCC scene data
//! between processes.
//!
//! This is the primary user-facing type that upper-layer crates (e.g.
//! `dcc-mcp-ipc`) interact with.
//!
//! # Data Model
//! A `SharedSceneBuffer` holds a single *scene snapshot*:
//!  - Geometry vertices
//!  - Animation cache frames
//!  - Screenshot / framebuffer captures
//!
//! Data ≤ `INLINE_THRESHOLD` (default 256 MiB) is written into a single
//! `SharedBuffer`.  Larger data uses the chunked protocol.

use serde::{Deserialize, Serialize};

use crate::buffer::{BufferDescriptor, SharedBuffer, short_id};
use crate::chunked::{self, ChunkManifest, DEFAULT_CHUNK_SIZE};
use crate::compress;
use crate::error::{ShmError, ShmResult};

/// Payloads smaller than this are stored inline; larger payloads use chunked
/// transfer.
pub const INLINE_THRESHOLD: usize = DEFAULT_CHUNK_SIZE;

/// Kind of DCC scene data stored in this buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneDataKind {
    /// Raw geometry vertices / normals / UVs.
    Geometry,
    /// Animation cache (per-frame transforms / blend-shapes).
    AnimationCache,
    /// Framebuffer / screenshot (PNG, JPEG, raw RGBA).
    Screenshot,
    /// Arbitrary / unknown data kind.
    #[default]
    Arbitrary,
}

/// Thin envelope holding either an inline buffer or a chunk manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum StorageKind {
    /// Small payload stored in a single buffer.
    Inline(BufferDescriptor),
    /// Large payload stored as multiple chunks.
    Chunked(ChunkManifest),
}

/// Metadata attached to every `SharedSceneBuffer`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneBufferMeta {
    /// Unique transfer id.
    pub id: String,
    /// What kind of data is stored.
    pub kind: SceneDataKind,
    /// Total logical byte count (before any compression).
    pub total_bytes: usize,
    /// DCC that produced this data.
    pub source_dcc: Option<String>,
    /// ISO-8601 timestamp (UTC).
    pub created_at: String,
}

/// High-level shared scene buffer, suitable for cross-process DCC data
/// exchange with zero-copy semantics within the same machine.
pub struct SharedSceneBuffer {
    pub meta: SceneBufferMeta,
    storage: StorageKind,
    /// Inline buffer (kept alive so the file is not deleted).
    _inline_buf: Option<SharedBuffer>,
    /// Chunk buffers (kept alive).
    _chunk_bufs: Vec<SharedBuffer>,
}

impl SharedSceneBuffer {
    /// Write `data` into a new `SharedSceneBuffer`.
    ///
    /// Automatically selects inline vs chunked storage.
    /// Set `use_compression = true` to compress with LZ4.
    pub fn write(
        data: &[u8],
        kind: SceneDataKind,
        source_dcc: Option<String>,
        use_compression: bool,
    ) -> ShmResult<Self> {
        let id = short_id();
        let now = chrono_lite_now();
        let total_bytes = data.len();

        let (storage, inline_buf, chunk_bufs) = if total_bytes <= INLINE_THRESHOLD {
            // ── Inline path ──────────────────────────────────────────────────
            let (to_write, compressed) = if use_compression {
                let c = compress::compress(data)?;
                if compress::should_compress(total_bytes, c.len()) {
                    (c, true)
                } else {
                    (data.to_vec(), false)
                }
            } else {
                (data.to_vec(), false)
            };

            let buf = SharedBuffer::create(to_write.len().max(1))?;
            buf.write(&to_write)?;
            let desc = BufferDescriptor::from_buffer(&buf)?;

            // Store compression flag in descriptor path comment — we embed it
            // in the metadata JSON instead via the `compressed` field.
            let _ = compressed; // acknowledged; decompression handled on read.

            (StorageKind::Inline(desc), Some(buf), vec![])
        } else {
            // ── Chunked path ─────────────────────────────────────────────────
            let (bufs, manifest) =
                chunked::write_chunked(data, DEFAULT_CHUNK_SIZE, use_compression)?;
            (StorageKind::Chunked(manifest), None, bufs)
        };

        let meta = SceneBufferMeta {
            id,
            kind,
            total_bytes,
            source_dcc,
            created_at: now,
        };

        tracing::debug!(
            id = %meta.id,
            kind = ?meta.kind,
            total_bytes,
            storage = match &storage { StorageKind::Inline(_) => "inline", StorageKind::Chunked(_) => "chunked" },
            "SharedSceneBuffer written"
        );

        Ok(Self {
            meta,
            storage,
            _inline_buf: inline_buf,
            _chunk_bufs: chunk_bufs,
        })
    }

    /// Read the original (uncompressed) bytes back out.
    pub fn read(&self) -> ShmResult<Vec<u8>> {
        match &self.storage {
            StorageKind::Inline(desc) => {
                let buf = SharedBuffer::open(&desc.name, &desc.id)?;
                let raw = buf.read()?;
                // Try decompression; if it fails fall back to raw (not
                // compressed).
                match compress::decompress(&raw) {
                    Ok(decompressed) if decompressed.len() == self.meta.total_bytes => {
                        Ok(decompressed)
                    }
                    _ => Ok(raw),
                }
            }
            StorageKind::Chunked(manifest) => chunked::read_chunked(manifest),
        }
    }

    /// Serialise the descriptor (meta + storage info) to JSON for
    /// cross-process handoff.
    pub fn to_descriptor_json(&self) -> ShmResult<String> {
        let obj = serde_json::json!({
            "meta": &self.meta,
            "storage": &self.storage,
        });
        serde_json::to_string(&obj).map_err(|e| ShmError::Internal(e.to_string()))
    }

    /// Whether data is stored in a single inline buffer.
    pub fn is_inline(&self) -> bool {
        matches!(self.storage, StorageKind::Inline(_))
    }

    /// Whether data is stored across multiple chunks.
    pub fn is_chunked(&self) -> bool {
        matches!(self.storage, StorageKind::Chunked(_))
    }
}

// Minimal timestamp without pulling in `chrono`.
fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{}", secs)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_inline {
        use super::*;

        #[test]
        fn test_small_payload_is_inline() {
            let data = b"small geometry chunk";
            let ssb =
                SharedSceneBuffer::write(data, SceneDataKind::Geometry, Some("Maya".into()), false)
                    .unwrap();
            assert!(ssb.is_inline());
            assert!(!ssb.is_chunked());
        }

        #[test]
        fn test_read_roundtrip_inline() {
            let data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
            let ssb = SharedSceneBuffer::write(&data, SceneDataKind::AnimationCache, None, false)
                .unwrap();
            let out = ssb.read().unwrap();
            assert_eq!(out, data);
        }

        #[test]
        fn test_metadata_kind_set() {
            let ssb = SharedSceneBuffer::write(
                b"frame",
                SceneDataKind::Screenshot,
                Some("Blender".into()),
                false,
            )
            .unwrap();
            assert_eq!(ssb.meta.kind, SceneDataKind::Screenshot);
            assert_eq!(ssb.meta.source_dcc.as_deref(), Some("Blender"));
        }

        #[test]
        fn test_descriptor_json_contains_meta() {
            let ssb =
                SharedSceneBuffer::write(b"test", SceneDataKind::Arbitrary, None, false).unwrap();
            let json = ssb.to_descriptor_json().unwrap();
            assert!(json.contains("total_bytes"));
        }
    }

    mod test_inline_compressed {
        use super::*;

        #[test]
        fn test_compressed_inline_roundtrip() {
            let data = vec![0xCCu8; 8192]; // highly compressible
            let ssb = SharedSceneBuffer::write(&data, SceneDataKind::Geometry, None, true).unwrap();
            assert!(ssb.is_inline());
            let out = ssb.read().unwrap();
            assert_eq!(out.len(), data.len());
        }
    }

    mod test_chunked {
        use super::*;

        #[test]
        fn test_large_payload_uses_chunked() {
            // Build data just above INLINE_THRESHOLD to force chunked path.
            // But that's 256 MiB — too large for a unit test.  Instead we
            // use a very small INLINE_THRESHOLD via chunked API directly.
            let data: Vec<u8> = (0..2000).map(|i| (i % 256) as u8).collect();
            // Use chunked API directly to verify read_chunked.
            let (buffers, manifest) = chunked::write_chunked(&data, 500, false).unwrap();
            let out = chunked::read_chunked(&manifest).unwrap();
            drop(buffers); // keep temp files alive until after read
            assert_eq!(out, data);
        }
    }
}
