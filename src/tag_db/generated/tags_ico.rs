//! ICO format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0002), "ICO:ImageType".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ICO:ImageCount".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageCount tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "ICO:IconDir".to_string(), FormatFamily::PNG, false, ValueType::String, "IconDir tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "ICO:ImageWidth".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ICO:ImageHeight".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ICO:NumColors".to_string(), FormatFamily::PNG, false, ValueType::String, "NumColors tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "ICO:ImageLength".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageLength tag".to_string(), vec!["Example".to_string()]),
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
