//! Shared utilities for MakerNotes parsing
//!
//! This module provides common functionality used across all manufacturer
//! parsers to maximize code reuse and reduce duplication.

/// Common trait definition for all MakerNotes parsers
pub mod makernote_parser;
/// Array extraction utilities for IFD entries
pub mod array_extractors;
/// Common value interpretation and decoding functions
pub mod value_decoders;
/// Low-level byte parsing helper functions
pub mod byte_utils;

pub use makernote_parser::MakerNoteParser;
