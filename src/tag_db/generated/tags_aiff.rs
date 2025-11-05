//! AIFF format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xA792), "AIFF:Copyright".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Copyright tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x34A8), "AIFF:ID3".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ID3 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "AIFF:NumChannels".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "NumChannels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "AIFF:NumSampleFrames".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "NumSampleFrames tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "AIFF:SampleSize".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SampleSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "AIFF:SampleRate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SampleRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "AIFF:CompressionType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CompressionType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "AIFF:CompressorName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CompressorName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "AIFF:FormatVersionTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FormatVersionTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "AIFF:CommentTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CommentTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "AIFF:MarkerID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MarkerID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "AIFF:Comment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Comment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "AIFF:AIFF:NumSampleFrames".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AIFF:NumSampleFrames tag".to_string(), vec!["Example".to_string()]),
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
