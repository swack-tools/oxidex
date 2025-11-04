//! MinoltaRaw format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "MinoltaRaw:FirmwareID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FirmwareID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "MinoltaRaw:SensorHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "MinoltaRaw:SensorWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SensorWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "MinoltaRaw:ImageHeight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "MinoltaRaw:ImageWidth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "MinoltaRaw:RawDepth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RawDepth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "MinoltaRaw:BitDepth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BitDepth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "MinoltaRaw:StorageMethod".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "StorageMethod tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0059), "MinoltaRaw:Linear".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Linear tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0017), "MinoltaRaw:BayerPattern".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BayerPattern tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MinoltaRaw:GBRG".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GBRG tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "MinoltaRaw:WBScale".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WBScale tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MinoltaRaw:Saturation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Saturation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MinoltaRaw:Contrast".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Contrast tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MinoltaRaw:Sharpness".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Sharpness tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MinoltaRaw:WBMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WBMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MinoltaRaw:ProgramMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ProgramMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MinoltaRaw:Portrait".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Portrait tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MinoltaRaw:Text".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Text tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MinoltaRaw:Night Portrait".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Night Portrait tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MinoltaRaw:Sunset".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Sunset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MinoltaRaw:Sports".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Sports tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MinoltaRaw:ISOSetting".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ISOSetting tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00AE), "MinoltaRaw:80 (Zone Matching Low)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "80 (Zone Matching Low) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00B8), "MinoltaRaw:200 (Zone Matching High)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "200 (Zone Matching High) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "MinoltaRaw:WB_RBLevelsTungsten".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsTungsten tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "MinoltaRaw:WB_RBLevelsDaylight".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsDaylight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "MinoltaRaw:WB_RBLevelsCloudy".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsCloudy tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "MinoltaRaw:WB_RBLevelsCoolWhiteF".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsCoolWhiteF tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "MinoltaRaw:WB_RBLevelsFlash".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsFlash tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001C), "MinoltaRaw:WB_RBLevelsCustom".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsCustom tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "MinoltaRaw:WB_RBLevelsShade".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsShade tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0024), "MinoltaRaw:WB_RBLevelsDaylightF".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsDaylightF tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0028), "MinoltaRaw:WB_RBLevelsDayWhiteF".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsDayWhiteF tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002C), "MinoltaRaw:WB_RBLevelsWhiteF".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "WB_RBLevelsWhiteF tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0038), "MinoltaRaw:ColorFilter".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorFilter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0039), "MinoltaRaw:BWFilter".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BWFilter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003A), "MinoltaRaw:ZoneMatching".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZoneMatching tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MinoltaRaw:High Key".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "High Key tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MinoltaRaw:Low Key".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Low Key tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003B), "MinoltaRaw:Hue".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Hue tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003C), "MinoltaRaw:ColorTemperature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorTemperature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004A), "MinoltaRaw:ZoneMatching".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZoneMatching tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MinoltaRaw:High Key".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "High Key tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MinoltaRaw:Low Key".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Low Key tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004C), "MinoltaRaw:ColorTemperature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorTemperature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004D), "MinoltaRaw:ColorFilter".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorFilter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004E), "MinoltaRaw:ColorTemperature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorTemperature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004F), "MinoltaRaw:ColorFilter".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorFilter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0050), "MinoltaRaw:RawDataLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RawDataLength tag".to_string(), vec!["Example".to_string()]),
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
