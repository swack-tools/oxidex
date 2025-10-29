//! Infrastructure: Format adapters
//!
//! This module contains format-specific parsers implementing the FormatParser trait.
//! Each format is organized as a separate submodule.

#![allow(dead_code)]

pub mod common;
pub mod format_detector;
pub mod jpeg;
pub mod pdf;
pub mod png;
pub mod tiff;
pub mod xmp;

// Re-export the format detection function for convenient access
pub use format_detector::detect_format;
