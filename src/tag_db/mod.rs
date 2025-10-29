//! Generated tag database
//!
//! Contains the tag registry and generated tag definitions from ExifTool specifications.

#![allow(dead_code)]

pub mod generated_tags;
pub mod tag_registry;

// Re-export commonly used registry functions
pub use tag_registry::{get_tag_descriptor, tag_count};
