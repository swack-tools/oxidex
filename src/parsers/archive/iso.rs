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
/// Primary Volume Descriptor starts at sector 16 (offset 32768)
const PVD_OFFSET: u64 = 32768;
/// Sector size (ISO 9660)
const SECTOR_SIZE: u64 = 2048;

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

        let descriptor = reader.read(PVD_OFFSET, 1)?;
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

    /// Reads and inserts ISO date if valid
    fn insert_iso_date(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
        key: &str,
        offset: u64,
    ) -> Result<()> {
        let data = reader.read(offset, 17)?;
        if data.len() >= 17
            && !data[0..16].iter().all(|&b| b == b'0' || b == 0)
            && let (Ok(yr), Ok(mo), Ok(dy), Ok(hr), Ok(mi), Ok(se)) = (
                std::str::from_utf8(&data[0..4]),
                std::str::from_utf8(&data[4..6]),
                std::str::from_utf8(&data[6..8]),
                std::str::from_utf8(&data[8..10]),
                std::str::from_utf8(&data[10..12]),
                std::str::from_utf8(&data[12..14]),
            )
        {
            metadata.insert(
                key.to_string(),
                TagValue::String(format!("{}:{}:{} {}:{}:{}", yr, mo, dy, hr, mi, se)),
            );
        }
        Ok(())
    }

    /// Extracts BootSystem from a Boot Record descriptor at given sector offset.
    /// Returns Ok(true) if Boot Record found and data extracted, Ok(false) if not.
    fn extract_boot_system(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
        sector_offset: u64,
    ) -> Result<bool> {
        // Verify signature exists at this sector
        if reader.size() < sector_offset + (ISO_SIGNATURE_OFFSET - PVD_OFFSET) + 5 {
            return Ok(false);
        }
        let sig_offset = sector_offset + (ISO_SIGNATURE_OFFSET - PVD_OFFSET);
        let signature = reader.read(sig_offset, 5)?;
        if signature != ISO_SIGNATURE {
            return Ok(false);
        }
        // Check descriptor type == 0 (Boot Record)
        let desc_type = reader.read(sector_offset, 1)?;
        if desc_type[0] != 0 {
            return Ok(false);
        }
        // Read boot system identifier at offset 7 (string[32])
        let data = reader.read(sector_offset + 7, 32)?;
        let s = String::from_utf8_lossy(&data)
            .trim_end_matches(|c: char| c.is_whitespace() || c == '\0')
            .to_string();
        if !s.is_empty() {
            metadata.insert("BootSystem".to_string(), TagValue::String(s));
        }
        Ok(true)
    }

    /// Parses the 7-byte directory date/time and returns formatted string "YYYY:MM:DD HH:MM:SS+HH:MM"
    fn parse_dir_date(reader: &dyn FileReader, offset: u64) -> Result<Option<String>> {
        if reader.size() < offset + 7 {
            return Ok(None);
        }
        let data = reader.read(offset, 7)?;
        // All zeros means no date set
        if data.iter().all(|&b| b == 0) {
            return Ok(None);
        }
        let year_since_1900 = data[0] as u32;
        let month = data[1];
        let day = data[2];
        let hour = data[3];
        let minute = data[4];
        let second = data[5];
        let tz_raw = data[6] as i8; // signed, 15-minute intervals
        // Validate ranges (basic sanity)
        if month == 0 || month > 12 || day == 0 || day > 31 || hour > 23 || minute > 59 || second > 59 {
            return Ok(None);
        }
        let year = 1900 + year_since_1900;
        // Calculate timezone offset
        let tz_minutes = tz_raw as i32 * 15;
        let tz_hours = tz_minutes / 60;
        let tz_mins = (tz_minutes % 60).abs();
        let tz_sign = if tz_minutes < 0 { "-" } else { "+" };
        let tz_str = format!("{}{:02}:{:02}", tz_sign, tz_hours.abs(), tz_mins);
        Ok(Some(format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}",
            year, month, day, hour, minute, second, tz_str
        )))
    }

    /// Inserts RootDirectoryCreateDate from the root directory record in PVD
    fn insert_root_dir_date(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
        pvd_offset: u64,
    ) -> Result<()> {
        // Root directory record starts at offset 156 in PVD
        // Date field is at offset 18 within the directory record
        let date_offset = pvd_offset + 156 + 18;
        if let Some(date_str) = Self::parse_dir_date(reader, date_offset)? {
            metadata.insert(
                "RootDirectoryCreateDate".to_string(),
                TagValue::String(date_str),
            );
        }
        Ok(())
    }

    /// Extracts metadata from Primary Volume Descriptor
    fn extract_pvd_metadata(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        // String fields
        Self::insert_pvd_string(reader, metadata, "SystemID", PVD_OFFSET + 8, 32)?;
        Self::insert_pvd_string(reader, metadata, "VolumeID", PVD_OFFSET + 40, 32)?;
        Self::insert_pvd_string(reader, metadata, "VolumeSetID", PVD_OFFSET + 190, 128)?;
        Self::insert_pvd_string(reader, metadata, "PublisherID", PVD_OFFSET + 318, 128)?;
        Self::insert_pvd_string(reader, metadata, "DataPreparerID", PVD_OFFSET + 446, 128)?;
        Self::insert_pvd_string(reader, metadata, "ApplicationID", PVD_OFFSET + 574, 128)?;
        Self::insert_pvd_string(reader, metadata, "Publisher", PVD_OFFSET + 318, 128)?;
        Self::insert_pvd_string(reader, metadata, "DataPreparer", PVD_OFFSET + 446, 128)?;
        Self::insert_pvd_string(reader, metadata, "Software", PVD_OFFSET + 574, 128)?;
        Self::insert_pvd_string(reader, metadata, "CopyrightFileName", PVD_OFFSET + 702, 38)?;

        // Volume size calculation
        let volume_sectors = Self::read_u32_both(reader, PVD_OFFSET + 80)?;
        let block_size = Self::read_u32_both(reader, PVD_OFFSET + 128)?;
        metadata.insert(
            "BlockSize".to_string(),
            TagValue::String(block_size.to_string()),
        );
        metadata.insert(
            "VolumeSize".to_string(),
            TagValue::String((volume_sectors as u64 * block_size as u64).to_string()),
        );

        // Date fields
        Self::insert_iso_date(reader, metadata, "CreationDate", PVD_OFFSET + 813)?;
        Self::insert_iso_date(reader, metadata, "ModificationDate", PVD_OFFSET + 830)?;
        Self::insert_iso_date(reader, metadata, "ExpirationDate", PVD_OFFSET + 847)?;
        Self::insert_iso_date(reader, metadata, "EffectiveDate", PVD_OFFSET + 864)?;

        // Root directory date
        Self::insert_root_dir_date(reader, metadata, PVD_OFFSET)?;

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

        // Extract Primary Volume Descriptor metadata
        Self::extract_pvd_metadata(reader, &mut metadata)?;

        // Look for Boot Record to extract BootSystem
        // Check first sector (16) for Boot Record
        if !Self::extract_boot_system(reader, &mut metadata, PVD_OFFSET)? {
            // If not found at sector 16, try sector 17 (common for El Torito boot+primary pair)
            let sector17 = PVD_OFFSET + SECTOR_SIZE;
            if reader.size() > sector17 + 2000 {
                Self::extract_boot_system(reader, &mut metadata, sector17)?;
            }
            // Optionally continue scanning up to a limit (e.g., 64 sectors) to be thorough
            for i in 2..64 {
                let offset = PVD_OFFSET + SECTOR_SIZE * i;
                if reader.size() < offset + 2048 {
                    break;
                }
                if Self::extract_boot_system(reader, &mut metadata, offset)? {
                    break;
                }
            }
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
        assert_eq!(
            metadata.get("TestDate").unwrap(),
            &TagValue::String("2024:03:15 14:30:45".to_string())
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

        // Creation date at offset 813 (17 bytes: YYYYMMDDHHMMSSCC + timezone)
        data[33581..33598].copy_from_slice(b"20240315143045000");

        let reader = TestReader::new(data);
        let parser = ISOParser;
        let metadata = parser.parse(&reader).unwrap();

        // Verify extracted metadata
        assert_eq!(
            metadata.get("VolumeID").unwrap(),
            &TagValue::String("TEST_DISC_VOLUME".to_string())
        );
        assert_eq!(
            metadata.get("SystemID").unwrap(),
            &TagValue::String("LINUX".to_string())
        );
        assert_eq!(
            metadata.get("BlockSize").unwrap(),
            &TagValue::String("2048".to_string())
        );
        // Volume size = 10000 sectors * 2048 bytes
        assert_eq!(
            metadata.get("VolumeSize").unwrap(),
            &TagValue::String("20480000".to_string())
        );
        assert_eq!(
            metadata.get("PublisherID").unwrap(),
            &TagValue::String("TEST PUBLISHER".to_string())
        );
        assert_eq!(
            metadata.get("ApplicationID").unwrap(),
            &TagValue::String("MKISOFS".to_string())
        );
        assert_eq!(
            metadata.get("CreationDate").unwrap(),
            &TagValue::String("2024:03:15 14:30:45".to_string())
        );
    }
}
