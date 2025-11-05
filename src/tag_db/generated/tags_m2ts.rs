//! M2TS format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0001), "M2TS:44100".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "44100 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "M2TS:32000".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "32000 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "M2TS:32000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "32000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0021), "M2TS:40000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "40000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0022), "M2TS:48000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "48000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0023), "M2TS:56000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "56000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0024), "M2TS:64000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "64000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0025), "M2TS:80000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "80000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0026), "M2TS:96000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "96000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0027), "M2TS:112000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "112000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0028), "M2TS:128000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "128000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0029), "M2TS:160000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "160000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002A), "M2TS:192000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "192000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002B), "M2TS:224000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "224000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002C), "M2TS:256000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "256000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002D), "M2TS:320000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "320000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002E), "M2TS:384000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "384000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002F), "M2TS:448000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "448000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0030), "M2TS:512000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "512000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0031), "M2TS:576000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "576000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0032), "M2TS:640000 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "640000 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "M2TS:Not Dolby surround".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Not Dolby surround tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "M2TS:Dolby surround".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Dolby surround tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "M2TS:2/1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "2/1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "M2TS:3/1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "3/1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "M2TS:2/2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "2/2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "M2TS:3/2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "3/2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "M2TS:2 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "2 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "M2TS:3 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "3 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "M2TS:4 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "4 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "M2TS:5 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "5 max tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "M2TS:6 max".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "6 max tag".to_string(), vec!["Example".to_string()]),
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
