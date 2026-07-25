//! Shared test utilities for integration tests.
//!
//! This module provides common test infrastructure that can be imported
//! by integration tests to avoid duplicating TestReader and fixtures.
//!
//! # Usage
//!
//! In integration test files, import with:
//! ```ignore
//! #[path = "../common/mod.rs"]
//! mod common;
//! use common::TestReader;
//! ```

use oxidex::core::FileReader;
use std::io;

/// In-memory FileReader implementation for testing.
///
/// Wraps a `Vec<u8>` and implements the `FileReader` trait,
/// allowing tests to create virtual files from byte arrays.
#[allow(dead_code)]
pub struct TestReader {
    data: Vec<u8>,
}

#[allow(dead_code)]
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

    /// Returns a reference to the underlying data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

impl FileReader for TestReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start + length;

        if end > self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "read beyond end of data",
            ));
        }

        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}
