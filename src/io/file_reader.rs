//! FileReader trait implementations
//!
//! This module re-exports the concrete implementations of the `FileReader` trait:
//!
//! - `MMapReader`: Memory-mapped file access for zero-copy performance
//! - `BufferedReader`: In-memory buffered file access for simpler use cases
//!
//! Both readers provide the same interface via the `FileReader` trait but differ
//! in their memory and performance characteristics.

#![allow(dead_code)]

// Re-export both reader implementations for convenient access
pub use super::buffered_reader::BufferedReader;
pub use super::mmap_reader::MMapReader;
