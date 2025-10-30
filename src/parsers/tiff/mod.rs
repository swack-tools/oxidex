//! TIFF format parser
//!
//! Handles Image File Directory (IFD) parsing, TIFF tag extraction, and maker notes.

#![allow(dead_code)]

pub mod file_parser;
pub mod ifd_parser;
pub mod makernote_parser;
pub mod tag_parser;
pub mod tiff_enums;

// Re-export main parsing functions for convenience
pub use file_parser::parse_tiff_file;
