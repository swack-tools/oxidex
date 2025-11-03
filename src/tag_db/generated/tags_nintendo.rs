//! Nintendo format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x1101), "Nintendo:CameraInfo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CameraInfo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Nintendo:ModelID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ModelID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Nintendo:TimeStamp".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TimeStamp tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "Nintendo:InternalSerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "InternalSerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0028), "Nintendo:Parallax".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Parallax tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0030), "Nintendo:Category".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Category tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1000), "Nintendo:Mii".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Mii tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2000), "Nintendo:Man".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Man tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4000), "Nintendo:Woman".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Woman tag".to_string(), vec!["Example".to_string()]),
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
