//! Image File Directory (IFD) parsing
//!
//! This module handles parsing of TIFF IFD structures using nom parser combinators.
//! IFDs are the core structural element of TIFF files, containing arrays of tag entries
//! that store image metadata.
//!
//! # TIFF IFD Structure
//!
//! An IFD consists of:
//! 1. **Entry Count**: 2 bytes (u16) - number of tag entries in this IFD
//! 2. **Tag Entries**: 12 bytes each × entry_count
//!    - Tag ID: 2 bytes (u16) - identifies the tag (e.g., 0x010F for Make)
//!    - Field Type: 2 bytes (u16) - data type (1=Byte, 2=ASCII, 3=Short, etc.)
//!    - Value Count: 4 bytes (u32) - number of values (not bytes)
//!    - Value/Offset: 4 bytes (u32) - either inline value (if ≤4 bytes) or offset
//! 3. **Next IFD Offset**: 4 bytes (u32) - offset to next IFD, or 0 if last
//!
//! # Byte Order
//!
//! TIFF files can be either little-endian (0x4949 "II") or big-endian (0x4D4D "MM").
//! The byte order marker appears at the start of the TIFF file and affects all
//! multi-byte values in the IFD structure.
//!
//! # TIFF Variants
//!
//! While standard TIFF uses magic number 42 (0x002A), some RAW formats use variants:
//! - Panasonic RW2: uses 0x55 (85) as magic number, but otherwise standard TIFF structure
//! - The IFD parser works with all TIFF variants, as it only processes IFD structures
//!
//! # Value Storage
//!
//! Values are stored either inline or via offset:
//! - If `(type_size × count) ≤ 4 bytes`: value stored inline in Value/Offset field
//! - Otherwise: Value/Offset contains absolute file offset to value data
//!
//! # Type Aliases
//!
//! - `IfdEntry`: Tuple representing a single IFD entry (tag, type, count, data)
//! - `IfdEntries`: Vector of IFD entries
//!
//! # Example
//!
//! ```no_run
//! use oxidex::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
//! use oxidex::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("image.tif"))?;
//! let tags = parse_ifd(&reader, 8, ByteOrder::LittleEndian)?;
//!
//! for (tag_id, _, _, value) in tags {
//!     // value is Cow<[u8]>, use .as_ref() to get &[u8]
//!     println!("Tag 0x{:04X}: {} bytes", tag_id, value.as_ref().len());
//! }
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

use crate::core::FileReader;
use crate::error::{ExifToolError, Result};
use crate::io::{ByteOrder as IoByteOrder, EndianReader};
use crate::parsers::common::exif_types::ExifType;
use nom::{
    IResult,
    combinator::map,
    multi::count,
    number::complete::{be_u16, be_u32, le_u16, le_u32},
};
use std::borrow::Cow;

/// Type alias for IFD entry tuples: (tag, type, count, data)
///
/// Using `Cow<'static, [u8]>` allows zero-copy optimization for large values
/// while still supporting owned data when needed. The 'static lifetime is used
/// pragmatically since the data lifetime is tied to the FileReader, which we
/// don't want to propagate through the entire type system.
pub type IfdEntryTuple = (u16, u16, u32, Cow<'static, [u8]>);

/// Type alias for vector of IFD entries
pub type IfdEntries = Vec<IfdEntryTuple>;

/// Byte order (endianness) for TIFF data.
///
/// TIFF files begin with a 2-byte order marker:
/// - `0x4949` ("II") indicates little-endian (Intel byte order)
/// - `0x4D4D` ("MM") indicates big-endian (Motorola byte order)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// Little-endian byte order (0x4949 "II")
    LittleEndian,
    /// Big-endian byte order (0x4D4D "MM")
    BigEndian,
}

impl ByteOrder {
    /// ExifTool's `ExifByteOrder` value, e.g. `Little-endian (Intel, II)`.
    ///
    /// ExifTool reports this for every file with a TIFF header, and it was the
    /// single most-missed tag in the comparison corpus once the derived tags
    /// were in place -- the information was always available at the point the
    /// header is parsed, it was simply never recorded.
    #[must_use]
    pub const fn exif_byte_order_tag(self) -> &'static str {
        match self {
            ByteOrder::LittleEndian => "Little-endian (Intel, II)",
            ByteOrder::BigEndian => "Big-endian (Motorola, MM)",
        }
    }
}

impl ByteOrder {
    /// Converts TIFF ByteOrder to the shared io::ByteOrder enum.
    ///
    /// This enables using EndianReader with TIFF byte order specification.
    #[inline]
    pub fn to_io_byte_order(self) -> IoByteOrder {
        match self {
            ByteOrder::LittleEndian => IoByteOrder::Little,
            ByteOrder::BigEndian => IoByteOrder::Big,
        }
    }
}

/// Represents a single TIFF IFD tag entry.
///
/// Each entry is 12 bytes and contains the tag ID, type, count, and either
/// the value itself (if small enough) or an offset to the value data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfdEntry {
    /// Tag identifier (e.g., 0x010F for Make)
    pub tag_id: u16,
    /// EXIF data type (e.g., ASCII, Short, Long)
    pub field_type: u16,
    /// Number of values (not bytes)
    pub value_count: u32,
    /// Either inline value or offset to value data
    pub value_offset: u32,
}

/// Parses a TIFF Image File Directory (IFD) and extracts tag values.
///
/// This function reads an IFD structure at the specified offset and returns
/// a vector of (tag_id, raw_value) pairs. The raw values are returned as
/// owned byte vectors.
///
/// # Performance Notes
///
/// The current implementation optimizes inline values (≤4 bytes) by extracting
/// them directly from the IFD entry without file I/O. However, the API returns
/// `Vec<u8>` which requires allocation even for small values.
///
/// TODO: Consider using `Cow<[u8]>` to avoid copies for large external values.
/// This would require API changes and lifetime annotations but could reduce
/// allocations for large tags like MakerNotes.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing file data
/// - `ifd_offset`: Byte offset from start of TIFF data to the IFD
/// - `byte_order`: Endianness for parsing multi-byte values
///
/// # Returns
///
/// - `Ok(Vec<(u16, u16, u32, Cow<'static, [u8]>)>)`: Vector of (tag_id, field_type, value_count, raw_value_bytes) tuples
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Errors
///
/// Returns an error if:
/// - IFD offset is beyond file size
/// - Entry count is invalid
/// - Tag entry data is truncated
/// - Value offset points beyond file size
/// - Unknown or invalid field type encountered
///
/// # Example
///
/// ```no_run
/// use oxidex::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
/// use oxidex::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("image.tif"))?;
/// let tags = parse_ifd(&reader, 8, ByteOrder::LittleEndian)?;
///
/// // Find Make tag (0x010F)
/// for (tag_id, _, _, value) in &tags {
///     if *tag_id == 0x010F {
///         // value is Cow<[u8]>, use .as_ref() to get &[u8]
///         let make = String::from_utf8_lossy(value.as_ref());
///         println!("Make: {}", make);
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub fn parse_ifd(
    reader: &dyn FileReader,
    ifd_offset: u64,
    byte_order: ByteOrder,
) -> Result<IfdEntries> {
    let file_size = reader.size();

    // Validate IFD offset
    if ifd_offset >= file_size {
        return Err(ExifToolError::parse_error_at(
            "IFD offset beyond file size",
            ifd_offset as usize,
        ));
    }

    // Read entry count (2 bytes) using EndianReader for consistent byte order handling
    let entry_count_data = reader.read(ifd_offset, 2)?;
    let endian_reader = EndianReader::new(entry_count_data, byte_order.to_io_byte_order());
    let entry_count = endian_reader
        .u16_at(0)
        .ok_or_else(|| ExifToolError::parse_error("Failed to read IFD entry count"))?;

    // Calculate IFD size: 2 bytes (count) + 12 bytes per entry + 4 bytes (next IFD offset)
    let ifd_size = 2 + (entry_count as usize * 12) + 4;

    // Validate IFD size doesn't exceed file
    if ifd_offset + ifd_size as u64 > file_size {
        return Err(ExifToolError::parse_error_at(
            format!("IFD size ({} bytes) exceeds file bounds", ifd_size),
            ifd_offset as usize,
        ));
    }

    // Read entire IFD (excluding the initial 2-byte count we already read)
    let entries_start = ifd_offset + 2;
    let entries_size = entry_count as usize * 12;
    let entries_data = reader.read(entries_start, entries_size)?;

    // Parse IFD entries based on byte order
    let ifd_entries = match byte_order {
        ByteOrder::LittleEndian => {
            parse_ifd_entries_le(entries_data, entry_count)
                .map_err(|e| {
                    ExifToolError::parse_error_at(
                        format!("Failed to parse IFD entries (LE): {}", e),
                        entries_start as usize,
                    )
                })?
                .1
        }
        ByteOrder::BigEndian => {
            parse_ifd_entries_be(entries_data, entry_count)
                .map_err(|e| {
                    ExifToolError::parse_error_at(
                        format!("Failed to parse IFD entries (BE): {}", e),
                        entries_start as usize,
                    )
                })?
                .1
        }
    };

    // Extract tag values
    // Pre-allocate capacity to avoid reallocations since entry_count is known upfront
    let mut result = Vec::with_capacity(entry_count as usize);

    for entry in ifd_entries {
        // Get type information. ExifTool skips entries with an unknown/invalid
        // format type (e.g. type 0 written by some corrupted camera firmware)
        // rather than abandoning the directory, so a single bad entry doesn't
        // cost the entire IFD chain (IFD0 -> ExifIFD -> GPS -> IFD1).
        let Some(exif_type) = ExifType::from_u16(entry.field_type) else {
            continue;
        };

        let type_size = exif_type.size_in_bytes();
        let total_size = type_size * entry.value_count as usize;

        // Extract value bytes using Cow for zero-copy optimization
        let value_bytes = if total_size <= 4 {
            // Value is stored inline in the value_offset field
            // We need to create owned data since it's derived from the field value
            Cow::Owned(extract_inline_value(
                entry.value_offset,
                total_size,
                byte_order,
            ))
        } else {
            // Value is stored at an offset
            let value_offset = entry.value_offset as u64;

            // Validate offset
            if value_offset + total_size as u64 > file_size {
                return Err(ExifToolError::parse_error_at(
                    format!(
                        "Tag 0x{:04X} value offset ({}) + size ({}) exceeds file size",
                        entry.tag_id, value_offset, total_size
                    ),
                    value_offset as usize,
                ));
            }

            // Read value data from offset
            // For now, we use Cow::Owned since the FileReader API returns borrowed slices
            // but we can't guarantee the lifetime matches our 'static constraint.
            // Future optimization: change FileReader to support arena allocation or
            // return data with explicit lifetimes that can be borrowed.
            Cow::Owned(reader.read(value_offset, total_size)?.to_vec())
        };

        result.push((
            entry.tag_id,
            entry.field_type,
            entry.value_count,
            value_bytes,
        ));
    }

    Ok(result)
}

/// Returns the raw 4-byte `value_offset` field of the first entry carrying
/// `tag_id` in the IFD at `ifd_offset`.
///
/// [`parse_ifd`] resolves each entry to its bytes and then discards the offset
/// those bytes came from. That offset is the only way to relate a blob back to
/// its position in the TIFF, which MakerNote parsers need: a MakerNote is an
/// IFD whose own entries store TIFF-relative offsets, so reading a value that
/// does not fit in 4 bytes means converting a TIFF-relative offset into an
/// index inside the MakerNote blob - `value_offset - makernote_value_offset`.
///
/// # Parameters
/// - `reader`: reader whose offset 0 is the TIFF header
/// - `ifd_offset`: TIFF-relative offset of the IFD to scan
/// - `byte_order`: endianness of the TIFF
/// - `tag_id`: tag to look for
///
/// # Returns
/// The entry's `value_offset` field, or `None` when the IFD cannot be read or
/// holds no such tag. The value is returned verbatim: for entries whose data is
/// 4 bytes or shorter it is the inline data, not an offset, so callers that
/// care must check the size themselves.
pub fn find_entry_value_offset(
    reader: &dyn FileReader,
    ifd_offset: u64,
    byte_order: ByteOrder,
    tag_id: u16,
) -> Option<u32> {
    find_entry_position(reader, ifd_offset, byte_order, tag_id).map(|entry| entry.value_offset)
}

/// Where one IFD entry's value sits, and where the entry list around it ends.
///
/// [`find_entry_value_offset`] is this without the extent. The extra two fields
/// are what a caller needs to decide whether the offset can be *trusted*: the
/// value's length, so it can be bounds-checked against the block it points
/// into, and the declaring directory's end, so ExifTool's "Suspicious
/// MakerNotes offset" test (`Exif.pm:6549`) can reject a value that runs back
/// over the entry list.
pub struct EntryPosition {
    /// The entry's stored value offset, measured from the TIFF header.
    pub value_offset: u32,
    /// The value's length in bytes: `count * sizeof(type)`.
    pub value_len: u64,
    /// One past the last byte of the declaring IFD -- its count, its 12-byte
    /// entries and its next-IFD pointer. ExifTool's `$dirEnd`.
    pub dir_end: u64,
}

/// Locates the first entry carrying `tag_id` in the IFD at `ifd_offset`.
///
/// See [`EntryPosition`]. Returns `None` when the IFD cannot be read, holds no
/// such tag, or describes a value whose length overflows.
pub fn find_entry_position(
    reader: &dyn FileReader,
    ifd_offset: u64,
    byte_order: ByteOrder,
    tag_id: u16,
) -> Option<EntryPosition> {
    let io_order = byte_order.to_io_byte_order();
    let count_bytes = reader.read(ifd_offset, 2).ok()?;
    let entry_count = EndianReader::new(count_bytes, io_order).u16_at(0)?;

    // count + 12 bytes per entry + the next-IFD pointer.
    let dir_end = ifd_offset
        .checked_add(2)?
        .checked_add(u64::from(entry_count).checked_mul(12)?)?
        .checked_add(4)?;

    let entries_data = reader
        .read(ifd_offset + 2, entry_count as usize * 12)
        .ok()?;
    let entries = EndianReader::new(entries_data, io_order);

    (0..entry_count as usize).find_map(|i| {
        let base = i * 12;
        if entries.u16_at(base)? != tag_id {
            return None;
        }
        let field_type = entries.u16_at(base + 2)?;
        let value_count = u64::from(entries.u32_at(base + 4)?);
        Some(EntryPosition {
            value_offset: entries.u32_at(base + 8)?,
            value_len: value_count.checked_mul(tiff_type_size(field_type))?,
            dir_end,
        })
    })
}

/// Byte width of a TIFF field type, 0 for the codes TIFF does not define.
///
/// An unknown type yields a zero-length value rather than a guess.
fn tiff_type_size(field_type: u16) -> u64 {
    match field_type {
        1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,         // SHORT, SSHORT
        4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
        _ => 0,
    }
}

/// Extracts an inline value from the 4-byte value_offset field.
///
/// For values ≤4 bytes, TIFF stores them directly in the value_offset field.
/// Values are left-justified (stored in the first N bytes).
///
/// This function reconstructs the original bytes from the u32 value based on
/// the byte order used when the value was parsed.
fn extract_inline_value(value_offset: u32, size: usize, byte_order: ByteOrder) -> Vec<u8> {
    // Reconstruct the original bytes from the u32 value based on byte order.
    // The EndianReader provides the bytes_at method but we need to first
    // convert the u32 back to bytes in the correct order.
    let bytes = match byte_order {
        ByteOrder::LittleEndian => value_offset.to_le_bytes(),
        ByteOrder::BigEndian => value_offset.to_be_bytes(),
    };

    // TIFF spec: values are left-justified in the 4-byte field
    bytes[0..size].to_vec()
}

/// Parses IFD entries in little-endian byte order.
fn parse_ifd_entries_le(input: &[u8], entry_count: u16) -> IResult<&[u8], Vec<IfdEntry>> {
    use nom::Parser;
    count(parse_ifd_entry_le, entry_count as usize).parse(input)
}

/// Parses IFD entries in big-endian byte order.
fn parse_ifd_entries_be(input: &[u8], entry_count: u16) -> IResult<&[u8], Vec<IfdEntry>> {
    use nom::Parser;
    count(parse_ifd_entry_be, entry_count as usize).parse(input)
}

/// Parses a single IFD entry (12 bytes) in little-endian byte order.
fn parse_ifd_entry_le(input: &[u8]) -> IResult<&[u8], IfdEntry> {
    use nom::Parser;
    map(
        |input| {
            let (input, tag_id) = le_u16(input)?;
            let (input, field_type) = le_u16(input)?;
            let (input, value_count) = le_u32(input)?;
            let (input, value_offset) = le_u32(input)?;
            Ok((input, (tag_id, field_type, value_count, value_offset)))
        },
        |(tag_id, field_type, value_count, value_offset)| IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        },
    )
    .parse(input)
}

/// Parses a single IFD entry (12 bytes) in big-endian byte order.
fn parse_ifd_entry_be(input: &[u8]) -> IResult<&[u8], IfdEntry> {
    use nom::Parser;
    map(
        |input| {
            let (input, tag_id) = be_u16(input)?;
            let (input, field_type) = be_u16(input)?;
            let (input, value_count) = be_u32(input)?;
            let (input, value_offset) = be_u32(input)?;
            Ok((input, (tag_id, field_type, value_count, value_offset)))
        },
        |(tag_id, field_type, value_count, value_offset)| IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        },
    )
    .parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Creates a minimal TIFF IFD with 3 tags in little-endian format.
    ///
    /// Tags included:
    /// - 0x010F (Make): "Canon" (5 bytes at offset 100)
    /// - 0x0110 (Model): "EOS" (3 bytes inline)
    /// - 0x0132 (DateTime): "2024:01:01 12:00:00" (19 bytes at offset 106)
    fn create_sample_ifd_le() -> Vec<u8> {
        let mut data = vec![0u8; 200];

        // === IFD at offset 0 ===

        // Entry count: 3 tags (little-endian)
        data[0] = 0x03;
        data[1] = 0x00;

        // === Tag 1: Make (0x010F) ===
        // Offset 2: Tag ID = 0x010F
        data[2] = 0x0F;
        data[3] = 0x01;
        // Offset 4: Type = ASCII (2)
        data[4] = 0x02;
        data[5] = 0x00;
        // Offset 6: Count = 6 (includes null terminator)
        data[6] = 0x06;
        data[7] = 0x00;
        data[8] = 0x00;
        data[9] = 0x00;
        // Offset 10: Value offset = 100 (points to "Canon\0")
        data[10] = 0x64;
        data[11] = 0x00;
        data[12] = 0x00;
        data[13] = 0x00;

        // === Tag 2: Model (0x0110) ===
        // Offset 14: Tag ID = 0x0110
        data[14] = 0x10;
        data[15] = 0x01;
        // Offset 16: Type = ASCII (2)
        data[16] = 0x02;
        data[17] = 0x00;
        // Offset 18: Count = 4 (includes null terminator, fits inline)
        data[18] = 0x04;
        data[19] = 0x00;
        data[20] = 0x00;
        data[21] = 0x00;
        // Offset 22: Inline value = "EOS\0"
        data[22] = b'E';
        data[23] = b'O';
        data[24] = b'S';
        data[25] = 0x00;

        // === Tag 3: DateTime (0x0132) ===
        // Offset 26: Tag ID = 0x0132
        data[26] = 0x32;
        data[27] = 0x01;
        // Offset 28: Type = ASCII (2)
        data[28] = 0x02;
        data[29] = 0x00;
        // Offset 30: Count = 20 (includes null terminator)
        data[30] = 0x14;
        data[31] = 0x00;
        data[32] = 0x00;
        data[33] = 0x00;
        // Offset 34: Value offset = 106 (points to datetime string)
        data[34] = 0x6A;
        data[35] = 0x00;
        data[36] = 0x00;
        data[37] = 0x00;

        // Next IFD offset: 0 (no next IFD)
        data[38] = 0x00;
        data[39] = 0x00;
        data[40] = 0x00;
        data[41] = 0x00;

        // === Value data ===
        // Offset 100: "Canon\0"
        data[100..106].copy_from_slice(b"Canon\0");

        // Offset 106: "2024:01:01 12:00:00\0"
        data[106..126].copy_from_slice(b"2024:01:01 12:00:00\0");

        data
    }

    /// Creates a minimal TIFF IFD with 3 tags in big-endian format.
    fn create_sample_ifd_be() -> Vec<u8> {
        let mut data = vec![0u8; 200];

        // === IFD at offset 0 ===

        // Entry count: 3 tags (big-endian)
        data[0] = 0x00;
        data[1] = 0x03;

        // === Tag 1: Make (0x010F) ===
        data[2] = 0x01;
        data[3] = 0x0F;
        data[4] = 0x00;
        data[5] = 0x02; // ASCII
        data[6] = 0x00;
        data[7] = 0x00;
        data[8] = 0x00;
        data[9] = 0x06; // Count = 6
        data[10] = 0x00;
        data[11] = 0x00;
        data[12] = 0x00;
        data[13] = 0x64; // Offset = 100

        // === Tag 2: Model (0x0110) - inline ===
        data[14] = 0x01;
        data[15] = 0x10;
        data[16] = 0x00;
        data[17] = 0x02; // ASCII
        data[18] = 0x00;
        data[19] = 0x00;
        data[20] = 0x00;
        data[21] = 0x04; // Count = 4
        data[22] = b'E';
        data[23] = b'O';
        data[24] = b'S';
        data[25] = 0x00;

        // === Tag 3: DateTime (0x0132) ===
        data[26] = 0x01;
        data[27] = 0x32;
        data[28] = 0x00;
        data[29] = 0x02; // ASCII
        data[30] = 0x00;
        data[31] = 0x00;
        data[32] = 0x00;
        data[33] = 0x14; // Count = 20
        data[34] = 0x00;
        data[35] = 0x00;
        data[36] = 0x00;
        data[37] = 0x6A; // Offset = 106

        // Next IFD offset: 0
        data[38] = 0x00;
        data[39] = 0x00;
        data[40] = 0x00;
        data[41] = 0x00;

        // === Value data ===
        data[100..106].copy_from_slice(b"Canon\0");
        data[106..126].copy_from_slice(b"2024:01:01 12:00:00\0");

        data
    }

    #[test]
    fn test_parse_ifd_little_endian() {
        let data = create_sample_ifd_le();
        let reader = TestReader::new(data);

        let tags = parse_ifd(&reader, 0, ByteOrder::LittleEndian)
            .expect("Failed to parse little-endian IFD");

        // Should have 3 tags
        assert_eq!(tags.len(), 3);

        // Check Make tag (0x010F)
        let make = tags.iter().find(|(id, _, _, _)| *id == 0x010F);
        assert!(make.is_some());
        let (_, _, _, make_value) = make.unwrap();
        assert_eq!(make_value.as_ref(), b"Canon\0");

        // Check Model tag (0x0110)
        let model = tags.iter().find(|(id, _, _, _)| *id == 0x0110);
        assert!(model.is_some());
        let (_, _, _, model_value) = model.unwrap();
        assert_eq!(model_value.as_ref(), b"EOS\0");

        // Check DateTime tag (0x0132)
        let datetime = tags.iter().find(|(id, _, _, _)| *id == 0x0132);
        assert!(datetime.is_some());
        let (_, _, _, datetime_value) = datetime.unwrap();
        assert_eq!(datetime_value.as_ref(), b"2024:01:01 12:00:00\0");
    }

    #[test]
    fn test_parse_ifd_big_endian() {
        let data = create_sample_ifd_be();
        let reader = TestReader::new(data);

        let tags =
            parse_ifd(&reader, 0, ByteOrder::BigEndian).expect("Failed to parse big-endian IFD");

        // Should have 3 tags
        assert_eq!(tags.len(), 3);

        // Check Make tag
        let make = tags.iter().find(|(id, _, _, _)| *id == 0x010F);
        assert!(make.is_some());
        let (_, _, _, make_value) = make.unwrap();
        assert_eq!(make_value.as_ref(), b"Canon\0");

        // Check Model tag
        let model = tags.iter().find(|(id, _, _, _)| *id == 0x0110);
        assert!(model.is_some());
        let (_, _, _, model_value) = model.unwrap();
        assert_eq!(model_value.as_ref(), b"EOS\0");

        // Check DateTime tag
        let datetime = tags.iter().find(|(id, _, _, _)| *id == 0x0132);
        assert!(datetime.is_some());
        let (_, _, _, datetime_value) = datetime.unwrap();
        assert_eq!(datetime_value.as_ref(), b"2024:01:01 12:00:00\0");
    }

    #[test]
    fn test_parse_empty_ifd() {
        let mut data = vec![0u8; 10];
        // Entry count: 0
        data[0] = 0x00;
        data[1] = 0x00;
        // Next IFD offset: 0
        data[2] = 0x00;
        data[3] = 0x00;
        data[4] = 0x00;
        data[5] = 0x00;

        let reader = TestReader::new(data);
        let tags =
            parse_ifd(&reader, 0, ByteOrder::LittleEndian).expect("Failed to parse empty IFD");

        assert_eq!(tags.len(), 0);
    }

    #[test]
    fn test_parse_truncated_ifd() {
        let mut data = vec![0u8; 10];
        // Entry count: 5 (but not enough data for 5 entries)
        data[0] = 0x05;
        data[1] = 0x00;

        let reader = TestReader::new(data);
        let result = parse_ifd(&reader, 0, ByteOrder::LittleEndian);

        // Should fail due to truncated IFD
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ifd_offset_beyond_file() {
        let data = vec![0u8; 50];
        let reader = TestReader::new(data);

        let result = parse_ifd(&reader, 100, ByteOrder::LittleEndian);

        // Should fail because offset is beyond file size
        assert!(result.is_err());
        if let Err(ExifToolError::ParseError { message, offset }) = result {
            assert!(message.contains("beyond file size"));
            assert_eq!(offset, Some(100));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_parse_ifd_with_invalid_value_offset() {
        let mut data = vec![0u8; 100];

        // Entry count: 1
        data[0] = 0x01;
        data[1] = 0x00;

        // Tag entry with invalid offset
        data[2] = 0xFF; // Tag ID
        data[3] = 0xFF;
        data[4] = 0x02; // Type = ASCII
        data[5] = 0x00;
        data[6] = 0x0A; // Count = 10
        data[7] = 0x00;
        data[8] = 0x00;
        data[9] = 0x00;
        data[10] = 0xE8; // Offset = 1000 (beyond file)
        data[11] = 0x03;
        data[12] = 0x00;
        data[13] = 0x00;

        let reader = TestReader::new(data);
        let result = parse_ifd(&reader, 0, ByteOrder::LittleEndian);

        // Should fail due to invalid offset
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ifd_with_unknown_type() {
        let mut data = vec![0u8; 50];

        // Entry count: 1
        data[0] = 0x01;
        data[1] = 0x00;

        // Tag entry with unknown type
        data[2] = 0xFF; // Tag ID
        data[3] = 0xFF;
        data[4] = 0xFF; // Type = 255 (invalid)
        data[5] = 0x00;
        data[6] = 0x01; // Count = 1
        data[7] = 0x00;
        data[8] = 0x00;
        data[9] = 0x00;
        data[10] = 0x00; // Value
        data[11] = 0x00;
        data[12] = 0x00;
        data[13] = 0x00;

        let reader = TestReader::new(data);
        let result = parse_ifd(&reader, 0, ByteOrder::LittleEndian);

        // The bad entry is skipped (matching ExifTool), leaving an empty
        // directory rather than an error.
        let entries = result.expect("unknown type should be skipped, not fatal");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_ifd_skips_bad_type_entry_keeps_valid_entries() {
        // A directory with one type-0 entry (as written by e.g. the Samsung
        // GT-S9402 for tag 0x8827) sandwiched between valid entries must
        // yield the valid entries instead of aborting the whole IFD.
        let mut data = vec![0u8; 200];

        // Entry count: 3 (little-endian)
        data[0] = 0x03;
        data[1] = 0x00;

        // === Entry 1 (valid): Model (0x0110), ASCII, "EOS\0" inline ===
        data[2] = 0x10;
        data[3] = 0x01;
        data[4] = 0x02; // Type = ASCII
        data[5] = 0x00;
        data[6] = 0x04; // Count = 4
        data[7] = 0x00;
        data[8] = 0x00;
        data[9] = 0x00;
        data[10] = b'E';
        data[11] = b'O';
        data[12] = b'S';
        data[13] = 0x00;

        // === Entry 2 (malformed): ISO (0x8827) with type 0 ===
        data[14] = 0x27;
        data[15] = 0x88;
        data[16] = 0x00; // Type = 0 (invalid)
        data[17] = 0x00;
        data[18] = 0x01; // Count = 1
        data[19] = 0x00;
        data[20] = 0x00;
        data[21] = 0x00;
        data[22] = 0x64; // Value = 100
        data[23] = 0x00;
        data[24] = 0x00;
        data[25] = 0x00;

        // === Entry 3 (valid): Orientation (0x0112), SHORT, value 1 inline ===
        data[26] = 0x12;
        data[27] = 0x01;
        data[28] = 0x03; // Type = SHORT
        data[29] = 0x00;
        data[30] = 0x01; // Count = 1
        data[31] = 0x00;
        data[32] = 0x00;
        data[33] = 0x00;
        data[34] = 0x01; // Value = 1
        data[35] = 0x00;
        data[36] = 0x00;
        data[37] = 0x00;

        // Next IFD offset: 0
        data[38] = 0x00;
        data[39] = 0x00;
        data[40] = 0x00;
        data[41] = 0x00;

        let reader = TestReader::new(data);
        let entries = parse_ifd(&reader, 0, ByteOrder::LittleEndian)
            .expect("directory with one bad entry must still parse");

        // The two valid entries survive; the type-0 entry is dropped.
        assert_eq!(entries.len(), 2);

        let (tag_id, field_type, count, value) = &entries[0];
        assert_eq!(*tag_id, 0x0110);
        assert_eq!(*field_type, 2);
        assert_eq!(*count, 4);
        assert_eq!(value.as_ref(), b"EOS\0");

        let (tag_id, field_type, count, value) = &entries[1];
        assert_eq!(*tag_id, 0x0112);
        assert_eq!(*field_type, 3);
        assert_eq!(*count, 1);
        assert_eq!(value.as_ref(), &[0x01, 0x00]);

        // The malformed tag must not appear at all.
        assert!(entries.iter().all(|(tag, _, _, _)| *tag != 0x8827));
    }

    #[test]
    fn test_extract_inline_value_le() {
        // Test 1-byte inline value
        let value = extract_inline_value(0x12345678, 1, ByteOrder::LittleEndian);
        assert_eq!(value, vec![0x78]);

        // Test 2-byte inline value
        let value = extract_inline_value(0x12345678, 2, ByteOrder::LittleEndian);
        assert_eq!(value, vec![0x78, 0x56]);

        // Test 4-byte inline value
        let value = extract_inline_value(0x12345678, 4, ByteOrder::LittleEndian);
        assert_eq!(value, vec![0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn test_extract_inline_value_be() {
        // Test 1-byte inline value
        let value = extract_inline_value(0x12345678, 1, ByteOrder::BigEndian);
        assert_eq!(value, vec![0x12]);

        // Test 2-byte inline value
        let value = extract_inline_value(0x12345678, 2, ByteOrder::BigEndian);
        assert_eq!(value, vec![0x12, 0x34]);

        // Test 4-byte inline value
        let value = extract_inline_value(0x12345678, 4, ByteOrder::BigEndian);
        assert_eq!(value, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_byte_order_equality() {
        assert_eq!(ByteOrder::LittleEndian, ByteOrder::LittleEndian);
        assert_eq!(ByteOrder::BigEndian, ByteOrder::BigEndian);
        assert_ne!(ByteOrder::LittleEndian, ByteOrder::BigEndian);
    }
}
