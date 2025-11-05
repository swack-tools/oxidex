//! H264 format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0013), "H264:TimeCode".to_string(), FormatFamily::QuickTime, false, ValueType::String, "TimeCode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "H264:DateTimeOriginal".to_string(), FormatFamily::QuickTime, false, ValueType::String, "DateTimeOriginal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0070), "H264:Camera1".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Camera1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0071), "H264:Camera2".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Camera2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x007F), "H264:Shutter".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Shutter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A0), "H264:ExposureTime".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ExposureTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A1), "H264:FNumber".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A2), "H264:ExposureProgram".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ExposureProgram tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "H264:Manual".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Manual tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "H264:Program AE".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Program AE tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "H264:Aperture-priority AE".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Aperture-priority AE tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "H264:Shutter speed priority AE".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Shutter speed priority AE tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "H264:Creative (Slow speed)".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Creative (Slow speed) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "H264:Action (High speed)".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Action (High speed) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "H264:Portrait".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Portrait tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "H264:Landscape".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Landscape tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A3), "H264:BrightnessValue".to_string(), FormatFamily::QuickTime, false, ValueType::String, "BrightnessValue tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A4), "H264:ExposureCompensation".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ExposureCompensation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A5), "H264:MaxApertureValue".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MaxApertureValue tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A6), "H264:Flash".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Flash tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A7), "H264:CustomRendered".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CustomRendered tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "H264:Custom".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Custom tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A8), "H264:WhiteBalance".to_string(), FormatFamily::QuickTime, false, ValueType::String, "WhiteBalance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "H264:Manual".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Manual tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00A9), "H264:FocalLengthIn35mmFormat".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FocalLengthIn35mmFormat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00AA), "H264:SceneCaptureType".to_string(), FormatFamily::QuickTime, false, ValueType::String, "SceneCaptureType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "H264:Landscape".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Landscape tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "H264:Portrait".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Portrait tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "H264:Night".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Night tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00B0), "H264:GPSVersionID".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSVersionID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00B1), "H264:GPSLatitudeRef".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSLatitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00B2), "H264:GPSLatitude".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSLatitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00B5), "H264:GPSLongitudeRef".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSLongitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00B6), "H264:GPSLongitude".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSLongitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00B9), "H264:GPSAltitudeRef".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSAltitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "H264:Below Sea Level".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Below Sea Level tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00BA), "H264:GPSAltitude".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSAltitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00BB), "H264:GPSTimeStamp".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSTimeStamp tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00BE), "H264:GPSStatus".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSStatus tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00BF), "H264:GPSMeasureMode".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSMeasureMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "H264:3-Dimensional Measurement".to_string(), FormatFamily::QuickTime, false, ValueType::String, "3-Dimensional Measurement tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00C0), "H264:GPSDOP".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSDOP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00C1), "H264:GPSSpeedRef".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSSpeedRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00C2), "H264:GPSSpeed".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSSpeed tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00C3), "H264:GPSTrackRef".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSTrackRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00C4), "H264:GPSTrack".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSTrack tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00C5), "H264:GPSImgDirectionRef".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSImgDirectionRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00C6), "H264:GPSImgDirection".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSImgDirection tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00C7), "H264:GPSMapDatum".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSMapDatum tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00CA), "H264:GPSDateStamp".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GPSDateStamp tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00E0), "H264:MakeModel".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MakeModel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00E1), "H264:RecInfo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "RecInfo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00E4), "H264:Model".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Model tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00EE), "H264:FrameInfo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FrameInfo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "H264:ApertureSetting".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ApertureSetting tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00FE), "H264:Closed".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Closed tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "H264:Gain".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Gain tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "H264:Aperture-priority AE".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Aperture-priority AE tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "H264:Manual".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Manual tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "H264:Hold".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Hold tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "H264:1-Push".to_string(), FormatFamily::QuickTime, false, ValueType::String, "1-Push tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "H264:Daylight".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Daylight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "H264:Focus".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Focus tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "H264:ImageStabilization".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ImageStabilization tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "H264:Make".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Make tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "H264:RecordingMode".to_string(), FormatFamily::QuickTime, false, ValueType::String, "RecordingMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "H264:CaptureFrameRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CaptureFrameRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "H264:VideoFrameRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VideoFrameRate tag".to_string(), vec!["Example".to_string()]),
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
