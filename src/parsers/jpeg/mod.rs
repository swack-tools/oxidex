//! JPEG format parser
//!
//! Handles JPEG segment marker parsing, EXIF, XMP, IPTC, MPF, and other segment extraction.

#![allow(dead_code)]

pub mod app_parsers;
pub mod app_segments;
pub mod exif_parser;
pub mod flir_parser;
pub mod icc_chunk_assembler;
pub mod iptc_parser;
pub mod iptc_record1;
pub mod iptc_record2;
pub mod jfif_parser;
pub mod jpeg_hdr_parser;
pub mod mpf_parser;
pub mod quality_estimate;
pub mod segment_parser;
pub mod xmp_parser;

// Re-export segment parser types for convenient access
pub use segment_parser::{Segment, parse_segments};
