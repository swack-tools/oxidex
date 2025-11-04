//! AAC format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xBD51), "AAC:ProfileType".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ProfileType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "AAC:Low Complexity".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Low Complexity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "AAC:Scalable Sampling Rate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Scalable Sampling Rate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xEC6C), "AAC:SampleRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "SampleRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4F8A), "AAC:Channels".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Channels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "AAC:5+1".to_string(), FormatFamily::QuickTime, false, ValueType::String, "5+1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "AAC:7+1".to_string(), FormatFamily::QuickTime, false, ValueType::String, "7+1 tag".to_string(), vec!["Example".to_string()]),
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
