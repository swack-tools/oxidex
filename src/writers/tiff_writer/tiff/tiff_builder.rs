//! Builder pattern for complete TIFF file assembly
//!
//! This module provides a high-level builder for constructing complete TIFF
//! files with proper header, IFD hierarchy, and sub-IFD pointers.

use crate::core::metadata_map::MetadataMap;
use crate::error::Result;
use crate::parsers::tiff::ifd_parser::ByteOrder;

use super::byte_writer::write_tiff_header;
use super::ifd_builder::{EXIF_IFD_POINTER, GPS_INFO_IFD_POINTER, IfdBuilder};
use super::validator::separate_by_ifd;

// ============================================================================
// TYPE ALIASES
// ============================================================================

/// Type alias for the build result containing IFD bytes
/// Tuple of (main IFD bytes, optional EXIF IFD bytes, optional GPS IFD bytes)
type BuildResult = (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>);

/// Builder for constructing complete TIFF file structures.
///
/// The TiffBuilder orchestrates the creation of a complete TIFF file,
/// including:
/// - TIFF header
/// - IFD0 (main image metadata)
/// - ExifIFD (EXIF-specific tags)
/// - GPS IFD (GPS location data)
///
/// # Example
///
/// ```
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
/// use oxidex::writers::tiff_writer::TiffBuilder;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
/// metadata.insert("ExifIFD:ISO", TagValue::new_integer(400));
///
/// let tiff_data = TiffBuilder::new()
///     .with_byte_order(ByteOrder::LittleEndian)
///     .with_metadata(&metadata)?
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct TiffBuilder {
    /// Byte order for the TIFF file
    byte_order: ByteOrder,
    /// Main IFD metadata
    ifd0_metadata: MetadataMap,
    /// EXIF sub-IFD metadata
    exif_ifd_metadata: MetadataMap,
    /// GPS sub-IFD metadata
    gps_ifd_metadata: MetadataMap,
}

impl TiffBuilder {
    /// Creates a new TIFF builder with default settings.
    pub fn new() -> Self {
        Self {
            byte_order: ByteOrder::LittleEndian,
            ifd0_metadata: MetadataMap::new(),
            exif_ifd_metadata: MetadataMap::new(),
            gps_ifd_metadata: MetadataMap::new(),
        }
    }

    /// Sets the byte order for the TIFF file.
    pub fn with_byte_order(mut self, byte_order: ByteOrder) -> Self {
        self.byte_order = byte_order;
        self
    }

    /// Adds metadata to the TIFF file.
    ///
    /// Automatically separates tags into the appropriate IFDs based on
    /// their family prefix (IFD0:, ExifIFD:, GPS:, etc.).
    pub fn with_metadata(mut self, metadata: &MetadataMap) -> Result<Self> {
        let (ifd0, exif_ifd, gps_ifd) = separate_by_ifd(metadata);

        self.ifd0_metadata = ifd0;
        self.exif_ifd_metadata = exif_ifd;
        self.gps_ifd_metadata = gps_ifd;

        Ok(self)
    }

    /// Builds the complete TIFF file as bytes.
    ///
    /// The construction process:
    /// 1. Writes TIFF header (8 bytes)
    /// 2. Calculates IFD sizes and offsets
    /// 3. Builds IFD0 with pointers to sub-IFDs
    /// 4. Builds ExifIFD (if present)
    /// 5. Builds GPS IFD (if present)
    ///
    /// # Returns
    ///
    /// Complete TIFF file as bytes, ready to write to disk
    pub fn build(self) -> Result<Vec<u8>> {
        let mut output = Vec::new();

        // Write TIFF header (8 bytes)
        write_tiff_header(&mut output, self.byte_order);

        // Check if we have sub-IFDs
        let has_exif_ifd = !self.exif_ifd_metadata.is_empty();
        let has_gps_ifd = !self.gps_ifd_metadata.is_empty();

        if !has_exif_ifd && !has_gps_ifd {
            // Simple case: no sub-IFDs
            let ifd0_bytes = self.build_ifd0_without_pointers()?;
            output.extend_from_slice(&ifd0_bytes);
        } else {
            // Complex case: calculate offsets and build with pointers
            let (ifd0_bytes, exif_ifd_bytes, gps_ifd_bytes) =
                self.build_with_sub_ifds(has_exif_ifd, has_gps_ifd)?;

            output.extend_from_slice(&ifd0_bytes);
            if let Some(exif_bytes) = exif_ifd_bytes {
                output.extend_from_slice(&exif_bytes);
            }
            if let Some(gps_bytes) = gps_ifd_bytes {
                output.extend_from_slice(&gps_bytes);
            }
        }

        Ok(output)
    }

    /// Builds IFD0 without any sub-IFD pointers.
    fn build_ifd0_without_pointers(&self) -> Result<Vec<u8>> {
        IfdBuilder::new()
            .with_byte_order(self.byte_order)
            .with_start_offset(8) // After header
            .add_metadata(&self.ifd0_metadata)?
            .build()
    }

    /// Builds IFD0 with sub-IFDs and calculates correct offsets.
    ///
    /// This is the complex case where we need to:
    /// 1. Build IFD0 with placeholder pointers to get its size
    /// 2. Calculate where sub-IFDs will be located
    /// 3. Rebuild IFD0 with correct pointer offsets
    /// 4. Build sub-IFDs
    fn build_with_sub_ifds(&self, has_exif_ifd: bool, has_gps_ifd: bool) -> Result<BuildResult> {
        let ifd0_start = 8u64;

        // First pass: build IFD0 with placeholder pointers to determine size
        let placeholder_pointers = self.create_placeholder_pointers(has_exif_ifd, has_gps_ifd);

        let temp_ifd0 = IfdBuilder::new()
            .with_byte_order(self.byte_order)
            .with_start_offset(ifd0_start)
            .add_metadata(&self.ifd0_metadata)?
            .add_pointers(&placeholder_pointers)
            .build()?;

        let ifd0_size = temp_ifd0.len() as u64;

        // Calculate sub-IFD offsets
        let exif_ifd_offset = ifd0_start + ifd0_size;

        let gps_ifd_offset = if has_exif_ifd {
            // Calculate ExifIFD size to know where GPS IFD starts
            let exif_ifd_temp = IfdBuilder::new()
                .with_byte_order(self.byte_order)
                .with_start_offset(exif_ifd_offset)
                .add_metadata(&self.exif_ifd_metadata)?
                .build()?;
            exif_ifd_offset + exif_ifd_temp.len() as u64
        } else {
            exif_ifd_offset
        };

        // Second pass: build IFD0 with correct pointers
        let correct_pointers = self.create_correct_pointers(
            has_exif_ifd,
            has_gps_ifd,
            exif_ifd_offset,
            gps_ifd_offset,
        );

        let ifd0_bytes = IfdBuilder::new()
            .with_byte_order(self.byte_order)
            .with_start_offset(ifd0_start)
            .add_metadata(&self.ifd0_metadata)?
            .add_pointers(&correct_pointers)
            .build()?;

        // Build sub-IFDs
        let exif_ifd_bytes = if has_exif_ifd {
            Some(
                IfdBuilder::new()
                    .with_byte_order(self.byte_order)
                    .with_start_offset(exif_ifd_offset)
                    .add_metadata(&self.exif_ifd_metadata)?
                    .build()?,
            )
        } else {
            None
        };

        let gps_ifd_bytes = if has_gps_ifd {
            Some(
                IfdBuilder::new()
                    .with_byte_order(self.byte_order)
                    .with_start_offset(gps_ifd_offset)
                    .add_metadata(&self.gps_ifd_metadata)?
                    .build()?,
            )
        } else {
            None
        };

        Ok((ifd0_bytes, exif_ifd_bytes, gps_ifd_bytes))
    }

    /// Creates placeholder pointer entries (with offset 0).
    fn create_placeholder_pointers(
        &self,
        has_exif_ifd: bool,
        has_gps_ifd: bool,
    ) -> Vec<(u16, u32)> {
        let mut pointers = Vec::new();
        if has_exif_ifd {
            pointers.push((EXIF_IFD_POINTER, 0));
        }
        if has_gps_ifd {
            pointers.push((GPS_INFO_IFD_POINTER, 0));
        }
        pointers
    }

    /// Creates correct pointer entries with calculated offsets.
    fn create_correct_pointers(
        &self,
        has_exif_ifd: bool,
        has_gps_ifd: bool,
        exif_offset: u64,
        gps_offset: u64,
    ) -> Vec<(u16, u32)> {
        let mut pointers = Vec::new();
        if has_exif_ifd {
            pointers.push((EXIF_IFD_POINTER, exif_offset as u32));
        }
        if has_gps_ifd {
            pointers.push((GPS_INFO_IFD_POINTER, gps_offset as u32));
        }
        pointers
    }
}

impl Default for TiffBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;

    #[test]
    fn test_builder_simple_tiff() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let result = TiffBuilder::new()
            .with_byte_order(ByteOrder::LittleEndian)
            .with_metadata(&metadata)
            .unwrap()
            .build();

        assert!(result.is_ok());
        let bytes = result.unwrap();

        // Should start with TIFF header
        assert_eq!(&bytes[0..2], b"II"); // Little-endian marker
        assert_eq!(bytes[2], 0x2A); // Magic number
    }

    #[test]
    fn test_builder_with_exif_ifd() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("ExifIFD:ISO", TagValue::new_integer(400));

        let result = TiffBuilder::new()
            .with_byte_order(ByteOrder::LittleEndian)
            .with_metadata(&metadata)
            .unwrap()
            .build();

        assert!(result.is_ok());
        let bytes = result.unwrap();

        // Should have header + IFD0 + ExifIFD
        assert!(bytes.len() > 8);
    }

    #[test]
    fn test_builder_with_gps_ifd() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("GPS:Latitude", TagValue::new_string("37.7749"));

        let result = TiffBuilder::new()
            .with_byte_order(ByteOrder::LittleEndian)
            .with_metadata(&metadata)
            .unwrap()
            .build();

        assert!(result.is_ok());
        let bytes = result.unwrap();

        // Should have header + IFD0 + GPS IFD
        assert!(bytes.len() > 8);
    }

    #[test]
    fn test_builder_big_endian() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Nikon"));

        let result = TiffBuilder::new()
            .with_byte_order(ByteOrder::BigEndian)
            .with_metadata(&metadata)
            .unwrap()
            .build();

        assert!(result.is_ok());
        let bytes = result.unwrap();

        // Should start with big-endian marker
        assert_eq!(&bytes[0..2], b"MM");
    }
}
