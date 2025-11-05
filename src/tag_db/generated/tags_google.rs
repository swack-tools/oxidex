//! Google format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xBD96), "Google:ImageData".to_string(), FormatFamily::MakerNotes, false, ValueType::Binary, "ImageData tag".to_string(), vec!["Value".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Google:TimeLogText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TimeLogText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Google:SummaryText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SummaryText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xDB9F), "Google:FrameCount".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FrameCount tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5C5B), "Google:CreateDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CreateDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0785), "Google:DeviceMake".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DeviceMake tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0786), "Google:DeviceModel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DeviceModel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0787), "Google:DeviceCodename".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DeviceCodename tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0788), "Google:DeviceHardwareRevision".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DeviceHardwareRevision tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x078A), "Google:HDRPSoftware".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "HDRPSoftware tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x078B), "Google:AndroidRelease".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AndroidRelease tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x078C), "Google:SoftwareDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SoftwareDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x078D), "Google:Application".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Application tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE94B), "Google:AppVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AppVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xCFB1), "Google:ExposureTimeMin".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureTimeMin tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xCFB2), "Google:ExposureTimeMax".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExposureTimeMax tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD372), "Google:ISOMin".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "ISOMin tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD373), "Google:ISOMax".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "ISOMax tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE94F), "Google:MaxAnalogISO".to_string(), FormatFamily::MakerNotes, false, ValueType::Float, "MaxAnalogISO tag".to_string(), vec!["1.5".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Google:MeteringFrameCount".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MeteringFrameCount tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Google:OriginalPayloadFrameCount".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalPayloadFrameCount tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1AD6), "Google:InitParamsText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "InitParamsText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2EF0), "Google:LoggingMetadataText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LoggingMetadataText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE387), "Google:MergedImage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MergedImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAC4D), "Google:FinishedImage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FinishedImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xEC3B), "Google:PayloadFrame".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PayloadFrame tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x9DA1), "Google:PayloadMetadataText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PayloadMetadataText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xBC94), "Google:ShotLogDataText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ShotLogDataText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF940), "Google:ShotParamsText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ShotParamsText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2BDD), "Google:StaticMetadataText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "StaticMetadataText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3F86), "Google:SummaryText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SummaryText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB791), "Google:TimeLogText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TimeLogText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x23BA), "Google:UnusedLoggingMetadata".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "UnusedLoggingMetadata tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xCEC2), "Google:RectifaceText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RectifaceText tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF985), "Google:GoudaRequestText".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GoudaRequestText tag".to_string(), vec!["Example".to_string()]),
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
