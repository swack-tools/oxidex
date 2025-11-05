//! Sigma format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0002), "Sigma:SerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Sigma:DriveMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DriveMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Sigma:ResolutionMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ResolutionMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "Sigma:AFMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AFMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Sigma:FocusSetting".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FocusSetting tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Sigma:WhiteBalance".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WhiteBalance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Sigma:ExposureMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Sigma:MeteringMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MeteringMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Sigma:Multi-segment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Multi-segment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "Sigma:LensFocalRange".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LensFocalRange tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "Sigma:ColorSpace".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorSpace tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "Sigma:AdjustmentMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AdjustmentMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0016), "Sigma:Quality".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Quality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0017), "Sigma:Firmware".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Firmware tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "Sigma:Software".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Software tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0019), "Sigma:AutoBracket".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AutoBracket tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001E), "Sigma:PreviewImageSize".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PreviewImageSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0022), "Sigma:FileFormat".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FileFormat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0024), "Sigma:Calibration".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Calibration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0026), "Sigma:FileFormat".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FileFormat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002A), "Sigma:LensFocalRange".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LensFocalRange tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002B), "Sigma:LensMaxApertureRange".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LensMaxApertureRange tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002C), "Sigma:ColorMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Sigma:Sepia".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Sepia tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Sigma:B&W".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "B&W tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Sigma:Standard".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Standard tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Sigma:Vivid".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Vivid tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "Sigma:Neutral".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Neutral tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Sigma:Portrait".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Portrait tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Sigma:Landscape".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Landscape tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Sigma:FOV Classic Blue".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FOV Classic Blue tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0031), "Sigma:FNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0032), "Sigma:ExposureTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0033), "Sigma:ExposureTime2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureTime2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0034), "Sigma:BurstShot".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BurstShot tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0035), "Sigma:ExposureCompensation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureCompensation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0039), "Sigma:SensorTemperature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorTemperature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003A), "Sigma:FlashExposureComp".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FlashExposureComp tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003B), "Sigma:Firmware".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Firmware tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003C), "Sigma:WhiteBalance".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WhiteBalance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003D), "Sigma:PictureMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PictureMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0047), "Sigma:ExposureCompensation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureCompensation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0048), "Sigma:LensApertureRange".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LensApertureRange tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0049), "Sigma:FNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004A), "Sigma:ExposureTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004D), "Sigma:ExposureCompensation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureCompensation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0055), "Sigma:SensorTemperature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorTemperature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0056), "Sigma:FlashExposureComp".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FlashExposureComp tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0057), "Sigma:Firmware2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Firmware2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0058), "Sigma:WhiteBalance".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WhiteBalance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0059), "Sigma:DigitalFilter".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DigitalFilter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0084), "Sigma:Model".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Model tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0086), "Sigma:ISO".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ISO tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x011F), "Sigma:CameraCalibration".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CameraCalibration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0120), "Sigma:WBSettings".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WBSettings tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0121), "Sigma:WBSettings2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WBSettings2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0138), "Sigma:Fade".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Fade tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0139), "Sigma:Vignette".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Vignette tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Sigma:WB_RGBLevelsAuto".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsAuto tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Sigma:WB_RGBLevelsDaylight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsDaylight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Sigma:WB_RGBLevelsShade".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsShade tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Sigma:WB_RGBLevelsOvercast".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsOvercast tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "Sigma:WB_RGBLevelsIncandescent".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsIncandescent tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "Sigma:WB_RGBLevelsFluorescent".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsFluorescent tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "Sigma:WB_RGBLevelsFlash".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsFlash tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "Sigma:WB_RGBLevelsCustom1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsCustom1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "Sigma:WB_RGBLevelsCustom2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsCustom2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001B), "Sigma:WB_RGBLevelsCustom3".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsCustom3 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Sigma:WB_RGBLevelsUnknown0".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown0 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Sigma:WB_RGBLevelsUnknown1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Sigma:WB_RGBLevelsUnknown2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Sigma:WB_RGBLevelsUnknown3".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown3 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "Sigma:WB_RGBLevelsUnknown4".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown4 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "Sigma:WB_RGBLevelsUnknown5".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown5 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "Sigma:WB_RGBLevelsUnknown6".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown6 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "Sigma:WB_RGBLevelsUnknown7".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown7 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "Sigma:WB_RGBLevelsUnknown8".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown8 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001B), "Sigma:WB_RGBLevelsUnknown9".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RGBLevelsUnknown9 tag".to_string(), vec!["Example".to_string()]),
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
