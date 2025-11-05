//! ISO format family tags (auto-generated)

use crate::core::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0000), "ISO:BootRecord".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BootRecord tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ISO:PrimaryVolume".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PrimaryVolume tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "ISO:BootSystem".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BootSystem tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0027), "ISO:BootIdentifier".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BootIdentifier tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "ISO:System".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "System tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0028), "ISO:VolumeName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VolumeName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0050), "ISO:VolumeBlockCount".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "VolumeBlockCount tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0078), "ISO:VolumeSetDiskCount".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "VolumeSetDiskCount tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x007C), "ISO:VolumeSetDiskNumber".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "VolumeSetDiskNumber tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0080), "ISO:VolumeBlockSize".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "VolumeBlockSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0084), "ISO:PathTableSize".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "PathTableSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x008C), "ISO:PathTableLocation".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "PathTableLocation tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00AE), "ISO:RootDirectoryCreateDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RootDirectoryCreateDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00BE), "ISO:VolumeSetName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VolumeSetName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x013E), "ISO:Publisher".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Publisher tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x01BE), "ISO:DataPreparer".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "DataPreparer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x023E), "ISO:Software".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Software tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x02BE), "ISO:CopyrightFileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CopyrightFileName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x02E4), "ISO:AbstractFileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AbstractFileName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0308), "ISO:BibligraphicFileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BibligraphicFileName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x032D), "ISO:VolumeCreateDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VolumeCreateDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x033E), "ISO:VolumeModifyDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VolumeModifyDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x034F), "ISO:VolumeExpirationDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VolumeExpirationDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0360), "ISO:VolumeEffectiveDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VolumeEffectiveDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ISO:ISO:VolumeBlockSize".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ISO:VolumeBlockSize tag".to_string(), vec!["Example".to_string()]),
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
