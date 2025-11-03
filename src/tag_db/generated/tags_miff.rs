//! MIFF format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xF2B4), "MIFF:EXIF_Profile".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EXIF_Profile tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x9865), "MIFF:ICC_Profile".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ICC_Profile tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xA77A), "MIFF:IPTC_Profile".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTC_Profile tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD1F7), "MIFF:XMP_Profile".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XMP_Profile tag".to_string(), vec!["Example".to_string()]),
]);

pub fn get_tags() -> &'static HashMap<String, TagDescriptor> {
    static MAP: Lazy<HashMap<String, TagDescriptor>> = Lazy::new(|| {
        let mut map = HashMap::with_capacity(TAGS.len());
        for tag in TAGS.iter() {
            map.insert(tag.tag_name.clone(), tag.clone());
        }
        map
    });
    &MAP
}
