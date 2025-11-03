//! VCard format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xEAFE), "VCard:ABLabel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ABLabel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xDA64), "VCard:ABDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ABDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAC00), "VCard:ModifyDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ModifyDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5C07), "VCard:Importance".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Importance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "VCard:Normal".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Normal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "VCard:High".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "High tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2CD3), "VCard:InstanceType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "InstanceType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "VCard:Recurring Appointment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Recurring Appointment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "VCard:Single Instance of Recurring Appointment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Single Instance of Recurring Appointment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "VCard:Exception to Recurring Appointment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Exception to Recurring Appointment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAC4A), "VCard:MeetingLocations".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MeetingLocations tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x90B6), "VCard:TimeZone2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TimeZone2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAC00), "VCard:ModifyDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ModifyDate tag".to_string(), vec!["Example".to_string()]),
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
