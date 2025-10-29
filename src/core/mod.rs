//! Domain layer (hexagonal core)
//!
//! This module contains the core domain logic for metadata management,
//! including MetadataMap, TagValue, TagDescriptor, and core operations.
//! This layer is format-agnostic and contains no I/O or format-specific code.

#![allow(dead_code)]

pub mod metadata_map;
pub mod tag_value;
pub mod tag_descriptor;
pub mod operations;
pub mod validation;
pub mod format_parser_trait;
pub mod file_reader_trait;
