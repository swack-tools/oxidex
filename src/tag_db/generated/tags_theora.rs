//! Theora format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0080), "Theora:Identification".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Identification tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0081), "Theora:Comments".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Comments tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Theora:TheoraVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TheoraVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Theora:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "Theora:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "Theora:XOffset".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XOffset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "Theora:YOffset".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YOffset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "Theora:FrameRate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FrameRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0017), "Theora:PixelAspectRatio".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PixelAspectRatio tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001D), "Theora:ColorSpace".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorSpace tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Theora:Rec. 470M".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rec. 470M tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Theora:Rec. 470BG".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rec. 470BG tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001E), "Theora:NominalVideoBitrate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "NominalVideoBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0021), "Theora:Quality".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Quality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0022), "Theora:PixelFormat".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PixelFormat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Theora:4:2:2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "4:2:2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Theora:4:4:4".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "4:4:4 tag".to_string(), vec!["Example".to_string()]),
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
