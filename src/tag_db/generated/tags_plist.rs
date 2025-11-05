//! PLIST format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x606A), "PLIST:Cast".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Cast tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0592), "PLIST:Directors".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Directors tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x16CC), "PLIST:Producers".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Producers tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x81DF), "PLIST:Screenwriters".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Screenwriters tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x13A6), "PLIST:Codirectors".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Codirectors tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x01C1), "PLIST:Studio".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Studio tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x13F9), "PLIST:DateTimeOriginal".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateTimeOriginal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xCC01), "PLIST:Duration".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Duration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xCD62), "PLIST:GPSLatitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLatitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0779), "PLIST:GPSLongitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLongitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x8D69), "PLIST:GPSMapDatum".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSMapDatum tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7C73), "PLIST:SlowMotionRegionsStartTimeFlags".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SlowMotionRegionsStartTimeFlags tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PLIST:Has been rounded".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Has been rounded tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PLIST:Positive infinity".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Positive infinity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PLIST:Negative infinity".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Negative infinity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "PLIST:Indefinite".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Indefinite tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB1D3), "PLIST:SlowMotionRegionsDurationFlags".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SlowMotionRegionsDurationFlags tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PLIST:Has been rounded".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Has been rounded tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PLIST:Positive infinity".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Positive infinity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PLIST:Negative infinity".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Negative infinity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "PLIST:Indefinite".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Indefinite tag".to_string(), vec!["Example".to_string()]),
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
