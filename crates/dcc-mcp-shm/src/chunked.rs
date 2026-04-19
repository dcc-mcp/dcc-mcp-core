//! Chunked transfer for data that exceeds a single `SharedBuffer`'s capacity.
//!
//! # Protocol
//! Large payloads are split into fixed-size chunks, each stored in its own
//! `SharedBuffer`.  A `ChunkManifest` (JSON) describes the full transfer and
//! is exchanged via any side-channel (e.g. a small control message over IPC).
//!
//! # Chunk size
//! Default: 256 MiB — tunable per transfer.
//!
//! # Compression
//! Each chunk may be individually compressed with LZ4 (enabled via a flag).

use serde::{Deserialize, Serialize};

use crate::buffer::{BufferDescriptor, SharedBuffer, short_id};
use crate::compress;
use crate::error::{ShmError, ShmResult};

/// Default maximum chunk size (256 MiB).
pub const DEFAULT_CHUNK_SIZE: usize = 256 * 1024 * 1024;

/// Metadata for a single chunk within a transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    /// Zero-based index.
    pub index: usize,
    /// Buffer descriptor (path, id, capacity).
    pub descriptor: BufferDescriptor,
    /// Number of bytes written into this chunk (original, before compression).
    pub original_len: usize,
    /// Number of bytes stored (may be < original_len if compressed).
    pub stored_len: usize,
    /// Whether the data in the buffer is LZ4-compressed.
    pub compressed: bool,
}

/// Full manifest describing a multi-chunk transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    /// Transfer id (UUID).
    pub transfer_id: String,
    /// Original total byte count.
    pub total_bytes: usize,
    /// Ordered list of chunk descriptors.
    pub chunks: Vec<ChunkInfo>,
    /// Whether compression was requested.
    pub compression_enabled: bool,
}

impl ChunkManifest {
    /// Serialize to compact JSON.
    pub fn to_json(&self) -> ShmResult<String> {
        serde_json::to_string(self).map_err(|e| ShmError::Internal(e.to_string()))
    }

    /// Deserialise from JSON.
    pub fn from_json(s: &str) -> ShmResult<Self> {
        serde_json::from_str(s).map_err(|e| ShmError::Internal(e.to_string()))
    }
}

/// Split `data` into chunks, write each chunk into a fresh `SharedBuffer`,
/// and return the manifest.
///
/// # Arguments
/// * `data` — the full payload to transmit
/// * `chunk_size` — maximum bytes per chunk (default: 256 MiB)
/// * `compress` — if `true`, each chunk is LZ4-compressed before writing
pub fn write_chunked(
    data: &[u8],
    chunk_size: usize,
    use_compression: bool,
) -> ShmResult<(Vec<SharedBuffer>, ChunkManifest)> {
    if chunk_size == 0 {
        return Err(ShmError::InvalidArgument("chunk_size must be > 0".into()));
    }

    let transfer_id = short_id();
    let mut buffers: Vec<SharedBuffer> = Vec::new();
    let mut chunk_infos: Vec<ChunkInfo> = Vec::new();

    for (index, chunk) in data.chunks(chunk_size).enumerate() {
        let original_len = chunk.len();

        let (to_write, compressed) = if use_compression {
            let c = compress::compress(chunk)?;
            if compress::should_compress(original_len, c.len()) {
                (c, true)
            } else {
                (chunk.to_vec(), false)
            }
        } else {
            (chunk.to_vec(), false)
        };

        let stored_len = to_write.len();
        let buf = SharedBuffer::create(stored_len.max(1))?;
        buf.write(&to_write)?;

        let descriptor = BufferDescriptor::from_buffer(&buf)?;
        chunk_infos.push(ChunkInfo {
            index,
            descriptor,
            original_len,
            stored_len,
            compressed,
        });
        buffers.push(buf);
    }

    let manifest = ChunkManifest {
        transfer_id,
        total_bytes: data.len(),
        chunks: chunk_infos,
        compression_enabled: use_compression,
    };

    tracing::debug!(
        transfer_id = %manifest.transfer_id,
        total_bytes = data.len(),
        num_chunks = buffers.len(),
        "chunked write complete"
    );

    Ok((buffers, manifest))
}

/// Reassemble data from a manifest.
///
/// The caller must have access to the buffer files referenced in the manifest
/// (same process or same machine via a shared filesystem).
pub fn read_chunked(manifest: &ChunkManifest) -> ShmResult<Vec<u8>> {
    let mut result = Vec::with_capacity(manifest.total_bytes);

    for (i, chunk_info) in manifest.chunks.iter().enumerate() {
        if chunk_info.index != i {
            return Err(ShmError::ChunkOutOfRange {
                index: chunk_info.index,
                total: manifest.chunks.len(),
            });
        }

        let buf = SharedBuffer::open(&chunk_info.descriptor.name, &chunk_info.descriptor.id)?;
        let raw = buf.read()?;

        if chunk_info.compressed {
            let decompressed = compress::decompress(&raw)?;
            result.extend_from_slice(&decompressed);
        } else {
            result.extend_from_slice(&raw);
        }
    }

    if result.len() != manifest.total_bytes {
        return Err(ShmError::IncompleteChunks {
            received: result.len(),
            total: manifest.total_bytes,
        });
    }

    tracing::debug!(
        transfer_id = %manifest.transfer_id,
        total_bytes = result.len(),
        "chunked read complete"
    );

    Ok(result)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_write_chunked {
        use super::*;

        #[test]
        fn test_small_data_single_chunk() {
            let data = b"hello world";
            let (buffers, manifest) = write_chunked(data, DEFAULT_CHUNK_SIZE, false).unwrap();
            assert_eq!(buffers.len(), 1);
            assert_eq!(manifest.chunks.len(), 1);
            assert_eq!(manifest.total_bytes, data.len());
        }

        #[test]
        fn test_multi_chunk_split() {
            let data: Vec<u8> = (0..250u8).cycle().take(1000).collect();
            let chunk_size = 300;
            let (buffers, manifest) = write_chunked(&data, chunk_size, false).unwrap();
            // 1000 / 300 = 3 full + 1 partial = 4 chunks
            assert_eq!(buffers.len(), 4);
            assert_eq!(manifest.chunks.len(), 4);
        }

        #[test]
        fn test_zero_chunk_size_fails() {
            let result = write_chunked(b"data", 0, false);
            assert!(matches!(result, Err(ShmError::InvalidArgument(_))));
        }

        #[test]
        fn test_empty_data_no_chunks() {
            let (buffers, manifest) = write_chunked(b"", 1024, false).unwrap();
            assert_eq!(buffers.len(), 0);
            assert_eq!(manifest.total_bytes, 0);
        }
    }

    mod test_read_chunked {
        use super::*;

        #[test]
        fn test_roundtrip_no_compression() {
            let data: Vec<u8> = (0..200).map(|i| (i % 256) as u8).collect();
            let (buffers, manifest) = write_chunked(&data, 64, false).unwrap();
            let out = read_chunked(&manifest).unwrap();
            drop(buffers); // keep alive until after read
            assert_eq!(out, data);
        }

        #[test]
        fn test_roundtrip_with_compression() {
            let data = vec![0xABu8; 4096]; // highly compressible
            let (buffers, manifest) = write_chunked(&data, 1024, true).unwrap();
            let out = read_chunked(&manifest).unwrap();
            drop(buffers);
            assert_eq!(out, data);
        }

        #[test]
        fn test_large_payload_multi_chunk_roundtrip() {
            let data: Vec<u8> = (0..10_000).map(|i| (i % 256) as u8).collect();
            let (buffers, manifest) = write_chunked(&data, 1024, false).unwrap();
            let out = read_chunked(&manifest).unwrap();
            drop(buffers);
            assert_eq!(out, data);
        }
    }

    mod test_manifest_json {
        use super::*;

        #[test]
        fn test_manifest_json_roundtrip() {
            let data = b"test manifest serialization";
            let (_, manifest) = write_chunked(data, 1024, false).unwrap();
            let json = manifest.to_json().unwrap();
            let manifest2 = ChunkManifest::from_json(&json).unwrap();
            assert_eq!(manifest2.total_bytes, manifest.total_bytes);
            assert_eq!(manifest2.transfer_id, manifest.transfer_id);
        }
    }
}
