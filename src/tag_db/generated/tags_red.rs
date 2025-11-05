//! Red format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x1003), "Red:OtherDate2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OtherDate2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1004), "Red:OtherDate3".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OtherDate3 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1005), "Red:DateTimeOriginal".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateTimeOriginal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1023), "Red:DateCreated".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateCreated tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1024), "Red:TimeCreated".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TimeCreated tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1031), "Red:StorageFormatTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "StorageFormatTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x106E), "Red:LensMake".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LensMake tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1071), "Red:Model".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Model tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x107C), "Red:CameraOperator".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CameraOperator tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1086), "Red:VideoFormat".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VideoFormat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x10A1), "Red:Sensor".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Sensor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x200D), "Red:ColorTemperature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorTemperature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4037), "Red:CropArea".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropArea tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x403B), "Red:ISO".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ISO tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x406A), "Red:FNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x406B), "Red:FocalLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FocalLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x606C), "Red:FocusDistance".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FocusDistance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Red:RedcodeVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RedcodeVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0036), "Red:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003A), "Red:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003E), "Red:FrameRate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FrameRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0043), "Red:OriginalFileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalFileName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Red:RedcodeVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RedcodeVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004C), "Red:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0050), "Red:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0056), "Red:FrameRate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FrameRate tag".to_string(), vec!["Example".to_string()]),
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
