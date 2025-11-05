//! DSF format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0003), "DSF:FormatVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FormatVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "DSF:FormatID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FormatID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "DSF:ChannelType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ChannelType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "DSF:Stereo (Left, Right)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Stereo (Left, Right) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "DSF:3 Channels (Left, Right, Center)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "3 Channels (Left, Right, Center) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "DSF:Quad (Left, Right, Back L, Back R)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Quad (Left, Right, Back L, Back R) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "DSF:4 Channels (Left, Right, Center, Bass)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "4 Channels (Left, Right, Center, Bass) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "DSF:5 Channels (Left, Right, Center, Back L, Back R)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "5 Channels (Left, Right, Center, Back L, Back R) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "DSF:5.1 Channels (Left, Right, Center, Bass, Back L, Back R)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "5.1 Channels (Left, Right, Center, Bass, Back L, Back R) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "DSF:ChannelCount".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ChannelCount tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "DSF:SampleRate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SampleRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "DSF:BitsPerSample".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BitsPerSample tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "DSF:SampleCount".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "SampleCount tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "DSF:BlockSize".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BlockSize tag".to_string(), vec!["Example".to_string()]),
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
