//! ASF format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xF3C6), "ASF:Header".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Header tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC46E), "ASF:XMP".to_string(), FormatFamily::QuickTime, false, ValueType::String, "XMP tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xA522), "ASF:FileProperties".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FileProperties tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF006), "ASF:StreamProperties".to_string(), FormatFamily::QuickTime, false, ValueType::String, "StreamProperties tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x1823), "ASF:HeaderExtension".to_string(), FormatFamily::QuickTime, false, ValueType::String, "HeaderExtension tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3BF6), "ASF:CodecList".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CodecList tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xA149), "ASF:ContentDescription".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ContentDescription tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x9E41), "ASF:ContentBranding".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ContentBranding tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xFA81), "ASF:ExtendedContentDescr".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ExtendedContentDescr tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "ASF:Title".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Title tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ASF:Author".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Author tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ASF:Copyright".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Copyright tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ASF:Description".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Description tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ASF:Rating".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Rating tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "ASF:BannerImageType".to_string(), FormatFamily::QuickTime, false, ValueType::String, "BannerImageType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ASF:Bitmap".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Bitmap tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ASF:JPEG".to_string(), FormatFamily::QuickTime, false, ValueType::String, "JPEG tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ASF:GIF".to_string(), FormatFamily::QuickTime, false, ValueType::String, "GIF tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ASF:BannerImage".to_string(), FormatFamily::QuickTime, false, ValueType::String, "BannerImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ASF:BannerImageURL".to_string(), FormatFamily::QuickTime, false, ValueType::String, "BannerImageURL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ASF:CopyrightURL".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CopyrightURL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "ASF:PictureType".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ASF:32x32 PNG Icon".to_string(), FormatFamily::QuickTime, false, ValueType::String, "32x32 PNG Icon tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ASF:Other Icon".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Other Icon tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ASF:Front Cover".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Front Cover tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ASF:Back Cover".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Back Cover tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "ASF:Leaflet".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Leaflet tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "ASF:Media".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Media tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "ASF:Lead Artist".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Lead Artist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "ASF:Artist".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Artist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "ASF:Conductor".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Conductor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "ASF:Band".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Band tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "ASF:Composer".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Composer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "ASF:Lyricist".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Lyricist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "ASF:Recording Studio or Location".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Recording Studio or Location tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "ASF:Recording Session".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Recording Session tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "ASF:Performance".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Performance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "ASF:Capture from Movie or Video".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Capture from Movie or Video tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "ASF:Bright(ly) Colored Fish".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Bright(ly) Colored Fish tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "ASF:Illustration".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Illustration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "ASF:Band Logo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Band Logo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "ASF:Publisher Logo".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Publisher Logo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ASF:PictureMIMEType".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureMIMEType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ASF:PictureDescription".to_string(), FormatFamily::QuickTime, false, ValueType::String, "PictureDescription tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ASF:Picture".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Picture tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "ASF:FileID".to_string(), FormatFamily::QuickTime, false, ValueType::String, "FileID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "ASF:FileLength".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "FileLength tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0018), "ASF:CreationDate".to_string(), FormatFamily::QuickTime, false, ValueType::String, "CreationDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "ASF:DataPackets".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "DataPackets tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0028), "ASF:Duration".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Duration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0030), "ASF:SendDuration".to_string(), FormatFamily::QuickTime, false, ValueType::String, "SendDuration tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0038), "ASF:Preroll".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "Preroll tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0040), "ASF:Flags".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "Flags tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0044), "ASF:MinPacketSize".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "MinPacketSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0048), "ASF:MaxPacketSize".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "MaxPacketSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004C), "ASF:MaxBitrate".to_string(), FormatFamily::QuickTime, false, ValueType::Integer, "MaxBitrate tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "ASF:StreamType".to_string(), FormatFamily::QuickTime, false, ValueType::String, "StreamType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "ASF:ErrorCorrectionType".to_string(), FormatFamily::QuickTime, false, ValueType::String, "ErrorCorrectionType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "ASF:TimeOffset".to_string(), FormatFamily::QuickTime, false, ValueType::String, "TimeOffset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0030), "ASF:StreamNumber".to_string(), FormatFamily::QuickTime, false, ValueType::String, "StreamNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAC87), "ASF:Metadata".to_string(), FormatFamily::QuickTime, false, ValueType::String, "Metadata tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x60F8), "ASF:MetadataLibrary".to_string(), FormatFamily::QuickTime, false, ValueType::String, "MetadataLibrary tag".to_string(), vec!["Example".to_string()]),
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
