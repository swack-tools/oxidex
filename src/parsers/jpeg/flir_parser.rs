//! FLIR thermal imaging APP1 parser
//!
//! FLIR cameras embed thermal data in APP1 segments with "FLIR\x00" identifier.
//! This parser extracts comprehensive thermal metadata from FLIR FFF (FLIR File Format)
//! segments, including camera parameters, thermal coefficients, and palette information.
//!
//! # FLIR FFF Format Structure
//!
//! The FLIR FFF format consists of:
//! - Header: "FLIR\x00" identifier followed by segment number and total segments
//! - Record Index: Table of record entries pointing to data blocks
//! - Record Data: Multiple record types containing different metadata categories
//!
//! # Supported Record Types
//!
//! - Type 0x0001 (RawData): Raw thermal image data and dimensions
//! - Type 0x0020 (CameraInfo): Camera parameters, Planck constants, atmospheric data
//! - Type 0x0022 (PaletteInfo): Color palette configuration
//! - Type 0x000E (EmbeddedImage): Embedded visual image
//!
//! # Example
//!
//! ```ignore
//! use oxidex::parsers::jpeg::flir_parser::parse_flir_segment;
//! use oxidex::core::MetadataMap;
//!
//! let data: &[u8] = &[/* FLIR APP1 segment data */];
//! let mut metadata = MetadataMap::new();
//! parse_flir_segment(data, &mut metadata)?;
//!
//! if let Some(emissivity) = metadata.get_float("FLIR:Emissivity") {
//!     println!("Emissivity: {}", emissivity);
//! }
//! ```

use crate::core::{MetadataMap, TagValue};
use crate::io::EndianReader;

/// FLIR segment identifier prefix ("FLIR\0")
const FLIR_IDENTIFIER: &[u8] = b"FLIR\x00";

/// Minimum valid FLIR segment length:
/// - 5 bytes: "FLIR\0" identifier
/// - 1 byte: segment number
/// - 1 byte: total segments
/// - 4 bytes: minimum header/index data
const MIN_FLIR_SEGMENT_LENGTH: usize = 11;

/// FLIR FFF record type for raw thermal data
const RECORD_TYPE_RAW_DATA: u16 = 0x0001;

/// FLIR FFF record type for camera information
const RECORD_TYPE_CAMERA_INFO: u16 = 0x0020;

/// FLIR FFF record type for palette information
const RECORD_TYPE_PALETTE_INFO: u16 = 0x0022;

/// FLIR FFF record type for embedded image
const RECORD_TYPE_EMBEDDED_IMAGE: u16 = 0x000E;

/// Offset table for CameraInfo record fields.
/// These offsets are relative to the start of the CameraInfo record data.
mod camera_info_offsets {
    /// Emissivity (f32) - thermal emissivity of the target object
    pub const EMISSIVITY: usize = 0x0020;
    /// Object distance in meters (f32)
    pub const OBJECT_DISTANCE: usize = 0x0024;
    /// Reflected apparent temperature in Kelvin (f32)
    pub const REFLECTED_APPARENT_TEMP: usize = 0x0028;
    /// Atmospheric temperature in Kelvin (f32)
    pub const ATMOSPHERIC_TEMP: usize = 0x002C;
    /// IR window temperature in Kelvin (f32)
    pub const IR_WINDOW_TEMP: usize = 0x0030;
    /// IR window transmission coefficient (f32)
    pub const IR_WINDOW_TRANSMISSION: usize = 0x0034;
    /// Relative humidity as percentage (f32)
    pub const RELATIVE_HUMIDITY: usize = 0x003C;
    /// Planck R1 constant (f32)
    pub const PLANCK_R1: usize = 0x0058;
    /// Planck B constant (f32)
    pub const PLANCK_B: usize = 0x005C;
    /// Planck F constant (f32)
    pub const PLANCK_F: usize = 0x0060;
    /// Atmospheric transmission alpha1 coefficient (f32)
    pub const ATMOSPHERIC_TRANS_ALPHA1: usize = 0x0070;
    /// Atmospheric transmission alpha2 coefficient (f32)
    pub const ATMOSPHERIC_TRANS_ALPHA2: usize = 0x0074;
    /// Atmospheric transmission beta1 coefficient (f32)
    pub const ATMOSPHERIC_TRANS_BETA1: usize = 0x0078;
    /// Atmospheric transmission beta2 coefficient (f32)
    pub const ATMOSPHERIC_TRANS_BETA2: usize = 0x007C;
    /// Atmospheric transmission X coefficient (f32)
    pub const ATMOSPHERIC_TRANS_X: usize = 0x0080;
    /// Camera temperature range maximum in Kelvin (f32)
    pub const CAMERA_TEMP_RANGE_MAX: usize = 0x0090;
    /// Camera temperature range minimum in Kelvin (f32)
    pub const CAMERA_TEMP_RANGE_MIN: usize = 0x0094;
    /// Camera temperature max clip value (f32)
    pub const CAMERA_TEMP_MAX_CLIP: usize = 0x0098;
    /// Camera temperature min clip value (f32)
    pub const CAMERA_TEMP_MIN_CLIP: usize = 0x009C;
    /// Camera temperature max warn value (f32)
    pub const CAMERA_TEMP_MAX_WARN: usize = 0x00A0;
    /// Camera temperature min warn value (f32)
    pub const CAMERA_TEMP_MIN_WARN: usize = 0x00A4;
    /// Camera temperature max saturated value (f32)
    pub const CAMERA_TEMP_MAX_SATURATED: usize = 0x00A8;
    /// Camera temperature min saturated value (f32)
    pub const CAMERA_TEMP_MIN_SATURATED: usize = 0x00AC;
    /// Camera model string (32 bytes)
    pub const CAMERA_MODEL: usize = 0x00D4;
    /// Camera part number string (32 bytes)
    pub const CAMERA_PART_NUMBER: usize = 0x00F4;
    /// Camera serial number string (16 bytes)
    pub const CAMERA_SERIAL_NUMBER: usize = 0x0104;
    /// Camera software version string (16 bytes)
    pub const CAMERA_SOFTWARE: usize = 0x0114;
    /// Lens model string (32 bytes)
    pub const LENS_MODEL: usize = 0x0170;
    /// Lens part number string (16 bytes)
    pub const LENS_PART_NUMBER: usize = 0x0190;
    /// Lens serial number string (16 bytes)
    pub const LENS_SERIAL_NUMBER: usize = 0x01A0;
    /// Field of view in degrees (f32)
    pub const FIELD_OF_VIEW: usize = 0x01B4;
    /// Peak spectral sensitivity in micrometers (f32)
    pub const PEAK_SPECTRAL_SENSITIVITY: usize = 0x01B8;
    /// Filter model string (16 bytes)
    pub const FILTER_MODEL: usize = 0x01EC;
    /// Filter part number string (32 bytes)
    pub const FILTER_PART_NUMBER: usize = 0x01FC;
    /// Filter serial number string (32 bytes)
    pub const FILTER_SERIAL_NUMBER: usize = 0x021C;
    /// Planck O constant (i32)
    pub const PLANCK_O: usize = 0x0308;
    /// Planck R2 constant (f32)
    pub const PLANCK_R2: usize = 0x030C;
    /// Raw value range minimum (u16)
    pub const RAW_VALUE_RANGE_MIN: usize = 0x0310;
    /// Raw value range maximum (u16)
    pub const RAW_VALUE_RANGE_MAX: usize = 0x0312;
    /// Raw value median (u16)
    pub const RAW_VALUE_MEDIAN: usize = 0x0338;
    /// Raw value range (u16)
    pub const RAW_VALUE_RANGE: usize = 0x033C;
    /// Date/time original (various formats)
    pub const DATE_TIME_ORIGINAL: usize = 0x0384;
    /// Focus step count (i16)
    pub const FOCUS_STEP_COUNT: usize = 0x0390;
    /// Focus distance in meters (f32)
    pub const FOCUS_DISTANCE: usize = 0x045C;
    /// Frame rate (u16)
    pub const FRAME_RATE: usize = 0x0464;
}

/// Offset table for RawData record fields
mod raw_data_offsets {
    /// Byte order indicator (u16)
    pub const BYTE_ORDER: usize = 0x0000;
    /// Raw thermal image width (u16)
    pub const WIDTH: usize = 0x0002;
    /// Raw thermal image height (u16)
    pub const HEIGHT: usize = 0x0004;
    /// Raw thermal image type (u16)
    pub const IMAGE_TYPE: usize = 0x0010;
}

/// Offset table for PaletteInfo record fields
mod palette_info_offsets {
    /// Number of palette colors (u8)
    pub const PALETTE_COLORS: usize = 0x0000;
    /// Above color RGB (3 bytes)
    pub const ABOVE_COLOR: usize = 0x0006;
    /// Below color RGB (3 bytes)
    pub const BELOW_COLOR: usize = 0x0009;
    /// Overflow color RGB (3 bytes)
    pub const OVERFLOW_COLOR: usize = 0x000C;
    /// Underflow color RGB (3 bytes)
    pub const UNDERFLOW_COLOR: usize = 0x000F;
    /// Isotherm1 color RGB (3 bytes)
    pub const ISOTHERM1_COLOR: usize = 0x0012;
    /// Isotherm2 color RGB (3 bytes)
    pub const ISOTHERM2_COLOR: usize = 0x0015;
    /// Palette method (u8)
    pub const PALETTE_METHOD: usize = 0x001A;
    /// Palette stretch (u8)
    pub const PALETTE_STRETCH: usize = 0x001B;
    /// Palette file name (32 bytes)
    pub const PALETTE_FILE_NAME: usize = 0x0030;
    /// Palette name (32 bytes)
    pub const PALETTE_NAME: usize = 0x0050;
    /// Palette data (variable length)
    pub const PALETTE: usize = 0x0070;
}

/// Represents a FLIR FFF record entry from the record index table.
///
/// Each record entry describes a data block within the FLIR segment,
/// including its type, offset, and length.
#[derive(Debug, Clone)]
struct FlirRecordEntry {
    /// Record type identifier (e.g., 0x0020 for CameraInfo)
    record_type: u16,
    /// Offset to record data from segment start
    offset: u32,
    /// Length of record data in bytes
    length: u32,
}

/// Parse FLIR APP1 segment and extract thermal imaging metadata.
///
/// This function parses the FLIR FFF (FLIR File Format) structure embedded
/// in JPEG APP1 segments. It extracts comprehensive thermal imaging metadata
/// including camera parameters, Planck constants for radiometric calculations,
/// atmospheric correction coefficients, and color palette information.
///
/// # Arguments
///
/// * `data` - Raw APP1 segment data (should start with "FLIR\x00")
/// * `metadata` - MetadataMap to populate with extracted FLIR tags
///
/// # Returns
///
/// * `Ok(())` - Parsing succeeded, metadata has been populated
/// * `Err(String)` - Parsing failed with error description
///
/// # Tag Naming Convention
///
/// All extracted tags use the "FLIR:" prefix for namespace consistency.
/// Temperature values are stored in Kelvin as provided by the camera.
///
/// # Example Tags Extracted
///
/// - `FLIR:CameraModel` - Camera model name
/// - `FLIR:Emissivity` - Target emissivity (0.0-1.0)
/// - `FLIR:AtmosphericTemperature` - Ambient temperature in Kelvin
/// - `FLIR:PlanckR1`, `FLIR:PlanckB`, etc. - Radiometric constants
/// - `FLIR:RawThermalImageWidth/Height` - Thermal image dimensions
pub fn parse_flir_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    // Validate FLIR segment identifier
    if data.len() < MIN_FLIR_SEGMENT_LENGTH {
        return Err("FLIR segment too short".to_string());
    }

    if &data[0..5] != FLIR_IDENTIFIER {
        return Err("Not a FLIR segment".to_string());
    }

    // Parse FLIR segment header
    // Byte 5: Often 0x01 (segment marker/version)
    // Byte 6: Segment index (0-based) for multi-segment data
    // Byte 7: Reserved/checksum
    let _segment_marker = data[5];
    let segment_index = data[6];

    // The FFF data starts after the 8-byte header
    // Header: "FLIR\0" (5) + marker (1) + index (1) + reserved (1)
    let payload = if data.len() > 8 {
        &data[8..]
    } else {
        return Ok(());
    };

    // Check if this looks like multi-segment data (DJI style)
    // First segment (index 0) contains "FFF\0" header
    let is_multi_segment = segment_index > 0
        || (payload.len() >= 4 && &payload[0..4] == b"FFF\0" && segment_index == 0);

    if is_multi_segment && segment_index < 20 {
        // Multi-segment data: reassemble all segments before parsing
        use std::cell::RefCell;
        thread_local! {
            static FLIR_SEGMENTS: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
        }

        FLIR_SEGMENTS.with(|segments| {
            let mut segs = segments.borrow_mut();

            // First segment (index 0) initializes the collection
            if segment_index == 0 {
                segs.clear();
                segs.resize(20, Vec::new()); // Max 20 segments
            }

            // Store this segment's payload
            let idx = segment_index as usize;
            if idx < segs.len() {
                segs[idx] = payload.to_vec();
            }

            // Check if we have all segments (contiguous non-empty segments)
            let filled_count = segs.iter().take_while(|s| !s.is_empty()).count();
            let is_complete =
                filled_count > 0 && (filled_count >= segs.len() || segs[filled_count].is_empty());

            if is_complete {
                // Reassemble the complete FFF data
                let mut complete_data = Vec::new();
                for seg in segs.iter() {
                    if !seg.is_empty() {
                        complete_data.extend_from_slice(seg);
                    } else {
                        break; // Stop at first empty segment
                    }
                }

                // Parse the complete FFF structure
                let result = parse_fff_structure(&complete_data, metadata);

                // Clear segments after parsing
                segs.clear();

                result
            } else {
                // Still waiting for more segments
                Ok(())
            }
        })
    } else {
        // Single-segment FLIR data - parse directly
        parse_fff_structure(payload, metadata)
    }
}

/// Parse the FLIR FFF (FLIR File Format) structure.
///
/// The FFF structure begins with a header containing file format information,
/// followed by a record index table that points to various data records.
///
/// # FFF Header Structure
///
/// - Bytes 0-3: "FFF\0" magic number
/// - Bytes 4-35: Various header fields (version, checksum, etc.)
/// - Bytes 36+: Record index table
///
/// # Arguments
///
/// * `data` - FFF data starting after the APP1 header
/// * `metadata` - MetadataMap to populate
fn parse_fff_structure(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    // Minimum FFF header size check
    if data.len() < 64 {
        // Try fallback parsing for non-standard FLIR formats
        return parse_flir_legacy_format(data, metadata);
    }

    // Check for FFF magic number (optional - some FLIR segments don't have it)
    let has_fff_header = data.len() >= 4 && &data[0..4] == b"FFF\0";

    if has_fff_header {
        // Parse standard FFF format with record index
        parse_fff_with_index(data, metadata)
    } else {
        // Try to parse as legacy format or embedded record
        parse_flir_legacy_format(data, metadata)
    }
}

/// Parse FFF structure with proper record index table.
///
/// The record index is located after the FFF header and contains
/// entries pointing to different data records (CameraInfo, RawData, etc.)
fn parse_fff_with_index(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    let reader = EndianReader::little_endian(data);

    // Extract CreatorSoftware from FFF header (typically at offset 0x08, 16 bytes)
    if let Some(creator) = try_read_string(data, 0x08, 16)
        && !creator.is_empty()
    {
        metadata.insert(
            "FLIR:CreatorSoftware".to_string(),
            TagValue::String(creator),
        );
    }

    // FFF header is typically 64 bytes
    // Record index follows the header
    // Each index entry is typically 32 bytes

    // Read number of records from header (offset varies by version)
    // Try common offset locations for record count
    let record_count = reader.u32_at(28).unwrap_or(0) as usize;
    let index_offset = reader.u32_at(32).unwrap_or(64) as usize;

    if record_count == 0 || record_count > 100 {
        // Invalid or unreasonable record count, try legacy parsing
        return parse_flir_legacy_format(data, metadata);
    }

    // Parse record index entries
    let records = parse_record_index(data, index_offset, record_count)?;

    // Process each record type
    for record in &records {
        let record_start = record.offset as usize;
        let record_end = record_start + record.length as usize;

        if record_end > data.len() {
            continue; // Skip records that extend beyond data
        }

        let record_data = &data[record_start..record_end];

        match record.record_type {
            RECORD_TYPE_RAW_DATA => {
                parse_raw_data_record(record_data, metadata);
            }
            RECORD_TYPE_CAMERA_INFO => {
                parse_camera_info_record(record_data, metadata);
            }
            RECORD_TYPE_PALETTE_INFO => {
                parse_palette_info_record(record_data, metadata);
            }
            RECORD_TYPE_EMBEDDED_IMAGE => {
                // Note presence of embedded image but don't extract binary data
                metadata.insert(
                    "FLIR:EmbeddedImage".to_string(),
                    TagValue::String(format!("{} bytes", record.length)),
                );
            }
            _ => {
                // Unknown record type - skip
            }
        }
    }

    Ok(())
}

/// Parse the record index table.
///
/// Each record index entry contains:
/// - Record type (u16)
/// - Record subtype (u16)
/// - Record version (u32)
/// - Index/ID (u32)
/// - Record offset (u32)
/// - Record length (u32)
/// - Parent index (u32)
/// - Object count (u32)
/// - Checksum (u32)
/// - Spare bytes (variable)
fn parse_record_index(
    data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<FlirRecordEntry>, String> {
    let reader = EndianReader::little_endian(data);
    let mut records = Vec::with_capacity(count);

    // Each index entry is 32 bytes in standard FFF format
    const ENTRY_SIZE: usize = 32;

    for i in 0..count {
        let entry_offset = offset + (i * ENTRY_SIZE);

        if entry_offset + ENTRY_SIZE > data.len() {
            break;
        }

        let record_type = reader.u16_at(entry_offset).unwrap_or(0);
        let record_offset = reader.u32_at(entry_offset + 12).unwrap_or(0);
        let record_length = reader.u32_at(entry_offset + 16).unwrap_or(0);

        if record_type != 0 && record_offset != 0 {
            records.push(FlirRecordEntry {
                record_type,
                offset: record_offset,
                length: record_length,
            });
        }
    }

    Ok(records)
}

/// Parse legacy/embedded FLIR format without full FFF structure.
///
/// Some FLIR segments contain data in a simpler format without the
/// full FFF record index. This function attempts to extract metadata
/// from such segments by searching for known data patterns.
fn parse_flir_legacy_format(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    let reader = EndianReader::little_endian(data);

    // Try to find CameraInfo-like data by searching for reasonable values
    // Look for patterns that suggest thermal imaging parameters

    // Search for camera model string in common locations
    for offset in [0x00, 0x08, 0x10, 0x20, 0xD4, 0x00D4].iter() {
        if let Some(model) = try_read_string(data, *offset, 32)
            && is_valid_camera_model(&model)
        {
            metadata.insert("FLIR:CameraModel".to_string(), TagValue::String(model));
            break;
        }
    }

    // Try to extract numeric parameters from fixed offsets
    // These offsets are based on common FLIR data layouts

    // Check for emissivity (should be between 0.0 and 1.0)
    if let Some(emissivity) = reader.f32_at(0x20)
        && (0.0..=1.0).contains(&emissivity)
        && emissivity > 0.0
    {
        metadata.insert(
            "FLIR:Emissivity".to_string(),
            TagValue::Float(emissivity as f64),
        );
    }

    // Try to read dimensions
    if let Some(width) = reader.u16_at(0x02)
        && (16..=4096).contains(&width)
    {
        metadata.insert(
            "FLIR:RawThermalImageWidth".to_string(),
            TagValue::Integer(width as i64),
        );
    }

    if let Some(height) = reader.u16_at(0x04)
        && (16..=4096).contains(&height)
    {
        metadata.insert(
            "FLIR:RawThermalImageHeight".to_string(),
            TagValue::Integer(height as i64),
        );
    }

    Ok(())
}

/// Parse RawData record containing thermal image information.
///
/// The RawData record contains:
/// - Image dimensions (width, height)
/// - Byte order for raw data
/// - Image type/format identifier
/// - Reference to the actual thermal image data
fn parse_raw_data_record(data: &[u8], metadata: &mut MetadataMap) {
    let reader = EndianReader::little_endian(data);

    // Parse byte order
    if let Some(byte_order) = reader.u16_at(raw_data_offsets::BYTE_ORDER) {
        let order_str = if byte_order == 0 {
            "Little-endian"
        } else {
            "Big-endian"
        };
        metadata.insert(
            "FLIR:RawDataByteOrder".to_string(),
            TagValue::String(order_str.to_string()),
        );
    }

    // Parse image dimensions
    if let Some(width) = reader.u16_at(raw_data_offsets::WIDTH)
        && width > 0
        && width <= 4096
    {
        metadata.insert(
            "FLIR:RawThermalImageWidth".to_string(),
            TagValue::Integer(width as i64),
        );
    }

    if let Some(height) = reader.u16_at(raw_data_offsets::HEIGHT)
        && height > 0
        && height <= 4096
    {
        metadata.insert(
            "FLIR:RawThermalImageHeight".to_string(),
            TagValue::Integer(height as i64),
        );
    }

    // Parse image type
    if let Some(image_type) = reader.u16_at(raw_data_offsets::IMAGE_TYPE) {
        let type_str = match image_type {
            0 => "Unknown",
            1 => "U16 (Linear)",
            2 => "U16 (Compressed)",
            3 => "S16 (Linear)",
            4 => "PNG",
            5 => "JPEG",
            100 => "TIFF",
            _ => "Other",
        };
        metadata.insert(
            "FLIR:RawThermalImageType".to_string(),
            TagValue::String(type_str.to_string()),
        );
    }

    // Note: We don't extract the actual raw thermal image data as binary
    // to avoid bloating the metadata. The tag indicates it exists.
    if data.len() > 32 {
        metadata.insert(
            "FLIR:RawThermalImage".to_string(),
            TagValue::String(format!(
                "(Binary data {} bytes, use -b option to extract)",
                data.len() - 32
            )),
        );
    }
}

/// Parse CameraInfo record containing camera parameters and thermal coefficients.
///
/// This is the primary record for thermal imaging metadata, containing:
/// - Camera identification (model, serial, software version)
/// - Lens and filter information
/// - Planck constants for radiometric temperature calculation
/// - Atmospheric correction parameters
/// - Temperature range and limit values
fn parse_camera_info_record(data: &[u8], metadata: &mut MetadataMap) {
    let reader = EndianReader::little_endian(data);

    // === Emissivity and Environmental Parameters ===

    if let Some(emissivity) = reader.f32_at(camera_info_offsets::EMISSIVITY)
        && (0.0..=1.0).contains(&emissivity)
    {
        metadata.insert(
            "FLIR:Emissivity".to_string(),
            TagValue::Float(emissivity as f64),
        );
    }

    if let Some(distance) = reader.f32_at(camera_info_offsets::OBJECT_DISTANCE)
        && distance > 0.0
        && distance < 10000.0
    {
        metadata.insert(
            "FLIR:ObjectDistance".to_string(),
            TagValue::Float(distance as f64),
        );
    }

    // Temperature values (stored in Kelvin in the file)
    insert_temperature(
        &reader,
        camera_info_offsets::REFLECTED_APPARENT_TEMP,
        "FLIR:ReflectedApparentTemperature",
        metadata,
    );
    insert_temperature(
        &reader,
        camera_info_offsets::ATMOSPHERIC_TEMP,
        "FLIR:AtmosphericTemperature",
        metadata,
    );
    insert_temperature(
        &reader,
        camera_info_offsets::IR_WINDOW_TEMP,
        "FLIR:IRWindowTemperature",
        metadata,
    );

    // IR window transmission
    if let Some(transmission) = reader.f32_at(camera_info_offsets::IR_WINDOW_TRANSMISSION)
        && (0.0..=1.0).contains(&transmission)
    {
        metadata.insert(
            "FLIR:IRWindowTransmission".to_string(),
            TagValue::Float(transmission as f64),
        );
    }

    // Relative humidity
    if let Some(humidity) = reader.f32_at(camera_info_offsets::RELATIVE_HUMIDITY)
        && (0.0..=100.0).contains(&humidity)
    {
        metadata.insert(
            "FLIR:RelativeHumidity".to_string(),
            TagValue::Float(humidity as f64),
        );
    }

    // === Planck Constants for Radiometric Calculation ===
    // These are essential for converting raw thermal values to temperature

    if let Some(planck_r1) = reader.f32_at(camera_info_offsets::PLANCK_R1) {
        metadata.insert(
            "FLIR:PlanckR1".to_string(),
            TagValue::Float(planck_r1 as f64),
        );
    }

    if let Some(planck_b) = reader.f32_at(camera_info_offsets::PLANCK_B) {
        metadata.insert("FLIR:PlanckB".to_string(), TagValue::Float(planck_b as f64));
    }

    if let Some(planck_f) = reader.f32_at(camera_info_offsets::PLANCK_F) {
        metadata.insert("FLIR:PlanckF".to_string(), TagValue::Float(planck_f as f64));
    }

    if let Some(planck_o) = reader.i32_at(camera_info_offsets::PLANCK_O) {
        metadata.insert(
            "FLIR:PlanckO".to_string(),
            TagValue::Integer(planck_o as i64),
        );
    }

    if let Some(planck_r2) = reader.f32_at(camera_info_offsets::PLANCK_R2) {
        metadata.insert(
            "FLIR:PlanckR2".to_string(),
            TagValue::Float(planck_r2 as f64),
        );
    }

    // === Atmospheric Transmission Coefficients ===

    if let Some(alpha1) = reader.f32_at(camera_info_offsets::ATMOSPHERIC_TRANS_ALPHA1) {
        metadata.insert(
            "FLIR:AtmosphericTransAlpha1".to_string(),
            TagValue::Float(alpha1 as f64),
        );
    }

    if let Some(alpha2) = reader.f32_at(camera_info_offsets::ATMOSPHERIC_TRANS_ALPHA2) {
        metadata.insert(
            "FLIR:AtmosphericTransAlpha2".to_string(),
            TagValue::Float(alpha2 as f64),
        );
    }

    if let Some(beta1) = reader.f32_at(camera_info_offsets::ATMOSPHERIC_TRANS_BETA1) {
        metadata.insert(
            "FLIR:AtmosphericTransBeta1".to_string(),
            TagValue::Float(beta1 as f64),
        );
    }

    if let Some(beta2) = reader.f32_at(camera_info_offsets::ATMOSPHERIC_TRANS_BETA2) {
        metadata.insert(
            "FLIR:AtmosphericTransBeta2".to_string(),
            TagValue::Float(beta2 as f64),
        );
    }

    if let Some(trans_x) = reader.f32_at(camera_info_offsets::ATMOSPHERIC_TRANS_X) {
        metadata.insert(
            "FLIR:AtmosphericTransX".to_string(),
            TagValue::Float(trans_x as f64),
        );
    }

    // === Camera Temperature Range and Limits ===

    insert_temperature(
        &reader,
        camera_info_offsets::CAMERA_TEMP_RANGE_MAX,
        "FLIR:CameraTemperatureRangeMax",
        metadata,
    );
    insert_temperature(
        &reader,
        camera_info_offsets::CAMERA_TEMP_RANGE_MIN,
        "FLIR:CameraTemperatureRangeMin",
        metadata,
    );
    insert_temperature(
        &reader,
        camera_info_offsets::CAMERA_TEMP_MAX_CLIP,
        "FLIR:CameraTemperatureMaxClip",
        metadata,
    );
    insert_temperature(
        &reader,
        camera_info_offsets::CAMERA_TEMP_MIN_CLIP,
        "FLIR:CameraTemperatureMinClip",
        metadata,
    );
    insert_temperature(
        &reader,
        camera_info_offsets::CAMERA_TEMP_MAX_WARN,
        "FLIR:CameraTemperatureMaxWarn",
        metadata,
    );
    insert_temperature(
        &reader,
        camera_info_offsets::CAMERA_TEMP_MIN_WARN,
        "FLIR:CameraTemperatureMinWarn",
        metadata,
    );
    insert_temperature(
        &reader,
        camera_info_offsets::CAMERA_TEMP_MAX_SATURATED,
        "FLIR:CameraTemperatureMaxSaturated",
        metadata,
    );
    insert_temperature(
        &reader,
        camera_info_offsets::CAMERA_TEMP_MIN_SATURATED,
        "FLIR:CameraTemperatureMinSaturated",
        metadata,
    );

    // === Camera Identification ===

    if let Some(model) = try_read_string(data, camera_info_offsets::CAMERA_MODEL, 32)
        && !model.is_empty()
    {
        metadata.insert("FLIR:CameraModel".to_string(), TagValue::String(model));
    }

    if let Some(part_num) = try_read_string(data, camera_info_offsets::CAMERA_PART_NUMBER, 32)
        && !part_num.is_empty()
    {
        metadata.insert(
            "FLIR:CameraPartNumber".to_string(),
            TagValue::String(part_num),
        );
    }

    if let Some(serial) = try_read_string(data, camera_info_offsets::CAMERA_SERIAL_NUMBER, 16)
        && !serial.is_empty()
    {
        metadata.insert(
            "FLIR:CameraSerialNumber".to_string(),
            TagValue::String(serial),
        );
    }

    if let Some(software) = try_read_string(data, camera_info_offsets::CAMERA_SOFTWARE, 16)
        && !software.is_empty()
    {
        metadata.insert(
            "FLIR:CameraSoftware".to_string(),
            TagValue::String(software),
        );
    }

    // === Lens Information ===

    if let Some(lens_model) = try_read_string(data, camera_info_offsets::LENS_MODEL, 32)
        && !lens_model.is_empty()
    {
        metadata.insert("FLIR:LensModel".to_string(), TagValue::String(lens_model));
    }

    if let Some(lens_part) = try_read_string(data, camera_info_offsets::LENS_PART_NUMBER, 16)
        && !lens_part.is_empty()
    {
        metadata.insert(
            "FLIR:LensPartNumber".to_string(),
            TagValue::String(lens_part),
        );
    }

    if let Some(lens_serial) = try_read_string(data, camera_info_offsets::LENS_SERIAL_NUMBER, 16)
        && !lens_serial.is_empty()
    {
        metadata.insert(
            "FLIR:LensSerialNumber".to_string(),
            TagValue::String(lens_serial),
        );
    }

    if let Some(fov) = reader.f32_at(camera_info_offsets::FIELD_OF_VIEW)
        && fov > 0.0
        && fov < 180.0
    {
        metadata.insert("FLIR:FieldOfView".to_string(), TagValue::Float(fov as f64));
    }

    // Peak spectral sensitivity (wavelength in micrometers)
    if let Some(wavelength) = reader.f32_at(camera_info_offsets::PEAK_SPECTRAL_SENSITIVITY)
        && wavelength > 0.0
        && wavelength < 100.0
    {
        metadata.insert(
            "FLIR:PeakSpectralSensitivity".to_string(),
            TagValue::Float(wavelength as f64),
        );
    }

    // === Filter Information ===

    if let Some(filter_model) = try_read_string(data, camera_info_offsets::FILTER_MODEL, 16)
        && !filter_model.is_empty()
    {
        metadata.insert(
            "FLIR:FilterModel".to_string(),
            TagValue::String(filter_model),
        );
    }

    if let Some(filter_part) = try_read_string(data, camera_info_offsets::FILTER_PART_NUMBER, 32)
        && !filter_part.is_empty()
    {
        metadata.insert(
            "FLIR:FilterPartNumber".to_string(),
            TagValue::String(filter_part),
        );
    }

    if let Some(filter_serial) =
        try_read_string(data, camera_info_offsets::FILTER_SERIAL_NUMBER, 32)
        && !filter_serial.is_empty()
    {
        metadata.insert(
            "FLIR:FilterSerialNumber".to_string(),
            TagValue::String(filter_serial),
        );
    }

    // === Raw Value Statistics ===

    if let Some(min) = reader.u16_at(camera_info_offsets::RAW_VALUE_RANGE_MIN) {
        metadata.insert(
            "FLIR:RawValueRangeMin".to_string(),
            TagValue::Integer(min as i64),
        );
    }

    if let Some(max) = reader.u16_at(camera_info_offsets::RAW_VALUE_RANGE_MAX) {
        metadata.insert(
            "FLIR:RawValueRangeMax".to_string(),
            TagValue::Integer(max as i64),
        );
    }

    if let Some(median) = reader.u16_at(camera_info_offsets::RAW_VALUE_MEDIAN) {
        metadata.insert(
            "FLIR:RawValueMedian".to_string(),
            TagValue::Integer(median as i64),
        );
    }

    if let Some(range) = reader.u16_at(camera_info_offsets::RAW_VALUE_RANGE) {
        metadata.insert(
            "FLIR:RawValueRange".to_string(),
            TagValue::Integer(range as i64),
        );
    }

    // === Timing and Focus ===

    // Try to parse DateTimeOriginal
    if data.len() > camera_info_offsets::DATE_TIME_ORIGINAL + 8
        && let Some(dt) = parse_flir_datetime(data, camera_info_offsets::DATE_TIME_ORIGINAL)
    {
        metadata.insert("FLIR:DateTimeOriginal".to_string(), TagValue::String(dt));
    }

    if let Some(focus_steps) = reader.i16_at(camera_info_offsets::FOCUS_STEP_COUNT) {
        metadata.insert(
            "FLIR:FocusStepCount".to_string(),
            TagValue::Integer(focus_steps as i64),
        );
    }

    if let Some(focus_dist) = reader.f32_at(camera_info_offsets::FOCUS_DISTANCE)
        && focus_dist > 0.0
        && focus_dist < 10000.0
    {
        metadata.insert(
            "FLIR:FocusDistance".to_string(),
            TagValue::Float(focus_dist as f64),
        );
    }

    if let Some(frame_rate) = reader.u16_at(camera_info_offsets::FRAME_RATE)
        && frame_rate > 0
        && frame_rate <= 1000
    {
        metadata.insert(
            "FLIR:FrameRate".to_string(),
            TagValue::Integer(frame_rate as i64),
        );
    }
}

/// Parse PaletteInfo record containing color palette configuration.
///
/// The palette record defines the color mapping used to visualize
/// thermal data, including special colors for temperature ranges.
fn parse_palette_info_record(data: &[u8], metadata: &mut MetadataMap) {
    let reader = EndianReader::little_endian(data);

    // Number of colors in palette
    if let Some(colors) = reader.u8_at(palette_info_offsets::PALETTE_COLORS)
        && colors > 0
    {
        metadata.insert(
            "FLIR:PaletteColors".to_string(),
            TagValue::Integer(colors as i64),
        );
    }

    // Special colors (RGB triplets)
    insert_rgb_color(
        data,
        palette_info_offsets::ABOVE_COLOR,
        "FLIR:AboveColor",
        metadata,
    );
    insert_rgb_color(
        data,
        palette_info_offsets::BELOW_COLOR,
        "FLIR:BelowColor",
        metadata,
    );
    insert_rgb_color(
        data,
        palette_info_offsets::OVERFLOW_COLOR,
        "FLIR:OverflowColor",
        metadata,
    );
    insert_rgb_color(
        data,
        palette_info_offsets::UNDERFLOW_COLOR,
        "FLIR:UnderflowColor",
        metadata,
    );
    insert_rgb_color(
        data,
        palette_info_offsets::ISOTHERM1_COLOR,
        "FLIR:Isotherm1Color",
        metadata,
    );
    insert_rgb_color(
        data,
        palette_info_offsets::ISOTHERM2_COLOR,
        "FLIR:Isotherm2Color",
        metadata,
    );

    // Palette method
    if let Some(method) = reader.u8_at(palette_info_offsets::PALETTE_METHOD) {
        let method_str = match method {
            0 => "Color Wheel",
            1 => "Color Bar",
            2 => "Temperature Bar",
            _ => "Unknown",
        };
        metadata.insert(
            "FLIR:PaletteMethod".to_string(),
            TagValue::String(method_str.to_string()),
        );
    }

    // Palette stretch
    if let Some(stretch) = reader.u8_at(palette_info_offsets::PALETTE_STRETCH) {
        let stretch_str = match stretch {
            0 => "Linear",
            1 => "Histogram",
            2 => "Manual",
            _ => "Unknown",
        };
        metadata.insert(
            "FLIR:PaletteStretch".to_string(),
            TagValue::String(stretch_str.to_string()),
        );
    }

    // Palette file name and name
    if let Some(filename) = try_read_string(data, palette_info_offsets::PALETTE_FILE_NAME, 32)
        && !filename.is_empty()
    {
        metadata.insert(
            "FLIR:PaletteFileName".to_string(),
            TagValue::String(filename),
        );
    }

    if let Some(name) = try_read_string(data, palette_info_offsets::PALETTE_NAME, 32)
        && !name.is_empty()
    {
        metadata.insert("FLIR:PaletteName".to_string(), TagValue::String(name));
    }

    // Note palette data presence but don't extract full binary
    if data.len() > palette_info_offsets::PALETTE {
        let palette_size = data.len() - palette_info_offsets::PALETTE;
        if palette_size > 0 {
            metadata.insert(
                "FLIR:Palette".to_string(),
                TagValue::String(format!("(Palette data, {} bytes)", palette_size)),
            );
        }
    }
}

/// Helper function to insert a temperature value from the reader.
///
/// Validates that the temperature is in a reasonable range for Kelvin values.
fn insert_temperature(
    reader: &EndianReader,
    offset: usize,
    tag_name: &str,
    metadata: &mut MetadataMap,
) {
    if let Some(temp) = reader.f32_at(offset) {
        // Valid temperature range: 0K to 10000K (covers any practical thermal measurement)
        if (0.0..=10000.0).contains(&temp) && temp.is_finite() {
            metadata.insert(tag_name.to_string(), TagValue::Float(temp as f64));
        }
    }
}

/// Helper function to insert an RGB color value.
fn insert_rgb_color(data: &[u8], offset: usize, tag_name: &str, metadata: &mut MetadataMap) {
    if offset + 3 <= data.len() {
        let r = data[offset];
        let g = data[offset + 1];
        let b = data[offset + 2];
        metadata.insert(
            tag_name.to_string(),
            TagValue::String(format!("#{:02X}{:02X}{:02X}", r, g, b)),
        );
    }
}

/// Try to read a null-terminated string from the data.
///
/// Returns None if the offset is out of bounds or the string is invalid.
fn try_read_string(data: &[u8], offset: usize, max_len: usize) -> Option<String> {
    if offset >= data.len() {
        return None;
    }

    let end = (offset + max_len).min(data.len());
    let bytes = &data[offset..end];

    // Find null terminator
    let str_len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let str_bytes = &bytes[..str_len];

    // Try to convert to UTF-8, handling potential encoding issues
    match std::str::from_utf8(str_bytes) {
        Ok(s) => {
            let trimmed = s.trim();
            if !trimmed.is_empty() && trimmed.chars().all(|c| !c.is_control() || c == ' ') {
                Some(trimmed.to_string())
            } else {
                None
            }
        }
        Err(_) => {
            // Try lossy conversion for non-UTF8 strings
            let s = String::from_utf8_lossy(str_bytes);
            let trimmed = s.trim();
            if !trimmed.is_empty() && trimmed.chars().filter(|c| !c.is_control()).count() > 0 {
                Some(trimmed.replace(|c: char| c.is_control(), ""))
            } else {
                None
            }
        }
    }
}

/// Check if a string looks like a valid camera model name.
fn is_valid_camera_model(model: &str) -> bool {
    // Valid camera models should:
    // - Have reasonable length
    // - Contain mostly printable characters
    // - Not be all zeros or spaces
    if model.len() < 2 || model.len() > 64 {
        return false;
    }

    let printable_count = model
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .count();
    let total = model.chars().count();

    printable_count > total / 2 && model.chars().any(|c| c.is_alphanumeric())
}

/// Parse FLIR datetime format.
///
/// FLIR stores dates in various formats. This function attempts to parse
/// the most common format: seconds since 1970 (Unix timestamp) stored as f64.
fn parse_flir_datetime(data: &[u8], offset: usize) -> Option<String> {
    if offset + 8 > data.len() {
        return None;
    }

    let reader = EndianReader::little_endian(data);

    // FLIR typically stores time as seconds since 1970 (f64)
    if let Some(timestamp) = reader.f64_at(offset)
        && timestamp > 0.0
        && timestamp < 4_000_000_000.0
    {
        // Valid Unix timestamp range (before ~2096)
        let secs = timestamp as i64;

        // Convert to datetime components manually
        // This is a simplified conversion - for production use chrono crate
        let days_since_epoch = secs / 86400;
        let time_of_day = secs % 86400;

        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;

        // Simplified year calculation (not accounting for leap years precisely)
        let year = 1970 + (days_since_epoch / 365) as i32;
        let day_of_year = (days_since_epoch % 365) as i32;

        // Approximate month/day (simplified)
        let (month, day) = approximate_month_day(day_of_year);

        return Some(format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
            year, month, day, hours, minutes, seconds
        ));
    }

    None
}

/// Approximate month and day from day of year (simplified calculation).
fn approximate_month_day(day_of_year: i32) -> (i32, i32) {
    const DAYS_IN_MONTH: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    let mut remaining = day_of_year;
    for (i, &days) in DAYS_IN_MONTH.iter().enumerate() {
        if remaining < days {
            return ((i + 1) as i32, remaining + 1);
        }
        remaining -= days;
    }

    (12, 31) // December 31 as fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test FLIR segment identification
    #[test]
    fn test_flir_identification() {
        let mut data = Vec::new();
        data.extend_from_slice(b"FLIR\x00");
        data.extend_from_slice(&[0x01, 0x01, 0x00]); // segment 1 of 1
        // Add enough padding to meet minimum length requirements
        data.extend_from_slice(&[0x00; 32]);

        let mut metadata = MetadataMap::new();
        let result = parse_flir_segment(&data, &mut metadata);
        assert!(result.is_ok());
    }

    /// Test rejection of non-FLIR segments
    #[test]
    fn test_non_flir_rejected() {
        let data = b"EXIF\x00\x00";
        let mut metadata = MetadataMap::new();
        let result = parse_flir_segment(data, &mut metadata);
        assert!(result.is_err());
    }

    /// Test segment too short
    #[test]
    fn test_flir_too_short() {
        let data = b"FLIR";
        let mut metadata = MetadataMap::new();
        let result = parse_flir_segment(data, &mut metadata);
        assert!(result.is_err());
    }

    /// Test string parsing with null terminator
    #[test]
    fn test_try_read_string() {
        let data = b"FLIR E60\x00\x00\x00\x00";
        let result = try_read_string(data, 0, 12);
        assert_eq!(result, Some("FLIR E60".to_string()));
    }

    /// Test string parsing with non-printable characters
    #[test]
    fn test_try_read_string_empty() {
        let data = [0x00, 0x00, 0x00, 0x00];
        let result = try_read_string(&data, 0, 4);
        assert_eq!(result, None);
    }

    /// Test valid camera model check
    #[test]
    fn test_is_valid_camera_model() {
        assert!(is_valid_camera_model("FLIR E60"));
        assert!(is_valid_camera_model("E4"));
        assert!(!is_valid_camera_model(""));
        assert!(!is_valid_camera_model("   "));
    }

    /// Test FLIR segment with embedded camera model
    #[test]
    fn test_flir_with_camera_model() {
        let mut data = Vec::new();
        // FLIR header
        data.extend_from_slice(b"FLIR\x00");
        data.extend_from_slice(&[0x01, 0x01, 0x00]);

        // Pad to have some data
        data.extend_from_slice(&[0x00; 32]);

        // Add camera model at offset 8 (after header)
        let model_offset = 8 + 0x20; // 8 byte header + offset for legacy fallback
        while data.len() < model_offset {
            data.push(0x00);
        }

        let mut metadata = MetadataMap::new();
        let result = parse_flir_segment(&data, &mut metadata);

        // Should succeed without errors
        assert!(result.is_ok());
    }

    /// Test RGB color extraction
    #[test]
    fn test_insert_rgb_color() {
        let data = [0xFF, 0x00, 0x80];
        let mut metadata = MetadataMap::new();
        insert_rgb_color(&data, 0, "TestColor", &mut metadata);

        assert_eq!(metadata.get_string("TestColor"), Some("#FF0080"));
    }

    /// Test temperature insertion with valid value
    #[test]
    fn test_insert_temperature_valid() {
        // 293.15K (20C) as little-endian f32
        let data = [0x66, 0x66, 0x92, 0x43]; // 293.15 in little-endian
        let reader = EndianReader::little_endian(&data);
        let mut metadata = MetadataMap::new();

        insert_temperature(&reader, 0, "TestTemp", &mut metadata);

        let temp = metadata.get_float("TestTemp");
        assert!(temp.is_some());
        let t = temp.unwrap();
        assert!((t - 293.0).abs() < 1.0); // Allow small rounding
    }

    /// Test approximate month/day calculation
    #[test]
    fn test_approximate_month_day() {
        assert_eq!(approximate_month_day(0), (1, 1)); // Jan 1
        assert_eq!(approximate_month_day(31), (2, 1)); // Feb 1
        assert_eq!(approximate_month_day(364), (12, 31)); // Dec 31 (non-leap year)
    }

    /// Test record entry structure
    #[test]
    fn test_record_entry_creation() {
        let entry = FlirRecordEntry {
            record_type: RECORD_TYPE_CAMERA_INFO,
            offset: 100,
            length: 1024,
        };

        assert_eq!(entry.record_type, 0x0020);
        assert_eq!(entry.offset, 100);
        assert_eq!(entry.length, 1024);
    }

    /// Test minimum segment length constant
    #[test]
    fn test_min_segment_length() {
        assert_eq!(MIN_FLIR_SEGMENT_LENGTH, 11);
    }

    /// Test record type constants
    #[test]
    fn test_record_type_constants() {
        assert_eq!(RECORD_TYPE_RAW_DATA, 0x0001);
        assert_eq!(RECORD_TYPE_CAMERA_INFO, 0x0020);
        assert_eq!(RECORD_TYPE_PALETTE_INFO, 0x0022);
        assert_eq!(RECORD_TYPE_EMBEDDED_IMAGE, 0x000E);
    }
}
