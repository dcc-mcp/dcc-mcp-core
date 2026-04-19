//! `BufferPool` — a fixed-capacity pool of pre-allocated [`SharedBuffer`]s.
//!
//! # Purpose
//! Allocating a new shared-memory region involves creating an ipckit segment
//! and writing the header.  For high-frequency use-cases (e.g.
//! 30 fps scene snapshots) repeated allocation is expensive.  A pool amortises
//! this cost by recycling buffers.
//!
//! # Usage
//! ```rust,no_run
//! use dcc_mcp_shm::pool::BufferPool;
//!
//! let pool = BufferPool::new(4, 1024 * 1024).unwrap(); // 4 × 1 MiB
//! let guard = pool.acquire().unwrap();
//! guard.write(b"scene data").unwrap();
//! // `guard` auto-returns on Drop
//! ```

use std::sync::Arc;

use parking_lot::Mutex;

use crate::buffer::SharedBuffer;
use crate::error::{ShmError, ShmResult};

/// A single slot in the pool.
struct Slot {
    buffer: SharedBuffer,
    in_use: bool,
}

/// A pooled buffer handle.  The buffer is automatically returned to the pool
/// when this guard is dropped.
pub struct PooledBuffer {
    /// Arc to the pool — kept alive until guard is dropped.
    pool: Arc<Mutex<Vec<Slot>>>,
    /// Index of our slot.
    index: usize,
    /// Clone of the inner buffer for caller use.
    pub buffer: SharedBuffer,
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        let mut slots = self.pool.lock();
        if let Some(slot) = slots.get_mut(self.index) {
            // Clear data and mark as free.
            let _ = slot.buffer.clear();
            slot.in_use = false;
        }
        tracing::trace!(index = self.index, "PooledBuffer returned to pool");
    }
}

impl std::ops::Deref for PooledBuffer {
    type Target = SharedBuffer;
    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}

/// A fixed-capacity pool of reusable [`SharedBuffer`]s.
pub struct BufferPool {
    slots: Arc<Mutex<Vec<Slot>>>,
    capacity: usize,
    buffer_size: usize,
}

impl BufferPool {
    /// Create a new pool with `capacity` slots, each holding `buffer_size` bytes.
    pub fn new(capacity: usize, buffer_size: usize) -> ShmResult<Self> {
        if capacity == 0 {
            return Err(ShmError::InvalidArgument(
                "pool capacity must be > 0".into(),
            ));
        }
        if buffer_size == 0 {
            return Err(ShmError::InvalidArgument("buffer_size must be > 0".into()));
        }

        let mut slots = Vec::with_capacity(capacity);
        for i in 0..capacity {
            let id = format!("pool-{}-{}", uuid::Uuid::new_v4(), i);
            let buffer = SharedBuffer::create_with_id(id, buffer_size)?;
            slots.push(Slot {
                buffer,
                in_use: false,
            });
        }

        tracing::debug!(capacity, buffer_size, "BufferPool created");

        Ok(Self {
            slots: Arc::new(Mutex::new(slots)),
            capacity,
            buffer_size,
        })
    }

    /// Try to acquire a free buffer from the pool.
    ///
    /// Returns `Err(ShmError::PoolExhausted)` if all slots are in use.
    pub fn acquire(&self) -> ShmResult<PooledBuffer> {
        let mut slots = self.slots.lock();
        for (i, slot) in slots.iter_mut().enumerate() {
            if !slot.in_use {
                slot.in_use = true;
                let buffer = slot.buffer.clone();
                tracing::trace!(index = i, "PooledBuffer acquired");
                return Ok(PooledBuffer {
                    pool: Arc::clone(&self.slots),
                    index: i,
                    buffer,
                });
            }
        }
        Err(ShmError::PoolExhausted {
            capacity: self.capacity,
        })
    }

    /// Number of slots currently free.
    pub fn available(&self) -> usize {
        let slots = self.slots.lock();
        slots.iter().filter(|s| !s.in_use).count()
    }

    /// Total number of slots.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Per-buffer capacity in bytes.
    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_creation {
        use super::*;

        #[test]
        fn test_pool_capacity() {
            let pool = BufferPool::new(3, 512).unwrap();
            assert_eq!(pool.capacity(), 3);
            assert_eq!(pool.available(), 3);
        }

        #[test]
        fn test_zero_capacity_fails() {
            assert!(matches!(
                BufferPool::new(0, 512),
                Err(ShmError::InvalidArgument(_))
            ));
        }

        #[test]
        fn test_zero_buffer_size_fails() {
            assert!(matches!(
                BufferPool::new(2, 0),
                Err(ShmError::InvalidArgument(_))
            ));
        }
    }

    mod test_acquire {
        use super::*;

        #[test]
        fn test_acquire_decrements_available() {
            let pool = BufferPool::new(2, 256).unwrap();
            let _g1 = pool.acquire().unwrap();
            assert_eq!(pool.available(), 1);
            let _g2 = pool.acquire().unwrap();
            assert_eq!(pool.available(), 0);
        }

        #[test]
        fn test_pool_exhausted_returns_error() {
            let pool = BufferPool::new(1, 128).unwrap();
            let _g = pool.acquire().unwrap();
            let result = pool.acquire();
            assert!(matches!(result, Err(ShmError::PoolExhausted { .. })));
        }

        #[test]
        fn test_drop_returns_to_pool() {
            let pool = BufferPool::new(1, 256).unwrap();
            {
                let _g = pool.acquire().unwrap();
                assert_eq!(pool.available(), 0);
            } // dropped here
            assert_eq!(pool.available(), 1);
        }

        #[test]
        fn test_data_cleared_on_return() {
            let pool = BufferPool::new(1, 256).unwrap();
            {
                let guard = pool.acquire().unwrap();
                guard.write(b"sensitive").unwrap();
            }
            let guard2 = pool.acquire().unwrap();
            assert_eq!(guard2.data_len().unwrap(), 0);
        }
    }

    mod test_deref {
        use super::*;

        #[test]
        fn test_pooled_buffer_deref_write_read() {
            let pool = BufferPool::new(1, 1024).unwrap();
            let guard = pool.acquire().unwrap();
            guard.write(b"hello pool").unwrap();
            assert_eq!(guard.read().unwrap(), b"hello pool");
        }
    }
}
