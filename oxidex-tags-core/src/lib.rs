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

use oxidex_tags_shared::find_table;
use std::sync::LazyLock;

pub mod types;
pub use oxidex_tags_shared::{Tag, TagDatabase, TagTable};
pub use types::*;

// Include pre-compiled binary tag data generated at build time
// This is significantly faster than parsing YAML at runtime
const CORE_TAGS_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/core_tags.bin"));

/// Lazily-initialized core tag database
///
/// Uses binary deserialization (bincode 2.0 serde API) instead of YAML parsing for faster initialization.
/// The Lazy wrapper ensures thread-safe initialization on first access.
pub static CORE_TAGS: LazyLock<TagDatabase> = LazyLock::new(|| {
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
    find_table(&CORE_TAGS, name).ok()
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

    #[test]
    fn test_forensic_timezone_tags() {
        // Test that critical forensic EXIF tags for timeline reconstruction are present
        let exif_table = get_tag_table("Exif::Main").expect("Exif::Main table should exist");

        // Timezone offset tags
        let offset_time = exif_table
            .tags
            .iter()
            .find(|tag| tag.id == "0x9010")
            .expect("OffsetTime (0x9010) should exist");
        assert_eq!(offset_time.name, "OffsetTime");

        let offset_time_original = exif_table
            .tags
            .iter()
            .find(|tag| tag.id == "0x9011")
            .expect("OffsetTimeOriginal (0x9011) should exist");
        assert_eq!(offset_time_original.name, "OffsetTimeOriginal");

        let offset_time_digitized = exif_table
            .tags
            .iter()
            .find(|tag| tag.id == "0x9012")
            .expect("OffsetTimeDigitized (0x9012) should exist");
        assert_eq!(offset_time_digitized.name, "OffsetTimeDigitized");

        // Subsecond precision tags
        let subsec_time = exif_table
            .tags
            .iter()
            .find(|tag| tag.id == "0x9290")
            .expect("SubSecTime (0x9290) should exist");
        assert_eq!(subsec_time.name, "SubSecTime");

        let subsec_time_original = exif_table
            .tags
            .iter()
            .find(|tag| tag.id == "0x9291")
            .expect("SubSecTimeOriginal (0x9291) should exist");
        assert_eq!(subsec_time_original.name, "SubSecTimeOriginal");

        let subsec_time_digitized = exif_table
            .tags
            .iter()
            .find(|tag| tag.id == "0x9292")
            .expect("SubSecTimeDigitized (0x9292) should exist");
        assert_eq!(subsec_time_digitized.name, "SubSecTimeDigitized");
    }
}
