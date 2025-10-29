//! JPEG format parser
//!
//! Handles JPEG segment marker parsing, EXIF, XMP, and IPTC segment extraction.

#![allow(dead_code)]

pub mod segment_parser;
pub mod exif_parser;
pub mod xmp_parser;
pub mod iptc_parser;
