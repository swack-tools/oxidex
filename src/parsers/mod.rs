//! Infrastructure: Format adapters
//!
//! This module contains format-specific parsers implementing the FormatParser trait.
//! Each format is organized as a separate submodule.

#![allow(dead_code)]

/// File type detection mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorMode {
    /// Fast signature-based detection (default)
    Signature,
    /// AI-powered detection using Magika (requires --features magika)
    Magika,
}

impl Default for DetectorMode {
    fn default() -> Self {
        DetectorMode::Signature
    }
}

pub mod archive;
pub mod audio;
pub mod canon_vrd;
pub mod common;
pub mod detection;
pub mod document;
pub mod elf;
pub mod font;
pub mod icc;
pub mod image;
pub mod jpeg;
pub mod macho;
pub mod pdf;
pub mod pe;
pub mod png;
pub mod quicktime;
pub mod raw;
pub mod specialized;
pub mod text;
pub mod tiff;
pub mod trailer;
pub mod video;
pub mod xmp;

// Optional AI-powered file detection (feature: magika)
#[cfg(feature = "magika")]
pub mod magika_detector;

// Re-export the format detection function for convenient access
pub use detection::detect_format;
