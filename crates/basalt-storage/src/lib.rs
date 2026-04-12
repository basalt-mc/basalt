//! Basalt storage with the BSR (Basalt Region) format.
//!
//! Provides fast, compact persistence using a custom binary region
//! format with LZ4 compression. Region files cover 32×32 chunks
//! with O(1) offset-table lookup.
//!
//! The storage is format-agnostic — it stores and retrieves raw
//! bytes. Chunk serialization/deserialization is handled by the
//! caller (basalt-world).
//!
//! # Format highlights
//!
//! - LZ4 compression: 4.4 GB/s decompression, minimal CPU overhead
//! - Region files with 1024-entry offset table for O(1) access
//! - Header with magic bytes and version for forward compatibility

mod region;

pub use region::RegionStorage;
