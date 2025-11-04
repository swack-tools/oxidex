//! MIE format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0001), "MIE:Differential Corrected".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Differential Corrected tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6B8A), "MIE:FullSizeImageType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FullSizeImageType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6E5C), "MIE:FullSizeImageName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FullSizeImageName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6B8A), "MIE:PreviewImageType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreviewImageType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6E5C), "MIE:PreviewImageName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreviewImageName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6B8A), "MIE:ThumbnailImageType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ThumbnailImageType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6E5C), "MIE:ThumbnailImageName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ThumbnailImageName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6B8A), "MIE:RelatedAudioFileType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RelatedAudioFileType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6E5C), "MIE:RelatedAudioFileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RelatedAudioFileName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6B8A), "MIE:RelatedVideoFileType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RelatedVideoFileType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6E5C), "MIE:RelatedVideoFileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RelatedVideoFileName tag".to_string(), vec!["Example".to_string()]),
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
