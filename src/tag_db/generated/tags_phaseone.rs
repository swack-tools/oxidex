//! PhaseOne format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0100), "PhaseOne:CameraOrientation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CameraOrientation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "PhaseOne:Rotate 90 CW".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rotate 90 CW tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "PhaseOne:Rotate 270 CW".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rotate 270 CW tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "PhaseOne:Rotate 180".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Rotate 180 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0102), "PhaseOne:SerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0105), "PhaseOne:ISO".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ISO tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0106), "PhaseOne:ColorMatrix1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorMatrix1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0107), "PhaseOne:WB_RGBLevels".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "WB_RGBLevels tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0108), "PhaseOne:SensorWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0109), "PhaseOne:SensorHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x010D), "PhaseOne:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x010E), "PhaseOne:RawFormat".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RawFormat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x010F), "PhaseOne:RawData".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RawData tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0110), "PhaseOne:SensorCalibration".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0112), "PhaseOne:DateTimeOriginal".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateTimeOriginal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0204), "PhaseOne:System".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "System tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0210), "PhaseOne:SensorTemperature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorTemperature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0211), "PhaseOne:SensorTemperature2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorTemperature2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0212), "PhaseOne:UnknownDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "UnknownDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x021C), "PhaseOne:StripOffsets".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "StripOffsets tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0226), "PhaseOne:ColorMatrix2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorMatrix2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0267), "PhaseOne:AFAdjustment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AFAdjustment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x022B), "PhaseOne:PhaseOne_0x022b".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PhaseOne_0x022b tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0258), "PhaseOne:PhaseOne_0x0258".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PhaseOne_0x0258 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x025A), "PhaseOne:PhaseOne_0x025a".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PhaseOne_0x025a tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0301), "PhaseOne:FirmwareVersions".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FirmwareVersions tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0400), "PhaseOne:ShutterSpeedValue".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ShutterSpeedValue tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0401), "PhaseOne:ApertureValue".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ApertureValue tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0402), "PhaseOne:ExposureCompensation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureCompensation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0403), "PhaseOne:FocalLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FocalLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0410), "PhaseOne:CameraModel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CameraModel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0412), "PhaseOne:LensModel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LensModel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0414), "PhaseOne:MaxApertureValue".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MaxApertureValue tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0415), "PhaseOne:MinApertureValue".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MinApertureValue tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0455), "PhaseOne:Viewfinder".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Viewfinder tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0401), "PhaseOne:AllColorFlatField1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AllColorFlatField1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0404), "PhaseOne:SensorCalibration_0x0404".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x0404 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0405), "PhaseOne:SensorCalibration_0x0405".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x0405 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0406), "PhaseOne:SensorCalibration_0x0406".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x0406 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0407), "PhaseOne:SerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0408), "PhaseOne:SensorCalibration_0x0408".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x0408 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x040B), "PhaseOne:RedBlueFlatField".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RedBlueFlatField tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x040F), "PhaseOne:SensorCalibration_0x040f".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x040f tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0410), "PhaseOne:AllColorFlatField2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AllColorFlatField2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0413), "PhaseOne:SensorCalibration_0x0413".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x0413 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0414), "PhaseOne:SensorCalibration_0x0414".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x0414 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0416), "PhaseOne:AllColorFlatField3".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AllColorFlatField3 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0418), "PhaseOne:SensorCalibration_0x0418".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x0418 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0419), "PhaseOne:LinearizationCoefficients1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LinearizationCoefficients1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x041A), "PhaseOne:LinearizationCoefficients2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LinearizationCoefficients2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x041C), "PhaseOne:SensorCalibration_0x041c".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x041c tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x041E), "PhaseOne:SensorCalibration_0x041e".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorCalibration_0x041e tag".to_string(), vec!["Example".to_string()]),
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
