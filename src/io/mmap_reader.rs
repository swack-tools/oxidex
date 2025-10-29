//! Memory-mapped file reader implementation
//!
//! This module provides `MMapReader`, a zero-copy file reader using memory-mapped
//! I/O via the `memmap2` crate. This implementation is optimal for:
//!
//! - Large files where loading entire contents into memory is impractical
//! - Random access patterns (e.g., seeking to EXIF segments in JPEG files)
//! - Performance-critical parsing where allocation overhead must be minimized
//!
//! # Architecture
//!
//! `MMapReader` is a **secondary adapter** implementing the `FileReader` port.
//! It maps the entire file into the process's virtual address space using
//! `mmap(2)` (Unix) or `MapViewOfFile` (Windows), enabling zero-copy slice access.
//!
//! # Safety
//!
//! Memory-mapping is inherently unsafe because:
//! - The file could be modified by another process while mapped
//! - The file could be truncated, invalidating the mapping
//! - On some platforms, accessing unmapped regions causes segmentation faults
//!
//! This implementation uses `memmap2::Mmap`, which provides a safe abstraction
//! by ensuring the file remains open and the mapping valid for the lifetime of
//! the `Mmap` object.
//!
//! # Limitations
//!
//! - Requires the file to fit within the process's virtual address space
//!   (not an issue on 64-bit systems, but limits 32-bit to ~2-3 GB files)
//! - The entire file is mapped even if only small portions are accessed
//!   (the OS handles paging, so physical memory usage is proportional to access)
//! - Not suitable for files that change frequently during reading

#![allow(dead_code)]

use crate::core::FileReader;
use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::path::Path;

/// Memory-mapped file reader providing zero-copy access to file contents.
///
/// This reader maps the entire file into memory using `mmap(2)` and returns
/// slices directly from the mapped region, avoiding buffer allocations.
///
/// # Thread Safety
///
/// `MMapReader` is `Send` and `Sync` because `Mmap` provides read-only access
/// to the mapped region. Multiple threads can safely read from the same mapping
/// concurrently.
///
/// # Examples
///
/// ```no_run
/// use exiftool_rs::io::MMapReader;
/// use exiftool_rs::core::FileReader;
/// use std::path::Path;
///
/// # fn example() -> std::io::Result<()> {
/// let reader = MMapReader::new(Path::new("image.jpg"))?;
///
/// // Read JPEG SOI marker at offset 0
/// let header = reader.read(0, 2)?;
/// assert_eq!(header, &[0xFF, 0xD8]);
///
/// // Check file size
/// println!("File size: {} bytes", reader.size());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct MMapReader {
    /// File handle kept alive to ensure the memory mapping remains valid.
    /// Must not be dropped before `mmap`.
    _file: File,

    /// Memory-mapped region containing the file contents.
    /// The slice `&mmap[..]` is equivalent to the entire file contents.
    mmap: Mmap,
}

impl MMapReader {
    /// Creates a new memory-mapped reader for the file at the given path.
    ///
    /// # Parameters
    ///
    /// - `path`: Path to the file to be mapped
    ///
    /// # Returns
    ///
    /// - `Ok(MMapReader)`: Successfully opened and mapped the file
    /// - `Err(io::Error)`: Failed to open or map the file
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - The file does not exist or cannot be opened
    /// - The process lacks permission to read the file
    /// - The file cannot be memory-mapped (e.g., empty files, special files)
    /// - The system has insufficient virtual address space
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use exiftool_rs::core::FileReader;
    /// use exiftool_rs::io::MMapReader;
    /// use std::path::Path;
    ///
    /// # fn example() -> std::io::Result<()> {
    /// let reader = MMapReader::new(Path::new("test.jpg"))?;
    /// println!("Mapped file of size: {}", reader.size());
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;

        // SAFETY: We're creating a read-only memory mapping of a file we own.
        // The `file` handle is stored in the struct to ensure it remains open
        // for the lifetime of the mapping. The `memmap2::Mmap` type ensures
        // safe access by preventing modification and handling platform-specific
        // edge cases.
        let mmap = unsafe { Mmap::map(&file)? };

        Ok(Self { _file: file, mmap })
    }
}

impl FileReader for MMapReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;

        // Check for potential overflow before addition
        let end = start.checked_add(length).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "offset + length overflows usize",
            )
        })?;

        // Check bounds against mapped region
        if end > self.mmap.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "read beyond end of file: requested [{}, {}), file size {}",
                    start,
                    end,
                    self.mmap.len()
                ),
            ));
        }

        // Return slice directly from memory-mapped region (zero-copy)
        Ok(&self.mmap[start..end])
    }

    fn size(&self) -> u64 {
        self.mmap.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Helper function to create a temporary file with specified content
    fn create_test_file(content: &[u8]) -> io::Result<NamedTempFile> {
        let mut file = NamedTempFile::new()?;
        file.write_all(content)?;
        file.flush()?;
        Ok(file)
    }

    #[test]
    fn test_mmap_reader_creation() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();
        assert_eq!(reader.size(), 13);
    }

    #[test]
    fn test_read_at_offset_zero() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        let data = reader.read(0, 5).unwrap();
        assert_eq!(data, b"Hello");
    }

    #[test]
    fn test_read_at_middle_offset() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        let data = reader.read(7, 5).unwrap();
        assert_eq!(data, b"World");
    }

    #[test]
    fn test_read_at_end_of_file() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        // Read last character
        let data = reader.read(12, 1).unwrap();
        assert_eq!(data, b"!");
    }

    #[test]
    fn test_read_zero_bytes() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        // Reading zero bytes should succeed at any valid offset
        let data = reader.read(0, 0).unwrap();
        assert_eq!(data.len(), 0);

        let data = reader.read(5, 0).unwrap();
        assert_eq!(data.len(), 0);

        // Even at the exact end of the file
        let data = reader.read(13, 0).unwrap();
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_read_entire_file() {
        let content = b"Hello, World!";
        let file = create_test_file(content).unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        let data = reader.read(0, content.len()).unwrap();
        assert_eq!(data, content);
    }

    #[test]
    fn test_read_beyond_end_of_file() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        // Try to read beyond the end
        let result = reader.read(10, 10);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_read_at_invalid_offset() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        // Offset beyond file size
        let result = reader.read(100, 5);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_empty_file() {
        let file = create_test_file(b"").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        assert_eq!(reader.size(), 0);

        // Reading zero bytes from empty file should succeed
        let data = reader.read(0, 0).unwrap();
        assert_eq!(data.len(), 0);

        // Reading any data from empty file should fail
        let result = reader.read(0, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_sequential_reads() {
        let file = create_test_file(b"ABCDEFGHIJKLMNOP").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        // Perform multiple reads to ensure the reader maintains state correctly
        assert_eq!(reader.read(0, 4).unwrap(), b"ABCD");
        assert_eq!(reader.read(4, 4).unwrap(), b"EFGH");
        assert_eq!(reader.read(8, 4).unwrap(), b"IJKL");
        assert_eq!(reader.read(12, 4).unwrap(), b"MNOP");
    }

    #[test]
    fn test_size_method() {
        let content = b"Test content with known size";
        let file = create_test_file(content).unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        assert_eq!(reader.size(), content.len() as u64);
    }

    #[test]
    fn test_large_offset_overflow() {
        let file = create_test_file(b"Small file").unwrap();
        let reader = MMapReader::new(file.path()).unwrap();

        // Try to cause integer overflow with offset + length
        let result = reader.read(u64::MAX, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_nonexistent_file() {
        let result = MMapReader::new(Path::new("/nonexistent/path/to/file.txt"));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }
}
