//! ISO 9660 filesystem image parser
//!
//! Implements comprehensive metadata extraction from ISO disc images including
//! volume descriptors, disc labels, dates, and file system information.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{Dir, cond::Ctx, find_table, process_binary_data};
use crate::io::ByteOrder;
use crate::io::EndianReader;
use std::collections::HashMap;

/// ISO 9660 volume descriptor signature: "CD001"
const ISO_SIGNATURE: &[u8] = b"CD001";
/// Volume descriptors begin at sector 16 (offset 32768). The primary
/// descriptor can follow a boot record or another descriptor.
const DESCRIPTOR_START: u64 = 32768;

/// Size of one ISO 9660 volume descriptor.
const DESCRIPTOR_SIZE: u64 = 2048;

/// Descriptor type byte: a boot record.
const DESCRIPTOR_BOOT_RECORD: u8 = 0;

/// Descriptor type byte: the set terminator.
const DESCRIPTOR_TERMINATOR: u8 = 255;

/// ISO parser for extracting metadata from ISO disc images
pub struct ISOParser;

impl ISOParser {
    /// Verifies ISO 9660 signature at offset 32769
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        // Native ProcessISO does not accept a partial descriptor, even if
        // its CD001 header and some table fields fit in the available bytes.
        if reader.size() < DESCRIPTOR_START + DESCRIPTOR_SIZE {
            return Ok(false);
        }

        let header = reader.read(DESCRIPTOR_START, 6)?;
        Ok(matches!(header[0], 0..=3 | DESCRIPTOR_TERMINATOR)
            && header.get(1..6) == Some(ISO_SIGNATURE))
    }

    /// Reads volume descriptor type (byte at offset 32768)
    pub fn read_descriptor_type(reader: &dyn FileReader) -> Result<u8> {
        if reader.size() <= DESCRIPTOR_START {
            return Ok(0);
        }

        let descriptor = reader.read(DESCRIPTOR_START, 1)?;
        Ok(descriptor[0])
    }

    /// Reads a string field from the PVD and inserts into metadata if non-empty
    fn insert_pvd_string(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
        key: &str,
        offset: u64,
        length: usize,
    ) -> Result<()> {
        let data = reader.read(offset, length)?;
        let s = String::from_utf8_lossy(data)
            .trim_end_matches(|c: char| c.is_whitespace() || c == '\0')
            .to_string();
        if !s.is_empty() {
            metadata.insert(key.to_string(), TagValue::String(s));
        }
        Ok(())
    }

    /// Reads both-endian format (LSB then MSB, 8 bytes total), returns LSB value
    fn read_u32_both(reader: &dyn FileReader, offset: u64) -> Result<u32> {
        let data = reader.read(offset, 8)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u32_at(0).unwrap_or(0))
    }

    /// Reads the little-endian half of a both-endian 16-bit field (4 bytes).
    ///
    /// `VolumeBlockSize` is `int16u` in ExifTool's table, not `int32u`.
    /// Reading four bytes swallows the big-endian twin that follows and
    /// reports 526336 where the block size is 2048.
    fn read_u16_both(reader: &dyn FileReader, offset: u64) -> Result<u16> {
        let data = reader.read(offset, 4)?;
        let r = EndianReader::little_endian(data);
        Ok(r.u16_at(0).unwrap_or(0))
    }

    /// Reads the 7-byte binary directory timestamp at PVD offset 174.
    ///
    /// Unlike the 17-byte ASCII volume dates, this one is packed binary:
    /// year-since-1900, month, day, hour, minute, second, then a signed
    /// quarter-hour UTC offset (ExifTool ISO.pm, `RootDirectoryCreateDate`).
    fn insert_directory_date(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
        key: &str,
        offset: u64,
    ) -> Result<()> {
        let d = reader.read(offset, 7)?;
        if d.len() < 7 || d[..6].iter().all(|&b| b == 0) {
            return Ok(());
        }
        let tz = d[6] as i8 as i32 * 15;
        let (sign, tz) = if tz < 0 { ('-', -tz) } else { ('+', tz) };
        metadata.insert(
            key.to_string(),
            TagValue::String(format!(
                "{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}{:02}:{:02}",
                1900 + d[0] as u32,
                d[1],
                d[2],
                d[3],
                d[4],
                d[5],
                sign,
                tz / 60,
                tz % 60,
            )),
        );
        Ok(())
    }

    /// Reads and inserts ISO date if valid
    fn insert_iso_date(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
        key: &str,
        offset: u64,
    ) -> Result<()> {
        let data = reader.read(offset, 17)?;
        // 16 ASCII digits then one signed byte of quarter-hour UTC offset.
        // The hundredths and the offset are part of the value ExifTool
        // prints (`2016:01:08 10:00:26.00+00:00`); dropping them turned two
        // correct dates into value mismatches.
        if data.len() >= 17
            && !data[0..16].iter().all(|&b| b == b'0' || b == 0)
            && let (Ok(yr), Ok(mo), Ok(dy), Ok(hr), Ok(mi), Ok(se), Ok(cs)) = (
                std::str::from_utf8(&data[0..4]),
                std::str::from_utf8(&data[4..6]),
                std::str::from_utf8(&data[6..8]),
                std::str::from_utf8(&data[8..10]),
                std::str::from_utf8(&data[10..12]),
                std::str::from_utf8(&data[12..14]),
                std::str::from_utf8(&data[14..16]),
            )
        {
            let tz = data[16] as i8 as i32 * 15;
            let (sign, tz) = if tz < 0 { ('-', -tz) } else { ('+', tz) };
            metadata.insert(
                key.to_string(),
                TagValue::String(format!(
                    "{}:{}:{} {}:{}:{}.{}{}{:02}:{:02}",
                    yr,
                    mo,
                    dy,
                    hr,
                    mi,
                    se,
                    cs,
                    sign,
                    tz / 60,
                    tz % 60
                )),
            );
        }
        Ok(())
    }

    /// Walks complete descriptors as `ProcessISO` does in pinned ISO.pm.
    /// Returns primary descriptor offsets in file order. Pinned `ProcessISO`
    /// processes each one; later defined fields can replace earlier values.
    fn find_primary_descriptors(reader: &dyn FileReader) -> Result<Vec<u64>> {
        let mut offset = DESCRIPTOR_START;
        let mut primary_offsets = Vec::new();
        while offset
            .checked_add(DESCRIPTOR_SIZE)
            .is_some_and(|end| end <= reader.size())
        {
            let head = reader.read(offset, 6)?;
            if head.get(1..6) != Some(ISO_SIGNATURE) {
                break;
            }
            match head[0] {
                1 => primary_offsets.push(offset),
                DESCRIPTOR_TERMINATOR => break,
                0 | 2 | 3 => {}
                _ => break,
            }
            offset += DESCRIPTOR_SIZE;
        }
        Ok(primary_offsets)
    }

    /// Reads `BootSystem` from the source-described boot record, if present.
    ///
    /// The descriptors form a chain of 2048-byte sectors from 32768, each
    /// tagged by a leading type byte, and the boot record is NOT first --
    /// this image has the primary volume at 32768 and the boot record at
    /// 34816. Reading a fixed offset finds only whichever descriptor happens
    /// to lead, which is why BootSystem was the one tag #184 left behind.
    fn extract_boot_record(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        let Some(table) = find_table("ISO", "BootRecord").filter(|table| table.enabled()) else {
            return Ok(());
        };
        let mut offset = DESCRIPTOR_START;
        // Earlier descriptors belong to ExifTool's Copy1 group. This map
        // cannot represent that family-4 identity, so retain only the final
        // ISO-group winner instead of reporting an earlier copy as ISO.
        let mut boot_system = None;
        while offset
            .checked_add(DESCRIPTOR_SIZE)
            .is_some_and(|end| end <= reader.size())
        {
            let head = reader.read(offset, 6)?;
            if head.get(1..6) != Some(ISO_SIGNATURE) {
                break;
            }
            match head[0] {
                DESCRIPTOR_TERMINATOR => break,
                DESCRIPTOR_BOOT_RECORD => {
                    let descriptor = reader.read(offset, DESCRIPTOR_SIZE as usize)?;
                    let mut members = HashMap::new();
                    let mut ctx = Ctx::new(&mut members);
                    let mut emitted = Vec::new();
                    process_binary_data(
                        table,
                        Dir::whole(descriptor, ByteOrder::Little),
                        &mut ctx,
                        &mut emitted,
                    );
                    for tag in emitted {
                        if tag.name == "BootSystem" {
                            boot_system = Some(tag.value);
                        }
                    }
                }
                1 | 2 | 3 => {}
                _ => break,
            }
            offset += DESCRIPTOR_SIZE;
        }
        if let Some(value) = boot_system {
            metadata.insert("ISO:BootSystem", value);
        }
        Ok(())
    }

    /// Extracts metadata from Primary Volume Descriptor
    fn extract_pvd_metadata(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
        pvd_offset: u64,
    ) -> Result<()> {
        // Offsets and names are ExifTool's `ISO::PrimaryVolume` table verbatim,
        // and the names matter as much as the offsets: this parser previously
        // read every field from the right place and then filed it under a name
        // of its own invention (VolumeID, PublisherID, ApplicationID), so not
        // one of the 14 tags ExifTool reports could ever match.
        Self::insert_pvd_string(reader, metadata, "ISO:System", pvd_offset + 8, 32)?;
        Self::insert_pvd_string(reader, metadata, "ISO:VolumeName", pvd_offset + 40, 32)?;
        Self::insert_pvd_string(reader, metadata, "ISO:VolumeSetName", pvd_offset + 190, 128)?;
        Self::insert_pvd_string(reader, metadata, "ISO:Publisher", pvd_offset + 318, 128)?;
        Self::insert_pvd_string(reader, metadata, "ISO:DataPreparer", pvd_offset + 446, 128)?;
        Self::insert_pvd_string(reader, metadata, "ISO:Software", pvd_offset + 574, 128)?;
        Self::insert_pvd_string(
            reader,
            metadata,
            "ISO:CopyrightFileName",
            pvd_offset + 702,
            38,
        )?;
        Self::insert_pvd_string(
            reader,
            metadata,
            "ISO:AbstractFileName",
            pvd_offset + 740,
            36,
        )?;
        Self::insert_pvd_string(
            reader,
            metadata,
            // ExifTool's spelling, typo included -- a "corrected" name is a
            // name that does not match.
            "ISO:BibligraphicFileName",
            pvd_offset + 776,
            37,
        )?;

        // Reported as the raw count and size only. VolumeSize is a Composite
        // tag (VolumeBlockCount * VolumeBlockSize, ISO.pm:119-126), not a
        // field on the descriptor: emitting it here too put it under `ISO:`
        // where the pinned oracle puts it under `Composite:`, and left
        // `ISO::VolumeSize` in codegen_composite.py's "never fire" list
        // because nothing computed it. `composite::compute` computes it now.
        let block_count = Self::read_u32_both(reader, pvd_offset + 80)?;
        let block_size = Self::read_u16_both(reader, pvd_offset + 128)?;
        metadata.insert(
            "ISO:VolumeBlockCount".to_string(),
            TagValue::String(block_count.to_string()),
        );
        metadata.insert(
            "ISO:VolumeBlockSize".to_string(),
            TagValue::String(block_size.to_string()),
        );
        Self::insert_directory_date(
            reader,
            metadata,
            "ISO:RootDirectoryCreateDate",
            pvd_offset + 174,
        )?;
        Self::insert_iso_date(reader, metadata, "ISO:VolumeCreateDate", pvd_offset + 813)?;
        Self::insert_iso_date(reader, metadata, "ISO:VolumeModifyDate", pvd_offset + 830)?;
        Self::insert_iso_date(
            reader,
            metadata,
            "ISO:VolumeExpirationDate",
            pvd_offset + 847,
        )?;
        Self::insert_iso_date(
            reader,
            metadata,
            "ISO:VolumeEffectiveDate",
            pvd_offset + 864,
        )?;

        Ok(())
    }
}

impl FormatParser for ISOParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Every row here is read from the file (`metadata_map::file_rows`):
        // a caller's later `insert`/`get_mut` is what counts as assigned.
        crate::core::metadata_map::file_rows(|| -> Result<MetadataMap> {
            // Verify signature
            if !Self::verify_signature(reader)? {
                return Err(ExifToolError::parse_error("Invalid ISO 9660 signature"));
            }

            let mut metadata = MetadataMap::new();

            metadata.insert("FileType".to_string(), TagValue::String("ISO".to_string()));

            // Descriptor type: 1=Primary, 2=Supplementary, 255=Terminator
            let descriptor_type = Self::read_descriptor_type(reader)?;
            metadata.insert(
                "VolumeDescriptorType".to_string(),
                TagValue::String(descriptor_type.to_string()),
            );

            // Process every primary descriptor: a later defined field replaces
            // its predecessor, while an absent field leaves the earlier value.
            // Publish only the ISO-group winners; earlier physical copies have
            // a Copy1 family-4 identity this map cannot represent.
            let mut primary_values = MetadataMap::new();
            for pvd_offset in Self::find_primary_descriptors(reader)? {
                Self::extract_pvd_metadata(reader, &mut primary_values, pvd_offset)?;
            }
            for (key, occurrence) in primary_values.winners_in_file_order() {
                metadata.insert(key.clone(), occurrence.raw.clone());
            }

            // The boot record lives in a later descriptor sector, if at all.
            Self::extract_boot_record(reader, &mut metadata)?;

            Ok(metadata)
        })
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::ISO)
    }
}

/// Standalone function for parsing ISO metadata
///
/// This function provides a convenient interface for parsing ISO 9660 disc image metadata
/// by instantiating the ISOParser and calling its parse method.
///
/// # Arguments
///
/// * `reader` - A FileReader providing access to the ISO file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error description
pub fn parse_iso_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    // Every row here is read from the file (`metadata_map::file_rows`):
    // a caller's later `insert`/`get_mut` is what counts as assigned.
    crate::core::metadata_map::file_rows(|| -> std::result::Result<MetadataMap, String> {
        let parser = ISOParser;
        parser
            .parse(reader)
            .map_err(|e| format!("ISO parse error: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_iso_signature() {
        let mut data = vec![0u8; (DESCRIPTOR_START + DESCRIPTOR_SIZE) as usize];
        data[32768] = 0x01; // Primary volume descriptor
        data[32769..32774].copy_from_slice(b"CD001");
        let reader = TestReader::new(data);
        assert!(ISOParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_parse_iso_date() {
        // Valid date: 2024:03:15 14:30:45
        let mut data = vec![0u8; 32800];
        data[32768..32785].copy_from_slice(b"2024031514304500\x00");
        let reader = TestReader::new(data);
        let mut metadata = MetadataMap::new();
        ISOParser::insert_iso_date(&reader, &mut metadata, "TestDate", 32768).unwrap();
        // Hundredths and UTC offset included, matching what ExifTool prints.
        assert_eq!(
            metadata.get("TestDate").unwrap(),
            &TagValue::String("2024:03:15 14:30:45.00+00:00".to_string())
        );

        // All zeros (unset date) should not insert any value
        let mut data2 = vec![0u8; 32800];
        data2[32768..32785].copy_from_slice(b"0000000000000000\x00");
        let reader2 = TestReader::new(data2);
        let mut metadata2 = MetadataMap::new();
        ISOParser::insert_iso_date(&reader2, &mut metadata2, "TestDate", 32768).unwrap();
        assert!(!metadata2.contains_key("TestDate"));
    }

    #[test]
    fn test_pvd_metadata_extraction() {
        // ProcessISO requires a complete 2048-byte descriptor sector.
        let mut data = vec![0u8; (DESCRIPTOR_START + DESCRIPTOR_SIZE) as usize];

        // PVD header
        data[32768] = 0x01; // Primary volume descriptor
        data[32769..32774].copy_from_slice(b"CD001");

        // Volume ID at offset 40 (32 bytes)
        data[32808..32824].copy_from_slice(b"TEST_DISC_VOLUME");

        // System ID at offset 8 (32 bytes)
        data[32776..32781].copy_from_slice(b"LINUX");

        // Volume Space Size at offset 80 (both-endian format)
        // 10000 sectors in LSB format
        data[32848..32852].copy_from_slice(&10000u32.to_le_bytes());
        data[32852..32856].copy_from_slice(&10000u32.to_be_bytes());

        // Block Size at offset 128 (both-endian format)
        // 2048 bytes
        data[32896..32900].copy_from_slice(&2048u32.to_le_bytes());
        data[32900..32904].copy_from_slice(&2048u32.to_be_bytes());

        // Publisher ID at offset 318 (128 bytes)
        data[33086..33100].copy_from_slice(b"TEST PUBLISHER");

        // Application ID at offset 574 (128 bytes)
        data[33342..33349].copy_from_slice(b"MKISOFS");

        // Creation date at offset 813: 16 ASCII digits (YYYYMMDDHHMMSSCC)
        // then ONE BINARY byte of UTC offset in 15-minute units. An ASCII
        // '0' here is 48, i.e. +12:00 -- which is what this fixture used to
        // say while claiming to mean UTC.
        data[33581..33597].copy_from_slice(b"2024031514304500");
        data[33597] = 0;

        let reader = TestReader::new(data);
        let parser = ISOParser;
        let metadata = parser.parse(&reader).unwrap();

        // Names are ExifTool's, and carry the ISO family prefix: the old
        // VolumeID / PublisherID / ApplicationID spellings read the right
        // bytes under names ExifTool never emits, so they matched nothing.
        assert_eq!(
            metadata.get("ISO:VolumeName").unwrap(),
            &TagValue::String("TEST_DISC_VOLUME".to_string())
        );
        assert_eq!(
            metadata.get("ISO:System").unwrap(),
            &TagValue::String("LINUX".to_string())
        );
        // int16u, so the big-endian twin at +2 must not be read into it.
        assert_eq!(
            metadata.get("ISO:VolumeBlockSize").unwrap(),
            &TagValue::String("2048".to_string())
        );
        // The raw count; VolumeSize is a Composite of count * size upstream.
        assert_eq!(
            metadata.get("ISO:VolumeBlockCount").unwrap(),
            &TagValue::String("10000".to_string())
        );
        assert_eq!(
            metadata.get("ISO:Publisher").unwrap(),
            &TagValue::String("TEST PUBLISHER".to_string())
        );
        assert_eq!(
            metadata.get("ISO:Software").unwrap(),
            &TagValue::String("MKISOFS".to_string())
        );
        assert_eq!(
            metadata.get("ISO:VolumeCreateDate").unwrap(),
            &TagValue::String("2024:03:15 14:30:45.00+00:00".to_string())
        );
    }

    fn descriptor_image(types: &[u8]) -> Vec<u8> {
        let mut data = vec![0; DESCRIPTOR_START as usize + types.len() * DESCRIPTOR_SIZE as usize];
        for (index, &kind) in types.iter().enumerate() {
            let offset = DESCRIPTOR_START as usize + index * DESCRIPTOR_SIZE as usize;
            data[offset] = kind;
            data[offset + 1..offset + 6].copy_from_slice(ISO_SIGNATURE);
            data[offset + 6] = 1;
        }
        data
    }

    #[test]
    fn boot_before_primary_uses_primary_relative_offsets() {
        // The pinned native comparison in descriptor-order-probe.json reports
        // the PVD values from sector 17, after a boot record at sector 16.
        let mut data = descriptor_image(&[0, 1, 2, DESCRIPTOR_TERMINATOR]);
        let boot = DESCRIPTOR_START as usize;
        data[boot + 7..boot + 11].copy_from_slice(b"BOOT");
        let primary = boot + DESCRIPTOR_SIZE as usize;
        data[primary + 40..primary + 44].copy_from_slice(b"DISC");
        data[primary + 80..primary + 84].copy_from_slice(&1234u32.to_le_bytes());
        data[primary + 128..primary + 130].copy_from_slice(&2048u16.to_le_bytes());

        let metadata = ISOParser.parse(&TestReader::new(data)).unwrap();
        assert_eq!(
            metadata.get("ISO:BootSystem"),
            Some(&TagValue::String("BOOT".into()))
        );
        assert_eq!(
            metadata.get("ISO:VolumeName"),
            Some(&TagValue::String("DISC".into()))
        );
        assert_eq!(
            metadata.get("ISO:VolumeBlockCount"),
            Some(&TagValue::String("1234".into()))
        );
        assert_eq!(
            metadata.get("ISO:VolumeBlockSize"),
            Some(&TagValue::String("2048".into()))
        );
    }

    #[test]
    fn boot_system_stops_at_embedded_nul() {
        // boot-record-probe.json: native ISO:BootSystem is "EL TORITO";
        // the current parser incorrectly appends "GARBAGE" after the NUL.
        let mut data = descriptor_image(&[1, 0, DESCRIPTOR_TERMINATOR]);
        let boot = DESCRIPTOR_START as usize + DESCRIPTOR_SIZE as usize;
        data[boot + 7..boot + 24].copy_from_slice(b"EL TORITO\0GARBAGE");
        let metadata = ISOParser.parse(&TestReader::new(data)).unwrap();
        assert_eq!(
            metadata.get("ISO:BootSystem"),
            Some(&TagValue::String("EL TORITO".into()))
        );
    }

    #[test]
    fn boot_system_preserves_empty_and_perl_final_newline() {
        let empty = descriptor_image(&[1, 0, DESCRIPTOR_TERMINATOR]);
        let metadata = ISOParser.parse(&TestReader::new(empty.clone())).unwrap();
        assert_eq!(
            metadata.get("ISO:BootSystem"),
            Some(&TagValue::String(String::new()))
        );

        let mut newline = empty;
        let boot = DESCRIPTOR_START as usize + DESCRIPTOR_SIZE as usize;
        newline[boot + 7..boot + 13].copy_from_slice(b"ABC  \n");
        let metadata = ISOParser.parse(&TestReader::new(newline)).unwrap();
        assert_eq!(
            metadata.get("ISO:BootSystem"),
            Some(&TagValue::String("ABC\n".into()))
        );
    }

    #[test]
    fn truncated_boot_sector_does_not_emit_boot_system() {
        // `ProcessISO` reads the full 2048-byte sector before dispatching.
        let mut data = descriptor_image(&[1, 0]);
        let boot = DESCRIPTOR_START as usize + DESCRIPTOR_SIZE as usize;
        data[boot + 7..boot + 11].copy_from_slice(b"BOOT");
        data.truncate(boot + 7 + 32);
        let metadata = ISOParser.parse(&TestReader::new(data)).unwrap();
        assert!(!metadata.contains_key("ISO:BootSystem"));
    }

    #[test]
    fn first_descriptor_requires_complete_sector_and_known_type() {
        let mut partial = descriptor_image(&[1]);
        partial.truncate(DESCRIPTOR_START as usize + 900);
        assert!(!ISOParser::verify_signature(&TestReader::new(partial)).unwrap());

        let unknown = descriptor_image(&[4, 1, DESCRIPTOR_TERMINATOR]);
        assert!(!ISOParser::verify_signature(&TestReader::new(unknown)).unwrap());
    }

    #[test]
    fn later_primary_descriptor_replaces_earlier_value() {
        // Pinned ISO.pm processes every PVD in order. A native probe of
        // repeated-primary.iso reports "SECOND PRIMARY" from the later one.
        let mut data = descriptor_image(&[1, 1, DESCRIPTOR_TERMINATOR]);
        let first = DESCRIPTOR_START as usize;
        let second = first + DESCRIPTOR_SIZE as usize;
        data[first + 40..first + 45].copy_from_slice(b"FIRST");
        data[second + 40..second + 46].copy_from_slice(b"SECOND");
        let metadata = ISOParser.parse(&TestReader::new(data)).unwrap();
        assert_eq!(
            metadata.get("ISO:VolumeName"),
            Some(&TagValue::String("SECOND".into()))
        );
        assert_eq!(
            metadata
                .occurrences()
                .filter(|row| row.name.as_ref() == "VolumeName")
                .count(),
            1,
            "earlier descriptor is a Copy1 group, not another ISO occurrence"
        );
    }

    #[test]
    fn later_boot_descriptor_replaces_earlier_value() {
        // native-additional-probes.json reports the later BootSystem value.
        let mut data = descriptor_image(&[1, 0, 0, DESCRIPTOR_TERMINATOR]);
        let first = DESCRIPTOR_START as usize + DESCRIPTOR_SIZE as usize;
        let second = first + DESCRIPTOR_SIZE as usize;
        data[first + 7..first + 12].copy_from_slice(b"FIRST");
        data[second + 7..second + 13].copy_from_slice(b"SECOND");
        let metadata = ISOParser.parse(&TestReader::new(data)).unwrap();
        assert_eq!(
            metadata.get("ISO:BootSystem"),
            Some(&TagValue::String("SECOND".into()))
        );
        assert_eq!(
            metadata
                .occurrences()
                .filter(|row| row.name.as_ref() == "BootSystem")
                .count(),
            1,
            "earlier descriptor is a Copy1 group, not another ISO occurrence"
        );
    }
}
