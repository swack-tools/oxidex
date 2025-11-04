//! PCX format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "PCX:Manufacturer".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Manufacturer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PCX:Software".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Software tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PCX:PC Paintbrush 2.8 (with palette)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PC Paintbrush 2.8 (with palette) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PCX:PC Paintbrush 2.8 (without palette)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PC Paintbrush 2.8 (without palette) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "PCX:PC Paintbrush for Windows".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PC Paintbrush for Windows tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "PCX:PC Paintbrush 3.0+".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PC Paintbrush 3.0+ tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PCX:Encoding".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Encoding tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PCX:BitsPerPixel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BitsPerPixel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "PCX:LeftMargin".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LeftMargin tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "PCX:TopMargin".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TopMargin tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "PCX:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "PCX:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "PCX:XResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "PCX:YResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0041), "PCX:ColorPlanes".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorPlanes tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0042), "PCX:BytesPerLine".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "BytesPerLine tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0044), "PCX:ColorMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PCX:Color Palette".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Color Palette tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PCX:Grayscale".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Grayscale tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0046), "PCX:ScreenWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ScreenWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0048), "PCX:ScreenHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ScreenHeight tag".to_string(), vec!["100".to_string()]),
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
