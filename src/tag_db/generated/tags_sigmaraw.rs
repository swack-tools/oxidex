//! SigmaRaw format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0001), "SigmaRaw:FileVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FileVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "SigmaRaw:ImageUniqueID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageUniqueID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "SigmaRaw:MarkBits".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MarkBits tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "SigmaRaw:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "SigmaRaw:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "SigmaRaw:Rotation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rotation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "SigmaRaw:WhiteBalance".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WhiteBalance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "SigmaRaw:SceneCaptureType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SceneCaptureType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "SigmaRaw:FileVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FileVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "SigmaRaw:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "SigmaRaw:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "SigmaRaw:Rotation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rotation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "SigmaRaw:Unused".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Unused tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "SigmaRaw:ExposureAdjust".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureAdjust tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "SigmaRaw:Contrast".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Contrast tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "SigmaRaw:Shadow".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Shadow tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "SigmaRaw:Highlight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Highlight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "SigmaRaw:Saturation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Saturation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "SigmaRaw:Sharpness".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Sharpness tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "SigmaRaw:RedAdjust".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RedAdjust tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "SigmaRaw:GreenAdjust".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GreenAdjust tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "SigmaRaw:BlueAdjust".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BlueAdjust tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "SigmaRaw:X3FillLight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "X3FillLight tag".to_string(), vec!["Example".to_string()]),
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
