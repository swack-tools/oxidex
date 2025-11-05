//! OpenEXR format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x000A), "OpenEXR:Long names".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Long names tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "OpenEXR:Deep data".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Deep data tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "OpenEXR:Multipart".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Multipart tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "OpenEXR:RLE".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RLE tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "OpenEXR:ZIPS".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZIPS tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "OpenEXR:ZIP".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZIP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "OpenEXR:PIZ".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PIZ tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "OpenEXR:PXR24".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PXR24 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "OpenEXR:B44".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "B44 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "OpenEXR:B44A".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "B44A tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "OpenEXR:Cube".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Cube tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "OpenEXR:Decreasing Y".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Decreasing Y tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "OpenEXR:Random Y".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Random Y tag".to_string(), vec!["Example".to_string()]),
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
