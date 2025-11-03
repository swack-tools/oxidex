//! DjVu format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0002), "DjVu:ImageHeight".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "DjVu:DjVuVersion".to_string(), FormatFamily::PNG, false, ValueType::String, "DjVuVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "DjVu:SpatialResolution".to_string(), FormatFamily::PNG, false, ValueType::String, "SpatialResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "DjVu:Gamma".to_string(), FormatFamily::PNG, false, ValueType::String, "Gamma tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "DjVu:Orientation".to_string(), FormatFamily::PNG, false, ValueType::String, "Orientation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "DjVu:Rotate 180".to_string(), FormatFamily::PNG, false, ValueType::String, "Rotate 180 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "DjVu:Rotate 90 CW".to_string(), FormatFamily::PNG, false, ValueType::String, "Rotate 90 CW tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "DjVu:Rotate 270 CW".to_string(), FormatFamily::PNG, false, ValueType::String, "Rotate 270 CW tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "DjVu:SubfileType".to_string(), FormatFamily::PNG, false, ValueType::String, "SubfileType tag".to_string(), vec!["Example".to_string()]),
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
