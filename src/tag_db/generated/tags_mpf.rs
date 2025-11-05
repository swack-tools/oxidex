//! MPF format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0xB000), "MPF:MPFVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MPFVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB001), "MPF:NumberOfImages".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "NumberOfImages tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB002), "MPF:MPImageList".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MPImageList tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB003), "MPF:ImageUIDList".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageUIDList tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB004), "MPF:TotalFrames".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TotalFrames tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB101), "MPF:MPIndividualNum".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MPIndividualNum tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB201), "MPF:PanOrientation".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanOrientation tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPF:Start at top right".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Start at top right tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPF:Start at top left".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Start at top left tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPF:Start at bottom left".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Start at bottom left tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MPF:Start at bottom right".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Start at bottom right tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPF:Right to left".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Right to left tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPF:Top to bottom".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Top to bottom tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MPF:Bottom to top".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Bottom to top tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0010), "MPF:Clockwise".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Clockwise tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "MPF:Counter clockwise".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Counter clockwise tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0030), "MPF:Zigzag (row start)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Zigzag (row start) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0040), "MPF:Zigzag (column start)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Zigzag (column start) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB202), "MPF:PanOverlapH".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanOverlapH tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB203), "MPF:PanOverlapV".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PanOverlapV tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB204), "MPF:BaseViewpointNum".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BaseViewpointNum tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB205), "MPF:ConvergenceAngle".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ConvergenceAngle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB206), "MPF:BaselineLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BaselineLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB207), "MPF:VerticalDivergence".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VerticalDivergence tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB208), "MPF:AxisDistanceX".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AxisDistanceX tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB209), "MPF:AxisDistanceY".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AxisDistanceY tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB20A), "MPF:AxisDistanceZ".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AxisDistanceZ tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB20B), "MPF:YawAngle".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "YawAngle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB20C), "MPF:PitchAngle".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PitchAngle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0xB20D), "MPF:RollAngle".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RollAngle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPF:Dependent child image".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Dependent child image tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MPF:Dependent parent image".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Dependent parent image tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPF:Large Thumbnail (VGA equivalent)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Large Thumbnail (VGA equivalent) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPF:Large Thumbnail (full HD equivalent)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Large Thumbnail (full HD equivalent) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPF:Large Thumbnail (4K equivalent)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Large Thumbnail (4K equivalent) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MPF:Large Thumbnail (8K equivalent)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Large Thumbnail (8K equivalent) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "MPF:Large Thumbnail (16K equivalent)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Large Thumbnail (16K equivalent) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPF:Multi-frame Panorama".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Multi-frame Panorama tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPF:Multi-frame Disparity".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Multi-frame Disparity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "MPF:Multi-angle".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Multi-angle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "MPF:Baseline MP Primary Image".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Baseline MP Primary Image tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "MPF:MPImageLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MPImageLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "MPF:MPImageStart".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MPImageStart tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "MPF:DependentImage1EntryNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DependentImage1EntryNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "MPF:DependentImage2EntryNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DependentImage2EntryNumber tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "MPF:MPImageLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MPImageLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "MPF:MPImageType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MPImageType tag".to_string(), vec!["Example".to_string()]),
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
