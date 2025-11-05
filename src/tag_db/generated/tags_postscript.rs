//! PostScript format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0002), "PostScript:CMYK".to_string(), FormatFamily::PostScript, false, ValueType::String, "CMYK tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PostScript:Millimeters".to_string(), FormatFamily::PostScript, false, ValueType::String, "Millimeters tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PostScript:Points".to_string(), FormatFamily::PostScript, false, ValueType::String, "Points tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PostScript:Picas".to_string(), FormatFamily::PostScript, false, ValueType::String, "Picas tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "PostScript:Centimeters".to_string(), FormatFamily::PostScript, false, ValueType::String, "Centimeters tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "PostScript:Pixels".to_string(), FormatFamily::PostScript, false, ValueType::String, "Pixels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PostScript:PostScript:BoundingBox".to_string(), FormatFamily::PostScript, false, ValueType::String, "PostScript:BoundingBox tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PostScript:PostScript:BoundingBox".to_string(), FormatFamily::PostScript, false, ValueType::String, "PostScript:BoundingBox tag".to_string(), vec!["Example".to_string()]),
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
