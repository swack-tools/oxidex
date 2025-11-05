//! Microsoft format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "Microsoft:PanoramicStitchVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanoramicStitchVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Microsoft:PanoramicStitchCameraMotion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanoramicStitchCameraMotion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Microsoft:Affine".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Affine tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Microsoft:3D Rotation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "3D Rotation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "Microsoft:Homography".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Homography tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Microsoft:PanoramicStitchMapType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanoramicStitchMapType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Microsoft:Horizontal Cylindrical".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Horizontal Cylindrical tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Microsoft:Horizontal Spherical".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Horizontal Spherical tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0101), "Microsoft:Vertical Cylindrical".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Vertical Cylindrical tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0102), "Microsoft:Vertical Spherical".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Vertical Spherical tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Microsoft:PanoramicStitchTheta0".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanoramicStitchTheta0 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Microsoft:PanoramicStitchTheta1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanoramicStitchTheta1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "Microsoft:PanoramicStitchPhi0".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanoramicStitchPhi0 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Microsoft:PanoramicStitchPhi1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanoramicStitchPhi1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF8BD), "Microsoft:AlbumArtist".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "AlbumArtist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xDA2E), "Microsoft:AlbumCoverURL".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "AlbumCoverURL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0222), "Microsoft:AlbumTitle".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "AlbumTitle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4837), "Microsoft:Category".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Category tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4C59), "Microsoft:Composer".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Composer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD20E), "Microsoft:Conductor".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Conductor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4DA7), "Microsoft:ContentDistributor".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "ContentDistributor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xDEC5), "Microsoft:Director".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Director tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5979), "Microsoft:EncodingTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "EncodingTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xF974), "Microsoft:InitialKey".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "InitialKey tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4030), "Microsoft:MediaClassPrimaryID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaClassPrimaryID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7662), "Microsoft:MediaClassSecondaryID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaClassSecondaryID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE0A0), "Microsoft:MediaOriginalBroadcastDateTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MediaOriginalBroadcastDateTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x4AB0), "Microsoft:Mood".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Mood tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0DD3), "Microsoft:OriginalAlbumTitle".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "OriginalAlbumTitle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x70F1), "Microsoft:OriginalArtist".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "OriginalArtist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x72F5), "Microsoft:OriginalLyricist".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "OriginalLyricist tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xD8CB), "Microsoft:ParentalRating".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "ParentalRating tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7B3A), "Microsoft:Period".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Period tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5ACB), "Microsoft:Producer".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Producer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x912A), "Microsoft:Provider".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Provider tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x7DE3), "Microsoft:Publisher".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Publisher tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x95C6), "Microsoft:SharedUserRating".to_string(), FormatFamily::MakerNotes, true, ValueType::Integer, "SharedUserRating tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE731), "Microsoft:Subtitle".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Subtitle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xA9EC), "Microsoft:Writer".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "Writer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x97F6), "Microsoft:Year".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Year tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2C85), "Microsoft:PromotionURL".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "PromotionURL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x93CB), "Microsoft:AuthorURL".to_string(), FormatFamily::MakerNotes, true, ValueType::String, "AuthorURL tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xBD3C), "Microsoft:DateAcquired".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateAcquired tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x311F), "Microsoft:DateModified".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateModified tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3120), "Microsoft:DateCreated".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateCreated tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3121), "Microsoft:DateAccessed".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateAccessed tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xED0F), "Microsoft:Author".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Author tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x5617), "Microsoft:Copyright".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Copyright tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xCD94), "Microsoft:Year".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Year tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xE06B), "Microsoft:DatePictureTaken".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DatePictureTaken tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x904E), "Microsoft:DateArchived".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateArchived tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x3C7E), "Microsoft:DateCompleted".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateCompleted tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x8383), "Microsoft:DateImported".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateImported tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB4A7), "Microsoft:DateLastSaved".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateLastSaved tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x538B), "Microsoft:Creator".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Creator tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xAC09), "Microsoft:Date".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Date tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x2602), "Microsoft:DateVisited".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateVisited tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x8F75), "Microsoft:DateReleased".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateReleased tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC8C0), "Microsoft:DateReceived".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateReceived tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xC8AA), "Microsoft:DateSent".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DateSent tag".to_string(), vec!["Example".to_string()]),
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
