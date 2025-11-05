//! Motorola format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x5500), "Motorola:BuildNumber".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "BuildNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5501), "Motorola:SerialNumber".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "SerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x64D0), "Motorola:DriveMode".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "DriveMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x665E), "Motorola:Sensor".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Sensor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6705), "Motorola:ManufactureDate".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "ManufactureDate tag".to_string(), vec!["Example".to_string()]),
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
