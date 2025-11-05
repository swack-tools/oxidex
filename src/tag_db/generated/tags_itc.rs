//! ITC format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0010), "ITC:DataType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DataType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "ITC:LibraryID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LibraryID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ITC:TrackID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TrackID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ITC:DataLocation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DataLocation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "ITC:ImageType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "ITC:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "ITC:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
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
