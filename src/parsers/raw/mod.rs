//! Camera Raw Format Parsers
//!
//! This module provides parsers for camera raw file formats from various manufacturers.
//! Most raw formats are based on TIFF/EXIF structure with manufacturer-specific extensions.

// Submodules
pub mod format_detection;
pub mod metadata;

// Format-specific parsers
pub mod raf_parser;

// Re-export the public API
pub use format_detection::{RawFormat, detect_raw_format};
pub use metadata::parse_raw_metadata;
