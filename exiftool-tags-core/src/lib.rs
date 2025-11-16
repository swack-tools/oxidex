//! Core metadata tags for exiftool-rs
//!
//! Contains universal metadata standards: EXIF, XMP, IPTC, GPS, ICC Profile
//!
//! ## Performance Note
//!
//! This crate uses pre-compiled binary tag data instead of runtime YAML parsing.
//! Tag definitions are serialized to binary format at build time and embedded
//! directly in the compiled binary, eliminating the ~40ms cold start penalty
//! from parsing YAML files on first access.

use once_cell::sync::Lazy;

pub mod types;
pub use types::*;

// Include pre-compiled binary tag data generated at build time
// This is significantly faster than parsing YAML at runtime
const CORE_TAGS_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/core_tags.bin"));

/// Lazily-initialized core tag database
///
/// Uses binary deserialization (bincode 2.0 serde API) instead of YAML parsing for faster initialization.
/// The Lazy wrapper ensures thread-safe initialization on first access.
pub static CORE_TAGS: Lazy<TagDatabase> = Lazy::new(|| {
    bincode::serde::decode_from_slice(CORE_TAGS_BIN, bincode::config::legacy())
        .expect("Failed to deserialize pre-compiled core tags binary data")
        .0 // decode_from_slice returns (T, usize), extract the decoded value
});

/// Get a specific tag table by name
///
/// # Arguments
///
/// * `name` - The name of the tag table to retrieve (e.g., "Exif::Main", "GPS::Main")
///
/// # Returns
///
/// An Option containing a reference to the TagTable if found, or None if not found
pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    CORE_TAGS.tables.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_tags_loads() {
        // Force initialization and verify tags loaded successfully
        let _tags = &*CORE_TAGS;
        assert!(!CORE_TAGS.tables.is_empty());
    }

    #[test]
    fn test_get_tag_table() {
        // Test with actual table names from YAML
        let exif = get_tag_table("Exif::Main");
        assert!(exif.is_some(), "Should find Exif::Main table");

        let gps = get_tag_table("GPS::Main");
        assert!(gps.is_some(), "Should find GPS::Main table");
    }
}
