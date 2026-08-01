//! APP6 segment parser for JPEG files
//!
//! JPEG APP6 segments (marker 0xFFE6) contain various proprietary metadata formats:
//! - GoPro GPMF (GoPro Metadata Format) - Action camera telemetry and settings
//! - HP/Toshiba TDHD (True Definition High Definition) - Stereo image metadata
//! - NITF (National Imagery Transmission Format) - Geospatial metadata
//! - IPTC-NAA - Legacy IPTC records (rare, mostly superseded by APP13)
//!
//! # GoPro GPMF Format
//!
//! GoPro cameras embed extensive metadata in APP6 segments including:
//! - Camera settings (FOV, resolution, frame rate, protune, etc.)
//! - Sensor telemetry (GPS, accelerometer, gyroscope)
//! - Image processing parameters (lens distortion, color grading)
//! - Device information (model, serial number, firmware version)
//!
//! The GPMF format uses a tag-length-value (TLV) structure with FourCC identifiers.
//! Each record consists of:
//! - FourCC key (4 bytes) - Tag identifier
//! - Type (1 byte) - Data type indicator
//! - Size (1 byte) - Size of each element
//! - Count (2 bytes, big-endian) - Number of elements
//! - Data (variable) - Payload data
//!
//! # References
//!
//! - GoPro GPMF Specification: https://github.com/gopro/gpmf-parser
//! - ExifTool APP6 Tags: lib/Image/ExifTool/GoPro.pm
//! - JPEG Specification: ITU-T T.81 / ISO/IEC 10918-1
//!
//! # Example
//!
//! ```ignore
//! use oxidex::parsers::jpeg::app_segments::app6::parse_app6;
//!
//! let data: &[u8] = &[/* APP6 segment data */];
//! let metadata = parse_app6(data)?;
//!
//! if let Some(model) = metadata.get_string("APP6:Model") {
//!     println!("Camera model: {}", model);
//! }
//! ```

use super::perl_number;
use crate::core::value_formatter::format_exif_datetime;
use crate::core::{MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::parsers::common::print_im::{PRINT_IM_VERSION_TAG, decode_print_im_version};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Minimum APP6 payload length before ExifTool will read it as an InfiRay
/// MixMode record (ExifTool.pm:8162: `$$self{HasIJPEG} and $length >= 129`).
const INFIRAY_MIXMODE_MIN_LENGTH: usize = 129;

/// Parses APP6 segment data and extracts metadata.
///
/// This function dispatches to format-specific parsers based on the segment
/// identifier, using the same conditions as ExifTool's JPEG.pm APP6 table:
/// - Toshiba PrintIM - starts with "EPPIM\0"
/// - NITF data - starts with "NITF\0"
/// - TDHD data (HP/Toshiba) - starts with "TDHD\x01\0\0\0"
/// - GoPro GPMF data - starts with "GoPro\0"
/// - InfiRay MixMode - only in an IJPEG file, so see `parse_app6_ijpeg`
/// - Other formats extract nothing (matching ExifTool without -u)
///
/// # Arguments
///
/// * `data` - Raw APP6 segment data (excluding the APP6 marker and length bytes)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - A metadata map containing extracted APP6 tags
/// * `Err(ExifToolError)` - If the data is malformed or unsupported
///
/// # Errors
///
/// Returns an error if:
/// - The segment is too short to contain valid metadata
/// - The format is recognized but parsing fails
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::app6::parse_app6;
///
/// // Parse a GoPro GPMF segment
/// let gpmf_data = &[/* GPMF data */];
/// let metadata = parse_app6(gpmf_data)?;
/// assert!(metadata.contains_key("APP6:Model"));
/// ```
pub fn parse_app6(data: &[u8]) -> Result<MetadataMap> {
    parse_app6_ijpeg(data, false)
}

/// Parses APP6 segment data, optionally allowing the InfiRay MixMode layout.
///
/// InfiRay's APP6 record carries no identifier of its own: ExifTool only
/// reads it once an APP2 segment matching `/^....IJPEG\0/` has set
/// `$$self{HasIJPEG}` (ExifTool.pm:7968). `is_ijpeg` is that flag, which only
/// the caller walking the whole segment list can know.
///
/// # Arguments
///
/// * `data` - Raw APP6 segment data (excluding the APP6 marker and length bytes)
/// * `is_ijpeg` - Whether the file carries an InfiRay IJPEG APP2 version header
///
/// # Returns
///
/// * `Ok(MetadataMap)` - A metadata map containing extracted APP6 tags
/// * `Err(ExifToolError)` - If the data is malformed or unsupported
///
/// # Errors
///
/// Returns an error if the segment is too short to hold any identifier.
pub fn parse_app6_ijpeg(data: &[u8], is_ijpeg: bool) -> Result<MetadataMap> {
    // Minimum APP6 segment should have at least a few bytes
    if data.len() < 4 {
        return Err(ExifToolError::parse_error(
            "APP6 segment too short to contain valid metadata",
        ));
    }

    // Dispatch on the same identifier conditions ExifTool's actual READ path
    // uses (ExifTool.pm's ProcessJPEG APP6 handling, not JPEG.pm's table
    // Condition which is never consulted for reads), and in the same order:
    // EPPIM, NITF, HP TDHD, GoPro, then the identifier-less InfiRay MixMode.

    if data.starts_with(b"EPPIM\0") {
        return Ok(parse_eppim(data));
    }

    if data.starts_with(b"NITF\0") {
        return Ok(parse_nitf(&data[5..]));
    }

    // ExifTool also requires segment length > 12 for TDHD (ExifTool.pm:8146);
    // an 8-byte bare identifier extracts nothing.
    if data.starts_with(b"TDHD\x01\0\0\0") && data.len() > 12 {
        return parse_tdhd(data);
    }

    if data.starts_with(b"GoPro\0") {
        return parse_gpmf(&data[6..]);
    }

    if is_ijpeg && data.len() >= INFIRAY_MIXMODE_MIN_LENGTH {
        return Ok(parse_infiray_mix_mode(data));
    }

    // Unknown APP6 formats (DJI DTAT, Motorola MMIMETA, ...) extract
    // nothing, matching ExifTool's default (no -u) behavior.
    Ok(MetadataMap::new())
}

/// Parse Toshiba's EPPIM APP6 wrapper (`JPEG.pm` `%JPEG::EPPIM`).
///
/// `ExifTool.pm` removes the six-byte `EPPIM\0` identifier and calls
/// `ProcessTIFF`; the TIFF table declares only tag 0xc4a5, a PrintIM
/// SubDirectory. This follows that directory edge and never searches the
/// payload for a coincidental `PrintIM` byte sequence.
fn parse_eppim(data: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(tiff) = data.get(6..) else {
        return metadata;
    };
    if tiff.len() < 8 {
        return metadata;
    }

    let byte_order = match tiff.get(..2) {
        Some(b"II") => ByteOrder::LittleEndian,
        Some(b"MM") => ByteOrder::BigEndian,
        _ => return metadata,
    };
    let reader = EndianReader::new(tiff, byte_order.to_io_byte_order());
    if reader.u16_at(2) != Some(42) {
        return metadata;
    }
    let Some(ifd_offset) = reader.u32_at(4).map(|n| n as usize) else {
        return metadata;
    };
    let Some(entry_count) = reader.u16_at(ifd_offset).map(usize::from) else {
        return metadata;
    };

    for index in 0..entry_count {
        let Some(entry_at) = ifd_offset
            .checked_add(2)
            .and_then(|at| index.checked_mul(12).and_then(|n| at.checked_add(n)))
        else {
            break;
        };
        let (Some(tag), Some(field_type), Some(value_count), Some(value_or_offset)) = (
            reader.u16_at(entry_at),
            reader.u16_at(entry_at + 2),
            reader.u32_at(entry_at + 4),
            reader.u32_at(entry_at + 8),
        ) else {
            break;
        };
        if tag != 0xC4A5 {
            continue;
        }

        let Some(unit_size) = tiff_field_size(field_type) else {
            continue;
        };
        let Some(value_len) = unit_size.checked_mul(value_count as usize) else {
            continue;
        };
        let value = if value_len <= 4 {
            tiff.get(entry_at + 8..entry_at + 8 + value_len)
        } else {
            let start = value_or_offset as usize;
            start
                .checked_add(value_len)
                .and_then(|end| tiff.get(start..end))
        };
        if let Some(version) = value.and_then(|v| decode_print_im_version(v, byte_order)) {
            metadata.insert(PRINT_IM_VERSION_TAG, TagValue::new_string(version));
        }
        break;
    }

    metadata
}

fn tiff_field_size(field_type: u16) -> Option<usize> {
    Some(match field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

/// Maps GPMF FourCC codes to ExifTool tag names (GoPro.pm %GoPro::GPMF).
///
/// Entries ExifTool marks `Unknown => 1` (DVID, EMPT, TSMP, TYPE, STNM, UNIT,
/// ...) are omitted so they stay hidden, matching default ExifTool output.
/// Unmapped FourCCs are skipped entirely.
fn gopro_tag_name(fourcc: &str) -> Option<&'static str> {
    Some(match fourcc {
        "AALP" => "AudioLevel",
        "ABSC" => "AutoBoostScore",
        "ALLD" => "AutoLowLightDuration",
        "APTO" => "AudioProtuneOption",
        "ARUW" => "AspectRatioUnwarped",
        "ARWA" => "AspectRatioWarped",
        "AUBT" => "AudioBlueTooth",
        "AUDO" => "AudioSetting",
        "AUPT" => "AutoProtune",
        "BITR" => "BitrateSetting",
        "CASN" => "CameraSerialNumber",
        "CDAT" => "CreationDate",
        "CDTM" => "CaptureDelayTimer",
        "CLDP" => "ClassificationDataPresent",
        "CORI" => "CameraOrientation",
        "CPIN" => "ChapterNumber",
        "CTRL" => "ControlLevel",
        "DUST" => "DurationSetting",
        "DVNM" => "DeviceName",
        "DZMX" => "DigitalZoomAmount",
        "DZOM" => "DigitalZoomOn",
        "DZST" => "DigitalZoom",
        "EISA" => "ElectronicImageStabilization",
        "EISE" => "ElectronicStabilizationOn",
        "EXPT" => "ExposureType",
        "FACE" => "FaceDetected",
        "FCNM" => "FaceNumbers",
        "FMWR" => "FirmwareVersion",
        "FWVS" => "OtherFirmware",
        "GPSA" => "GPSAltitudeSystem",
        "GRAV" => "GravityVector",
        "HCTL" => "HorizonControl",
        "HDRV" => "HDRVideo",
        "HSGT" => "HindsightSettings",
        "HUES" => "PredominantHue",
        "IORI" => "ImageOrientation",
        "ISOE" => "ISOSpeeds",
        "LOGS" => "HealthLogs",
        "MAGN" => "Magnetometer",
        "MAPX" => "MappingXCoefficients",
        "MAPY" => "MappingYCoefficients",
        "MINF" => "Model",
        "MMOD" => "MediaMode",
        "MTRX" => "AccelerometerMatrix",
        // GoPro.pm's %GPMF hash lists MUID twice: the earlier entry names it
        // MediaUID, the later one (forum12825) MediaUniqueID. Perl keeps the
        // LAST duplicate key, so ExifTool reports MediaUniqueID.
        "MUID" => "MediaUniqueID",
        "MWET" => "MicrophoneWet",
        "MXCF" => "MappingXMode",
        "MYCF" => "MappingYMode",
        "ORDP" => "OrientationDataPresent",
        "OREN" => "AutoRotation",
        "ORIN" => "InputOrientation",
        "ORIO" => "OutputOrientation",
        "PHDR" => "HDRSetting",
        "PIMD" => "ProtuneISOMode",
        "PIMN" => "AutoISOMin",
        "PIMX" => "AutoISOMax",
        "POLY" => "PolynomialCoefficients",
        "PRES" => "PhotoResolution",
        "PRJT" => "LensProjection",
        "PRTN" => "Protune",
        "PTCL" => "ColorMode",
        "PTEV" => "ExposureCompensation",
        "PTSH" => "Sharpness",
        "PTWB" => "WhiteBalance",
        "PWPR" => "PowerProfile",
        "PYCF" => "PolynomialPower",
        "RAMP" => "SpeedRampSetting",
        "RATE" => "Rate",
        "SCAP" => "ScheduleCapture",
        "SCEN" => "SceneClassification",
        "SCTM" => "ScheduleCaptureTime",
        "SMTR" => "SpotMeter",
        "SROT" => "SensorReadoutTime",
        "TIMO" => "TimeOffset",
        "TZON" => "TimeZone",
        "UNIF" => "InputUniformity",
        "VERS" => "MetadataVersion",
        "VFOV" => "FieldOfView",
        "VFPS" => "VideoFrameRate",
        "VRES" => "VideoFrameSize",
        "WBAL" => "ColorTemperatures",
        "WNDM" => "WindProcessing",
        "YAVG" => "LumaAverage",
        "ZFOV" => "DiagonalFieldOfView",
        "ZMPL" => "ZoomScaleNormalization",
        _ => return None,
    })
}

/// GoPro tags ExifTool marks `Binary => 1`, whose value it never prints.
///
/// The byte count in ExifTool's placeholder is the length of the *converted*
/// value string, not of the record: CORI's four quaternion floats occupy 16
/// bytes on disk but convert to "1 0 0 0" and report 7.
const BINARY_TAGS: &[&str] = &["CORI", "GRAV", "IORI"];

/// Renders a Unix epoch second the way ExifTool's `ConvertUnixTime` does.
///
/// The conversion is UTC: `ConvertUnixTime` only reaches for `localtime` when
/// passed a true second argument, and GoPro.pm's CDAT passes none.
fn convert_unix_time(seconds: i64) -> String {
    // ExifTool reports the epoch itself as an all-zero date, not as 1970.
    if seconds == 0 {
        return "0000:00:00 00:00:00".to_string();
    }
    chrono::DateTime::from_timestamp(seconds, 0)
        .map_or_else(|| seconds.to_string(), |dt| format_exif_datetime(&dt))
}

/// Renders a signed count of minutes as ExifTool's `TimeZoneString` does.
fn time_zone_string(minutes: i64) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let minutes = minutes.unsigned_abs();
    format!("{}{:02}:{:02}", sign, minutes / 60, minutes % 60)
}

/// Substitutes ExifTool's placeholder for a tag it declares `Binary => 1`.
fn binary_placeholder(value: &TagValue) -> TagValue {
    let printed = match value {
        TagValue::String(s) => s.clone(),
        TagValue::Integer(i) => i.to_string(),
        TagValue::Float(f) => perl_number(*f),
        // Unreachable: every GoPro tag in BINARY_TAGS is a numeric record,
        // and those decode to exactly the three variants above.
        other => return other.clone(),
    };
    TagValue::String(format!(
        "(Binary data {} bytes, use -b option to extract)",
        printed.len()
    ))
}

/// Applies ExifTool print conversions for the GoPro tags that define them.
fn gopro_print_conv(fourcc: &str, value: TagValue) -> TagValue {
    // Tags using %noYes = ( N => 'No', Y => 'Yes' ) in GoPro.pm
    const NO_YES_TAGS: &[&str] = &[
        "AUBT", "AUPT", "CLDP", "DZOM", "EISE", "HDRV", "ORDP", "SCAP", "SMTR",
    ];

    // The two integer-valued conversions come first; the string paths below
    // would pass their values through untouched.
    if let Some(n) = value.as_integer() {
        match fourcc {
            // CDAT: RawConv => 'ConvertUnixTime($val)'
            "CDAT" => return TagValue::String(convert_unix_time(n)),
            // TZON: PrintConv => 'Image::ExifTool::TimeZoneString($val)'
            "TZON" => return TagValue::String(time_zone_string(n)),
            _ => {}
        }
    }

    // MUID (MediaUniqueID) is a list of int32u rendered as concatenated
    // zero-padded hex:
    //   PrintConv => 'my @a = split " ", $val;
    //                 $_ = sprintf("%.8x",$_) foreach @a; join("", @a)'
    // The list arrives here already space-joined (or as a lone Integer when
    // the record holds a single element).
    if fourcc == "MUID" {
        return match &value {
            TagValue::String(s) => TagValue::String(
                s.split_whitespace()
                    .map(|w| match w.parse::<u32>() {
                        Ok(n) => format!("{:08x}", n),
                        // Anything that isn't an int32u is passed through
                        // rather than silently rounded to a neighbouring value.
                        Err(_) => w.to_string(),
                    })
                    .collect::<String>(),
            ),
            TagValue::Integer(n) => match u32::try_from(*n) {
                Ok(n) => TagValue::String(format!("{:08x}", n)),
                Err(_) => value,
            },
            _ => value,
        };
    }

    let TagValue::String(s) = &value else {
        return value;
    };
    let mapped = match (fourcc, s.as_str()) {
        ("OREN", "U") => "Up",
        ("OREN", "D") => "Down",
        ("OREN", "A") => "Auto",
        ("PRTN", "N") => "Off",
        ("PRTN", "Y") => "On",
        ("VFOV", "W") => "Wide",
        ("VFOV", "S") => "Super View",
        ("VFOV", "L") => "Linear",
        // VERS: PrintConv => '$val =~ tr/ /./; $val' (e.g. "7 6 5" -> "7.6.5")
        ("VERS", _) => return TagValue::String(s.replace(' ', ".")),
        (f, "N") if NO_YES_TAGS.contains(&f) => "No",
        (f, "Y") if NO_YES_TAGS.contains(&f) => "Yes",
        _ => return value,
    };
    TagValue::String(mapped.to_string())
}

/// Parses GoPro GPMF (GoPro Metadata Format) data.
///
/// GPMF uses a hierarchical TLV (Tag-Length-Value) structure with FourCC tags.
/// This parser extracts camera settings, telemetry, and device information.
///
/// # Arguments
///
/// * `data` - Raw GPMF data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Extracted GoPro metadata
/// * `Err(ExifToolError)` - If parsing fails
///
/// # GPMF Structure
///
/// Each GPMF record:
/// - FourCC (4 bytes) - Tag identifier (ASCII)
/// - Type (1 byte) - Data type ('b'=byte, 's'=short, 'l'=long, 'f'=float, 'c'=string, etc.)
/// - Size (1 byte) - Bytes per element
/// - Count (2 bytes, BE) - Number of elements
/// - Data (variable) - Padded to 4-byte alignment
///
/// # Example Tags
///
/// - DEVC: Device container
/// - DVNM: Device name (camera model)
/// - FWVS: Firmware version
/// - STNM: Stream name
/// - CAMD: Camera metadata
fn parse_gpmf(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();
    parse_gpmf_records(data, &mut metadata, 0);
    Ok(metadata)
}

/// Maximum nesting depth for GPMF container records (format 0). Guards
/// against pathological/malicious streams driving unbounded recursion;
/// containers nested beyond this depth are skipped rather than recursed
/// into, but sibling records at the current level continue to be walked.
const MAX_GPMF_DEPTH: u8 = 16;

/// Walks GPMF TLV records, inserting known tags into the metadata map.
///
/// Mirrors ExifTool's ProcessGoPro: stops at the null tag ("\0\0\0\0") or at
/// a FourCC containing characters outside [-_a-zA-Z0-9 ]; skips FourCCs
/// without a known tag name; recurses into container records (format 0).
fn parse_gpmf_records(data: &[u8], metadata: &mut MetadataMap, depth: u8) {
    let mut offset = 0;

    while offset + 8 <= data.len() {
        let fourcc_bytes = &data[offset..offset + 4];
        let format = data[offset + 4];
        let size = data[offset + 5] as usize;
        let reader = EndianReader::big_endian(&data[offset + 6..]);
        let count = reader.u16_at(0).unwrap_or(0) as usize;
        offset += 8;

        // Stop at the null terminator record
        if fourcc_bytes == [0, 0, 0, 0] {
            break;
        }
        // Stop on malformed FourCCs (ExifTool: 'Unrecognized GoPro record')
        if !fourcc_bytes
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b' ')
        {
            break;
        }

        let data_size = size * count;
        if offset + data_size > data.len() {
            break; // Truncated record (ExifTool: 'Truncated GoPro record')
        }
        let value_data = &data[offset..offset + data_size];
        offset += (data_size + 3) & !3; // data is padded to a 4-byte boundary

        let fourcc = std::str::from_utf8(fourcc_bytes).unwrap_or_default();

        // Containers (format 0, e.g. DEVC/STRM) nest further GPMF records.
        // Beyond MAX_GPMF_DEPTH, skip recursing into the container but keep
        // walking its siblings at the current level.
        if format == 0 {
            if depth < MAX_GPMF_DEPTH {
                parse_gpmf_records(value_data, metadata, depth + 1);
            }
            continue;
        }

        // Unknown FourCCs are extracted by ExifTool only with -u; skip them.
        let Some(tag_name) = gopro_tag_name(fourcc) else {
            continue;
        };
        if let Some(value) = decode_gpmf_value(format, size, count, value_data) {
            let value = gopro_print_conv(fourcc, value);
            let value = if BINARY_TAGS.contains(&fourcc) {
                binary_placeholder(&value)
            } else {
                value
            };
            metadata.insert(format!("APP6:{}", tag_name), value);
        }
    }
}

/// Byte width of one scalar of a GPMF format code.
///
/// This is ExifTool's `%goProFmt` (GPMF code -> ExifTool format) composed with
/// `FormatSize`, plus the `%goProSize` overrides for the three fixed-width
/// codes that map to 'undef'. Codes absent from ExifTool's table also read as
/// 'undef', whose scalar is a single byte.
fn gpmf_scalar_width(format: u8) -> usize {
    match format {
        b'b' | b'B' | b'c' => 1,
        b's' | b'S' => 2,
        b'l' | b'L' | b'f' | b'q' => 4,
        b'j' | b'J' | b'd' | b'Q' => 8,
        b'F' => 4,         // FourCC   (%goProSize)
        b'G' | b'U' => 16, // UUID, 16-byte date (%goProSize)
        _ => 1,            // 'undef'
    }
}

/// True for the GPMF codes ExifTool reads as text or as opaque bytes.
///
/// These are the formats whose multi-element records it unpacks as a
/// fixed-width list rather than reading straight through.
fn is_textual_format(format: u8) -> bool {
    !matches!(
        format,
        b'b' | b'B' | b's' | b'S' | b'l' | b'L' | b'j' | b'J' | b'f' | b'd' | b'q' | b'Q'
    )
}

/// Decodes a GPMF record payload into a TagValue.
///
/// Mirrors ExifTool's ProcessGoPro. Two of its rules are easy to get wrong:
///
/// 1. A record's `count` is the number of *structures*, and `size` their
///    stride -- not the number of scalars. ExifTool multiplies them into a
///    byte length and hands that to ReadValue, which derives the scalar count
///    from the format's own width (`int($size / $len)`). POLY arrives as one
///    28-byte element and is seven floats.
/// 2. Textual and opaque formats are the exception: with more than one
///    element of more than one byte, ExifTool unpacks them as a fixed-width
///    list, `A$len` for strings (which drops the padding) and `a$len` for
///    'undef' (which does not). PYCF's seven NUL-padded 8-byte slots are
///    seven elements, not one 56-byte blob.
///
/// Multi-scalar numeric records join with a space, as ReadValue does. Floats
/// render through [`perl_number`], since ExifTool is just interpolating a
/// Perl double into a string.
fn decode_gpmf_value(format: u8, size: usize, count: usize, data: &[u8]) -> Option<TagValue> {
    if data.is_empty() {
        return None;
    }

    // Rule 2: fixed-width list of textual/opaque elements.
    if is_textual_format(format) && count > 1 && size > 1 {
        let elements: Option<Vec<TagValue>> = data
            .chunks_exact(size)
            .map(|chunk| decode_gpmf_list_element(format, chunk).map(TagValue::String))
            .collect();
        // A slot that is not text leaves the whole list undecodable; keep the
        // payload rather than emitting a list with holes punched in it.
        return Some(match elements.filter(|e| !e.is_empty()) {
            Some(elements) => TagValue::Array(elements),
            None => TagValue::Binary(data.to_vec()),
        });
    }

    // Rule 1: one ReadValue over the whole payload, counting scalars by width.
    let width = gpmf_scalar_width(format);
    // ReadValue yields nothing when the payload cannot hold a single scalar.
    let scalars = data.len() / width;
    if scalars == 0 {
        return None;
    }
    let reader = EndianReader::big_endian(data);

    let int_at = |off: usize| -> Option<i64> {
        match format {
            b'b' => reader.i8_at(off).map(i64::from),
            b'B' => reader.u8_at(off).map(i64::from),
            b's' => reader.i16_at(off).map(i64::from),
            b'S' => reader.u16_at(off).map(i64::from),
            b'l' => reader.i32_at(off).map(i64::from),
            b'L' => reader.u32_at(off).map(i64::from),
            b'j' => reader.i64_at(off),
            b'J' => reader.u64_at(off).map(|v| v as i64),
            _ => None,
        }
    };

    match format {
        b'b' | b'B' | b's' | b'S' | b'l' | b'L' | b'j' | b'J' => {
            let values: Vec<i64> = (0..scalars).map_while(|i| int_at(i * width)).collect();
            match values.as_slice() {
                [] => None,
                [single] => Some(TagValue::Integer(*single)),
                many => Some(TagValue::String(
                    many.iter()
                        .map(i64::to_string)
                        .collect::<Vec<_>>()
                        .join(" "),
                )),
            }
        }
        b'f' | b'd' => {
            let float_at = |off: usize| -> Option<f64> {
                if format == b'f' {
                    reader.f32_at(off).map(f64::from)
                } else {
                    reader.f64_at(off)
                }
            };
            let values: Vec<String> = (0..scalars)
                .map_while(|i| float_at(i * width))
                .map(perl_number)
                .collect();
            (!values.is_empty()).then(|| TagValue::String(values.join(" ")))
        }
        // Everything else reads as text or as raw bytes: 'c' strings, plus
        // the 'undef' codes 'F' (FourCC, e.g. PRJT's "GPRO"), 'G' (UUID),
        // 'U' (date) and '?' (TYPE-defined structure, which we cannot decode
        // without tracking TYPE). 'q'/'Q' fixed-point is not yet handled and
        // falls through here too.
        _ => Some(match decode_gpmf_text(format, data) {
            Some(text) => TagValue::String(text),
            None => TagValue::Binary(data.to_vec()),
        }),
    }
}

/// Decodes a whole textual GPMF payload, as ExifTool's ReadValue does.
///
/// A 'string' is truncated at its first NUL (ExifTool.pm:6311,
/// `s/\0.*//s`); every other code reads as 'undef', whose bytes pass through
/// untouched. Returns `None` when the bytes are not valid UTF-8, leaving the
/// caller to keep them as binary.
fn decode_gpmf_text(format: u8, data: &[u8]) -> Option<String> {
    let bytes = if format == b'c' {
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        &data[..end]
    } else {
        data
    };
    std::str::from_utf8(bytes).ok().map(str::to_string)
}

/// Decodes one fixed-width slot of a GPMF list, as Perl's unpack does.
///
/// `A$len` (strings) strips trailing spaces and NULs from each slot -- note
/// that it strips the *padding*, and does not cut at the first NUL the way
/// ReadValue does. `a$len` ('undef') passes the bytes through untouched.
fn decode_gpmf_list_element(format: u8, chunk: &[u8]) -> Option<String> {
    let bytes = if format == b'c' {
        let end = chunk
            .iter()
            .rposition(|&b| b != 0 && b != b' ')
            .map_or(0, |i| i + 1);
        &chunk[..end]
    } else {
        chunk
    };
    std::str::from_utf8(bytes).ok().map(str::to_string)
}

/// Parses TDHD (True Definition High Definition) metadata.
///
/// TDHD is used by HP and Toshiba cameras for stereo/3D image metadata.
/// The format stores information about left/right eye images and depth maps.
///
/// # Arguments
///
/// * `data` - Raw TDHD data (starts with "TDHD" identifier)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Extracted TDHD metadata
/// * `Err(ExifToolError)` - If parsing fails
fn parse_tdhd(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Caller has verified the "TDHD\x01\0\0\0" identifier (8 bytes).
    // Detailed field parsing (ExifTool HP.pm %HP::TDHD) is not yet ported;
    // expose the raw payload for now.
    metadata.insert(
        "APP6:TDHDData".to_string(),
        TagValue::Binary(data[8..].to_vec()),
    );

    Ok(metadata)
}

/// Parses NITF (National Imagery Transmission Format) metadata
/// (`%Image::ExifTool::JPEG::NITF`).
///
/// NITF is used for geospatial imagery metadata in defense/intelligence
/// applications. The table declares no `FORMAT`, so it defaults to `int8u`
/// and the numeric keys are plain byte offsets into the record; ExifTool
/// reads it big-endian (`SetByteOrder('MM')`, ExifTool.pm:8142).
///
/// # Arguments
///
/// * `data` - NITF record, i.e. the APP6 payload AFTER the "NITF\0" identifier
///
/// # Returns
///
/// A metadata map of the NITF tags present; a short record simply yields
/// fewer tags, as ExifTool's binary-data reader does.
fn parse_nitf(data: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let mut put = |name: &str, value: TagValue| {
        metadata.insert(format!("APP6:{}", name), value);
    };
    // PrintConv hash misses report the raw code, never a neighbouring label.
    let lookup = |code: i64, table: &[(i64, &str)]| -> TagValue {
        match table.iter().find(|(k, _)| *k == code) {
            Some((_, label)) => TagValue::String((*label).to_string()),
            None => TagValue::String(format!("Unknown ({})", code)),
        }
    };
    let be_u16 = |off: usize| -> Option<u16> {
        data.get(off..off + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };

    // 0: NITFVersion, int8u[2], ValueConv sprintf("%d.%.2d", ...)
    if let Some(pair) = data.get(0..2) {
        put(
            "NITFVersion",
            TagValue::String(format!("{}.{:02}", pair[0], pair[1])),
        );
    }
    // 2: ImageFormat, ValueConv chr($val & 0xff), PrintConv { B => 'IMode B' }
    if let Some(&code) = data.get(2) {
        let letter = code as char;
        put(
            "ImageFormat",
            if letter == 'B' {
                TagValue::String("IMode B".to_string())
            } else {
                TagValue::String(format!("Unknown ({})", letter))
            },
        );
    }
    if let Some(v) = be_u16(3) {
        put("BlocksPerRow", TagValue::Integer(v as i64));
    }
    if let Some(v) = be_u16(5) {
        put("BlocksPerColumn", TagValue::Integer(v as i64));
    }
    if let Some(&code) = data.get(7) {
        put("ImageColor", lookup(code as i64, &[(0, "Monochrome")]));
    }
    if let Some(&v) = data.get(8) {
        put("BitDepth", TagValue::Integer(v as i64));
    }
    if let Some(&code) = data.get(9) {
        put(
            "ImageClass",
            lookup(
                code as i64,
                &[(0, "General Purpose"), (4, "Tactical Imagery")],
            ),
        );
    }
    if let Some(&code) = data.get(10) {
        put(
            "JPEGProcess",
            lookup(
                code as i64,
                &[
                    (1, "Baseline sequential DCT, Huffman coding, 8-bit samples"),
                    (4, "Extended sequential DCT, Huffman coding, 12-bit samples"),
                ],
            ),
        );
    }
    if let Some(&v) = data.get(11) {
        put("Quality", TagValue::Integer(v as i64));
    }
    if let Some(&code) = data.get(12) {
        put("StreamColor", lookup(code as i64, &[(0, "Monochrome")]));
    }
    if let Some(&v) = data.get(13) {
        put("StreamBitDepth", TagValue::Integer(v as i64));
    }
    // 14: Flags, int32u, PrintConv sprintf("0x%x", $val)
    if let Some(bytes) = data.get(14..18) {
        let flags = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        put("Flags", TagValue::String(format!("0x{:x}", flags)));
    }

    metadata
}

/// Parses an InfiRay IJPEG visual/infrared mixing-mode record
/// (`%Image::ExifTool::InfiRay::MixMode`), read little-endian
/// (`SetByteOrder('II')`, ExifTool.pm:8164).
///
/// The record carries no identifier; the caller decides whether the file is
/// an IJPEG. Offsets are byte offsets: MixMode int8u at 0x00,
/// FusionIntensity float at 0x01, OffsetAdjustment float at 0x05, and
/// CorrectionAsix float[30] at 0x09.
fn parse_infiray_mix_mode(data: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let le_f32 = |off: usize| -> Option<f32> {
        data.get(off..off + 4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };

    if let Some(&mode) = data.first() {
        metadata.insert("APP6:MixMode".to_string(), TagValue::Integer(mode as i64));
    }
    // PrintConv => 'sprintf("%.1f %%", $val * 100)'
    if let Some(intensity) = le_f32(0x01) {
        metadata.insert(
            "APP6:FusionIntensity".to_string(),
            TagValue::String(format!("{:.1} %", intensity as f64 * 100.0)),
        );
    }
    if let Some(offset) = le_f32(0x05) {
        metadata.insert(
            "APP6:OffsetAdjustment".to_string(),
            TagValue::String(perl_number(offset as f64)),
        );
    }
    // float[30]: ExifTool joins list elements with a single space.
    let axis: Option<Vec<String>> = (0..30)
        .map(|i| le_f32(0x09 + i * 4).map(|v| perl_number(v as f64)))
        .collect();
    if let Some(axis) = axis {
        metadata.insert(
            "APP6:CorrectionAsix".to_string(),
            TagValue::String(axis.join(" ")),
        );
    }

    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one GPMF TLV record: FourCC + format + element size + count +
    /// data padded to a 4-byte boundary.
    fn gpmf_record(fourcc: &[u8; 4], fmt: u8, size: u8, count: u16, data: &[u8]) -> Vec<u8> {
        let mut rec = fourcc.to_vec();
        rec.push(fmt);
        rec.push(size);
        rec.extend_from_slice(&count.to_be_bytes());
        rec.extend_from_slice(data);
        while rec.len() % 4 != 0 {
            rec.push(0);
        }
        rec
    }

    /// APP6 payload as written by GoPro cameras: "GoPro\0" + GPMF records.
    fn gopro_payload(records: &[Vec<u8>]) -> Vec<u8> {
        let mut p = b"GoPro\0".to_vec();
        for rec in records {
            p.extend_from_slice(rec);
        }
        p
    }

    #[test]
    fn test_parse_app6_gopro_maps_fourccs_to_exiftool_names() {
        let payload = gopro_payload(&[
            gpmf_record(b"MINF", b'c', 1, 11, b"HERO8 Black"),
            gpmf_record(b"CASN", b'c', 1, 14, b"C3221324545448"),
            gpmf_record(b"FMWR", b'c', 1, 15, b"HD8.01.01.60.00"),
            gpmf_record(b"RATE", b'c', 1, 6, b"4_1SEC"),
        ]);
        let metadata = parse_app6(&payload).unwrap();
        // ExifTool 13.55: -G1 group GoPro, tag names from GoPro.pm GPMF table
        assert_eq!(metadata.get_string("APP6:Model"), Some("HERO8 Black"));
        assert_eq!(
            metadata.get_string("APP6:CameraSerialNumber"),
            Some("C3221324545448")
        );
        assert_eq!(
            metadata.get_string("APP6:FirmwareVersion"),
            Some("HD8.01.01.60.00")
        );
        assert_eq!(metadata.get_string("APP6:Rate"), Some("4_1SEC"));
    }

    #[test]
    fn test_parse_app6_gopro_print_conversions() {
        let payload = gopro_payload(&[
            gpmf_record(b"OREN", b'c', 1, 1, b"U"),
            gpmf_record(b"PRTN", b'c', 1, 1, b"N"),
            gpmf_record(b"VERS", b'B', 1, 3, &[7, 6, 5]),
        ]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("APP6:AutoRotation"), Some("Up"));
        assert_eq!(metadata.get_string("APP6:Protune"), Some("Off"));
        assert_eq!(metadata.get_string("APP6:MetadataVersion"), Some("7.6.5"));
    }

    #[test]
    fn test_parse_app6_gopro_media_unique_id_is_concatenated_hex() {
        // ExifTool 13.55 on combined-samples/GoPro.jpg:
        //   MediaUniqueID : 491b313ca89d1416...
        let payload = gopro_payload(&[gpmf_record(
            b"MUID",
            b'L',
            4,
            3,
            &[
                0x49, 0x1b, 0x31, 0x3c, 0xa8, 0x9d, 0x14, 0x16, 0x00, 0x00, 0x00, 0x00,
            ],
        )]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(
            metadata.get_string("APP6:MediaUniqueID"),
            Some("491b313ca89d141600000000")
        );
        // The pre-forum12825 name must not be emitted
        assert!(metadata.get("APP6:MediaUID").is_none());
    }

    #[test]
    fn test_parse_app6_gopro_numeric_values() {
        let payload = gopro_payload(&[
            gpmf_record(b"PIMX", b'L', 4, 1, &1600u32.to_be_bytes()),
            gpmf_record(b"PIMN", b'L', 4, 1, &100u32.to_be_bytes()),
        ]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_integer("APP6:AutoISOMax"), Some(1600));
        assert_eq!(metadata.get_integer("APP6:AutoISOMin"), Some(100));
    }

    #[test]
    fn test_parse_app6_gopro_unknown_fourcc_skipped() {
        // ExifTool extracts unknown GPMF tags only with the -u option;
        // known tags around it still parse.
        let payload = gopro_payload(&[
            gpmf_record(b"XXXX", b'c', 1, 4, b"junk"),
            gpmf_record(b"RATE", b'c', 1, 6, b"4_1SEC"),
        ]);
        let metadata = parse_app6(&payload).unwrap();
        assert!(metadata.get("APP6:XXXX").is_none());
        assert_eq!(metadata.get_string("APP6:Rate"), Some("4_1SEC"));
    }

    #[test]
    fn test_parse_app6_gopro_container_recursion() {
        // DEVC (format 0) nests further GPMF records
        let inner = gpmf_record(b"DVNM", b'c', 1, 11, b"HERO8 Black");
        let payload = gopro_payload(&[gpmf_record(b"DEVC", 0, 1, inner.len() as u16, &inner)]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("APP6:DeviceName"), Some("HERO8 Black"));
    }

    #[test]
    fn test_parse_app6_gopro_stops_at_null_tag() {
        let mut records = vec![gpmf_record(b"RATE", b'c', 1, 6, b"4_1SEC")];
        records.push(gpmf_record(&[0, 0, 0, 0], 0, 0, 0, &[]));
        records.push(gpmf_record(b"CASN", b'c', 1, 4, b"1234"));
        let payload = gopro_payload(&records);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("APP6:Rate"), Some("4_1SEC"));
        // Records after the null terminator are not parsed (ExifTool behavior)
        assert!(metadata.get("APP6:CameraSerialNumber").is_none());
    }

    /// Wraps `inner` in `levels` nested DEVC container records (format 0).
    fn nest_gpmf(levels: usize, inner: Vec<u8>) -> Vec<u8> {
        let mut cur = inner;
        for _ in 0..levels {
            cur = gpmf_record(b"DEVC", 0, 1, cur.len() as u16, &cur);
        }
        cur
    }

    #[test]
    fn test_parse_app6_gpmf_recursion_depth_capped() {
        let rate = gpmf_record(b"RATE", b'c', 1, 6, b"4_1SEC");

        // RATE nested 40 DEVC containers deep, well beyond the recursion
        // cap (16) — parsing must complete (no stack overflow) and the
        // innermost record must NOT be extracted since it's unreachable.
        let deep = nest_gpmf(40, rate.clone());
        let deep_payload = gopro_payload(&[deep]);
        let deep_metadata = parse_app6(&deep_payload).unwrap();
        assert_eq!(deep_metadata.get_string("APP6:Rate"), None);

        // Shallow control: RATE nested only 2 levels deep, well within the
        // cap — must still be extracted normally.
        let shallow = nest_gpmf(2, rate);
        let shallow_payload = gopro_payload(&[shallow]);
        let shallow_metadata = parse_app6(&shallow_payload).unwrap();
        assert_eq!(shallow_metadata.get_string("APP6:Rate"), Some("4_1SEC"));
    }

    /// The APP6 NITF record of combined-samples/ExifTool.jpg, byte for byte.
    /// Expected values from `exiftool -G0 -s combined-samples/ExifTool.jpg`
    /// (ExifTool 13.55).
    #[test]
    fn test_parse_app6_nitf_matches_exiftool() {
        let mut payload = b"NITF\0".to_vec();
        payload.extend_from_slice(&[
            0x02, 0x00, // NITFVersion 2.00
            0x42, // ImageFormat 'B'
            0x00, 0x01, // BlocksPerRow 1
            0x00, 0x01, // BlocksPerColumn 1
            0x00, // ImageColor Monochrome
            0x08, // BitDepth 8
            0x00, // ImageClass General Purpose
            0x01, // JPEGProcess baseline
            0x01, // Quality 1
            0x00, // StreamColor Monochrome
            0x08, // StreamBitDepth 8
            0x01, 0x01, 0x00, 0x00, // Flags 0x1010000
        ]);
        let m = parse_app6(&payload).unwrap();

        assert_eq!(m.get_string("APP6:NITFVersion"), Some("2.00"));
        assert_eq!(m.get_string("APP6:ImageFormat"), Some("IMode B"));
        assert_eq!(m.get_integer("APP6:BlocksPerRow"), Some(1));
        assert_eq!(m.get_integer("APP6:BlocksPerColumn"), Some(1));
        assert_eq!(m.get_string("APP6:ImageColor"), Some("Monochrome"));
        assert_eq!(m.get_integer("APP6:BitDepth"), Some(8));
        assert_eq!(m.get_string("APP6:ImageClass"), Some("General Purpose"));
        assert_eq!(
            m.get_string("APP6:JPEGProcess"),
            Some("Baseline sequential DCT, Huffman coding, 8-bit samples")
        );
        assert_eq!(m.get_integer("APP6:Quality"), Some(1));
        assert_eq!(m.get_string("APP6:StreamColor"), Some("Monochrome"));
        assert_eq!(m.get_integer("APP6:StreamBitDepth"), Some(8));
        assert_eq!(m.get_string("APP6:Flags"), Some("0x1010000"));
        assert_eq!(m.len(), 12);
        // The raw-blob placeholder this table replaced must be gone
        assert!(m.get("APP6:NITFData").is_none());
    }

    #[test]
    fn test_parse_app6_eppim_follows_its_tiff_print_im_directory() {
        // ExifTool.jpg's structure reduced to one EPPIM IFD entry. Offsets in
        // this embedded TIFF are measured after the six-byte EPPIM identifier.
        let mut payload = b"EPPIM\0II\x2a\0\x08\0\0\0\x01\0".to_vec();
        payload.extend_from_slice(&0xC4A5u16.to_le_bytes());
        payload.extend_from_slice(&7u16.to_le_bytes()); // UNDEFINED
        payload.extend_from_slice(&22u32.to_le_bytes());
        payload.extend_from_slice(&26u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes()); // next IFD
        payload.extend_from_slice(b"PrintIM\0");
        payload.extend_from_slice(b"0250");
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&[0; 6]);

        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("PrintIM:PrintIMVersion"), Some("0250"));
        assert_eq!(metadata.len(), 1);
    }

    #[test]
    fn test_parse_app6_eppim_does_not_scan_for_print_im() {
        let mut payload = b"EPPIM\0II\x2a\0\x08\0\0\0\0\0\0\0\0\0".to_vec();
        payload.extend_from_slice(b"PrintIM\0");
        payload.extend_from_slice(b"0250\0\0\0\0");

        assert!(parse_app6(&payload).unwrap().is_empty());
    }

    #[test]
    fn test_parse_app6_nitf_unknown_codes_report_themselves() {
        let mut payload = b"NITF\0".to_vec();
        payload.extend_from_slice(&[
            0x02, 0x00, 0x43, // ImageFormat 'C' -- no PrintConv entry
            0x00, 0x01, 0x00, 0x01, 0x09, // ImageColor 9
            0x08, 0x09, // ImageClass 9
            0x09, // JPEGProcess 9
            0x01, 0x09, // StreamColor 9
            0x08, 0x00, 0x00, 0x00, 0x00,
        ]);
        let m = parse_app6(&payload).unwrap();
        assert_eq!(m.get_string("APP6:ImageFormat"), Some("Unknown (C)"));
        assert_eq!(m.get_string("APP6:ImageColor"), Some("Unknown (9)"));
        assert_eq!(m.get_string("APP6:ImageClass"), Some("Unknown (9)"));
        assert_eq!(m.get_string("APP6:JPEGProcess"), Some("Unknown (9)"));
        assert_eq!(m.get_string("APP6:StreamColor"), Some("Unknown (9)"));
    }

    /// The APP6 payload of combined-samples/InfiRay.jpg. Expected values from
    /// `exiftool -G0 -s combined-samples/InfiRay.jpg` (ExifTool 13.55).
    #[test]
    fn test_parse_app6_infiray_mix_mode_matches_exiftool() {
        let mut payload = vec![0x00]; // MixMode 0
        payload.extend_from_slice(&1.0f32.to_le_bytes()); // FusionIntensity
        payload.extend_from_slice(&2.0f32.to_le_bytes()); // OffsetAdjustment
        payload.resize(192, 0); // CorrectionAsix float[30], all zero

        let m = parse_app6_ijpeg(&payload, true).unwrap();
        assert_eq!(m.get_integer("APP6:MixMode"), Some(0));
        assert_eq!(m.get_string("APP6:FusionIntensity"), Some("100.0 %"));
        assert_eq!(m.get_string("APP6:OffsetAdjustment"), Some("2"));
        assert_eq!(
            m.get_string("APP6:CorrectionAsix"),
            Some(["0"; 30].join(" ").as_str())
        );
        assert_eq!(m.len(), 4);

        // Without the APP2 IJPEG version header ExifTool reads nothing here,
        // because this record has no identifier of its own.
        assert!(parse_app6_ijpeg(&payload, false).unwrap().is_empty());

        // ExifTool's gate is `$length >= 129`
        let short = vec![0u8; 128];
        assert!(parse_app6_ijpeg(&short, true).unwrap().is_empty());
    }

    #[test]
    fn test_parse_app6_nitf_requires_nitf_identifier() {
        // ExifTool's actual READ dispatch (ExifTool.pm:8140) matches
        // `/^NITF\0/` with DirStart=5; JPEG.pm's table Condition ("NTIF\0")
        // never governs reads. Verified empirically against exiftool 13.55:
        // a "NITF\0" APP6 payload yields NITF:* tags; a "NTIF\0" payload
        // yields only an "Unknown APP6 'NTIF' segment" warning, no tags.
        let mut nitf = b"NITF\0".to_vec();
        nitf.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        let metadata = parse_app6(&nitf).unwrap();
        assert!(metadata.contains_key("APP6:NITFVersion"));

        // "NTIF\0" must NOT match the real dispatch condition
        let mut ntif = b"NTIF\0".to_vec();
        ntif.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        let metadata = parse_app6(&ntif).unwrap();
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_app6_tdhd_requires_version_bytes_and_length_over_12() {
        // ExifTool's gate is "TDHD\x01\0\0\0" AND segment length > 12
        // (ExifTool.pm:8146: `/^TDHD\x01\0\0\0/ and $length > 12`).
        let mut tdhd = b"TDHD\x01\0\0\0".to_vec(); // 8-byte identifier
        tdhd.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]); // 13 bytes total, > 12
        let metadata = parse_app6(&tdhd).unwrap();
        assert!(metadata.contains_key("APP6:TDHDData"));

        // Bare "TDHD" without the version bytes must NOT match
        let bare = b"TDHDxxxx".to_vec();
        let metadata = parse_app6(&bare).unwrap();
        assert!(metadata.is_empty());

        // Exactly 12 bytes (identifier + 4 more) fails the "length > 12" gate
        let mut exactly_12 = b"TDHD\x01\0\0\0".to_vec();
        exactly_12.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 12 bytes total
        let metadata = parse_app6(&exactly_12).unwrap();
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_app6_unknown_format_yields_no_tags() {
        // ExifTool ignores unrecognized APP6 payloads (without -u); no
        // binary-blob tag is emitted.
        let data = b"UNKN\x00\x00\x00\x00";
        let metadata = parse_app6(data).unwrap();
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_app6_too_short() {
        let data = b"AB";
        let result = parse_app6(data);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------------
    // GPMF value conversion, against exiftool 13.59 on the real samples.
    //
    // Every record below is byte-for-byte the one in the named sample (see
    // the APP6 "GoPro" segment), and every expectation is the corresponding
    // line of `exiftool -G -a -s <sample>`.
    // ------------------------------------------------------------------------

    #[test]
    fn test_gpmf_size_is_a_struct_stride_not_a_scalar_count() {
        // GoProHERO12Black.jpg: POLY fmt='f' size=28 count=1 -- one 28-byte
        // structure, which ReadValue splits into seven floats. Reading
        // `count` as the scalar count yields only the leading 0.
        //   [APP6] PolynomialCoefficients : 0 2.10214114189148 0.142030909657478
        //          -1.01693856716156 0.630049288272858 -2.19073308213753e-13
        //          1.06550110802548e-13
        let poly = [
            0x00, 0x00, 0x00, 0x00, 0x40, 0x06, 0x89, 0x7b, 0x3e, 0x11, 0x70, 0x8d, 0xbf, 0x82,
            0x2b, 0x0b, 0x3f, 0x21, 0x4a, 0xe9, 0xaa, 0x76, 0xa7, 0x95, 0x29, 0xef, 0xed, 0xf5,
        ];
        let payload = gopro_payload(&[gpmf_record(b"POLY", b'f', 28, 1, &poly)]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(
            metadata.get_string("APP6:PolynomialCoefficients"),
            Some(
                "0 2.10214114189148 0.142030909657478 -1.01693856716156 \
                 0.630049288272858 -2.19073308213753e-13 1.06550110802548e-13"
            )
        );
    }

    #[test]
    fn test_gpmf_floats_use_perl_stringification() {
        // GoProHERO8Black.jpg:  [APP6] DiagonalFieldOfView : 148.342163085938
        // GoProHERO12Black.jpg: [APP6] ZoomScaleNormalization : 0.713476538658142
        //                       [APP6] AspectRatioWarped     : 1.14285719394684
        let payload = gopro_payload(&[
            gpmf_record(b"ZFOV", b'f', 4, 1, &[0x43, 0x14, 0x57, 0x98]),
            gpmf_record(b"ZMPL", b'f', 4, 1, &[0x3f, 0x36, 0xa6, 0x66]),
            gpmf_record(b"ARWA", b'f', 4, 1, &[0x3f, 0x92, 0x49, 0x25]),
            gpmf_record(b"ARUW", b'f', 4, 1, &[0x3f, 0x92, 0x49, 0x25]),
            // A whole value keeps no decimal point: MappingXCoefficients : 1
            gpmf_record(b"MAPX", b'f', 4, 1, &[0x3f, 0x80, 0x00, 0x00]),
        ]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(
            metadata.get_string("APP6:DiagonalFieldOfView"),
            Some("148.342163085938")
        );
        assert_eq!(
            metadata.get_string("APP6:ZoomScaleNormalization"),
            Some("0.713476538658142")
        );
        assert_eq!(
            metadata.get_string("APP6:AspectRatioWarped"),
            Some("1.14285719394684")
        );
        assert_eq!(
            metadata.get_string("APP6:AspectRatioUnwarped"),
            Some("1.14285719394684")
        );
        assert_eq!(metadata.get_string("APP6:MappingXCoefficients"), Some("1"));
    }

    #[test]
    fn test_gpmf_creation_date_and_time_zone_convert() {
        // GoProHERO12Black.jpg: CDAT fmt='J' 0x64f6388d, TZON fmt='s' 0xff10
        //   [APP6] CreationDate : 2023:09:04 20:05:33   (ConvertUnixTime, UTC)
        //   [APP6] TimeZone     : -04:00                (-240 minutes)
        let payload = gopro_payload(&[
            gpmf_record(
                b"CDAT",
                b'J',
                8,
                1,
                &[0x00, 0x00, 0x00, 0x00, 0x64, 0xf6, 0x38, 0x8d],
            ),
            gpmf_record(b"TZON", b's', 2, 1, &[0xff, 0x10]),
        ]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(
            metadata.get_string("APP6:CreationDate"),
            Some("2023:09:04 20:05:33")
        );
        assert_eq!(metadata.get_string("APP6:TimeZone"), Some("-04:00"));
    }

    #[test]
    fn test_gpmf_time_zone_string_covers_both_signs_and_partial_hours() {
        // ExifTool's TimeZoneString over a signed count of minutes.
        assert_eq!(time_zone_string(-240), "-04:00");
        assert_eq!(time_zone_string(60), "+01:00");
        assert_eq!(time_zone_string(0), "+00:00");
        assert_eq!(time_zone_string(330), "+05:30");
        assert_eq!(time_zone_string(-570), "-09:30");
    }

    #[test]
    fn test_gpmf_epoch_zero_is_an_all_zero_date() {
        // ConvertUnixTime returns this sentinel rather than 1970.
        assert_eq!(convert_unix_time(0), "0000:00:00 00:00:00");
    }

    #[test]
    fn test_gpmf_fixed_width_string_list_splits_and_drops_padding() {
        // GoProHERO12Black.jpg: PYCF fmt='c' size=8 count=7 -- seven 8-byte
        // NUL-padded slots, which ExifTool unpacks as "(A8)7".
        //   [APP6] PolynomialPower : r0, r1, r2, r3, r4, r5, r6
        let mut pycf = Vec::new();
        for i in 0..7u8 {
            pycf.extend_from_slice(b"r");
            pycf.push(b'0' + i);
            pycf.extend_from_slice(&[0; 6]);
        }
        let payload = gopro_payload(&[gpmf_record(b"PYCF", b'c', 8, 7, &pycf)]);
        let metadata = parse_app6(&payload).unwrap();
        let Some(TagValue::Array(elements)) = metadata.get("APP6:PolynomialPower") else {
            panic!(
                "PolynomialPower should be a list, got {:?}",
                metadata.get("APP6:PolynomialPower")
            );
        };
        let elements: Vec<&str> = elements
            .iter()
            .map(|e| match e {
                TagValue::String(s) => s.as_str(),
                other => panic!("unexpected element {:?}", other),
            })
            .collect();
        assert_eq!(elements, ["r0", "r1", "r2", "r3", "r4", "r5", "r6"]);
    }

    #[test]
    fn test_gpmf_single_element_string_is_not_split() {
        // GoProHERO12Black.jpg: MXCF fmt='c' size=8 count=1 -- count is 1, so
        // the fixed-width list rule does not apply and ReadValue truncates the
        // whole payload at its first NUL.
        //   [APP6] MappingXMode : x1
        let payload = gopro_payload(&[gpmf_record(b"MXCF", b'c', 8, 1, b"x1\0\0\0\0\0\0")]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("APP6:MappingXMode"), Some("x1"));
    }

    #[test]
    fn test_gpmf_string_truncates_at_first_nul_not_at_trailing_nuls() {
        // ReadValue's 'string' rule is s/\0.*//s: everything from the FIRST
        // NUL goes, even when non-NUL bytes follow it. A trailing-trim
        // (trim_end_matches) would keep the garbage after the terminator.
        let payload = gopro_payload(&[gpmf_record(b"PHDR", b'c', 1, 8, b"OFF\0HDR\0")]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("APP6:HDRSetting"), Some("OFF"));
    }

    #[test]
    fn test_gpmf_binary_tags_report_the_converted_length() {
        // GoProHERO12Black.jpg. ExifTool declares these Binary => 1, and the
        // byte count in its placeholder is the length of the value it would
        // have printed -- not of the record, which is 16 and 12 bytes.
        //   [APP6] CameraOrientation : (Binary data 7 bytes, ...)   "1 0 0 0"
        //   [APP6] GravityVector     : (Binary data 54 bytes, ...)
        let quaternion = [
            0x3f, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let gravity = [
            0x3d, 0x13, 0xe1, 0x28, 0x3f, 0x7e, 0x59, 0xfd, 0x3d, 0xdb, 0x81, 0xb7,
        ];
        let payload = gopro_payload(&[
            gpmf_record(b"CORI", b'f', 16, 1, &quaternion),
            gpmf_record(b"IORI", b'f', 16, 1, &quaternion),
            gpmf_record(b"GRAV", b'f', 12, 1, &gravity),
        ]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(
            metadata.get_string("APP6:CameraOrientation"),
            Some("(Binary data 7 bytes, use -b option to extract)")
        );
        assert_eq!(
            metadata.get_string("APP6:ImageOrientation"),
            Some("(Binary data 7 bytes, use -b option to extract)")
        );
        assert_eq!(
            metadata.get_string("APP6:GravityVector"),
            Some("(Binary data 54 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn test_gpmf_fourcc_renders_as_text() {
        // GoProHERO12Black.jpg: PRJT fmt='F' size=4 count=1. ExifTool reads
        // 'F' as 'undef' and prints the four bytes verbatim.
        //   [APP6] LensProjection : GPRO
        let payload = gopro_payload(&[gpmf_record(b"PRJT", b'F', 4, 1, b"GPRO")]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("APP6:LensProjection"), Some("GPRO"));
    }

    #[test]
    fn test_gpmf_multi_scalar_integers_still_join_with_spaces() {
        // VERS fmt='B' size=1 count=3 -- one byte per element, so `count` and
        // the scalar count agree here and the PrintConv still applies.
        //   [APP6] MetadataVersion : 8.2.2
        let payload = gopro_payload(&[gpmf_record(b"VERS", b'B', 1, 3, &[8, 2, 2])]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("APP6:MetadataVersion"), Some("8.2.2"));
    }

    #[test]
    fn test_gpmf_media_unique_id_reads_eight_words_through_the_stride() {
        // GoProHERO12Black.jpg: MUID fmt='L' size=4 count=8 -- eight
        // big-endian uint32s, which GoPro.pm's PrintConv reformats as
        // sprintf('%.8x') each and concatenates with no separator.
        //   [APP6] MediaUniqueID : 70048c6e0dfb69c0e2ceb4f32a51cae5
        //                          317ea9c8979eea8b23ed73fe35671534
        let muid = [
            0x70, 0x04, 0x8c, 0x6e, 0x0d, 0xfb, 0x69, 0xc0, 0xe2, 0xce, 0xb4, 0xf3, 0x2a, 0x51,
            0xca, 0xe5, 0x31, 0x7e, 0xa9, 0xc8, 0x97, 0x9e, 0xea, 0x8b, 0x23, 0xed, 0x73, 0xfe,
            0x35, 0x67, 0x15, 0x34,
        ];
        let payload = gopro_payload(&[gpmf_record(b"MUID", b'L', 4, 8, &muid)]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(
            metadata.get_string("APP6:MediaUniqueID"),
            Some("70048c6e0dfb69c0e2ceb4f32a51cae5317ea9c8979eea8b23ed73fe35671534")
        );
    }

    #[test]
    fn test_gpmf_record_too_short_for_one_scalar_yields_nothing() {
        // ReadValue returns '' when the payload cannot hold a single scalar,
        // so no tag is produced rather than a value read out of bounds.
        let payload = gopro_payload(&[gpmf_record(b"ZFOV", b'f', 1, 2, &[0x43, 0x14])]);
        let metadata = parse_app6(&payload).unwrap();
        assert!(metadata.get("APP6:DiagonalFieldOfView").is_none());
    }
}
