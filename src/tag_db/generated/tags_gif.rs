//! GIF format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xF6BB), "GIF:XMP".to_string(), FormatFamily::PNG, false, ValueType::String, "XMP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7E30), "GIF:ICC_Profile".to_string(), FormatFamily::PNG, false, ValueType::String, "ICC_Profile tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7826), "GIF:MIDIControl".to_string(), FormatFamily::PNG, false, ValueType::String, "MIDIControl tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2ED5), "GIF:MIDISong".to_string(), FormatFamily::PNG, false, ValueType::String, "MIDISong tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6F6A), "GIF:JUMBF".to_string(), FormatFamily::PNG, false, ValueType::String, "JUMBF tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "GIF:ImageWidth".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GIF:ImageHeight".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "GIF:BackgroundColor".to_string(), FormatFamily::PNG, false, ValueType::String, "BackgroundColor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "GIF:PixelAspectRatio".to_string(), FormatFamily::PNG, false, ValueType::String, "PixelAspectRatio tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GIF:AnimationIterations".to_string(), FormatFamily::PNG, false, ValueType::String, "AnimationIterations tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "GIF:MIDIControlVersion".to_string(), FormatFamily::PNG, false, ValueType::String, "MIDIControlVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GIF:SequenceNumber".to_string(), FormatFamily::PNG, false, ValueType::String, "SequenceNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GIF:MelodicPolyphony".to_string(), FormatFamily::PNG, false, ValueType::String, "MelodicPolyphony tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GIF:PercussivePolyphony".to_string(), FormatFamily::PNG, false, ValueType::String, "PercussivePolyphony tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "GIF:ChannelUsage".to_string(), FormatFamily::PNG, false, ValueType::String, "ChannelUsage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "GIF:DelayTime".to_string(), FormatFamily::PNG, false, ValueType::String, "DelayTime tag".to_string(), vec!["Example".to_string()]),
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
