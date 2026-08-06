//! Tag Value Conversion Utilities
//!
//! This module provides utilities for converting raw bytes from TIFF IFD entries
//! into strongly-typed TagValue instances. It handles all EXIF field types including
//! RATIONAL, SRATIONAL, SHORT, LONG, ASCII, and UNDEFINED.
//!
//! The conversion process includes:
//! - Type-specific handlers for each EXIF field type
//! - Special formatting for GPS coordinates, DateTime, and exposure settings
//! - Heuristic conversion for unknown or ambiguous types
//! - Utility functions for reading multi-byte values in different byte orders

use crate::core::TagValue;
use crate::core::formatters::{exiftool_rational_number, perl_number, print_exposure_time};
use crate::core::operations_helpers::{
    is_datetime_string, is_printable_ascii, parse_exif_datetime, read_f32, read_f64, read_i32,
    read_u16, read_u32,
};
use crate::parsers::common::exif_types::ExifType;
use crate::parsers::tiff::ifd_parser::ByteOrder;

// ============================================================================
// PUBLIC API
// ============================================================================

/// Converts raw bytes from IFD to a TagValue.
///
/// This function interprets raw bytes according to the EXIF field type,
/// converting them to the appropriate TagValue variant.
///
/// # Arguments
///
/// * `bytes` - The raw bytes to convert
/// * `field_type` - The EXIF field type (from IFD entry)
/// * `value_count` - The number of values (from IFD entry)
/// * `tag_id` - The tag ID (for enum mapping and special handling)
/// * `byte_order` - The byte order for interpreting multi-byte values
///
/// # Returns
///
/// A TagValue representing the data
pub fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    // Exif.pm 13.59 tag 0xA20C applies `PrintSFR` to its opaque payload.
    // The header and rational matrix are both byte-order-dependent, so this
    // must happen while the TIFF reader's byte order is still available.
    // Invalid payloads deliberately fall through as binary data, just as
    // PrintSFR returns its original value when the structure does not check
    // out.
    if tag_id == 0xA20C
        && let Some(value) = format_spatial_frequency_response(bytes, byte_order)
    {
        return TagValue::new_string(value);
    }

    // Exif.pm 13.59 declares SampleFormat as a four-element PrintConv array,
    // with the same `%sampleFormat` enum table repeated for each pixel sample.
    // ExifTool splits the SHORT list, converts each of those four values, and
    // joins the printed results with `; `. Do this before the generic SHORT
    // decoder flattens a multi-value field into an undecorated string.
    if tag_id == 0x0153
        && field_type == 3
        && let Some(value) = format_sample_format(bytes, value_count, byte_order)
    {
        return TagValue::new_string(value);
    }

    // Exif.pm declares LensSerialNumber as a string. NikonZ7_2.jpg stores
    // `20147348\0 \0`; ExifTool's string reader terminates at the first NUL,
    // rather than retaining the padding after it.
    if tag_id == 0xA435 && field_type == 2 {
        let text = String::from_utf8_lossy(bytes)
            .split('\0')
            .next()
            .unwrap_or_default()
            .to_string();
        return TagValue::new_string(text);
    }

    // Exif.pm 0x9287 (`LearningOptOutIn`) is a variable-length int16u
    // sequence. The first value is a pair count; each following usage/choice
    // value alternates between the two exact PrintConv maps.
    if tag_id == 0x9287 && matches!(field_type, 3 | 4) {
        if let Some(value) = format_learning_opt_out_in(bytes, byte_order) {
            return TagValue::new_string(value);
        }
    }

    // Try special tag handlers first (GPS_VERSION_ID, EXIF_VERSION, etc.)
    if let Some(value) = handle_special_byte_tags(tag_id, bytes) {
        return value;
    }

    // Try to convert field_type to ExifType
    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            // RATIONAL (type 5): two 32-bit unsigned integers (numerator/denominator)
            ExifType::Rational if bytes.len() >= 8 => {
                return handle_rational_type(bytes, value_count, tag_id, byte_order);
            }

            // SRATIONAL (type 10): two 32-bit signed integers (numerator/denominator)
            ExifType::SRational if bytes.len() >= 8 => {
                return handle_srational_type(bytes, value_count, byte_order);
            }

            // SHORT (type 3): unsigned 16-bit integers
            ExifType::Short if bytes.len() >= 2 => {
                return handle_short_type(bytes, value_count, byte_order);
            }

            // SSHORT (type 8): signed 16-bit integers. ExifTool's
            // TimeZoneOffset (0x882a) is a variable-count int16s field.
            ExifType::SShort if bytes.len() >= 2 => {
                return handle_sshort_type(bytes, value_count, byte_order);
            }

            // LONG (type 4): unsigned 32-bit integers
            ExifType::Long if bytes.len() >= 4 => {
                return handle_long_type(bytes, value_count, byte_order);
            }

            // SLONG (type 9): signed 32-bit integers
            ExifType::SLong if bytes.len() >= 4 => {
                return handle_slong_type(bytes, value_count, byte_order);
            }

            // FLOAT (type 11): IEEE 754 single precision
            ExifType::Float if bytes.len() >= 4 => {
                return handle_float_type(bytes, value_count, byte_order);
            }

            // DOUBLE (type 12): IEEE 754 double precision
            ExifType::Double if bytes.len() >= 8 => {
                return handle_double_type(bytes, value_count, byte_order);
            }

            // ASCII (type 2): null-terminated string
            ExifType::Ascii => {
                // Exif.pm 0x8298 stores photographer and editor notices as
                // NUL-separated strings but exposes them separated by a newline.
                if tag_id == 0x8298 {
                    let mut parts = bytes.split(|byte| *byte == 0);
                    let photographer = String::from_utf8_lossy(parts.next().unwrap_or_default());
                    let editor = String::from_utf8_lossy(parts.next().unwrap_or_default());
                    let photographer = photographer.trim_end_matches(' ');
                    let editor = editor.trim_end_matches(' ');
                    return TagValue::new_string(if editor.is_empty() {
                        photographer.to_string()
                    } else {
                        format!("{photographer}\n{editor}")
                    });
                }
                let value = handle_ascii_type(bytes);
                // Exif.pm 0x010f Make declares
                // `RawConv => '$val =~ s/\s+$//; $$self{Make} = $val'`.
                if tag_id == 0x010f
                    && let TagValue::String(value) = value
                {
                    return TagValue::new_string(value.trim_end().to_string());
                }
                // Exif.pm 13.59 tag 0x0110 applies `$val =~ s/\s+$//` before
                // exposing Model. Keep the trim tag-specific: other ASCII
                // fields may treat trailing whitespace as data.
                if tag_id == 0x0110
                    && let TagValue::String(value) = value
                {
                    return TagValue::new_string(value.trim_end().to_string());
                }
                // Exif.pm 13.59 tag 0x0131 applies `$val =~ s/\s+$//` before
                // exposing Software. Keep the trim tag-specific: other ASCII
                // fields may treat trailing whitespace as data.
                if tag_id == 0x0131
                    && let TagValue::String(value) = value
                {
                    return TagValue::new_string(value.trim_end().to_string());
                }
                // Artist has its own identical RawConv in Exif.pm 13.59.
                if tag_id == 0x013b
                    && let TagValue::String(value) = value
                {
                    return TagValue::new_string(value.trim_end().to_string());
                }
                return value;
            }

            // BYTE (type 1) and UNDEFINED (type 7): binary or heuristic conversion
            ExifType::Byte | ExifType::Undefined => {
                // For UNDEFINED type, if no specific handler matched, return binary
                if field_type == 7 {
                    return TagValue::new_binary(bytes.to_vec());
                }
                // Fall through to heuristic conversion for BYTE type
            }

            _ => {
                // Fall through to heuristic conversion below
            }
        }
    }

    // Fallback heuristic conversion for unknown types or when type-specific logic doesn't apply
    heuristic_bytes_to_tag_value(bytes, byte_order)
}

/// Ports ExifTool 13.59's `Exif::PrintSFR` exactly.
///
/// The first two unsigned shorts are the number of column labels and rows.
/// They are followed by NUL-separated labels and a column-major matrix of
/// unsigned rational values at the end of the payload.
fn format_spatial_frequency_response(bytes: &[u8], byte_order: ByteOrder) -> Option<String> {
    if bytes.len() <= 4 {
        return None;
    }

    let column_count = usize::from(read_u16(bytes.get(..2)?, byte_order));
    let row_count = usize::from(read_u16(bytes.get(2..4)?, byte_order));
    let matrix_len = column_count.checked_mul(row_count)?.checked_mul(8)?;
    let matrix_offset = bytes.len().checked_sub(matrix_len)?;

    // PrintSFR requires the matrix to leave the four-byte header intact.
    if matrix_offset < 4 {
        return None;
    }

    // Perl's `split /\0/, ..., $n + 1` must find all `n` separators. The last
    // field is discarded because it is the data after the final label.
    let mut labels = bytes[4..]
        .splitn(column_count.checked_add(1)?, |byte| *byte == 0)
        .collect::<Vec<_>>();
    if labels.len() != column_count.checked_add(1)? {
        return None;
    }
    labels.pop();

    let labels = labels
        .into_iter()
        .map(|label| std::str::from_utf8(label).ok())
        .collect::<Option<Vec<_>>>()?;

    let mut columns = Vec::with_capacity(column_count);
    for (column, label) in labels.into_iter().enumerate() {
        let mut rows = Vec::with_capacity(row_count);
        for row in 0..row_count {
            let entry = column.checked_add(row.checked_mul(column_count)?)?;
            let offset = matrix_offset.checked_add(entry.checked_mul(8)?)?;
            let numerator = read_u32(bytes.get(offset..offset.checked_add(4)?)?, byte_order);
            let denominator = read_u32(
                bytes.get(offset.checked_add(4)?..offset.checked_add(8)?)?,
                byte_order,
            );
            rows.push(if denominator == 0 {
                if numerator == 0 {
                    "undef".to_string()
                } else {
                    "inf".to_string()
                }
            } else {
                exiftool_rational_number(numerator as f64 / denominator as f64)
            });
        }
        columns.push(format!("{label}={}", rows.join(",")));
    }

    Some(columns.join("; "))
}

fn format_sample_format(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> Option<String> {
    let count = usize::try_from(value_count).ok()?;
    let bytes = bytes.get(..count.checked_mul(2)?)?;

    Some(
        bytes
            .chunks_exact(2)
            .enumerate()
            .map(|(index, pair)| {
                let value = match byte_order {
                    ByteOrder::LittleEndian => u16::from_le_bytes([pair[0], pair[1]]),
                    ByteOrder::BigEndian => u16::from_be_bytes([pair[0], pair[1]]),
                };

                if index >= 4 {
                    return value.to_string();
                }

                crate::parsers::tiff::tiff_enums::tiff_enum_to_string(0x0153, i64::from(value))
                    .unwrap_or_else(|| format!("Unknown ({value})"))
            })
            .collect::<Vec<_>>()
            .join("; "),
    )
}

/// Applies ExifTool 13.59's exact `TileOffsets` ValueConv.
///
/// Exif.pm's 0x0144 entry returns a scalar reference when the already-decoded,
/// space-separated value is longer than 32 bytes. ExifTool consequently treats
/// that string as binary data: normal output shows its binary placeholder and
/// `-b` returns the ASCII offset list itself.
pub(crate) fn apply_tile_offsets_value_conv(tag_id: u16, value: TagValue) -> TagValue {
    if tag_id == 0x0144
        && let TagValue::String(text) = &value
        && text.len() > 32
    {
        return TagValue::new_binary(text.as_bytes().to_vec());
    }

    value
}

fn format_learning_opt_out_in(bytes: &[u8], byte_order: ByteOrder) -> Option<String> {
    let bytes = bytes.get(..bytes.len().checked_sub(bytes.len() % 2)?)?;
    let values: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([pair[0], pair[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([pair[0], pair[1]]),
        })
        .collect();

    let _pair_count = *values.first()?;
    Some(
        values[1..]
            .iter()
            .enumerate()
            .map(|(index, value)| match index % 2 {
                0 => match value {
                    0 => "All / Individual Usage Not Specified".to_string(),
                    1 => "Non-Generative AI/ML Training".to_string(),
                    2 => "Generative AI/ML Training".to_string(),
                    3 => "Data Mining".to_string(),
                    4 => "Input to Foundation Model (Trained AI/ML Model)".to_string(),
                    value => format!("Unknown({value})"),
                },
                _ => match value {
                    0 => "Opt-out".to_string(),
                    1 => "Opt-in".to_string(),
                    2 => "Unspecified".to_string(),
                    value => format!("Unknown({value})"),
                },
            })
            .collect::<Vec<_>>()
            .join("; "),
    )
}

/// Parses a string value to a TagValue *without discarding the source text*.
///
/// The text handed to this function is what a text-based container actually
/// stores -- an IPTC IIM dataset, an XMP property -- and for those, the stored
/// characters are what ExifTool prints. Written into a JPEG with
/// `-IPTC:EnvelopeNumber=00000042 -IPTC:ProgramVersion=01.00`,
/// `exiftool -G1 -s` reports:
///
/// ```text
/// [IPTC]          EnvelopeNumber                  : 00000042
/// [IPTC]          ProgramVersion                  : 01.00
/// ```
///
/// IPTC.pm gives neither dataset a PrintConv, so ExifTool echoes the stored
/// characters back. Parsing that text into a number throws the characters
/// away, and nothing downstream can put them back:
///
/// - `TagValue::Float` has no single rendering in this crate. The CLI prints
///   `f64::to_string` -- the shortest form that round-trips, so `01.00` comes
///   back as `1` -- and the comparison harness prints `{:.5}` with trailing
///   zeros trimmed, which also gives `1`. The digit count is simply not in the
///   value any more, so no third renderer could recover it either. This
///   function therefore never produces a `Float`.
/// - `TagValue::Integer` is lossless only when the parse is reversible.
///   `00000042` parses to 42 and prints back as `42`, and a digit string too
///   wide for `i64` missed the integer branch entirely and came back from the
///   `f64` branch as `12345678901234567000`.
///
/// So an integer literal is still parsed, but only when `i64::to_string`
/// reproduces the input byte for byte. That keeps `TagValue::Integer` -- and
/// with it the enum PrintConv lookups that match on it, `friendly_enum_name`
/// in the CLI and `decode_enum` in the comparison harness -- working for every
/// plain integer. Everything else keeps its text.
pub fn parse_string_to_tag_value(value: &str) -> TagValue {
    if let Ok(int_val) = value.parse::<i64>()
        && int_val.to_string() == value
    {
        return TagValue::Integer(int_val);
    }
    TagValue::String(value.to_string())
}

// ============================================================================
// SPECIAL TAG HANDLERS
// ============================================================================

/// Handles special byte-encoded tags that need custom formatting.
fn handle_special_byte_tags(tag_id: u16, bytes: &[u8]) -> Option<TagValue> {
    // Tag ID constants
    const GPS_VERSION_ID: u16 = 0x0000;
    const EXIF_VERSION: u16 = 0x9000;
    const COMPONENTS_CONFIGURATION: u16 = 0x9101;

    // The Windows Explorer XP* tags (0x9c9b-0x9c9f). Each is declared
    // `Format => 'undef', Writable => 'int8u'` with
    // `ValueConv => '$self->Decode($val,"UCS2","II")'`
    // (Exif.pm:2586 XPTitle, :2594 XPComment, :2602 XPAuthor,
    //  :2610 XPKeywords, :2618 XPSubject) -- the bytes are UTF-16
    // little-endian text, NUL-terminated.
    const XP_TITLE: u16 = 0x9C9B;
    const XP_SUBJECT: u16 = 0x9C9F;

    if (XP_TITLE..=XP_SUBJECT).contains(&tag_id) {
        return Some(TagValue::new_string(decode_utf16le_string(bytes)));
    }

    match tag_id {
        // GPS Version ID (4 bytes: major.minor.rev.0)
        GPS_VERSION_ID if bytes.len() >= 4 => Some(TagValue::new_string(format!(
            "{}.{}.{}.{}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        ))),

        // Exif Version (4 bytes: ASCII "0232")
        EXIF_VERSION if bytes.len() >= 4 => {
            let version = String::from_utf8_lossy(&bytes[0..4]);
            Some(TagValue::new_string(version.to_string()))
        }

        // ComponentsConfiguration (4 bytes with component IDs)
        COMPONENTS_CONFIGURATION if bytes.len() >= 4 => {
            let component_names = bytes
                .iter()
                .take(4)
                .map(|&b| match b {
                    0 => "-",
                    1 => "Y",
                    2 => "Cb",
                    3 => "Cr",
                    4 => "R",
                    5 => "G",
                    6 => "B",
                    _ => "?",
                })
                .collect::<Vec<_>>();
            Some(TagValue::new_string(component_names.join(", ")))
        }

        _ => None,
    }
}

/// Decodes a NUL-terminated UTF-16 little-endian byte string.
///
/// This is ExifTool's `Decode($val,"UCS2","II")` for the XP* tags: pairs of
/// bytes, low byte first, stopping at the first NUL code unit (which is the
/// terminator ExifTool's `ValueConvInv` writes back). An odd trailing byte is
/// dropped rather than misread as half a code unit.
///
/// An empty value decodes to the empty string, which is what
/// `exiftool -G1 -s` prints for `[IFD0] XPTitle` on
/// Nikon/NikonCoolpixS9900.jpg. Before this, the two NUL bytes fell through
/// to the generic heuristic and came back as the integer `0`.
fn decode_utf16le_string(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let unit = u16::from_le_bytes([pair[0], pair[1]]);
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}

// ============================================================================
// TYPE-SPECIFIC HANDLERS
// ============================================================================

/// Handles RATIONAL type fields (type 5).
fn handle_rational_type(
    bytes: &[u8],
    value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    // GPS coordinate tags (3 rationals: degrees, minutes, seconds)
    const GPS_LATITUDE: u16 = 0x0002;
    const GPS_LONGITUDE: u16 = 0x0004;
    const GPS_DEST_LATITUDE: u16 = 0x0014;
    const GPS_DEST_LONGITUDE: u16 = 0x0016;
    const GPS_ALTITUDE: u16 = 0x0006;
    const GPS_DOP: u16 = 0x000B;

    // GPS timestamp (3 rationals: hours, minutes, seconds)
    const GPS_TIMESTAMP: u16 = 0x0007;

    // GPS movement and tracking tags (single rational)
    const GPS_SPEED: u16 = 0x000D;
    const GPS_TRACK: u16 = 0x000F;
    const GPS_IMG_DIRECTION: u16 = 0x0011;
    const GPS_DEST_BEARING: u16 = 0x0018;
    const GPS_DEST_DISTANCE: u16 = 0x001A;
    const GPS_H_POSITIONING_ERROR: u16 = 0x001F;

    const EXPOSURE_TIME: u16 = 0x829A;
    const LENS_INFO: u16 = 0xA432; // LensInfo tag (4 rationals)

    // Check if this is an array of rationals (count > 1)
    if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
        // Special handling for GPS coordinates (3 rationals: degrees, minutes, seconds)
        if matches!(
            tag_id,
            GPS_LATITUDE | GPS_LONGITUDE | GPS_DEST_LATITUDE | GPS_DEST_LONGITUDE
        ) && value_count == 3
        {
            return format_gps_coordinate(bytes, byte_order);
        }

        // Special handling for LensInfo (4 rationals: min_focal, max_focal, min_aperture_min, min_aperture_max)
        if tag_id == LENS_INFO && value_count == 4 {
            return format_lens_info(bytes, byte_order);
        }

        // Special handling for GPSTimeStamp (3 rationals: hours, minutes, seconds)
        // ExifTool formats this as "HH:MM:SS" (e.g., "15:38:33")
        if tag_id == GPS_TIMESTAMP && value_count == 3 {
            return format_gps_timestamp(bytes, byte_order);
        }

        // Parse array of rationals and format as space-separated decimals
        return parse_rational_array(bytes, value_count, byte_order);
    }

    // Single rational value - parse numerator and denominator
    let numerator = read_u32(&bytes[0..4], byte_order);
    let denominator = read_u32(&bytes[4..8], byte_order);

    // Special handling for GPS Altitude.
    //
    // GPS.pm:124 -- `PrintConv => '$val =~ /^(inf|undef)$/ ? $val : "$val m"'`.
    // The only transform is the unit; the number itself is whatever
    // GetRational64u already rounded it to. Rounding to one decimal here
    // printed `28.0 m` for Apple_iPhone13Pro.jpg where ExifTool prints
    // `27.99831776 m`.
    if tag_id == GPS_ALTITUDE && denominator != 0 {
        let value = numerator as f64 / denominator as f64;
        return TagValue::new_string(format!("{} m", exiftool_rational_number(value)));
    }

    // Special handling for GPS movement tags
    // ExifTool displays whole numbers without decimals ("20" not "20.00")
    if denominator != 0 {
        let value = numerator as f64 / denominator as f64;

        match tag_id {
            // GPSDOP is a bare rational64u in GPS.pm, so display the
            // GetRational64u quotient rather than exposing its stored fraction.
            GPS_DOP => {
                return TagValue::new_string(exiftool_rational_number(value));
            }
            // GPSSpeed - format with precision, no unit (unit is in GPSSpeedRef)
            GPS_SPEED => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSTrack - direction in degrees (0-359.99)
            GPS_TRACK => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSImgDirection - camera pointing direction in degrees (0-359.99)
            GPS_IMG_DIRECTION => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSDestBearing - bearing to destination in degrees (0-359.99)
            GPS_DEST_BEARING => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSDestDistance - distance to destination (unit in GPSDestDistanceRef)
            GPS_DEST_DISTANCE => {
                return TagValue::new_string(format_gps_numeric_value(value));
            }
            // GPSHPositioningError - horizontal positioning error in meters
            GPS_H_POSITIONING_ERROR => {
                return TagValue::new_string(format!("{} m", format_gps_numeric_value(value)));
            }
            _ => {}
        }
    }

    // ExposureTime (Exif.pm:1824) carries
    // `PrintConv => 'Image::ExifTool::Exif::PrintExposureTime($val)'`, whose
    // breakpoint is a quarter of a second, not one second (Exif.pm:5606):
    //
    //     if ($secs < 0.25001 and $secs > 0) {
    //         return sprintf("1/%d",int(0.5 + 1/$secs));
    //     }
    //     $_ = sprintf("%.1f",$secs);
    //     s/\.0$//;
    //
    // The hand-written formatter that used to live here split at 1.0 instead,
    // so every exposure in [0.25001, 1) printed as a fraction ExifTool never
    // prints -- 5/9 s came out `1/2` where ExifTool says `0.6`, and 0.8 s came
    // out `1/1` where ExifTool says `0.8`. Both readings are plausible shutter
    // speeds, so nothing downstream could tell they were wrong. It also left
    // the trailing `.0` that `s/\.0$//` removes (`4.0` for ExifTool's `4`) and
    // divided by a zero `$secs`, printing `1/18446744073709551615`.
    //
    // `print_exposure_time` is the verified port of that subroutine; use it
    // rather than keeping a seventeenth private copy. (#340 consolidated the
    // other sixteen but did not reach this one, which is the copy that renders
    // the EXIF ExposureTime tag itself.)
    if tag_id == EXPOSURE_TIME && denominator != 0 {
        let secs = numerator as f64 / denominator as f64;
        return TagValue::new_string(print_exposure_time(secs));
    }

    // TIFF RATIONAL (type 5) is UNSIGNED, but `TagValue::Rational` stores
    // i32s, so a component above i32::MAX cannot be kept without flipping its
    // sign. OlympusOM-1.jpg writes Acceleration (Exif.pm:2554, rational64u)
    // and Pressure (Exif.pm:2544, rational64u) as 0/4294967295; stored as
    // 0/-1 those divided to negative zero and printed `-0` where
    // `exiftool -G1 -s` prints `0`. Render the quotient here instead, the same
    // way GetRational64u does (ExifTool.pm:6091-6097).
    // (A zero denominator keeps the existing Rational-with-zero path, which
    // downstream turns into "undef"; only a representable-quotient overflow is
    // intercepted here.)
    if denominator != 0 && (numerator > i32::MAX as u32 || denominator > i32::MAX as u32) {
        return TagValue::new_string(exiftool_rational_number(
            numerator as f64 / denominator as f64,
        ));
    }

    TagValue::new_rational(numerator as i32, denominator as i32)
}

/// Formats a GPS coordinate from 3 rational values.
fn format_gps_coordinate(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    let Some(degrees) = gps_coordinate_degrees(bytes, byte_order) else {
        return TagValue::new_string("");
    };

    TagValue::new_string(format_dms(degrees))
}

/// Return the full-precision ValueConv form of a three-rational GPS coordinate.
///
/// The visible tag is rounded to hundredths of an arc-second, but Composite
/// GPS tags consume decimal degrees before that PrintConv.  Keep this private
/// form beside the visible DMS string so a derived position never reparses a
/// rounded display value.
pub(crate) fn gps_coordinate_degrees(bytes: &[u8], byte_order: ByteOrder) -> Option<f64> {
    if bytes.len() < 24 {
        return None;
    }
    let mut dms = Vec::new();
    for i in 0..3 {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
        // ExifTool leaves a coordinate with an invalid rational empty. Turning
        // 0/0 into zero would manufacture a real location at the equator.
        if denominator == 0 {
            return None;
        }
        dms.push(numerator as f64 / denominator as f64);
    }
    // EXIF permits fractional degrees or minutes as well as fractional
    // seconds. ExifTool normalizes all three into DMS and its default
    // CoordFormat prints seconds to two decimals. GPS.jpg, for example, stores
    // `54 59.38 0` and therefore means 54 deg 59' 22.80", not 54 deg 59' 0".
    Some(dms[0] + dms[1] / 60.0 + dms[2] / 3600.0)
}

/// ExifTool's `ToDMS($self, $val, 1)` with the default CoordFormat.
///
/// The format string is `q{%d deg %d' %.2f"}` (GPS.pm:528) -- seconds always
/// carry exactly two decimals, which is why `exiftool -G1 -s` prints
/// `35 deg 48' 8.00"` and not `35 deg 48' 8"`.
///
/// The carry that follows it in ToDMS (GPS.pm:558-563) is reproduced too: the
/// seconds are rounded FIRST, and if rounding pushes them to 60 they roll into
/// the minutes, and a minute of 60 rolls into the degrees. Without it,
/// 72.999999 deg would print as `72 deg 59' 60.00"`.
///
/// Called without a hemisphere reference, which is the `[GPS] GPSLatitude`
/// case; ToDMS takes `$val = abs($val)` there (GPS.pm:521) and prints no sign.
/// For the `$ref`-carrying call shape, see [`format_dms_with_ref`].
pub(crate) fn format_dms(degrees_total: f64) -> String {
    let value = degrees_total.abs();
    let mut degrees = value.trunc();
    let mut minutes = ((value - degrees) * 60.0).trunc();
    let seconds = (value - degrees - minutes / 60.0) * 3600.0;

    // Round to the printed precision before testing for the carry, exactly as
    // ToDMS does with `sprintf($fmt[-1], $c[-1])`.
    let mut seconds: f64 = format!("{:.2}", seconds).parse().unwrap_or(seconds);
    if seconds >= 60.0 {
        seconds -= 60.0;
        minutes += 1.0;
        if minutes >= 60.0 {
            minutes -= 60.0;
            degrees += 1.0;
        }
    }

    format!("{} deg {}' {:.2}\"", degrees, minutes, seconds)
}

/// ExifTool's `ToDMS($self, $val, 1, $ref)` -- the same conversion as
/// [`format_dms`], but for a signed coordinate that carries its hemisphere in
/// the printed string rather than in a separate `*Ref` tag.
///
/// GPS.pm:505-515 is the whole difference: a negative value is negated, the
/// reference letter flips through `{N => 'S', E => 'W'}`, and the letter is
/// appended after a single space. `positive_ref` is the letter ExifTool passes
/// for a non-negative value (`"N"` for latitude, `"E"` for longitude), so this
/// takes the same argument the Perl call site does.
///
/// Used by FLIR's GPSInfo record, where `FLIR.pm` stores latitude and
/// longitude as signed doubles with no companion Ref tag for the value.
pub(crate) fn format_dms_with_ref(degrees_total: f64, positive_ref: char) -> String {
    let reference = if degrees_total < 0.0 {
        match positive_ref {
            'N' => 'S',
            'E' => 'W',
            other => other,
        }
    } else {
        positive_ref
    };
    format!("{} {}", format_dms(degrees_total), reference)
}

/// Formats GPSTimeStamp with GPS.pm's `ConvertTimeStamp` and `PrintTimeStamp`.
fn format_gps_timestamp(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    let mut values = [0.0; 3];
    for (i, value) in values.iter_mut().enumerate() {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
        *value = match denominator {
            0 if numerator == 0 => 0.0, // GetRational64u's `undef`, coerced by `|| 0`
            0 => f64::INFINITY,         // GetRational64u's `inf`
            _ => numerator as f64 / denominator as f64,
        };
    }

    // GPS.pm:466-477 combines the rational components before splitting them
    // back into a clock time. Its 9-place ValueConv rounding may carry into a
    // later minute; PrintTimeStamp then rounds the display to microseconds.
    let mut remaining = ((values[0] * 60.0 + values[1]) * 60.0) + values[2];
    let hours = (remaining / 3600.0).trunc();
    remaining -= hours * 3600.0;
    let minutes = (remaining / 60.0).trunc();
    remaining -= minutes * 60.0;

    let converted_seconds = format!("{remaining:012.9}");
    let converted_seconds = if converted_seconds
        .parse::<f64>()
        .is_ok_and(|seconds| seconds >= 60.0)
    {
        "00".to_string()
    } else {
        converted_seconds
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    };

    let seconds = converted_seconds.parse::<f64>().unwrap_or(f64::NAN);
    let printed_seconds = ((seconds * 1_000_000.0 + 0.5).trunc()) / 1_000_000.0;
    let printed_seconds = if printed_seconds < 10.0 {
        format!("0{printed_seconds}")
    } else {
        printed_seconds.to_string()
    };

    TagValue::new_string(format!("{hours:02.0}:{minutes:02.0}:{printed_seconds}"))
}

/// Formats LensInfo from 4 rational values (min_focal, max_focal, min_f_at_min, min_f_at_max).
///
/// LensInfo contains:
/// - [0] = Minimum focal length (mm)
/// - [1] = Maximum focal length (mm)
/// - [2] = Minimum F-number at minimum focal length
/// - [3] = Minimum F-number at maximum focal length
///
/// Formatted as: "focal_min-focal_maxmm f/aperture_min-aperture_max"
/// Example: "24-70mm f/2.8-2.8" or "50mm f/1.8" or "3.99mm f/1.8"
///
/// # Formatting Rules (ExifTool compatibility)
///
/// - Focal lengths preserve decimal precision when present (e.g., "3.99mm")
/// - Whole numbers display without decimals (e.g., "24mm" not "24.0mm")
/// - No space between number and "mm" (e.g., "3.99mm" not "3.99 mm")
fn format_lens_info(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    let mut values = Vec::new();
    for i in 0..4 {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
        if denominator != 0 {
            values.push(numerator as f64 / denominator as f64);
        } else {
            values.push(0.0);
        }
    }

    let min_focal = values[0];
    let max_focal = values[1];
    let min_f_at_min = values[2];
    let min_f_at_max = values[3];

    /// Helper to format focal length with appropriate precision.
    /// Whole numbers display without decimals, fractional values preserve precision.
    /// Uses up to 2 decimal places, trimming trailing zeros.
    fn format_focal(f: f64) -> String {
        // Format with 2 decimal places then trim trailing zeros
        let formatted = format!("{:.2}", f);
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    }

    // Format focal length range (no space before "mm")
    let focal_str = if (min_focal - max_focal).abs() < 0.01 {
        // Prime lens (single focal length)
        format!("{}mm", format_focal(min_focal))
    } else {
        // Zoom lens (focal range)
        format!("{}-{}mm", format_focal(min_focal), format_focal(max_focal))
    };

    // Format aperture range - keep one decimal for f-numbers, trim if whole
    let format_aperture = |f: f64| -> String {
        if f.fract().abs() < 0.001 {
            format!("{:.0}", f)
        } else {
            // Format with 1 decimal, trim trailing zeros
            format!("{:.1}", f)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        }
    };

    let aperture_str = if (min_f_at_min - min_f_at_max).abs() < 0.01 {
        // Constant aperture (e.g., f/2.8 or f/4)
        format!("f/{}", format_aperture(min_f_at_min))
    } else {
        // Variable aperture (e.g., f/3.5-5.6)
        format!(
            "f/{}-{}",
            format_aperture(min_f_at_min),
            format_aperture(min_f_at_max)
        )
    };

    let formatted = format!("{} {}", focal_str, aperture_str);
    TagValue::new_string(formatted)
}

/// Parses an array of rational values into a space-separated string.
fn parse_rational_array(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    let mut values = Vec::new();
    for i in 0..value_count as usize {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
        if denominator != 0 {
            values.push(numerator as f64 / denominator as f64);
        } else {
            values.push(numerator as f64);
        }
    }
    // Each element is a 64-bit rational in its own right, so each is rounded
    // the way GetRational64u/GetRational64s round one (ExifTool.pm:6090-6097).
    // A fixed ten decimal places instead printed AppleQT-200.jpg's WhitePoint
    // as `0.3127000000 0.3290000000` where `exiftool -G1 -s` prints
    // `0.3127 0.329`, and its PrimaryChromaticities likewise.
    let formatted = values
        .iter()
        .map(|v| exiftool_rational_number(*v))
        .collect::<Vec<_>>()
        .join(" ");
    TagValue::new_string(formatted)
}

/// Handles SRATIONAL type fields (type 10).
fn handle_srational_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
        return parse_srational_array(bytes, value_count, byte_order);
    }

    let numerator = read_i32(&bytes[0..4], byte_order);
    let denominator = read_i32(&bytes[4..8], byte_order);
    TagValue::new_rational(numerator, denominator)
}

/// Parses an array of signed rational values.
fn parse_srational_array(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    let mut values = Vec::new();
    for i in 0..value_count as usize {
        let offset = i * 8;
        let numerator = read_i32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_i32(&bytes[offset + 4..offset + 8], byte_order);
        if denominator != 0 {
            values.push(numerator as f64 / denominator as f64);
        } else {
            values.push(numerator as f64);
        }
    }
    // Each element is a 64-bit rational in its own right, so each is rounded
    // the way GetRational64u/GetRational64s round one (ExifTool.pm:6090-6097).
    // A fixed ten decimal places instead printed AppleQT-200.jpg's WhitePoint
    // as `0.3127000000 0.3290000000` where `exiftool -G1 -s` prints
    // `0.3127 0.329`, and its PrimaryChromaticities likewise.
    let formatted = values
        .iter()
        .map(|v| exiftool_rational_number(*v))
        .collect::<Vec<_>>()
        .join(" ");
    TagValue::new_string(formatted)
}

/// Handles SHORT type fields (type 3).
fn handle_short_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 2) {
        let mut values = Vec::new();
        for i in 0..value_count as usize {
            let offset = i * 2;
            let value = read_u16(&bytes[offset..offset + 2], byte_order);
            values.push(value.to_string());
        }
        return TagValue::new_string(values.join(" "));
    }

    let value = read_u16(&bytes[0..2], byte_order) as i64;
    TagValue::new_integer(value)
}

#[cfg(test)]
mod sample_format_tests {
    use super::*;

    /// ExifTool 13.59 Exif.pm declares SampleFormat's PrintConv as four
    /// repeated `%sampleFormat` tables. Its conversion engine splits the raw
    /// SHORT list and rejoins PrintConv results with `; `.
    #[test]
    fn sample_format_converts_each_samples_per_pixel_value() {
        let raw = [
            1u16.to_le_bytes(),
            2u16.to_le_bytes(),
            3u16.to_le_bytes(),
            6u16.to_le_bytes(),
        ]
        .concat();

        assert_eq!(
            raw_bytes_to_tag_value(&raw, 3, 4, 0x0153, ByteOrder::LittleEndian),
            TagValue::new_string("Unsigned; Signed; Float; Complex float")
        );
    }

    /// ExifTool's hash PrintConv fallback is `Unknown ($val)`, not the bare
    /// numeric value emitted when OxiDex's generic enum lookup falls through.
    #[test]
    fn sample_format_preserves_exiftool_unknown_value_wording() {
        assert_eq!(
            raw_bytes_to_tag_value(&7u16.to_be_bytes(), 3, 1, 0x0153, ByteOrder::BigEndian,),
            TagValue::new_string("Unknown (7)")
        );
    }
}

/// Handles SSHORT type fields (type 8) - signed 16-bit integers.
fn handle_sshort_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    let read_i16 = |bytes: &[u8]| match byte_order {
        ByteOrder::LittleEndian => i16::from_le_bytes([bytes[0], bytes[1]]),
        ByteOrder::BigEndian => i16::from_be_bytes([bytes[0], bytes[1]]),
    };

    if value_count > 1 && bytes.len() >= value_count as usize * 2 {
        let values = bytes[..value_count as usize * 2]
            .chunks_exact(2)
            .map(read_i16)
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        return TagValue::new_string(values.join(" "));
    }

    TagValue::new_integer(i64::from(read_i16(&bytes[..2])))
}

/// Handles LONG type fields (type 4).
fn handle_long_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 4) {
        let mut values = Vec::new();
        for i in 0..value_count as usize {
            let offset = i * 4;
            let value = read_u32(&bytes[offset..offset + 4], byte_order);
            values.push(value.to_string());
        }
        return TagValue::new_string(values.join(" "));
    }

    let value = read_u32(&bytes[0..4], byte_order) as i64;
    TagValue::new_integer(value)
}

/// Handles SLONG type fields (type 9) - signed 32-bit integers.
fn handle_slong_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 4) {
        let mut values = Vec::new();
        for i in 0..value_count as usize {
            let offset = i * 4;
            let value = read_i32(&bytes[offset..offset + 4], byte_order);
            values.push(value.to_string());
        }
        return TagValue::new_string(values.join(" "));
    }

    let value = read_i32(&bytes[0..4], byte_order) as i64;
    TagValue::new_integer(value)
}

/// Handles FLOAT type fields (type 11) - IEEE 754 single precision.
///
/// ExifTool reads these with `GetFloat` (`ExifTool.pm:6085`), a bare
/// `unpack 'f'` with none of the `RoundFloat($val, 7)` that
/// `GetRational64u`/`GetRational64s` apply (`ExifTool.pm:6098`). The unpacked
/// single widens to a Perl NV -- a double -- so the printed text is Perl's
/// default numeric stringification of the *widened* value, `%.15g`. Written
/// with `-IFD0:JXLDistance=0.1`, `exiftool -G1 -s` prints
/// `0.100000001490116`, not `0.1`: those are the 15 significant digits of the
/// nearest float to 0.1, and the extra digits are the widening made visible.
///
/// [`perl_number`] is that `%.15g`, so the value is rendered here rather than
/// carried as [`TagValue::Float`]. That variant has no single rendering in
/// this crate -- the CLI prints `f64::to_string` in one place and `{:.2}` in
/// another -- and `f64::to_string` never emits exponent form at all, so a
/// float holding 1e20 comes back as `100000002004088000000` where ExifTool
/// prints `1.00000002004088e+20`. See this module's header for the same
/// reasoning applied to text-sourced values.
fn handle_float_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 4) {
        let values: Vec<String> = (0..value_count as usize)
            .map(|i| {
                let offset = i * 4;
                perl_number(read_f32(&bytes[offset..offset + 4], byte_order) as f64)
            })
            .collect();
        return TagValue::new_string(values.join(" "));
    }

    TagValue::new_string(perl_number(read_f32(&bytes[0..4], byte_order) as f64))
}

/// Handles DOUBLE type fields (type 12) - IEEE 754 double precision.
///
/// `GetDouble` (`ExifTool.pm:6086`) is the 8-byte counterpart of `GetFloat`,
/// and is likewise unrounded. No widening happens here -- the stored double
/// *is* the Perl NV -- so `-IFD0:RawToPreviewGain=0.1` reads back as plain
/// `0.1` where the single-precision `JXLDistance` on the same file reads back
/// as `0.100000001490116`. Both go through the same `%.15g`; only the stored
/// precision differs.
fn handle_double_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
        let values: Vec<String> = (0..value_count as usize)
            .map(|i| {
                let offset = i * 8;
                perl_number(read_f64(&bytes[offset..offset + 8], byte_order))
            })
            .collect();
        return TagValue::new_string(values.join(" "));
    }

    TagValue::new_string(perl_number(read_f64(&bytes[0..8], byte_order)))
}

/// Handles ASCII type fields (type 2).
fn handle_ascii_type(bytes: &[u8]) -> TagValue {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim_end_matches('\0');
    if !s.is_empty() {
        if is_datetime_string(s)
            && let Ok(dt) = parse_exif_datetime(s)
        {
            return TagValue::DateTime(dt);
        }
        return TagValue::new_string(s.to_string());
    }
    TagValue::new_string(String::new())
}

/// Applies heuristic conversion for unknown or ambiguous byte sequences.
fn heuristic_bytes_to_tag_value(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    if bytes.len() == 2 {
        let value = read_u16(bytes, byte_order) as i64;
        return TagValue::new_integer(value);
    } else if bytes.len() == 4 {
        let null_count = bytes.iter().filter(|&&b| b == 0).count();
        let has_printable = bytes.iter().any(|&b| (32..=126).contains(&b));

        // If multiple nulls or no printable chars, treat as integer using the
        // actual byte order from the TIFF file (not hardcoded little-endian)
        if null_count > 1 || !has_printable {
            let value = read_u32(bytes, byte_order) as i64;
            return TagValue::new_integer(value);
        }

        if bytes.iter().all(|&b| (32..=126).contains(&b) || b == 0) {
            let s = String::from_utf8_lossy(bytes);
            let s = s.trim_end_matches('\0');
            if !s.is_empty() && s.len() >= 3 {
                return TagValue::new_string(s.to_string());
            }
        }

        let value = read_u32(bytes, byte_order) as i64;
        return TagValue::new_integer(value);
    }

    if is_printable_ascii(bytes) {
        let s = String::from_utf8_lossy(bytes);
        let s = s.trim_end_matches('\0');
        if !s.is_empty() {
            if is_datetime_string(s)
                && let Ok(dt) = parse_exif_datetime(s)
            {
                return TagValue::DateTime(dt);
            }
            return TagValue::new_string(s.to_string());
        }
    }

    TagValue::new_binary(bytes.to_vec())
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================
// Note: Utility functions (read_u16, read_u32, read_i32, is_datetime_string,
// parse_exif_datetime) are imported from operations_helpers module
// to avoid duplication.

/// Formats a GPS numeric value for ExifTool compatibility.
///
/// GPS values like GPSImgDirection, GPSSpeed, GPSTrack, GPSDestBearing, and
/// GPSDestDistance are formatted to match ExifTool's output:
/// - Whole numbers display without decimals: "20" not "20.00"
/// - Fractional values display with minimal precision (trailing zeros trimmed)
///
/// # Arguments
///
/// * `value` - The floating-point GPS value to format
///
/// # Returns
///
/// A string formatted to match ExifTool's GPS numeric output.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(format_gps_numeric_value(20.0), "20");
/// assert_eq!(format_gps_numeric_value(45.5), "45.5");
/// assert_eq!(format_gps_numeric_value(123.456), "123.456");
/// ```
/// GPSSpeed, GPSTrack, GPSImgDirection, GPSDestBearing and GPSDestDistance are
/// all declared in GPS.pm as bare `Writable => 'rational64u'` with no
/// PrintConv (GPS.pm:207, :220, :233, :277, :291), so ExifTool prints exactly
/// what `GetRational64u` gave it: `RoundFloat($num/$den, 10)`.
///
/// Nine decimal places is not the same rule, and the difference shows on real
/// files: Apple_iPhone13Pro.jpg's GPSDestBearing is `358.8270572` under
/// `exiftool -G1 -s` and was `358.827057183` here, its GPSSpeed
/// `0.2700000107` against `0.270000011`.
fn format_gps_numeric_value(value: f64) -> String {
    exiftool_rational_number(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::tiff::ifd_parser::ByteOrder;

    #[test]
    fn samsung_galaxy_a55_timezone_offset_matches_pinned_exiftool() {
        let path = std::path::Path::new(
            "/tmp/oxidex-exiftool-cache/combined-samples/Samsung/SamsungGalaxyA55_5G.jpg",
        );
        let metadata =
            crate::core::operations::read_metadata(path).expect("Samsung Galaxy A55 parses");

        assert_eq!(metadata.get_integer("ExifIFD:TimeZoneOffset"), Some(2));
    }

    #[test]
    fn sshort_preserves_signed_values_and_byte_order() {
        assert_eq!(
            raw_bytes_to_tag_value(&[0xF8, 0xFF], 8, 1, 0x882A, ByteOrder::LittleEndian)
                .as_integer(),
            Some(-8)
        );
        assert_eq!(
            raw_bytes_to_tag_value(&[0xFF, 0xF8], 8, 1, 0x882A, ByteOrder::BigEndian).as_integer(),
            Some(-8)
        );
    }

    /// Helper function to create RATIONAL bytes (numerator/denominator)
    fn make_rational_bytes(numerator: u32, denominator: u32, byte_order: ByteOrder) -> Vec<u8> {
        let mut bytes = Vec::new();
        match byte_order {
            ByteOrder::LittleEndian => {
                bytes.extend_from_slice(&numerator.to_le_bytes());
                bytes.extend_from_slice(&denominator.to_le_bytes());
            }
            ByteOrder::BigEndian => {
                bytes.extend_from_slice(&numerator.to_be_bytes());
                bytes.extend_from_slice(&denominator.to_be_bytes());
            }
        }
        bytes
    }

    // ========================================================================
    // parse_string_to_tag_value: the text is the value
    // ========================================================================

    #[test]
    fn test_parse_string_keeps_text_a_number_cannot_carry() {
        // Ground truth, from a JPEG written with
        //   exiftool -IPTC:EnvelopeNumber=00000042 -IPTC:ProgramVersion=01.00
        // then read back with `exiftool -G1 -s`:
        //   [IPTC]  EnvelopeNumber : 00000042
        //   [IPTC]  ProgramVersion : 01.00
        // Parsing these as i64/f64 printed "42" and "1".
        assert_eq!(
            parse_string_to_tag_value("00000042"),
            TagValue::String("00000042".to_string())
        );
        assert_eq!(
            parse_string_to_tag_value("01.00"),
            TagValue::String("01.00".to_string())
        );

        // A decimal literal is never re-rendered through f64. Leica's
        // XMP-xmpDSA properties on LeicaQ3_43.jpg read, under `exiftool -G1 -s`:
        //   FocalLength35mm     : 45.0000000000
        //   ScalingFactorHeight : 0.9642280340
        assert_eq!(
            parse_string_to_tag_value("45.0000000000"),
            TagValue::String("45.0000000000".to_string())
        );
        assert_eq!(
            parse_string_to_tag_value("0.9642280340"),
            TagValue::String("0.9642280340".to_string())
        );

        // Exponent form too -- f64 round-tripping rewrites it as a decimal
        // ExifTool never emits.
        assert_eq!(
            parse_string_to_tag_value("2.269635e+03"),
            TagValue::String("2.269635e+03".to_string())
        );

        // Wider than i64: this missed the integer branch and came back from
        // the float branch as 12345678901234567000.
        assert_eq!(
            parse_string_to_tag_value("12345678901234567890"),
            TagValue::String("12345678901234567890".to_string())
        );
    }

    #[test]
    fn model_trims_trailing_whitespace_like_exiftool() {
        // Exif.pm 13.59 tag 0x0110 applies
        // `RawConv => '$val =~ s/\s+$//; ...'` before exposing Model.
        let value =
            raw_bytes_to_tag_value(b"Camera X \t\0", 2, 11, 0x0110, ByteOrder::LittleEndian);

        assert_eq!(value.as_string(), Some("Camera X"));
    }

    #[test]
    fn artist_trims_trailing_whitespace_like_exiftool() {
        // Exif.pm 13.59 tag 0x013b applies
        // `RawConv => '$val =~ s/\s+$//; $val'` before exposing Artist.
        let value =
            raw_bytes_to_tag_value(b"Ada Lovelace \t\0", 2, 15, 0x013b, ByteOrder::LittleEndian);

        assert_eq!(value.as_string(), Some("Ada Lovelace"));
    }

    #[test]
    fn test_parse_string_still_yields_integer_when_reversible() {
        // Plain integer literals keep TagValue::Integer, which is what the
        // enum PrintConv lookups match on (friendly_enum_name in the CLI,
        // decode_enum in the comparison harness) and what
        // exiftool_compat's percentage rule reads via as_integer().
        assert_eq!(parse_string_to_tag_value("4"), TagValue::Integer(4));
        assert_eq!(parse_string_to_tag_value("0"), TagValue::Integer(0));
        assert_eq!(parse_string_to_tag_value("-5"), TagValue::Integer(-5));

        // Not reversible: i64::to_string drops the sign and the padding.
        assert_eq!(
            parse_string_to_tag_value("+5"),
            TagValue::String("+5".to_string())
        );
        assert_eq!(
            parse_string_to_tag_value("-0"),
            TagValue::String("-0".to_string())
        );

        // Non-numeric text is unchanged, as before.
        assert_eq!(
            parse_string_to_tag_value("2.5.0"),
            TagValue::String("2.5.0".to_string())
        );
        assert_eq!(
            parse_string_to_tag_value("Phil Harvey"),
            TagValue::String("Phil Harvey".to_string())
        );
    }

    #[test]
    fn gps_coordinate_normalizes_fractional_minutes_like_exiftool() {
        // GPS.jpg stores 54 degrees, 59.38 minutes, zero seconds. Fractional
        // minutes must be carried into seconds by the default DMS rendering.
        let bytes =
            make_rational_array_bytes(&[(54, 1), (5938, 100), (0, 1)], ByteOrder::LittleEndian);
        assert_eq!(
            raw_bytes_to_tag_value(&bytes, 5, 3, 0x0002, ByteOrder::LittleEndian).as_string(),
            Some("54 deg 59' 22.80\"")
        );
    }

    #[test]
    fn copyright_decodes_photographer_and_editor_separator() {
        let value = raw_bytes_to_tag_value(
            b"Photographer\0Editor\0",
            2,
            20,
            0x8298,
            ByteOrder::LittleEndian,
        );

        assert_eq!(value.as_string(), Some("Photographer\nEditor"));
    }

    #[test]
    fn make_discards_trailing_whitespace_like_exiftool_rawconv() {
        let value =
            raw_bytes_to_tag_value(b"RICOH      \0", 2, 12, 0x010F, ByteOrder::LittleEndian);

        assert_eq!(value.as_string(), Some("RICOH"));
    }

    #[test]
    fn gps_coordinate_refuses_invalid_rationals() {
        let bytes = make_rational_array_bytes(&[(0, 0), (0, 0), (0, 0)], ByteOrder::LittleEndian);
        assert_eq!(
            raw_bytes_to_tag_value(&bytes, 5, 3, 0x0002, ByteOrder::LittleEndian).as_string(),
            Some("")
        );
    }

    #[test]
    fn test_gps_speed_formatting() {
        // Test GPSSpeed (tag 0x000D) - whole numbers without decimals (ExifTool compatible)
        let bytes = make_rational_bytes(25, 1, ByteOrder::BigEndian); // 25
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000D, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "25"); // Not "25.00"
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test with fractional value
        let bytes = make_rational_bytes(1234, 100, ByteOrder::BigEndian); // 12.34
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000D, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "12.34");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_dop_formats_rational_like_exiftool() {
        let bytes = make_rational_bytes(260, 100, ByteOrder::BigEndian);

        let value = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000B, ByteOrder::BigEndian);

        assert_eq!(value.as_string(), Some("2.6"));
    }

    #[test]
    fn test_gps_speed_ref_formatting() {
        // Test GPSSpeedRef (tag 0x000C) - should be ASCII string (K, M, or N)
        let bytes = b"K\0"; // km/h
        let result = raw_bytes_to_tag_value(bytes, 2, 2, 0x000C, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "K");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_track_formatting() {
        // Test GPSTrack (tag 0x000F) - direction in degrees (0-359.99)
        let bytes = make_rational_bytes(27512, 100, ByteOrder::BigEndian); // 275.12 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000F, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "275.12");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test with integer degrees - whole numbers without decimals
        let bytes = make_rational_bytes(90, 1, ByteOrder::BigEndian); // 90 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000F, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "90"); // Not "90.00"
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_track_ref_formatting() {
        // Test GPSTrackRef (tag 0x000E) - should be ASCII string (T or M)
        let bytes = b"T\0"; // True north
        let result = raw_bytes_to_tag_value(bytes, 2, 2, 0x000E, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "T");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_img_direction_formatting() {
        // Test GPSImgDirection (tag 0x0011) - camera pointing direction
        let bytes = make_rational_bytes(18050, 100, ByteOrder::LittleEndian); // 180.50 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x0011, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "180.5"); // Trailing zero trimmed
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test whole number - no decimals
        let bytes = make_rational_bytes(20, 1, ByteOrder::LittleEndian); // 20 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x0011, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "20"); // Not "20.00"
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_img_direction_ref_formatting() {
        // Test GPSImgDirectionRef (tag 0x0010) - should be ASCII string (T or M)
        let bytes = b"M\0"; // Magnetic north
        let result = raw_bytes_to_tag_value(bytes, 2, 2, 0x0010, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "M");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_dest_bearing_formatting() {
        // Test GPSDestBearing (tag 0x0018) - bearing to destination
        let bytes = make_rational_bytes(4525, 100, ByteOrder::BigEndian); // 45.25 degrees
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x0018, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "45.25");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_dest_distance_formatting() {
        // Test GPSDestDistance (tag 0x001A) - distance to destination
        let bytes = make_rational_bytes(12345, 1000, ByteOrder::LittleEndian); // 12.345
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x001A, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "12.345");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_h_positioning_error_formatting() {
        // Test GPSHPositioningError (tag 0x001F) - horizontal positioning error in meters
        let bytes = make_rational_bytes(525, 100, ByteOrder::BigEndian); // 5.25 m
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x001F, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "5.25 m");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test with integer value - whole numbers without decimals
        let bytes = make_rational_bytes(10, 1, ByteOrder::BigEndian); // 10 m
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x001F, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "10 m"); // Not "10.00 m"
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_movement_tags_with_zero_denominator() {
        // Test that zero denominators don't cause division by zero
        let bytes = make_rational_bytes(100, 0, ByteOrder::BigEndian);

        // Should fall through to rational representation
        let result = raw_bytes_to_tag_value(&bytes, 5, 1, 0x000D, ByteOrder::BigEndian);

        if let TagValue::Rational {
            numerator,
            denominator,
        } = result
        {
            assert_eq!(numerator, 100);
            assert_eq!(denominator, 0);
        } else {
            panic!(
                "Expected Rational variant for zero denominator, got {:?}",
                result
            );
        }
    }

    #[test]
    fn test_gps_movement_tags_forensic_scenario() {
        // Forensic scenario: Vehicle moving at 55.5 km/h, heading 275.5 degrees

        // GPSSpeed: 55.5
        let speed_bytes = make_rational_bytes(555, 10, ByteOrder::BigEndian);
        let speed = raw_bytes_to_tag_value(&speed_bytes, 5, 1, 0x000D, ByteOrder::BigEndian);
        if let TagValue::String(s) = speed {
            assert_eq!(s, "55.5"); // Trailing zero trimmed
        } else {
            panic!("Expected String for GPSSpeed");
        }

        // GPSSpeedRef: K (km/h)
        let speed_ref = raw_bytes_to_tag_value(b"K\0", 2, 2, 0x000C, ByteOrder::BigEndian);
        if let TagValue::String(s) = speed_ref {
            assert_eq!(s, "K");
        } else {
            panic!("Expected String for GPSSpeedRef");
        }

        // GPSTrack: 275.5 degrees
        let track_bytes = make_rational_bytes(2755, 10, ByteOrder::BigEndian);
        let track = raw_bytes_to_tag_value(&track_bytes, 5, 1, 0x000F, ByteOrder::BigEndian);
        if let TagValue::String(s) = track {
            assert_eq!(s, "275.5"); // Trailing zero trimmed
        } else {
            panic!("Expected String for GPSTrack");
        }

        // GPSTrackRef: T (true north)
        let track_ref = raw_bytes_to_tag_value(b"T\0", 2, 2, 0x000E, ByteOrder::BigEndian);
        if let TagValue::String(s) = track_ref {
            assert_eq!(s, "T");
        } else {
            panic!("Expected String for GPSTrackRef");
        }

        // GPSImgDirection: 90.25 degrees (camera pointing east)
        let img_dir_bytes = make_rational_bytes(9025, 100, ByteOrder::BigEndian);
        let img_dir = raw_bytes_to_tag_value(&img_dir_bytes, 5, 1, 0x0011, ByteOrder::BigEndian);
        if let TagValue::String(s) = img_dir {
            assert_eq!(s, "90.25");
        } else {
            panic!("Expected String for GPSImgDirection");
        }

        // GPSHPositioningError: 8.5 m
        let error_bytes = make_rational_bytes(85, 10, ByteOrder::BigEndian);
        let error = raw_bytes_to_tag_value(&error_bytes, 5, 1, 0x001F, ByteOrder::BigEndian);
        if let TagValue::String(s) = error {
            assert_eq!(s, "8.5 m"); // Trailing zero trimmed
        } else {
            panic!("Expected String for GPSHPositioningError");
        }
    }

    // ============================================================================
    // GPS TIMESTAMP TESTS
    // ============================================================================

    /// Helper function to create rational bytes array (for LensInfo, GPSTimeStamp, etc.)
    fn make_rational_array_bytes(values: &[(u32, u32)], byte_order: ByteOrder) -> Vec<u8> {
        let mut bytes = Vec::new();
        for &(num, den) in values {
            match byte_order {
                ByteOrder::LittleEndian => {
                    bytes.extend_from_slice(&num.to_le_bytes());
                    bytes.extend_from_slice(&den.to_le_bytes());
                }
                ByteOrder::BigEndian => {
                    bytes.extend_from_slice(&num.to_be_bytes());
                    bytes.extend_from_slice(&den.to_be_bytes());
                }
            }
        }
        bytes
    }

    #[test]
    fn test_gps_timestamp_basic() {
        // Test GPSTimeStamp (tag 0x0007) - should format as "HH:MM:SS"
        // Input: 15 hours, 38 minutes, 33 seconds
        let bytes = make_rational_array_bytes(&[(15, 1), (38, 1), (33, 1)], ByteOrder::BigEndian);

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "15:38:33");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_timestamp_zero_padded() {
        // Test zero-padding for hours/minutes/seconds
        // Input: 8 hours, 5 minutes, 3 seconds -> "08:05:03"
        let bytes = make_rational_array_bytes(&[(8, 1), (5, 1), (3, 1)], ByteOrder::LittleEndian);

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "08:05:03");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_timestamp_fractional_seconds() {
        // Test fractional seconds (e.g., 33.5 seconds)
        // Input: 15 hours, 38 minutes, 33.5 seconds
        let bytes = make_rational_array_bytes(
            &[(15, 1), (38, 1), (67, 2)], // 67/2 = 33.5
            ByteOrder::BigEndian,
        );

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "15:38:33.5");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn gps_timestamp_normalizes_fractional_components_and_rounds_to_microseconds() {
        // GPS.pm's ConvertTimeStamp first combines all three rational components
        // into elapsed seconds, then PrintTimeStamp rounds a fractional result
        // to the nearest microsecond for display.
        let bytes = make_rational_array_bytes(
            &[(31, 2), (153, 4), (3_312_345_679, 100_000_000)],
            ByteOrder::BigEndian,
        );

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::BigEndian);

        assert_eq!(result.as_string(), Some("16:08:48.123457"));
    }

    #[test]
    fn test_gps_timestamp_midnight() {
        // Test midnight (00:00:00)
        let bytes = make_rational_array_bytes(&[(0, 1), (0, 1), (0, 1)], ByteOrder::LittleEndian);

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::LittleEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "00:00:00");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_gps_timestamp_end_of_day() {
        // Test 23:59:59
        let bytes = make_rational_array_bytes(&[(23, 1), (59, 1), (59, 1)], ByteOrder::BigEndian);

        let result = raw_bytes_to_tag_value(&bytes, 5, 3, 0x0007, ByteOrder::BigEndian);

        if let TagValue::String(s) = result {
            assert_eq!(s, "23:59:59");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    // ============================================================================
    // DEVICE IDENTIFICATION TESTS - For Forensic Device Attribution
    // ============================================================================

    #[test]
    fn test_lens_info_prime_lens() {
        // 50mm f/1.8 prime lens - common forensic scenario
        let bytes = make_rational_array_bytes(
            &[(50, 1), (50, 1), (18, 10), (18, 10)],
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm
            assert_eq!(s, "50mm f/1.8");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_zoom_constant_aperture() {
        // 24-70mm f/2.8 zoom lens with constant aperture - professional camera
        let bytes = make_rational_array_bytes(
            &[(24, 1), (70, 1), (28, 10), (28, 10)],
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm, no trailing .0
            assert_eq!(s, "24-70mm f/2.8");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_zoom_variable_aperture() {
        // 18-55mm f/3.5-5.6 zoom lens - common kit lens
        let bytes = make_rational_array_bytes(
            &[(18, 1), (55, 1), (35, 10), (56, 10)],
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm
            assert_eq!(s, "18-55mm f/3.5-5.6");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_big_endian() {
        // 85mm f/1.4 prime lens with big-endian byte order
        let bytes = make_rational_array_bytes(
            &[(85, 1), (85, 1), (14, 10), (14, 10)],
            ByteOrder::BigEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::BigEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm
            assert_eq!(s, "85mm f/1.4");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_telephoto() {
        // 70-200mm f/4 telephoto zoom lens
        let bytes = make_rational_array_bytes(
            &[(70, 1), (200, 1), (40, 10), (40, 10)],
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool format: no space before mm, no trailing .0
            assert_eq!(s, "70-200mm f/4");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_info_fractional_focal_rounding() {
        // Test focal length rounding: 3.99mm should round to 4mm
        // This matches ExifTool's behavior for smartphone lenses
        let bytes = make_rational_array_bytes(
            &[(399, 100), (399, 100), (18, 10), (18, 10)], // 3.99mm f/1.8
            ByteOrder::LittleEndian,
        );

        let result = raw_bytes_to_tag_value(
            &bytes,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            // ExifTool keeps exact value (3.99), no rounding. Format: no space before mm
            assert_eq!(s, "3.99mm f/1.8");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }

        // Test a truly fractional value that shouldn't round
        let bytes2 = make_rational_array_bytes(
            &[(45, 10), (45, 10), (28, 10), (28, 10)], // 4.5mm f/2.8
            ByteOrder::LittleEndian,
        );

        let result2 = raw_bytes_to_tag_value(
            &bytes2,
            5, // ExifType::Rational
            4,
            0xA432, // LensInfo
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result2 {
            // 4.5 stays as 4.5 (not rounded), ExifTool format: no space before mm
            assert_eq!(s, "4.5mm f/2.8");
        } else {
            panic!("Expected String variant, got {:?}", result2);
        }
    }

    #[test]
    fn test_image_unique_id() {
        // ImageUniqueID is a 32-character hex string for unique image identification
        let unique_id = b"0123456789ABCDEF0123456789ABCDEF\0";

        let result = raw_bytes_to_tag_value(
            unique_id,
            2, // ExifType::Ascii
            33,
            0xA420, // ImageUniqueID
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "0123456789ABCDEF0123456789ABCDEF");
            assert_eq!(s.len(), 32, "ImageUniqueID should be 32 characters");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_owner_name() {
        // OwnerName - camera owner for attribution
        let owner_name = b"John Doe\0";

        let result = raw_bytes_to_tag_value(
            owner_name,
            2, // ExifType::Ascii
            9,
            0xA430, // OwnerName
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "John Doe");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_body_serial_number() {
        // BodySerialNumber (tag 0xA431) - camera body serial for forensic attribution
        let serial = b"1234567890\0";

        let result = raw_bytes_to_tag_value(
            serial,
            2, // ExifType::Ascii
            11,
            0xA431, // SerialNumber (BodySerialNumber)
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "1234567890");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_make() {
        // LensMake - lens manufacturer
        let lens_make = b"Canon\0";

        let result = raw_bytes_to_tag_value(
            lens_make,
            2, // ExifType::Ascii
            6,
            0xA433, // LensMake
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "Canon");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_model() {
        // LensModel - specific lens model name
        let lens_model = b"EF 50mm f/1.8 STM\0";

        let result = raw_bytes_to_tag_value(
            lens_model,
            2, // ExifType::Ascii
            18,
            0xA434, // LensModel
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "EF 50mm f/1.8 STM");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_lens_serial_number() {
        // LensSerialNumber - lens serial for unique identification
        let lens_serial = b"ABC123456\0";

        let result = raw_bytes_to_tag_value(
            lens_serial,
            2, // ExifType::Ascii
            10,
            0xA435, // LensSerialNumber
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "ABC123456");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_camera_serial_number_dng() {
        // CameraSerialNumber (DNG/Adobe tag 0xC62F)
        let camera_serial = b"DNG9876543210\0";

        let result = raw_bytes_to_tag_value(
            camera_serial,
            2, // ExifType::Ascii
            14,
            0xC62F, // CameraSerialNumber (DNG)
            ByteOrder::LittleEndian,
        );

        if let TagValue::String(s) = result {
            assert_eq!(s, "DNG9876543210");
        } else {
            panic!("Expected String variant, got {:?}", result);
        }
    }

    #[test]
    fn test_forensic_device_attribution_scenario() {
        // Complete forensic scenario: Camera + Lens identification

        // Camera body serial number
        let body_serial = b"CN123456789\0";
        let body_result =
            raw_bytes_to_tag_value(body_serial, 2, 12, 0xA431, ByteOrder::LittleEndian);
        assert!(matches!(body_result, TagValue::String(ref s) if s == "CN123456789"));

        // Lens serial number
        let lens_serial = b"LS987654321\0";
        let lens_result =
            raw_bytes_to_tag_value(lens_serial, 2, 12, 0xA435, ByteOrder::LittleEndian);
        assert!(matches!(lens_result, TagValue::String(ref s) if s == "LS987654321"));

        // Lens info: 24-70mm f/2.8 (ExifTool format: no space before mm)
        let lens_info_bytes = make_rational_array_bytes(
            &[(24, 1), (70, 1), (28, 10), (28, 10)],
            ByteOrder::LittleEndian,
        );
        let lens_info_result =
            raw_bytes_to_tag_value(&lens_info_bytes, 5, 4, 0xA432, ByteOrder::LittleEndian);
        assert!(matches!(lens_info_result, TagValue::String(ref s) if s == "24-70mm f/2.8"));

        // Owner name
        let owner = b"Evidence Photographer\0";
        let owner_result = raw_bytes_to_tag_value(owner, 2, 22, 0xA430, ByteOrder::LittleEndian);
        assert!(matches!(owner_result, TagValue::String(ref s) if s == "Evidence Photographer"));

        // Image unique ID
        let unique_id = b"ABCDEF0123456789FEDCBA9876543210\0";
        let id_result = raw_bytes_to_tag_value(unique_id, 2, 33, 0xA420, ByteOrder::LittleEndian);
        assert!(
            matches!(id_result, TagValue::String(ref s) if s == "ABCDEF0123456789FEDCBA9876543210")
        );
    }

    // ------------------------------------------------------------------------
    // FLOAT (type 11) and DOUBLE (type 12)
    // ------------------------------------------------------------------------

    /// `0xCD49` JXLDistance, `Writable => 'float'` (Exif.pm).
    const JXL_DISTANCE: u16 = 0xCD49;
    /// `0xC7A8` RawToPreviewGain, `Writable => 'double'` (Exif.pm).
    const RAW_TO_PREVIEW_GAIN: u16 = 0xC7A8;

    fn float_bytes(v: f32, byte_order: ByteOrder) -> Vec<u8> {
        match byte_order {
            ByteOrder::LittleEndian => v.to_le_bytes().to_vec(),
            ByteOrder::BigEndian => v.to_be_bytes().to_vec(),
        }
    }

    fn double_bytes(v: f64, byte_order: ByteOrder) -> Vec<u8> {
        match byte_order {
            ByteOrder::LittleEndian => v.to_le_bytes().to_vec(),
            ByteOrder::BigEndian => v.to_be_bytes().to_vec(),
        }
    }

    fn as_str(v: TagValue) -> String {
        match v {
            TagValue::String(s) => s,
            other => panic!("expected a string rendering, got {other:?}"),
        }
    }

    /// Before FLOAT was decoded, these 4 bytes missed every arm of
    /// `raw_bytes_to_tag_value` and reached `heuristic_bytes_to_tag_value`,
    /// whose `bytes.len() == 4` branch read them as a u32: 1.5f32 is
    /// 0x3FC00000, so `oxidex -j -e` printed 1069547520 under a real ExifTool
    /// tag name while `exiftool -G1 -s` printed 1.5.
    #[test]
    fn test_float_is_not_read_as_raw_ieee754_bits() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let bytes = float_bytes(1.5, order);
            let value = raw_bytes_to_tag_value(&bytes, 11, 1, JXL_DISTANCE, order);
            assert_eq!(as_str(value), "1.5");
            // The exact integer the heuristic used to produce.
            assert_ne!(
                as_str(raw_bytes_to_tag_value(&bytes, 11, 1, JXL_DISTANCE, order)),
                "1069547520"
            );
        }
    }

    /// The DOUBLE counterpart: 8 bytes matched neither the 2- nor the 4-byte
    /// heuristic branch and were not printable ASCII, so they fell through to
    /// `TagValue::Binary` and printed as
    /// "(Binary data 8 bytes, use -b option to extract)".
    #[test]
    fn test_double_is_not_read_as_binary_blob() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let bytes = double_bytes(1.5, order);
            let value = raw_bytes_to_tag_value(&bytes, 12, 1, RAW_TO_PREVIEW_GAIN, order);
            assert_eq!(as_str(value), "1.5");
        }
    }

    /// Both values verified against ExifTool 13.59 (the `.exiftool-version`
    /// pin) on a JPEG written with `-IFD0:JXLDistance=0.1
    /// -IFD0:RawToPreviewGain=0.1`:
    ///
    /// ```text
    /// [IFD0]  JXLDistance      : 0.100000001490116
    /// [IFD0]  RawToPreviewGain : 0.1
    /// ```
    ///
    /// The single widens to a double before Perl stringifies it, so the float
    /// keeps 15 significant digits of 0.1's nearest float while the double
    /// prints plain `0.1`. A shared `%.15g` produces both.
    #[test]
    fn test_float_and_double_match_exiftool_precision() {
        let order = ByteOrder::LittleEndian;

        let float_val =
            raw_bytes_to_tag_value(&float_bytes(0.1, order), 11, 1, JXL_DISTANCE, order);
        assert_eq!(as_str(float_val), "0.100000001490116");

        let double_val =
            raw_bytes_to_tag_value(&double_bytes(0.1, order), 12, 1, RAW_TO_PREVIEW_GAIN, order);
        assert_eq!(as_str(double_val), "0.1");
    }

    /// `%g` switches to exponent form outside `-4 <= exp < 15`, and Perl signs
    /// and 2-pads the exponent. `f64::to_string` never emits exponent form at
    /// all -- it would render this as `100000002004088000000` -- which is why
    /// these are rendered rather than carried as [`TagValue::Float`].
    #[test]
    fn test_float_and_double_use_exponent_form_like_perl() {
        let order = ByteOrder::LittleEndian;

        let big = raw_bytes_to_tag_value(&float_bytes(1e20, order), 11, 1, JXL_DISTANCE, order);
        assert_eq!(as_str(big), "1.00000002004088e+20");

        let small = raw_bytes_to_tag_value(&float_bytes(2.5e-8, order), 11, 1, JXL_DISTANCE, order);
        assert_eq!(as_str(small), "2.50000002921524e-08");

        let big_d = raw_bytes_to_tag_value(
            &double_bytes(1e20, order),
            12,
            1,
            RAW_TO_PREVIEW_GAIN,
            order,
        );
        assert_eq!(as_str(big_d), "1e+20");
    }

    /// Multi-value FLOAT/DOUBLE join with a space, as the SHORT/LONG/SLONG
    /// handlers already do for their arrays.
    #[test]
    fn test_float_and_double_arrays_join_with_spaces() {
        let order = ByteOrder::LittleEndian;

        let mut floats = float_bytes(1.5, order);
        floats.extend(float_bytes(2.25, order));
        floats.extend(float_bytes(-0.5, order));
        let value = raw_bytes_to_tag_value(&floats, 11, 3, JXL_DISTANCE, order);
        assert_eq!(as_str(value), "1.5 2.25 -0.5");

        let mut doubles = double_bytes(1.5, order);
        doubles.extend(double_bytes(2.25, order));
        let value = raw_bytes_to_tag_value(&doubles, 12, 2, RAW_TO_PREVIEW_GAIN, order);
        assert_eq!(as_str(value), "1.5 2.25");
    }

    /// A truncated entry must not panic on the fixed-width slice. Too-short
    /// buffers miss the guarded arms and fall through to the heuristic, which
    /// is what every other numeric type does with a short buffer.
    #[test]
    fn test_truncated_float_and_double_do_not_panic() {
        let order = ByteOrder::LittleEndian;
        for len in 0..4 {
            let _ = raw_bytes_to_tag_value(&vec![0u8; len], 11, 1, JXL_DISTANCE, order);
        }
        for len in 0..8 {
            let _ = raw_bytes_to_tag_value(&vec![0u8; len], 12, 1, RAW_TO_PREVIEW_GAIN, order);
        }
    }

    /// Exif.pm 13.59 `PrintSFR` uses a column-major unsigned-rational matrix
    /// after the NUL-separated labels. Check both TIFF byte orders because the
    /// PrintConv reads its shorts and rationals through ExifTool's active
    /// byte-order accessors.
    #[test]
    fn spatial_frequency_response_matches_pinned_exiftool_printsfr() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let mut bytes = Vec::new();
            match order {
                ByteOrder::LittleEndian => {
                    bytes.extend_from_slice(&2u16.to_le_bytes());
                    bytes.extend_from_slice(&2u16.to_le_bytes());
                }
                ByteOrder::BigEndian => {
                    bytes.extend_from_slice(&2u16.to_be_bytes());
                    bytes.extend_from_slice(&2u16.to_be_bytes());
                }
            }
            bytes.extend_from_slice(b"Red\0Blue\0");

            // PrintSFR indexes as i + j*n: Red = 1.5,3 and Blue = 2.25,4.
            for (numerator, denominator) in [(3u32, 2u32), (9, 4), (3, 1), (4, 1)] {
                match order {
                    ByteOrder::LittleEndian => {
                        bytes.extend_from_slice(&numerator.to_le_bytes());
                        bytes.extend_from_slice(&denominator.to_le_bytes());
                    }
                    ByteOrder::BigEndian => {
                        bytes.extend_from_slice(&numerator.to_be_bytes());
                        bytes.extend_from_slice(&denominator.to_be_bytes());
                    }
                }
            }

            assert_eq!(
                raw_bytes_to_tag_value(&bytes, 7, bytes.len() as u32, 0xA20C, order).as_string(),
                Some("Red=1.5,3; Blue=2.25,4"),
            );
        }
    }

    #[test]
    fn malformed_spatial_frequency_response_stays_binary() {
        let bytes = [1, 0, 1, 0, b'X']; // Missing the label terminator and matrix.
        assert_eq!(
            raw_bytes_to_tag_value(
                &bytes,
                7,
                bytes.len() as u32,
                0xA20C,
                ByteOrder::LittleEndian
            ),
            TagValue::Binary(bytes.to_vec())
        );
    }
}
