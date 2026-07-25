//! APP segment parsers for JPEG files
//!
//! This module contains parsers for various APP segments beyond the standard
//! EXIF, XMP, and IPTC segments.

pub mod app0;
pub mod app10_hdr;
pub mod app11_jpeg_hdr;
pub mod app12_agfa;
pub mod app12_olympus;
pub mod app14_adobe;
pub mod app6;
pub mod jumbf;
pub mod photoshop;

// Re-export main parsing functions
pub use app0::parse_app0;
pub use app6::parse_app6;
pub use app10_hdr::parse_app10_hdr;
pub use app11_jpeg_hdr::parse_app11_jpeg_hdr;
pub use app12_agfa::parse_app12_agfa;
pub use app12_olympus::parse_app12_olympus;
pub use app14_adobe::parse_app14_adobe;
pub use jumbf::parse_jumbf;
pub use photoshop::parse_photoshop_irb;
