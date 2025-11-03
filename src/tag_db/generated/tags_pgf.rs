//! PGF format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0003), "PGF:PGFVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PGFVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "PGF:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "PGF:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "PGF:PyramidLevels".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PyramidLevels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "PGF:Quality".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Quality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "PGF:BitsPerPixel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BitsPerPixel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "PGF:ColorComponents".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorComponents tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "PGF:ColorMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PGF:Grayscale".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Grayscale tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PGF:Indexed".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Indexed tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PGF:RGB".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RGB tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "PGF:CMYK".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CMYK tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "PGF:Multichannel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Multichannel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "PGF:Duotone".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Duotone tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "PGF:Lab".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Lab tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "PGF:BackgroundColor".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BackgroundColor tag".to_string(), vec!["Example".to_string()]),
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
