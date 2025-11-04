//! APE format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xF650), "APE:ToolVersion".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ToolVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD413), "APE:ToolName".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ToolName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "APE:APEVersion".to_string(), FormatFamily::QuickTime, false, ValueType::String, "APEVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "APE:CompressionLevel".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CompressionLevel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "APE:Channels".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Channels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "APE:SampleRate".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "SampleRate tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "APE:TotalFrames".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "TotalFrames tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "APE:FinalFrameBlocks".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "FinalFrameBlocks tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "APE:CompressionLevel".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CompressionLevel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "APE:BlocksPerFrame".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "BlocksPerFrame tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "APE:FinalFrameBlocks".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "FinalFrameBlocks tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "APE:TotalFrames".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "TotalFrames tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "APE:BitsPerSample".to_string(), FormatFamily::QuickTime, false, ValueType::String, "BitsPerSample tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "APE:Channels".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Channels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "APE:SampleRate".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "SampleRate tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "APE:APE:TotalFrames".to_string(), FormatFamily::QuickTime, false, ValueType::String, "APE:TotalFrames tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "APE:APE:BlocksPerFrame".to_string(), FormatFamily::QuickTime, false, ValueType::String, "APE:BlocksPerFrame tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "APE:APE:FinalFrameBlocks".to_string(), FormatFamily::QuickTime, false, ValueType::String, "APE:FinalFrameBlocks tag".to_string(), vec!["Example".to_string()]),
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
