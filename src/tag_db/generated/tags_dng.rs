//! DNG format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "DNG:OriginalRawImage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalRawImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "DNG:OriginalRawResource".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalRawResource tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "DNG:OriginalRawFileType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalRawFileType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "DNG:OriginalRawCreator".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalRawCreator tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "DNG:OriginalTHMImage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalTHMImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "DNG:OriginalTHMResource".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalTHMResource tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "DNG:OriginalTHMFileType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalTHMFileType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "DNG:OriginalTHMCreator".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalTHMCreator tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3F0E), "DNG:AdobeMRW".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AdobeMRW tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF4CD), "DNG:AdobeSR2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AdobeSR2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4309), "DNG:AdobeRAF".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AdobeRAF tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD792), "DNG:AdobePano".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AdobePano tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC501), "DNG:AdobeKoda".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AdobeKoda tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x137E), "DNG:AdobeLeaf".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AdobeLeaf tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "DNG:SeqID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SeqID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "DNG:SeqType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SeqType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "DNG:SeqFrameInfo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SeqFrameInfo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "DNG:SeqIndex".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "SeqIndex tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "DNG:SeqCount".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "SeqCount tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "DNG:SeqFinal".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "SeqFinal tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "DNG:PDRVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "PDRVersion tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "DNG:DynamicRange".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "DynamicRange tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "DNG:HintMaxOutputValue".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "HintMaxOutputValue tag".to_string(), vec!["1.5".to_string()]),
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
