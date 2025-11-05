//! Specialty format metadata tags
//!
//! Contains tags for DICOM, FITS, MRC, and other medical/scientific formats

pub use exiftool_tags_core::types::*;
use once_cell::sync::Lazy;

const SPECIALTY_TAGS_YAML: &str = include_str!("specialty_tags.yaml");

pub static SPECIALTY_TAGS: Lazy<TagDatabase> = Lazy::new(|| {
    serde_yaml::from_str(SPECIALTY_TAGS_YAML).expect("Failed to parse specialty tags YAML")
});

pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    SPECIALTY_TAGS.tables.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_specialty_tags_loads() {
        let _tags = &*SPECIALTY_TAGS;
        assert!(!SPECIALTY_TAGS.tables.is_empty());
    }
}
