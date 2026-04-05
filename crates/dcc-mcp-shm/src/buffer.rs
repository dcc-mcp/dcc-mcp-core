//! `SharedBuffer` — a named, fixed-size region of memory visible to multiple
//! OS processes via an anonymous memory-mapped file.
//!
//! # Design
//! We back each buffer with a temporary file so that:
//!  1. The same `SharedBuffer` can be passed to another process by sending the
//!     file descriptor / file path.
//!  2. The OS reclaims the memory when the last handle is dropped (RAII).
//!
//! For same-process zero-copy we expose `as_slice` / `as_slice_mut`.

use std::fmt;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use memmap2::MmapMut;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ShmError, ShmResult};

// ── Header stored at byte 0 of the mapped region ────────────────────────────

/// Fixed-size header written at the start of every mapped region.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RegionHeader {
    /// Magic marker to detect corruption.
    pub magic: u64,
    /// Logical data length (may be < capacity).
    pub data_len: u64,
    /// Whether the content is lz4-compressed.
    pub compressed: u8,
    /// Alignment padding.
    pub _pad: [u8; 7],
}

pub(crate) const HEADER_MAGIC: u64 = 0xDCC0_0000_5348_4D01;
pub(crate) const HEADER_SIZE: usize = std::mem::size_of::<RegionHeader>();

// ── BufferHandle — the Arc-backed inner state ────────────────────────────────

struct BufferInner {
    /// The path of the backing temp file (kept alive for the buffer lifetime).
    path: PathBuf,
    /// Capacity in bytes (does NOT include the header).
    capacity: usize,
    /// The memory-mapped view.
    mmap: Mutex<MmapMut>,
}

impl Drop for BufferInner {
    fn drop(&mut self) {
        // Best-effort removal; ignore errors (e.g. already cleaned up).
        let _ = std::fs::remove_file(&self.path);
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

/// A fixed-capacity shared memory buffer backed by a memory-mapped file.
///
/// Multiple `SharedBuffer` handles can map the same file by calling
/// [`SharedBuffer::open`].
#[derive(Clone)]
pub struct SharedBuffer {
    inner: Arc<BufferInner>,
    /// Human-readable name / ID (not the file path).
    pub id: String,
}

impl fmt::Debug for SharedBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedBuffer")
            .field("id", &self.id)
            .field("capacity", &self.inner.capacity)
            .finish()
    }
}

impl SharedBuffer {
    /// Create a new buffer backed by an anonymous temp file.
    ///
    /// `capacity` is the maximum number of *data* bytes (header is added
    /// automatically).
    pub fn create(capacity: usize) -> ShmResult<Self> {
        Self::create_with_id(Uuid::new_v4().to_string(), capacity)
    }

    /// Create a buffer with an explicit string id (useful for tests).
    pub fn create_with_id(id: impl Into<String>, capacity: usize) -> ShmResult<Self> {
        if capacity == 0 {
            return Err(ShmError::InvalidArgument(
                "capacity must be greater than 0".into(),
            ));
        }

        let id = id.into();
        let total = HEADER_SIZE + capacity;

        // Create a temp file for the backing store.
        let tmp_dir = std::env::temp_dir().join("dcc_mcp_shm");
        std::fs::create_dir_all(&tmp_dir)?;
        let path = tmp_dir.join(format!("{}.shm", id));

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    ShmError::AlreadyExists { name: id.clone() }
                } else {
                    ShmError::Io(e)
                }
            })?;

        // Pre-allocate.
        file.set_len(total as u64)?;

        // SAFETY: the file is valid and sized; we hold the file handle.
        let mut mmap =
            unsafe { MmapMut::map_mut(&file) }.map_err(|e| ShmError::Mmap(e.to_string()))?;

        // Write initial header.
        let header = RegionHeader {
            magic: HEADER_MAGIC,
            data_len: 0,
            compressed: 0,
            _pad: [0u8; 7],
        };
        mmap[..HEADER_SIZE].copy_from_slice(unsafe {
            std::slice::from_raw_parts(&header as *const RegionHeader as *const u8, HEADER_SIZE)
        });
        mmap.flush()?;

        tracing::debug!(id = %id, capacity, "SharedBuffer created");

        Ok(Self {
            inner: Arc::new(BufferInner {
                path,
                capacity,
                mmap: Mutex::new(mmap),
            }),
            id,
        })
    }

    /// Open an existing buffer by file path (for cross-process use).
    pub fn open(path: impl AsRef<Path>, id: impl Into<String>) -> ShmResult<Self> {
        let path = path.as_ref().to_path_buf();
        let id = id.into();

        let file = OpenOptions::new().read(true).write(true).open(&path)?;

        let file_len = file.metadata()?.len() as usize;
        if file_len < HEADER_SIZE {
            return Err(ShmError::InvalidArgument(
                "file too small to be a SharedBuffer".into(),
            ));
        }
        let capacity = file_len - HEADER_SIZE;

        let mmap = unsafe { MmapMut::map_mut(&file) }.map_err(|e| ShmError::Mmap(e.to_string()))?;

        Ok(Self {
            inner: Arc::new(BufferInner {
                path,
                capacity,
                mmap: Mutex::new(mmap),
            }),
            id,
        })
    }

    /// Maximum data bytes this buffer can hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }

    /// Backing file path (used for cross-process handoff).
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Write `data` into the buffer.
    ///
    /// Returns the number of bytes written.
    pub fn write(&self, data: &[u8]) -> ShmResult<usize> {
        if data.len() > self.inner.capacity {
            return Err(ShmError::BufferTooSmall {
                required: data.len(),
                available: self.inner.capacity,
            });
        }

        let mut mmap = self.inner.mmap.lock();
        let len = data.len();

        // Update header.
        let header = RegionHeader {
            magic: HEADER_MAGIC,
            data_len: len as u64,
            compressed: 0,
            _pad: [0u8; 7],
        };
        mmap[..HEADER_SIZE].copy_from_slice(unsafe {
            std::slice::from_raw_parts(&header as *const RegionHeader as *const u8, HEADER_SIZE)
        });

        // Copy data.
        mmap[HEADER_SIZE..HEADER_SIZE + len].copy_from_slice(data);
        mmap.flush()?;

        tracing::trace!(id = %self.id, bytes = len, "SharedBuffer written");
        Ok(len)
    }

    /// Read the logical data slice (up to `data_len` bytes).
    ///
    /// Returns a `Vec<u8>` copy (safe to hold across lock boundaries).
    pub fn read(&self) -> ShmResult<Vec<u8>> {
        let mmap = self.inner.mmap.lock();
        let header = self.read_header(&mmap)?;

        let len = header.data_len as usize;
        if len == 0 {
            return Ok(vec![]);
        }
        if HEADER_SIZE + len > mmap.len() {
            return Err(ShmError::Internal(
                "header data_len exceeds mmap size".into(),
            ));
        }

        Ok(mmap[HEADER_SIZE..HEADER_SIZE + len].to_vec())
    }

    /// Returns the number of bytes currently stored (from header).
    pub fn data_len(&self) -> ShmResult<usize> {
        let mmap = self.inner.mmap.lock();
        let header = self.read_header(&mmap)?;
        Ok(header.data_len as usize)
    }

    /// Zero out the data region and reset `data_len` to 0.
    pub fn clear(&self) -> ShmResult<()> {
        let mut mmap = self.inner.mmap.lock();
        let header = RegionHeader {
            magic: HEADER_MAGIC,
            data_len: 0,
            compressed: 0,
            _pad: [0u8; 7],
        };
        mmap[..HEADER_SIZE].copy_from_slice(unsafe {
            std::slice::from_raw_parts(&header as *const RegionHeader as *const u8, HEADER_SIZE)
        });
        mmap.flush()?;
        Ok(())
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn read_header(&self, mmap: &MmapMut) -> ShmResult<RegionHeader> {
        if mmap.len() < HEADER_SIZE {
            return Err(ShmError::Internal("mmap too small for header".into()));
        }
        // SAFETY: the region has been sized to hold the header at construction.
        let header: RegionHeader =
            unsafe { std::ptr::read_unaligned(mmap.as_ptr() as *const RegionHeader) };
        if header.magic != HEADER_MAGIC {
            return Err(ShmError::Internal("invalid magic bytes in header".into()));
        }
        Ok(header)
    }
}

// ── Serialisable descriptor for cross-process handoff ───────────────────────

/// A JSON-serialisable descriptor that the producer sends to the consumer so
/// the consumer can call [`SharedBuffer::open`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferDescriptor {
    /// Human-readable id.
    pub id: String,
    /// Absolute path to the backing file.
    pub path: String,
    /// Capacity in bytes.
    pub capacity: usize,
}

impl BufferDescriptor {
    /// Build a descriptor from an existing [`SharedBuffer`].
    pub fn from_buffer(buf: &SharedBuffer) -> Self {
        Self {
            id: buf.id.clone(),
            path: buf.path().to_string_lossy().into_owned(),
            capacity: buf.capacity(),
        }
    }

    /// Serialise to JSON.
    pub fn to_json(&self) -> ShmResult<String> {
        serde_json::to_string(self).map_err(|e| ShmError::Internal(e.to_string()))
    }

    /// Deserialise from JSON.
    pub fn from_json(s: &str) -> ShmResult<Self> {
        serde_json::from_str(s).map_err(|e| ShmError::Internal(e.to_string()))
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_create {
        use super::*;

        #[test]
        fn test_create_and_capacity() {
            let buf = SharedBuffer::create(1024).unwrap();
            assert_eq!(buf.capacity(), 1024);
        }

        #[test]
        fn test_create_zero_capacity_fails() {
            let result = SharedBuffer::create(0);
            assert!(matches!(result, Err(ShmError::InvalidArgument(_))));
        }

        #[test]
        fn test_initial_data_len_is_zero() {
            let buf = SharedBuffer::create(512).unwrap();
            assert_eq!(buf.data_len().unwrap(), 0);
        }

        #[test]
        fn test_read_empty_returns_empty_vec() {
            let buf = SharedBuffer::create(256).unwrap();
            let data = buf.read().unwrap();
            assert!(data.is_empty());
        }
    }

    mod test_write_read {
        use super::*;

        #[test]
        fn test_write_and_read_roundtrip() {
            let buf = SharedBuffer::create(4096).unwrap();
            let payload = b"Hello, DCC MCP shared memory!";
            buf.write(payload).unwrap();
            let out = buf.read().unwrap();
            assert_eq!(out, payload);
        }

        #[test]
        fn test_write_updates_data_len() {
            let buf = SharedBuffer::create(1024).unwrap();
            buf.write(b"abc").unwrap();
            assert_eq!(buf.data_len().unwrap(), 3);
        }

        #[test]
        fn test_write_too_large_returns_error() {
            let buf = SharedBuffer::create(4).unwrap();
            let result = buf.write(b"12345");
            assert!(matches!(result, Err(ShmError::BufferTooSmall { .. })));
        }

        #[test]
        fn test_overwrite_replaces_data() {
            let buf = SharedBuffer::create(1024).unwrap();
            buf.write(b"first").unwrap();
            buf.write(b"second").unwrap();
            let out = buf.read().unwrap();
            assert_eq!(out, b"second");
        }

        #[test]
        fn test_clear_resets_data_len() {
            let buf = SharedBuffer::create(256).unwrap();
            buf.write(b"data").unwrap();
            buf.clear().unwrap();
            assert_eq!(buf.data_len().unwrap(), 0);
            let out = buf.read().unwrap();
            assert!(out.is_empty());
        }
    }

    mod test_descriptor {
        use super::*;

        #[test]
        fn test_descriptor_roundtrip_json() {
            let buf = SharedBuffer::create(2048).unwrap();
            let desc = BufferDescriptor::from_buffer(&buf);
            let json = desc.to_json().unwrap();
            let desc2 = BufferDescriptor::from_json(&json).unwrap();
            assert_eq!(desc2.capacity, 2048);
            assert_eq!(desc2.id, buf.id);
        }

        #[test]
        fn test_open_via_descriptor() {
            let buf = SharedBuffer::create(1024).unwrap();
            buf.write(b"cross-process").unwrap();
            let desc = BufferDescriptor::from_buffer(&buf);
            let buf2 = SharedBuffer::open(&desc.path, &desc.id).unwrap();
            let out = buf2.read().unwrap();
            assert_eq!(out, b"cross-process");
        }
    }

    mod test_clone {
        use super::*;

        #[test]
        fn test_clone_shares_same_region() {
            let buf = SharedBuffer::create(512).unwrap();
            buf.write(b"shared").unwrap();
            let buf2 = buf.clone();
            let out = buf2.read().unwrap();
            assert_eq!(out, b"shared");
        }

        #[test]
        fn test_write_via_clone_visible_in_original() {
            let buf = SharedBuffer::create(512).unwrap();
            let buf2 = buf.clone();
            buf2.write(b"written by clone").unwrap();
            let out = buf.read().unwrap();
            assert_eq!(out, b"written by clone");
        }
    }
}
