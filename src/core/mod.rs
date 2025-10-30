//! Domain layer (hexagonal core)
//!
//! This module contains the core domain logic for metadata management,
//! including MetadataMap, TagValue, TagDescriptor, and core operations.
//! This layer is format-agnostic and contains no I/O or format-specific code.

#![allow(dead_code)]

pub mod date_shift;
pub mod file_format;
pub mod file_reader_trait;
pub mod format_parser_trait;
pub mod metadata_map;
pub mod operations;
pub mod tag_descriptor;
pub mod tag_value;
pub mod validation;

// Re-export commonly used types for convenience
pub use file_format::FileFormat;
pub use file_reader_trait::FileReader;
pub use format_parser_trait::FormatParser;
pub use metadata_map::MetadataMap;
pub use operations::{modify_tag, read_metadata, write_metadata};
pub use tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
pub use tag_value::TagValue;
pub use validation::validate_tag_value;
