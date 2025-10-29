//! Infrastructure: I/O abstraction
//!
//! This module provides file system abstraction with memory-mapped and buffered readers.
//!
//! # Available Readers
//!
//! - `MMapReader`: Zero-copy memory-mapped file access using `memmap2`
//! - `BufferedReader`: In-memory buffered file access using `Vec<u8>`
//!
//! Both implement the `FileReader` trait from the core module.

#![allow(dead_code)]

pub mod buffered_reader;
pub mod file_reader;
pub mod mmap_reader;

// Re-export reader implementations for convenient access
pub use buffered_reader::BufferedReader;
pub use mmap_reader::MMapReader;
