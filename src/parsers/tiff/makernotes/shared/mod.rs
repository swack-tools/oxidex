//! Shared utilities for MakerNotes parsing
//!
//! This module provides common functionality used across all manufacturer
//! parsers to maximize code reuse and reduce duplication.

/// Array extraction utilities for IFD entries
pub mod array_extractors;
/// Low-level byte parsing helper functions
pub mod byte_utils;
/// Common trait definition for all MakerNotes parsers
pub mod makernote_parser;
/// Common value interpretation and decoding functions
pub mod value_decoders;

pub use makernote_parser::MakerNoteParser;
