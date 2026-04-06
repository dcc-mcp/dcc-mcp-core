//! Error types for the dcc-mcp-shm crate.

use std::io;
use thiserror::Error;

/// All errors produced by this crate.
#[derive(Debug, Error)]
pub enum ShmError {
    /// The requested shared memory region name is already in use.
    #[error("shared memory region '{name}' already exists")]
    AlreadyExists { name: String },

    /// The requested shared memory region was not found.
    #[error("shared memory region '{name}' not found")]
    NotFound { name: String },

    /// The provided buffer is too small for the requested operation.
    #[error("buffer too small: need {required} bytes, have {available}")]
    BufferTooSmall { required: usize, available: usize },

    /// Data exceeds the maximum allowed size.
    #[error("data size {size} exceeds maximum {max}")]
    DataTooLarge { size: usize, max: usize },

    /// Chunk index out of range during chunked transfer.
    #[error("chunk index {index} out of range (total {total})")]
    ChunkOutOfRange { index: usize, total: usize },

    /// The chunk sequence is incomplete (some chunks missing).
    #[error("incomplete chunk sequence: received {received} of {total} chunks")]
    IncompleteChunks { received: usize, total: usize },

    /// Compression error.
    #[error("compression failed: {0}")]
    CompressionError(String),

    /// Decompression error.
    #[error("decompression failed: {0}")]
    DecompressionError(String),

    /// I/O error (wraps [`std::io::Error`]).
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Memory-mapping error.
    #[error("mmap error: {0}")]
    Mmap(String),

    /// Pool has no available buffers.
    #[error("buffer pool exhausted (capacity {capacity})")]
    PoolExhausted { capacity: usize },

    /// Invalid argument supplied by the caller.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// Internal / unexpected error.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Convenience result alias.
pub type ShmResult<T> = Result<T, ShmError>;

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_display {
        use super::*;

        #[test]
        fn already_exists_display() {
            let err = ShmError::AlreadyExists {
                name: "scene_buf".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("scene_buf"), "{s}");
            assert!(s.contains("already exists"), "{s}");
        }

        #[test]
        fn not_found_display() {
            let err = ShmError::NotFound {
                name: "missing_buf".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("missing_buf"), "{s}");
        }

        #[test]
        fn buffer_too_small_display() {
            let err = ShmError::BufferTooSmall {
                required: 1024,
                available: 512,
            };
            let s = err.to_string();
            assert!(s.contains("1024"), "{s}");
            assert!(s.contains("512"), "{s}");
        }

        #[test]
        fn data_too_large_display() {
            let err = ShmError::DataTooLarge {
                size: 2_000_000,
                max: 1_000_000,
            };
            let s = err.to_string();
            assert!(s.contains("2000000"), "{s}");
            assert!(s.contains("1000000"), "{s}");
        }

        #[test]
        fn chunk_out_of_range_display() {
            let err = ShmError::ChunkOutOfRange { index: 5, total: 3 };
            let s = err.to_string();
            assert!(s.contains('5'), "{s}");
            assert!(s.contains('3'), "{s}");
        }

        #[test]
        fn incomplete_chunks_display() {
            let err = ShmError::IncompleteChunks {
                received: 4,
                total: 10,
            };
            let s = err.to_string();
            assert!(s.contains('4'), "{s}");
            assert!(s.contains("10"), "{s}");
        }

        #[test]
        fn compression_error_display() {
            let err = ShmError::CompressionError("lz4 failed".to_string());
            let s = err.to_string();
            assert!(s.contains("lz4 failed"), "{s}");
        }

        #[test]
        fn decompression_error_display() {
            let err = ShmError::DecompressionError("corrupt data".to_string());
            let s = err.to_string();
            assert!(s.contains("corrupt data"), "{s}");
        }

        #[test]
        fn io_display() {
            let io_err = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof");
            let err = ShmError::Io(io_err);
            let s = err.to_string();
            assert!(s.contains("eof"), "{s}");
        }

        #[test]
        fn mmap_display() {
            let err = ShmError::Mmap("mmap failed".to_string());
            let s = err.to_string();
            assert!(s.contains("mmap failed"), "{s}");
        }

        #[test]
        fn pool_exhausted_display() {
            let err = ShmError::PoolExhausted { capacity: 8 };
            let s = err.to_string();
            assert!(s.contains('8'), "{s}");
        }

        #[test]
        fn invalid_argument_display() {
            let err = ShmError::InvalidArgument("size must be > 0".to_string());
            let s = err.to_string();
            assert!(s.contains("size must be > 0"), "{s}");
        }

        #[test]
        fn internal_display() {
            let err = ShmError::Internal("invariant violated".to_string());
            let s = err.to_string();
            assert!(s.contains("invariant violated"), "{s}");
        }
    }

    mod test_from {
        use super::*;

        #[test]
        fn from_io_error() {
            let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
            let err: ShmError = io_err.into();
            assert!(matches!(err, ShmError::Io(_)));
        }
    }

    mod test_debug {
        use super::*;

        #[test]
        fn all_variants_are_debug() {
            let variants: Vec<ShmError> = vec![
                ShmError::AlreadyExists {
                    name: "n".to_string(),
                },
                ShmError::NotFound {
                    name: "n".to_string(),
                },
                ShmError::BufferTooSmall {
                    required: 1,
                    available: 0,
                },
                ShmError::DataTooLarge { size: 2, max: 1 },
                ShmError::ChunkOutOfRange { index: 0, total: 0 },
                ShmError::IncompleteChunks {
                    received: 1,
                    total: 2,
                },
                ShmError::CompressionError("c".to_string()),
                ShmError::DecompressionError("d".to_string()),
                ShmError::Io(std::io::Error::other("e")),
                ShmError::Mmap("m".to_string()),
                ShmError::PoolExhausted { capacity: 4 },
                ShmError::InvalidArgument("a".to_string()),
                ShmError::Internal("i".to_string()),
            ];
            for v in &variants {
                assert!(!format!("{v:?}").is_empty());
            }
        }
    }
}
