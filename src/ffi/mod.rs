//! C FFI bindings
//!
//! This module provides C-compatible function exports for cross-language integration.
//!
//! # Module Organization
//! - `error`: Error codes and error handling utilities
//! - `context`: Internal context structure and opaque handle types
//! - `lifecycle`: Handle creation and destruction functions
//! - `read_tags`: Functions for reading metadata from files
//! - `write_tags`: Functions for writing metadata to files
//!
//! # Safety
//!
//! All functions are designed to be safe to call from C. Panics are caught at the FFI
//! boundary and converted to error codes. However, callers must follow the documented
//! contracts (e.g., not passing NULL where prohibited, destroying handles properly).

#![allow(dead_code)]
// Clippy warning about raw pointer dereferencing in public functions is not applicable
// to FFI functions - they must accept raw pointers and cannot be marked `unsafe` on the
// Rust side because they're called from C.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

// Module declarations
pub mod context;
pub mod error;
pub mod lifecycle;
pub mod read_tags;
pub mod write_tags;

// Re-export error codes for C API consumers
pub use error::{
    EXIFTOOL_ERR_INTERNAL, EXIFTOOL_ERR_INVALID_TAG_VALUE, EXIFTOOL_ERR_IO,
    EXIFTOOL_ERR_NULL_POINTER, EXIFTOOL_ERR_PARSE, EXIFTOOL_ERR_TAG_NOT_FOUND,
    EXIFTOOL_ERR_UNSUPPORTED_FORMAT, EXIFTOOL_OK,
};

// Re-export the opaque handle type
pub use context::ExifToolHandle;

// Re-export all FFI functions for C API
pub use error::exiftool_get_last_error;
pub use lifecycle::{exiftool_create, exiftool_destroy};
pub use read_tags::{
    exiftool_get_tag_count, exiftool_get_tag_float, exiftool_get_tag_integer,
    exiftool_get_tag_name_at, exiftool_get_tag_string, exiftool_has_tag, exiftool_read_file,
};
pub use write_tags::{
    exiftool_remove_tag, exiftool_set_tag_float, exiftool_set_tag_integer, exiftool_set_tag_string,
    exiftool_write_file,
};
