//! Stim format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "Stim:StimVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "StimVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Stim:ApplicationData".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ApplicationData tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Stim:ImageArrangement".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageArrangement tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Stim:Cross View Alignment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Cross View Alignment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Stim:ImageRotation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageRotation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Stim:ScalingFactor".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ScalingFactor tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "Stim:CropXSize".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropXSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Stim:CropYSize".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropYSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Stim:CropX".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropX tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Stim:CropY".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropY tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "Stim:ViewType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ViewType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Stim:Pop-up Effect".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Pop-up Effect tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "Stim:RepresentativeImage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RepresentativeImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Stim:Right Viewpoint".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Right Viewpoint tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "Stim:ConvergenceBaseImage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ConvergenceBaseImage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Stim:Right Viewpoint".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Right Viewpoint tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00FF), "Stim:Equivalent for Both Viewpoints".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Equivalent for Both Viewpoints tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "Stim:AssumedDisplaySize".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AssumedDisplaySize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "Stim:AssumedDistanceView".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AssumedDistanceView tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "Stim:RepresentativeDisparityNear".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RepresentativeDisparityNear tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "Stim:RepresentativeDisparityFar".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RepresentativeDisparityFar tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "Stim:InitialDisplayEffect".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "InitialDisplayEffect tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Stim:On".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "On tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "Stim:ConvergenceDistance".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ConvergenceDistance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "Stim:CameraArrangementInterval".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CameraArrangementInterval tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "Stim:ShootingCount".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ShootingCount tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Stim:CropXCommonOffset".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropXCommonOffset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Stim:Individual Offset Setting".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Individual Offset Setting tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Stim:CropXViewpointNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropXViewpointNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Stim:CropXOffset".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropXOffset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Stim:CropXViewpointNumber2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropXViewpointNumber2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Stim:CropXOffset2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropXOffset2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "Stim:CropYCommonOffset".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropYCommonOffset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Stim:Individual Offset Setting".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Individual Offset Setting tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Stim:CropYViewpointNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropYViewpointNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Stim:CropYOffset".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropYOffset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Stim:CropYViewpointNumber2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropYViewpointNumber2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Stim:CropYOffset2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CropYOffset2 tag".to_string(), vec!["Example".to_string()]),
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
