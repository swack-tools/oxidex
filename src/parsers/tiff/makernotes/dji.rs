//! DJI Drone MakerNote parser
//!
//! Parses DJI-specific EXIF MakerNote tags from aerial drones including
//! Mavic, Phantom, and Inspire series. Contains flight telemetry, GPS coordinates,
//! altitude, gimbal orientation, and camera settings.
//!
//! ## Supported Models
//! - Mavic Series (Mini, Air, Pro, Enterprise)
//! - Phantom Series (3, 4, 4 Pro)
//! - Inspire Series (1, 2, RAW)
//! - Osmo Series (handheld gimbals)
//! - Zenmuse Camera Series
//!
//! ## Key Features
//! - GPS coordinates (latitude, longitude, altitude)
//! - Flight speed and direction
//! - Gimbal pitch, roll, yaw
//! - Home point distance
//! - Battery level and voltage
//! - Camera exposure and ISO
//! - Flight mode (GPS, ATTI, Sport)
//! - Obstacle avoidance status
//!
//! ## Architecture
//! DJI stores flight data in a proprietary binary format within MakerNotes.
//! Most values are stored as 32-bit integers or floats with specific scaling factors.
//!
//! ## Code Duplication Reduction
//! This module uses the TagRegistry pattern to eliminate repetitive match arms.
//! Previously, the parse() method contained 30+ nearly-identical match cases,
//! resulting in 113% code duplication. The registry pattern consolidates all tag
//! definitions into a single static registry, reducing duplication to near-zero
//! while maintaining full functionality.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_i16_array;
use super::shared::tag_registry::TagRegistry;

use super::registries::dji::dji_registry;

// ============================================================================
// Tag ID Constants
// ============================================================================

// DJI MakerNote Tag IDs
// Based on reverse engineering of DJI drone JPEG files
// Public constants are exported for use in the registry module
const DJI_MAKE: u16 = 0x0001; // Manufacturer name "DJI"
const DJI_MODEL: u16 = 0x0003; // Drone model (e.g., "FC6310")
const DJI_FIRMWARE_VERSION: u16 = 0x0004; // Firmware version string
const DJI_SERIAL_NUMBER: u16 = 0x000A; // Drone serial number
const DJI_FLIGHT_DATA: u16 = 0x0100; // Flight telemetry array
/// GPS latitude (signed int, scale: 1e-7)
pub const DJI_GPS_LATITUDE: u16 = 0x0101;
/// GPS longitude (signed int, scale: 1e-7)
pub const DJI_GPS_LONGITUDE: u16 = 0x0102;
/// Absolute altitude MSL (meters)
pub const DJI_GPS_ALTITUDE: u16 = 0x0103;
/// Relative altitude from takeoff (meters)
pub const DJI_RELATIVE_ALTITUDE: u16 = 0x0104;
/// Gimbal pitch angle (degrees, -90 to +30)
pub const DJI_GIMBAL_PITCH: u16 = 0x0105;
/// Gimbal roll angle (degrees)
pub const DJI_GIMBAL_ROLL: u16 = 0x0106;
/// Gimbal yaw angle (degrees, 0-360)
pub const DJI_GIMBAL_YAW: u16 = 0x0107;
/// Ground speed (m/s)
pub const DJI_FLIGHT_SPEED: u16 = 0x0108;
/// Flight direction (degrees, 0-360)
pub const DJI_FLIGHT_DIRECTION: u16 = 0x0109;
/// Aircraft yaw/heading (degrees)
pub const DJI_AIRCRAFT_YAW: u16 = 0x010A;
/// Aircraft pitch (degrees)
pub const DJI_AIRCRAFT_PITCH: u16 = 0x010B;
/// Aircraft roll (degrees)
pub const DJI_AIRCRAFT_ROLL: u16 = 0x010C;
/// Distance from home point (meters)
pub const DJI_HOME_DISTANCE: u16 = 0x010D;
/// Battery percentage (0-100)
pub const DJI_BATTERY_LEVEL: u16 = 0x010E;
/// Battery voltage (millivolts)
pub const DJI_BATTERY_VOLTAGE: u16 = 0x010F;
/// Flight time (seconds)
pub const DJI_FLIGHT_TIME: u16 = 0x0110;
/// Flight mode code
pub const DJI_FLIGHT_MODE: u16 = 0x0111;
/// GPS signal strength (0-5)
pub const DJI_GPS_SIGNAL: u16 = 0x0112;
/// Number of GPS satellites
pub const DJI_SATELLITE_COUNT: u16 = 0x0113;
/// Obstacle avoidance status
pub const DJI_OBSTACLE_AVOID: u16 = 0x0114;
/// Camera ISO value
pub const DJI_CAMERA_ISO: u16 = 0x0115;
/// Shutter speed (1/n)
pub const DJI_CAMERA_SHUTTER: u16 = 0x0116;
/// Aperture f-number (f/n)
pub const DJI_CAMERA_APERTURE: u16 = 0x0117;
/// Exposure compensation (EV)
pub const DJI_CAMERA_EV: u16 = 0x0118;
/// White balance mode
pub const DJI_CAMERA_WB: u16 = 0x0119;
/// Image format (JPEG/RAW/DNG)
pub const DJI_IMAGE_FORMAT: u16 = 0x011A;
/// Color mode (Normal/D-Cinelike/D-Log)
pub const DJI_COLOR_MODE: u16 = 0x011B;
/// Hasselblad camera flag
pub const DJI_HASSELBLAD: u16 = 0x011C;
const DJI_DEWARP_DATA: u16 = 0x011D; // Lens distortion correction data
const DJI_HYPERLAPSE_MODE: u16 = 0x011E; // Hyperlapse/Timelapse mode

/// DJI Phantom-style MakerNote attitude/velocity fields.  ExifTool's
/// `DJI::Main` declares these as inline `float` values, not the scaled
/// integer telemetry fields used by newer DJI models.
const DJI_FLOAT_FIELDS: &[(u16, &str)] = &[
    (0x0003, "SpeedX"),
    (0x0004, "SpeedY"),
    (0x0005, "SpeedZ"),
    (0x0006, "Pitch"),
    (0x0007, "Yaw"),
    (0x0008, "Roll"),
    (0x0009, "CameraPitch"),
    (0x000a, "CameraYaw"),
    (0x000b, "CameraRoll"),
];

// DJI signature for validation
const DJI_SIGNATURE: &[u8] = b"DJI";

// ============================================================================
// Static Tag Registry
// ============================================================================
// Tag registry and decoders are now in registries/dji.rs module

/// Static registry containing all DJI MakerNote tag definitions
///
/// This Lazy-initialized registry is created from the dji_registry() function
/// in the registries module, which maps tag IDs to their names and decoders.
static DJI_TAGS: Lazy<TagRegistry> = Lazy::new(dji_registry);

// ============================================================================
// Value Extraction Helpers
// ============================================================================

/// Extracts a 32-bit signed integer from IFD entry
///
/// Handles both inline values (stored in value_offset field) and
/// offset-based values (read from data buffer). Supports both byte orders.
///
/// # Arguments
/// * `entry` - IFD entry containing the value
/// * `data` - Raw MakerNote data
/// * `byte_order` - Byte order for reading
///
/// # Returns
/// Extracted i32 value or None if extraction fails
fn extract_i32(entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> Option<i32> {
    // For LONG/SLONG types with count=1, value might be inline
    if entry.value_count == 1 && (entry.field_type == 4 || entry.field_type == 9) {
        // LONG (4) or SLONG (9) - value is inline in value_offset field
        return Some(entry.value_offset as i32);
    }

    // Read from offset in data buffer using EndianReader
    let offset = entry.value_offset as usize;
    let reader = EndianReader::new(data, byte_order.to_io_byte_order());
    reader.i32_at(offset)
}

/// Extracts an ASCII string from IFD entry
///
/// Handles both inline strings (count <= 4, stored in value_offset) and
/// offset-based strings (read from data buffer). Automatically strips
/// null terminators and validates UTF-8 encoding.
///
/// # Arguments
/// * `entry` - IFD entry containing the string
/// * `data` - Raw MakerNote data
///
/// # Returns
/// Extracted string or None if extraction fails or string is empty
fn extract_string(entry: &IfdEntry, data: &[u8]) -> Option<String> {
    if entry.field_type != 2 {
        // Not ASCII type
        return None;
    }

    let offset = entry.value_offset as usize;
    let count = entry.value_count as usize;

    if count <= 4 {
        // Inline string in value_offset field
        let bytes = entry.value_offset.to_le_bytes();
        let s = String::from_utf8_lossy(&bytes[..count.min(4)])
            .trim_end_matches('\0')
            .to_string();
        return if s.is_empty() { None } else { Some(s) };
    }

    // String at offset in data buffer
    if offset + count > data.len() {
        return None;
    }

    let s = String::from_utf8_lossy(&data[offset..offset + count])
        .trim_end_matches('\0')
        .to_string();

    if s.is_empty() { None } else { Some(s) }
}

/// One `DJI::Info` record value, as `ProcessDJIInfo` (DJI.pm:960-983,
/// pinned 13.59) hands it to `HandleTag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DjiInfoValue {
    /// `$val =~ /^([\x20-\x7e]+)\0*$/` matched: the printable run, with the
    /// trailing NULs (and a final newline `$` allows) dropped.
    Text(String),
    /// Anything else stays a binary scalar reference, printed by ExifTool as
    /// `(Binary data N bytes, use -b option to extract)`.
    Binary(Vec<u8>),
}

impl DjiInfoValue {
    /// ExifTool's printed form of the value.
    pub fn print(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Binary(bytes) => format!(
                "(Binary data {} bytes, use -b option to extract)",
                bytes.len()
            ),
        }
    }
}

/// `%Image::ExifTool::DJI::Info` (DJI.pm:74-92, pinned 13.59): the record
/// names ExifTool knows. Anything else is named by `HandleTag`'s
/// `MakeTagInfo` path -- see [`make_tag_info_name`].
const DJI_INFO_TAGS: &[(&[u8], &str)] = &[
    (b"ae_dbg_info", "AEDebugInfo"),
    (b"ae_histogram_info", "AEHistogramInfo"),
    (b"ae_local_histogram", "AELocalHistogram"),
    (b"ae_liveview_histogram_info", "AELiveViewHistogramInfo"),
    (b"ae_liveview_local_histogram", "AELiveViewLocalHistogram"),
    (b"awb_dbg_info", "AWBDebugInfo"),
    (b"af_dbg_info", "AFDebugInfo"),
    (b"hiso", "Histogram"),
    (b"xidiri", "Xidiri"),
    (b"GimbalDegree(Y,P,R)", "GimbalDegree"),
    (b"FlightDegree(Y,P,R)", "FlightDegree"),
    (b"adj_dbg_info", "ADJDebugInfo"),
    (b"sensor_id", "SensorID"),
    (b"FlightSpeed(X,Y,Z)", "FlightSpeed"),
    (b"hyperlapse_dbg_info", "HyperlapsDebugInfo"),
];

/// The tag name `HandleTag($tagTbl, $tag, $val, MakeTagInfo => 1)` gives a
/// record ID the table does not declare (ExifTool.pm:9310-9318, then
/// `AddTagToTable`'s own clean-up at ExifTool.pm:9254-9265). DJI_FC9313.jpg's
/// `awb_dbg_data_v2` becomes `Awb_Dbg_Data_V2`.
fn make_tag_info_name(tag: &[u8]) -> String {
    // s/([A-Z]) ([A-Z][ A-Z])/${1}_$2/g -- underline between acronyms.
    let mut step1 = Vec::with_capacity(tag.len());
    let mut i = 0;
    while i < tag.len() {
        if i + 3 < tag.len()
            && tag[i].is_ascii_uppercase()
            && tag[i + 1] == b' '
            && tag[i + 2].is_ascii_uppercase()
            && (tag[i + 3] == b' ' || tag[i + 3].is_ascii_uppercase())
        {
            step1.extend_from_slice(&[tag[i], b'_', tag[i + 2], tag[i + 3]]);
            i += 4;
        } else {
            step1.push(tag[i]);
            i += 1;
        }
    }
    // s/([^A-Za-z])([a-z])/$1\u$2/g -- capitalize words.
    let mut step2 = Vec::with_capacity(step1.len());
    let mut i = 0;
    while i < step1.len() {
        if i + 1 < step1.len()
            && !step1[i].is_ascii_alphabetic()
            && step1[i + 1].is_ascii_lowercase()
        {
            step2.extend_from_slice(&[step1[i], step1[i + 1].to_ascii_uppercase()]);
            i += 2;
        } else {
            step2.push(step1[i]);
            i += 1;
        }
    }
    // tr/-_a-zA-Z0-9//dc -- remove illegal characters.
    let mut name: String = step2
        .into_iter()
        .filter(|byte| *byte == b'-' || *byte == b'_' || byte.is_ascii_alphanumeric())
        .map(char::from)
        .collect();
    if name.len() < 2 || name.starts_with(|c: char| c == '-' || c.is_ascii_digit()) {
        name.insert_str(0, "Tag");
    }
    // ucfirst, then AddTagToTable's "must start with a letter" rule.
    if let Some(first) = name.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    if name.len() < 2 || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name.insert_str(0, "Tag");
    }
    name
}

/// Whether a record's closing `]` at `close` satisfies `(?=(\[|$))`: the
/// next byte opens another record, or the string ends there -- Perl's `$`
/// also matches just before one final newline.
fn closes_record(data: &[u8], close: usize) -> bool {
    match data.get(close + 1) {
        None | Some(b'[') => true,
        Some(b'\n') => close + 2 == data.len(),
        Some(_) => false,
    }
}

/// `ProcessDJIInfo` (DJI.pm:960-983, pinned 13.59) over one directory:
/// `while ($$dataPt =~ /\G\[(.*?)\](?=(\[|$))/sg)`. Records must be
/// contiguous from the start; each is the shortest `[...]` whose `]` is
/// followed by the next `[` or the end, so a payload may itself contain `]`
/// and its length is never assumed. Each record splits once at `:` into the
/// tag ID and its value; a record with no `:` is skipped.
///
/// Returns `(tag name, value)` in record order.
pub fn process_dji_info(data: &[u8]) -> Vec<(String, DjiInfoValue)> {
    let mut records = Vec::new();
    let mut pos = 0;
    while data.get(pos) == Some(&b'[') {
        let Some(close) =
            (pos + 1..data.len()).find(|&index| data[index] == b']' && closes_record(data, index))
        else {
            break;
        };
        let record = &data[pos + 1..close];
        pos = close + 1;

        // my ($tag, $val) = split /:/, $1, 2; next unless defined $val;
        let Some(colon) = record.iter().position(|byte| *byte == b':') else {
            continue;
        };
        let (tag, value) = (&record[..colon], &record[colon + 1..]);

        let name = DJI_INFO_TAGS
            .iter()
            .find(|(id, _)| *id == tag)
            .map_or_else(|| make_tag_info_name(tag), |(_, name)| (*name).to_string());
        records.push((name, dji_info_value(value)));
    }
    records
}

/// `$val =~ /^([\x20-\x7e]+)\0*$/ ? $1 : \$val` (DJI.pm:974-979).
fn dji_info_value(value: &[u8]) -> DjiInfoValue {
    let body = value.strip_suffix(b"\n").unwrap_or(value);
    let printable_end = body
        .iter()
        .position(|byte| !(0x20..=0x7e).contains(byte))
        .unwrap_or(body.len());
    if printable_end > 0 && body[printable_end..].iter().all(|byte| *byte == 0) {
        DjiInfoValue::Text(String::from_utf8_lossy(&body[..printable_end]).into_owned())
    } else {
        DjiInfoValue::Binary(value.to_vec())
    }
}

// ============================================================================
// DJI MakerNote Parser Implementation
// ============================================================================

/// DJI MakerNote parser implementing the MakerNoteParser trait
///
/// Parses DJI drone-specific metadata from MakerNote IFD entries.
/// Extracts flight telemetry, GPS data, gimbal angles, camera settings,
/// and other drone-specific information using the TagRegistry pattern
/// for efficient, maintainable tag handling.
#[derive(Default)]
pub struct DjiParser;

impl DjiParser {
    /// Creates a new DJI parser instance
    pub fn new() -> Self {
        DjiParser
    }

    /// Parse a single IFD entry and extract tag value
    ///
    /// This method uses the DJI_TAGS registry to eliminate repetitive match arms.
    /// Instead of 30+ individual match cases (113% duplication), all tags are handled
    /// through centralized registry lookups, reducing duplication to near-zero.
    ///
    /// The method handles three types of DJI tags:
    /// 1. String tags (make, model, firmware, serial number)
    /// 2. i32 tags (GPS coordinates, altitudes)
    /// 3. i16 tags (gimbal angles, flight data, camera settings)
    ///
    /// # Arguments
    /// * `entry` - IFD entry to parse
    /// * `data` - Full MakerNote data buffer
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - HashMap to insert extracted tags into
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        // DJI::Main 0x0003..0x000b is a compact, ordinary TIFF IFD used by
        // Phantom-era cameras such as the FC330.  The four value bytes are
        // the IEEE-754 float itself; treating them as a value offset (or as
        // integer telemetry) loses all nine fields.  Its PrintConv is
        // `sprintf("%+.2f", $val)` in the pinned ExifTool 13.59 table.
        if entry.field_type == 11 && entry.value_count == 1 {
            if let Some((_, tag_name)) = DJI_FLOAT_FIELDS
                .iter()
                .find(|(tag_id, _)| *tag_id == entry.tag_id)
            {
                let value = f32::from_bits(entry.value_offset);
                tags.insert(format!("DJI:{tag_name}"), format!("{value:+.2}"));
                return;
            }
        }

        // Handle string tags separately (not in registry)
        match entry.tag_id {
            DJI_MAKE | DJI_MODEL | DJI_FIRMWARE_VERSION | DJI_SERIAL_NUMBER => {
                if let Some(s) = extract_string(entry, data) {
                    let tag_name = match entry.tag_id {
                        DJI_MAKE => "Make",
                        DJI_MODEL => "Model",
                        DJI_FIRMWARE_VERSION => "FirmwareVersion",
                        DJI_SERIAL_NUMBER => "SerialNumber",
                        _ => return,
                    };
                    tags.insert(format!("DJI:{}", tag_name), s);
                }
                return;
            }
            _ => {}
        }

        // Check if this tag is registered in our tag registry
        if let Some(tag_name) = DJI_TAGS.get_tag_name(entry.tag_id) {
            // Try i32 extraction first (for GPS coordinates and altitudes)
            if let Some(value) = extract_i32(entry, data, byte_order) {
                let decoded = DJI_TAGS.decode_i32(entry.tag_id, value);
                tags.insert(format!("DJI:{}", tag_name), decoded);
                return;
            }

            // Try i16 array extraction (most DJI tags)
            if let Some(array) = extract_i16_array(entry, data, byte_order)
                && let Some(&value) = array.first()
            {
                let decoded = DJI_TAGS.decode_i16(entry.tag_id, value);
                tags.insert(format!("DJI:{}", tag_name), decoded);
            }
        }
        // Unknown tags are silently skipped for forward compatibility
    }
}

impl MakerNoteParser for DjiParser {
    fn manufacturer_name(&self) -> &'static str {
        "DJI"
    }

    fn tag_prefix(&self) -> &'static str {
        "DJI:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Check for DJI signature in first bytes
        if data.len() < 3 {
            return false;
        }
        // Accept if DJI signature present OR if data is at least 8 bytes
        // (some DJI files omit the signature)
        data.starts_with(DJI_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // MakerNotes.pm:93-98 `MakerNoteDJIInfo`: a note starting
        // `[ae_dbg_info:` is the whole `DJI::Info` record stream (FC2204,
        // Mavic 2 Enterprise), not a TIFF-style IFD.
        if data.starts_with(b"[ae_dbg_info:") {
            for (name, value) in process_dji_info(data) {
                tags.insert(format!("DJI:{name}"), value.print());
            }
            return Ok(());
        }

        if data.len() < 8 {
            return Err("DJI MakerNote data too short".to_string());
        }

        // Skip DJI signature if present
        let start_offset = if data.starts_with(DJI_SIGNATURE) {
            3
        } else {
            0
        };
        let parse_data = &data[start_offset..];

        // Parse TIFF-style IFD entries
        if parse_data.len() < 2 {
            return Ok(());
        }

        // Read number of entries using EndianReader
        let reader = EndianReader::new(parse_data, byte_order.to_io_byte_order());
        let num_entries = reader.u16_at(0).unwrap_or(0) as usize;

        // Sanity check on entry count
        if num_entries == 0 || num_entries > 200 {
            return Ok(());
        }

        let mut offset = 2;
        let entry_size = 12; // Standard IFD entry size

        // Process each IFD entry
        for _ in 0..num_entries {
            if offset + entry_size > parse_data.len() {
                break;
            }

            let entry_data = &parse_data[offset..offset + entry_size];
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

            // Parse IFD entry fields using EndianReader
            let tag = entry_reader.u16_at(0).unwrap_or(0);
            let field_type = entry_reader.u16_at(2).unwrap_or(0);
            let count = entry_reader.u32_at(4).unwrap_or(0);
            let value_offset = entry_reader.u32_at(8).unwrap_or(0);

            let entry = IfdEntry {
                tag_id: tag,
                field_type,
                value_count: count,
                value_offset,
            };

            self.parse_entry(&entry, parse_data, byte_order, tags);

            offset += entry_size;
        }

        Ok(())
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::tiff::makernotes::registries::dji::{
        COLOR_MODE, FLIGHT_MODE, GPS_SIGNAL, IMAGE_FORMAT, OBSTACLE_AVOIDANCE, WHITE_BALANCE,
        decode_obstacle_avoidance, format_altitude, format_aperture, format_battery_level,
        format_degrees, format_ev, format_flight_time, format_gimbal_angle, format_gps_coordinate,
        format_iso, format_meters, format_shutter_speed, format_speed, format_voltage,
    };

    /// DJI::Main 0x0003..0x000b are inline `float` values with ExifTool's
    /// `sprintf("%+.2f")` PrintConv; the four value bytes are the float
    /// itself, already decoded in the file's byte order into `value_offset`.
    #[test]
    fn test_inline_float_fields_use_signed_two_decimal_print_conv() {
        let parser = DjiParser::new();
        let mut tags = HashMap::new();
        for (tag_id, value) in [(0x0005u16, -0.3f32), (0x0007, 32.0), (0x0009, -179.7)] {
            let entry = IfdEntry {
                tag_id,
                field_type: 11,
                value_count: 1,
                value_offset: value.to_bits(),
            };
            parser.parse_entry(&entry, &[], ByteOrder::LittleEndian, &mut tags);
        }
        assert_eq!(tags.get("DJI:SpeedZ").map(String::as_str), Some("-0.30"));
        assert_eq!(tags.get("DJI:Yaw").map(String::as_str), Some("+32.00"));
        assert_eq!(
            tags.get("DJI:CameraPitch").map(String::as_str),
            Some("-179.70")
        );
    }

    /// Only a single inline `float` takes the float arm; any other format for
    /// the same IDs falls through to the existing handling, unchanged.
    #[test]
    fn test_float_arm_requires_single_float_format() {
        let parser = DjiParser::new();
        let mut tags = HashMap::new();
        let entry = IfdEntry {
            tag_id: 0x0006,
            field_type: 4,
            value_count: 1,
            value_offset: (-4.1f32).to_bits(),
        };
        parser.parse_entry(&entry, &[], ByteOrder::LittleEndian, &mut tags);
        assert!(!tags.values().any(|v| v == "-4.10"));
    }

    #[test]
    fn test_dji_parser_creation() {
        let parser = DjiParser::new();
        assert_eq!(parser.manufacturer_name(), "DJI");
        assert_eq!(parser.tag_prefix(), "DJI:");
    }

    #[test]
    fn test_flight_mode_decoder() {
        assert_eq!(FLIGHT_MODE.decode(0), "Manual");
        assert_eq!(FLIGHT_MODE.decode(2), "GPS");
        assert_eq!(FLIGHT_MODE.decode(4), "Sport");
        assert_eq!(FLIGHT_MODE.decode(10), "Return to Home");
        assert_eq!(FLIGHT_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_white_balance_decoder() {
        assert_eq!(WHITE_BALANCE.decode(0), "Auto");
        assert_eq!(WHITE_BALANCE.decode(1), "Sunny");
        assert_eq!(WHITE_BALANCE.decode(2), "Cloudy");
        assert_eq!(WHITE_BALANCE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_color_mode_decoder() {
        assert_eq!(COLOR_MODE.decode(0), "Normal");
        assert_eq!(COLOR_MODE.decode(1), "D-Cinelike");
        assert_eq!(COLOR_MODE.decode(2), "D-Log");
        assert_eq!(COLOR_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_image_format_decoder() {
        assert_eq!(IMAGE_FORMAT.decode(0), "JPEG");
        assert_eq!(IMAGE_FORMAT.decode(1), "RAW");
        assert_eq!(IMAGE_FORMAT.decode(2), "JPEG + RAW");
        assert_eq!(IMAGE_FORMAT.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_gps_signal_decoder() {
        assert_eq!(GPS_SIGNAL.decode(0), "None");
        assert_eq!(GPS_SIGNAL.decode(3), "Good");
        assert_eq!(GPS_SIGNAL.decode(5), "Excellent");
        assert_eq!(GPS_SIGNAL.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_diagnostic_packet_stream() {
        fn append_packet(data: &mut Vec<u8>, prefix: &[u8], payload_len: usize) {
            data.push(b'[');
            data.extend_from_slice(prefix);
            data.resize(data.len() + payload_len, 0);
            data.push(b']');
        }

        let mut data = Vec::new();
        append_packet(&mut data, b"ae_dbg_info:", 256);
        append_packet(&mut data, b"ae_histogram_info:", 4096);
        append_packet(&mut data, b"ae_liveview_histogram_info:", 4096);
        append_packet(&mut data, b"ae_liveview_local_histogram:", 2048);
        append_packet(&mut data, b"adj_dbg_info:", 1024);

        let mut tags = HashMap::new();
        assert!(
            DjiParser
                .parse(&data, ByteOrder::LittleEndian, &mut tags)
                .is_ok()
        );

        assert_eq!(
            tags.get("DJI:AEDebugInfo").map(String::as_str),
            Some("(Binary data 256 bytes, use -b option to extract)")
        );
        assert_eq!(
            tags.get("DJI:AEHistogramInfo").map(String::as_str),
            Some("(Binary data 4096 bytes, use -b option to extract)")
        );
        assert_eq!(
            tags.get("DJI:AELiveViewHistogramInfo").map(String::as_str),
            Some("(Binary data 4096 bytes, use -b option to extract)")
        );
        assert_eq!(
            tags.get("DJI:AELiveViewLocalHistogram").map(String::as_str),
            Some("(Binary data 2048 bytes, use -b option to extract)")
        );
        assert_eq!(
            tags.get("DJI:ADJDebugInfo").map(String::as_str),
            Some("(Binary data 1024 bytes, use -b option to extract)")
        );
        assert_eq!(tags.len(), 5);
    }

    /// ExifTool.pm:9310-9318 + AddTagToTable: names for record IDs the
    /// `DJI::Info` table does not declare. Each expectation was checked
    /// against the same substitutions run by the pinned perl 5.38.2.
    #[test]
    fn make_tag_info_names_follow_handle_tag() {
        assert_eq!(make_tag_info_name(b"awb_dbg_data_v2"), "Awb_Dbg_Data_V2");
        assert_eq!(make_tag_info_name(b"scap_info"), "Scap_Info");
        assert_eq!(make_tag_info_name(b"sisr_info"), "Sisr_Info");
        assert_eq!(make_tag_info_name(b"A B CD"), "A_BCD");
        assert_eq!(make_tag_info_name(b"x(y)z"), "XYZ");
        assert_eq!(make_tag_info_name(b"9lives"), "Tag9Lives");
        assert_eq!(make_tag_info_name(b"_abc"), "Tag_Abc");
        assert_eq!(make_tag_info_name(b""), "Tag");
        assert_eq!(make_tag_info_name(b"q"), "Tagq");
    }

    /// DJI.pm:971-980: contiguous records only, shortest `]` followed by `[`
    /// or the end (or a final newline), split once at `:`, printable values
    /// keep text with trailing NULs dropped. `AB]x` is closed by the `]`
    /// before the final newline, not the one inside it.
    #[test]
    fn process_dji_info_follows_the_record_regex() {
        let records = process_dji_info(
            b"[FlightSpeed(X,Y,Z):53,-55,0][hiso:disable, ][nocolon][sensor_id:AB]x]\n",
        );
        assert_eq!(
            records,
            vec![
                (
                    "FlightSpeed".to_string(),
                    DjiInfoValue::Text("53,-55,0".to_string())
                ),
                (
                    "Histogram".to_string(),
                    DjiInfoValue::Text("disable, ".to_string())
                ),
                (
                    "SensorID".to_string(),
                    DjiInfoValue::Text("AB]x".to_string())
                ),
            ]
        );
        // Not contiguous from the start: nothing (`\G` anchors at 0).
        assert!(process_dji_info(b" [sensor_id:AB]").is_empty());
        // An unterminated record ends the loop but keeps earlier records.
        assert_eq!(process_dji_info(b"[sensor_id:AB][xidiri:1").len(), 1);
        // A printable value followed by NULs and a final newline is text.
        assert_eq!(
            process_dji_info(b"[sensor_id:AB\0\0\n]"),
            vec![("SensorID".to_string(), DjiInfoValue::Text("AB".to_string()))]
        );
        // An empty value is a zero-length binary scalar.
        assert_eq!(
            process_dji_info(b"[xidiri:]")[0].1.print(),
            "(Binary data 0 bytes, use -b option to extract)"
        );
    }

    /// Only a note starting `[ae_dbg_info:` is `DJI::Info`
    /// (MakerNotes.pm:93-98); records elsewhere in an IFD note are not.
    #[test]
    fn dji_info_makernote_requires_the_ae_dbg_info_prefix() {
        let mut tags = HashMap::new();
        let mut data = vec![0u8; 8];
        data.extend_from_slice(b"[sensor_id:AB]");
        DjiParser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .expect("parses");
        assert!(tags.get("DJI:SensorID").is_none());
    }

    #[test]
    fn test_obstacle_avoidance_decoder() {
        assert_eq!(decode_obstacle_avoidance(0), "Disabled");
        assert_eq!(OBSTACLE_AVOIDANCE.decode(0x01), "Front");
        assert_eq!(OBSTACLE_AVOIDANCE.decode(0x03), "Front, Back");
        assert_eq!(
            OBSTACLE_AVOIDANCE.decode(0x3F),
            "Front, Back, Left, Right, Top, Bottom"
        );
    }

    #[test]
    fn test_format_gps_coordinate() {
        assert_eq!(format_gps_coordinate(377_123_456), "37.7123456");
        assert_eq!(format_gps_coordinate(-1_221_234_567), "-122.1234567");
    }

    #[test]
    fn test_format_altitude() {
        assert_eq!(format_altitude(12000), "120.00 m");
        assert_eq!(format_altitude(5050), "50.50 m");
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(1500), "15.00 m/s");
        assert_eq!(format_speed(0), "0.00 m/s");
    }

    #[test]
    fn test_format_gimbal_angle() {
        assert_eq!(format_gimbal_angle(-900), "-90.0°");
        assert_eq!(format_gimbal_angle(0), "0.0°");
        assert_eq!(format_gimbal_angle(300), "30.0°");
    }

    #[test]
    fn test_format_voltage() {
        assert_eq!(format_voltage(15400), "15.40 V");
        assert_eq!(format_voltage(12600), "12.60 V");
    }

    #[test]
    fn test_format_flight_time() {
        assert_eq!(format_flight_time(125), "2:05");
        assert_eq!(format_flight_time(3661), "61:01");
        assert_eq!(format_flight_time(0), "0:00");
    }

    #[test]
    fn test_format_aperture() {
        assert_eq!(format_aperture(28), "f/2.8");
        assert_eq!(format_aperture(40), "f/4.0");
        assert_eq!(format_aperture(56), "f/5.6");
    }

    #[test]
    fn test_format_ev() {
        assert_eq!(format_ev(10), "+1.0 EV");
        assert_eq!(format_ev(-7), "-0.7 EV");
        assert_eq!(format_ev(0), "+0.0 EV");
    }

    #[test]
    fn test_format_shutter_speed() {
        assert_eq!(format_shutter_speed(125), "1/125 s");
        assert_eq!(format_shutter_speed(1), "1 s");
        assert_eq!(format_shutter_speed(0), "Unknown");
    }

    #[test]
    fn test_registry_has_all_tags() {
        // Verify that the registry contains all expected tags
        assert!(DJI_TAGS.has_tag(DJI_GPS_LATITUDE));
        assert!(DJI_TAGS.has_tag(DJI_GPS_LONGITUDE));
        assert!(DJI_TAGS.has_tag(DJI_GIMBAL_PITCH));
        assert!(DJI_TAGS.has_tag(DJI_FLIGHT_MODE));
        assert!(DJI_TAGS.has_tag(DJI_GPS_SIGNAL));
        assert!(DJI_TAGS.has_tag(DJI_CAMERA_WB));
    }

    #[test]
    fn test_registry_tag_names() {
        // Verify tag name lookups
        assert_eq!(DJI_TAGS.get_tag_name(DJI_GPS_LATITUDE), Some("GPSLatitude"));
        assert_eq!(DJI_TAGS.get_tag_name(DJI_FLIGHT_MODE), Some("FlightMode"));
        assert_eq!(DJI_TAGS.get_tag_name(DJI_GIMBAL_PITCH), Some("GimbalPitch"));
    }

    #[test]
    fn test_format_degrees() {
        assert_eq!(format_degrees(0), "0°");
        assert_eq!(format_degrees(180), "180°");
        assert_eq!(format_degrees(359), "359°");
    }

    #[test]
    fn test_format_meters() {
        assert_eq!(format_meters(100), "100 m");
        assert_eq!(format_meters(0), "0 m");
    }

    #[test]
    fn test_format_battery_level() {
        assert_eq!(format_battery_level(100), "100%");
        assert_eq!(format_battery_level(50), "50%");
        assert_eq!(format_battery_level(0), "0%");
    }

    #[test]
    fn test_format_iso() {
        assert_eq!(format_iso(100), "100");
        assert_eq!(format_iso(3200), "3200");
        assert_eq!(format_iso(0), "Unknown");
        assert_eq!(format_iso(-1), "Unknown");
    }

    // TODO: Re-enable these tests after DJI migration is complete
    // #[test]
    // fn test_format_satellite_count() {
    //     assert_eq!(format_satellite_count(12), "12");
    //     assert_eq!(format_satellite_count(0), "0");
    //     assert_eq!(format_satellite_count(-1), "Unknown");
    // }

    // #[test]
    // fn test_decode_hasselblad() {
    //     assert_eq!(decode_hasselblad(0), "No");
    //     assert_eq!(decode_hasselblad(1), "Yes");
    //     assert_eq!(decode_hasselblad(100), "Yes");
    // }
}
