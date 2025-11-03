//! PhotoMechanic format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0002), "PhotoMechanic:SoftEdit".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SoftEdit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D1), "PhotoMechanic:RawCropLeft".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RawCropLeft tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D2), "PhotoMechanic:RawCropTop".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RawCropTop tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D3), "PhotoMechanic:RawCropRight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RawCropRight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D4), "PhotoMechanic:RawCropBottom".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RawCropBottom tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D5), "PhotoMechanic:ConstrainedCropWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ConstrainedCropWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D6), "PhotoMechanic:ConstrainedCropHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ConstrainedCropHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D7), "PhotoMechanic:FrameNum".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FrameNum tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D8), "PhotoMechanic:Rotation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rotation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PhotoMechanic:90".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "90 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PhotoMechanic:180".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "180 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PhotoMechanic:270".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "270 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D9), "PhotoMechanic:CropLeft".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropLeft tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00DA), "PhotoMechanic:CropTop".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropTop tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00DB), "PhotoMechanic:CropRight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropRight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00DC), "PhotoMechanic:CropBottom".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropBottom tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00DD), "PhotoMechanic:Tagged".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Tagged tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00DE), "PhotoMechanic:ColorClass".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorClass tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00DF), "PhotoMechanic:Rating".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rating tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00EC), "PhotoMechanic:PreviewCropLeft".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreviewCropLeft tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00ED), "PhotoMechanic:PreviewCropTop".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreviewCropTop tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00EE), "PhotoMechanic:PreviewCropRight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreviewCropRight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00EF), "PhotoMechanic:PreviewCropBottom".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreviewCropBottom tag".to_string(), vec!["Example".to_string()]),
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
