//! Infrastructure: I/O abstraction
//!
//! This module provides file system abstraction with memory-mapped and buffered readers.

#![allow(dead_code)]

pub mod file_reader;
pub mod mmap_reader;
pub mod buffered_reader;
