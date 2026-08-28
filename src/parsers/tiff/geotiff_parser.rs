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

use super::geotiff_printconv::{geokey_name, geokey_print_conv, print_conv_lookup};

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

    // ProcessGeoTiff (GeoTiff.pm:2146-2147) requires the header AND every
    // declared entry before emitting anything -- a shorter blob is "Bad
    // GeoTIFF directory" and produces no tags, not even the version.
    if directory.len() < 8 {
        return result;
    }
    let num_keys = read_u16(directory, 6, is_little_endian) as usize;
    if directory.len() < 8 * (num_keys + 1) {
        return result;
    }

    let version = read_u16(directory, 0, is_little_endian);
    let key_revision = read_u16(directory, 2, is_little_endian);
    let minor_revision = read_u16(directory, 4, is_little_endian);

    // "$version.$revision.$minorRev" -- GeoTiff.pm:2161
    result.insert(
        "GeoTiff:GeoTiffVersion".to_string(),
        format!("{}.{}.{}", version, key_revision, minor_revision),
    );

    // Parse each key entry (4 u16 values each, after the 8-byte header)
    for i in 0..num_keys {
        let entry = 8 * (i + 1);
        let key_id = read_u16(directory, entry, is_little_endian);

        // `GetTagInfo($tagTable, $tag) or next` (GeoTiff.pm:2167): keys
        // absent from %Main are skipped, never reported under a made-up name.
        let Some(tag_name) = geokey_name(key_id) else {
            continue;
        };

        let tag_location = read_u16(directory, entry + 2, is_little_endian);
        let count = read_u16(directory, entry + 4, is_little_endian) as usize;
        let value_offset = read_u16(directory, entry + 6, is_little_endian) as usize;

        // %geoTiffFormat (GeoTiff.pm:25-30) maps the location to a format;
        // an unknown location or missing/short parameter data is Warn+next
        // (GeoTiff.pm:2174, :2189) -- omit, never emit a stand-in value.
        let raw = match tag_location {
            // Value stored in the offset field itself; count implied 1
            // (GeoTiff.pm:2183-2185).
            0 => Some(format!("{}", value_offset)),
            // int16u array inside the directory data itself.
            34735 => read_int16u_array(directory, value_offset, count, is_little_endian),
            // Doubles in GeoDoubleParamsTag.
            34736 => double_params
                .and_then(|d| read_double_array(d, value_offset, count, is_little_endian)),
            // Strings in GeoAsciiParamsTag.
            34737 => ascii_params.and_then(|a| read_ascii_value(a, value_offset, count)),
            _ => None,
        };
        let Some(raw) = raw else {
            continue;
        };

        result.insert(
            format!("GeoTiff:{}", tag_name),
            apply_print_conv(key_id, raw),
        );
    }

    result
}

/// Applies the tag's PrintConv exactly as ExifTool does.
///
/// GeoTiff PrintConvs are plain hashes, and Perl hash lookup is by exact
/// string key: only a value whose canonical decimal form equals the raw
/// string can match. A miss prints as `Unknown (raw)` -- ExifTool.pm:3633's
/// HASH fallback (GeoTiff declares no BITMASK/OTHER, and no GeoTiff tag has
/// PrintHex). Tags without a PrintConv print the raw value unchanged.
fn apply_print_conv(key_id: u16, raw: String) -> String {
    let Some(map) = geokey_print_conv(key_id) else {
        return raw;
    };
    if let Ok(numeric) = raw.parse::<u16>() {
        if numeric.to_string() == raw {
            if let Some(converted) = print_conv_lookup(map, numeric) {
                return converted.to_string();
            }
        }
    }
    format!("Unknown ({})", raw)
}

/// Reads `count` u16 values starting at u16-index `offset` (location 34735:
/// the values live in the GeoKeyDirectory data itself). Joined with spaces,
/// matching ReadValue's scalar-context join (ExifTool.pm:6329).
fn read_int16u_array(
    data: &[u8],
    offset: usize,
    count: usize,
    is_little_endian: bool,
) -> Option<String> {
    // `length($$dataPt) < $size*($offset+$count)` is Warn+next
    // (GeoTiff.pm:2188-2191).
    if data.len() < 2 * (offset + count) {
        return None;
    }
    // A defined count of 0 makes ReadValue return '' (ExifTool.pm:6297),
    // which FoundTag still records as an empty value.
    let values: Vec<String> = (0..count)
        .map(|i| read_u16(data, (offset + i) * 2, is_little_endian).to_string())
        .collect();
    Some(values.join(" "))
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

/// Reads `count` doubles starting at double-index `offset` in the
/// GeoDoubleParamsTag data, joined with spaces (ReadValue's scalar-context
/// join, ExifTool.pm:6329), each stringified the way Perl does.
///
/// Bounds: `length($$dataPt) < $size*($offset+$count)` is Warn+next
/// (GeoTiff.pm:2188-2191). ExifTool's own check runs against the value with
/// a 2-byte byte-order marker appended by the GeoTiffDoubleParams RawConv
/// (Exif.pm:2087); we bound against the real double data instead, so a
/// directory whose last double would overlap that marker is omitted rather
/// than decoded partly from the marker bytes.
fn read_double_array(
    data: &[u8],
    offset: usize,
    count: usize,
    is_little_endian: bool,
) -> Option<String> {
    if data.len() < 8 * (offset + count) {
        return None;
    }
    let values: Vec<String> = (0..count)
        .map(|i| format_exiftool_double(read_f64(data, (offset + i) * 8, is_little_endian)))
        .collect();
    Some(values.join(" "))
}

/// Reads a `count`-byte string at byte `offset` in the GeoAsciiParamsTag
/// data, with ExifTool's exact string pipeline:
///
/// 1. bounds check `length($$dataPt) < $size*($offset+$count)` -> Warn+next
///    (GeoTiff.pm:2188-2191);
/// 2. ReadValue truncates a 'string' at the first NUL
///    (`$vals[0] =~ s/\0.*//s`, ExifTool.pm:6311);
/// 3. ProcessGeoTiff strips ONE trailing NUL or '|' terminator
///    (`$val =~ s/(\0|\|)$//`, GeoTiff.pm:2196).
fn read_ascii_value(data: &str, offset: usize, count: usize) -> Option<String> {
    if data.len() < offset + count {
        return None;
    }
    let value = data.get(offset..offset + count)?;
    let value = match value.find('\0') {
        Some(nul) => &value[..nul],
        None => value,
    };
    Some(value.strip_suffix('|').unwrap_or(value).to_string())
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
    fn test_geokey_name_mirrors_main_table() {
        assert_eq!(geokey_name(1024), Some("GTModelType"));
        assert_eq!(geokey_name(1025), Some("GTRasterType"));
        assert_eq!(geokey_name(2048), Some("GeographicType"));
        assert_eq!(geokey_name(3072), Some("ProjectedCSType"));
        // Newly carried names -- GeoTiff.pm:554 (2062), :2055 (3096),
        // ChartTIFF extensions (:2073-2127).
        assert_eq!(geokey_name(2062), Some("GeogToWGS84"));
        assert_eq!(geokey_name(3096), Some("ProjRectifiedGridAngle"));
        assert_eq!(geokey_name(47001), Some("ChartFormat"));
        assert_eq!(geokey_name(47017), Some("ChartContourInterval"));
        // Keys absent from %Main are skipped (GeoTiff.pm:2167), never named.
        assert_eq!(geokey_name(9999), None);
    }

    #[test]
    fn test_apply_print_conv_exact_and_unknown_fallback() {
        // The two values wave 1 refused as approximations, now exact:
        // GeoTiff.pm:873 and :33.
        assert_eq!(
            apply_print_conv(3072, "26918".to_string()),
            "NAD83 UTM zone 18N"
        );
        assert_eq!(apply_print_conv(3076, "9001".to_string()), "Linear Meter");
        // The old code printed 'WGS 84 / UTM zone 17N'; GeoTiff.pm:1454 says
        // there is no space in WGS84 and no slash.
        assert_eq!(
            apply_print_conv(3072, "32617".to_string()),
            "WGS84 UTM zone 17N"
        );
        // Hash miss -> "Unknown (val)" (ExifTool.pm:3633).
        assert_eq!(
            apply_print_conv(3072, "12345".to_string()),
            "Unknown (12345)"
        );
        // Perl hash lookup is by exact string key: a joined multi-value or a
        // non-canonical number cannot match.
        assert_eq!(
            apply_print_conv(3076, "9001 9002".to_string()),
            "Unknown (9001 9002)"
        );
        assert_eq!(
            apply_print_conv(3076, "09001".to_string()),
            "Unknown (09001)"
        );
        // No PrintConv -> raw value unchanged (e.g. 3082 ProjFalseEasting).
        assert_eq!(apply_print_conv(3082, "500000".to_string()), "500000");
    }

    #[test]
    fn test_read_ascii_value_exact_string_pipeline() {
        // One trailing '|' terminator is stripped (GeoTiff.pm:2196)...
        assert_eq!(
            read_ascii_value("Hough UTM zone 17N|Other value|", 0, 19).as_deref(),
            Some("Hough UTM zone 17N")
        );
        // ...but only one: s/(\0|\|)$// is not a greedy trim.
        assert_eq!(read_ascii_value("ab||", 0, 4).as_deref(), Some("ab|"));
        // ReadValue truncates a 'string' at the first NUL (ExifTool.pm:6311).
        assert_eq!(read_ascii_value("ab\0cd", 0, 5).as_deref(), Some("ab"));
        // Short data is Warn+next (GeoTiff.pm:2188-2191), not a clamped read.
        assert_eq!(read_ascii_value("abc", 0, 4), None);
        assert_eq!(read_ascii_value("abc", 4, 1), None);
    }

    #[test]
    fn test_read_int16u_array_from_directory() {
        // Directory-resident int16u values (location 34735): count values at
        // u16-index offset, space-joined (ExifTool.pm:6329).
        let data: &[u8] = &[0x01, 0x00, 0x02, 0x00, 0x2C, 0x01, 0xFF, 0xFF];
        assert_eq!(
            read_int16u_array(data, 2, 2, true).as_deref(),
            Some("300 65535")
        );
        assert_eq!(
            read_int16u_array(data, 0, 4, true).as_deref(),
            Some("1 2 300 65535")
        );
        // Out of bounds -> omitted (GeoTiff.pm:2188-2191).
        assert_eq!(read_int16u_array(data, 3, 2, true), None);
    }

    #[test]
    fn test_read_double_array_bounds() {
        let mut data = Vec::new();
        data.extend_from_slice(&1.5f64.to_le_bytes());
        data.extend_from_slice(&(-2.0f64).to_le_bytes());
        assert_eq!(
            read_double_array(&data, 0, 2, true).as_deref(),
            Some("1.5 -2")
        );
        assert_eq!(read_double_array(&data, 1, 1, true).as_deref(), Some("-2"));
        // Out of bounds -> omitted, never zero-filled.
        assert_eq!(read_double_array(&data, 1, 2, true), None);
    }

    // One pin per transcribed PrintConv map: entry count plus hand-checked
    // entries read directly from the pinned ExifTool 13.59 GeoTiff.pm (line
    // numbers cited), independent of the generator that emitted the maps.
    mod printconv_map_pins {
        use crate::parsers::tiff::geotiff_printconv::*;

        fn get(map: &'static [(u16, &str)], key: u16) -> Option<&'static str> {
            print_conv_lookup(map, key)
        }

        #[test]
        fn gt_model_type() {
            // GeoTiff.pm:111-116
            assert_eq!(GT_MODEL_TYPE.len(), 4);
            assert_eq!(get(GT_MODEL_TYPE, 1), Some("Projected"));
            assert_eq!(get(GT_MODEL_TYPE, 3), Some("Geocentric"));
            assert_eq!(get(GT_MODEL_TYPE, 32767), Some("User Defined"));
        }

        #[test]
        fn gt_raster_type() {
            // GeoTiff.pm:119-124
            assert_eq!(GT_RASTER_TYPE.len(), 3);
            assert_eq!(get(GT_RASTER_TYPE, 1), Some("Pixel Is Area"));
            assert_eq!(get(GT_RASTER_TYPE, 2), Some("Pixel Is Point"));
        }

        #[test]
        fn epsg_units() {
            // %epsg_units, GeoTiff.pm:32-57
            assert_eq!(EPSG_UNITS.len(), 24);
            assert_eq!(get(EPSG_UNITS, 9001), Some("Linear Meter")); // :33
            assert_eq!(
                get(EPSG_UNITS, 9015),
                Some("Linear Mile International Nautical")
            ); // :47
            assert_eq!(get(EPSG_UNITS, 9102), Some("Angular Degree")); // :49
            assert_eq!(get(EPSG_UNITS, 9108), Some("Angular DMS Hemisphere")); // :55
            assert_eq!(get(EPSG_UNITS, 32767), Some("User Defined")); // :56
        }

        #[test]
        fn epsg_vertcs() {
            // %epsg_vertcs, GeoTiff.pm:59-100
            assert_eq!(EPSG_VERTCS.len(), 40);
            assert_eq!(get(EPSG_VERTCS, 0), Some("Undefined")); // :60
            assert_eq!(get(EPSG_VERTCS, 5030), Some("WGS 84 ellipsoid")); // :89
            assert_eq!(get(EPSG_VERTCS, 5106), Some("Caspian Sea")); // :98
        }

        #[test]
        fn epsg_gcs() {
            // GeographicType PrintConv, GeoTiff.pm:129-307
            assert_eq!(EPSG_GCS.len(), 176);
            assert_eq!(get(EPSG_GCS, 4001), Some("Airy 1830")); // :131
            assert_eq!(get(EPSG_GCS, 4326), Some("WGS 84")); // :290
            assert_eq!(get(EPSG_GCS, 4902), Some("NDG Paris")); // :305
        }

        #[test]
        fn epsg_datum() {
            // GeogGeodeticDatum PrintConv, GeoTiff.pm:312-471
            assert_eq!(EPSG_DATUM.len(), 157);
            assert_eq!(get(EPSG_DATUM, 6001), Some("Airy 1830")); // :314
            assert_eq!(get(EPSG_DATUM, 6902), Some("Nord de Guerre")); // :469
        }

        #[test]
        fn epsg_pm() {
            // GeogPrimeMeridian PrintConv, GeoTiff.pm:475-489
            assert_eq!(EPSG_PM.len(), 12);
            assert_eq!(get(EPSG_PM, 8901), Some("Greenwich")); // :477
            assert_eq!(get(EPSG_PM, 8911), Some("Stockholm")); // :487
        }

        #[test]
        fn epsg_ellipse() {
            // GeogEllipsoid PrintConv, GeoTiff.pm:505-543
            assert_eq!(EPSG_ELLIPSE.len(), 36);
            assert_eq!(get(EPSG_ELLIPSE, 7001), Some("Airy 1830")); // :507
            assert_eq!(get(EPSG_ELLIPSE, 32767), Some("User Defined")); // :542
        }

        #[test]
        fn epsg_pcs() {
            // ProjectedCSType PrintConv, GeoTiff.pm:557-1559
            assert_eq!(EPSG_PCS.len(), 995);
            assert_eq!(get(EPSG_PCS, 2100), Some("GGRS87 Greek Grid")); // :559
            // GeoTiff.pm assigns 2177 twice (:562 'zone 6', :563 'zone 7');
            // Perl's last-assignment-wins is what ExifTool prints.
            assert_eq!(get(EPSG_PCS, 2177), Some("ETRS89 Poland CS2000 zone 7"));
            assert_eq!(get(EPSG_PCS, 26918), Some("NAD83 UTM zone 18N")); // :873
            assert_eq!(get(EPSG_PCS, 32617), Some("WGS84 UTM zone 17N")); // :1454
            assert_eq!(get(EPSG_PCS, 32760), Some("WGS84 UTM zone 60S")); // :1557
        }

        #[test]
        fn epsg_proj() {
            // Projection PrintConv, GeoTiff.pm:1564-1994
            assert_eq!(EPSG_PROJ.len(), 428);
            assert_eq!(get(EPSG_PROJ, 10101), Some("Alabama CS27 East")); // :1566
            assert_eq!(get(EPSG_PROJ, 16001), Some("UTM zone 1N")); // :1825
            assert_eq!(get(EPSG_PROJ, 16060), Some("UTM zone 60N")); // :1884
        }

        #[test]
        fn geo_ctrans() {
            // ProjCoordTrans PrintConv, GeoTiff.pm:1998-2030
            assert_eq!(GEO_CTRANS.len(), 29);
            assert_eq!(get(GEO_CTRANS, 1), Some("Transverse Mercator"));
            // The old hand map said 'Lambert Conformal Conic'; ExifTool says:
            assert_eq!(get(GEO_CTRANS, 8), Some("Lambert Conf Conic 2SP"));
            assert_eq!(get(GEO_CTRANS, 28), Some("Cylindrical Equal Area"));
        }

        #[test]
        fn chart_format() {
            // ChartFormat PrintConv, GeoTiff.pm:2075-2088
            assert_eq!(CHART_FORMAT.len(), 11);
            assert_eq!(get(CHART_FORMAT, 47500), Some("General"));
            assert_eq!(get(CHART_FORMAT, 47510), Some("Inset"));
        }

        #[test]
        fn chart_sounding_datum() {
            // ChartSoundingDatum PrintConv, GeoTiff.pm:2097-2113
            assert_eq!(CHART_SOUNDING_DATUM.len(), 14);
            assert_eq!(
                get(CHART_SOUNDING_DATUM, 47600),
                Some("Equatorial Spring Low Water")
            );
            assert_eq!(
                get(CHART_SOUNDING_DATUM, 47613),
                Some("Tropic Lower Low Water")
            );
        }

        #[test]
        fn geokey_names_complete() {
            // %Main has 64 numeric keys (63 real GeoTIFF keys + the synthetic
            // GeoTiffVersion at key 1), GeoTiff.pm:102-2128.
            assert_eq!(GEOKEY_NAMES.len(), 64);
            // Maps must be sorted for binary_search lookup.
            for map in [
                GT_MODEL_TYPE,
                GT_RASTER_TYPE,
                EPSG_GCS,
                EPSG_DATUM,
                EPSG_PM,
                EPSG_UNITS,
                EPSG_ELLIPSE,
                EPSG_PCS,
                EPSG_PROJ,
                GEO_CTRANS,
                EPSG_VERTCS,
                CHART_FORMAT,
                CHART_SOUNDING_DATUM,
                GEOKEY_NAMES,
            ] {
                assert!(map.windows(2).all(|w| w[0].0 < w[1].0));
            }
        }
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

    /// End-to-end directory walk over every location kind and conversion
    /// path. This exact byte layout, wrapped in a minimal little-endian TIFF,
    /// was diffed against the pinned ExifTool 13.59 oracle
    /// (`scripts/compare_file.py`: compared 17 tags, MISSING 0, WRONG 0);
    /// the expectations below are the oracle's own `-G1 -s` output.
    #[test]
    fn test_parse_geotiff_keys_oracle_parity() {
        let mut words: Vec<u16> = vec![1, 1, 0, 9];
        #[rustfmt::skip]
        words.extend_from_slice(&[
            1024, 0, 1, 2,        // GTModelType -> Geographic
            2048, 0, 1, 4326,     // GeographicType -> WGS 84 (epsg_gcs)
            2049, 34737, 5, 0,    // GeogCitation -> "Test|" -> "Test"
            2054, 0, 1, 9102,     // GeogAngularUnits -> Angular Degree
            2059, 34736, 1, 0,    // GeogInvFlattening -> 298.257223563
            2062, 34735, 3, 40,   // GeogToWGS84 -> u16 x3 from the directory
            3075, 0, 1, 8,        // ProjCoordTrans -> Lambert Conf Conic 2SP
            3076, 0, 1, 12345,    // ProjLinearUnits -> Unknown (12345)
            999, 0, 1, 7,         // not in %Main -> skipped by both tools
        ]);
        words.extend_from_slice(&[11, 22, 33]); // the loc-34735 payload
        let directory: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let doubles = 298.257223563f64.to_le_bytes();

        let result = parse_geotiff_keys(&directory, Some(&doubles), Some("Test|\0"), true);

        let expect = [
            ("GeoTiff:GeoTiffVersion", "1.1.0"),
            ("GeoTiff:GTModelType", "Geographic"),
            ("GeoTiff:GeographicType", "WGS 84"),
            ("GeoTiff:GeogCitation", "Test"),
            ("GeoTiff:GeogAngularUnits", "Angular Degree"),
            ("GeoTiff:GeogInvFlattening", "298.257223563"),
            ("GeoTiff:GeogToWGS84", "11 22 33"),
            ("GeoTiff:ProjCoordTrans", "Lambert Conf Conic 2SP"),
            ("GeoTiff:ProjLinearUnits", "Unknown (12345)"),
        ];
        for (tag, value) in expect {
            assert_eq!(result.get(tag).map(String::as_str), Some(value), "{tag}");
        }
        assert_eq!(result.len(), expect.len(), "key 999 must be skipped");
    }
}
