//! FITS format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0641), "FITS:ObservationDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ObservationDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC1E0), "FITS:ObservationTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ObservationTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE21C), "FITS:ObservationDateEnd".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ObservationDateEnd tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x9DBB), "FITS:ObservationTimeEnd".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ObservationTimeEnd tag".to_string(), vec!["Example".to_string()]),
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
