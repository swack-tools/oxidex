use crate::io::EndianReader;
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

/// Where a MakerNote's own IFD begins inside the block handed to the parser.
///
/// This is ExifTool's `$subdirStart`: the signature, when the vendor writes
/// one, is skipped and the IFD's 2-byte entry count follows. Returns `None`
/// when the block is too short to hold an entry count at that position.
pub fn ifd_start_offset(data: &[u8], config: &IfdParserConfig) -> Option<usize> {
    let start_offset = match config.signature {
        Some(sig) if data.len() >= sig.len() && &data[..sig.len()] == sig => {
            config.signature_offset
        }
        _ => 0,
    };
    (start_offset + 2 <= data.len()).then_some(start_offset)
}

/// Resolves the byte order a MakerNote's own IFD is written in.
///
/// # Why a MakerNote's byte order is not the file's
///
/// 57 of the 94 MakerNote SubDirectories in ExifTool's `MakerNotes.pm` declare
/// `ByteOrder => 'Unknown'` -- Canon, Nikon3, Sony, Olympus, Pentax, Panasonic,
/// Leica, Casio, Minolta, DJI, Samsung2, Sigma and the rest -- because a
/// MakerNote is a self-contained block that a camera may write in its own
/// endianness regardless of the enclosing TIFF's. `MakerNoteCanon`
/// (MakerNotes.pm:61-69) is one:
///
/// ```text
///     Name => 'MakerNoteCanon',
///     # (starts with an IFD)
///     Condition => '$$self{Make} =~ /^Canon/',
///     SubDirectory => {
///         TagTable => 'Image::ExifTool::Canon::Main',
///         ProcessProc => \&ProcessCanon,
///         ByteOrder => 'Unknown',
///     },
/// ```
///
/// This is not a rare corner. Of the 38 corpus JPEGs whose MakerNote ExifTool
/// re-bases, 24 carry a little-endian Canon MakerNote inside a big-endian
/// ("MM") TIFF -- every EOS M50, PowerShot SX600HS, IXY 640 and so on. Read in
/// the file's order their entry count of 31 comes back as 0x1F00 = 7936, which
/// [`parse_ifd_entries`] rejects, so the whole directory yields nothing.
///
/// # The test
///
/// ExifTool resolves an `Unknown` order from the directory's own entry count
/// (`Exif.pm:6886-6893`):
///
/// ```text
///     # attempt to determine the byte ordering by checking
///     # the number of directory entries.  This is an int16u
///     # that should be a reasonable value.
///     my $num = Get16u($subdirDataPt, $subdirStart);
///     if ($num & 0xff00 and ($num>>8) > ($num&0xff)) {
///         # This looks wrong, we shouldn't have this many entries
///         my %otherOrder = ( II=>'MM', MM=>'II' );
///         $newByteOrder = $otherOrder{$oldByteOrder};
///     } else {
///         $newByteOrder = $oldByteOrder;
///     }
/// ```
///
/// It is deliberately conservative: it only swaps when the high byte is set
/// *and* exceeds the low byte, which is to say when the count read in the
/// current order is not a plausible number of IFD entries. A directory that
/// already reads sanely is never moved.
pub fn resolve_makernote_byte_order(
    data: &[u8],
    config: &IfdParserConfig,
    byte_order: ByteOrder,
) -> ByteOrder {
    let Some(start_offset) = ifd_start_offset(data, config) else {
        return byte_order;
    };
    resolve_byte_order_at(data, start_offset, byte_order)
}

/// [`resolve_makernote_byte_order`] for a parser that has already located its
/// own IFD.
///
/// Same rule, same `Exif.pm:6886-6893` predicate; the only difference is that
/// the caller passes `$subdirStart` directly instead of having it derived from
/// an [`IfdParserConfig`] signature. Vendors whose header length varies with
/// the signature -- Panasonic writes a 12-byte "Panasonic\0\0\0", an 8-byte
/// "LEICA\0\0\0" and an 18-byte "LEICA CAMERA AG\0" into the same table --
/// cannot express that offset as a single fixed `signature_offset`.
pub fn resolve_byte_order_at(
    data: &[u8],
    ifd_offset: usize,
    byte_order: ByteOrder,
) -> ByteOrder {
    let Some(ifd) = data.get(ifd_offset..) else {
        return byte_order;
    };
    let reader = EndianReader::new(ifd, byte_order.to_io_byte_order());
    let Some(num) = reader.u16_at(0) else {
        return byte_order;
    };
    if num & 0xff00 != 0 && (num >> 8) > (num & 0xff) {
        match byte_order {
            ByteOrder::LittleEndian => ByteOrder::BigEndian,
            ByteOrder::BigEndian => ByteOrder::LittleEndian,
        }
    } else {
        byte_order
    }
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

    // Create EndianReader for all byte order-aware parsing
    let reader = EndianReader::new(parse_data, byte_order.to_io_byte_order());

    // Read number of IFD entries (2 bytes at start of IFD)
    let entry_count = reader
        .u16_at(0)
        .ok_or_else(|| "Failed to read IFD entry count".to_string())?
        as usize;

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

        // Parse IFD entry fields using EndianReader
        // Format: [tag:2][type:2][count:4][value_offset:4]
        let tag_id = reader.u16_at(offset).unwrap_or(0);
        let field_type = reader.u16_at(offset + 2).unwrap_or(0);
        let value_count = reader.u32_at(offset + 4).unwrap_or(0);
        let value_offset = reader.u32_at(offset + 8).unwrap_or(0);

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

    fn no_signature() -> IfdParserConfig<'static> {
        IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 100,
        }
    }

    /// The corpus case: a 31-entry little-endian Canon MakerNote inside a
    /// big-endian TIFF reads back as 0x1F00 = 7936 entries.
    #[test]
    fn an_impossible_entry_count_swaps_the_byte_order() {
        let data = [0x1F, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            resolve_makernote_byte_order(&data, &no_signature(), ByteOrder::BigEndian),
            ByteOrder::LittleEndian
        );
    }

    /// The same test, the other way round.
    #[test]
    fn the_swap_works_in_both_directions() {
        let data = [0x00, 0x1F, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            resolve_makernote_byte_order(&data, &no_signature(), ByteOrder::LittleEndian),
            ByteOrder::BigEndian
        );
    }

    /// A directory that already reads sanely is never moved, whichever order it
    /// is in -- the high byte is zero, so ExifTool's test cannot fire.
    #[test]
    fn a_plausible_entry_count_is_left_alone() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let data = match order {
                ByteOrder::LittleEndian => [0x1F, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                ByteOrder::BigEndian => [0x00, 0x1F, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            };
            assert_eq!(
                resolve_makernote_byte_order(&data, &no_signature(), order),
                order
            );
        }
    }

    /// `($num>>8) > ($num&0xff)` -- a count whose high byte is set but does not
    /// exceed the low byte is left alone, because swapping it would produce a
    /// larger number, not a smaller one.
    #[test]
    fn a_high_byte_no_greater_than_the_low_byte_is_left_alone() {
        // Read big-endian these bytes are 0x0102 = 258 entries: high 1, low 2.
        // Swapping would give 0x0201 = 513, which is worse, so ExifTool keeps
        // the order it has.
        let data = [0x01, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            resolve_makernote_byte_order(&data, &no_signature(), ByteOrder::BigEndian),
            ByteOrder::BigEndian
        );
    }

    /// The count is read at the IFD, not at the vendor signature in front of it.
    #[test]
    fn the_count_is_read_past_the_signature() {
        let config = IfdParserConfig {
            signature: Some(b"Nikon\0"),
            signature_offset: 6,
            max_entries: 100,
        };
        let mut data = b"Nikon\0".to_vec();
        data.extend_from_slice(&[0x1F, 0x00]);
        data.resize(24, 0);
        assert_eq!(
            resolve_makernote_byte_order(&data, &config, ByteOrder::BigEndian),
            ByteOrder::LittleEndian
        );
        // Read at offset 0 instead, the bytes are "Ni" = 0x4E69: high 0x4E,
        // low 0x69, so the test would not fire at all.
        assert_eq!(
            resolve_makernote_byte_order(&data, &no_signature(), ByteOrder::BigEndian),
            ByteOrder::BigEndian
        );
    }

    /// A block with no room for an entry count keeps the caller's order.
    #[test]
    fn a_block_too_short_to_hold_a_count_keeps_the_callers_order() {
        assert_eq!(
            resolve_makernote_byte_order(&[0x1F], &no_signature(), ByteOrder::BigEndian),
            ByteOrder::BigEndian
        );
    }

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
