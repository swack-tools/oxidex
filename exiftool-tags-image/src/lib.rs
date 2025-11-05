//! Image format metadata tags
//!
//! Contains tags for PNG, GIF, JPEG2000, TIFF, BMP, etc.

use once_cell::sync::Lazy;
pub use exiftool_tags_core::types::*;

const IMAGE_TAGS_YAML: &str = include_str!("image_tags.yaml");

pub static IMAGE_TAGS: Lazy<TagDatabase> = Lazy::new(|| {
    serde_yaml::from_str(IMAGE_TAGS_YAML)
        .expect("Failed to parse image tags YAML")
});

pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    IMAGE_TAGS.tables.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_tags_loads() {
        let _tags = &*IMAGE_TAGS;
        assert!(!IMAGE_TAGS.tables.is_empty());
    }
}
