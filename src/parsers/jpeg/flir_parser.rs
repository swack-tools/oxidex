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

use crate::core::tag_conversion::format_dms_with_ref;
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

/// FLIR FFF record type for GPS information.
///
/// `FLIR.pm` %Image::ExifTool::FLIR::Main entry 0x2b:
/// ```text
///     0x2b => {
///         Name => 'GPSInfo',
///         SubDirectory => {
///             TagTable => 'Image::ExifTool::FLIR::GPSInfo',
///             ByteOrder => 'LittleEndian',
///         },
///     },
/// ```
/// The SubDirectory pins the byte order to little-endian regardless of the
/// enclosing FFF container's order, which is why `parse_gps_info_record` does
/// not take a `FlirEndian` the way the raw-data and camera-info records do.
const RECORD_TYPE_GPS_INFO: u16 = 0x002B;

/// Field offsets within the GPSInfo record, from
/// `%Image::ExifTool::FLIR::GPSInfo` (FLIR.pm). Offsets ExifTool marks as
/// unknown (0x0c, 0x24-0x3f, 0x78, 0xa4, 0xb2) are deliberately absent.
mod gps_info_offsets {
    /// `int32u` GPSValid; also gates GPSLatitude/Longitude/Altitude.
    pub const GPS_VALID: usize = 0x00;
    /// `undef[4]` GPSVersionID, stored as four ASCII digits.
    pub const GPS_VERSION_ID: usize = 0x04;
    /// `string[2]` GPSLatitudeRef.
    pub const GPS_LATITUDE_REF: usize = 0x08;
    /// `string[2]` GPSLongitudeRef.
    pub const GPS_LONGITUDE_REF: usize = 0x0A;
    /// `double` signed GPSLatitude.
    pub const GPS_LATITUDE: usize = 0x10;
    /// `double` signed GPSLongitude.
    pub const GPS_LONGITUDE: usize = 0x18;
    /// `float` GPSAltitude.
    pub const GPS_ALTITUDE: usize = 0x20;
    /// `float` GPSDOP.
    pub const GPS_DOP: usize = 0x40;
    /// `string[2]` GPSSpeedRef.
    pub const GPS_SPEED_REF: usize = 0x44;
    /// `string[2]` GPSTrackRef.
    pub const GPS_TRACK_REF: usize = 0x46;
    /// `string[2]` GPSImgDirectionRef.
    pub const GPS_IMG_DIRECTION_REF: usize = 0x48;
    /// `float` GPSSpeed.
    pub const GPS_SPEED: usize = 0x4C;
    /// `float` GPSTrack.
    pub const GPS_TRACK: usize = 0x50;
    /// `float` GPSImgDirection.
    pub const GPS_IMG_DIRECTION: usize = 0x54;
    /// `string[16]` GPSMapDatum.
    pub const GPS_MAP_DATUM: usize = 0x58;
}

/// FFF header: `string[16]` file creator (ExifTool `FLIR::Header` tag 4).
const FFF_HEADER_CREATOR_SOFTWARE: usize = 0x04;

/// FFF header: `int32u` file format version (should be 100..200).
const FFF_HEADER_VERSION: usize = 0x14;

/// FFF header: `int32u` offset to the record directory.
const FFF_HEADER_DIR_OFFSET: usize = 0x18;

/// FFF header: `int32u` number of entries in the record directory.
const FFF_HEADER_DIR_COUNT: usize = 0x1C;

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
    /// Raw thermal image width (u16)
    pub const WIDTH: usize = 0x0002;
    /// Raw thermal image height (u16)
    pub const HEIGHT: usize = 0x0004;
    /// Start of the embedded raw thermal image (ExifTool tag index 16 with
    /// `FORMAT => 'int16u'`, i.e. byte offset 0x20)
    pub const IMAGE_DATA: usize = 0x0020;
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

/// Byte order in effect while decoding a FLIR FFF structure.
///
/// FLIR data is *not* consistently little-endian: ExifTool notes in
/// `FLIR.pm` `ProcessFLIR` that "in my samples FLIR APP1 is big-endian, FFF
/// files are little-endian", and individual records may flip the order again
/// via their own byte-order marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlirEndian {
    Little,
    Big,
}

impl FlirEndian {
    /// Build a reader over `data` using this byte order.
    fn reader<'a>(self, data: &'a [u8]) -> EndianReader<'a> {
        match self {
            FlirEndian::Little => EndianReader::little_endian(data),
            FlirEndian::Big => EndianReader::big_endian(data),
        }
    }

    /// The opposite byte order.
    fn flipped(self) -> Self {
        match self {
            FlirEndian::Little => FlirEndian::Big,
            FlirEndian::Big => FlirEndian::Little,
        }
    }
}

/// Byte order for a record that carries a byte-order marker in its first
/// `int16u`.
///
/// ExifTool, FLIR.pm (`CameraInfo` 0x00 / `RawData` 0x00):
///
/// ```text
///     0x00 => {
///         # use this tag only to determine the byte order
///         # (the value should be 0x0002 if the byte order is correct)
///         Name => 'CameraInfoByteOrder',
///         Format => 'int16u',
///         Hidden => 1,
///         RawConv => 'ToggleByteOrder() if $val >= 0x0100; undef',
///     },
/// ```
fn record_endian(data: &[u8], outer: FlirEndian) -> FlirEndian {
    match outer.reader(data).u16_at(0) {
        Some(marker) if marker >= 0x0100 => outer.flipped(),
        _ => outer,
    }
}

/// Drop the trailing zeros (and a then-trailing decimal point) from a rendered
/// decimal, the way C's — and therefore Perl's — `%g` conversion does.
///
/// This is *only* correct for `%g`. Perl's `%.Nf` keeps every digit it was
/// asked for, so a fixed-precision PrintConv such as `sprintf("%.2f",0.8)`
/// prints `0.80`, not `0.8`; running that through this helper is what used to
/// make `Emissivity`, `IRWindowTransmission` and the `AtmosphericTrans*`
/// coefficients disagree with `exiftool -G1 -s`.
fn trim_g_zeros(formatted: String) -> String {
    if !formatted.contains('.') {
        return formatted;
    }
    let trimmed = formatted.trim_end_matches('0');
    trimmed.strip_suffix('.').unwrap_or(trimmed).to_string()
}

/// Format a float with `sig` significant digits, as Perl's `sprintf("%.*g")`.
///
/// ExifTool, FLIR.pm:
///
/// ```text
/// my %float8g = ( Format => 'float', PrintConv => 'sprintf("%.8g",$val)' );
/// ```
fn sprintf_g(value: f64, sig: usize) -> String {
    if !value.is_finite() {
        return format!("{value}");
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let exponent = value.abs().log10().floor() as i32;
    if exponent < -4 || exponent >= sig as i32 {
        return format!("{:.*e}", sig.saturating_sub(1), value);
    }
    let decimals = (sig as i32 - 1 - exponent).max(0) as usize;
    trim_g_zeros(format!("{:.*}", decimals, value))
}

/// Read a fixed-width, NUL-padded string field.
///
/// Unlike [`try_read_string`] this returns `Some("")` for an empty field, which
/// is what ExifTool does for `Format => 'string[N]'` tags: `FilterModel`,
/// `LensPartNumber` and friends are reported as empty strings rather than
/// omitted.
fn read_fixed_string(data: &[u8], offset: usize, len: usize) -> Option<String> {
    let end = offset.checked_add(len)?;
    if end > data.len() {
        return None;
    }
    let bytes = &data[offset..end];
    let str_len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    Some(
        String::from_utf8_lossy(&bytes[..str_len])
            .trim_end()
            .to_string(),
    )
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

    // Parse the FLIR APP1 chunk header, per ExifTool.pm:7865-7871:
    //
    //     my $chunkNum  = Get8u($segDataPt, 6);
    //     my $chunksTot = Get8u($segDataPt, 7) + 1; # (note the "+ 1"!)
    //
    // Byte 5 is a marker ExifTool does not read. Byte 7 is NOT reserved: it
    // is the index of the LAST chunk, so the chunk count is that plus one.
    let _segment_marker = data[5];
    let chunk_number = usize::from(data[6]);
    let chunk_total = usize::from(data[7]) + 1;

    // The FFF data starts after the 8-byte header
    // Header: "FLIR\0" (5) + marker (1) + index (1) + last-chunk index (1)
    let payload = if data.len() > 8 {
        &data[8..]
    } else {
        return Ok(());
    };

    if chunk_total <= 1 {
        // A lone chunk is the whole FFF structure; nothing to accumulate.
        return parse_fff_structure(payload, metadata);
    }

    // Multi-chunk FLIR data (DJI style): concatenate every chunk in index
    // order and parse once, exactly as ExifTool.pm:7880-7899 does.
    //
    // The previous completion test was `the next slot is empty`, evaluated
    // against a fixed 20-slot vector. That is true the moment chunk 0 lands,
    // so DJI_XT2.jpg -- 11 chunks whose record directory points past the end
    // of chunk 0 -- was parsed from chunk 0 alone and the buffer cleared
    // before chunk 1 arrived. Every record beyond the FFF header was then
    // dropped by the `record_end > data.len()` bounds check in
    // `parse_fff_structure`, leaving CreatorSoftware as the only FLIR tag.
    use std::cell::RefCell;
    thread_local! {
        static FLIR_CHUNKS: RefCell<FlirChunkBuffer> =
            const { RefCell::new(FlirChunkBuffer { total: 0, chunks: Vec::new() }) };
    }

    FLIR_CHUNKS.with(|buffer| {
        let mut buffer = buffer.borrow_mut();

        // ExifTool aborts the whole accumulation when a later chunk disagrees
        // about the total (`undef $flirCount if $chunksTot != $flirTotal`).
        // Restarting on a fresh total is the same recovery.
        //
        // Chunk 0 also restarts, which is what keeps this buffer per-FILE.
        // ExifTool gets that for free -- `@flirChunk` lives on the ExifTool
        // object, one per file -- but this is a thread_local reused for every
        // file the thread parses. Without the chunk-0 reset, a truncated FLIR
        // image would leave partial chunks behind and the next image with the
        // same chunk count would append onto them through the duplicate-chunk
        // branch below, silently splicing two files together. The corpus has
        // no such pair today, so nothing catches this but the reset itself.
        if buffer.total != chunk_total || chunk_number == 0 {
            buffer.total = chunk_total;
            buffer.chunks = vec![None; chunk_total];
        }

        let Some(slot) = buffer.chunks.get_mut(chunk_number) else {
            return Ok(());
        };
        // A duplicate chunk number is appended rather than replacing the
        // first copy, matching ExifTool's 'Duplicate FLIR chunk number(s)'
        // branch.
        match slot {
            Some(existing) => existing.extend_from_slice(payload),
            None => *slot = Some(payload.to_vec()),
        }

        if buffer.chunks.iter().any(Option::is_none) {
            // Still waiting for more chunks.
            return Ok(());
        }

        let mut complete_data = Vec::new();
        for chunk in buffer.chunks.iter().flatten() {
            complete_data.extend_from_slice(chunk);
        }
        buffer.total = 0;
        buffer.chunks = Vec::new();

        parse_fff_structure(&complete_data, metadata)
    })
}

/// Accumulator for the chunks of one multi-chunk FLIR APP1 payload.
///
/// `total` is `byte 7 + 1` from the chunk header and `chunks` is indexed by
/// `byte 6`, so an out-of-order or missing chunk is detectable rather than
/// silently truncating the reassembled FFF structure.
struct FlirChunkBuffer {
    total: usize,
    chunks: Vec<Option<Vec<u8>>>,
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

    if !has_fff_header {
        // Try to parse as legacy format or embedded record
        return parse_flir_legacy_format(data, metadata);
    }

    // Determine byte order by validating the file format version, exactly as
    // ExifTool does in FLIR.pm `ProcessFLIR`:
    //
    // ```text
    //     # determine byte ordering by validating version number
    //     for ($i=0; ; ++$i) {
    //         my $ver = Get32u(\$hdr, 0x14);
    //         last if $ver >= 100 and $ver < 200; # (have seen 100 and 101 - PH)
    //         ToggleByteOrder();
    // ```
    let endian = match (
        EndianReader::big_endian(data).u32_at(FFF_HEADER_VERSION),
        EndianReader::little_endian(data).u32_at(FFF_HEADER_VERSION),
    ) {
        (Some(be), _) if (100..200).contains(&be) => FlirEndian::Big,
        (_, Some(le)) if (100..200).contains(&le) => FlirEndian::Little,
        _ => return parse_flir_legacy_format(data, metadata),
    };

    parse_fff_with_index(data, metadata, endian)
}

/// Parse FFF structure with proper record index table.
///
/// The record index is located after the FFF header and contains
/// entries pointing to different data records (CameraInfo, RawData, etc.)
fn parse_fff_with_index(
    data: &[u8],
    metadata: &mut MetadataMap,
    endian: FlirEndian,
) -> Result<(), String> {
    let reader = endian.reader(data);

    // ExifTool, FLIR.pm (`Image::ExifTool::FLIR::Header`):
    //
    // ```text
    //     4 => { Name => 'CreatorSoftware', Format => 'string[16]' },
    // ```
    if let Some(creator) = read_fixed_string(data, FFF_HEADER_CREATOR_SOFTWARE, 16) {
        metadata.insert(
            "FLIR:CreatorSoftware".to_string(),
            TagValue::String(creator),
        );
    }

    // ExifTool, FLIR.pm `ProcessFLIR`:
    //
    // ```text
    //     # 0x18 - int32u offset to record directory
    //     # 0x1c - int32u number of entries in record directory
    //     my $pos = Get32u(\$hdr, 0x18);
    //     my $num = Get32u(\$hdr, 0x1c);
    // ```
    let index_offset = reader.u32_at(FFF_HEADER_DIR_OFFSET).unwrap_or(0) as usize;
    let record_count = reader.u32_at(FFF_HEADER_DIR_COUNT).unwrap_or(0) as usize;

    if record_count == 0 || record_count > 100 {
        // Invalid or unreasonable record count, try legacy parsing
        return parse_flir_legacy_format(data, metadata);
    }

    // Parse record index entries
    let records = parse_record_index(data, index_offset, record_count, endian)?;

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
                parse_raw_data_record(record_data, metadata, record_endian(record_data, endian));
            }
            RECORD_TYPE_CAMERA_INFO => {
                parse_camera_info_record(record_data, metadata, record_endian(record_data, endian));
            }
            RECORD_TYPE_PALETTE_INFO => {
                parse_palette_info_record(record_data, metadata);
            }
            RECORD_TYPE_GPS_INFO => {
                parse_gps_info_record(record_data, metadata);
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
    endian: FlirEndian,
) -> Result<Vec<FlirRecordEntry>, String> {
    let reader = endian.reader(data);
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

    // Extract camera temperature range and limit values
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

    Ok(())
}

/// Parse RawData record containing thermal image information.
///
/// The RawData record contains:
/// - Image dimensions (width, height)
/// - Byte order for raw data
/// - Image type/format identifier
/// - Reference to the actual thermal image data
fn parse_raw_data_record(data: &[u8], metadata: &mut MetadataMap, endian: FlirEndian) {
    let reader = endian.reader(data);

    // Note: the `int16u` at offset 0 is ExifTool's hidden `RawDataByteOrder`
    // marker; it selects `endian` (see `record_endian`) and is not emitted.

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

    // Parse image type. ExifTool derives this from the payload's magic number
    // rather than from a numeric field (FLIR.pm `GetImageType`):
    //
    // ```text
    //     my $type = 'DAT';
    //     if ($val =~ /^\x89PNG\r\n\x1a\n/) {
    //         $type = 'PNG';
    //     } elsif ($val =~ /^\xff\xd8\xff/) { # (haven't seen this, but just in case - PH)
    //         $type = 'JPG';
    //     } elsif (length $val != $w * $h * 2) {
    //         $et->Warn("Unrecognized FLIR $tag data format");
    //     } elsif (GetByteOrder() eq 'II') {
    //         $val = Image::ExifTool::MakeTiffHeader($w,$h,1,16) . $val;
    //         $type = 'TIFF';
    // ```
    if data.len() > raw_data_offsets::IMAGE_DATA {
        let image = &data[raw_data_offsets::IMAGE_DATA..];
        let width = reader.u16_at(raw_data_offsets::WIDTH).unwrap_or(0) as usize;
        let height = reader.u16_at(raw_data_offsets::HEIGHT).unwrap_or(0) as usize;

        let type_str = if image.starts_with(b"\x89PNG\r\n\x1a\n") {
            "PNG"
        } else if image.starts_with(b"\xff\xd8\xff") {
            "JPG"
        } else if image.len() == width * height * 2 && endian == FlirEndian::Little {
            "TIFF"
        } else {
            "DAT"
        };
        metadata.insert(
            "FLIR:RawThermalImageType".to_string(),
            TagValue::String(type_str.to_string()),
        );

        // The image itself is not embedded in the metadata map; report it the
        // way ExifTool reports binary tags without `-b`.
        metadata.insert(
            "FLIR:RawThermalImage".to_string(),
            TagValue::String(format!(
                "(Binary data {} bytes, use -b option to extract)",
                image.len()
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
fn parse_camera_info_record(data: &[u8], metadata: &mut MetadataMap, endian: FlirEndian) {
    let reader = endian.reader(data);

    // Note: the `int16u` at offset 0 is ExifTool's hidden `CameraInfoByteOrder`
    // marker; it selects `endian` (see `record_endian`) and is not emitted.

    // === Emissivity and Environmental Parameters ===

    // ExifTool, FLIR.pm: `0x20 => { Name => 'Emissivity', %float2f },`
    // with `my %float2f = ( Format => 'float', PrintConv => 'sprintf("%.2f",$val)' );`
    if let Some(emissivity) = reader.f32_at(camera_info_offsets::EMISSIVITY) {
        metadata.insert(
            "FLIR:Emissivity".to_string(),
            TagValue::String(format!("{:.2}", emissivity)),
        );
    }

    // ExifTool, FLIR.pm:
    // `0x24 => { Name => 'ObjectDistance', Format => 'float', PrintConv => 'sprintf("%.2f m",$val)' },`
    if let Some(distance) = reader.f32_at(camera_info_offsets::OBJECT_DISTANCE) {
        metadata.insert(
            "FLIR:ObjectDistance".to_string(),
            TagValue::String(format!("{:.2} m", distance)),
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

    // ExifTool, FLIR.pm: `0x34 => { Name => 'IRWindowTransmission', %float2f },`
    if let Some(transmission) = reader.f32_at(camera_info_offsets::IR_WINDOW_TRANSMISSION) {
        metadata.insert(
            "FLIR:IRWindowTransmission".to_string(),
            TagValue::String(format!("{:.2}", transmission)),
        );
    }

    // ExifTool, FLIR.pm:
    //
    // ```text
    //     0x3c => {
    //         Name => 'RelativeHumidity',
    //         Format => 'float',
    //         ValueConv => '$val > 2 ? $val / 100 : $val', # have seen value expressed as percent in FFF file
    //         PrintConv => 'sprintf("%.1f %%",$val*100)',
    //     },
    // ```
    if let Some(humidity) = reader.f32_at(camera_info_offsets::RELATIVE_HUMIDITY) {
        let fraction = if humidity > 2.0 {
            humidity / 100.0
        } else {
            humidity
        };
        metadata.insert(
            "FLIR:RelativeHumidity".to_string(),
            TagValue::String(format!("{:.1} %", fraction * 100.0)),
        );
    }

    // === Planck Constants for Radiometric Calculation ===
    //
    // ExifTool, FLIR.pm:
    //
    // ```text
    //     0x58 => { Name => 'PlanckR1', %float8g }, #1
    //     0x5c => { Name => 'PlanckB',  %float8g }, #1
    //     0x60 => { Name => 'PlanckF',  %float8g }, #1
    // ```
    insert_float8g(
        &reader,
        camera_info_offsets::PLANCK_R1,
        "FLIR:PlanckR1",
        metadata,
    );
    insert_float8g(
        &reader,
        camera_info_offsets::PLANCK_B,
        "FLIR:PlanckB",
        metadata,
    );
    insert_float8g(
        &reader,
        camera_info_offsets::PLANCK_F,
        "FLIR:PlanckF",
        metadata,
    );

    // ExifTool, FLIR.pm: `0x308 => { Name => 'PlanckO', Format => 'int32s' },`
    if let Some(planck_o) = reader.i32_at(camera_info_offsets::PLANCK_O) {
        metadata.insert(
            "FLIR:PlanckO".to_string(),
            TagValue::Integer(planck_o as i64),
        );
    }

    // ExifTool, FLIR.pm: `0x30c => { Name => 'PlanckR2', %float8g }, #1`
    insert_float8g(
        &reader,
        camera_info_offsets::PLANCK_R2,
        "FLIR:PlanckR2",
        metadata,
    );

    // === Atmospheric Transmission Coefficients ===
    //
    // ExifTool, FLIR.pm (`my %float6f = ( Format => 'float', PrintConv => 'sprintf("%.6f",$val)' );`):
    //
    // ```text
    //     0x070 => { Name => 'AtmosphericTransAlpha1', %float6f }, #1 (value: 0.006569)
    //     0x074 => { Name => 'AtmosphericTransAlpha2', %float6f }, #1 (value: 0.012620)
    //     0x078 => { Name => 'AtmosphericTransBeta1',  %float6f }, #1 (value: -0.002276)
    //     0x07c => { Name => 'AtmosphericTransBeta2',  %float6f }, #1 (value: -0.006670)
    //     0x080 => { Name => 'AtmosphericTransX',      %float6f }, #1 (value: 1.900000)
    // ```
    insert_float6f(
        &reader,
        camera_info_offsets::ATMOSPHERIC_TRANS_ALPHA1,
        "FLIR:AtmosphericTransAlpha1",
        metadata,
    );
    insert_float6f(
        &reader,
        camera_info_offsets::ATMOSPHERIC_TRANS_ALPHA2,
        "FLIR:AtmosphericTransAlpha2",
        metadata,
    );
    insert_float6f(
        &reader,
        camera_info_offsets::ATMOSPHERIC_TRANS_BETA1,
        "FLIR:AtmosphericTransBeta1",
        metadata,
    );
    insert_float6f(
        &reader,
        camera_info_offsets::ATMOSPHERIC_TRANS_BETA2,
        "FLIR:AtmosphericTransBeta2",
        metadata,
    );
    insert_float6f(
        &reader,
        camera_info_offsets::ATMOSPHERIC_TRANS_X,
        "FLIR:AtmosphericTransX",
        metadata,
    );

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

    // === Camera, Lens and Filter Identification ===
    //
    // ExifTool, FLIR.pm:
    //
    // ```text
    //     0xd4 => { Name => 'CameraModel',        Format => 'string[32]' },
    //     0xf4 => { Name => 'CameraPartNumber',   Format => 'string[16]' }, #1
    //     0x104 => { Name => 'CameraSerialNumber',Format => 'string[16]' }, #1
    //     0x114 => { Name => 'CameraSoftware',    Format => 'string[16]' }, #1/PH (NC)
    //     0x170 => { Name => 'LensModel',         Format => 'string[32]' },
    //     0x190 => { Name => 'LensPartNumber',    Format => 'string[16]' },
    //     0x1a0 => { Name => 'LensSerialNumber',  Format => 'string[16]' },
    //     0x1ec => { Name => 'FilterModel',       Format => 'string[16]' },
    //     0x1fc => { Name => 'FilterPartNumber',  Format => 'string[32]' },
    //     0x21c => { Name => 'FilterSerialNumber',Format => 'string[32]' },
    // ```
    //
    // These are emitted even when blank: ExifTool reports empty `string[N]`
    // fields as empty values rather than omitting them.
    for (offset, len, tag) in [
        (camera_info_offsets::CAMERA_MODEL, 32, "FLIR:CameraModel"),
        (
            camera_info_offsets::CAMERA_PART_NUMBER,
            16,
            "FLIR:CameraPartNumber",
        ),
        (
            camera_info_offsets::CAMERA_SERIAL_NUMBER,
            16,
            "FLIR:CameraSerialNumber",
        ),
        (
            camera_info_offsets::CAMERA_SOFTWARE,
            16,
            "FLIR:CameraSoftware",
        ),
        (camera_info_offsets::LENS_MODEL, 32, "FLIR:LensModel"),
        (
            camera_info_offsets::LENS_PART_NUMBER,
            16,
            "FLIR:LensPartNumber",
        ),
        (
            camera_info_offsets::LENS_SERIAL_NUMBER,
            16,
            "FLIR:LensSerialNumber",
        ),
        (camera_info_offsets::FILTER_MODEL, 16, "FLIR:FilterModel"),
        (
            camera_info_offsets::FILTER_PART_NUMBER,
            32,
            "FLIR:FilterPartNumber",
        ),
        (
            camera_info_offsets::FILTER_SERIAL_NUMBER,
            32,
            "FLIR:FilterSerialNumber",
        ),
    ] {
        if let Some(value) = read_fixed_string(data, offset, len) {
            metadata.insert(tag.to_string(), TagValue::String(value));
        }
    }

    // ExifTool, FLIR.pm:
    // `0x1b4 => { Name => 'FieldOfView', Format => 'float', PrintConv => 'sprintf("%.1f deg", $val) }, #1`
    if let Some(fov) = reader.f32_at(camera_info_offsets::FIELD_OF_VIEW) {
        metadata.insert(
            "FLIR:FieldOfView".to_string(),
            TagValue::String(format!("{:.1} deg", fov)),
        );
    }

    // === Raw Value Statistics ===
    //
    // ExifTool, FLIR.pm:
    //
    // ```text
    //     0x310 => { Name => 'RawValueRangeMin',  Format => 'int16u', Groups => { 2 => 'Image' } }, #forum10060
    //     0x312 => { Name => 'RawValueRangeMax',  Format => 'int16u', Groups => { 2 => 'Image' } }, #forum10060
    //     0x338 => { Name => 'RawValueMedian',    Format => 'int16u', Groups => { 2 => 'Image' } },
    //     0x33c => { Name => 'RawValueRange',     Format => 'int16u', Groups => { 2 => 'Image' } },
    // ```
    for (offset, tag) in [
        (
            camera_info_offsets::RAW_VALUE_RANGE_MIN,
            "FLIR:RawValueRangeMin",
        ),
        (
            camera_info_offsets::RAW_VALUE_RANGE_MAX,
            "FLIR:RawValueRangeMax",
        ),
        (camera_info_offsets::RAW_VALUE_MEDIAN, "FLIR:RawValueMedian"),
        (camera_info_offsets::RAW_VALUE_RANGE, "FLIR:RawValueRange"),
    ] {
        if let Some(value) = reader.u16_at(offset) {
            metadata.insert(tag.to_string(), TagValue::Integer(value as i64));
        }
    }

    // === Timing and Focus ===

    if let Some(datetime) = flir_camera_datetime(&reader, camera_info_offsets::DATE_TIME_ORIGINAL) {
        metadata.insert(
            "FLIR:DateTimeOriginal".to_string(),
            TagValue::String(datetime),
        );
    }

    // ExifTool, FLIR.pm: `0x390 => { Name => 'FocusStepCount', Format => 'int16u' },`
    if let Some(focus_steps) = reader.u16_at(camera_info_offsets::FOCUS_STEP_COUNT) {
        metadata.insert(
            "FLIR:FocusStepCount".to_string(),
            TagValue::Integer(focus_steps as i64),
        );
    }

    // ExifTool, FLIR.pm:
    // `0x45c => { Name => 'FocusDistance', Format => 'float', PrintConv => 'sprintf("%.1f m",$val) },`
    if let Some(focus_dist) = reader.f32_at(camera_info_offsets::FOCUS_DISTANCE) {
        metadata.insert(
            "FLIR:FocusDistance".to_string(),
            TagValue::String(format!("{:.1} m", focus_dist)),
        );
    }

    // ExifTool, FLIR.pm: `0x464 => { Name => 'FrameRate',  Format => 'int16u' }, #SebastianHani`
    if let Some(frame_rate) = reader.u16_at(camera_info_offsets::FRAME_RATE) {
        metadata.insert(
            "FLIR:FrameRate".to_string(),
            TagValue::Integer(frame_rate as i64),
        );
    }
}

/// Parse the GPSInfo record (`%Image::ExifTool::FLIR::GPSInfo`, FLIR.pm:613).
///
/// This is a plain `ProcessBinaryData` table at fixed offsets, always
/// little-endian per the SubDirectory declaration on record 0x2b. It is a
/// separate GPS fix from the file's EXIF GPS IFD and ExifTool reports it under
/// its own family-0 group (`APP1`), so the two coexist rather than one
/// overwriting the other.
///
/// Several entries carry a `RawConv` that suppresses the tag rather than
/// printing a placeholder; each is reproduced at its call site below, because
/// emitting a tag ExifTool withholds is as much a parity failure as omitting
/// one it emits.
fn parse_gps_info_record(data: &[u8], metadata: &mut MetadataMap) {
    let reader = EndianReader::little_endian(data);

    // 0x00 GPSValid: int32u, PrintConv => { 0 => 'No', 1 => 'Yes' }. A value
    // outside that pair has no PrintConv entry, so ExifTool falls back to
    // "Unknown (n)".
    let Some(gps_valid) = reader.u32_at(gps_info_offsets::GPS_VALID) else {
        return;
    };
    metadata.insert(
        "FLIR:GPSValid".to_string(),
        TagValue::new_string(match gps_valid {
            0 => "No".to_string(),
            1 => "Yes".to_string(),
            other => format!("Unknown ({other})"),
        }),
    );

    // 0x04 GPSVersionID: undef[4] with `PrintConv => 'join ".", split //, $val'`
    // -- the four bytes are ASCII digits, split into characters and rejoined
    // with dots, so the bytes "2200" print as "2.2.0.0". `RawConv` drops an
    // all-NUL value.
    if let Some(version) = reader.bytes_at(gps_info_offsets::GPS_VERSION_ID, 4)
        && version != [0, 0, 0, 0]
    {
        let text = version
            .iter()
            .map(|byte| (*byte as char).to_string())
            .collect::<Vec<_>>()
            .join(".");
        metadata.insert("FLIR:GPSVersionID".to_string(), TagValue::new_string(text));
    }

    insert_flir_gps_ref(
        &reader,
        gps_info_offsets::GPS_LATITUDE_REF,
        "FLIR:GPSLatitudeRef",
        &[("N", "North"), ("S", "South")],
        metadata,
    );
    insert_flir_gps_ref(
        &reader,
        gps_info_offsets::GPS_LONGITUDE_REF,
        "FLIR:GPSLongitudeRef",
        &[("E", "East"), ("W", "West")],
        metadata,
    );

    // 0x10/0x18/0x20: latitude, longitude and altitude are each guarded by
    // `Condition => '$$self{GPSValid}'`, so they exist only when GPSValid is
    // truthy. Latitude and longitude are signed doubles printed through
    // ToDMS with a hemisphere letter; altitude is a float printed as
    // `sprintf("%.2f m", $val)`.
    if gps_valid != 0 {
        if let Some(latitude) = reader.f64_at(gps_info_offsets::GPS_LATITUDE) {
            metadata.insert(
                "FLIR:GPSLatitude".to_string(),
                TagValue::new_string(format_dms_with_ref(latitude, 'N')),
            );
        }
        if let Some(longitude) = reader.f64_at(gps_info_offsets::GPS_LONGITUDE) {
            metadata.insert(
                "FLIR:GPSLongitude".to_string(),
                TagValue::new_string(format_dms_with_ref(longitude, 'E')),
            );
        }
        if let Some(altitude) = reader.f32_at(gps_info_offsets::GPS_ALTITUDE) {
            metadata.insert(
                "FLIR:GPSAltitude".to_string(),
                TagValue::new_string(format!("{:.2} m", altitude)),
            );
        }
    }

    // 0x40 GPSDOP: float, `RawConv => '$val > 0 ? $val : undef'` -- the note
    // in FLIR.pm says 0 and 1 are both seen as junk, but only non-positive is
    // actually filtered, so that is what this filters.
    if let Some(dop) = reader.f32_at(gps_info_offsets::GPS_DOP)
        && dop > 0.0
    {
        metadata.insert(
            "FLIR:GPSDOP".to_string(),
            TagValue::new_string(format!("{:.2}", dop)),
        );
    }

    insert_flir_gps_ref(
        &reader,
        gps_info_offsets::GPS_SPEED_REF,
        "FLIR:GPSSpeedRef",
        &[("K", "km/h"), ("M", "mph"), ("N", "knots")],
        metadata,
    );
    insert_flir_gps_ref(
        &reader,
        gps_info_offsets::GPS_TRACK_REF,
        "FLIR:GPSTrackRef",
        &[("M", "Magnetic North"), ("T", "True North")],
        metadata,
    );
    insert_flir_gps_ref(
        &reader,
        gps_info_offsets::GPS_IMG_DIRECTION_REF,
        "FLIR:GPSImgDirectionRef",
        &[("M", "Magnetic North"), ("T", "True North")],
        metadata,
    );

    // 0x4c/0x50/0x54: `%float2f` (FLIR.pm:48) is `Format => 'float'` plus
    // `sprintf("%.2f",$val)`, and each carries `RawConv => '$val < 0 ? undef :
    // $val'`. Zero is kept -- DJI_XT2.jpg stores 0 for all three and ExifTool
    // prints "0.00".
    for (offset, key) in [
        (gps_info_offsets::GPS_SPEED, "FLIR:GPSSpeed"),
        (gps_info_offsets::GPS_TRACK, "FLIR:GPSTrack"),
        (gps_info_offsets::GPS_IMG_DIRECTION, "FLIR:GPSImgDirection"),
    ] {
        if let Some(value) = reader.f32_at(offset)
            && value >= 0.0
        {
            metadata.insert(
                key.to_string(),
                TagValue::new_string(format!("{:.2}", value)),
            );
        }
    }

    // 0x58 GPSMapDatum: string[16], `RawConv => 'length($val) ? $val : undef'`.
    if let Some(datum) = reader.cstr_at(gps_info_offsets::GPS_MAP_DATUM, 16)
        && !datum.is_empty()
    {
        metadata.insert(
            "FLIR:GPSMapDatum".to_string(),
            TagValue::new_string(datum.to_string()),
        );
    }
}

/// Emit one of GPSInfo's `string[2]` reference tags.
///
/// Every such entry in `%Image::ExifTool::FLIR::GPSInfo` pairs
/// `RawConv => 'length($val) ? $val : undef'` with a small PrintConv hash, so
/// an empty field is skipped entirely and a letter outside the hash falls back
/// to ExifTool's `Unknown (x)` rendering.
fn insert_flir_gps_ref(
    reader: &EndianReader<'_>,
    offset: usize,
    key: &str,
    print_conv: &[(&str, &str)],
    metadata: &mut MetadataMap,
) {
    let Some(raw) = reader.cstr_at(offset, 2) else {
        return;
    };
    if raw.is_empty() {
        return;
    }
    let value = print_conv
        .iter()
        .find(|(code, _)| *code == raw)
        .map(|(_, text)| (*text).to_string())
        .unwrap_or_else(|| format!("Unknown ({raw})"));
    metadata.insert(key.to_string(), TagValue::new_string(value));
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

    // ExifTool, FLIR.pm has no PrintConv on either of these, so the raw
    // numbers are reported:
    //
    // ```text
    //     0x1a => { Name => 'PaletteMethod' }, #JD
    //     0x1b => { Name => 'PaletteStretch' }, #JD
    // ```
    if let Some(method) = reader.u8_at(palette_info_offsets::PALETTE_METHOD) {
        metadata.insert(
            "FLIR:PaletteMethod".to_string(),
            TagValue::Integer(method as i64),
        );
    }

    if let Some(stretch) = reader.u8_at(palette_info_offsets::PALETTE_STRETCH) {
        metadata.insert(
            "FLIR:PaletteStretch".to_string(),
            TagValue::Integer(stretch as i64),
        );
    }

    // Palette file name and name. ExifTool, FLIR.pm:
    //
    // ```text
    //     0x30 => {
    //         Name => 'PaletteFileName',
    //         Format => 'string[32]',
    //         # (not valid for all images)
    //         RawConv => q{
    //             $val =~ s/\0.*//;
    //             $val =~ /^[\x20-\x7e]{3,31}$/ ? $val : undef;
    //         },
    //     },
    // ```
    for (offset, tag) in [
        (
            palette_info_offsets::PALETTE_FILE_NAME,
            "FLIR:PaletteFileName",
        ),
        (palette_info_offsets::PALETTE_NAME, "FLIR:PaletteName"),
    ] {
        if let Some(value) = read_fixed_string(data, offset, 32)
            && (3..=31).contains(&value.len())
            && value.bytes().all(|b| (0x20..=0x7e).contains(&b))
        {
            metadata.insert(tag.to_string(), TagValue::String(value));
        }
    }

    // ExifTool, FLIR.pm:
    //
    // ```text
    //     0x70 => {
    //         Name => 'Palette',
    //         Format => 'undef[3*$$self{PaletteColors}]',
    //         Notes => 'Y Cr Cb byte values for each palette color',
    //         Binary => 1,
    //     },
    // ```
    if let Some(colors) = reader.u8_at(palette_info_offsets::PALETTE_COLORS) {
        let palette_len = 3 * colors as usize;
        if palette_len > 0 && palette_info_offsets::PALETTE + palette_len <= data.len() {
            metadata.insert(
                "FLIR:Palette".to_string(),
                TagValue::String(format!(
                    "(Binary data {} bytes, use -b option to extract)",
                    palette_len
                )),
            );
        }
    }
}

/// Insert a temperature stored as float Kelvin.
///
/// ExifTool, FLIR.pm:
///
/// ```text
/// # tag information for floating point Kelvin tag
/// my %floatKelvin = (
///     Format => 'float',
///     ValueConv => '$val - 273.15',
///     PrintConv => 'sprintf("%.1f C",$val)',
/// );
/// ```
fn insert_temperature(
    reader: &EndianReader,
    offset: usize,
    tag_name: &str,
    metadata: &mut MetadataMap,
) {
    if let Some(kelvin) = reader.f32_at(offset)
        && kelvin.is_finite()
    {
        metadata.insert(
            tag_name.to_string(),
            TagValue::String(format!("{:.1} C", kelvin as f64 - 273.15)),
        );
    }
}

/// Insert a float rendered with `sprintf("%.8g")` (ExifTool's `%float8g`).
fn insert_float8g(
    reader: &EndianReader,
    offset: usize,
    tag_name: &str,
    metadata: &mut MetadataMap,
) {
    if let Some(value) = reader.f32_at(offset) {
        metadata.insert(
            tag_name.to_string(),
            TagValue::String(sprintf_g(value as f64, 8)),
        );
    }
}

/// Insert a float rendered with `sprintf("%.6f")` (ExifTool's `%float6f`).
fn insert_float6f(
    reader: &EndianReader,
    offset: usize,
    tag_name: &str,
    metadata: &mut MetadataMap,
) {
    if let Some(value) = reader.f32_at(offset) {
        metadata.insert(
            tag_name.to_string(),
            TagValue::String(format!("{:.6}", value)),
        );
    }
}

/// Insert a colour stored as three `int8u` components.
///
/// ExifTool, FLIR.pm: `0x06 => { Name => 'AboveColor', Format => 'int8u[3]',
/// Notes => 'Y Cr Cb color components' }` — reported as space-separated
/// decimal components, not as a hex triplet.
fn insert_rgb_color(data: &[u8], offset: usize, tag_name: &str, metadata: &mut MetadataMap) {
    if offset + 3 <= data.len() {
        metadata.insert(
            tag_name.to_string(),
            TagValue::String(format!(
                "{} {} {}",
                data[offset],
                data[offset + 1],
                data[offset + 2]
            )),
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

/// Decode the FLIR CameraInfo `DateTimeOriginal` field.
///
/// ExifTool, FLIR.pm (`CameraInfo` 0x384, `Format => 'undef[10]'`):
///
/// ```text
///     0x384 => {
///         Name => 'DateTimeOriginal',
///         Description => 'Date/Time Original',
///         Format => 'undef[10]',
///         Groups => { 2 => 'Time' },
///         RawConv => q{
///             my $tm = Get32u(\$val, 0);
///             my $ss = Get32u(\$val, 4) & 0xffff;
///             my $tz = Get16s(\$val, 8);
///             ConvertUnixTime($tm - $tz * 60) . sprintf('.%.3d', $ss) . TimeZoneString(-$tz);
///         },
///         PrintConv => '$self->ConvertDateTime($val)',
///     },
/// ```
fn flir_camera_datetime(reader: &EndianReader, offset: usize) -> Option<String> {
    let seconds = reader.u32_at(offset)? as i64;
    let millis = reader.u32_at(offset + 4)? & 0xffff;
    let tz_minutes = reader.i16_at(offset + 8)? as i64;

    let (year, month, day, hour, minute, second) = unix_to_datetime(seconds - tz_minutes * 60);

    Some(format!(
        "{year:04}:{month:02}:{day:02} {hour:02}:{minute:02}:{second:02}.{millis:03}{tz}",
        tz = time_zone_string(-tz_minutes)
    ))
}

/// Render a UTC offset in minutes as ExifTool's `TimeZoneString` does
/// (`+HH:MM` / `-HH:MM`).
fn time_zone_string(minutes: i64) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let abs = minutes.abs();
    format!("{sign}{:02}:{:02}", abs / 60, abs % 60)
}

/// Convert a Unix timestamp to broken-down UTC calendar fields.
///
/// Equivalent to ExifTool's `ConvertUnixTime`, which is `gmtime` based, so
/// leap years are handled exactly rather than approximated.
fn unix_to_datetime(timestamp: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = timestamp.div_euclid(86_400);
    let time_of_day = timestamp.rem_euclid(86_400);

    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    // Howard Hinnant's `civil_from_days` algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    (year, month, day, hour, minute, second)
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

    /// A multi-chunk FLIR payload must not be parsed until every chunk has
    /// arrived.
    ///
    /// This is the regression that hid 61 of DJI_XT2.jpg's 62 FLIR tags: the
    /// old completion test fired on chunk 0, so the record directory was read
    /// against a truncated buffer and every record whose extent fell past the
    /// end of chunk 0 was dropped by the bounds check in
    /// `parse_fff_structure`. The chunk total lives in byte 7 of the APP1
    /// header as `last index`, hence ExifTool.pm:7870's `+ 1`.
    #[test]
    fn multi_chunk_flir_waits_for_every_chunk() {
        /// One APP1 FLIR chunk: "FLIR\0", marker, chunk index, last index.
        fn chunk(index: u8, last_index: u8, payload: &[u8]) -> Vec<u8> {
            let mut data = Vec::from(b"FLIR\x00".as_slice());
            data.extend_from_slice(&[0x01, index, last_index]);
            data.extend_from_slice(payload);
            data
        }

        // A three-chunk FFF whose record directory sits in the final chunk,
        // so nothing can be decoded until all three have been seen.
        let mut header = Vec::from(b"FFF\x00".as_slice());
        header.resize(64, 0);

        let mut metadata = MetadataMap::new();
        assert!(parse_flir_segment(&chunk(0, 2, &header), &mut metadata).is_ok());
        assert!(
            metadata.is_empty(),
            "chunk 0 of 3 must not be parsed on its own"
        );
        assert!(parse_flir_segment(&chunk(1, 2, &[0u8; 32]), &mut metadata).is_ok());
        assert!(
            metadata.is_empty(),
            "chunk 1 of 3 must not be parsed on its own"
        );

        // The final chunk completes the buffer, so parsing runs exactly once.
        assert!(parse_flir_segment(&chunk(2, 2, &[0u8; 32]), &mut metadata).is_ok());
    }

    /// A truncated image must not bleed into the next one.
    ///
    /// The chunk buffer is a `thread_local` reused across files, so chunk 0
    /// restarts it. Without that, the leftover chunk 0 of an image whose
    /// remaining chunks never arrived would be appended to -- not replaced by
    /// -- the next image's chunk 0, splicing two files into one FFF buffer.
    #[test]
    fn chunk_zero_restarts_after_a_truncated_image() {
        fn chunk(index: u8, last_index: u8, payload: &[u8]) -> Vec<u8> {
            let mut data = Vec::from(b"FLIR\x00".as_slice());
            data.extend_from_slice(&[0x01, index, last_index]);
            data.extend_from_slice(payload);
            data
        }

        let mut first = Vec::from(b"FFF\x00".as_slice());
        first.resize(64, 0xAA);
        let mut metadata = MetadataMap::new();

        // An image that announces two chunks but only ever delivers chunk 0.
        assert!(parse_flir_segment(&chunk(0, 1, &first), &mut metadata).is_ok());

        // The next image reuses the same chunk count. Its chunk 0 must
        // replace the stale one, so the completed buffer is exactly the
        // second image's two chunks and not four chunks' worth of bytes.
        let mut second = Vec::from(b"FFF\x00".as_slice());
        second.resize(64, 0);
        assert!(parse_flir_segment(&chunk(0, 1, &second), &mut metadata).is_ok());
        assert!(parse_flir_segment(&chunk(1, 1, &[0u8; 32]), &mut metadata).is_ok());
    }

    /// Byte 7 is the LAST chunk index, so a lone chunk carries 0 there and is
    /// parsed immediately rather than waiting for a chunk that never comes.
    #[test]
    fn single_chunk_flir_parses_immediately() {
        let mut data = Vec::from(b"FLIR\x00".as_slice());
        data.extend_from_slice(&[0x01, 0x00, 0x00]);
        data.extend_from_slice(b"FFF\x00");
        data.resize(8 + 64, 0);

        let mut metadata = MetadataMap::new();
        assert!(parse_flir_segment(&data, &mut metadata).is_ok());
    }

    /// Colour components are reported as decimal "Y Cr Cb", per FLIR.pm's
    /// `Format => 'int8u[3]'`.
    #[test]
    fn test_insert_rgb_color() {
        let data = [170, 128, 128];
        let mut metadata = MetadataMap::new();
        insert_rgb_color(&data, 0, "TestColor", &mut metadata);

        assert_eq!(metadata.get_string("TestColor"), Some("170 128 128"));
    }

    /// `%floatKelvin` converts to Celsius and prints `sprintf("%.1f C")`.
    #[test]
    fn test_insert_temperature_converts_kelvin_to_celsius() {
        // 293.15 K == 20.0 C, little-endian f32
        let data = 293.15f32.to_le_bytes();
        let reader = EndianReader::little_endian(&data);
        let mut metadata = MetadataMap::new();

        insert_temperature(&reader, 0, "TestTemp", &mut metadata);

        assert_eq!(metadata.get_string("TestTemp"), Some("20.0 C"));
    }

    /// `sprintf("%.8g", ...)` reproduces ExifTool's Planck constant rendering.
    #[test]
    fn test_sprintf_g_matches_perl() {
        assert_eq!(sprintf_g(13799.2685546875, 8), "13799.269");
        assert_eq!(sprintf_g(1374.5, 8), "1374.5");
        assert_eq!(sprintf_g(0.02224181778728962, 8), "0.022241818");
    }

    /// `%g` drops trailing zeros and the decimal point they leave behind.
    /// `%.Nf` does not — that distinction is checked by
    /// [`test_fixed_precision_keeps_trailing_zeros`].
    #[test]
    fn test_trim_g_zeros() {
        assert_eq!(trim_g_zeros("0.012620".to_string()), "0.01262");
        assert_eq!(trim_g_zeros("1.00".to_string()), "1");
        assert_eq!(trim_g_zeros("13799.2690".to_string()), "13799.269");
        assert_eq!(trim_g_zeros("1374".to_string()), "1374");
    }

    /// `exiftool -G1 -s` prints the string a numeric PrintConv returned, so
    /// FLIR's `%float2f`/`%float6f` tags keep every digit `sprintf` produced:
    ///
    /// ```text
    /// my %float2f = ( Format => 'float', PrintConv => 'sprintf("%.2f",$val)' );
    /// my %float6f = ( Format => 'float', PrintConv => 'sprintf("%.6f",$val)' );
    /// ```
    ///
    /// Measured on ExifTool 13.55 against `t/images/FLIR.jpg`: `Emissivity`
    /// is `0.80`, `IRWindowTransmission` is `1.00`,
    /// `AtmosphericTransAlpha2` is `0.012620`, `AtmosphericTransBeta2` is
    /// `-0.006670` and `AtmosphericTransX` is `1.900000`.
    #[test]
    fn test_fixed_precision_keeps_trailing_zeros() {
        assert_eq!(format!("{:.2}", 0.8f32), "0.80");
        assert_eq!(format!("{:.2}", 1.0f32), "1.00");
        assert_eq!(format!("{:.6}", 0.01262f32), "0.012620");
        assert_eq!(format!("{:.6}", -0.00667f32), "-0.006670");
        assert_eq!(format!("{:.6}", 1.9f32), "1.900000");
    }

    /// The CameraInfo `DateTimeOriginal` field is a 10-byte binary record:
    /// `int32u` Unix seconds, `int32u` (& 0xffff) milliseconds and `int16s`
    /// timezone minutes, per FLIR.pm's RawConv at 0x384.
    #[test]
    fn test_flir_camera_datetime() {
        let mut data = Vec::new();
        data.extend_from_slice(&1_328_966_228u32.to_le_bytes()); // 2012:02:11 13:17:08 UTC
        data.extend_from_slice(&253u32.to_le_bytes());
        data.extend_from_slice(&(-60i16).to_le_bytes());

        let reader = EndianReader::little_endian(&data);
        assert_eq!(
            flir_camera_datetime(&reader, 0),
            Some("2012:02:11 14:17:08.253+01:00".to_string())
        );
    }

    /// Leap years must be exact, not approximated.
    #[test]
    fn test_unix_to_datetime_leap_year() {
        // 2012-02-29T00:00:00Z
        assert_eq!(unix_to_datetime(1_330_473_600), (2012, 2, 29, 0, 0, 0));
        // 1970-01-01T00:00:00Z
        assert_eq!(unix_to_datetime(0), (1970, 1, 1, 0, 0, 0));
        // 2000-03-01T12:34:56Z (400-year leap rule)
        assert_eq!(unix_to_datetime(951_914_096), (2000, 3, 1, 12, 34, 56));
    }

    /// A record whose byte-order marker reads >= 0x0100 flips the byte order
    /// for that record's contents (FLIR.pm `CameraInfoByteOrder`).
    #[test]
    fn test_record_endian_flips_on_marker() {
        // 0x0002 read big-endian == 2, so the outer order is already correct.
        assert_eq!(
            record_endian(&[0x00, 0x02], FlirEndian::Big),
            FlirEndian::Big
        );
        // 0x0200 read big-endian == 512 >= 0x0100, so the record is flipped.
        assert_eq!(
            record_endian(&[0x02, 0x00], FlirEndian::Big),
            FlirEndian::Little
        );
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
