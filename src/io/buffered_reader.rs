//! Buffered file reader implementation
//!
//! This module provides `BufferedReader`, a file reader that loads the entire
//! file contents into memory upon construction, then serves slices from the
//! in-memory buffer. This implementation is suitable for:
//!
//! - Small to medium-sized files where memory consumption is acceptable
//! - Files that will be accessed multiple times with random access patterns
//! - Scenarios where the file system has high seek latency
//! - Testing and development when memory-mapping is not available
//!
//! # Architecture
//!
//! `BufferedReader` is a **secondary adapter** implementing the `FileReader` port.
//! Unlike true streaming readers, it reads the entire file into a `Vec<u8>` at
//! construction time, trading memory for simplicity and consistent performance.
//!
//! # Design Rationale
//!
//! The `FileReader` trait requires returning borrowed slices (`&[u8]`) with a
//! lifetime tied to `&self`. This constraint makes true streaming (reading only
//! requested portions) complex because:
//!
//! 1. We cannot return references from interior mutable state (e.g., `RefCell`)
//!    because the borrow guard would be dropped at function return
//! 2. We cannot store temporary buffers in the struct and return slices because
//!    subsequent reads would invalidate previous slices
//!
//! By loading the entire file upfront, we can return stable slices just like
//! `MMapReader`, with the trade-off of higher memory usage.
//!
//! # Memory Considerations
//!
//! - Memory usage: O(file size) - entire file loaded into heap
//! - Suitable for files up to ~100 MB on typical systems
//! - For larger files, prefer `MMapReader` which uses virtual memory paging
//!
//! # Thread Safety
//!
//! `BufferedReader` is `Send` and `Sync` because the internal buffer is immutable
//! after construction. Multiple threads can safely read from the same buffer
//! concurrently.

#![allow(dead_code)]

use crate::core::FileReader;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Buffered file reader that loads entire file into memory.
///
/// This reader opens a file, reads all contents into a `Vec<u8>`, then serves
/// slices from this buffer. All reads are zero-copy after the initial load.
///
/// # Thread Safety
///
/// `BufferedReader` is `Send` and `Sync` because the buffer is immutable after
/// construction. Multiple threads can safely read concurrently.
///
/// # Examples
///
/// ```no_run
/// use exiftool_rs::io::BufferedReader;
/// use exiftool_rs::core::FileReader;
/// use std::path::Path;
///
/// # fn example() -> std::io::Result<()> {
/// let reader = BufferedReader::new(Path::new("image.jpg"))?;
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
pub struct BufferedReader {
    /// In-memory buffer containing the entire file contents.
    /// Loaded once at construction, then never modified.
    buffer: Vec<u8>,
}

impl BufferedReader {
    /// Creates a new buffered reader by loading the entire file into memory.
    ///
    /// # Parameters
    ///
    /// - `path`: Path to the file to be read
    ///
    /// # Returns
    ///
    /// - `Ok(BufferedReader)`: Successfully opened and loaded the file
    /// - `Err(io::Error)`: Failed to open or read the file
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - The file does not exist or cannot be opened
    /// - The process lacks permission to read the file
    /// - Reading the file fails (e.g., I/O error, disk failure)
    /// - The system has insufficient memory to load the file
    ///
    /// # Memory Usage
    ///
    /// This function allocates `file_size` bytes to store the file contents.
    /// For large files (>100 MB), consider using `MMapReader` instead.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use exiftool_rs::core::FileReader;
    /// use exiftool_rs::io::BufferedReader;
    /// use std::path::Path;
    ///
    /// # fn example() -> std::io::Result<()> {
    /// let reader = BufferedReader::new(Path::new("test.jpg"))?;
    /// println!("Loaded file of size: {}", reader.size());
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;

        // Read entire file into memory
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        Ok(Self { buffer })
    }
}

impl FileReader for BufferedReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;

        // Check for potential overflow before addition
        let end = start.checked_add(length).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "offset + length overflows usize",
            )
        })?;

        // Check bounds against buffer
        if end > self.buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "read beyond end of file: requested [{}, {}), file size {}",
                    start,
                    end,
                    self.buffer.len()
                ),
            ));
        }

        // Return slice directly from in-memory buffer (zero-copy)
        Ok(&self.buffer[start..end])
    }

    fn size(&self) -> u64 {
        self.buffer.len() as u64
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
    fn test_buffered_reader_creation() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();
        assert_eq!(reader.size(), 13);
    }

    #[test]
    fn test_read_at_offset_zero() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();

        let data = reader.read(0, 5).unwrap();
        assert_eq!(data, b"Hello");
    }

    #[test]
    fn test_read_at_middle_offset() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();

        let data = reader.read(7, 5).unwrap();
        assert_eq!(data, b"World");
    }

    #[test]
    fn test_read_at_end_of_file() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();

        // Read last character
        let data = reader.read(12, 1).unwrap();
        assert_eq!(data, b"!");
    }

    #[test]
    fn test_read_zero_bytes() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();

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
        let reader = BufferedReader::new(file.path()).unwrap();

        let data = reader.read(0, content.len()).unwrap();
        assert_eq!(data, content);
    }

    #[test]
    fn test_read_beyond_end_of_file() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();

        // Try to read beyond the end
        let result = reader.read(10, 10);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_read_at_invalid_offset() {
        let file = create_test_file(b"Hello, World!").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();

        // Offset beyond file size
        let result = reader.read(100, 5);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_empty_file() {
        let file = create_test_file(b"").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();

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
        let reader = BufferedReader::new(file.path()).unwrap();

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
        let reader = BufferedReader::new(file.path()).unwrap();

        assert_eq!(reader.size(), content.len() as u64);
    }

    #[test]
    fn test_large_offset_overflow() {
        let file = create_test_file(b"Small file").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();

        // Try to cause integer overflow with offset + length
        let result = reader.read(u64::MAX, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_nonexistent_file() {
        let result = BufferedReader::new(Path::new("/nonexistent/path/to/file.txt"));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn test_overlapping_reads() {
        let file = create_test_file(b"ABCDEFGHIJKLMNOP").unwrap();
        let reader = BufferedReader::new(file.path()).unwrap();

        // Test that multiple overlapping reads work correctly
        let slice1 = reader.read(0, 8).unwrap();
        let slice2 = reader.read(4, 8).unwrap();

        assert_eq!(slice1, b"ABCDEFGH");
        assert_eq!(slice2, b"EFGHIJKL");

        // Verify slices remain valid simultaneously
        assert_eq!(slice1[4], b'E');
        assert_eq!(slice2[0], b'E');
    }
}
