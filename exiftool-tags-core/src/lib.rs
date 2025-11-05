//! Core metadata tags for exiftool-rs
//!
//! Contains universal metadata standards: EXIF, XMP, IPTC, GPS, ICC Profile

use once_cell::sync::Lazy;

pub mod types;
pub use types::*;

// Embed YAML data at compile time
const CORE_TAGS_YAML: &str = include_str!("core_tags.yaml");

/// Lazily-initialized core tag database
pub static CORE_TAGS: Lazy<TagDatabase> = Lazy::new(|| {
    serde_yaml::from_str(CORE_TAGS_YAML)
        .expect("Failed to parse core tags YAML")
});

/// Get a specific tag table by name
pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    CORE_TAGS.tables.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_tags_loads() {
        // Force initialization
        let _tags = &*CORE_TAGS;
        assert!(!CORE_TAGS.tables.is_empty());
    }

    #[test]
    fn test_get_tag_table() {
        let exif = get_tag_table("EXIF");
        assert!(exif.is_some());
    }
}
