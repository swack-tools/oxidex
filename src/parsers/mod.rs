//! Infrastructure: Format adapters
//!
//! This module contains format-specific parsers implementing the FormatParser trait.
//! Each format is organized as a separate submodule.

#![allow(dead_code)]

pub mod common;
pub mod format_detector;
pub mod jpeg;
pub mod png;
pub mod tiff;
pub mod xmp;
