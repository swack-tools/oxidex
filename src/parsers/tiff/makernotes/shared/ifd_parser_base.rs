use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};

/// Configuration for IFD parsing
///
/// Allows each parser to specify its specific signature and validation rules
/// while using the shared IFD parsing implementation.
pub struct IfdParserConfig<'a> {
    /// Optional manufacturer signature to detect and skip (e.g., b"GoPro", b"Photoshop 3.0")
    pub signature: Option<&'a [u8]>,

    /// Number of bytes to skip after signature (if present)
    pub signature_offset: usize,

    /// Maximum valid entry count for validation (typically 200-500)
    pub max_entries: usize,
}

/// Parse IFD entries from MakerNote data with a callback for each entry
///
/// This function extracts the common IFD parsing boilerplate that was duplicated
/// across 10+ makernote parsers. Each parser provides a config and callback,
/// eliminating 70-90 lines of duplicated code per file.
///
/// # Architecture
///
/// **Before** (duplicated 70-90 lines in each parser):
/// ```text
/// parse() {
///     // Skip signature
///     // Read entry count
///     // Loop through entries
///     //   - Parse tag, field_type, count, value_offset
///     //   - Create IfdEntry
///     //   - Call parser-specific logic
/// }
/// ```
///
/// **After** (2-3 lines in each parser):
/// ```text
/// parse() {
///     parse_ifd_entries(data, byte_order, config, |entry, data| {
///         // Parser-specific logic only
///     })
/// }
/// ```
///
/// # Arguments
///
/// * `data` - Full MakerNote data buffer
/// * `byte_order` - Byte order for multi-byte value parsing (little or big endian)
/// * `config` - Parser-specific configuration (signature, offset, validation)
/// * `entry_callback` - Closure called for each IFD entry with the entry and data
///
/// # Returns
///
/// * `Ok(())` - Successfully parsed all entries
/// * `Err(String)` - Data too short, invalid entry count, or parsing error
///
/// # Example
///
/// ```ignore
/// let config = IfdParserConfig {
///     signature: Some(b"GoPro"),
///     signature_offset: 5,
///     max_entries: 200,
/// };
///
/// parse_ifd_entries(data, byte_order, &config, |entry, data| {
///     // Extract tag value using entry and data
///     // Add to tags HashMap
/// })?;
/// ```
///
/// # Performance
///
/// - O(n) where n = number of IFD entries
/// - Zero-cost abstraction: callback is inlined by compiler
/// - No heap allocations beyond what callback performs
pub fn parse_ifd_entries<F>(
    data: &[u8],
    byte_order: ByteOrder,
    config: &IfdParserConfig,
    mut entry_callback: F,
) -> Result<(), String>
where
    F: FnMut(&IfdEntry, &[u8]),
{
    // Minimum IFD size: 2 bytes for entry count
    if data.len() < 2 {
        return Err("MakerNote data too short for IFD".to_string());
    }

    // Determine start offset by checking for manufacturer signature
    let start_offset = if let Some(sig) = config.signature {
        if data.len() >= sig.len() && &data[..sig.len()] == sig {
            config.signature_offset
        } else {
            0
        }
    } else {
        0
    };

    // Ensure we have enough data after skipping signature
    if start_offset >= data.len() || start_offset + 2 > data.len() {
        return Err("Invalid signature offset or data too short".to_string());
    }

    let parse_data = &data[start_offset..];

    // Read number of IFD entries (2 bytes at start of IFD)
    let entry_count = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([parse_data[0], parse_data[1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([parse_data[0], parse_data[1]]),
    } as usize;

    // Validate entry count to avoid processing corrupted data
    if entry_count == 0 || entry_count > config.max_entries {
        return Err(format!(
            "Invalid entry count: {} (expected 1-{})",
            entry_count, config.max_entries
        ));
    }

    // Parse each IFD entry (12 bytes each, standard TIFF IFD format)
    const ENTRY_SIZE: usize = 12;
    let mut offset = 2; // Start after entry count

    for _ in 0..entry_count {
        // Ensure we have enough data for a complete entry
        if offset + ENTRY_SIZE > parse_data.len() {
            break; // Incomplete entry, stop parsing gracefully
        }

        let entry_data = &parse_data[offset..offset + ENTRY_SIZE];

        // Parse IFD entry fields based on byte order
        // Format: [tag:2][type:2][count:4][value_offset:4]

        let tag_id = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[0], entry_data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([entry_data[0], entry_data[1]]),
        };

        let field_type = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[2], entry_data[3]]),
            ByteOrder::BigEndian => u16::from_be_bytes([entry_data[2], entry_data[3]]),
        };

        let value_count = match byte_order {
            ByteOrder::LittleEndian => {
                u32::from_le_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
            }
            ByteOrder::BigEndian => {
                u32::from_be_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
            }
        };

        let value_offset = match byte_order {
            ByteOrder::LittleEndian => {
                u32::from_le_bytes([entry_data[8], entry_data[9], entry_data[10], entry_data[11]])
            }
            ByteOrder::BigEndian => {
                u32::from_be_bytes([entry_data[8], entry_data[9], entry_data[10], entry_data[11]])
            }
        };

        // Create IFD entry structure
        let entry = IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        };

        // Call parser-specific callback to process this entry
        // Callback receives the parsed entry and the full data buffer
        entry_callback(&entry, parse_data);

        offset += ENTRY_SIZE;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ifd_entries_little_endian() {
        // Construct minimal IFD: [entry_count:2][tag:2][type:2][count:4][offset:4]
        // Entry count: 1
        // Tag: 0x0001, Type: 3 (SHORT), Count: 1, Value: 42
        let data = vec![
            0x01, 0x00, // entry_count = 1 (little-endian)
            0x01, 0x00, // tag = 0x0001
            0x03, 0x00, // field_type = 3 (SHORT)
            0x01, 0x00, 0x00, 0x00, // value_count = 1
            0x2A, 0x00, 0x00, 0x00, // value_offset = 42
        ];

        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 100,
        };

        let mut entries_parsed = 0;
        let result = parse_ifd_entries(&data, ByteOrder::LittleEndian, &config, |entry, _data| {
            assert_eq!(entry.tag_id, 0x0001);
            assert_eq!(entry.field_type, 3);
            assert_eq!(entry.value_count, 1);
            assert_eq!(entry.value_offset, 42);
            entries_parsed += 1;
        });

        assert!(result.is_ok());
        assert_eq!(entries_parsed, 1);
    }

    #[test]
    fn test_parse_ifd_entries_big_endian() {
        // Same test but with big-endian byte order
        let data = vec![
            0x00, 0x01, // entry_count = 1 (big-endian)
            0x00, 0x01, // tag = 0x0001
            0x00, 0x03, // field_type = 3
            0x00, 0x00, 0x00, 0x01, // value_count = 1
            0x00, 0x00, 0x00, 0x2A, // value_offset = 42
        ];

        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 100,
        };

        let mut entries_parsed = 0;
        let result = parse_ifd_entries(&data, ByteOrder::BigEndian, &config, |entry, _data| {
            assert_eq!(entry.tag_id, 0x0001);
            assert_eq!(entry.field_type, 3);
            assert_eq!(entry.value_count, 1);
            assert_eq!(entry.value_offset, 42);
            entries_parsed += 1;
        });

        assert!(result.is_ok());
        assert_eq!(entries_parsed, 1);
    }

    #[test]
    fn test_parse_ifd_entries_with_signature() {
        // Data with GoPro signature at start
        let data = vec![
            b'G', b'o', b'P', b'r', b'o', // Signature
            0x01, 0x00, // entry_count = 1
            0x01, 0x00, // tag = 0x0001
            0x03, 0x00, // field_type = 3
            0x01, 0x00, 0x00, 0x00, // value_count = 1
            0x2A, 0x00, 0x00, 0x00, // value_offset = 42
        ];

        let config = IfdParserConfig {
            signature: Some(b"GoPro"),
            signature_offset: 5,
            max_entries: 200,
        };

        let mut entries_parsed = 0;
        let result = parse_ifd_entries(&data, ByteOrder::LittleEndian, &config, |entry, _data| {
            assert_eq!(entry.tag_id, 0x0001);
            entries_parsed += 1;
        });

        assert!(result.is_ok());
        assert_eq!(entries_parsed, 1);
    }

    #[test]
    fn test_parse_ifd_entries_invalid_count() {
        // Entry count exceeds max_entries
        let data = vec![
            0xFF, 0x03, // entry_count = 1023 (exceeds max of 100)
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 100,
        };

        let result = parse_ifd_entries(&data, ByteOrder::LittleEndian, &config, |_entry, _data| {});

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid entry count"));
    }

    #[test]
    fn test_parse_ifd_entries_data_too_short() {
        // Data buffer too short for IFD
        let data = vec![0x01];

        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 100,
        };

        let result = parse_ifd_entries(&data, ByteOrder::LittleEndian, &config, |_entry, _data| {});

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ifd_entries_multiple_entries() {
        // IFD with 2 entries
        let data = vec![
            0x02, 0x00, // entry_count = 2
            // Entry 1
            0x01, 0x00, // tag = 0x0001
            0x03, 0x00, // field_type = 3
            0x01, 0x00, 0x00, 0x00, // value_count = 1
            0x0A, 0x00, 0x00, 0x00, // value_offset = 10
            // Entry 2
            0x02, 0x00, // tag = 0x0002
            0x04, 0x00, // field_type = 4
            0x01, 0x00, 0x00, 0x00, // value_count = 1
            0x14, 0x00, 0x00, 0x00, // value_offset = 20
        ];

        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 100,
        };

        let mut entries_parsed = 0;
        let result = parse_ifd_entries(&data, ByteOrder::LittleEndian, &config, |entry, _data| {
            match entries_parsed {
                0 => {
                    assert_eq!(entry.tag_id, 0x0001);
                    assert_eq!(entry.value_offset, 10);
                }
                1 => {
                    assert_eq!(entry.tag_id, 0x0002);
                    assert_eq!(entry.value_offset, 20);
                }
                _ => panic!("Too many entries"),
            }
            entries_parsed += 1;
        });

        assert!(result.is_ok());
        assert_eq!(entries_parsed, 2);
    }
}
