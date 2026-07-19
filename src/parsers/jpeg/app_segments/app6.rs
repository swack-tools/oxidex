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
//! if let Some(model) = metadata.get_string("GoPro:Model") {
//!     println!("Camera model: {}", model);
//! }
//! ```

use crate::core::{MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// Parses APP6 segment data and extracts metadata.
///
/// This function dispatches to format-specific parsers based on the segment
/// identifier, using the same conditions as ExifTool's JPEG.pm APP6 table:
/// - GoPro GPMF data - starts with "GoPro\0"
/// - TDHD data (HP/Toshiba) - starts with "TDHD\x01\0\0\0"
/// - NITF data - starts with "NTIF\0"
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
/// assert!(metadata.contains_key("GoPro:Model"));
/// ```
pub fn parse_app6(data: &[u8]) -> Result<MetadataMap> {
    // Minimum APP6 segment should have at least a few bytes
    if data.len() < 4 {
        return Err(ExifToolError::parse_error(
            "APP6 segment too short to contain valid metadata",
        ));
    }

    // Dispatch on the same identifier conditions ExifTool uses (JPEG.pm APP6):
    // GoPro: /^GoPro\0/, HP TDHD: /^TDHD\x01\0\0\0/, NITF: /^NTIF\0/.

    if data.starts_with(b"GoPro\0") {
        return parse_gpmf(&data[6..]);
    }

    if data.starts_with(b"TDHD\x01\0\0\0") {
        return parse_tdhd(data);
    }

    if data.starts_with(b"NTIF\0") {
        return parse_nitf(data);
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
        "MUID" => "MediaUID",
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
    parse_gpmf_records(data, &mut metadata);
    Ok(metadata)
}

/// Walks GPMF TLV records, inserting known tags into the metadata map.
///
/// Mirrors ExifTool's ProcessGoPro: stops at the null tag ("\0\0\0\0") or at
/// a FourCC containing characters outside [-_a-zA-Z0-9 ]; skips FourCCs
/// without a known tag name; recurses into container records (format 0).
fn parse_gpmf_records(data: &[u8], metadata: &mut MetadataMap) {
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

        // Containers (format 0, e.g. DEVC/STRM) nest further GPMF records
        if format == 0 {
            parse_gpmf_records(value_data, metadata);
            continue;
        }

        // Unknown FourCCs are extracted by ExifTool only with -u; skip them.
        let Some(tag_name) = gopro_tag_name(fourcc) else {
            continue;
        };
        if let Some(value) = decode_gpmf_value(format, size, count, value_data) {
            metadata.insert(
                format!("GoPro:{}", tag_name),
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

/// Parses NITF (National Imagery Transmission Format) metadata.
///
/// NITF is used for geospatial imagery metadata in defense/intelligence applications.
/// The format includes image classification, geolocation, and sensor information.
///
/// # Arguments
///
/// * `data` - Raw NITF data (starts with "NITF" identifier)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Extracted NITF metadata
/// * `Err(ExifToolError)` - If parsing fails
fn parse_nitf(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Caller has verified the "NTIF\0" identifier (5 bytes).
    // Detailed field parsing (ExifTool JPEG.pm %JPEG::NITF) is not yet
    // ported; expose the raw payload for now.
    metadata.insert(
        "APP6:NITFData".to_string(),
        TagValue::Binary(data[5..].to_vec()),
    );

    Ok(metadata)
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
        assert_eq!(metadata.get_string("GoPro:Model"), Some("HERO8 Black"));
        assert_eq!(
            metadata.get_string("GoPro:CameraSerialNumber"),
            Some("C3221324545448")
        );
        assert_eq!(
            metadata.get_string("GoPro:FirmwareVersion"),
            Some("HD8.01.01.60.00")
        );
        assert_eq!(metadata.get_string("GoPro:Rate"), Some("4_1SEC"));
    }

    #[test]
    fn test_parse_app6_gopro_print_conversions() {
        let payload = gopro_payload(&[
            gpmf_record(b"OREN", b'c', 1, 1, b"U"),
            gpmf_record(b"PRTN", b'c', 1, 1, b"N"),
            gpmf_record(b"VERS", b'B', 1, 3, &[7, 6, 5]),
        ]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("GoPro:AutoRotation"), Some("Up"));
        assert_eq!(metadata.get_string("GoPro:Protune"), Some("Off"));
        assert_eq!(metadata.get_string("GoPro:MetadataVersion"), Some("7.6.5"));
    }

    #[test]
    fn test_parse_app6_gopro_numeric_values() {
        let payload = gopro_payload(&[
            gpmf_record(b"PIMX", b'L', 4, 1, &1600u32.to_be_bytes()),
            gpmf_record(b"PIMN", b'L', 4, 1, &100u32.to_be_bytes()),
        ]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_integer("GoPro:AutoISOMax"), Some(1600));
        assert_eq!(metadata.get_integer("GoPro:AutoISOMin"), Some(100));
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
        assert!(metadata.get("GoPro:XXXX").is_none());
        assert_eq!(metadata.get_string("GoPro:Rate"), Some("4_1SEC"));
    }

    #[test]
    fn test_parse_app6_gopro_container_recursion() {
        // DEVC (format 0) nests further GPMF records
        let inner = gpmf_record(b"DVNM", b'c', 1, 11, b"HERO8 Black");
        let payload = gopro_payload(&[gpmf_record(b"DEVC", 0, 1, inner.len() as u16, &inner)]);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("GoPro:DeviceName"), Some("HERO8 Black"));
    }

    #[test]
    fn test_parse_app6_gopro_stops_at_null_tag() {
        let mut records = vec![gpmf_record(b"RATE", b'c', 1, 6, b"4_1SEC")];
        records.push(gpmf_record(&[0, 0, 0, 0], 0, 0, 0, &[]));
        records.push(gpmf_record(b"CASN", b'c', 1, 4, b"1234"));
        let payload = gopro_payload(&records);
        let metadata = parse_app6(&payload).unwrap();
        assert_eq!(metadata.get_string("GoPro:Rate"), Some("4_1SEC"));
        // Records after the null terminator are not parsed (ExifTool behavior)
        assert!(metadata.get("GoPro:CameraSerialNumber").is_none());
    }

    #[test]
    fn test_parse_app6_nitf_requires_ntif_identifier() {
        // Real NITF APP6 segments start with "NTIF\0" (ExifTool JPEG.pm)
        let mut ntif = b"NTIF\0".to_vec();
        ntif.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        let metadata = parse_app6(&ntif).unwrap();
        assert!(metadata.contains_key("APP6:NITFData"));

        // The misspelled "NITF" prefix must NOT match
        let mut nitf = b"NITF\0".to_vec();
        nitf.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        let metadata = parse_app6(&nitf).unwrap();
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_app6_tdhd_requires_version_bytes() {
        // ExifTool's gate is "TDHD\x01\0\0\0"
        let mut tdhd = b"TDHD\x01\0\0\0".to_vec();
        tdhd.extend_from_slice(&[0xAA, 0xBB]);
        let metadata = parse_app6(&tdhd).unwrap();
        assert!(metadata.contains_key("APP6:TDHDData"));

        // Bare "TDHD" without the version bytes must NOT match
        let bare = b"TDHDxxxx".to_vec();
        let metadata = parse_app6(&bare).unwrap();
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
