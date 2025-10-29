//! FileReader trait definition (Port)
//!
//! This module defines the `FileReader` trait, which serves as a secondary port
//! in the hexagonal architecture. It abstracts file I/O operations to enable:
//!
//! - Zero-copy access via memory-mapped files
//! - Testing with in-memory buffers
//! - Support for various I/O backends (filesystem, network, embedded)
//!
//! # Architectural Role
//!
//! `FileReader` is a **secondary port** (infrastructure interface) that the domain
//! layer depends on. Concrete implementations (adapters) include:
//!
//! - `MMapReader`: Memory-mapped file access for performance
//! - `BufferedReader`: Standard buffered file reading
//! - `TestReader`: In-memory buffer for unit testing
//!
//! By depending on this trait rather than concrete I/O types, the core library
//! remains testable and independent of filesystem details.

#![allow(dead_code)]

use std::io;

/// Abstraction for reading file data with zero-copy semantics.
///
/// This trait enables format parsers to read file contents without coupling
/// to specific I/O implementations. Implementations should prioritize zero-copy
/// access where possible (e.g., returning slices into memory-mapped regions).
///
/// # Object Safety
///
/// This trait is object-safe and designed for use with `dyn FileReader`.
/// All methods use `&self` and return borrowed data or primitive types.
///
/// # Lifetime Considerations
///
/// The `read()` method returns a slice borrowed from `self`. Callers must ensure
/// the reader remains alive for the lifetime of the returned slice.
///
/// # Examples
///
/// ```no_run
/// use exiftool_rs::core::FileReader;
/// use std::io;
///
/// struct InMemoryReader {
///     data: Vec<u8>,
/// }
///
/// impl FileReader for InMemoryReader {
///     fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
///         let start = offset as usize;
///         let end = start + length;
///
///         if end > self.data.len() {
///             return Err(io::Error::new(
///                 io::ErrorKind::UnexpectedEof,
///                 "read beyond end of file"
///             ));
///         }
///
///         Ok(&self.data[start..end])
///     }
///
///     fn size(&self) -> u64 {
///         self.data.len() as u64
///     }
/// }
/// ```
///
/// # Implementer Contracts
///
/// Implementations MUST:
/// - Return borrowed slices valid for the lifetime of `&self`
/// - Return `Err` if `offset + length` exceeds file size
/// - Return consistent `size()` values during the reader's lifetime
/// - Be thread-safe if intended for concurrent access
pub trait FileReader {
    /// Reads a slice of bytes from the file at the specified offset.
    ///
    /// # Parameters
    ///
    /// - `offset`: Byte offset from the start of the file (0-indexed)
    /// - `length`: Number of bytes to read
    ///
    /// # Returns
    ///
    /// - `Ok(&[u8])`: Borrowed slice of exactly `length` bytes
    /// - `Err(io::Error)`: I/O error or out-of-bounds access
    ///
    /// # Errors
    ///
    /// This method returns an error if:
    /// - `offset + length` exceeds the file size
    /// - An I/O error occurs during reading
    /// - The underlying resource is no longer available
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use exiftool_rs::core::FileReader;
    /// # fn example(reader: &dyn FileReader) -> std::io::Result<()> {
    /// // Read JPEG SOI marker (0xFFD8) at start of file
    /// let header = reader.read(0, 2)?;
    /// assert_eq!(header, &[0xFF, 0xD8]);
    ///
    /// // Read 4 bytes at offset 100
    /// let data = reader.read(100, 4)?;
    /// assert_eq!(data.len(), 4);
    /// # Ok(())
    /// # }
    /// ```
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]>;

    /// Returns the total size of the file in bytes.
    ///
    /// # Returns
    ///
    /// The file size in bytes. For empty files, returns 0.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use exiftool_rs::core::FileReader;
    /// # fn example(reader: &dyn FileReader) {
    /// let size = reader.size();
    /// println!("File size: {} bytes", size);
    ///
    /// // Check if file is large enough for JPEG header
    /// if size < 2 {
    ///     eprintln!("File too small to be a valid JPEG");
    /// }
    /// # }
    /// ```
    fn size(&self) -> u64;
}
