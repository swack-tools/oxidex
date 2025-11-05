//! BPG format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0004), "BPG:PixelFormat".to_string(), FormatFamily::PNG, false, ValueType::String, "PixelFormat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "BPG:4:2:0 (chroma at 0.5, 0.5)".to_string(), FormatFamily::PNG, false, ValueType::String, "4:2:0 (chroma at 0.5, 0.5) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "BPG:4:2:2 (chroma at 0.5, 0)".to_string(), FormatFamily::PNG, false, ValueType::String, "4:2:2 (chroma at 0.5, 0) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "BPG:4:4:4".to_string(), FormatFamily::PNG, false, ValueType::String, "4:4:4 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "BPG:4:2:0 (chroma at 0, 0.5)".to_string(), FormatFamily::PNG, false, ValueType::String, "4:2:0 (chroma at 0, 0.5) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "BPG:4:2:2 (chroma at 0, 0)".to_string(), FormatFamily::PNG, false, ValueType::String, "4:2:2 (chroma at 0, 0) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1000), "BPG:Alpha Exists (color not premultiplied)".to_string(), FormatFamily::PNG, false, ValueType::String, "Alpha Exists (color not premultiplied) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1004), "BPG:Alpha Exists (color premultiplied)".to_string(), FormatFamily::PNG, false, ValueType::String, "Alpha Exists (color premultiplied) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "BPG:Alpha Exists (W color component)".to_string(), FormatFamily::PNG, false, ValueType::String, "Alpha Exists (W color component) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "BPG:RGB".to_string(), FormatFamily::PNG, false, ValueType::String, "RGB tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "BPG:YCgCo".to_string(), FormatFamily::PNG, false, ValueType::String, "YCgCo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "BPG:YCbCr (BT 709)".to_string(), FormatFamily::PNG, false, ValueType::String, "YCbCr (BT 709) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "BPG:YCbCr (BT 2020)".to_string(), FormatFamily::PNG, false, ValueType::String, "YCbCr (BT 2020) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "BPG:BT 2020 Constant Luminance".to_string(), FormatFamily::PNG, false, ValueType::String, "BT 2020 Constant Luminance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "BPG:Limited Range".to_string(), FormatFamily::PNG, false, ValueType::String, "Limited Range tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "BPG:Extension Present".to_string(), FormatFamily::PNG, false, ValueType::String, "Extension Present tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "BPG:ImageWidth".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "BPG:ImageHeight".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "BPG:ImageLength".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "BPG:EXIF".to_string(), FormatFamily::PNG, false, ValueType::String, "EXIF tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "BPG:ICC_Profile".to_string(), FormatFamily::PNG, false, ValueType::String, "ICC_Profile tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "BPG:XMP".to_string(), FormatFamily::PNG, false, ValueType::String, "XMP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "BPG:ThumbnailBPG".to_string(), FormatFamily::PNG, false, ValueType::String, "ThumbnailBPG tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "BPG:AnimationControl".to_string(), FormatFamily::PNG, false, ValueType::String, "AnimationControl tag".to_string(), vec!["Example".to_string()]),
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
