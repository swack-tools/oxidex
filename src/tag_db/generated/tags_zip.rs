//! ZIP format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0002), "ZIP:ZipRequiredVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZipRequiredVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ZIP:ZipBitFlag".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZipBitFlag tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ZIP:ZipCompression".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZipCompression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ZIP:Shrunk".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Shrunk tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ZIP:Reduced with compression factor 1".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Reduced with compression factor 1 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ZIP:Reduced with compression factor 2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Reduced with compression factor 2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ZIP:Reduced with compression factor 3".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Reduced with compression factor 3 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "ZIP:Reduced with compression factor 4".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Reduced with compression factor 4 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "ZIP:Imploded".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Imploded tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "ZIP:Tokenized".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Tokenized tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "ZIP:Deflated".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Deflated tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "ZIP:Enhanced Deflate using Deflate64(tm)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Enhanced Deflate using Deflate64(tm) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "ZIP:Imploded (old IBM TERSE)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Imploded (old IBM TERSE) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "ZIP:BZIP2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BZIP2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000E), "ZIP:LZMA (EFS)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LZMA (EFS) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "ZIP:IBM TERSE (new)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IBM TERSE (new) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "ZIP:IBM LZ77 z Architecture (PFS)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "IBM LZ77 z Architecture (PFS) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "ZIP:ZipModifyDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZipModifyDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "ZIP:ZipCRC".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ZipCRC tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "ZIP:ZipCompressedSize".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ZipCompressedSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "ZIP:ZipUncompressedSize".to_string(), FormatFamily::MakerNotes, false, ValueType::Integer, "ZipUncompressedSize tag".to_string(), vec!["100".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "ZIP:ZipFileNameLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZipFileNameLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "ZIP:ZipFileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ZipFileName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ZIP:Compression".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Compression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ZIP:Flags".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Flags tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ZIP:CRC16".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CRC16 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ZIP:ExtraFields".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExtraFields tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ZIP:FileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FileName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ZIP:Comment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Comment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ZIP:ModifyDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ModifyDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "ZIP:ExtraFlags".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ExtraFlags tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ZIP:Maximum Compression".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Maximum Compression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ZIP:Fastest Algorithm".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Fastest Algorithm tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "ZIP:OperatingSystem".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OperatingSystem tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ZIP:Amiga".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Amiga tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ZIP:VMS (or OpenVMS)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VMS (or OpenVMS) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ZIP:Unix".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Unix tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ZIP:VM/CMS".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "VM/CMS tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "ZIP:Atari TOS".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Atari TOS tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "ZIP:HPFS filesystem (OS/2, NT)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "HPFS filesystem (OS/2, NT) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "ZIP:Macintosh".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Macintosh tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "ZIP:Z-System".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Z-System tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0009), "ZIP:CP/M".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CP/M tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "ZIP:TOPS-20".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "TOPS-20 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "ZIP:NTFS filesystem (NT)".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "NTFS filesystem (NT) tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "ZIP:QDOS".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "QDOS tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "ZIP:Acorn RISCOS".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Acorn RISCOS tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x00FF), "ZIP:unknown".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "unknown tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "ZIP:ArchivedFileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ArchivedFileName tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "ZIP:Comment".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Comment tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0000), "ZIP:CompressedSize".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CompressedSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "ZIP:UncompressedSize".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "UncompressedSize tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "ZIP:OperatingSystem".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OperatingSystem tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "ZIP:OS/2".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OS/2 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "ZIP:Win32".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Win32 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "ZIP:Unix".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Unix tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000D), "ZIP:ModifyDate".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ModifyDate tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0012), "ZIP:PackingMethod".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PackingMethod tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0031), "ZIP:Fastest".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Fastest tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0032), "ZIP:Fast".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Fast tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0033), "ZIP:Normal".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Normal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0034), "ZIP:Good Compression".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Good Compression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0035), "ZIP:Best Compression".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Best Compression tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0013), "ZIP:FileNameLength".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FileNameLength tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0019), "ZIP:ArchivedFileName".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ArchivedFileName tag".to_string(), vec!["Example".to_string()]),
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
