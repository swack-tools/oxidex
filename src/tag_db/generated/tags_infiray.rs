//! InfiRay format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "InfiRay:IJPEGVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IJPEGVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "InfiRay:IJPEGOrgType".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IJPEGOrgType tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "InfiRay:IJPEGDispType".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IJPEGDispType tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "InfiRay:IJPEGRotate".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IJPEGRotate tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "InfiRay:IJPEGMirrorFlip".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IJPEGMirrorFlip tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "InfiRay:ImageColorSwitchable".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ImageColorSwitchable tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "InfiRay:ThermalColorPalette".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ThermalColorPalette tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "InfiRay:IRDataSize".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IRDataSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0028), "InfiRay:IRDataFormat".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IRDataFormat tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002A), "InfiRay:IRImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IRImageWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002C), "InfiRay:IRImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IRImageHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002E), "InfiRay:IRImageBpp".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IRImageBpp tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0030), "InfiRay:TempDataSize".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "TempDataSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0038), "InfiRay:TempDataFormat".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "TempDataFormat tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003A), "InfiRay:TempImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "TempImageWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003C), "InfiRay:TempImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "TempImageHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003E), "InfiRay:TempImageBpp".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "TempImageBpp tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0040), "InfiRay:VisibleDataSize".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "VisibleDataSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0048), "InfiRay:VisibleDataFormat".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "VisibleDataFormat tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004A), "InfiRay:VisibleImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "VisibleImageWidth tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004C), "InfiRay:VisibleImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "VisibleImageHeight tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004E), "InfiRay:VisibleImageBpp".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "VisibleImageBpp tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "InfiRay:IJPEGTempVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IJPEGTempVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "InfiRay:FactDefEmissivity".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefEmissivity tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "InfiRay:FactDefTau".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefTau tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "InfiRay:FactDefTa".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefTa tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "InfiRay:FactDefTu".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefTu tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "InfiRay:FactDefDist".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefDist tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "InfiRay:FactDefA0".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefA0 tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "InfiRay:FactDefB0".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefB0 tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "InfiRay:FactDefA1".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefA1 tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "InfiRay:FactDefB1".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefB1 tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001C), "InfiRay:FactDefP0".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefP0 tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "InfiRay:FactDefP1".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefP1 tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0024), "InfiRay:FactDefP2".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactDefP2 tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0044), "InfiRay:FactRelSensorTemp".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactRelSensorTemp tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0046), "InfiRay:FactRelShutterTemp".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactRelShutterTemp tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0048), "InfiRay:FactRelLensTemp".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactRelLensTemp tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0064), "InfiRay:FactStatusGain".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactStatusGain tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0065), "InfiRay:FactStatusEnvOK".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactStatusEnvOK tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0066), "InfiRay:FactStatusDistOK".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactStatusDistOK tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0067), "InfiRay:FactStatusTempMap".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "FactStatusTempMap tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "InfiRay:EnvironmentTemp".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "EnvironmentTemp tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "InfiRay:Distance".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "Distance tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "InfiRay:Emissivity".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "Emissivity tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "InfiRay:Humidity".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "Humidity tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "InfiRay:ReferenceTemp".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "ReferenceTemp tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "InfiRay:TempUnit".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "TempUnit tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0021), "InfiRay:ShowCenterTemp".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ShowCenterTemp tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0022), "InfiRay:ShowMaxTemp".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ShowMaxTemp tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0023), "InfiRay:ShowMinTemp".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ShowMinTemp tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0024), "InfiRay:TempMeasureCount".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "TempMeasureCount tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "InfiRay:MixMode".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "MixMode tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "InfiRay:FusionIntensity".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "FusionIntensity tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "InfiRay:OffsetAdjustment".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "OffsetAdjustment tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "InfiRay:CorrectionAsix".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CorrectionAsix tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "InfiRay:WorkingMode".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "WorkingMode tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "InfiRay:IntegralTime".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IntegralTime tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "InfiRay:IntegratTimeHdr".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "IntegratTimeHdr tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "InfiRay:GainStable".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "GainStable tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "InfiRay:TempControlEnable".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "TempControlEnable tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "InfiRay:DeviceTemp".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "DeviceTemp tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "InfiRay:IsothermalMax".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "IsothermalMax tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "InfiRay:IsothermalMin".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "IsothermalMin tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "InfiRay:ChromaBarMax".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "ChromaBarMax tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "InfiRay:ChromaBarMin".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "ChromaBarMin tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "InfiRay:IRSensorManufacturer".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IRSensorManufacturer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0040), "InfiRay:IRSensorName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IRSensorName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0080), "InfiRay:IRSensorPartNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IRSensorPartNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00C0), "InfiRay:IRSensorSerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IRSensorSerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0100), "InfiRay:IRSensorFirmware".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IRSensorFirmware tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0140), "InfiRay:IRSensorAperture".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "IRSensorAperture tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0144), "InfiRay:IRFocalLength".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "IRFocalLength tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0180), "InfiRay:VisibleSensorManufacturer".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VisibleSensorManufacturer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x01C0), "InfiRay:VisibleSensorName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VisibleSensorName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0200), "InfiRay:VisibleSensorPartNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VisibleSensorPartNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0240), "InfiRay:VisibleSensorSerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VisibleSensorSerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0280), "InfiRay:VisibleSensorFirmware".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VisibleSensorFirmware tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x02C0), "InfiRay:VisibleSensorAperture".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "VisibleSensorAperture tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x02C4), "InfiRay:VisibleFocalLength".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "VisibleFocalLength tag".to_string(), vec!["1.5".to_string()]),
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
