//! FotoStation format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0001), "FotoStation:IPTC".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTC tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "FotoStation:SoftEdit".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SoftEdit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "FotoStation:ThumbnailImage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ThumbnailImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "FotoStation:PreviewImage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreviewImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "FotoStation:OriginalImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "FotoStation:OriginalImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "FotoStation:ColorPlanes".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorPlanes tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "FotoStation:XYResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XYResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "FotoStation:Rotation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rotation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "FotoStation:CropLeft".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropLeft tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "FotoStation:CropTop".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropTop tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "FotoStation:CropRight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropRight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "FotoStation:CropBottom".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropBottom tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "FotoStation:CropRotation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropRotation tag".to_string(), vec!["Example".to_string()]),
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
