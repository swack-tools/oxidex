//! GoPro format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0003), "GoPro:3-Dimensional Measurement".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "3-Dimensional Measurement tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "GoPro:GPSLatitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLatitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GoPro:GPSLongitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLongitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GoPro:GPSAltitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSAltitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GoPro:GPSSpeed".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSSpeed tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "GoPro:GPSSpeed3D".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSSpeed3D tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "GoPro:GPSLatitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLatitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GoPro:GPSLongitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLongitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GoPro:GPSAltitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSAltitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GoPro:GPSSpeed".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSSpeed tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "GoPro:GPSSpeed3D".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSSpeed3D tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "GoPro:GPSDays".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSDays tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "GoPro:GPSDateTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSDateTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "GoPro:GPSDOP".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSDOP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "GoPro:GPSMeasureMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSMeasureMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GoPro:3-Dimensional Measurement".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "3-Dimensional Measurement tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "GoPro:GPSDateTimeRaw".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSDateTimeRaw tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GoPro:GPSLatitudeRaw".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLatitudeRaw tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GoPro:GPSLongitudeRaw".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLongitudeRaw tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GoPro:GPSAltitudeRaw".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSAltitudeRaw tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "GoPro:GPRI_Unknown4".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPRI_Unknown4 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "GoPro:GPRI_Unknown5".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPRI_Unknown5 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "GoPro:GPSDateTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSDateTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GoPro:GPSLatitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLatitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GoPro:GPSLongitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSLongitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GoPro:GPSAltitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSAltitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "GoPro:GLPI_Unknown4".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GLPI_Unknown4 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "GoPro:GPSSpeedX".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSSpeedX tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "GoPro:GPSSpeedY".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSSpeedY tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "GoPro:GPSSpeedZ".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSSpeedZ tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "GoPro:GPSTrack".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GPSTrack tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "GoPro:BatteryCurrent".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BatteryCurrent tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "GoPro:BatteryCapacity".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BatteryCapacity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "GoPro:KBAT_Unknown2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "KBAT_Unknown2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "GoPro:BatteryTemperature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BatteryTemperature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "GoPro:BatteryVoltage1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BatteryVoltage1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "GoPro:BatteryVoltage2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BatteryVoltage2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "GoPro:BatteryVoltage3".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BatteryVoltage3 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "GoPro:BatteryVoltage4".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BatteryVoltage4 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "GoPro:BatteryTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BatteryTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "GoPro:KBAT_Unknown9".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "KBAT_Unknown9 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "GoPro:KBAT_Unknown10".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "KBAT_Unknown10 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "GoPro:KBAT_Unknown11".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "KBAT_Unknown11 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "GoPro:KBAT_Unknown12".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "KBAT_Unknown12 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "GoPro:KBAT_Unknown13".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "KBAT_Unknown13 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "GoPro:BatteryLevel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BatteryLevel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "GoPro:FirmwareVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FirmwareVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0017), "GoPro:SerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0057), "GoPro:OtherSerialNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OtherSerialNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0066), "GoPro:Model".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Model tag".to_string(), vec!["Example".to_string()]),
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
