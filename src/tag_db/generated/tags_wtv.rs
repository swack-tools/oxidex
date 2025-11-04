//! WTV format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xB9C4), "WTV:Metdata".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Metdata tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB1B4), "WTV:Duration".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Duration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xECAE), "WTV:MediaIsDelay".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsDelay tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x9FE4), "WTV:MediaIsFinale".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsFinale tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xEC61), "WTV:MediaIsLive".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsLive tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x725B), "WTV:MediaIsMovie".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsMovie tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3A6E), "WTV:MediaIsPremiere".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsPremiere tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5FD0), "WTV:MediaIsRepeat".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsRepeat tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB9CD), "WTV:MediaIsSAP".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsSAP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5A9F), "WTV:MediaIsSport".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsSport tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xCA2D), "WTV:MediaIsStereo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsStereo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xDCF7), "WTV:MediaIsSubtitled".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsSubtitled tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7097), "WTV:MediaIsTape".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaIsTape tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE0A0), "WTV:MediaOriginalBroadcastDateTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaOriginalBroadcastDateTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x8F27), "WTV:MediaOriginalChannel".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaOriginalChannel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x8242), "WTV:MediaOriginalChannelSubNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaOriginalChannelSubNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x561C), "WTV:MediaOriginalRunTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaOriginalRunTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x263F), "WTV:MediaThumbRatingAttributes".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaThumbRatingAttributes tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5BEB), "WTV:MediaThumbTimeStamp".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaThumbTimeStamp tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7C0A), "WTV:OriginalReleaseTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OriginalReleaseTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0ECA), "WTV:VideoClosedCaptioning".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VideoClosedCaptioning tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC823), "WTV:ATSCContent".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ATSCContent tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x50BA), "WTV:Bitrate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Bitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2088), "WTV:ContentProtected".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ContentProtected tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x8746), "WTV:DTVContent".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DTVContent tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1596), "WTV:EncodeTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EncodeTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3F95), "WTV:EndTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EndTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC910), "WTV:ExpirationDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExpirationDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD0AC), "WTV:ExpirationSpan".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExpirationSpan tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x860A), "WTV:HDContent".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "HDContent tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAEFB), "WTV:Watched".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Watched tag".to_string(), vec!["Example".to_string()]),
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
