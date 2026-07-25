//! Camera manufacturer metadata tags
//!
//! Contains tags for Canon, Nikon, Sony, Panasonic, Olympus, Fujifilm, etc.
//!
//! ## Performance Note
//!
//! This crate uses pre-compiled binary tag data instead of runtime YAML parsing.
//! Tag definitions are serialized to binary format at build time and embedded
//! directly in the compiled binary, eliminating the ~40ms cold start penalty
//! from parsing YAML files on first access.

pub use oxidex_tags_core::types::*;
use oxidex_tags_shared::find_table;
pub use oxidex_tags_shared::{Tag, TagDatabase, TagTable};
use std::sync::LazyLock;

// Include pre-compiled binary tag data generated at build time
// This is significantly faster than parsing YAML at runtime
const CAMERA_TAGS_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/camera_tags.bin"));

/// Lazily-initialized camera tag database
///
/// Uses binary deserialization (bincode 2.0 serde API) instead of YAML parsing for faster initialization.
/// The Lazy wrapper ensures thread-safe initialization on first access.
pub static CAMERA_TAGS: LazyLock<TagDatabase> = LazyLock::new(|| {
    bincode::serde::decode_from_slice(CAMERA_TAGS_BIN, bincode::config::legacy())
        .expect("Failed to deserialize pre-compiled camera tags binary data")
        .0 // decode_from_slice returns (T, usize), extract the decoded value
});

/// Get a specific tag table by name
///
/// # Arguments
///
/// * `name` - The name of the tag table to retrieve (e.g., "Canon::Main", "Nikon::Main")
///
/// # Returns
///
/// An Option containing a reference to the TagTable if found, or None if not found
pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    find_table(&CAMERA_TAGS, name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidex_tags_shared::validate_database;

    #[test]
    fn test_camera_tags_loads() {
        // Force initialization and verify tags loaded successfully
        let _tags = &*CAMERA_TAGS;
        assert!(!CAMERA_TAGS.tables.is_empty());
        validate_database(&CAMERA_TAGS).expect("Camera tags should satisfy shared validation");
    }
}
