//! Domain layer (hexagonal core)
//!
//! This module contains the core domain logic for metadata management,
//! including MetadataMap, TagValue, TagDescriptor, and core operations.
//! This layer is format-agnostic and contains no I/O or format-specific code.

#![allow(dead_code)]

pub mod binary_decoders;
pub mod date_shift;
pub mod exif_enums;
pub mod exiftool_compat;
pub mod file_format;
pub mod file_metadata;
pub mod file_reader_trait;
pub mod flag_utils;
pub mod format_dispatch;
pub mod format_parser_trait;
pub mod formatters;
pub mod jpeg_helpers;
pub mod metadata;
pub mod metadata_map;
pub mod operations;
pub mod operations_helpers;
pub mod tag_conversion;
pub mod tag_normalization;
pub mod tag_value;
pub mod tiff_helpers;
pub mod validation;
pub mod value_formatter;

// Re-export tag descriptor types from exiftool-tags crate
pub use oxidex_tags::{FormatFamily, TagDescriptor, TagId, ValueType};

// Re-export commonly used types for convenience
pub use file_format::FileFormat;
pub use file_reader_trait::FileReader;
pub use flag_utils::decode_flags;
pub use format_parser_trait::FormatParser;
pub use metadata::Metadata;
pub use metadata_map::MetadataMap;
pub use operations::{clear_all_metadata, modify_tag, read_metadata, remove_tag, write_metadata};
pub use tag_normalization::{normalize_metadata_map, normalize_tag_family};
pub use tag_value::TagValue;
pub use validation::validate_tag_value;
