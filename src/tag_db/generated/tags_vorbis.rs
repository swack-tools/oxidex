//! Vorbis format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0001), "Vorbis:Identification".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Identification tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Vorbis:Comments".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Comments tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Vorbis:VorbisVersion".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VorbisVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Vorbis:AudioChannels".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioChannels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "Vorbis:SampleRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "SampleRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Vorbis:MaximumBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MaximumBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "Vorbis:NominalBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "NominalBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "Vorbis:MinimumBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MinimumBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Vorbis:FileSize".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FileSize tag".to_string(), vec!["Example".to_string()]),
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
