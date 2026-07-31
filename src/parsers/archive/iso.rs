//! ISO 9660 filesystem image parser
//!
//! Implements comprehensive metadata extraction from ISO disc images including
//! volume descriptors, disc labels, dates, and file system information.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// ISO 9660 signature at offset 32769: "CD001"
const ISO_SIGNATURE: &[u8] = b"CD001";
const ISO_SIGNATURE_OFFSET: u64 = 32769;
/// ISO 9660 volume descriptors begin at sector 16 and occupy one sector each.
const VOLUME_DESCRIPTOR_START_OFFSET: u64 = 32768;
const VOLUME_DESCRIPTOR_SECTOR_SIZE: u64 = 2048;
const VOLUME_DESCRIPTOR_HEADER_SIZE: u64 = 7;

/// ISO parser for extracting metadata from ISO disc images
pub struct ISOParser;

impl ISOParser {
    /// Verifies ISO 9660 signature at offset 32769
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < ISO_SIGNATURE_OFFSET + 5 {
            return Ok(false);
        }

        let signature = reader.read(ISO_SIGNATURE_OFFSET, 5)?;
        Ok(signature == ISO_SIGNATURE)
    }

    /// Reads volume descriptor type (byte at offset 32768)
    pub fn read_descriptor_type(reader: &dyn FileReader) -> Result<u8> {
        if reader.size() < ISO_SIGNATURE_OFFSET {
            return Ok(0);
        }

        let descriptor = reader.read(VOLUME_DESCRIPTOR_START_OFFSET, 1)?;
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

    /// Walks the ISO 9660 volume descriptor sequence, extracting boot-record
    /// metadata and returning the first Primary Volume Descriptor offset.
    fn scan_volume_descriptors(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
    ) -> Result<Option<u64>> {
        let mut offset = VOLUME_DESCRIPTOR_START_OFFSET;
        let mut primary_volume_offset = None;

        while offset
            .checked_add(VOLUME_DESCRIPTOR_HEADER_SIZE)
            .is_some_and(|end| end <= reader.size())
        {
            let header = reader.read(offset, VOLUME_DESCRIPTOR_HEADER_SIZE as usize)?;
            if header.get(1..6) != Some(ISO_SIGNATURE) {
                break;
            }

            let Some(descriptor_type) = header.first().copied() else {
                break;
            };
            match descriptor_type {
                // ISO.pm BootRecord tag 7 is a fixed 32-byte string immediately
                // after the seven-byte volume descriptor header. ExifTool's
                // string format terminates at NUL before its ValueConv removes
                // trailing spaces.
                0 => {
                    let field_offset = offset + VOLUME_DESCRIPTOR_HEADER_SIZE;
                    if field_offset
                        .checked_add(32)
                        .is_some_and(|end| end <= reader.size())
                    {
                        let data = reader.read(field_offset, 32)?;
                        let string_bytes = match data.iter().position(|&byte| byte == 0) {
                            Some(end) => &data[..end],
                            None => data,
                        };
                        let value = String::from_utf8_lossy(string_bytes)
                            .trim_end_matches(' ')
                            .to_string();
                        if !value.is_empty() {
                            metadata.insert(
                                "ISO:BootSystem".to_string(),
                                TagValue::String(value),
                            );
                        }
                    }
                }
                1 if primary_volume_offset.is_none() => {
                    primary_volume_offset = Some(offset);
                }
                255 => break,
                _ => {}
            }

            let Some(next_offset) = offset.checked_add(VOLUME_DESCRIPTOR_SECTOR_SIZE) else {
                break;
            };
            offset = next_offset;
        }

        Ok(primary_volume_offset)
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

        // Reported as the raw count and size; VolumeSize is a Composite tag
        // (VolumeBlockCount * VolumeBlockSize), not a field on the descriptor.
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
        // Verify signature
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid ISO 9660 signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("ISO".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Descriptor type: 1=Primary, 2=Supplementary, 255=Terminator
        let descriptor_type = Self::read_descriptor_type(reader)?;
        metadata.insert(
            "VolumeDescriptorType".to_string(),
            TagValue::String(descriptor_type.to_string()),
        );

        // Boot records may precede the Primary Volume Descriptor, so walk the
        // descriptor sequence instead of treating sector 16 as the PVD.
        let primary_volume_offset = Self::scan_volume_descriptors(reader, &mut metadata)?;
        if let Some(pvd_offset) = primary_volume_offset {
            Self::extract_pvd_metadata(reader, &mut metadata, pvd_offset)?;
        }

        Ok(metadata)
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
    let parser = ISOParser;
    parser
        .parse(reader)
        .map_err(|e| format!("ISO parse error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_iso_signature() {
        let mut data = vec![0u8; 32774];
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
        // Create minimal ISO structure with PVD (need at least 33649 bytes for effective date)
        let mut data = vec![0u8; 33700];

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
}
