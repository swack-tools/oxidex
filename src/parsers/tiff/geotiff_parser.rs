//! GeoTiff key parsing
//!
//! Parses GeoTiff key directory (tag 0x87AF) and associated double/string parameters.
//! GeoTiff stores geographic metadata using a key-value system embedded in TIFF tags.
//!
//! # GeoTiff Tag Structure
//!
//! - **Tag 34735 (0x87AF)** - GeoKeyDirectoryTag: Array of u16 values
//!   - Header: [KeyDirectoryVersion, KeyRevision, MinorRevision, NumberOfKeys]
//!   - Each key: [KeyID, TIFFTagLocation, Count, Value/Offset]
//!     - TIFFTagLocation = 0: Value is stored directly in the Value field
//!     - TIFFTagLocation = 34736: Value is an offset into GeoDoubleParamsTag
//!     - TIFFTagLocation = 34737: Value is an offset into GeoAsciiParamsTag
//!
//! - **Tag 34736 (0x87B0)** - GeoDoubleParamsTag: Array of f64 values
//! - **Tag 34737 (0x87B1)** - GeoAsciiParamsTag: String values separated by '|'

#![allow(dead_code)]

use std::collections::HashMap;

/// GeoTiff tag ID for the GeoKeyDirectoryTag (34735)
pub const GEOTIFF_DIRECTORY_TAG: u16 = 0x87AF;
/// GeoTiff tag ID for the GeoDoubleParamsTag (34736) - stores double precision values
pub const GEOTIFF_DOUBLE_PARAMS_TAG: u16 = 0x87B0;
/// GeoTiff tag ID for the GeoAsciiParamsTag (34737) - stores ASCII string values
pub const GEOTIFF_ASCII_PARAMS_TAG: u16 = 0x87B1;
/// TIFF tag ID for ModelTransformation (34264) - stores 4x4 transformation matrix
pub const MODEL_TRANSFORMATION_TAG: u16 = 0x85D8;

/// Parses GeoTiff keys from the directory tag and parameter tags.
///
/// # Parameters
/// - `directory`: Raw bytes of GeoKeyDirectoryTag (0x87AF) as u16 values
/// - `double_params`: Optional raw bytes of GeoDoubleParamsTag (0x87B0)
/// - `ascii_params`: Optional GeoAsciiParamsTag string (0x87B1)
/// - `is_little_endian`: Byte order
///
/// # Returns
/// HashMap of tag name to value string
pub fn parse_geotiff_keys(
    directory: &[u8],
    double_params: Option<&[u8]>,
    ascii_params: Option<&str>,
    is_little_endian: bool,
) -> HashMap<String, String> {
    let mut result = HashMap::new();

    // Need at least 8 bytes for header (4 u16 values)
    if directory.len() < 8 {
        return result;
    }

    // Parse header
    let version = read_u16(directory, 0, is_little_endian);
    let key_revision = read_u16(directory, 2, is_little_endian);
    let minor_revision = read_u16(directory, 4, is_little_endian);
    let num_keys = read_u16(directory, 6, is_little_endian) as usize;

    // Add version tag
    result.insert(
        "GeoTiff:GeoTiffVersion".to_string(),
        format!("{}.{}.{}", version, key_revision, minor_revision),
    );

    // Each key entry is 4 u16 values (8 bytes)
    // After header (8 bytes), we have num_keys entries
    let entries_start = 8;
    let required_len = entries_start + num_keys * 8;

    if directory.len() < required_len {
        return result;
    }

    // Parse each key entry
    for i in 0..num_keys {
        let offset = entries_start + i * 8;
        let key_id = read_u16(directory, offset, is_little_endian);
        let tag_location = read_u16(directory, offset + 2, is_little_endian);
        let count = read_u16(directory, offset + 4, is_little_endian) as usize;
        let value_offset = read_u16(directory, offset + 6, is_little_endian);

        let tag_name = geokey_to_name(key_id);
        let value = match tag_location {
            0 => {
                // Value is stored directly in the value_offset field
                format_geokey_value(key_id, value_offset as u32)
            }
            34736 => {
                // Value is in GeoDoubleParamsTag
                if let Some(doubles) = double_params {
                    extract_double_value(doubles, value_offset as usize, count, is_little_endian)
                } else {
                    format!("{}", value_offset)
                }
            }
            34737 => {
                // Value is in GeoAsciiParamsTag
                if let Some(ascii) = ascii_params {
                    extract_ascii_value(ascii, value_offset as usize, count)
                } else {
                    format!("{}", value_offset)
                }
            }
            _ => format!("{}", value_offset),
        };

        result.insert(format!("GeoTiff:{}", tag_name), value);
    }

    result
}

/// Parses the ModelTransformation tag (4x4 matrix)
pub fn parse_model_transformation(data: &[u8], is_little_endian: bool) -> Option<String> {
    // ModelTransformation is an array of 16 f64 values (128 bytes)
    if data.len() < 128 {
        return None;
    }

    let mut values = Vec::with_capacity(16);
    for i in 0..16 {
        let offset = i * 8;
        let value = read_f64(data, offset, is_little_endian);
        values.push(value);
    }

    Some(
        values
            .iter()
            .map(|v| format_exiftool_double(*v))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Formats an f64 the way ExifTool (Perl) stringifies floating point values.
///
/// Perl's default numeric stringification uses roughly `%.15g` semantics
/// (15 significant digits, fixed vs. scientific notation chosen by
/// magnitude, trailing zeros stripped). Rust's default `Display` for `f64`
/// instead prints the shortest string that round-trips exactly, which can
/// require up to 17 significant digits and therefore diverges from
/// ExifTool's output (e.g. `33.41791964296692` vs. `33.4179196429669`).
/// This function reproduces the `%.15g` behavior so numeric TIFF/GeoTiff
/// values match ExifTool byte-for-byte.
fn format_exiftool_double(value: f64) -> String {
    const SIG: usize = 15;

    if value == 0.0 {
        return "0".to_string();
    }
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }

    let neg = value.is_sign_negative();
    let av = value.abs();

    // Render in scientific notation with SIG significant digits; Rust's
    // formatter performs correct rounding (including mantissa carry, e.g.
    // 9.9999999999999996 -> 1.00000000000000e1) for us.
    let sci = format!("{:.*e}", SIG - 1, av);
    let mut parts = sci.splitn(2, 'e');
    let mantissa_str = parts.next().unwrap();
    let exp: i32 = parts.next().unwrap().parse().unwrap_or(0);

    // All significant digits, without the decimal point.
    let digits: String = mantissa_str.chars().filter(|c| *c != '.').collect();

    let use_sci = exp < -4 || exp >= SIG as i32;
    let result = if use_sci {
        let mut m = String::new();
        m.push(digits.chars().next().unwrap_or('0'));
        let rest: String = digits.chars().skip(1).collect();
        let rest_trimmed = rest.trim_end_matches('0');
        if !rest_trimmed.is_empty() {
            m.push('.');
            m.push_str(rest_trimmed);
        }
        let exp_sign = if exp >= 0 { "+" } else { "-" };
        format!("{}e{}{:02}", m, exp_sign, exp.abs())
    } else {
        let point_pos = exp + 1;
        let mut s = String::new();
        if point_pos <= 0 {
            s.push_str("0.");
            for _ in 0..(-point_pos) {
                s.push('0');
            }
            s.push_str(&digits);
        } else {
            let point_pos = point_pos as usize;
            if point_pos >= digits.len() {
                s.push_str(&digits);
                for _ in 0..(point_pos - digits.len()) {
                    s.push('0');
                }
            } else {
                s.push_str(&digits[..point_pos]);
                s.push('.');
                s.push_str(&digits[point_pos..]);
            }
        }
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    };

    if neg { format!("-{}", result) } else { result }
}

/// Reads a u16 from bytes with the specified byte order
fn read_u16(data: &[u8], offset: usize, is_little_endian: bool) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    if is_little_endian {
        u16::from_le_bytes([data[offset], data[offset + 1]])
    } else {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    }
}

/// Reads an f64 from bytes with the specified byte order
fn read_f64(data: &[u8], offset: usize, is_little_endian: bool) -> f64 {
    if offset + 8 > data.len() {
        return 0.0;
    }
    let bytes: [u8; 8] = data[offset..offset + 8].try_into().unwrap_or([0; 8]);
    if is_little_endian {
        f64::from_le_bytes(bytes)
    } else {
        f64::from_be_bytes(bytes)
    }
}

/// Extracts a double value from GeoDoubleParamsTag
fn extract_double_value(
    data: &[u8],
    offset: usize,
    count: usize,
    is_little_endian: bool,
) -> String {
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let byte_offset = (offset + i) * 8;
        let value = read_f64(data, byte_offset, is_little_endian);
        values.push(format_exiftool_double(value));
    }
    values.join(" ")
}

/// Extracts an ASCII value from GeoAsciiParamsTag
fn extract_ascii_value(data: &str, offset: usize, count: usize) -> String {
    if offset >= data.len() {
        return String::new();
    }
    let end = (offset + count).min(data.len());
    let value = &data[offset..end];
    // Remove trailing '|' if present (GeoTiff uses | as separator)
    value.trim_end_matches('|').to_string()
}

/// Maps GeoTiff key IDs to human-readable names
fn geokey_to_name(key_id: u16) -> &'static str {
    match key_id {
        // GeoTiff Configuration Keys
        1024 => "GTModelType",
        1025 => "GTRasterType",
        1026 => "GTCitation",
        // Geographic CS Parameter Keys
        2048 => "GeographicType",
        2049 => "GeogCitation",
        2050 => "GeogGeodeticDatum",
        2051 => "GeogPrimeMeridian",
        2052 => "GeogLinearUnits",
        2053 => "GeogLinearUnitSize",
        2054 => "GeogAngularUnits",
        2055 => "GeogAngularUnitSize",
        2056 => "GeogEllipsoid",
        2057 => "GeogSemiMajorAxis",
        2058 => "GeogSemiMinorAxis",
        2059 => "GeogInvFlattening",
        2060 => "GeogAzimuthUnits",
        2061 => "GeogPrimeMeridianLong",
        // Projected CS Parameter Keys
        3072 => "ProjectedCSType",
        3073 => "PCSCitation",
        3074 => "Projection",
        3075 => "ProjCoordTrans",
        3076 => "ProjLinearUnits",
        3077 => "ProjLinearUnitSize",
        3078 => "ProjStdParallel1",
        3079 => "ProjStdParallel2",
        3080 => "ProjNatOriginLong",
        3081 => "ProjNatOriginLat",
        3082 => "ProjFalseEasting",
        3083 => "ProjFalseNorthing",
        3084 => "ProjFalseOriginLong",
        3085 => "ProjFalseOriginLat",
        3086 => "ProjFalseOriginEasting",
        3087 => "ProjFalseOriginNorthing",
        3088 => "ProjCenterLong",
        3089 => "ProjCenterLat",
        3090 => "ProjCenterEasting",
        3091 => "ProjCenterNorthing",
        3092 => "ProjScaleAtNatOrigin",
        3093 => "ProjScaleAtCenter",
        3094 => "ProjAzimuthAngle",
        3095 => "ProjStraightVertPoleLong",
        // Vertical CS Keys
        4096 => "VerticalCSType",
        4097 => "VerticalCitation",
        4098 => "VerticalDatum",
        4099 => "VerticalUnits",
        _ => "Unknown",
    }
}

/// Formats a direct GeoTiff key value (when TIFFTagLocation = 0)
fn format_geokey_value(key_id: u16, value: u32) -> String {
    match key_id {
        // GTModelType
        1024 => match value {
            1 => "Projected".to_string(),
            2 => "Geographic".to_string(),
            3 => "Geocentric".to_string(),
            32767 => "User Defined".to_string(),
            _ => format!("{}", value),
        },
        // GTRasterType
        1025 => match value {
            1 => "Pixel Is Area".to_string(),
            2 => "Pixel Is Point".to_string(),
            32767 => "User Defined".to_string(),
            _ => format!("{}", value),
        },
        // GeographicType (EPSG codes)
        2048 => match value {
            4326 => "WGS 84".to_string(),
            4269 => "NAD83".to_string(),
            4267 => "NAD27".to_string(),
            32767 => "User Defined".to_string(),
            _ => format!("{}", value),
        },
        // GeogGeodeticDatum
        2050 => match value {
            6326 => "WGS 84".to_string(),
            6269 => "NAD83".to_string(),
            6267 => "NAD27".to_string(),
            32767 => "User Defined".to_string(),
            _ => format!("{}", value),
        },
        // ProjectedCSType
        3072 => {
            // UTM zone detection
            if value >= 32601 && value <= 32660 {
                format!("WGS 84 / UTM zone {}N", value - 32600)
            } else if value >= 32701 && value <= 32760 {
                format!("WGS 84 / UTM zone {}S", value - 32700)
            } else if value == 32767 {
                "User Defined".to_string()
            } else {
                format!("{}", value)
            }
        }
        // Projection
        3074 => {
            // UTM projection mapping
            if value >= 16001 && value <= 16060 {
                format!("UTM zone {}N", value - 16000)
            } else if value >= 16101 && value <= 16160 {
                format!("UTM zone {}S", value - 16100)
            } else if value == 32767 {
                "User Defined".to_string()
            } else {
                format!("{}", value)
            }
        }
        // ProjCoordTrans
        3075 => match value {
            1 => "Transverse Mercator".to_string(),
            7 => "Mercator".to_string(),
            8 => "Lambert Conformal Conic".to_string(),
            11 => "Albers Equal Area".to_string(),
            32767 => "User Defined".to_string(),
            _ => format!("{}", value),
        },
        // LinearUnits
        3076 | 2052 => match value {
            9001 => "m".to_string(),
            9002 => "ft".to_string(),
            9003 => "us ft".to_string(),
            32767 => "User Defined".to_string(),
            _ => format!("{}", value),
        },
        // AngularUnits
        2054 => match value {
            9101 => "rad".to_string(),
            9102 => "deg".to_string(),
            9103 => "arc min".to_string(),
            9104 => "arc sec".to_string(),
            9105 => "grad".to_string(),
            32767 => "User Defined".to_string(),
            _ => format!("{}", value),
        },
        _ => format!("{}", value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_u16() {
        let data = [0x34, 0x12, 0xCD, 0xAB];
        assert_eq!(read_u16(&data, 0, true), 0x1234);
        assert_eq!(read_u16(&data, 0, false), 0x3412);
        assert_eq!(read_u16(&data, 2, true), 0xABCD);
        assert_eq!(read_u16(&data, 2, false), 0xCDAB);
    }

    #[test]
    fn test_geokey_to_name() {
        assert_eq!(geokey_to_name(1024), "GTModelType");
        assert_eq!(geokey_to_name(1025), "GTRasterType");
        assert_eq!(geokey_to_name(2048), "GeographicType");
        assert_eq!(geokey_to_name(3072), "ProjectedCSType");
        assert_eq!(geokey_to_name(9999), "Unknown");
    }

    #[test]
    fn test_format_geokey_value() {
        assert_eq!(format_geokey_value(1024, 1), "Projected");
        assert_eq!(format_geokey_value(1024, 2), "Geographic");
        assert_eq!(format_geokey_value(1025, 1), "Pixel Is Area");
        assert_eq!(format_geokey_value(3072, 32617), "WGS 84 / UTM zone 17N");
    }

    #[test]
    fn test_extract_ascii_value() {
        let ascii = "Hough UTM zone 17N|Other value|";
        assert_eq!(extract_ascii_value(ascii, 0, 18), "Hough UTM zone 17N");
    }

    #[test]
    fn test_format_exiftool_double_matches_perl_15_sig_figs() {
        // Values pulled directly from a real ExifTool ModelTransform output;
        // ExifTool (Perl) stringifies doubles with ~15 significant digits,
        // while Rust's default `{}` uses the shortest round-trip
        // representation (often 16-17 digits). Verify we match ExifTool.
        assert_eq!(
            format_exiftool_double(33.417919642966924),
            "33.4179196429669"
        );
        assert_eq!(
            format_exiftool_double(35.836331379428414),
            "35.8363313794284"
        );
        assert_eq!(
            format_exiftool_double(691955.1656840311),
            "691955.165684031"
        );
        assert_eq!(
            format_exiftool_double(2791710.9901260315),
            "2791710.99012603"
        );
        assert_eq!(
            format_exiftool_double(-33.417919642966924),
            "-33.4179196429669"
        );
        assert_eq!(format_exiftool_double(0.0), "0");
        assert_eq!(format_exiftool_double(1.0), "1");
    }

    #[test]
    fn test_parse_model_transformation_matches_exiftool() {
        let values: [f64; 16] = [
            33.417919642966924,
            35.836331379428414,
            0.0,
            691955.1656840311,
            35.836331379428414,
            -33.417919642966924,
            0.0,
            2791710.9901260315,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        let mut data = Vec::with_capacity(128);
        for v in values {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let result = parse_model_transformation(&data, true).expect("should parse");
        assert_eq!(
            result,
            "33.4179196429669 35.8363313794284 0 691955.165684031 35.8363313794284 -33.4179196429669 0 2791710.99012603 0 0 0 0 0 0 0 1"
        );
    }

    #[test]
    fn test_parse_geotiff_keys_simple() {
        // Create a simple GeoKeyDirectory with version 1.1.0 and 2 keys
        // Header: version=1, revision=1, minor=0, numKeys=2
        // Key 1: GTModelType (1024), TIFFTagLocation=0, Count=1, Value=1 (Projected)
        // Key 2: GTRasterType (1025), TIFFTagLocation=0, Count=1, Value=1 (PixelIsArea)
        let directory: Vec<u8> = vec![
            0x01, 0x00, // Version: 1
            0x01, 0x00, // Key Revision: 1
            0x00, 0x00, // Minor Revision: 0
            0x02, 0x00, // Number of keys: 2
            // Key 1: GTModelType
            0x00, 0x04, // KeyID: 1024
            0x00, 0x00, // TIFFTagLocation: 0
            0x01, 0x00, // Count: 1
            0x01, 0x00, // Value: 1 (Projected)
            // Key 2: GTRasterType
            0x01, 0x04, // KeyID: 1025
            0x00, 0x00, // TIFFTagLocation: 0
            0x01, 0x00, // Count: 1
            0x01, 0x00, // Value: 1 (Pixel Is Area)
        ];

        let result = parse_geotiff_keys(&directory, None, None, true);

        assert_eq!(
            result.get("GeoTiff:GeoTiffVersion"),
            Some(&"1.1.0".to_string())
        );
        assert_eq!(
            result.get("GeoTiff:GTModelType"),
            Some(&"Projected".to_string())
        );
        assert_eq!(
            result.get("GeoTiff:GTRasterType"),
            Some(&"Pixel Is Area".to_string())
        );
    }
}
