//! Shared utilities for MakerNotes parsing
//!
//! This module provides common functionality used across all manufacturer
//! parsers to maximize code reuse and reduce duplication.
//!
//! ## Module Organization
//!
//! ### Core Utilities
//! - **array_extractors**: Array extraction utilities for IFD entries
//! - **byte_utils**: Low-level byte parsing helper functions
//! - **makernote_parser**: Common trait definition for all MakerNotes parsers
//! - **value_decoders**: Common value interpretation and decoding functions
//!
//! ### Advanced Decoding Utilities (New)
//! - **generic_decoders**: Reusable decoder types for common patterns (On/Off, Yes/No, etc.)
//! - **decoder_macros**: Declarative macros for creating decoders with minimal boilerplate
//! - **tag_registry**: Tag registration system for organizing tag definitions and decoders
//!
//! ## Duplication Reduction Impact
//!
//! These new utilities can reduce code duplication from ~500-1300% down to <50% by:
//! - Eliminating hundreds of nearly-identical decoder functions
//! - Providing pre-built decoders for common patterns
//! - Centralizing tag definitions in registries
//! - Using macros for declarative decoder creation

/// Array extraction utilities for IFD entries
pub mod array_extractors;
/// Array schema system for declarative CameraSettings-style array parsing
pub mod array_schemas;
/// Low-level byte parsing helper functions
pub mod byte_utils;
/// Declarative macros for creating decoders with minimal boilerplate
pub mod decoder_macros;
/// Generic decoders for common MakerNote value patterns
pub mod generic_decoders;
/// Shared IFD parsing implementation to eliminate parse() duplication
pub mod ifd_parser_base;
/// Unified lens database infrastructure
pub mod lens_database;
/// Common trait definition for all MakerNotes parsers
pub mod makernote_parser;
/// Tag registry system for organizing and managing MakerNote tags
pub mod tag_registry;
/// Common value interpretation and decoding functions
pub mod value_decoders;
/// Common value extraction helpers (strings, integers)
pub mod value_extractors;

pub use array_schemas::{ArrayIndexDef, ArraySchema};
pub use lens_database::{CombinedLensDb, LensDatabase, RangeLensDb, StaticLensDb};
pub use makernote_parser::MakerNoteParser;

#[cfg(test)]
mod tests;
