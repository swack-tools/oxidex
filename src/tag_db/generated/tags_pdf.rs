//! PDF format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x3F68), "PDF:AppleKeywords".to_string(), FormatFamily::PDF, false, ValueType::String, "AppleKeywords tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PDF:Modify".to_string(), FormatFamily::PDF, false, ValueType::String, "Modify tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "PDF:Copy".to_string(), FormatFamily::PDF, false, ValueType::String, "Copy tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "PDF:Annotate".to_string(), FormatFamily::PDF, false, ValueType::String, "Annotate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "PDF:Fill forms".to_string(), FormatFamily::PDF, false, ValueType::String, "Fill forms tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "PDF:Extract".to_string(), FormatFamily::PDF, false, ValueType::String, "Extract tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "PDF:Assemble".to_string(), FormatFamily::PDF, false, ValueType::String, "Assemble tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "PDF:Print high-res".to_string(), FormatFamily::PDF, false, ValueType::String, "Print high-res tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PDF:Fill forms, Create page templates, Sign".to_string(), FormatFamily::PDF, false, ValueType::String, "Fill forms, Create page templates, Sign tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PDF:Fill forms, Create page templates, Sign, Create/Delete/Edit annotations".to_string(), FormatFamily::PDF, false, ValueType::String, "Fill forms, Create page templates, Sign, Create/Delete/Edit annotations tag".to_string(), vec!["Example".to_string()]),
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
