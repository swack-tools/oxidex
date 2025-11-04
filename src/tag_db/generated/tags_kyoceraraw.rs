//! KyoceraRaw format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0001), "KyoceraRaw:FirmwareVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FirmwareVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "KyoceraRaw:Model".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Model tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0019), "KyoceraRaw:Make".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Make tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0021), "KyoceraRaw:DateTimeOriginal".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateTimeOriginal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0034), "KyoceraRaw:ISO".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ISO tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0038), "KyoceraRaw:ExposureTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003C), "KyoceraRaw:WB_RGGBLevels".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGGBLevels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0058), "KyoceraRaw:FNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0068), "KyoceraRaw:MaxAperture".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MaxAperture tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0070), "KyoceraRaw:FocalLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FocalLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x007C), "KyoceraRaw:Lens".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Lens tag".to_string(), vec!["Example".to_string()]),
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
