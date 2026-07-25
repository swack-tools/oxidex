//! Multi-Picture Format (MPF) parser for JPEG APP2 segments
//!
//! MPF is used in dual-camera phones and 3D cameras to store multiple images
//! in a single JPEG file. The MPF data is stored in an APP2 segment with the
//! "MPF\x00" identifier, followed by a TIFF-like IFD structure.
//!
//! # MPF Structure
//!
//! ```text
//! APP2 marker (0xFFE2)
//! Length (2 bytes, big-endian)
//! "MPF\x00" identifier (4 bytes)
//! TIFF header (8 bytes: byte order + magic 42 + IFD offset)
//! MP Index IFD (IFD 0) - contains MPFVersion, NumberOfImages, MPEntry
//! MP Attribute IFD (per image) - contains positioning/3D metadata
//! ```
//!
//! # References
//!
//! - CIPA DC-007-2009 Multi-Picture Format Specification

use crate::core::{MetadataMap, TagValue};
use crate::io::EndianReader;

// =============================================================================
// Constants: MPF Tag IDs
// =============================================================================

/// MPF Version tag (MP Index IFD)
const MPF_VERSION: u16 = 0xB000;
/// Number of Images tag (MP Index IFD)
const NUMBER_OF_IMAGES: u16 = 0xB001;
/// MP Entry tag (MP Index IFD) - contains image info array
const MP_ENTRY: u16 = 0xB002;
/// Image UID List tag (MP Index IFD)
const IMAGE_UID_LIST: u16 = 0xB003;
/// Total Frames tag (MP Index IFD)
const TOTAL_FRAMES: u16 = 0xB004;

/// MP Individual Number tag (MP Attribute IFD)
const MP_INDIVIDUAL_NUM: u16 = 0xB101;
/// Panorama Orientation tag (MP Attribute IFD)
const PAN_ORIENTATION: u16 = 0xB201;
/// Panorama Horizontal Overlap tag (MP Attribute IFD)
const PAN_OVERLAP_H: u16 = 0xB202;
/// Panorama Vertical Overlap tag (MP Attribute IFD)
const PAN_OVERLAP_V: u16 = 0xB203;
/// Base Viewpoint Number tag (MP Attribute IFD)
const BASE_VIEWPOINT_NUM: u16 = 0xB204;
/// Convergence Angle tag (MP Attribute IFD)
const CONVERGENCE_ANGLE: u16 = 0xB205;
/// Baseline Length tag (MP Attribute IFD)
const BASELINE_LENGTH: u16 = 0xB206;
/// Vertical Divergence tag (MP Attribute IFD)
const VERTICAL_DIVERGENCE: u16 = 0xB207;
/// Axis Distance X tag (MP Attribute IFD)
const AXIS_DISTANCE_X: u16 = 0xB208;
/// Axis Distance Y tag (MP Attribute IFD)
const AXIS_DISTANCE_Y: u16 = 0xB209;
/// Axis Distance Z tag (MP Attribute IFD)
const AXIS_DISTANCE_Z: u16 = 0xB20A;
/// Yaw Angle tag (MP Attribute IFD)
const YAW_ANGLE: u16 = 0xB20B;
/// Pitch Angle tag (MP Attribute IFD)
const PITCH_ANGLE: u16 = 0xB20C;
/// Roll Angle tag (MP Attribute IFD)
const ROLL_ANGLE: u16 = 0xB20D;

// =============================================================================
// MPF Byte Order Detection
// =============================================================================

/// Byte order for MPF TIFF-like structure
#[derive(Debug, Clone, Copy, PartialEq)]
enum MpfByteOrder {
    LittleEndian,
    BigEndian,
}

// =============================================================================
// Public API
// =============================================================================

/// Parses an MPF APP2 segment and extracts Multi-Picture Format metadata.
///
/// MPF segments start with the "MPF\x00" identifier followed by a TIFF-like
/// IFD structure containing:
/// - MP Index IFD with version, image count, and entry array
/// - Optional MP Attribute IFDs for each image with positioning metadata
///
/// # Arguments
///
/// * `data` - Raw APP2 segment data (should start with "MPF\x00")
/// * `metadata` - MetadataMap to populate with extracted MPF tags
///
/// # Returns
///
/// * `Ok(())` - Successfully parsed MPF data
/// * `Err(String)` - Parse error with description
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::mpf_parser::parse_mpf_segment;
/// use oxidex::core::MetadataMap;
///
/// let mut metadata = MetadataMap::new();
/// parse_mpf_segment(app2_data, &mut metadata)?;
/// ```
pub fn parse_mpf_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    // Minimum size: 4 (identifier) + 8 (TIFF header) = 12 bytes
    if data.len() < 12 {
        return Err("MPF segment too short".to_string());
    }

    // Verify MPF identifier "MPF\x00"
    if &data[0..4] != b"MPF\0" {
        return Err("Not an MPF segment (invalid identifier)".to_string());
    }

    // TIFF-like structure starts at offset 4 (after "MPF\0")
    let tiff_data = &data[4..];

    // Detect byte order from TIFF header (bytes 0-1)
    let byte_order = detect_byte_order(&tiff_data[0..2])?;

    // Create EndianReader based on detected byte order
    let reader = match byte_order {
        MpfByteOrder::LittleEndian => EndianReader::little_endian(tiff_data),
        MpfByteOrder::BigEndian => EndianReader::big_endian(tiff_data),
    };

    // Verify TIFF magic number 42 (bytes 2-3)
    let magic = reader.u16_at(2).ok_or("Failed to read TIFF magic")?;
    if magic != 42 {
        return Err(format!(
            "Invalid MPF TIFF magic number: expected 42, got {}",
            magic
        ));
    }

    // Read MP Index IFD offset (bytes 4-7)
    let mp_index_ifd_offset = reader.u32_at(4).ok_or("Failed to read IFD offset")? as usize;

    // Parse MP Index IFD
    parse_mp_index_ifd(&reader, mp_index_ifd_offset, metadata)?;

    Ok(())
}

// =============================================================================
// Internal Functions
// =============================================================================

/// Detects the byte order from the TIFF header marker.
///
/// # Arguments
///
/// * `marker` - 2-byte slice containing "II" (little-endian) or "MM" (big-endian)
///
/// # Returns
///
/// * `Ok(MpfByteOrder)` - Detected byte order
/// * `Err(String)` - Invalid byte order marker
fn detect_byte_order(marker: &[u8]) -> Result<MpfByteOrder, String> {
    match marker {
        b"II" => Ok(MpfByteOrder::LittleEndian),
        b"MM" => Ok(MpfByteOrder::BigEndian),
        _ => Err(format!(
            "Invalid MPF byte order marker: {:02X} {:02X}",
            marker.first().unwrap_or(&0),
            marker.get(1).unwrap_or(&0)
        )),
    }
}

/// Parses the MP Index IFD (IFD 0) containing MPF version, image count, and entry data.
///
/// # Arguments
///
/// * `reader` - EndianReader with correct byte order for the MPF data
/// * `offset` - Offset to the start of the IFD within the TIFF structure
/// * `metadata` - MetadataMap to populate with tags
fn parse_mp_index_ifd(
    reader: &EndianReader,
    offset: usize,
    metadata: &mut MetadataMap,
) -> Result<(), String> {
    // Read IFD entry count (2 bytes)
    let entry_count = reader
        .u16_at(offset)
        .ok_or("Failed to read MP Index IFD entry count")? as usize;

    // Each IFD entry is 12 bytes
    // Structure: tag (2) + type (2) + count (4) + value/offset (4)
    let mut mp_entry_data: Option<Vec<u8>> = None;
    let mut mp_entry_count: usize = 0;

    for i in 0..entry_count {
        let entry_offset = offset + 2 + (i * 12);

        // Read tag ID
        let tag_id = reader.u16_at(entry_offset).ok_or("Failed to read tag ID")?;
        // Read field type
        let field_type = reader
            .u16_at(entry_offset + 2)
            .ok_or("Failed to read field type")?;
        // Read value count
        let value_count = reader
            .u32_at(entry_offset + 4)
            .ok_or("Failed to read value count")? as usize;
        // Read value/offset (4 bytes)
        let value_or_offset = reader
            .u32_at(entry_offset + 8)
            .ok_or("Failed to read value/offset")?;

        match tag_id {
            MPF_VERSION => {
                // MPFVersion is typically 4 bytes representing "0100" (version 1.0)
                let version = parse_mpf_version(reader, value_count, value_or_offset)?;
                metadata.insert("MPF:MPFVersion".to_string(), TagValue::String(version));
            }
            NUMBER_OF_IMAGES => {
                // NumberOfImages is a LONG (4 bytes)
                metadata.insert(
                    "MPF:NumberOfImages".to_string(),
                    TagValue::Integer(value_or_offset as i64),
                );
            }
            MP_ENTRY => {
                // MPEntry is an array of 16-byte structures (UNDEFINED type)
                // Value count is total bytes, offset points to data
                let data_offset = value_or_offset as usize;
                let data_size = value_count;

                // Store for later processing
                if let Some(bytes) = reader.bytes_at(data_offset, data_size) {
                    mp_entry_data = Some(bytes.to_vec());
                    // Each MP Entry is 16 bytes
                    mp_entry_count = data_size / 16;
                }
            }
            IMAGE_UID_LIST => {
                // ImageUIDList - 33 bytes per image (UNDEFINED type)
                // Match ExifTool format exactly (no comma)
                metadata.insert(
                    "MPF:ImageUIDList".to_string(),
                    TagValue::String(format!(
                        "(Binary data {} bytes, use -b option to extract)",
                        value_count
                    )),
                );
            }
            TOTAL_FRAMES => {
                // TotalFrames - LONG
                metadata.insert(
                    "MPF:TotalFrames".to_string(),
                    TagValue::Integer(value_or_offset as i64),
                );
            }
            _ => {
                // Unknown tag in MP Index IFD
                let tag_name = format!("MPF:0x{:04X}", tag_id);
                let value =
                    parse_generic_ifd_value(reader, field_type, value_count, value_or_offset);
                metadata.insert(tag_name, value);
            }
        }
    }

    // Process MP Entry array if present
    if let Some(entry_data) = mp_entry_data {
        parse_mp_entry_array(&entry_data, mp_entry_count, reader, metadata)?;
    }

    // Check for MP Attribute IFD offset (after IFD entries + next IFD pointer)
    let next_ifd_offset_pos = offset + 2 + (entry_count * 12);
    if let Some(attr_ifd_offset) = reader.u32_at(next_ifd_offset_pos)
        && attr_ifd_offset > 0
    {
        // Parse MP Attribute IFD (IFD 1). This is optional/supplementary data;
        // some cameras (e.g. Fujifilm) write a malformed or absent Attribute IFD,
        // so a failure here should not invalidate the Index IFD data already parsed.
        let _ = parse_mp_attribute_ifd(reader, attr_ifd_offset as usize, metadata);
    }

    Ok(())
}

/// Parses the MPFVersion tag value into a string.
///
/// The version is stored as 4 ASCII characters (e.g., "0100" for version 1.0).
/// Per ExifTool compatibility, we output the raw 4-character string as-is.
///
/// # Arguments
///
/// * `reader` - EndianReader for accessing the data
/// * `value_count` - Number of bytes in the version field
/// * `value_or_offset` - Either the inline value or offset to data
fn parse_mpf_version(
    reader: &EndianReader,
    value_count: usize,
    value_or_offset: u32,
) -> Result<String, String> {
    // Version is 4 ASCII bytes: "0100" = version 1.0
    // ExifTool outputs the raw format "0100", not "1.0"
    if value_count <= 4 {
        // Value is inline in the 4-byte field
        let bytes = value_or_offset.to_le_bytes();
        if let Ok(s) = std::str::from_utf8(&bytes[..value_count]) {
            // Return raw version string (e.g., "0100") for ExifTool compatibility
            return Ok(s.trim_end_matches('\0').to_string());
        }
    } else {
        // Value is at offset
        let offset = value_or_offset as usize;
        if let Some(bytes) = reader.bytes_at(offset, value_count.min(4))
            && let Ok(s) = std::str::from_utf8(bytes)
        {
            // Return raw version string for ExifTool compatibility
            return Ok(s.trim_end_matches('\0').to_string());
        }
    }
    Ok("Unknown".to_string())
}

/// Parses the MP Entry array containing individual image information.
///
/// Each MP Entry is 16 bytes:
/// - Bytes 0-3: Individual Image Attribute (flags, format, type)
/// - Bytes 4-7: Individual Image Size
/// - Bytes 8-11: Individual Image Data Offset
/// - Bytes 12-13: Dependent Image 1 Entry Number
/// - Bytes 14-15: Dependent Image 2 Entry Number
///
/// This function outputs both:
/// 1. Numbered per-image tags (MPF:MPImage1Flags, MPF:MPImage2Flags, etc.)
/// 2. Generic tags for the last non-primary image (MPF:MPImageFlags, etc.)
///    which matches ExifTool's behavior for compatibility.
///
/// # Arguments
///
/// * `data` - Raw bytes of the MP Entry array
/// * `count` - Number of entries in the array
/// * `reader` - EndianReader for byte order handling
/// * `metadata` - MetadataMap to populate with per-image tags
fn parse_mp_entry_array(
    data: &[u8],
    count: usize,
    reader: &EndianReader,
    metadata: &mut MetadataMap,
) -> Result<(), String> {
    // Create reader with same byte order as the main data
    let entry_reader = EndianReader::new(data, reader.byte_order());

    // Track the last non-primary image for generic tags (ExifTool compatibility)
    let mut last_non_primary_idx: Option<usize> = None;

    // First pass: identify the last non-primary image
    for i in 0..count {
        let entry_offset = i * 16;
        if entry_offset + 16 > data.len() {
            break;
        }
        let image_attr = entry_reader.u32_at(entry_offset).unwrap_or(0);
        let image_type = image_attr & 0x00FFFFFF;
        // Non-primary images are anything that's not 0x030000 (Baseline MP Primary Image)
        if image_type != 0x030000 {
            last_non_primary_idx = Some(i);
        }
    }

    for i in 0..count {
        let entry_offset = i * 16;

        if entry_offset + 16 > data.len() {
            break;
        }

        // Read Individual Image Attribute (4 bytes)
        let image_attr = entry_reader
            .u32_at(entry_offset)
            .ok_or("Failed to read image attribute")?;

        // Read Individual Image Size (4 bytes)
        let image_size = entry_reader
            .u32_at(entry_offset + 4)
            .ok_or("Failed to read image size")?;

        // Read Individual Image Data Offset (4 bytes)
        let image_offset = entry_reader
            .u32_at(entry_offset + 8)
            .ok_or("Failed to read image offset")?;

        // Read Dependent Image 1 Entry Number (2 bytes)
        let dep_image1 = entry_reader
            .u16_at(entry_offset + 12)
            .ok_or("Failed to read dependent image 1")?;

        // Read Dependent Image 2 Entry Number (2 bytes)
        let dep_image2 = entry_reader
            .u16_at(entry_offset + 14)
            .ok_or("Failed to read dependent image 2")?;

        // Parse image attribute bits
        // Bits 31-30: Dependent Parent/Child Image Flag
        // Bit 29: Representative Image Flag
        // Bits 26-24: Image Data Format (0=JPEG)
        // Bits 23-0: Type (defined by format)
        let dep_flag = (image_attr >> 30) & 0x03;
        let representative = (image_attr >> 29) & 0x01;
        let data_format = (image_attr >> 24) & 0x07;
        let image_type = image_attr & 0x00FFFFFF;

        let entry_prefix = format!("MPF:MPImage{}", i + 1);

        // Image Flags interpretation - numbered format
        let dep_flag_str = match dep_flag {
            0 => "Independent",
            1 => "Dependent parent",
            2 => "Dependent child",
            3 => "Both dependent parent and child",
            _ => "Unknown",
        };
        metadata.insert(
            format!("{}Flags", entry_prefix),
            TagValue::String(dep_flag_str.to_string()),
        );

        // Representative image
        if representative == 1 {
            metadata.insert(
                format!("{}Representative", entry_prefix),
                TagValue::String("Yes".to_string()),
            );
        }

        // Image format
        let format_str = match data_format {
            0 => "JPEG",
            _ => "Unknown",
        };
        metadata.insert(
            format!("{}Format", entry_prefix),
            TagValue::String(format_str.to_string()),
        );

        // Image type interpretation
        let type_str = decode_image_type(image_type);
        metadata.insert(
            format!("{}Type", entry_prefix),
            TagValue::String(type_str.clone()),
        );

        // Image size
        metadata.insert(
            format!("{}Size", entry_prefix),
            TagValue::Integer(image_size as i64),
        );

        // Image offset (0 for first image = start of JPEG)
        metadata.insert(
            format!("{}Offset", entry_prefix),
            TagValue::Integer(image_offset as i64),
        );

        // Dependent image entry numbers (0 = none)
        if dep_image1 != 0 {
            metadata.insert(
                format!("{}DependentImage1", entry_prefix),
                TagValue::Integer(dep_image1 as i64),
            );
        }
        if dep_image2 != 0 {
            metadata.insert(
                format!("{}DependentImage2", entry_prefix),
                TagValue::Integer(dep_image2 as i64),
            );
        }

        // Output generic MPImage tags for the last non-primary image
        // (ExifTool compatibility - it outputs these for the "current" image)
        if Some(i) == last_non_primary_idx {
            // Generic flags format uses "image" suffix like ExifTool
            let generic_flag_str = match dep_flag {
                0 => "(none)",
                1 => "Dependent parent image",
                2 => "Dependent child image",
                3 => "Both dependent parent and child image",
                _ => "Unknown",
            };
            metadata.insert(
                "MPF:MPImageFlags".to_string(),
                TagValue::String(generic_flag_str.to_string()),
            );
            metadata.insert(
                "MPF:MPImageFormat".to_string(),
                TagValue::String(format_str.to_string()),
            );

            // Generic type uses different format (VGA equivalent, full HD equivalent)
            let generic_type_str = decode_image_type_generic(image_type);
            metadata.insert(
                "MPF:MPImageType".to_string(),
                TagValue::String(generic_type_str),
            );

            // MPImageLength and MPImageStart match ExifTool naming
            metadata.insert(
                "MPF:MPImageLength".to_string(),
                TagValue::Integer(image_size as i64),
            );
            metadata.insert(
                "MPF:MPImageStart".to_string(),
                TagValue::Integer(image_offset as i64),
            );

            // Always output DependentImageNEntryNumber for generic tags
            metadata.insert(
                "MPF:DependentImage1EntryNumber".to_string(),
                TagValue::Integer(dep_image1 as i64),
            );
            metadata.insert(
                "MPF:DependentImage2EntryNumber".to_string(),
                TagValue::Integer(dep_image2 as i64),
            );
        }
    }

    Ok(())
}

/// Decodes the image type field into ExifTool's generic tag format.
///
/// This uses the "VGA equivalent" / "full HD equivalent" format that ExifTool
/// uses for generic MPImageType tags (as opposed to numbered MPImage1Type etc).
///
/// # Arguments
///
/// * `image_type` - 24-bit image type code
fn decode_image_type_generic(image_type: u32) -> String {
    match image_type {
        0x000000 => "Undefined".to_string(),
        0x010001 => "Large Thumbnail (VGA equivalent)".to_string(),
        0x010002 => "Large Thumbnail (full HD equivalent)".to_string(),
        0x020001 => "Multi-Frame Panorama".to_string(),
        0x020002 => "Multi-Frame Disparity".to_string(),
        0x020003 => "Multi-Angle".to_string(),
        0x030000 => "Baseline MP Primary Image".to_string(),
        _ => format!("Unknown (0x{:06X})", image_type),
    }
}

/// Decodes the image type field from MP Entry into a human-readable string.
///
/// # Arguments
///
/// * `image_type` - 24-bit image type code
///
/// # Returns
///
/// Human-readable image type description matching ExifTool format
fn decode_image_type(image_type: u32) -> String {
    // For JPEG format, type codes are defined as per CIPA DC-007-2009:
    // 0x000000 = Undefined
    // 0x010001 = Large thumbnail (VGA equivalent / class 1)
    // 0x010002 = Large thumbnail (full HD equivalent / class 2)
    // 0x020001 = Multi-frame panorama
    // 0x020002 = Multi-frame disparity
    // 0x020003 = Multi-angle
    // 0x030000 = Baseline MP primary image
    //
    // Note: ExifTool outputs "Large Thumbnail (VGA equivalent)" for class 1
    // and "Large Thumbnail (full HD equivalent)" for class 2 in generic
    // MPImage tags, but uses "Large Thumbnail (Class 1)" and "Large Thumbnail
    // (Class 2)" in numbered MPImageN tags. We use the Class format for
    // consistency with numbered tags.
    match image_type {
        0x000000 => "Undefined".to_string(),
        0x010001 => "Large Thumbnail (Class 1)".to_string(),
        0x010002 => "Large Thumbnail (Class 2)".to_string(),
        0x020001 => "Multi-Frame Panorama".to_string(),
        0x020002 => "Multi-Frame Disparity".to_string(),
        0x020003 => "Multi-Angle".to_string(),
        0x030000 => "Baseline MP Primary Image".to_string(),
        _ => format!("Unknown (0x{:06X})", image_type),
    }
}

/// Parses the MP Attribute IFD containing per-image positioning and 3D metadata.
///
/// # Arguments
///
/// * `reader` - EndianReader for accessing the data
/// * `offset` - Offset to the start of the Attribute IFD
/// * `metadata` - MetadataMap to populate with attribute tags
fn parse_mp_attribute_ifd(
    reader: &EndianReader,
    offset: usize,
    metadata: &mut MetadataMap,
) -> Result<(), String> {
    // Read IFD entry count
    let entry_count = reader
        .u16_at(offset)
        .ok_or("Failed to read MP Attribute IFD entry count")? as usize;

    for i in 0..entry_count {
        let entry_offset = offset + 2 + (i * 12);

        let tag_id = reader.u16_at(entry_offset).ok_or("Failed to read tag ID")?;
        let field_type = reader
            .u16_at(entry_offset + 2)
            .ok_or("Failed to read field type")?;
        let value_count = reader
            .u32_at(entry_offset + 4)
            .ok_or("Failed to read value count")? as usize;
        let value_or_offset = reader
            .u32_at(entry_offset + 8)
            .ok_or("Failed to read value/offset")?;

        let tag_name = match tag_id {
            MP_INDIVIDUAL_NUM => "MPF:MPIndividualNum".to_string(),
            PAN_ORIENTATION => "MPF:PanOrientation".to_string(),
            PAN_OVERLAP_H => "MPF:PanOverlapH".to_string(),
            PAN_OVERLAP_V => "MPF:PanOverlapV".to_string(),
            BASE_VIEWPOINT_NUM => "MPF:BaseViewpointNum".to_string(),
            CONVERGENCE_ANGLE => "MPF:ConvergenceAngle".to_string(),
            BASELINE_LENGTH => "MPF:BaselineLength".to_string(),
            VERTICAL_DIVERGENCE => "MPF:VerticalDivergence".to_string(),
            AXIS_DISTANCE_X => "MPF:AxisDistanceX".to_string(),
            AXIS_DISTANCE_Y => "MPF:AxisDistanceY".to_string(),
            AXIS_DISTANCE_Z => "MPF:AxisDistanceZ".to_string(),
            YAW_ANGLE => "MPF:YawAngle".to_string(),
            PITCH_ANGLE => "MPF:PitchAngle".to_string(),
            ROLL_ANGLE => "MPF:RollAngle".to_string(),
            _ => format!("MPF:0x{:04X}", tag_id),
        };

        let value = parse_generic_ifd_value(reader, field_type, value_count, value_or_offset);
        metadata.insert(tag_name, value);
    }

    Ok(())
}

/// Parses a generic IFD value based on field type.
///
/// # Arguments
///
/// * `reader` - EndianReader for accessing the data
/// * `field_type` - TIFF field type (1=BYTE, 2=ASCII, 3=SHORT, 4=LONG, 5=RATIONAL, etc.)
/// * `value_count` - Number of values
/// * `value_or_offset` - Either the inline value or offset to data
fn parse_generic_ifd_value(
    reader: &EndianReader,
    field_type: u16,
    value_count: usize,
    value_or_offset: u32,
) -> TagValue {
    // Calculate total bytes needed
    let bytes_per_value = match field_type {
        1 | 2 | 7 => 1, // BYTE, ASCII, UNDEFINED
        3 => 2,         // SHORT
        4 | 9 => 4,     // LONG, SLONG
        5 | 10 => 8,    // RATIONAL, SRATIONAL
        _ => 1,
    };
    let total_bytes = bytes_per_value * value_count;

    // Value is inline if it fits in 4 bytes
    if total_bytes <= 4 {
        match field_type {
            1 => TagValue::Integer((value_or_offset & 0xFF) as i64),
            3 => TagValue::Integer((value_or_offset & 0xFFFF) as i64),
            4 => TagValue::Integer(value_or_offset as i64),
            9 => TagValue::Integer(value_or_offset as i32 as i64),
            2 => {
                // ASCII - inline string
                let bytes = value_or_offset.to_le_bytes();
                std::str::from_utf8(&bytes[..value_count.min(4)])
                    .map(|s| TagValue::String(s.trim_end_matches('\0').to_string()))
                    .unwrap_or_else(|_| TagValue::Integer(value_or_offset as i64))
            }
            _ => TagValue::Integer(value_or_offset as i64),
        }
    } else {
        // Value is at offset
        let offset = value_or_offset as usize;
        match field_type {
            2 => {
                // ASCII string
                reader
                    .cstr_at(offset, value_count)
                    .map(|s| TagValue::String(s.to_string()))
                    .unwrap_or_else(|| TagValue::String("(invalid)".to_string()))
            }
            5 => {
                // RATIONAL - unsigned
                reader
                    .rational_at(offset)
                    .map(|(num, denom)| {
                        if denom != 0 {
                            TagValue::Rational {
                                numerator: num as i32,
                                denominator: denom as i32,
                            }
                        } else {
                            TagValue::Integer(0)
                        }
                    })
                    .unwrap_or_else(|| TagValue::Integer(0))
            }
            10 => {
                // SRATIONAL - signed
                reader
                    .srational_at(offset)
                    .map(|(num, denom)| {
                        if denom != 0 {
                            TagValue::Rational {
                                numerator: num,
                                denominator: denom,
                            }
                        } else {
                            TagValue::Integer(0)
                        }
                    })
                    .unwrap_or_else(|| TagValue::Integer(0))
            }
            _ => {
                // Binary data
                reader
                    .bytes_at(offset, value_count)
                    .map(|bytes| TagValue::Binary(bytes.to_vec()))
                    .unwrap_or_else(|| TagValue::Binary(vec![]))
            }
        }
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a minimal valid MPF segment with "MPF\x00" identifier and TIFF header.
    fn create_minimal_mpf_segment() -> Vec<u8> {
        let mut data = Vec::new();

        // "MPF\x00" identifier
        data.extend_from_slice(b"MPF\0");

        // TIFF header (little-endian)
        data.extend_from_slice(b"II"); // Byte order mark
        data.extend_from_slice(&42u16.to_le_bytes()); // Magic number 42
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD offset (after header)

        // MP Index IFD starts at offset 8 (from TIFF start)
        // IFD entry count: 2 entries
        data.extend_from_slice(&2u16.to_le_bytes());

        // Entry 1: MPFVersion (0xB000)
        data.extend_from_slice(&MPF_VERSION.to_le_bytes()); // Tag ID
        data.extend_from_slice(&2u16.to_le_bytes()); // Type: ASCII
        data.extend_from_slice(&4u32.to_le_bytes()); // Count
        data.extend_from_slice(b"0100"); // Value: "0100"

        // Entry 2: NumberOfImages (0xB001)
        data.extend_from_slice(&NUMBER_OF_IMAGES.to_le_bytes()); // Tag ID
        data.extend_from_slice(&4u16.to_le_bytes()); // Type: LONG
        data.extend_from_slice(&1u32.to_le_bytes()); // Count
        data.extend_from_slice(&2u32.to_le_bytes()); // Value: 2 images

        // Next IFD offset (0 = no more IFDs)
        data.extend_from_slice(&0u32.to_le_bytes());

        data
    }

    /// Creates an MPF segment with MP Entry data for testing.
    fn create_mpf_segment_with_entries() -> Vec<u8> {
        let mut data = Vec::new();

        // "MPF\x00" identifier
        data.extend_from_slice(b"MPF\0");

        // TIFF header (little-endian)
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD offset

        // MP Index IFD at offset 8
        // 3 entries
        data.extend_from_slice(&3u16.to_le_bytes());

        // Entry 1: MPFVersion
        data.extend_from_slice(&MPF_VERSION.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes()); // ASCII
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(b"0100");

        // Entry 2: NumberOfImages
        data.extend_from_slice(&NUMBER_OF_IMAGES.to_le_bytes());
        data.extend_from_slice(&4u16.to_le_bytes()); // LONG
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes()); // 2 images

        // Entry 3: MPEntry (offset to entry array)
        let mp_entry_offset = 8 + 2 + (3 * 12) + 4; // After IFD header + entries + next IFD ptr
        data.extend_from_slice(&MP_ENTRY.to_le_bytes());
        data.extend_from_slice(&7u16.to_le_bytes()); // UNDEFINED
        data.extend_from_slice(&32u32.to_le_bytes()); // 32 bytes (2 entries)
        data.extend_from_slice(&(mp_entry_offset as u32).to_le_bytes());

        // Next IFD offset (0 = no more IFDs)
        data.extend_from_slice(&0u32.to_le_bytes());

        // MP Entry array (2 entries of 16 bytes each)
        // Entry 1: Primary image (representative)
        let attr1: u32 = 0x20000000 | 0x030000; // Representative + Baseline MP Primary
        data.extend_from_slice(&attr1.to_le_bytes()); // Image attribute
        data.extend_from_slice(&100000u32.to_le_bytes()); // Size
        data.extend_from_slice(&0u32.to_le_bytes()); // Offset (0 for first image)
        data.extend_from_slice(&0u16.to_le_bytes()); // Dependent image 1
        data.extend_from_slice(&0u16.to_le_bytes()); // Dependent image 2

        // Entry 2: Thumbnail
        let attr2: u32 = 0x010001; // Large Thumbnail (class 1)
        data.extend_from_slice(&attr2.to_le_bytes());
        data.extend_from_slice(&50000u32.to_le_bytes());
        data.extend_from_slice(&100000u32.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());

        data
    }

    #[test]
    fn test_parse_minimal_mpf_segment() {
        let data = create_minimal_mpf_segment();
        let mut metadata = MetadataMap::new();

        let result = parse_mpf_segment(&data, &mut metadata);
        assert!(result.is_ok(), "Failed to parse: {:?}", result);

        // Check MPFVersion - should be raw "0100" format for ExifTool compatibility
        assert!(
            metadata.contains_key("MPF:MPFVersion"),
            "Missing MPFVersion"
        );
        assert_eq!(
            metadata.get_string("MPF:MPFVersion"),
            Some("0100"),
            "MPFVersion should be raw '0100' format"
        );

        // Check NumberOfImages
        assert_eq!(
            metadata.get_integer("MPF:NumberOfImages"),
            Some(2),
            "Wrong NumberOfImages"
        );
    }

    #[test]
    fn test_parse_mpf_segment_with_entries() {
        let data = create_mpf_segment_with_entries();
        let mut metadata = MetadataMap::new();

        let result = parse_mpf_segment(&data, &mut metadata);
        assert!(result.is_ok(), "Failed to parse: {:?}", result);

        // Check that numbered MP entry tags were parsed
        assert!(
            metadata.contains_key("MPF:MPImage1Type"),
            "Missing MPImage1Type"
        );
        assert!(
            metadata.contains_key("MPF:MPImage2Type"),
            "Missing MPImage2Type"
        );

        // Check image sizes (numbered tags)
        assert_eq!(
            metadata.get_integer("MPF:MPImage1Size"),
            Some(100000),
            "Wrong MPImage1Size"
        );
        assert_eq!(
            metadata.get_integer("MPF:MPImage2Size"),
            Some(50000),
            "Wrong MPImage2Size"
        );

        // Check generic tags (ExifTool compatibility) - should be from last non-primary image
        // Entry 2 is a Large Thumbnail (class 1), so generic tags should reflect that
        assert!(
            metadata.contains_key("MPF:MPImageFlags"),
            "Missing generic MPImageFlags"
        );
        assert!(
            metadata.contains_key("MPF:MPImageFormat"),
            "Missing generic MPImageFormat"
        );
        assert!(
            metadata.contains_key("MPF:MPImageType"),
            "Missing generic MPImageType"
        );
        assert!(
            metadata.contains_key("MPF:MPImageLength"),
            "Missing generic MPImageLength"
        );
        assert!(
            metadata.contains_key("MPF:MPImageStart"),
            "Missing generic MPImageStart"
        );

        // Verify generic tag values match the second entry (thumbnail)
        assert_eq!(
            metadata.get_integer("MPF:MPImageLength"),
            Some(50000),
            "Wrong MPImageLength"
        );
        assert_eq!(
            metadata.get_integer("MPF:MPImageStart"),
            Some(100000),
            "Wrong MPImageStart"
        );
        assert_eq!(
            metadata.get_string("MPF:MPImageType"),
            Some("Large Thumbnail (VGA equivalent)"),
            "Wrong MPImageType"
        );
        assert_eq!(
            metadata.get_string("MPF:MPImageFlags"),
            Some("(none)"),
            "Wrong MPImageFlags"
        );
    }

    #[test]
    fn test_parse_invalid_identifier() {
        let data = b"NOT_MPF\0II*\0\x08\0\0\0";
        let mut metadata = MetadataMap::new();

        let result = parse_mpf_segment(data, &mut metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not an MPF segment"));
    }

    #[test]
    fn test_parse_too_short() {
        let data = b"MPF\0II*";
        let mut metadata = MetadataMap::new();

        let result = parse_mpf_segment(data, &mut metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too short"));
    }

    #[test]
    fn test_parse_invalid_byte_order() {
        let mut data = create_minimal_mpf_segment();
        // Corrupt byte order marker
        data[4] = b'X';
        data[5] = b'X';

        let mut metadata = MetadataMap::new();
        let result = parse_mpf_segment(&data, &mut metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("byte order"));
    }

    #[test]
    fn test_parse_big_endian_mpf() {
        let mut data = Vec::new();

        // "MPF\x00" identifier
        data.extend_from_slice(b"MPF\0");

        // TIFF header (big-endian)
        data.extend_from_slice(b"MM");
        data.extend_from_slice(&42u16.to_be_bytes());
        data.extend_from_slice(&8u32.to_be_bytes());

        // IFD entry count: 1
        data.extend_from_slice(&1u16.to_be_bytes());

        // Entry: NumberOfImages
        data.extend_from_slice(&NUMBER_OF_IMAGES.to_be_bytes());
        data.extend_from_slice(&4u16.to_be_bytes()); // LONG
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&3u32.to_be_bytes()); // 3 images

        // Next IFD offset
        data.extend_from_slice(&0u32.to_be_bytes());

        let mut metadata = MetadataMap::new();
        let result = parse_mpf_segment(&data, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(metadata.get_integer("MPF:NumberOfImages"), Some(3));
    }

    #[test]
    fn test_decode_image_type() {
        assert_eq!(decode_image_type(0x000000), "Undefined");
        assert_eq!(decode_image_type(0x010001), "Large Thumbnail (Class 1)");
        assert_eq!(decode_image_type(0x020001), "Multi-Frame Panorama");
        assert_eq!(decode_image_type(0x030000), "Baseline MP Primary Image");
        assert!(decode_image_type(0xFFFFFF).contains("Unknown"));
    }

    #[test]
    fn test_detect_byte_order() {
        assert_eq!(
            detect_byte_order(b"II").unwrap(),
            MpfByteOrder::LittleEndian
        );
        assert_eq!(detect_byte_order(b"MM").unwrap(), MpfByteOrder::BigEndian);
        assert!(detect_byte_order(b"XX").is_err());
    }
}
