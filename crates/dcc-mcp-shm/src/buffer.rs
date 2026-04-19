//! `SharedBuffer` — a named, fixed-size region of memory visible to multiple
//! OS processes via ipckit's OS-native shared memory.
//!
//! # Design
//! We back each buffer with an ipckit `SharedMemory` region (POSIX `shm_open`
//! on Unix, `CreateFileMappingW` on Windows) so that:
//!  1. The same `SharedBuffer` can be opened by another process using the
//!     segment name.
//!  2. The OS reclaims the memory when the last handle is dropped (RAII).
//!  3. On Windows the kernel reference-counts named file-mappings; on Unix
//!     the owner's Drop calls `shm_unlink`.
//!
//! # Header layout
//! ```text
//! Offset  Size  Content
//! 0       8     magic (0xDCC0_0000_5348_4D01)
//! 8       8     data_len (logical data length)
//! 16      8     capacity (max data bytes, excluding header)
//! 24      8     created_at_secs (UNIX timestamp)
//! 32      8     ttl_secs (0 = no TTL)
//! 40      1     compressed flag
//! 41      7     reserved / alignment padding
//! 48      ..    User data (up to `capacity` bytes)
//! ```
//!
//! # Orphan GC
//! DCC applications can crash at any time, leaving shared memory segments
//! behind.  Call [`gc_orphans`] at startup (and optionally in an idle loop)
//! to scan for and remove stale segments whose TTL has expired.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ipckit::SharedMemory;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ShmError, ShmResult};

// ── Header stored at byte 0 of the shared region ────────────────────────────

/// Fixed-size header written at the start of every shared region.
///
/// Total size: 48 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RegionHeader {
    /// Magic marker to detect corruption (0xDCC0_0000_5348_4D01).
    pub magic: u64,
    /// Logical data length (may be < capacity).
    pub data_len: u64,
    /// Capacity in bytes (excludes header) — stored so `open` can recover
    /// the original value even when the OS rounds up the mapping size.
    pub capacity: u64,
    /// Creation timestamp (seconds since UNIX epoch).
    pub created_at_secs: u64,
    /// Time-to-live in seconds (0 = no TTL / never expires).
    pub ttl_secs: u64,
    /// Whether the content is lz4-compressed.
    pub compressed: u8,
    /// Alignment padding.
    pub _pad: [u8; 7],
}

pub(crate) const HEADER_MAGIC: u64 = 0xDCC0_0000_5348_4D01;
pub(crate) const HEADER_SIZE: usize = std::mem::size_of::<RegionHeader>();

/// Prefix used for all ipckit segment names created by dcc-mcp-shm.
const SEGMENT_PREFIX: &str = "ds_";

/// Maximum length for a POSIX shared-memory name (macOS limit is ~31 bytes).
const MAX_SEGMENT_NAME_LEN: usize = 31;

/// Build a segment name from an id, ensuring it stays within POSIX limits.
///
/// If `prefix + id` exceeds `MAX_SEGMENT_NAME_LEN`, the id is truncated.
fn segment_name(id: &str) -> String {
    let max_id_len = MAX_SEGMENT_NAME_LEN.saturating_sub(SEGMENT_PREFIX.len());
    if id.len() <= max_id_len {
        format!("{SEGMENT_PREFIX}{id}")
    } else {
        format!("{SEGMENT_PREFIX}{}", &id[..max_id_len])
    }
}

/// Generate a short random id suitable for use as a segment name component.
///
/// Returns the first 16 hex characters of a UUID v4 (64 bits of randomness),
/// which keeps the total segment name well under the macOS 31-byte limit.
fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].to_string()
}

// ── BufferHandle — the Arc-backed inner state ────────────────────────────────

struct BufferInner {
    /// The ipckit shared memory region.
    shm: Mutex<SharedMemory>,
    /// Capacity in bytes (does NOT include the header).
    capacity: usize,
}

// No custom Drop needed — ipckit::SharedMemory handles OS cleanup on Drop.

// ── Public API ───────────────────────────────────────────────────────────────

/// A fixed-capacity shared memory buffer backed by an ipckit shared segment.
///
/// Multiple `SharedBuffer` handles can map the same segment by calling
/// [`SharedBuffer::open`].
#[derive(Clone)]
pub struct SharedBuffer {
    inner: Arc<BufferInner>,
    /// Human-readable name / ID (also part of the ipckit segment name).
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
    /// Create a new buffer backed by an ipckit shared memory segment.
    ///
    /// `capacity` is the maximum number of *data* bytes (header is added
    /// automatically).
    pub fn create(capacity: usize) -> ShmResult<Self> {
        Self::create_with_ttl(short_id(), capacity, None)
    }

    /// Create a buffer with an explicit string id (used as the ipckit segment name).
    pub fn create_with_id(id: impl Into<String>, capacity: usize) -> ShmResult<Self> {
        Self::create_with_ttl(id, capacity, None)
    }

    /// Create a buffer with an optional time-to-live.
    ///
    /// When `ttl` is `Some`, the segment header stores the creation timestamp
    /// and TTL so that [`gc_orphans`] can detect and remove stale segments
    /// left behind by crashed DCC processes.
    pub fn create_with_ttl(
        id: impl Into<String>,
        capacity: usize,
        ttl: Option<Duration>,
    ) -> ShmResult<Self> {
        if capacity == 0 {
            return Err(ShmError::InvalidArgument(
                "capacity must be greater than 0".into(),
            ));
        }

        let id = id.into();
        let total = HEADER_SIZE + capacity;

        let seg_name = segment_name(&id);

        let mut shm = SharedMemory::create(&seg_name, total).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("already exists") || msg.contains("AlreadyExists") {
                ShmError::AlreadyExists { name: id.clone() }
            } else {
                ShmError::Internal(format!("SharedMemory::create failed: {msg}"))
            }
        })?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let header = RegionHeader {
            magic: HEADER_MAGIC,
            data_len: 0,
            capacity: capacity as u64,
            created_at_secs: now,
            ttl_secs: ttl.map(|d| d.as_secs()).unwrap_or(0),
            compressed: 0,
            _pad: [0u8; 7],
        };
        let header_bytes = unsafe {
            std::slice::from_raw_parts(&header as *const RegionHeader as *const u8, HEADER_SIZE)
        };
        shm.write(0, header_bytes)
            .map_err(|e| ShmError::Internal(format!("header write failed: {e}")))?;

        tracing::debug!(id = %id, capacity, ttl_secs = header.ttl_secs, "SharedBuffer created");

        Ok(Self {
            inner: Arc::new(BufferInner {
                shm: Mutex::new(shm),
                capacity,
            }),
            id,
        })
    }

    /// Open an existing buffer by its ipckit segment name (for cross-process use).
    pub fn open(name: impl AsRef<str>, id: impl Into<String>) -> ShmResult<Self> {
        let name = name.as_ref().to_string();
        let id = id.into();

        let shm = SharedMemory::open(&name).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("not found") || msg.contains("NotFound") {
                ShmError::NotFound { name: name.clone() }
            } else {
                ShmError::Internal(format!("SharedMemory::open failed: {msg}"))
            }
        })?;

        let total = shm.size();
        if total < HEADER_SIZE {
            return Err(ShmError::InvalidArgument(
                "segment too small to be a SharedBuffer".into(),
            ));
        }

        // Read capacity from the header rather than computing from OS mapping
        // size (which may be page-aligned and larger than the original request).
        let header_bytes = shm
            .read(0, HEADER_SIZE)
            .map_err(|e| ShmError::Internal(format!("header read failed: {e}")))?;
        let header: RegionHeader =
            unsafe { std::ptr::read_unaligned(header_bytes.as_ptr() as *const RegionHeader) };
        if header.magic != HEADER_MAGIC {
            return Err(ShmError::Internal("invalid magic bytes in header".into()));
        }
        let capacity = header.capacity as usize;

        Ok(Self {
            inner: Arc::new(BufferInner {
                shm: Mutex::new(shm),
                capacity,
            }),
            id,
        })
    }

    /// Maximum data bytes this buffer can hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }

    /// Segment name used for cross-process handoff (ipckit name).
    pub fn name(&self) -> String {
        segment_name(&self.id)
    }

    /// Returns `true` if this buffer's TTL has expired.
    ///
    /// Buffers without a TTL (`ttl_secs == 0`) never expire.
    pub fn is_expired(&self) -> ShmResult<bool> {
        let shm = self.inner.shm.lock();
        let header = self.read_header(&shm)?;
        Ok(is_header_expired(&header))
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

        let mut shm = self.inner.shm.lock();
        let len = data.len();

        // Read existing header to preserve TTL fields.
        let existing = self.read_header_unlocked(&shm)?;
        let header = RegionHeader {
            magic: HEADER_MAGIC,
            data_len: len as u64,
            capacity: self.inner.capacity as u64,
            created_at_secs: existing.created_at_secs,
            ttl_secs: existing.ttl_secs,
            compressed: 0,
            _pad: [0u8; 7],
        };
        let header_bytes = unsafe {
            std::slice::from_raw_parts(&header as *const RegionHeader as *const u8, HEADER_SIZE)
        };
        shm.write(0, header_bytes)
            .map_err(|e| ShmError::Internal(format!("header write failed: {e}")))?;

        // Copy data.
        shm.write(HEADER_SIZE, data)
            .map_err(|e| ShmError::Internal(format!("data write failed: {e}")))?;

        tracing::trace!(id = %self.id, bytes = len, "SharedBuffer written");
        Ok(len)
    }

    /// Read the logical data slice (up to `data_len` bytes).
    ///
    /// Returns a `Vec<u8>` copy (safe to hold across lock boundaries).
    pub fn read(&self) -> ShmResult<Vec<u8>> {
        let shm = self.inner.shm.lock();
        let header = self.read_header(&shm)?;

        let len = header.data_len as usize;
        if len == 0 {
            return Ok(vec![]);
        }
        if HEADER_SIZE + len > shm.size() {
            return Err(ShmError::Internal(
                "header data_len exceeds shm size".into(),
            ));
        }

        shm.read(HEADER_SIZE, len)
            .map_err(|e| ShmError::Internal(format!("data read failed: {e}")))
    }

    /// Returns the number of bytes currently stored (from header).
    pub fn data_len(&self) -> ShmResult<usize> {
        let shm = self.inner.shm.lock();
        let header = self.read_header(&shm)?;
        Ok(header.data_len as usize)
    }

    /// Zero out the data region and reset `data_len` to 0.
    pub fn clear(&self) -> ShmResult<()> {
        let mut shm = self.inner.shm.lock();
        let existing = self.read_header_unlocked(&shm)?;
        let header = RegionHeader {
            magic: HEADER_MAGIC,
            data_len: 0,
            capacity: self.inner.capacity as u64,
            created_at_secs: existing.created_at_secs,
            ttl_secs: existing.ttl_secs,
            compressed: 0,
            _pad: [0u8; 7],
        };
        let header_bytes = unsafe {
            std::slice::from_raw_parts(&header as *const RegionHeader as *const u8, HEADER_SIZE)
        };
        shm.write(0, header_bytes)
            .map_err(|e| ShmError::Internal(format!("clear header write failed: {e}")))?;
        Ok(())
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn read_header(&self, shm: &SharedMemory) -> ShmResult<RegionHeader> {
        Self::read_header_from(shm)
    }

    fn read_header_unlocked(&self, shm: &SharedMemory) -> ShmResult<RegionHeader> {
        Self::read_header_from(shm)
    }

    fn read_header_from(shm: &SharedMemory) -> ShmResult<RegionHeader> {
        if shm.size() < HEADER_SIZE {
            return Err(ShmError::Internal("shm too small for header".into()));
        }
        let bytes = shm
            .read(0, HEADER_SIZE)
            .map_err(|e| ShmError::Internal(format!("header read failed: {e}")))?;
        // SAFETY: the region has been sized to hold the header at construction.
        let header: RegionHeader =
            unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const RegionHeader) };
        if header.magic != HEADER_MAGIC {
            return Err(ShmError::Internal("invalid magic bytes in header".into()));
        }
        Ok(header)
    }
}

// ── TTL helper ──────────────────────────────────────────────────────────────

fn is_header_expired(header: &RegionHeader) -> bool {
    if header.ttl_secs == 0 {
        return false;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(header.created_at_secs) > header.ttl_secs
}

// ── Orphan GC ───────────────────────────────────────────────────────────────

/// Scan the OS shared-memory namespace for `ds_*` segments whose TTL
/// has expired and remove them.
///
/// Returns the number of segments removed.
///
/// # Platform note
/// On **Unix**, the scan enumerates `/dev/shm` (Linux) or `/tmp` (macOS);
/// on **Windows** the kernel reference-counts named file-mappings — they
/// vanish when all handles close, so GC is a no-op.
///
/// Call this at startup and optionally in an idle timer.
pub fn gc_orphans(max_age: Duration) -> usize {
    #[cfg(target_os = "linux")]
    {
        gc_orphans_scan("/dev/shm", max_age)
    }
    #[cfg(target_os = "macos")]
    {
        gc_orphans_scan("/tmp", max_age)
    }
    #[cfg(windows)]
    {
        let _ = max_age;
        0
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = max_age;
        0
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn gc_orphans_scan(shm_dir: &str, max_age: Duration) -> usize {
    use std::ffi::CString;

    let dir = match std::fs::read_dir(shm_dir) {
        Ok(d) => d,
        Err(_) => return 0,
    };

    let now = SystemTime::now();
    let max_age_secs = max_age.as_secs();
    let mut removed = 0;

    for entry in dir.flatten() {
        let fname = match entry.file_name().to_str() {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Only inspect segments that look like ours.
        if !fname.starts_with(SEGMENT_PREFIX) {
            continue;
        }

        let shm = match SharedMemory::open(&fname) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Read and validate header.
        if shm.size() < HEADER_SIZE {
            continue;
        }
        let bytes = match shm.read(0, HEADER_SIZE) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let header: RegionHeader =
            unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const RegionHeader) };
        if header.magic != HEADER_MAGIC {
            continue;
        }

        // Check if expired via TTL.
        let expired_by_ttl = is_header_expired(&header);

        // Also check max_age: if the segment is older than max_age and has
        // no active holders (the OS refcount would keep us from opening it
        // if the owner were still alive on some platforms, but we use
        // creation-time as a heuristic).
        let age = now
            .duration_since(UNIX_EPOCH + Duration::from_secs(header.created_at_secs))
            .unwrap_or_default();

        let expired_by_age = header.ttl_secs == 0 && age.as_secs() > max_age_secs;

        if !expired_by_ttl && !expired_by_age {
            continue;
        }

        // Unlink
        #[cfg(unix)]
        {
            let link_name = if fname.starts_with('/') {
                fname.clone()
            } else {
                format!("/{fname}")
            };
            let c_name = match CString::new(link_name) {
                Ok(n) => n,
                Err(_) => continue,
            };
            unsafe {
                libc::shm_unlink(c_name.as_ptr());
            }
            removed += 1;
        }
    }

    removed
}

// ── Serialisable descriptor for cross-process handoff ───────────────────────

/// A JSON-serialisable descriptor that the producer sends to the consumer so
/// the consumer can call [`SharedBuffer::open`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferDescriptor {
    /// Human-readable id.
    pub id: String,
    /// ipckit segment name (used by `SharedBuffer::open`).
    pub name: String,
    /// Capacity in bytes.
    pub capacity: usize,
    /// Optional TTL in seconds (0 = no TTL).
    #[serde(default)]
    pub ttl_secs: u64,
}

impl BufferDescriptor {
    /// Build a descriptor from an existing [`SharedBuffer`].
    pub fn from_buffer(buf: &SharedBuffer) -> ShmResult<Self> {
        let shm = buf.inner.shm.lock();
        let header = buf.read_header(&shm)?;
        Ok(Self {
            id: buf.id.clone(),
            name: buf.name(),
            capacity: buf.capacity(),
            ttl_secs: header.ttl_secs,
        })
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
            let desc = BufferDescriptor::from_buffer(&buf).unwrap();
            let json = desc.to_json().unwrap();
            let desc2 = BufferDescriptor::from_json(&json).unwrap();
            assert_eq!(desc2.capacity, 2048);
            assert_eq!(desc2.id, buf.id);
        }

        #[test]
        fn test_open_via_descriptor() {
            let buf = SharedBuffer::create(1024).unwrap();
            buf.write(b"cross-process").unwrap();
            let desc = BufferDescriptor::from_buffer(&buf).unwrap();
            let buf2 = SharedBuffer::open(&desc.name, &desc.id).unwrap();
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

    mod test_ttl {
        use super::*;

        #[test]
        fn test_no_ttl_never_expired() {
            let buf = SharedBuffer::create(256).unwrap();
            assert!(!buf.is_expired().unwrap());
        }

        #[test]
        fn test_long_ttl_not_expired() {
            let buf = SharedBuffer::create_with_ttl(
                "ttl-long",
                256,
                Some(Duration::from_secs(3600)).into(),
            )
            .unwrap();
            assert!(!buf.is_expired().unwrap());
        }

        #[test]
        fn test_descriptor_contains_ttl() {
            let buf = SharedBuffer::create_with_ttl(
                "ttl-desc",
                256,
                Some(Duration::from_secs(30)).into(),
            )
            .unwrap();
            let desc = BufferDescriptor::from_buffer(&buf).unwrap();
            assert_eq!(desc.ttl_secs, 30);
        }

        #[test]
        fn test_descriptor_zero_ttl_when_none() {
            let buf = SharedBuffer::create(256).unwrap();
            let desc = BufferDescriptor::from_buffer(&buf).unwrap();
            assert_eq!(desc.ttl_secs, 0);
        }
    }

    mod test_gc {
        use super::*;

        #[test]
        fn test_gc_orphans_returns_usize() {
            // Just ensure the function compiles and returns without panic.
            let _ = gc_orphans(Duration::from_secs(60));
        }
    }
}
