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
use crate::core::{MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// Minimum APP6 payload length before ExifTool will read it as an InfiRay
/// MixMode record (ExifTool.pm:8162: `$$self{HasIJPEG} and $length >= 129`).
const INFIRAY_MIXMODE_MIN_LENGTH: usize = 129;

/// Parses APP6 segment data and extracts metadata.
///
/// This function dispatches to format-specific parsers based on the segment
/// identifier, using the same conditions as ExifTool's JPEG.pm APP6 table:
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
    // NITF, HP TDHD, GoPro, then the identifier-less InfiRay MixMode.

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

    // Unknown APP6 formats (EPPIM, DJI DTAT, Motorola MMIMETA, ...) extract
    // nothing, matching ExifTool's default (no -u) behavior.
    Ok(MetadataMap::new())
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

/// Applies ExifTool print conversions for the GoPro tags that define them.
fn gopro_print_conv(fourcc: &str, value: TagValue) -> TagValue {
    // Tags using %noYes = ( N => 'No', Y => 'Yes' ) in GoPro.pm
    const NO_YES_TAGS: &[&str] = &[
        "AUBT", "AUPT", "CLDP", "DZOM", "EISE", "HDRV", "ORDP", "SCAP", "SMTR",
    ];

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
            metadata.insert(
                format!("APP6:{}", tag_name),
                gopro_print_conv(fourcc, value),
            );
        }
    }
}

/// Decodes a GPMF record payload into a TagValue.
///
/// Single numeric elements become Integer/Float; multi-element numerics are
/// space-joined strings (ExifTool's ReadValue list convention); 'c' data is a
/// NUL-trimmed string; unhandled formats are kept as Binary.
fn decode_gpmf_value(format: u8, size: usize, count: usize, data: &[u8]) -> Option<TagValue> {
    if data.is_empty() {
        return None;
    }
    let reader = EndianReader::big_endian(data);

    // Integer element reader for one element at byte offset `off`
    let int_at = |off: usize| -> Option<i64> {
        match format {
            b'b' => reader.i8_at(off).map(|v| v as i64),
            b'B' => reader.u8_at(off).map(|v| v as i64),
            b's' => reader.i16_at(off).map(|v| v as i64),
            b'S' => reader.u16_at(off).map(|v| v as i64),
            b'l' => reader.i32_at(off).map(|v| v as i64),
            b'L' => reader.u32_at(off).map(|v| v as i64),
            b'j' => reader.i64_at(off),
            b'J' => reader.u64_at(off).map(|v| v as i64),
            _ => None,
        }
    };

    match format {
        b'c' | b'C' => std::str::from_utf8(data)
            .ok()
            .map(|s| TagValue::String(s.trim_end_matches('\0').to_string())),
        b'b' | b'B' | b's' | b'S' | b'l' | b'L' | b'j' | b'J' => {
            if count == 1 {
                int_at(0).map(TagValue::Integer)
            } else {
                let values: Vec<String> = (0..count)
                    .map_while(|i| int_at(i * size))
                    .map(|v| v.to_string())
                    .collect();
                (!values.is_empty()).then(|| TagValue::String(values.join(" ")))
            }
        }
        b'f' | b'd' => {
            let float_at = |off: usize| -> Option<f64> {
                if format == b'f' {
                    reader.f32_at(off).map(|v| v as f64)
                } else {
                    reader.f64_at(off)
                }
            };
            if count == 1 {
                float_at(0).map(TagValue::Float)
            } else {
                let values: Vec<String> = (0..count)
                    .map_while(|i| float_at(i * size))
                    .map(|v| v.to_string())
                    .collect();
                (!values.is_empty()).then(|| TagValue::String(values.join(" ")))
            }
        }
        // 'F' (FourCC), 'G' (UUID), 'U' (date), 'q'/'Q' (fixed-point), '?'
        // (TYPE-defined structure) and anything else: keep raw bytes.
        _ => Some(TagValue::Binary(data.to_vec())),
    }
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
}
