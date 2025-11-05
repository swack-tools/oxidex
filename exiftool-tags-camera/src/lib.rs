//! Camera manufacturer metadata tags
//!
//! Contains tags for Canon, Nikon, Sony, Panasonic, Olympus, Fujifilm, etc.

pub use exiftool_tags_core::types::*;
use once_cell::sync::Lazy;

const CAMERA_TAGS_YAML: &str = include_str!("camera_tags.yaml");

pub static CAMERA_TAGS: Lazy<TagDatabase> =
    Lazy::new(|| serde_yaml::from_str(CAMERA_TAGS_YAML).expect("Failed to parse camera tags YAML"));

pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    CAMERA_TAGS.tables.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_tags_loads() {
        let _tags = &*CAMERA_TAGS;
        assert!(!CAMERA_TAGS.tables.is_empty());
    }
}
