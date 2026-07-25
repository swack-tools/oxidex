//! Binary I/O utilities for format parsers.
//!
//! This module provides shared utilities for reading binary data with different
//! byte orders, as well as file system abstraction with memory-mapped and buffered readers.
//!
//! # Modules
//!
//! - [`buffered_reader`]: In-memory file reader
//! - [`file_reader`]: File reader trait definition
//! - [`mmap_reader`]: Memory-mapped file reader
//! - [`endian_reader`]: Random-access binary reading with configurable byte order
//!
//! # Example
//!
//! ```
//! use oxidex::io::{EndianReader, ByteOrder};
//!
//! let data = [0x00, 0x01, 0x02, 0x03];
//! let reader = EndianReader::big_endian(&data);
//! assert_eq!(reader.u16_at(0), Some(0x0001));
//! assert_eq!(reader.u16_at(2), Some(0x0203));
//!
//! let reader = EndianReader::little_endian(&data);
//! assert_eq!(reader.u16_at(0), Some(0x0100));
//! ```

#![allow(dead_code)]

// File reader implementations
pub mod buffered_reader;
pub mod file_reader;
pub mod mmap_reader;

// Binary reading utilities
pub mod cursor;
pub mod endian_reader;
pub mod timestamp;

// Re-export for convenient access
pub use buffered_reader::BufferedReader;
pub use cursor::Cursor;
pub use endian_reader::{ByteOrder, EndianReader};
pub use mmap_reader::MMapReader;
