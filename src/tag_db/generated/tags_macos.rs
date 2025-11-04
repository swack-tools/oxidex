//! MacOS format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0002), "MacOS:RSRC".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RSRC tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "MacOS:ATTR".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ATTR tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MacOS:1 (Gray)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "1 (Gray) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MacOS:2 (Green)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "2 (Green) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MacOS:3 (Purple)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "3 (Purple) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MacOS:4 (Blue)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "4 (Blue) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MacOS:5 (Yellow)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "5 (Yellow) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MacOS:6 (Red)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "6 (Red) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "MacOS:7 (Orange)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "7 (Orange) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xFDF5), "MacOS:XAttrFinderInfo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrFinderInfo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MacOS:Shared".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Shared tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "MacOS:HasNoInits".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "HasNoInits tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "MacOS:Inited".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Inited tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "MacOS:CustomIcon".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CustomIcon tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "MacOS:Stationery".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Stationery tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "MacOS:NameLocked".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "NameLocked tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "MacOS:HasBundle".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "HasBundle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "MacOS:Invisible".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Invisible tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "MacOS:Alias".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Alias tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0016), "MacOS:HasRoutingInfo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "HasRoutingInfo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0017), "MacOS:ObjectBusy".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ObjectBusy tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "MacOS:CustomBadge".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CustomBadge tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001F), "MacOS:ExtendedFlagsValid".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExtendedFlagsValid tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE20F), "MacOS:XAttrQuarantine".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrQuarantine tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x99AA), "MacOS:XAttrAppleMailDateReceived".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrAppleMailDateReceived tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xCBE1), "MacOS:XAttrAppleMailDateSent".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrAppleMailDateSent tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0D0E), "MacOS:XAttrAppleMailIsRemoteAttachment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrAppleMailIsRemoteAttachment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3FA0), "MacOS:XAttrMDItemDownloadedDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrMDItemDownloadedDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x11CE), "MacOS:XAttrMDItemFinderComment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrMDItemFinderComment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xEB0D), "MacOS:XAttrMDItemWhereFroms".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrMDItemWhereFroms tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x39BC), "MacOS:XAttrMDLabel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrMDLabel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC1B1), "MacOS:XAttrResourceFork".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrResourceFork tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3104), "MacOS:XAttrLastUsedDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XAttrLastUsedDate tag".to_string(), vec!["Example".to_string()]),
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
