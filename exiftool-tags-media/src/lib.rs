//! Audio/video format metadata tags
//!
//! Contains tags for QuickTime, Matroska, MPEG, FLAC, AAC, etc.

pub use exiftool_tags_core::types::*;
use once_cell::sync::Lazy;

const MEDIA_TAGS_YAML: &str = include_str!("media_tags.yaml");

pub static MEDIA_TAGS: Lazy<TagDatabase> =
    Lazy::new(|| serde_yaml::from_str(MEDIA_TAGS_YAML).expect("Failed to parse media tags YAML"));

pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    MEDIA_TAGS.tables.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_tags_loads() {
        let _tags = &*MEDIA_TAGS;
        assert!(!MEDIA_TAGS.tables.is_empty());
    }
}
