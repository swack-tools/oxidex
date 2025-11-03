//! FLAC format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "FLAC:StreamInfo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "StreamInfo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "FLAC:Padding".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Padding tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "FLAC:SeekTable".to_string(), FormatFamily::QuickTime, false, ValueType::String, "SeekTable tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "FLAC:VorbisComment".to_string(), FormatFamily::QuickTime, false, ValueType::String, "VorbisComment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "FLAC:CueSheet".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CueSheet tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "FLAC:Picture".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Picture tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x968A), "FLAC:Channels".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Channels tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xDD12), "FLAC:BitsPerSample".to_string(), FormatFamily::QuickTime, false, ValueType::String, "BitsPerSample tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5BA3), "FLAC:MD5Signature".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MD5Signature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "FLAC:PictureType".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "FLAC:32x32 PNG Icon".to_string(), FormatFamily::QuickTime, false, ValueType::String, "32x32 PNG Icon tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "FLAC:Other Icon".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Other Icon tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "FLAC:Front Cover".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Front Cover tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "FLAC:Back Cover".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Back Cover tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "FLAC:Leaflet".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Leaflet tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "FLAC:Media".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Media tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "FLAC:Lead Artist".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Lead Artist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "FLAC:Artist".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Artist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "FLAC:Conductor".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Conductor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "FLAC:Band".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Band tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "FLAC:Composer".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Composer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "FLAC:Lyricist".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Lyricist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "FLAC:Recording Studio or Location".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Recording Studio or Location tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "FLAC:Recording Session".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Recording Session tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "FLAC:Performance".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Performance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "FLAC:Capture from Movie or Video".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Capture from Movie or Video tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "FLAC:Bright(ly) Colored Fish".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Bright(ly) Colored Fish tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "FLAC:Illustration".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Illustration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "FLAC:Band Logo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Band Logo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "FLAC:Publisher Logo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Publisher Logo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "FLAC:PictureMIMEType".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureMIMEType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "FLAC:PictureDescription".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureDescription tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "FLAC:PictureWidth".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureWidth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "FLAC:PictureHeight".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureHeight tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "FLAC:PictureBitsPerPixel".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureBitsPerPixel tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "FLAC:PictureIndexedColors".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureIndexedColors tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "FLAC:PictureLength".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "FLAC:Picture".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Picture tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "FLAC:FLAC:TotalSamples".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FLAC:TotalSamples tag".to_string(), vec!["Example".to_string()]),
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
