//! GIMP format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0011), "GIMP:Compression".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Compression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GIMP:RLE Encoding".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RLE Encoding tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GIMP:Zlib".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Zlib tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GIMP:Fractal".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Fractal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "GIMP:Resolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Resolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "GIMP:Tattoo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Tattoo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "GIMP:Parasites".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Parasites tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0016), "GIMP:Units".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Units tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GIMP:mm".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "mm tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GIMP:Points".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Points tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "GIMP:Picas".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Picas tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "GIMP:XCFVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XCFVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "GIMP:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "GIMP:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0016), "GIMP:ColorMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GIMP:Grayscale".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Grayscale tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GIMP:Indexed Color".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Indexed Color tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "GIMP:XResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GIMP:YResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6697), "GIMP:Comment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Comment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x8247), "GIMP:ExifData".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExifData tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE322), "GIMP:JPEGExifData".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JPEGExifData tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD941), "GIMP:IPTCData".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IPTCData tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x6A65), "GIMP:ICC_Profile".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ICC_Profile tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE8F3), "GIMP:ICCProfileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ICCProfileName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x09F7), "GIMP:XMP".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XMP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0429), "GIMP:XML".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "XML tag".to_string(), vec!["Example".to_string()]),
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
