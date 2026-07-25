//! Shared test utilities for unit tests within the crate.
//!
//! This module provides common test infrastructure like `TestReader`
//! that can be shared across all unit tests in the crate.
//!
//! # Usage
//!
//! In test modules within `src/`:
//! ```ignore
//! #[cfg(test)]
//! mod tests {
//!     use crate::test_support::TestReader;
//!     // ...
//! }
//! ```

use crate::core::FileReader;
use std::io;

/// In-memory FileReader implementation for unit testing.
///
/// Wraps a `Vec<u8>` and implements the `FileReader` trait,
/// allowing tests to create virtual files from byte arrays.
pub struct TestReader {
    data: Vec<u8>,
}

impl TestReader {
    /// Creates a new TestReader from a Vec<u8>.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Creates a new TestReader from a byte slice.
    pub fn from_slice(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
        }
    }
}

impl FileReader for TestReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.saturating_add(length).min(self.data.len());

        if start > self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "offset beyond end of data",
            ));
        }

        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}
