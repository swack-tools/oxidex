//! BMP format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "BMP:BMPVersion".to_string(), FormatFamily::PNG, false, ValueType::String, "BMPVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x007C), "BMP:Windows V5".to_string(), FormatFamily::PNG, false, ValueType::String, "Windows V5 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "BMP:ImageWidth".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "BMP:ImageHeight".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "BMP:Planes".to_string(), FormatFamily::PNG, false, ValueType::String, "Planes tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "BMP:BitDepth".to_string(), FormatFamily::PNG, false, ValueType::String, "BitDepth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "BMP:Compression".to_string(), FormatFamily::PNG, false, ValueType::String, "Compression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "BMP:8-Bit RLE".to_string(), FormatFamily::PNG, false, ValueType::String, "8-Bit RLE tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "BMP:4-Bit RLE".to_string(), FormatFamily::PNG, false, ValueType::String, "4-Bit RLE tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "BMP:Bitfields".to_string(), FormatFamily::PNG, false, ValueType::String, "Bitfields tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "BMP:ImageLength".to_string(), FormatFamily::PNG, false, ValueType::String, "ImageLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "BMP:PixelsPerMeterX".to_string(), FormatFamily::PNG, false, ValueType::String, "PixelsPerMeterX tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001C), "BMP:PixelsPerMeterY".to_string(), FormatFamily::PNG, false, ValueType::String, "PixelsPerMeterY tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "BMP:NumColors".to_string(), FormatFamily::PNG, false, ValueType::String, "NumColors tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0024), "BMP:NumImportantColors".to_string(), FormatFamily::PNG, false, ValueType::String, "NumImportantColors tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0028), "BMP:RedMask".to_string(), FormatFamily::PNG, false, ValueType::String, "RedMask tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002C), "BMP:GreenMask".to_string(), FormatFamily::PNG, false, ValueType::String, "GreenMask tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0030), "BMP:BlueMask".to_string(), FormatFamily::PNG, false, ValueType::String, "BlueMask tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0034), "BMP:AlphaMask".to_string(), FormatFamily::PNG, false, ValueType::String, "AlphaMask tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0038), "BMP:ColorSpace".to_string(), FormatFamily::PNG, false, ValueType::String, "ColorSpace tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "BMP:Device RGB".to_string(), FormatFamily::PNG, false, ValueType::String, "Device RGB tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "BMP:Device CMYK".to_string(), FormatFamily::PNG, false, ValueType::String, "Device CMYK tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003C), "BMP:RedEndpoint".to_string(), FormatFamily::PNG, false, ValueType::String, "RedEndpoint tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0048), "BMP:GreenEndpoint".to_string(), FormatFamily::PNG, false, ValueType::String, "GreenEndpoint tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0054), "BMP:BlueEndpoint".to_string(), FormatFamily::PNG, false, ValueType::String, "BlueEndpoint tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0060), "BMP:GammaRed".to_string(), FormatFamily::PNG, false, ValueType::String, "GammaRed tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0064), "BMP:GammaGreen".to_string(), FormatFamily::PNG, false, ValueType::String, "GammaGreen tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0068), "BMP:GammaBlue".to_string(), FormatFamily::PNG, false, ValueType::String, "GammaBlue tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x006C), "BMP:RenderingIntent".to_string(), FormatFamily::PNG, false, ValueType::String, "RenderingIntent tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "BMP:Proof (LCS_GM_GRAPHICS)".to_string(), FormatFamily::PNG, false, ValueType::String, "Proof (LCS_GM_GRAPHICS) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "BMP:Picture (LCS_GM_IMAGES)".to_string(), FormatFamily::PNG, false, ValueType::String, "Picture (LCS_GM_IMAGES) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "BMP:Absolute Colorimetric (LCS_GM_ABS_COLORIMETRIC)".to_string(), FormatFamily::PNG, false, ValueType::String, "Absolute Colorimetric (LCS_GM_ABS_COLORIMETRIC) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0070), "BMP:ProfileDataOffset".to_string(), FormatFamily::PNG, false, ValueType::String, "ProfileDataOffset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0074), "BMP:ProfileSize".to_string(), FormatFamily::PNG, false, ValueType::String, "ProfileSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "BMP:BMPVersion".to_string(), FormatFamily::PNG, false, ValueType::String, "BMPVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0040), "BMP:OS/2 V2".to_string(), FormatFamily::PNG, false, ValueType::String, "OS/2 V2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "BMP:ImageWidth".to_string(), FormatFamily::PNG, false, ValueType::Integer, "ImageWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "BMP:ImageHeight".to_string(), FormatFamily::PNG, false, ValueType::Integer, "ImageHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "BMP:Planes".to_string(), FormatFamily::PNG, false, ValueType::Integer, "Planes tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "BMP:BitDepth".to_string(), FormatFamily::PNG, false, ValueType::Integer, "BitDepth tag".to_string(), vec!["100".to_string()]),
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
