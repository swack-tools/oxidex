//! MPEG format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x7C21), "MPEG:MPEGAudioVersion".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MPEGAudioVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x64E1), "MPEG:AudioLayer".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AudioLayer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF0E1), "MPEG:ChannelMode".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ChannelMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPEG:Joint Stereo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Joint Stereo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPEG:Dual Channel".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Dual Channel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPEG:Single Channel".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Single Channel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x83F1), "MPEG:MSStereo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MSStereo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x83F2), "MPEG:IntensityStereo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "IntensityStereo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD9A1), "MPEG:ModeExtension".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ModeExtension tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPEG:Bands 8-31".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Bands 8-31 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPEG:Bands 12-31".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Bands 12-31 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPEG:Bands 16-31".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Bands 16-31 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x83F3), "MPEG:CopyrightFlag".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CopyrightFlag tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPEG:True".to_string(), FormatFamily::QuickTime, false, ValueType::String, "True tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x83F4), "MPEG:OriginalMedia".to_string(), FormatFamily::QuickTime, false, ValueType::String, "OriginalMedia tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPEG:True".to_string(), FormatFamily::QuickTime, false, ValueType::String, "True tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3701), "MPEG:Emphasis".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Emphasis tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPEG:50/15 ms".to_string(), FormatFamily::QuickTime, false, ValueType::String, "50/15 ms tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPEG:reserved".to_string(), FormatFamily::QuickTime, false, ValueType::String, "reserved tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPEG:CCIT J.17".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CCIT J.17 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF0E3), "MPEG:AspectRatio".to_string(), FormatFamily::QuickTime, false, ValueType::String, "AspectRatio tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC278), "MPEG:FrameRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FrameRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1FE6), "MPEG:VideoBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VideoBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPEG:VBRFrames".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VBRFrames tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPEG:VBRBytes".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VBRBytes tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPEG:VBRScale".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VBRScale tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MPEG:Encoder".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Encoder tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MPEG:LameVBRQuality".to_string(), FormatFamily::QuickTime, false, ValueType::String, "LameVBRQuality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MPEG:LameQuality".to_string(), FormatFamily::QuickTime, false, ValueType::String, "LameQuality tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "MPEG:LameHeader".to_string(), FormatFamily::QuickTime, false, ValueType::String, "LameHeader tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "MPEG:LameMethod".to_string(), FormatFamily::QuickTime, false, ValueType::String, "LameMethod tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPEG:ABR".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ABR tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPEG:VBR (old/rh)".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VBR (old/rh) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MPEG:VBR (new/mtrh)".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VBR (new/mtrh) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MPEG:VBR (old/rh)".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VBR (old/rh) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MPEG:VBR".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VBR tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "MPEG:CBR (2-pass)".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CBR (2-pass) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "MPEG:ABR (2-pass)".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ABR (2-pass) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "MPEG:LameLowPassFilter".to_string(), FormatFamily::QuickTime, false, ValueType::String, "LameLowPassFilter tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "MPEG:LameBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "LameBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "MPEG:LameStereoMode".to_string(), FormatFamily::QuickTime, false, ValueType::String, "LameStereoMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPEG:Stereo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Stereo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPEG:Dual Channels".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Dual Channels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPEG:Joint Stereo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Joint Stereo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MPEG:Forced Joint Stereo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Forced Joint Stereo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MPEG:Auto".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Auto tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "MPEG:Intensity Stereo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Intensity Stereo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPEG:MPEG:AudioBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MPEG:AudioBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPEG:MPEG:VideoBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MPEG:VideoBitrate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MPEG:MPEG:VBRFrames".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MPEG:VBRFrames tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MPEG:MPEG:SampleRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MPEG:SampleRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "MPEG:MPEG:MPEGAudioVersion".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MPEG:MPEGAudioVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPEG:MPEG:SampleRate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MPEG:SampleRate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPEG:MPEG:VBRBytes".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MPEG:VBRBytes tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPEG:MPEG:VBRFrames".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MPEG:VBRFrames tag".to_string(), vec!["Example".to_string()]),
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
