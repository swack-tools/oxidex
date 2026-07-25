//! Builder pattern for IFD construction
//!
//! This module provides a builder interface for constructing TIFF IFDs with
//! support for sub-IFD pointers and flexible serialization strategies.

use crate::core::metadata_map::MetadataMap;
use crate::error::Result;
use crate::parsers::tiff::ifd_parser::ByteOrder;

use super::byte_writer::{write_u16, write_u32};
use super::ifd_entry::{IfdEntryData, convert_tag_value_to_entry};
use super::validator::validate_tag_for_tiff;

/// Special tag IDs for IFD pointers
pub const EXIF_IFD_POINTER: u16 = 0x8769;
pub const GPS_INFO_IFD_POINTER: u16 = 0x8825;

/// Builder for constructing TIFF IFD structures.
///
/// The IfdBuilder provides a fluent interface for building IFD data,
/// handling entry collection, sorting, and serialization with proper
/// offset calculations.
///
/// # Example
///
/// ```
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
/// use oxidex::writers::tiff_writer::IfdBuilder;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
///
/// let ifd_bytes = IfdBuilder::new()
///     .with_byte_order(ByteOrder::LittleEndian)
///     .with_start_offset(8)
///     .add_metadata(&metadata)?
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct IfdBuilder {
    /// IFD entries to be serialized
    entries: Vec<IfdEntryData>,
    /// Byte order for serialization
    byte_order: ByteOrder,
    /// File offset where this IFD will be written
    start_offset: u64,
    /// Pointer entries for sub-IFDs (tag_id, offset)
    pointer_entries: Vec<(u16, u32)>,
}

impl IfdBuilder {
    /// Creates a new IFD builder with default settings.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            byte_order: ByteOrder::LittleEndian,
            start_offset: 0,
            pointer_entries: Vec::new(),
        }
    }

    /// Sets the byte order for serialization.
    pub fn with_byte_order(mut self, byte_order: ByteOrder) -> Self {
        self.byte_order = byte_order;
        self
    }

    /// Sets the file offset where this IFD will be written.
    ///
    /// This is important for calculating correct offsets for value data
    /// that doesn't fit inline in the IFD entries.
    pub fn with_start_offset(mut self, offset: u64) -> Self {
        self.start_offset = offset;
        self
    }

    /// Adds metadata tags to the IFD.
    ///
    /// Converts each tag in the metadata to an IFD entry. Tags that cannot
    /// be converted (unsupported types, etc.) are silently skipped.
    ///
    /// # Errors
    ///
    /// Returns an error if a tag validation fails or conversion encounters
    /// an unrecoverable issue.
    pub fn add_metadata(mut self, metadata: &MetadataMap) -> Result<Self> {
        for (tag_name, tag_value) in metadata.iter() {
            // Validate and get numeric tag ID - skip tags that aren't writable to TIFF
            let tag_id = match validate_tag_for_tiff(tag_name) {
                Ok(id) => id,
                Err(_) => continue, // Skip non-TIFF tags silently
            };

            // Convert to entry data
            if let Some(entry) = convert_tag_value_to_entry(tag_id, tag_value, self.byte_order)? {
                self.entries.push(entry);
            }
            // If conversion returns None, skip this tag (unsupported type)
        }

        Ok(self)
    }

    /// Adds a pointer entry for a sub-IFD.
    ///
    /// Pointer entries reference other IFDs in the TIFF structure, such as
    /// ExifIFD or GPS IFD. The offset should point to where the sub-IFD is
    /// located in the file.
    ///
    /// # Parameters
    ///
    /// - `tag_id`: Tag identifier for the pointer (e.g., EXIF_IFD_POINTER)
    /// - `offset`: File offset where the sub-IFD is located
    pub fn add_pointer(mut self, tag_id: u16, offset: u32) -> Self {
        self.pointer_entries.push((tag_id, offset));
        self
    }

    /// Adds multiple pointer entries at once.
    pub fn add_pointers(mut self, pointers: &[(u16, u32)]) -> Self {
        self.pointer_entries.extend_from_slice(pointers);
        self
    }

    /// Builds the complete IFD structure as bytes.
    ///
    /// Performs the following steps:
    /// 1. Adds pointer entries to the entry list
    /// 2. Sorts entries by tag ID (required by TIFF spec)
    /// 3. Calculates offsets for value data
    /// 4. Writes entry count, entries, and next IFD offset
    /// 5. Appends value area data
    ///
    /// # Returns
    ///
    /// Complete IFD structure as bytes, ready to write to a file
    pub fn build(mut self) -> Result<Vec<u8>> {
        // Add pointer entries
        for &(tag_id, offset) in &self.pointer_entries {
            let offset_bytes = encode_u32_for_byte_order(offset, self.byte_order);
            self.entries.push(IfdEntryData::new(
                tag_id,
                crate::parsers::common::exif_types::ExifType::Long,
                1,
                offset_bytes,
            ));
        }

        // Sort entries by tag ID (required by TIFF spec)
        self.entries.sort_by_key(|e| e.tag_id);

        // Calculate offsets
        let entry_count = self.entries.len() as u16;
        let ifd_header_size = 2 + (entry_count as usize * 12) + 4;
        let value_area_start = self.start_offset + ifd_header_size as u64;

        // Build IFD bytes
        let mut result = Vec::new();
        write_u16(&mut result, entry_count, self.byte_order);

        let mut current_value_offset = value_area_start;
        let mut value_area_data = Vec::new();

        // Write entries
        for entry in &self.entries {
            write_entry(
                &mut result,
                entry,
                &mut current_value_offset,
                &mut value_area_data,
                self.byte_order,
            )?;
        }

        // Write next IFD offset (0 = no next IFD)
        write_u32(&mut result, 0, self.byte_order);

        // Append value area
        result.extend_from_slice(&value_area_data);

        Ok(result)
    }

    /// Calculates the size this IFD will occupy when serialized.
    ///
    /// Useful for calculating offsets before actually building the IFD.
    pub fn calculate_size(&self) -> usize {
        let entry_count = self.entries.len() + self.pointer_entries.len();
        let header_size = 2 + (entry_count * 12) + 4;

        let value_area_size: usize = self
            .entries
            .iter()
            .filter(|e| !e.is_inline())
            .map(|e| e.value_size())
            .sum();

        header_size + value_area_size
    }
}

impl Default for IfdBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes a single IFD entry to the output buffer.
///
/// For inline values (≤4 bytes), packs them into the value_offset field.
/// For larger values, writes the offset and appends data to value_area_data.
fn write_entry(
    output: &mut Vec<u8>,
    entry: &IfdEntryData,
    current_value_offset: &mut u64,
    value_area_data: &mut Vec<u8>,
    byte_order: ByteOrder,
) -> Result<()> {
    // Write tag ID
    write_u16(output, entry.tag_id, byte_order);

    // Write field type
    write_u16(output, entry.field_type.as_u16(), byte_order);

    // Write value count
    write_u32(output, entry.value_count, byte_order);

    // Write value or offset
    if entry.is_inline() {
        // Pack value inline (left-justified in 4-byte field)
        let mut inline_bytes = [0u8; 4];
        inline_bytes[..entry.value_bytes.len()].copy_from_slice(&entry.value_bytes);
        output.extend_from_slice(&inline_bytes);
    } else {
        // Write offset to value area
        write_u32(output, *current_value_offset as u32, byte_order);

        // Append value data to value area
        value_area_data.extend_from_slice(&entry.value_bytes);

        // Update offset for next value
        *current_value_offset += entry.value_size() as u64;
    }

    Ok(())
}

/// Encodes a u32 value in the appropriate byte order.
fn encode_u32_for_byte_order(value: u32, byte_order: ByteOrder) -> Vec<u8> {
    match byte_order {
        ByteOrder::LittleEndian => value.to_le_bytes().to_vec(),
        ByteOrder::BigEndian => value.to_be_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;
    use crate::io::EndianReader;

    #[test]
    fn test_builder_empty_ifd() {
        let result = IfdBuilder::new()
            .with_byte_order(ByteOrder::LittleEndian)
            .build();

        assert!(result.is_ok());
        let bytes = result.unwrap();

        // Should have: 2 bytes (count=0) + 4 bytes (next IFD offset=0)
        assert_eq!(bytes.len(), 6);
    }

    #[test]
    fn test_builder_with_metadata() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let result = IfdBuilder::new()
            .with_byte_order(ByteOrder::LittleEndian)
            .with_start_offset(0)
            .add_metadata(&metadata)
            .unwrap()
            .build();

        assert!(result.is_ok());
        let bytes = result.unwrap();

        // Should have entries
        assert!(bytes.len() > 6);

        // Entry count should be 1
        let reader = EndianReader::little_endian(&bytes);
        let count = reader.u16_at(0).unwrap_or(0);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_builder_with_pointers() {
        let result = IfdBuilder::new()
            .with_byte_order(ByteOrder::LittleEndian)
            .add_pointer(EXIF_IFD_POINTER, 1000)
            .build();

        assert!(result.is_ok());
        let bytes = result.unwrap();

        // Entry count should be 1
        let reader = EndianReader::little_endian(&bytes);
        let count = reader.u16_at(0).unwrap_or(0);
        assert_eq!(count, 1);

        // Tag ID should be EXIF_IFD_POINTER
        let tag_id = reader.u16_at(2).unwrap_or(0);
        assert_eq!(tag_id, EXIF_IFD_POINTER);
    }

    #[test]
    fn test_calculate_size() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Model", TagValue::new_string("EOS")); // Inline

        let builder = IfdBuilder::new()
            .with_start_offset(0)
            .add_metadata(&metadata)
            .unwrap();

        let size = builder.calculate_size();
        assert_eq!(size, 18); // 2 (count) + 12 (entry) + 4 (next IFD)
    }
}
