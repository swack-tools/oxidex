//! FlashPix metadata parser
//!
//! FlashPix is an image file format based on Microsoft's OLE (Object Linking and
//! Embedding) Compound File Binary Format. This module extracts metadata from
//! FlashPix property sets.
//!
//! # Format Structure
//!
//! FlashPix files contain:
//! - CompObj stream: Component Object information
//! - SummaryInformation: Basic document properties (Title, Author, etc.)
//! - DocumentSummaryInformation: Extended document properties
//! - Image properties: Resolution, color space, etc.

use crate::core::{MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// Property set identifiers (FMTID)
const FMTID_SUMMARY_INFO: [u8; 16] = [
    0xE0, 0x85, 0x9F, 0xF2, 0xF9, 0x4F, 0x68, 0x10,
    0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3, 0xD9,
];

const FMTID_DOC_SUMMARY_INFO: [u8; 16] = [
    0x02, 0xD5, 0xCD, 0xD5, 0x9C, 0x2E, 0x1B, 0x10,
    0x93, 0x97, 0x08, 0x00, 0x2B, 0x2C, 0xF9, 0xAE,
];

/// Property IDs for SummaryInformation
const PID_CODEPAGE: u32 = 0x01;
const PID_TITLE: u32 = 0x02;
const PID_SUBJECT: u32 = 0x03;
const PID_AUTHOR: u32 = 0x04;
const PID_KEYWORDS: u32 = 0x05;
const PID_COMMENTS: u32 = 0x06;
const PID_TEMPLATE: u32 = 0x07;
const PID_LASTAUTHOR: u32 = 0x08;
const PID_REVNUMBER: u32 = 0x09;
const PID_EDITTIME: u32 = 0x0A;
const PID_LASTPRINTED: u32 = 0x0B;
const PID_CREATE_DTM: u32 = 0x0C;
const PID_LASTSAVE_DTM: u32 = 0x0D;
const PID_PAGECOUNT: u32 = 0x0E;

/// Property IDs for DocumentSummaryInformation
const PID_COMPANY: u32 = 0x0F;
const PID_MANAGER: u32 = 0x0E;
const PID_CATEGORY: u32 = 0x02;

/// Variable Types (VT)
const VT_EMPTY: u16 = 0x0000;
const VT_NULL: u16 = 0x0001;
const VT_I2: u16 = 0x0002;
const VT_I4: u16 = 0x0003;
const VT_LPSTR: u16 = 0x001E;
const VT_LPWSTR: u16 = 0x001F;
const VT_FILETIME: u16 = 0x0040;

/// Parse a FlashPix CompObj stream
pub fn parse_compobj(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    if data.len() < 28 {
        return Ok(metadata);
    }

    let reader = EndianReader::little_endian(data);

    // Skip header (4 bytes version, 4 bytes reserved)
    // Parse CLSID (16 bytes at offset 8)
    let clsid_offset = 8;
    if data.len() >= clsid_offset + 16 {
        let clsid = &data[clsid_offset..clsid_offset + 16];
        let clsid_str = format!(
            "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            clsid[3], clsid[2], clsid[1], clsid[0],
            clsid[5], clsid[4],
            clsid[7], clsid[6],
            clsid[8], clsid[9],
            clsid[10], clsid[11], clsid[12], clsid[13], clsid[14], clsid[15]
        );
        metadata.insert("FlashPix:CompObjCLSID", TagValue::String(clsid_str));
    }

    // Try to parse user type string (length-prefixed string at offset 24)
    let mut offset = 24;
    if data.len() > offset + 4 {
        let str_len = reader.u32_at(offset).unwrap_or(0) as usize;
        offset += 4;

        if data.len() >= offset + str_len && str_len > 0 && str_len < 256 {
            if let Ok(user_type) = std::str::from_utf8(&data[offset..offset + str_len]) {
                let user_type_clean = user_type.trim_end_matches('\0');
                if !user_type_clean.is_empty() {
                    metadata.insert(
                        "FlashPix:CompObjUserType",
                        TagValue::String(user_type_clean.to_string()),
                    );
                }
            }
        }
    }

    Ok(metadata)
}

/// Parse a property value based on its type
fn parse_property_value(data: &[u8], offset: usize, vt_type: u16) -> Option<TagValue> {
    let reader = EndianReader::little_endian(data);

    match vt_type {
        VT_I2 => {
            // 16-bit signed integer
            reader.i16_at(offset as u64)
                .ok()
                .map(|v| TagValue::Integer(v as i64))
        }
        VT_I4 => {
            // 32-bit signed integer
            reader.i32_at(offset as u64)
                .ok()
                .map(|v| TagValue::Integer(v as i64))
        }
        VT_LPSTR => {
            // Length-prefixed ANSI string
            let str_len = reader.u32_at(offset as u64).ok()? as usize;
            if str_len == 0 || str_len > 65536 {
                return None;
            }

            let str_offset = offset + 4;
            if data.len() < str_offset + str_len {
                return None;
            }

            let str_data = &data[str_offset..str_offset + str_len];
            std::str::from_utf8(str_data)
                .ok()
                .map(|s| TagValue::String(s.trim_end_matches('\0').to_string()))
        }
        VT_LPWSTR => {
            // Length-prefixed Unicode string (UTF-16LE)
            let char_count = reader.u32_at(offset as u64).ok()? as usize;
            if char_count == 0 || char_count > 32768 {
                return None;
            }

            let byte_len = char_count * 2;
            let str_offset = offset + 4;
            if data.len() < str_offset + byte_len {
                return None;
            }

            let str_data = &data[str_offset..str_offset + byte_len];
            let u16_chars: Vec<u16> = str_data
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();

            String::from_utf16(&u16_chars)
                .ok()
                .map(|s| TagValue::String(s.trim_end_matches('\0').to_string()))
        }
        VT_FILETIME => {
            // 64-bit FILETIME (100-nanosecond intervals since Jan 1, 1601)
            reader.u64_at(offset as u64)
                .ok()
                .map(|ft| {
                    // Convert FILETIME to Unix timestamp
                    let unix_epoch_filetime = 116444736000000000u64;
                    if ft > unix_epoch_filetime {
                        let unix_time = (ft - unix_epoch_filetime) / 10000000;
                        TagValue::Integer(unix_time as i64)
                    } else {
                        TagValue::Integer(0)
                    }
                })
        }
        VT_EMPTY | VT_NULL => None,
        _ => None,
    }
}

/// Parse a property set stream
pub fn parse_property_set(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    if data.len() < 48 {
        return Ok(metadata);
    }

    let reader = EndianReader::little_endian(data);

    // Parse property set header
    let byte_order = reader.u16_at(0).unwrap_or(0);
    if byte_order != 0xFFFE {
        return Ok(metadata);
    }

    // Format version (should be 0 or 1)
    let _format = reader.u16_at(2).unwrap_or(0);

    // Skip OS version and CLSID (bytes 4-23)

    // Number of property set sections
    let num_sections = reader.u32_at(24).unwrap_or(0);
    if num_sections == 0 {
        return Ok(metadata);
    }

    // Parse first section header (FMTID + offset)
    let section_offset = reader.u32_at(44).unwrap_or(0) as usize;

    if data.len() < section_offset + 8 {
        return Ok(metadata);
    }

    // Parse section
    let section_size = reader.u32_at(section_offset as u64).unwrap_or(0) as usize;
    let num_properties = reader.u32_at((section_offset + 4) as u64).unwrap_or(0);

    if num_properties == 0 || num_properties > 1000 {
        return Ok(metadata);
    }

    // Parse property ID/offset pairs
    let prop_list_offset = section_offset + 8;

    for i in 0..num_properties as usize {
        let entry_offset = prop_list_offset + (i * 8);
        if data.len() < entry_offset + 8 {
            break;
        }

        let prop_id = reader.u32_at(entry_offset as u64).unwrap_or(0);
        let prop_offset = reader.u32_at((entry_offset + 4) as u64).unwrap_or(0) as usize;
        let abs_offset = section_offset + prop_offset;

        if data.len() < abs_offset + 4 {
            continue;
        }

        // Parse property type
        let vt_type = reader.u16_at(abs_offset as u64).unwrap_or(0);
        let value_offset = abs_offset + 4;

        if let Some(value) = parse_property_value(data, value_offset, vt_type) {
            let tag_name = match prop_id {
                PID_CODEPAGE => "FlashPix:CodePage",
                PID_TITLE => "FlashPix:Title",
                PID_SUBJECT => "FlashPix:Subject",
                PID_AUTHOR => "FlashPix:Author",
                PID_KEYWORDS => "FlashPix:Keywords",
                PID_COMMENTS => "FlashPix:Comments",
                PID_TEMPLATE => "FlashPix:Template",
                PID_LASTAUTHOR => "FlashPix:LastModifiedBy",
                PID_REVNUMBER => "FlashPix:RevisionNumber",
                PID_EDITTIME => "FlashPix:TotalEditTime",
                PID_LASTPRINTED => "FlashPix:LastPrinted",
                PID_CREATE_DTM => "FlashPix:CreateDate",
                PID_LASTSAVE_DTM => "FlashPix:ModifyDate",
                PID_PAGECOUNT => "FlashPix:Pages",
                PID_COMPANY => "FlashPix:Company",
                PID_MANAGER => "FlashPix:Manager",
                PID_CATEGORY => "FlashPix:Category",
                _ => continue,
            };

            metadata.insert(tag_name, value);
        }
    }

    Ok(metadata)
}

/// Parse FlashPix version from image contents
pub fn parse_flashpix_version(data: &[u8]) -> Result<String> {
    // FlashPix version is typically in the format 0x01000200 for version 1.2
    if data.len() < 4 {
        return Err(ExifToolError::parse_error("Data too short"));
    }

    let reader = EndianReader::little_endian(data);
    let version_raw = reader.u32_at(0).unwrap_or(0);

    let major = (version_raw >> 16) & 0xFFFF;
    let minor = version_raw & 0xFFFF;

    Ok(format!("{}.{}", major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_flashpix_version() {
        // Version 1.2 (0x00010002 in little-endian = 02 00 01 00)
        let data = [0x02, 0x00, 0x01, 0x00];
        let result = parse_flashpix_version(&data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "1.2");
    }

    #[test]
    fn test_parse_property_value_i4() {
        let data = [0x0A, 0x00, 0x00, 0x00]; // 10 in little-endian
        let result = parse_property_value(&data, 0, VT_I4);
        assert!(result.is_some());
        if let Some(TagValue::Integer(v)) = result {
            assert_eq!(v, 10);
        } else {
            panic!("Expected Integer value");
        }
    }

    #[test]
    fn test_parse_property_value_lpstr() {
        let mut data = Vec::new();
        data.extend_from_slice(&[0x06, 0x00, 0x00, 0x00]); // Length: 6
        data.extend_from_slice(b"Hello\0"); // String with null terminator

        let result = parse_property_value(&data, 0, VT_LPSTR);
        assert!(result.is_some());
        if let Some(TagValue::String(s)) = result {
            assert_eq!(s, "Hello");
        } else {
            panic!("Expected String value");
        }
    }

    #[test]
    fn test_parse_compobj_minimal() {
        let mut data = vec![0u8; 48];
        // Version
        data[0..4].copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        // CLSID at offset 8
        data[8..24].copy_from_slice(&[
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
        ]);

        let result = parse_compobj(&data);
        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert!(metadata.contains_key("FlashPix:CompObjCLSID"));
    }
}
