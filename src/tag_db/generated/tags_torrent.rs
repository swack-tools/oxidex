//! Torrent format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x9782), "Torrent:AnnounceList1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AnnounceList1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xBC8F), "Torrent:Creator".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Creator tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE44F), "Torrent:CreateDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CreateDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1A3C), "Torrent:URLList1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "URLList1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x46C5), "Torrent:File1Duration".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "File1Duration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6C53), "Torrent:File1Media".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "File1Media tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x400D), "Torrent:MD5Sum".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MD5Sum tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x106F), "Torrent:NameUTF-8".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "NameUTF-8 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x9058), "Torrent:PieceLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PieceLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD345), "Torrent:Pieces".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Pieces tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2DC6), "Torrent:Profile1Width".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Profile1Width tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x9127), "Torrent:Profile1Height".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Profile1Height tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2455), "Torrent:Profile1AudioCodec".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Profile1AudioCodec tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE960), "Torrent:Profile1VideoCodec".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Profile1VideoCodec tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3AE6), "Torrent:File1Length".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "File1Length tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x400D), "Torrent:File1MD5Sum".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "File1MD5Sum tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6425), "Torrent:File1Path".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "File1Path tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xDE89), "Torrent:File1PathUTF-8".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "File1PathUTF-8 tag".to_string(), vec!["Example".to_string()]),
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
