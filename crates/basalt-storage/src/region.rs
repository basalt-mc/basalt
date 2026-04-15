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
}
