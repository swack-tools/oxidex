//! Document format metadata tags
//!
//! Contains tags for PDF, PostScript, fonts, archives, etc.

use once_cell::sync::Lazy;
pub use exiftool_tags_core::types::*;

const DOCUMENT_TAGS_YAML: &str = include_str!("document_tags.yaml");

pub static DOCUMENT_TAGS: Lazy<TagDatabase> = Lazy::new(|| {
    serde_yaml::from_str(DOCUMENT_TAGS_YAML)
        .expect("Failed to parse document tags YAML")
});

pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    DOCUMENT_TAGS.tables.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_tags_loads() {
        let _tags = &*DOCUMENT_TAGS;
        assert!(!DOCUMENT_TAGS.tables.is_empty());
    }
}
