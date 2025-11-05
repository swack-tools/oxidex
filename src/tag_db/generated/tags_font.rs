//! Font format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "Font:Copyright".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Copyright tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Font:FontFamily".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FontFamily tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Font:FontSubfamily".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FontSubfamily tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Font:FontSubfamilyID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FontSubfamilyID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Font:PostScriptFontName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PostScript Font Name".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Font:Trademark".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Trademark tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Font:Manufacturer".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Manufacturer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Font:Designer".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Designer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "Font:Description".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Description tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "Font:VendorURL".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VendorURL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "Font:DesignerURL".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DesignerURL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "Font:License".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "License tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "Font:LicenseInfoURL".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LicenseInfoURL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "Font:PreferredFamily".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreferredFamily tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "Font:PreferredSubfamily".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreferredSubfamily tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "Font:CompatibleFontName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CompatibleFontName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "Font:SampleText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SampleText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "Font:PostScriptFontName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PostScriptFontName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "Font:WWSFamilyName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WWSFamilyName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0016), "Font:WWSSubfamilyName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WWSSubfamilyName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Font:PFMVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PFMVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Font:Copyright".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Copyright tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0042), "Font:FontType".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FontType tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0044), "Font:PointSize".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "PointSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0046), "Font:YResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "YResolution tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0048), "Font:XResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "XResolution tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004A), "Font:Ascent".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "Ascent tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004C), "Font:InternalLeading".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "InternalLeading tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004E), "Font:ExternalLeading".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ExternalLeading tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0050), "Font:Italic".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Italic tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0051), "Font:Underline".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Underline tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0052), "Font:Strikeout".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Strikeout tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0053), "Font:Weight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "Weight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0055), "Font:CharacterSet".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CharacterSet tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0056), "Font:PixWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "PixWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0058), "Font:PixHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "PixHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x005A), "Font:PitchAndFamily".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PitchAndFamily tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x005B), "Font:AvgWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "AvgWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x005D), "Font:MaxWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "MaxWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x005F), "Font:FirstChar".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FirstChar tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0060), "Font:LastChar".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LastChar tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0061), "Font:DefaultChar".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DefaultChar tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0062), "Font:BreakChar".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BreakChar tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0063), "Font:WidthBytes".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "WidthBytes tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x884F), "Font:CreateDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CreateDate tag".to_string(), vec!["Example".to_string()]),
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
