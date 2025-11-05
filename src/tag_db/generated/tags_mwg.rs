//! MWG format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0002), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MWG:EXIF:DateTimeOriginal".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EXIF:DateTimeOriginal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MWG:IPTC:DateCreated".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTC:DateCreated tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:IPTC:TimeCreated".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTC:TimeCreated tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:XMP-photoshop:DateCreated".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XMP-photoshop:DateCreated tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MWG:EXIF:CreateDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EXIF:CreateDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MWG:IPTC:DigitalCreationDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTC:DigitalCreationDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:IPTC:DigitalCreationTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTC:DigitalCreationTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:XMP-xmp:CreateDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XMP-xmp:CreateDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MWG:EXIF:ModifyDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EXIF:ModifyDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MWG:XMP-xmp:ModifyDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XMP-xmp:ModifyDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MWG:XMP-iptcExt:LocationShownCountryName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XMP-iptcExt:LocationShownCountryName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MWG:XMP-iptcExt:LocationShownProvinceState".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XMP-iptcExt:LocationShownProvinceState tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MWG:XMP-iptcExt:LocationShownCity".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XMP-iptcExt:LocationShownCity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MWG:XMP-iptcExt:LocationShownSublocation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XMP-iptcExt:LocationShownSublocation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MWG:CurrentIPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CurrentIPTCDigest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MWG:IPTCDigest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCDigest tag".to_string(), vec!["Example".to_string()]),
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
