//! PSP format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0001), "PSP:CreatorInfo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CreatorInfo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "PSP:ExtendedInfo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExtendedInfo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "PSP:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "PSP:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "PSP:ImageResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "ImageResolution tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "PSP:ResolutionUnit".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ResolutionUnit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PSP:inches".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "inches tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PSP:cm".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "cm tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "PSP:Compression".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Compression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PSP:RLE".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RLE tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PSP:LZ77".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LZ77 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PSP:JPEG".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JPEG tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "PSP:BitsPerSample".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "BitsPerSample tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "PSP:Planes".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "Planes tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0017), "PSP:NumColors".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "NumColors tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PSP:CreateDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CreateDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PSP:ModifyDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ModifyDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PSP:Artist".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Artist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "PSP:Copyright".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Copyright tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "PSP:Description".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Description tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "PSP:CreatorAppID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CreatorAppID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PSP:Paint Shop Pro".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Paint Shop Pro tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "PSP:CreatorAppVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CreatorAppVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PSP:EXIFInfo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EXIFInfo tag".to_string(), vec!["Example".to_string()]),
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
