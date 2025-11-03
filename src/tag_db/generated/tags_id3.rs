//! ID3 format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0021), "ID3:Artist".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Artist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003F), "ID3:Album".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Album tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x005D), "ID3:Year".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Year tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0061), "ID3:Comment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Comment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x007D), "ID3:Track".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Track tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x007F), "ID3:Genre".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Genre tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0040), "ID3:Artist2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Artist2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x007C), "ID3:Album2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Album2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00B8), "ID3:Speed".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Speed tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ID3:Medium".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Medium tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ID3:Fast".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Fast tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ID3:Hardcore".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Hardcore tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00B9), "ID3:Genre".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Genre tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D7), "ID3:StartTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "StartTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00DD), "ID3:EndTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EndTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x888E), "ID3:PictureFormat".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PictureFormat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x888F), "ID3:PictureType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PictureType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x8890), "ID3:PictureDescription".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PictureDescription tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xA27C), "ID3:JUMBF".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JUMBF tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ID3:Lyrics".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Lyrics tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ID3:Text Transcription".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Text Transcription tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ID3:Movement/part Name".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Movement/part Name tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ID3:Events".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Events tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "ID3:Chord".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Chord tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "ID3:Trivia/\"pop-up\" Information".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Trivia/\"pop-up\" Information tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "ID3:Web Page URL".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Web Page URL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "ID3:Image URL".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Image URL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ID3:ID3:Year".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ID3:Year tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ID3:ID3:Date".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ID3:Date tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ID3:ID3:Time".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ID3:Time tag".to_string(), vec!["Example".to_string()]),
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
