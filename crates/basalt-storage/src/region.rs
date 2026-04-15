//! Region file I/O for the BSR format.
//!
//! Each region file covers 32×32 = 1024 chunks and contains an offset
//! table for O(1) chunk lookup followed by LZ4-compressed data blobs.
//!
//! The storage is agnostic about the chunk format — it stores and
//! retrieves raw bytes. Serialization/deserialization is handled by
//! the caller (basalt-world).

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Magic bytes at the start of every BSR region file.
const MAGIC: &[u8; 4] = b"BSLT";
/// Current format version.
const VERSION: u16 = 1;
/// Size of the file header in bytes.
const HEADER_SIZE: u64 = 8;
/// Number of chunks per region (32×32).
const CHUNKS_PER_REGION: usize = 1024;
/// Size of the offset table in bytes.
const TABLE_SIZE: u64 = (CHUNKS_PER_REGION * 8) as u64;

/// Manages BSR region files on disk.
///
/// Region files are stored in a directory with names like `r.0.0.bsr`
/// where the numbers are region coordinates (chunk coords / 32).
/// The storage handles LZ4 compression/decompression transparently.
pub struct RegionStorage {
    /// Directory containing the region files.
    directory: PathBuf,
}

impl RegionStorage {
    /// Creates a new storage backed by the given directory.
    ///
    /// Creates the directory if it doesn't exist.
    pub fn new(directory: impl Into<PathBuf>) -> io::Result<Self> {
        let directory = directory.into();
        fs::create_dir_all(&directory)?;
        Ok(Self { directory })
    }

    /// Saves raw chunk data to the appropriate region file.
    ///
    /// Compresses with LZ4 before writing. Creates the region file
    /// if it doesn't exist.
    pub fn save_raw(&self, chunk_x: i32, chunk_z: i32, data: &[u8]) -> io::Result<()> {
        let (region_x, region_z) = chunk_to_region(chunk_x, chunk_z);
        let index = chunk_index_in_region(chunk_x, chunk_z);

        let compressed = lz4_flex::compress_prepend_size(data);

        let path = self.region_path(region_x, region_z);
        let mut file = self.open_or_create_region(&path)?;

        let mut table = read_offset_table(&mut file)?;

        file.seek(SeekFrom::End(0))?;
        let offset = file.stream_position()? as u32;
        file.write_all(&compressed)?;

        table[index] = (offset, compressed.len() as u32);
        write_offset_table(&mut file, &table)?;

        Ok(())
    }

    /// Loads raw chunk data from the appropriate region file.
    ///
    /// Returns the decompressed bytes, or `None` if the chunk hasn't
    /// been saved or the region file doesn't exist.
    pub fn load_raw(&self, chunk_x: i32, chunk_z: i32) -> io::Result<Option<Vec<u8>>> {
        let (region_x, region_z) = chunk_to_region(chunk_x, chunk_z);
        let index = chunk_index_in_region(chunk_x, chunk_z);

        let path = self.region_path(region_x, region_z);
        if !path.exists() {
            return Ok(None);
        }

        let mut file = File::open(&path)?;

        if !verify_header(&mut file)? {
            return Ok(None);
        }

        let table = read_offset_table(&mut file)?;
        let (offset, size) = table[index];

        if offset == 0 && size == 0 {
            return Ok(None);
        }

        file.seek(SeekFrom::Start(offset as u64))?;
        let mut compressed = vec![0u8; size as usize];
        file.read_exact(&mut compressed)?;

        let raw = lz4_flex::decompress_size_prepended(&compressed)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        Ok(Some(raw))
    }

    /// Saves multiple chunks in batch, grouped by region.
    pub fn save_raw_batch(&self, chunks: &[(i32, i32, &[u8])]) -> io::Result<()> {
        type RegionEntries<'a> = HashMap<(i32, i32), Vec<(usize, &'a [u8])>>;
        let mut by_region: RegionEntries<'_> = HashMap::new();
        for &(cx, cz, data) in chunks {
            let region = chunk_to_region(cx, cz);
            let index = chunk_index_in_region(cx, cz);
            by_region.entry(region).or_default().push((index, data));
        }

        for ((region_x, region_z), entries) in &by_region {
            let path = self.region_path(*region_x, *region_z);
            let mut file = self.open_or_create_region(&path)?;
            let mut table = read_offset_table(&mut file)?;

            for &(index, data) in entries {
                let compressed = lz4_flex::compress_prepend_size(data);
                file.seek(SeekFrom::End(0))?;
                let offset = file.stream_position()? as u32;
                file.write_all(&compressed)?;
                table[index] = (offset, compressed.len() as u32);
            }

            write_offset_table(&mut file, &table)?;
        }

        Ok(())
    }

    /// Compacts a region file by rewriting it with only live chunk data.
    ///
    /// Over time, `save_raw` appends new data without reclaiming space
    /// from previous saves of the same chunk. This leaves dead space in
    /// the file. `compact` rewrites the file with only the current data,
    /// eliminating all gaps.
    ///
    /// Returns the number of bytes reclaimed, or 0 if the region file
    /// doesn't exist.
    pub fn compact(&self, region_x: i32, region_z: i32) -> io::Result<u64> {
        let path = self.region_path(region_x, region_z);
        if !path.exists() {
            return Ok(0);
        }

        let mut file = File::options().read(true).write(true).open(&path)?;
        if !verify_header(&mut file)? {
            return Ok(0);
        }

        let old_size = file.seek(SeekFrom::End(0))?;
        let table = read_offset_table(&mut file)?;

        // Read all live blobs into memory
        let mut blobs: Vec<(usize, Vec<u8>)> = Vec::new();
        for (index, &(offset, size)) in table.iter().enumerate() {
            if offset == 0 && size == 0 {
                continue;
            }
            file.seek(SeekFrom::Start(offset as u64))?;
            let mut data = vec![0u8; size as usize];
            file.read_exact(&mut data)?;
            blobs.push((index, data));
        }

        // Rewrite: header + table + packed data
        file.seek(SeekFrom::Start(0))?;
        file.set_len(0)?;
        file.write_all(MAGIC)?;
        file.write_all(&VERSION.to_le_bytes())?;
        file.write_all(&[0u8; 2])?;

        let mut new_table = vec![(0u32, 0u32); CHUNKS_PER_REGION];

        // Write empty table placeholder
        file.write_all(&vec![0u8; TABLE_SIZE as usize])?;

        // Write blobs contiguously
        for (index, data) in &blobs {
            let offset = file.stream_position()? as u32;
            file.write_all(data)?;
            new_table[*index] = (offset, data.len() as u32);
        }

        write_offset_table(&mut file, &new_table)?;

        let new_size = file.seek(SeekFrom::End(0))?;
        Ok(old_size.saturating_sub(new_size))
    }

    /// Compacts all region files in the storage directory.
    ///
    /// Returns the total number of bytes reclaimed across all regions.
    pub fn compact_all(&self) -> io::Result<u64> {
        let mut total_reclaimed = 0u64;

        let entries = fs::read_dir(&self.directory)?;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("r.") || !name.ends_with(".bsr") {
                continue;
            }
            // Parse r.X.Z.bsr
            let parts: Vec<&str> = name
                .trim_start_matches("r.")
                .trim_end_matches(".bsr")
                .split('.')
                .collect();
            if parts.len() == 2
                && let (Ok(rx), Ok(rz)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>())
            {
                total_reclaimed += self.compact(rx, rz)?;
            }
        }

        Ok(total_reclaimed)
    }

    /// Returns the path for a region file.
    fn region_path(&self, region_x: i32, region_z: i32) -> PathBuf {
        self.directory.join(format!("r.{region_x}.{region_z}.bsr"))
    }

    /// Opens or creates a region file with a valid header and empty table.
    fn open_or_create_region(&self, path: &Path) -> io::Result<File> {
        if path.exists() {
            let mut file = File::options().read(true).write(true).open(path)?;
            if verify_header(&mut file)? {
                return Ok(file);
            }
        }

        let mut file = File::create(path)?;
        file.write_all(MAGIC)?;
        file.write_all(&VERSION.to_le_bytes())?;
        file.write_all(&[0u8; 2])?;
        file.write_all(&vec![0u8; TABLE_SIZE as usize])?;
        file.seek(SeekFrom::Start(0))?;

        File::options().read(true).write(true).open(path)
    }
}

/// Converts chunk coordinates to region coordinates.
fn chunk_to_region(chunk_x: i32, chunk_z: i32) -> (i32, i32) {
    (chunk_x >> 5, chunk_z >> 5)
}

/// Returns the index of a chunk within its region (0..1023).
fn chunk_index_in_region(chunk_x: i32, chunk_z: i32) -> usize {
    let local_x = chunk_x.rem_euclid(32);
    let local_z = chunk_z.rem_euclid(32);
    (local_z * 32 + local_x) as usize
}

/// Reads the offset table from a region file.
fn read_offset_table(file: &mut File) -> io::Result<Vec<(u32, u32)>> {
    file.seek(SeekFrom::Start(HEADER_SIZE))?;
    let mut buf = vec![0u8; TABLE_SIZE as usize];
    file.read_exact(&mut buf)?;

    let mut table = Vec::with_capacity(CHUNKS_PER_REGION);
    for i in 0..CHUNKS_PER_REGION {
        let offset = u32::from_le_bytes(buf[i * 8..i * 8 + 4].try_into().unwrap());
        let size = u32::from_le_bytes(buf[i * 8 + 4..i * 8 + 8].try_into().unwrap());
        table.push((offset, size));
    }
    Ok(table)
}

/// Writes the offset table to a region file.
fn write_offset_table(file: &mut File, table: &[(u32, u32)]) -> io::Result<()> {
    file.seek(SeekFrom::Start(HEADER_SIZE))?;
    let mut buf = Vec::with_capacity(TABLE_SIZE as usize);
    for &(offset, size) in table {
        buf.extend_from_slice(&offset.to_le_bytes());
        buf.extend_from_slice(&size.to_le_bytes());
    }
    file.write_all(&buf)?;
    file.sync_all()?;
    Ok(())
}

/// Verifies the magic bytes and version of a region file.
fn verify_header(file: &mut File) -> io::Result<bool> {
    file.seek(SeekFrom::Start(0))?;
    let mut header = [0u8; 8];
    if file.read(&mut header)? < 8 {
        return Ok(false);
    }
    Ok(&header[0..4] == MAGIC && u16::from_le_bytes([header[4], header[5]]) == VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_raw() {
        let dir = tempfile::tempdir().unwrap();
        let storage = RegionStorage::new(dir.path()).unwrap();

        let data = b"hello chunk data";
        storage.save_raw(0, 0, data).unwrap();

        let loaded = storage.load_raw(0, 0).unwrap().unwrap();
        assert_eq!(loaded, data);
    }

    #[test]
    fn load_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let storage = RegionStorage::new(dir.path()).unwrap();
        assert!(storage.load_raw(99, 99).unwrap().is_none());
    }

    #[test]
    fn multiple_chunks_in_same_region() {
        let dir = tempfile::tempdir().unwrap();
        let storage = RegionStorage::new(dir.path()).unwrap();

        storage.save_raw(0, 0, b"chunk_0_0").unwrap();
        storage.save_raw(1, 1, b"chunk_1_1").unwrap();

        assert_eq!(storage.load_raw(0, 0).unwrap().unwrap(), b"chunk_0_0");
        assert_eq!(storage.load_raw(1, 1).unwrap().unwrap(), b"chunk_1_1");
    }

    #[test]
    fn negative_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        let storage = RegionStorage::new(dir.path()).unwrap();

        storage.save_raw(-10, -20, b"negative").unwrap();
        assert_eq!(storage.load_raw(-10, -20).unwrap().unwrap(), b"negative");
    }

    #[test]
    fn batch_save() {
        let dir = tempfile::tempdir().unwrap();
        let storage = RegionStorage::new(dir.path()).unwrap();

        let data: Vec<(i32, i32, &[u8])> = vec![(0, 0, b"a"), (1, 0, b"b"), (2, 0, b"c")];
        storage.save_raw_batch(&data).unwrap();

        assert_eq!(storage.load_raw(0, 0).unwrap().unwrap(), b"a");
        assert_eq!(storage.load_raw(1, 0).unwrap().unwrap(), b"b");
        assert_eq!(storage.load_raw(2, 0).unwrap().unwrap(), b"c");
    }

    #[test]
    fn compact_reclaims_dead_space() {
        let dir = tempfile::tempdir().unwrap();
        let storage = RegionStorage::new(dir.path()).unwrap();

        // Save the same chunk 5 times — each appends, leaving dead data
        for i in 0..5u8 {
            storage.save_raw(0, 0, &[i; 1024]).unwrap();
        }

        let path = storage.region_path(0, 0);
        let size_before = std::fs::metadata(&path).unwrap().len();

        let reclaimed = storage.compact(0, 0).unwrap();
        let size_after = std::fs::metadata(&path).unwrap().len();

        assert!(reclaimed > 0, "should reclaim dead space");
        assert!(size_after < size_before, "file should be smaller");

        // Data should still be readable
        let loaded = storage.load_raw(0, 0).unwrap().unwrap();
        assert_eq!(loaded, vec![4u8; 1024]); // last save wins
    }

    #[test]
    fn compact_preserves_all_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let storage = RegionStorage::new(dir.path()).unwrap();

        storage.save_raw(0, 0, b"aaa").unwrap();
        storage.save_raw(1, 0, b"bbb").unwrap();
        storage.save_raw(2, 0, b"ccc").unwrap();

        // Overwrite one to create dead space
        storage.save_raw(1, 0, b"bbb_new").unwrap();

        storage.compact(0, 0).unwrap();

        assert_eq!(storage.load_raw(0, 0).unwrap().unwrap(), b"aaa");
        assert_eq!(storage.load_raw(1, 0).unwrap().unwrap(), b"bbb_new");
        assert_eq!(storage.load_raw(2, 0).unwrap().unwrap(), b"ccc");
    }

    #[test]
    fn compact_nonexistent_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        let storage = RegionStorage::new(dir.path()).unwrap();
        assert_eq!(storage.compact(99, 99).unwrap(), 0);
    }

    #[test]
    fn compact_all_reclaims_across_regions() {
        let dir = tempfile::tempdir().unwrap();
        let storage = RegionStorage::new(dir.path()).unwrap();

        // Two different regions: (0,0) in r.0.0 and (32,0) in r.1.0
        storage.save_raw(0, 0, b"first").unwrap();
        storage.save_raw(0, 0, b"second").unwrap(); // dead space in r.0.0
        storage.save_raw(32, 0, b"first").unwrap();
        storage.save_raw(32, 0, b"second").unwrap(); // dead space in r.1.0

        let reclaimed = storage.compact_all().unwrap();
        assert!(reclaimed > 0);

        // Both still readable
        assert_eq!(storage.load_raw(0, 0).unwrap().unwrap(), b"second");
        assert_eq!(storage.load_raw(32, 0).unwrap().unwrap(), b"second");
    }
}
