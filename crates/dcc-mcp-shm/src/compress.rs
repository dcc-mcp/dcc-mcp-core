//! Compression helpers using the `lz4_flex` frame format.
//!
//! We use LZ4 frame format (not raw block) because it embeds the content
//! size, making decompression straightforward without an out-of-band length.

use lz4_flex::frame::{FrameDecoder, FrameEncoder};
use std::io::{Read, Write};

use crate::error::{ShmError, ShmResult};

/// Compress `data` with LZ4 frame encoding.
///
/// Returns the compressed bytes.
pub fn compress(data: &[u8]) -> ShmResult<Vec<u8>> {
    let mut encoder = FrameEncoder::new(Vec::new());
    encoder
        .write_all(data)
        .map_err(|e| ShmError::CompressionError(e.to_string()))?;
    encoder
        .finish()
        .map_err(|e| ShmError::CompressionError(e.to_string()))
}

/// Decompress an LZ4 frame-encoded buffer.
///
/// Returns the original (decompressed) bytes.
pub fn decompress(data: &[u8]) -> ShmResult<Vec<u8>> {
    let mut decoder = FrameDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| ShmError::DecompressionError(e.to_string()))?;
    Ok(out)
}

/// Decompress an LZ4 frame-encoded buffer with a maximum output byte count.
pub fn decompress_with_limit(data: &[u8], max_output_bytes: usize) -> ShmResult<Vec<u8>> {
    let mut decoder = FrameDecoder::new(data);
    let limit = u64::try_from(max_output_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut limited = decoder.by_ref().take(limit);
    let mut out = Vec::new();
    limited
        .read_to_end(&mut out)
        .map_err(|e| ShmError::DecompressionError(e.to_string()))?;
    if out.len() > max_output_bytes {
        return Err(ShmError::DecompressionError(format!(
            "decompressed payload exceeds max_bytes={max_output_bytes}"
        )));
    }
    Ok(out)
}

/// Ratio threshold: only keep the compressed form when it is smaller.
pub fn should_compress(original_len: usize, compressed_len: usize) -> bool {
    compressed_len < original_len
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_roundtrip {
        use super::*;

        #[test]
        fn test_compress_decompress_roundtrip() {
            let original = b"Hello, this is a test payload that will compress well! ".repeat(100);
            let compressed = compress(&original).unwrap();
            let decompressed = decompress(&compressed).unwrap();
            assert_eq!(decompressed, original);
        }

        #[test]
        fn test_empty_roundtrip() {
            let compressed = compress(&[]).unwrap();
            let decompressed = decompress(&compressed).unwrap();
            assert!(decompressed.is_empty());
        }

        #[test]
        fn test_single_byte_roundtrip() {
            let compressed = compress(&[42u8]).unwrap();
            let decompressed = decompress(&compressed).unwrap();
            assert_eq!(decompressed, &[42u8]);
        }
    }

    mod test_compression_ratio {
        use super::*;

        #[test]
        fn test_repetitive_data_compresses_well() {
            let data = vec![0u8; 65536];
            let compressed = compress(&data).unwrap();
            assert!(compressed.len() < data.len());
        }

        #[test]
        fn test_should_compress_logic() {
            assert!(should_compress(1000, 800));
            assert!(!should_compress(100, 150));
        }
    }

    mod test_invalid {
        use super::*;

        #[test]
        fn test_decompress_invalid_data_returns_error() {
            let result = decompress(b"not a valid lz4 frame at all");
            assert!(result.is_err());
        }

        #[test]
        fn test_decompress_with_limit_rejects_large_output() {
            let compressed = compress(&[7u8; 128]).unwrap();
            let result = decompress_with_limit(&compressed, 32);
            assert!(result.is_err());
        }
    }
}
