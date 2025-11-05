//! MPC format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x1D77), "MPC:SampleRate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SampleRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7B81), "MPC:Quality".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Quality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MPC:0".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "0 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MPC:1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "MPC:2 (Telephone)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "2 (Telephone) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "MPC:3 (Thumb)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "3 (Thumb) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "MPC:4 (Radio)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "4 (Radio) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "MPC:5 (Standard)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "5 (Standard) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "MPC:6 (Xtreme)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "6 (Xtreme) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "MPC:7 (Insane)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "7 (Insane) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "MPC:8 (BrainDead)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "8 (BrainDead) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "MPC:9".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "9 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "MPC:10".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "10 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF6C6), "MPC:FastSeek".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FastSeek tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF6FC), "MPC:Gapless".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Gapless tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3B70), "MPC:EncoderVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EncoderVersion tag".to_string(), vec!["Example".to_string()]),
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
