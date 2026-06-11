//! Compatibility facade for generated tag database callers.
//!
//! Tag data is now exposed through the active registry backed by the
//! `oxidex-tags-*` domain crates.

#![allow(dead_code)]

use crate::core::TagDescriptor;

/// Legacy lookup function that delegates to the active registry.
pub fn get_generated_tag_descriptor(name: &str) -> Option<&TagDescriptor> {
    crate::tag_db::tag_registry::get_tag_descriptor(name)
}

/// Returns the number of tags reachable through the active registry.
pub fn generated_tag_count() -> usize {
    crate::tag_db::tag_registry::tag_count()
}
