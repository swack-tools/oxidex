//! Fallback tag database
//!
//! Tag generation from ExifTool source failed during build.
//! This file provides a fallback that delegates to the manually
//! curated tag registry to ensure the build succeeds.

#![allow(dead_code)]

use crate::core::tag_descriptor::TagDescriptor;

/// Fallback lookup function (delegates to manual registry)
pub fn get_generated_tag_descriptor(name: &str) -> Option<&TagDescriptor> {
    crate::tag_db::tag_registry::get_tag_descriptor(name)
}

/// Fallback tag count function (delegates to manual registry)
pub fn generated_tag_count() -> usize {
    crate::tag_db::tag_registry::tag_count()
}

