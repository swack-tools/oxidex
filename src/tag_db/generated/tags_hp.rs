//! HP format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0E00), "HP:PrintIM".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PrintIM tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4693), "HP:PreviewImage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreviewImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "HP:MaxAperture".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MaxAperture tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "HP:ExposureTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "HP:CameraDateTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CameraDateTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0034), "HP:ISO".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ISO tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x005C), "HP:SerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "HP:FNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "HP:ExposureTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "HP:CameraDateTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CameraDateTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0034), "HP:ISO".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ISO tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0058), "HP:SerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SerialNumber tag".to_string(), vec!["Example".to_string()]),
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
