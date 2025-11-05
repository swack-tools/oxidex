//! JPEG format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0006), "JPEG:HDRGainCurveSize".to_string(), FormatFamily::JPEG, false, ValueType::String, "HDRGainCurveSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "JPEG:HDRGainCurve".to_string(), FormatFamily::JPEG, false, ValueType::String, "HDRGainCurve tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "JPEG:JPSSeparation".to_string(), FormatFamily::JPEG, false, ValueType::String, "JPSSeparation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "JPEG:HdrLength".to_string(), FormatFamily::JPEG, false, ValueType::String, "HdrLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "JPEG:JPSFlags".to_string(), FormatFamily::JPEG, false, ValueType::String, "JPSFlags tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "JPEG:Half width".to_string(), FormatFamily::JPEG, false, ValueType::String, "Half width tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:Left field first".to_string(), FormatFamily::JPEG, false, ValueType::String, "Left field first tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "JPEG:Left Eye".to_string(), FormatFamily::JPEG, false, ValueType::String, "Left Eye tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:Right Eye".to_string(), FormatFamily::JPEG, false, ValueType::String, "Right Eye tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:Side By Side".to_string(), FormatFamily::JPEG, false, ValueType::String, "Side By Side tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "JPEG:Over Under".to_string(), FormatFamily::JPEG, false, ValueType::String, "Over Under tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "JPEG:Anaglyph".to_string(), FormatFamily::JPEG, false, ValueType::String, "Anaglyph tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "JPEG:JPSType".to_string(), FormatFamily::JPEG, false, ValueType::String, "JPSType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "JPEG:JPSComment".to_string(), FormatFamily::JPEG, false, ValueType::String, "JPSComment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC4A5), "JPEG:PrintIM".to_string(), FormatFamily::JPEG, false, ValueType::String, "PrintIM tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "JPEG:SPIFFVersion".to_string(), FormatFamily::JPEG, false, ValueType::String, "SPIFFVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:ProfileID".to_string(), FormatFamily::JPEG, false, ValueType::String, "ProfileID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "JPEG:Continuous-tone Base".to_string(), FormatFamily::JPEG, false, ValueType::String, "Continuous-tone Base tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:Continuous-tone Progressive".to_string(), FormatFamily::JPEG, false, ValueType::String, "Continuous-tone Progressive tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "JPEG:Bi-level Facsimile".to_string(), FormatFamily::JPEG, false, ValueType::String, "Bi-level Facsimile tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "JPEG:Continuous-tone Facsimile".to_string(), FormatFamily::JPEG, false, ValueType::String, "Continuous-tone Facsimile tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "JPEG:ColorComponents".to_string(), FormatFamily::JPEG, false, ValueType::String, "ColorComponents tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "JPEG:ImageHeight".to_string(), FormatFamily::JPEG, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "JPEG:ImageWidth".to_string(), FormatFamily::JPEG, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "JPEG:ColorSpace".to_string(), FormatFamily::JPEG, false, ValueType::String, "ColorSpace tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "JPEG:YCbCr, ITU-R BT 709, video".to_string(), FormatFamily::JPEG, false, ValueType::String, "YCbCr, ITU-R BT 709, video tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:No color space specified".to_string(), FormatFamily::JPEG, false, ValueType::String, "No color space specified tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "JPEG:YCbCr, ITU-R BT 601-1, RGB".to_string(), FormatFamily::JPEG, false, ValueType::String, "YCbCr, ITU-R BT 601-1, RGB tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "JPEG:YCbCr, ITU-R BT 601-1, video".to_string(), FormatFamily::JPEG, false, ValueType::String, "YCbCr, ITU-R BT 601-1, video tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "JPEG:Gray-scale".to_string(), FormatFamily::JPEG, false, ValueType::String, "Gray-scale tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "JPEG:PhotoYCC".to_string(), FormatFamily::JPEG, false, ValueType::String, "PhotoYCC tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "JPEG:RGB".to_string(), FormatFamily::JPEG, false, ValueType::String, "RGB tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "JPEG:CMY".to_string(), FormatFamily::JPEG, false, ValueType::String, "CMY tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "JPEG:CMYK".to_string(), FormatFamily::JPEG, false, ValueType::String, "CMYK tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "JPEG:YCCK".to_string(), FormatFamily::JPEG, false, ValueType::String, "YCCK tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "JPEG:CIELab".to_string(), FormatFamily::JPEG, false, ValueType::String, "CIELab tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "JPEG:BitsPerSample".to_string(), FormatFamily::JPEG, false, ValueType::String, "BitsPerSample tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "JPEG:Compression".to_string(), FormatFamily::JPEG, false, ValueType::String, "Compression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "JPEG:Modified Huffman".to_string(), FormatFamily::JPEG, false, ValueType::String, "Modified Huffman tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:Modified READ".to_string(), FormatFamily::JPEG, false, ValueType::String, "Modified READ tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "JPEG:Modified Modified READ".to_string(), FormatFamily::JPEG, false, ValueType::String, "Modified Modified READ tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "JPEG:JBIG".to_string(), FormatFamily::JPEG, false, ValueType::String, "JBIG tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "JPEG:JPEG".to_string(), FormatFamily::JPEG, false, ValueType::String, "JPEG tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "JPEG:ResolutionUnit".to_string(), FormatFamily::JPEG, false, ValueType::String, "ResolutionUnit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "JPEG:inches".to_string(), FormatFamily::JPEG, false, ValueType::String, "inches tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:cm".to_string(), FormatFamily::JPEG, false, ValueType::String, "cm tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "JPEG:YResolution".to_string(), FormatFamily::JPEG, false, ValueType::String, "YResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0016), "JPEG:XResolution".to_string(), FormatFamily::JPEG, false, ValueType::String, "XResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "JPEG:AdobeCMType".to_string(), FormatFamily::JPEG, false, ValueType::String, "AdobeCMType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "JPEG:DCTEncodeVersion".to_string(), FormatFamily::JPEG, false, ValueType::String, "DCTEncodeVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "JPEG:APP14Flags0".to_string(), FormatFamily::JPEG, false, ValueType::String, "APP14Flags0 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:APP14Flags1".to_string(), FormatFamily::JPEG, false, ValueType::String, "APP14Flags1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "JPEG:ColorTransform".to_string(), FormatFamily::JPEG, false, ValueType::String, "ColorTransform tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "JPEG:YCbCr".to_string(), FormatFamily::JPEG, false, ValueType::String, "YCbCr tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:YCCK".to_string(), FormatFamily::JPEG, false, ValueType::String, "YCCK tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "JPEG:InterleavedField".to_string(), FormatFamily::JPEG, false, ValueType::String, "InterleavedField tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "JPEG:Odd".to_string(), FormatFamily::JPEG, false, ValueType::String, "Odd tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:Even".to_string(), FormatFamily::JPEG, false, ValueType::String, "Even tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "JPEG:NITFVersion".to_string(), FormatFamily::JPEG, false, ValueType::String, "NITFVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "JPEG:ImageFormat".to_string(), FormatFamily::JPEG, false, ValueType::String, "ImageFormat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "JPEG:BlocksPerRow".to_string(), FormatFamily::JPEG, false, ValueType::String, "BlocksPerRow tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "JPEG:BlocksPerColumn".to_string(), FormatFamily::JPEG, false, ValueType::String, "BlocksPerColumn tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "JPEG:ImageColor".to_string(), FormatFamily::JPEG, false, ValueType::String, "ImageColor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "JPEG:BitDepth".to_string(), FormatFamily::JPEG, false, ValueType::String, "BitDepth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "JPEG:ImageClass".to_string(), FormatFamily::JPEG, false, ValueType::String, "ImageClass tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "JPEG:Tactical Imagery".to_string(), FormatFamily::JPEG, false, ValueType::String, "Tactical Imagery tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "JPEG:JPEGProcess".to_string(), FormatFamily::JPEG, false, ValueType::String, "JPEGProcess tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "JPEG:Extended sequential DCT, Huffman coding, 12-bit samples".to_string(), FormatFamily::JPEG, false, ValueType::String, "Extended sequential DCT, Huffman coding, 12-bit samples tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "JPEG:Quality".to_string(), FormatFamily::JPEG, false, ValueType::String, "Quality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "JPEG:StreamColor".to_string(), FormatFamily::JPEG, false, ValueType::String, "StreamColor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "JPEG:StreamBitDepth".to_string(), FormatFamily::JPEG, false, ValueType::String, "StreamBitDepth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "JPEG:Flags".to_string(), FormatFamily::JPEG, false, ValueType::String, "Flags tag".to_string(), vec!["Example".to_string()]),
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
