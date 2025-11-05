//! Flash format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0045), "Flash:FlashAttributes".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FlashAttributes tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Flash:ActionScript3".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ActionScript3 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Flash:HasMetadata".to_string(), FormatFamily::QuickTime, false, ValueType::String, "HasMetadata tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004D), "Flash:XMP".to_string(), FormatFamily::QuickTime, false, ValueType::String, "XMP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Flash:Audio".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Audio tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Flash:Video".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Video tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "Flash:Meta".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Meta tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF1C9), "Flash:AudioEncoding".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioEncoding tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Flash:MP3".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MP3 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Flash:Nellymoser".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Nellymoser tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00CF), "Flash:AudioSampleRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioSampleRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x98E9), "Flash:AudioBitsPerSample".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioBitsPerSample tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x98EA), "Flash:AudioChannels".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioChannels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Flash:2 (stereo)".to_string(), FormatFamily::QuickTime, false, ValueType::String, "2 (stereo) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00D1), "Flash:VideoEncoding".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VideoEncoding tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Flash:Screen Video".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Screen Video tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Flash:On2 VP6".to_string(), FormatFamily::QuickTime, false, ValueType::String, "On2 VP6 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF71B), "Flash:AudioCodecID".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioCodecID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xBD60), "Flash:AudioBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x136D), "Flash:AudioDelay".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioDelay tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x25E0), "Flash:AudioSampleRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioSampleRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB901), "Flash:AudioSampleSize".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioSampleSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xA0D7), "Flash:AudioSize".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD70D), "Flash:CreateDate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CreateDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4D94), "Flash:Duration".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Duration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x77CD), "Flash:FrameRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FrameRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xA95C), "Flash:HasAudio".to_string(), FormatFamily::QuickTime, false, ValueType::String, "HasAudio tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3B5D), "Flash:MetadataDate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MetadataDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x97F8), "Flash:Stereo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Stereo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xA4D8), "Flash:TotalDuration".to_string(), FormatFamily::QuickTime, false, ValueType::String, "TotalDuration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAECE), "Flash:TotalDataRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "TotalDataRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC105), "Flash:VideoBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VideoBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xEB8B), "Flash:XMP".to_string(), FormatFamily::QuickTime, false, ValueType::String, "XMP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC1EA), "Flash:Parameter".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Parameter tag".to_string(), vec!["Example".to_string()]),
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
