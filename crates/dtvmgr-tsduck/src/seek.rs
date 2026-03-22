//! TS file seek and chunk extraction.
//!
//! Extracts an aligned TS chunk from the middle of a recording file for
//! Amatsukaze-style recording target detection.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};

/// MPEG-TS packet size in bytes.
const TS_PACKET_SIZE: usize = 188;

/// Sync byte at the start of every TS packet.
const TS_SYNC_BYTE: u8 = 0x47;

/// Default chunk size to extract (10 MiB ≈ 4.7s at 17 Mbps).
pub const DEFAULT_CHUNK_SIZE: u64 = 10 * 1024 * 1024;

/// Minimum file size for mid-file seeking. Smaller files are read entirely.
const MIN_FILE_SIZE: u64 = 1024 * 1024;

/// Number of consecutive sync bytes (at 188-byte intervals) required to
/// confirm a valid packet boundary.
const SYNC_VERIFY_COUNT: usize = 8;

/// Maximum bytes to scan when searching for a packet boundary.
/// Must be large enough to hold one worst-case offset (187 bytes) plus
/// `SYNC_VERIFY_COUNT` full packets for verification.
const MAX_SCAN_BYTES: usize = TS_PACKET_SIZE.saturating_mul(SYNC_VERIFY_COUNT.saturating_add(2));

/// Extract an aligned TS chunk from the middle of a file.
///
/// For files smaller than [`MIN_FILE_SIZE`], returns the entire file contents.
/// For larger files, seeks to the midpoint, finds a valid packet boundary,
/// and extracts `chunk_size` bytes of aligned TS data. Any trailing
/// incomplete packet is trimmed.
///
/// # Errors
///
/// Returns an error if the file cannot be opened/read or no valid packet
/// boundary is found near the midpoint.
pub fn extract_middle_chunk(input_file: &Path, chunk_size: u64) -> Result<Vec<u8>> {
    let mut file = File::open(input_file)
        .with_context(|| format!("failed to open {}", input_file.display()))?;

    let file_size = file
        .metadata()
        .with_context(|| format!("failed to read metadata of {}", input_file.display()))?
        .len();

    if file_size == 0 {
        return Ok(Vec::new());
    }

    // Small files: read the entire content.
    if file_size < MIN_FILE_SIZE {
        let capacity = usize::try_from(file_size).unwrap_or(usize::MAX);
        let mut buf = Vec::with_capacity(capacity);
        file.read_to_end(&mut buf)
            .with_context(|| format!("failed to read {}", input_file.display()))?;
        return Ok(buf);
    }

    // Calculate the start offset centered on the midpoint.
    let mid = file_size.wrapping_div(2);
    let start = mid.saturating_sub(chunk_size.wrapping_div(2));

    // Seek and find a valid packet boundary.
    file.seek(SeekFrom::Start(start))
        .with_context(|| format!("failed to seek in {}", input_file.display()))?;

    let mut scan_buf = vec![0u8; MAX_SCAN_BYTES];
    let scan_read = file
        .read(&mut scan_buf)
        .with_context(|| format!("failed to read scan region in {}", input_file.display()))?;
    scan_buf.truncate(scan_read);

    let boundary_offset = find_packet_boundary(&scan_buf).with_context(|| {
        format!(
            "no valid TS packet boundary found in {}",
            input_file.display()
        )
    })?;

    let boundary_u64 = u64::try_from(boundary_offset).unwrap_or(u64::MAX);
    let aligned_start = start.saturating_add(boundary_u64);

    // Clamp read size to not exceed file end.
    let read_size = chunk_size.min(file_size.saturating_sub(aligned_start));

    file.seek(SeekFrom::Start(aligned_start)).with_context(|| {
        format!(
            "failed to seek to aligned position in {}",
            input_file.display()
        )
    })?;

    let buf_size = usize::try_from(read_size).unwrap_or(usize::MAX);
    let mut buf = vec![0u8; buf_size];
    let bytes_read = file
        .read(&mut buf)
        .with_context(|| format!("failed to read chunk from {}", input_file.display()))?;
    buf.truncate(bytes_read);

    // Trim trailing incomplete packet.
    let aligned_len = buf
        .len()
        .wrapping_div(TS_PACKET_SIZE)
        .saturating_mul(TS_PACKET_SIZE);
    buf.truncate(aligned_len);

    Ok(buf)
}

/// Find a valid TS packet boundary in a byte buffer.
///
/// Scans for `0x47` and verifies that sync bytes appear at every 188-byte
/// interval for [`SYNC_VERIFY_COUNT`] consecutive packets.
///
/// Returns the byte offset of the first valid boundary, or `None` if not found.
fn find_packet_boundary(buf: &[u8]) -> Option<usize> {
    let verify_span = TS_PACKET_SIZE.saturating_mul(SYNC_VERIFY_COUNT);

    for offset in 0..buf.len() {
        if buf.get(offset).copied() != Some(TS_SYNC_BYTE) {
            continue;
        }
        // Check if we have enough room for full verification.
        if offset.saturating_add(verify_span) > buf.len() {
            // Partial verification: accept if we can verify at least 2 packets.
            let available = buf
                .len()
                .saturating_sub(offset)
                .wrapping_div(TS_PACKET_SIZE);
            if available < 2 {
                continue;
            }
            let all_match = (1..available).all(|i| {
                let pos = offset.saturating_add(i.saturating_mul(TS_PACKET_SIZE));
                buf.get(pos).copied() == Some(TS_SYNC_BYTE)
            });
            if all_match {
                return Some(offset);
            }
            continue;
        }
        // Full verification.
        let all_match = (1..SYNC_VERIFY_COUNT).all(|i| {
            let pos = offset.saturating_add(i.saturating_mul(TS_PACKET_SIZE));
            buf.get(pos).copied() == Some(TS_SYNC_BYTE)
        });
        if all_match {
            return Some(offset);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::as_conversions,
        clippy::cast_possible_truncation
    )]

    use super::*;

    // ── find_packet_boundary ──

    #[test]
    fn test_find_boundary_at_start() {
        // Arrange — sync bytes at every 188-byte interval from offset 0
        let mut buf = vec![0u8; 188 * 10];
        for i in 0..10 {
            buf[i * 188] = 0x47;
        }

        // Act
        let offset = find_packet_boundary(&buf);

        // Assert
        assert_eq!(offset, Some(0));
    }

    #[test]
    fn test_find_boundary_with_offset() {
        // Arrange — valid boundary starts at byte 5
        let mut buf = vec![0u8; 5 + 188 * 10];
        for i in 0..10 {
            buf[5 + i * 188] = 0x47;
        }

        // Act
        let offset = find_packet_boundary(&buf);

        // Assert
        assert_eq!(offset, Some(5));
    }

    #[test]
    fn test_find_boundary_skips_false_sync() {
        // Arrange — false 0x47 at offset 0, real boundary at offset 3
        let mut buf = vec![0u8; 3 + 188 * 10];
        buf[0] = 0x47; // false positive
        for i in 0..10 {
            buf[3 + i * 188] = 0x47;
        }

        // Act
        let offset = find_packet_boundary(&buf);

        // Assert
        assert_eq!(offset, Some(3));
    }

    #[test]
    fn test_find_boundary_no_sync() {
        // Arrange — no sync bytes at all
        let buf = vec![0u8; 188 * 10];

        // Act
        let offset = find_packet_boundary(&buf);

        // Assert
        assert_eq!(offset, None);
    }

    #[test]
    fn test_find_boundary_short_buffer() {
        // Arrange — buffer too short for full verification but has 3 valid packets
        let mut buf = vec![0u8; 188 * 3];
        for i in 0..3 {
            buf[i * 188] = 0x47;
        }

        // Act
        let offset = find_packet_boundary(&buf);

        // Assert
        assert_eq!(offset, Some(0));
    }

    #[test]
    fn test_find_boundary_single_sync() {
        // Arrange — only one sync byte, not enough to verify
        let mut buf = vec![0u8; 100];
        buf[0] = 0x47;

        // Act
        let offset = find_packet_boundary(&buf);

        // Assert
        assert_eq!(offset, None);
    }

    // ── extract_middle_chunk ──

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_middle_chunk_small_file() {
        // Arrange — file smaller than MIN_FILE_SIZE, should return entire content
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small.ts");
        let data = vec![0xAA; 1000];
        std::fs::write(&path, &data).unwrap();

        // Act
        let chunk = extract_middle_chunk(&path, DEFAULT_CHUNK_SIZE).unwrap();

        // Assert
        assert_eq!(chunk.len(), 1000);
        assert_eq!(chunk, data);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_middle_chunk_empty_file() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.ts");
        std::fs::write(&path, b"").unwrap();

        // Act
        let chunk = extract_middle_chunk(&path, DEFAULT_CHUNK_SIZE).unwrap();

        // Assert
        assert!(chunk.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_middle_chunk_aligned_ts() {
        // Arrange — create a valid TS file with proper sync bytes
        let packet_count: usize = 10000; // ~1.8 MiB, above MIN_FILE_SIZE
        let mut data = vec![0u8; 188 * packet_count];
        for i in 0..packet_count {
            data[i * 188] = 0x47;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("aligned.ts");
        std::fs::write(&path, &data).unwrap();

        // Act — request a small chunk
        let chunk_size: u64 = 188 * 100; // 100 packets
        let chunk = extract_middle_chunk(&path, chunk_size).unwrap();

        // Assert
        assert!(chunk.len() <= chunk_size as usize);
        assert_eq!(chunk.len() % 188, 0); // aligned
        // All packet starts should be 0x47
        for i in 0..(chunk.len() / 188) {
            assert_eq!(chunk[i * 188], 0x47, "packet {i} missing sync byte");
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_middle_chunk_no_sync_bytes() {
        // Arrange — file above MIN_FILE_SIZE but no sync bytes
        let data = vec![0u8; 2 * 1024 * 1024];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nosync.ts");
        std::fs::write(&path, &data).unwrap();

        // Act
        let result = extract_middle_chunk(&path, DEFAULT_CHUNK_SIZE);

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("packet boundary"),
            "expected boundary error in: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_middle_chunk_file_not_found() {
        // Act
        let result = extract_middle_chunk(Path::new("/nonexistent/file.ts"), DEFAULT_CHUNK_SIZE);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn test_find_boundary_partial_verification_two_packets() {
        // Arrange — buffer with exactly 2 valid packets at offset 0, not enough for full verification
        let mut buf = vec![0u8; 188 * 2];
        buf[0] = 0x47;
        buf[188] = 0x47;

        // Act
        let offset = find_packet_boundary(&buf);

        // Assert — accepts partial verification with 2 packets
        assert_eq!(offset, Some(0));
    }

    #[test]
    fn test_find_boundary_partial_verification_false_then_real() {
        // Arrange — false 0x47 at 0, real boundary at 10 with only 2 packets
        let mut buf = vec![0u8; 10 + 188 * 2];
        buf[0] = 0x47; // false positive (no second sync at 188)
        buf[10] = 0x47;
        buf[10 + 188] = 0x47;

        // Act
        let offset = find_packet_boundary(&buf);

        // Assert
        assert_eq!(offset, Some(10));
    }

    #[test]
    fn test_find_boundary_empty_buffer() {
        // Act
        let offset = find_packet_boundary(&[]);

        // Assert
        assert_eq!(offset, None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_middle_chunk_large_request_clamped() {
        // Arrange — file is above MIN_FILE_SIZE but request a huge chunk
        let packet_count: usize = 10000;
        let mut data = vec![0u8; 188 * packet_count];
        for i in 0..packet_count {
            data[i * 188] = 0x47;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clamped.ts");
        std::fs::write(&path, &data).unwrap();

        // Act — request 100MB but file is ~1.8MB
        let chunk = extract_middle_chunk(&path, 100 * 1024 * 1024).unwrap();

        // Assert — chunk should not exceed file size and must be aligned
        assert!(chunk.len() <= data.len());
        assert_eq!(chunk.len() % 188, 0);
    }

    #[test]
    fn test_find_boundary_sync_at_end_insufficient() {
        // Arrange — single sync byte at the very end of buffer
        let mut buf = vec![0u8; 200];
        buf[199] = 0x47;

        // Act
        let offset = find_packet_boundary(&buf);

        // Assert — only 1 packet, not enough for verification
        assert_eq!(offset, None);
    }
}
