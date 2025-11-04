//! HTML format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xFEB1), "HTML:HTTP-equiv".to_string(), FormatFamily::PDF, false, ValueType::String, "HTTP-equiv tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xDA2C), "HTML:ContentLanguage".to_string(), FormatFamily::PDF, false, ValueType::String, "ContentLanguage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x78A3), "HTML:DocClass".to_string(), FormatFamily::PDF, false, ValueType::String, "DocClass tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x980C), "HTML:DocRights".to_string(), FormatFamily::PDF, false, ValueType::String, "DocRights tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAEAF), "HTML:DocType".to_string(), FormatFamily::PDF, false, ValueType::String, "DocType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4599), "HTML:ResourceType".to_string(), FormatFamily::PDF, false, ValueType::String, "ResourceType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x26C7), "HTML:RevisitAfter".to_string(), FormatFamily::PDF, false, ValueType::String, "RevisitAfter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5612), "HTML:CacheControl".to_string(), FormatFamily::PDF, false, ValueType::String, "CacheControl tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0863), "HTML:ContentDisposition".to_string(), FormatFamily::PDF, false, ValueType::String, "ContentDisposition tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xDA2C), "HTML:ContentLanguage".to_string(), FormatFamily::PDF, false, ValueType::String, "ContentLanguage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xFFA8), "HTML:ContentScriptType".to_string(), FormatFamily::PDF, false, ValueType::String, "ContentScriptType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5BAA), "HTML:ContentStyleType".to_string(), FormatFamily::PDF, false, ValueType::String, "ContentStyleType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x600E), "HTML:ContentType".to_string(), FormatFamily::PDF, false, ValueType::String, "ContentType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x9325), "HTML:DefaultStyle".to_string(), FormatFamily::PDF, false, ValueType::String, "DefaultStyle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x70D6), "HTML:ExtCache".to_string(), FormatFamily::PDF, false, ValueType::String, "ExtCache tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x161A), "HTML:PageEnter".to_string(), FormatFamily::PDF, false, ValueType::String, "PageEnter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB99C), "HTML:PageExit".to_string(), FormatFamily::PDF, false, ValueType::String, "PageExit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x9050), "HTML:PicsLabel".to_string(), FormatFamily::PDF, false, ValueType::String, "PicsLabel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC43E), "HTML:ReplyTo".to_string(), FormatFamily::PDF, false, ValueType::String, "ReplyTo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x662F), "HTML:SetCookie".to_string(), FormatFamily::PDF, false, ValueType::String, "SetCookie tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x28D2), "HTML:SiteEnter".to_string(), FormatFamily::PDF, false, ValueType::String, "SiteEnter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6FE4), "HTML:SiteExit".to_string(), FormatFamily::PDF, false, ValueType::String, "SiteExit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB20E), "HTML:WindowTarget".to_string(), FormatFamily::PDF, false, ValueType::String, "WindowTarget tag".to_string(), vec!["Example".to_string()]),
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
