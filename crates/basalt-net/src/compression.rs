use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use basalt_types::{Decode, Encode, VarInt};

use crate::error::{Error, Result};

/// Compresses packet data using zlib if the uncompressed size meets the threshold.
///
/// The Minecraft compressed packet format is:
/// - `VarInt(data_length)` — uncompressed size, or 0 if below threshold
/// - `data` — zlib-compressed bytes if data_length > 0, raw bytes otherwise
///
/// This function returns the compressed frame (data_length + data), NOT
/// including the outer packet length prefix — that is added by the framing layer.
pub fn compress_packet(data: &[u8], threshold: usize) -> Result<Vec<u8>> {
    let mut result = Vec::new();

    if data.len() >= threshold {
        // Compress: write uncompressed length + zlib data
        VarInt(data.len() as i32)
            .encode(&mut result)
            .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;

        // Level 3 favors speed over ratio — better for game server latency
        let mut encoder = ZlibEncoder::new(&mut result, Compression::new(3));
        encoder.write_all(data).map_err(Error::Io)?;
        encoder.finish().map_err(Error::Io)?;
    } else {
        // Below threshold: write 0 + raw data
        VarInt(0)
            .encode(&mut result)
            .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;
        result.extend_from_slice(data);
    }

    Ok(result)
}

/// Decompresses packet data from the Minecraft compressed format.
///
/// Reads the `VarInt(data_length)` prefix:
/// - If 0, returns the remaining bytes as-is (uncompressed)
/// - If > 0, decompresses the remaining bytes using zlib and validates
///   that the result matches the declared uncompressed size
///
/// The input should be the compressed frame content (after the outer
/// packet length prefix has been stripped by the framing layer).
pub fn decompress_packet(data: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = data;
    let data_length = VarInt::decode(&mut cursor)
        .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;

    if data_length.0 == 0 {
        // Not compressed — return remaining bytes as-is
        return Ok(cursor.to_vec());
    }

    let uncompressed_size = data_length.0 as usize;

    // Decompress using zlib
    let mut decompressed = Vec::with_capacity(uncompressed_size);
    let mut decoder = ZlibDecoder::new(cursor);
    decoder.read_to_end(&mut decompressed).map_err(Error::Io)?;

    if decompressed.len() != uncompressed_size {
        return Err(Error::Protocol(basalt_protocol::Error::Type(
            basalt_types::Error::InvalidData(format!(
                "decompressed size mismatch: expected {}, got {}",
                uncompressed_size,
                decompressed.len()
            )),
        )));
    }

    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_threshold_not_compressed() {
        let data = b"short data";
        let threshold = 256;

        let compressed = compress_packet(data, threshold).unwrap();

        // First byte should be VarInt(0) = 0x00 (not compressed)
        assert_eq!(compressed[0], 0x00);

        let decompressed = decompress_packet(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn above_threshold_compressed() {
        // Create data larger than threshold
        let data: Vec<u8> = (0..512).map(|i| (i % 256) as u8).collect();
        let threshold = 256;

        let compressed = compress_packet(&data, threshold).unwrap();

        // First VarInt should be the uncompressed size (512), not 0
        let mut cursor = compressed.as_slice();
        let data_length = VarInt::decode(&mut cursor).unwrap();
        assert_eq!(data_length.0, 512);

        // Compressed data should be smaller than original (for repetitive data)
        assert!(compressed.len() < data.len());

        let decompressed = decompress_packet(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn exact_threshold_compressed() {
        let data = vec![0xAB; 256];
        let threshold = 256;

        let compressed = compress_packet(&data, threshold).unwrap();
        let decompressed = decompress_packet(&compressed).unwrap();
        assert_eq!(decompressed, data);

        // Should be compressed (data_length > 0)
        let mut cursor = compressed.as_slice();
        let data_length = VarInt::decode(&mut cursor).unwrap();
        assert!(data_length.0 > 0);
    }

    #[test]
    fn empty_data_below_threshold() {
        let data = b"";
        let threshold = 256;

        let compressed = compress_packet(data, threshold).unwrap();
        let decompressed = decompress_packet(&compressed).unwrap();
        assert!(decompressed.is_empty());
    }

    #[test]
    fn roundtrip_various_sizes() {
        for size in [1, 10, 100, 255, 256, 257, 1000, 4096] {
            let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
            let threshold = 256;

            let compressed = compress_packet(&data, threshold).unwrap();
            let decompressed = decompress_packet(&compressed).unwrap();
            assert_eq!(decompressed, data, "failed for size {size}");
        }
    }

    #[test]
    fn large_packet_compression() {
        // Simulate chunk-sized data (highly compressible)
        let data = vec![0x00; 16384];
        let threshold = 256;

        let compressed = compress_packet(&data, threshold).unwrap();
        // Should compress very well since it's all zeros
        assert!(compressed.len() < data.len() / 10);

        let decompressed = decompress_packet(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }
}
