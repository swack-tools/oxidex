//! Jpeg2000 format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x5B80), "Jpeg2000:Resolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Resolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x952A), "Jpeg2000:Label".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Label tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE891), "Jpeg2000:URL".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "URL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Jpeg2000:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Jpeg2000:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Jpeg2000:NumberOfComponents".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "NumberOfComponents tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "Jpeg2000:BitsPerComponent".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BitsPerComponent tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "Jpeg2000:Compression".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Compression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Jpeg2000:Modified Huffman".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Modified Huffman tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Jpeg2000:Modified READ".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Modified READ tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Jpeg2000:Modified Modified READ".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Modified Modified READ tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Jpeg2000:JBIG".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JBIG tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "Jpeg2000:JPEG".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JPEG tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Jpeg2000:JPEG-LS".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JPEG-LS tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Jpeg2000:JPEG 2000".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JPEG 2000 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Jpeg2000:JBIG2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JBIG2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Jpeg2000:MajorBrand".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MajorBrand tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Jpeg2000:MinorVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MinorVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Jpeg2000:CompatibleBrands".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CompatibleBrands tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Jpeg2000:CaptureYResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CaptureYResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Jpeg2000:CaptureXResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CaptureXResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Jpeg2000:CaptureYResolutionUnit".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CaptureYResolutionUnit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Jpeg2000:CaptureXResolutionUnit".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CaptureXResolutionUnit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Jpeg2000:DisplayYResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DisplayYResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Jpeg2000:DisplayXResolution".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DisplayXResolution tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Jpeg2000:DisplayYResolutionUnit".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DisplayYResolutionUnit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Jpeg2000:DisplayXResolutionUnit".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DisplayXResolutionUnit tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Jpeg2000:ColorSpecMethod".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorSpecMethod tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Jpeg2000:Restricted ICC".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Restricted ICC tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Jpeg2000:Any ICC".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Any ICC tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Jpeg2000:Vendor Color".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Vendor Color tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Jpeg2000:ColorSpecPrecedence".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorSpecPrecedence tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Jpeg2000:ColorSpecApproximation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorSpecApproximation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Jpeg2000:Accurate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Accurate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Jpeg2000:Exceptional Quality".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Exceptional Quality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Jpeg2000:Reasonable Quality".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Reasonable Quality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Jpeg2000:Poor Quality".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Poor Quality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Jpeg2000:YCbCr(1)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YCbCr(1) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Jpeg2000:YCbCr(2)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YCbCr(2) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Jpeg2000:YCbCr(3)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YCbCr(3) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Jpeg2000:PhotoYCC".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PhotoYCC tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "Jpeg2000:CMY".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CMY tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "Jpeg2000:CMYK".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CMYK tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "Jpeg2000:YCCK".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YCCK tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "Jpeg2000:CIELab".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CIELab tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "Jpeg2000:Grayscale".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Grayscale tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "Jpeg2000:sYCC".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "sYCC tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "Jpeg2000:CIEJab".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CIEJab tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "Jpeg2000:e-sRGB".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "e-sRGB tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "Jpeg2000:ROMM-RGB".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ROMM-RGB tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0016), "Jpeg2000:YPbPr(1125/60)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YPbPr(1125/60) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0017), "Jpeg2000:YPbPr(1250/50)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YPbPr(1250/50) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "Jpeg2000:e-sYCC".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "e-sYCC tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x8F3A), "Jpeg2000:JUMDType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JUMDType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7EF4), "Jpeg2000:JUMDLabel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JUMDLabel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAF9F), "Jpeg2000:JUMDToggles".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JUMDToggles tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Jpeg2000:Label".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Label tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Jpeg2000:ID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Jpeg2000:Signature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Signature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0D1B), "Jpeg2000:JUMDID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JUMD ID".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xBCD1), "Jpeg2000:JUMDSignature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "JUMDSignature tag".to_string(), vec!["Example".to_string()]),
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
