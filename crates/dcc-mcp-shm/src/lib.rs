//! `dcc-mcp-shm` — Zero-copy shared memory transport for large DCC scene data.
//!
//! # Overview
//! DCC scene data (geometry, animation caches, framebuffers) can easily reach
//! gigabytes.  Python-only DCC MCP competitors transmit this data by
//! serialising and sending over TCP, which can take 10-30 s for a 1 GB scene.
//!
//! `dcc-mcp-shm` provides a **zero-copy** alternative: the DCC side writes
//! data directly into a memory-mapped file; the consumer reads from the same
//! mapped region without any copying or serialisation.
//!
//! # Modules
//! | Module | Purpose |
//! |--------|---------|
//! | [`buffer`] | `SharedBuffer` — single named region backed by a temp mmap file |
//! | [`compress`] | LZ4 frame compression/decompression helpers |
//! | [`pool`] | `BufferPool` — reusable pool of pre-allocated buffers |
//! | [`chunked`] | Chunked transfer for data > 256 MiB |
//! | [`scene`] | `SharedSceneBuffer` — user-facing high-level wrapper |
//! | [`python`] | PyO3 bindings (feature-gated) |
//! | [`error`] | `ShmError` and `ShmResult` |
//!
//! # Quick start
//! ```rust,no_run
//! use dcc_mcp_shm::scene::{SharedSceneBuffer, SceneDataKind};
//!
//! // DCC side: write scene data
//! let vertices: Vec<u8> = vec![0u8; 1024 * 1024]; // 1 MiB of vertex data
//! let ssb = SharedSceneBuffer::write(
//!     &vertices,
//!     SceneDataKind::Geometry,
//!     Some("Maya".to_string()),
//!     true, // LZ4 compression
//! )
//! .unwrap();
//!
//! // Send JSON descriptor to the Agent side via IPC…
//! let json = ssb.to_descriptor_json().unwrap();
//! println!("{}", json);
//!
//! // Agent side: read back the original bytes
//! let recovered = ssb.read().unwrap();
//! assert_eq!(recovered, vertices);
//! ```

pub mod buffer;
pub mod chunked;
pub mod compress;
pub mod error;
pub mod pool;
pub mod scene;

#[cfg(feature = "python-bindings")]
pub mod python;

// Re-export most-used types at crate root.
pub use buffer::{BufferDescriptor, SharedBuffer};
pub use chunked::{ChunkManifest, DEFAULT_CHUNK_SIZE};
pub use error::{ShmError, ShmResult};
pub use pool::BufferPool;
pub use scene::{SceneDataKind, SharedSceneBuffer};

#[cfg(feature = "python-bindings")]
pub use python::{PyBufferPool, PySceneDataKind, PySharedBuffer, PySharedSceneBuffer};
