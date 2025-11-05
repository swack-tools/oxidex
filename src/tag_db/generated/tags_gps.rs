//! GPS format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "GPS:GPSVersionID".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSVersionID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GPS:GPSLatitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSLatitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GPS:GPSLatitude".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSLatitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GPS:GPSLongitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSLongitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "GPS:GPSLongitude".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSLongitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "GPS:GPSAltitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSAltitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "GPS:GPSAltitude".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSAltitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "GPS:GPSTimeStamp".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSTimeStamp tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "GPS:GPSSatellites".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSSatellites tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "GPS:GPSStatus".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSStatus tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "GPS:GPSMeasureMode".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSMeasureMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GPS:3-Dimensional Measurement".to_string(), FormatFamily::GPS, false, ValueType::String, "3-Dimensional Measurement tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "GPS:GPSDOP".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDOP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "GPS:GPSSpeedRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSSpeedRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "GPS:GPSSpeed".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSSpeed tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "GPS:GPSTrackRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSTrackRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "GPS:GPSTrack".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSTrack tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "GPS:GPSImgDirectionRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSImgDirectionRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "GPS:GPSImgDirection".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSImgDirection tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "GPS:GPSMapDatum".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSMapDatum tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "GPS:GPSDestLatitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDestLatitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "GPS:GPSDestLatitude".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDestLatitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "GPS:GPSDestLongitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDestLongitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0016), "GPS:GPSDestLongitude".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDestLongitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0017), "GPS:GPSDestBearingRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDestBearingRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "GPS:GPSDestBearing".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDestBearing tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0019), "GPS:GPSDestDistanceRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDestDistanceRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001A), "GPS:GPSDestDistance".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDestDistance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001B), "GPS:GPSProcessingMethod".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSProcessingMethod tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001C), "GPS:GPSAreaInformation".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSAreaInformation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001D), "GPS:GPSDateStamp".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDateStamp tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001E), "GPS:GPSDifferential".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSDifferential tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GPS:Differential Corrected".to_string(), FormatFamily::GPS, false, ValueType::String, "Differential Corrected tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001F), "GPS:GPSHPositioningError".to_string(), FormatFamily::GPS, false, ValueType::String, "GPSHPositioningError tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GPS:GPS:GPSTimeStamp".to_string(), FormatFamily::GPS, false, ValueType::String, "GPS:GPSTimeStamp tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GPS:GPS:GPSLatitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPS:GPSLatitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GPS:GPS:GPSLongitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPS:GPSLongitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GPS:GPS:GPSLongitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPS:GPSLongitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GPS:GPS:GPSAltitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPS:GPSAltitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GPS:XMP:GPSAltitude".to_string(), FormatFamily::GPS, false, ValueType::String, "XMP:GPSAltitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GPS:XMP:GPSAltitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "XMP:GPSAltitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GPS:GPS:GPSDestLatitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPS:GPSDestLatitudeRef tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GPS:GPS:GPSDestLongitudeRef".to_string(), FormatFamily::GPS, false, ValueType::String, "GPS:GPSDestLongitudeRef tag".to_string(), vec!["Example".to_string()]),
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
