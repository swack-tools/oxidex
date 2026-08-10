//! ExifTool Compatibility Formatting Module
//!
//! This module provides a single entry point for transforming raw parsed metadata values
//! into ExifTool-compatible formatted strings. ExifTool is the de facto standard for
//! metadata extraction, and maintaining output compatibility ensures users can rely on
//! consistent behavior when migrating from or comparing against ExifTool.
//!
//! # Architecture
//!
//! The formatting pipeline consists of:
//!
//! 1. **`format_for_exiftool()`** - Main entry point that iterates over all metadata
//!    and applies tag-specific formatting rules.
//!
//! 2. **`format_tag_value()`** - Dispatch function that examines tag names and routes
//!    values to the appropriate formatter based on a priority-ordered rule set.
//!
//! 3. **Helper functions** - Tag name classification utilities that determine which
//!    formatting rules apply to each tag.
//!
//! # Formatting Rules (in priority order)
//!
//! 1. GPS latitude/longitude references (N/S/E/W -> North/South/East/West)
//! 2. GPS direction references (T/M -> True North/Magnetic North)
//! 3. GPS speed/distance references (K/M/N -> km/h/mph/knots)
//! 4. GPS status tags (GPSStatus, GPSMeasureMode, GPSDifferential)
//! 5. GPS altitude reference (binary 0x00/0x01 -> Above/Below Sea Level)
//! 6. GPS processing method (binary data with encoding prefix)
//! 7. Binary decoders (CFAPattern, SceneType, version bytes)
//! 8. APP14 flags (APP14Flags0/APP14Flags1: 0 -> "(none)")
//! 9. Enum tags (ExposureProgram integer -> string description)
//! 10. ICC_Profile matrix tags (5 decimal precision, MeasurementFlare with % suffix)
//! 11. Integer precision tags (ReferenceBlackWhite: whole numbers)
//! 12. Three decimal precision tags (YCbCrCoefficients)
//! 13. UserComment (binary data with encoding prefix)
//! 14. ThumbnailImage (binary -> "(Binary data X bytes, use -b option to extract)")
//! 15. Percentage tags (Quality, MeasurementFlare: append %)
//! 16. Unit suffixes (FocalLength -> "X mm", GPSAltitude -> "X m")
//! 17. Special values (infinity -> "undef", -0 -> "0")
//! 18. Default: return original value unchanged
//!
//! # Example
//!
//! ```rust,ignore
//! use oxidex::core::{MetadataMap, TagValue};
//! use oxidex::core::exiftool_compat::format_for_exiftool;
//!
//! let mut metadata = MetadataMap::new();
//! metadata.insert("EXIF:GPSLatitudeRef", TagValue::String("N".to_string()));
//! metadata.insert("EXIF:FocalLength", TagValue::String("50".to_string()));
//!
//! let formatted = format_for_exiftool(&metadata);
//!
//! assert_eq!(formatted.get_string("EXIF:GPSLatitudeRef"), Some("North"));
//! // 0x920a forces one decimal: Exif.pm:2401 `sprintf("%.1f mm",$val)`
//! assert_eq!(formatted.get_string("EXIF:FocalLength"), Some("50.0 mm"));
//! ```

use crate::core::binary_decoders::decode_user_comment;
use crate::core::formatters::exif_print_conv::{
    print_exposure_time, print_f_number, print_fraction,
};
use crate::core::formatters::gps_speed_ref::format_gps_dest_distance_ref;
use crate::core::formatters::gps_status::{
    format_gps_differential, format_gps_measure_mode, format_gps_status,
};
use crate::core::formatters::{
    decode_cfa_pattern, decode_gps_processing_method, decode_scene_type, decode_version_bytes,
    exiftool_rational_number, file_source_label_bytes, format_color_space,
    format_components_configuration, format_compression, format_contrast, format_custom_rendered,
    format_exposure_mode, format_exposure_program, format_file_source, format_flash,
    format_focal_plane_resolution_unit, format_gain_control, format_gps_altitude_ref,
    format_gps_direction_ref, format_gps_lat_ref, format_gps_lon_ref, format_gps_speed_ref,
    format_gray_response_unit, format_icc_value, format_integer_precision_values,
    format_interop_index, format_light_source, format_metering_mode, format_orientation,
    format_resolution_unit, format_saturation, format_scene_capture_type,
    format_security_classification, format_sensing_method, format_sharpness,
    format_subject_distance_range, format_three_decimal_values, format_white_balance,
    format_with_unit, format_ycbcr_positioning, format_ycbcr_subsampling_string, is_icc_matrix_tag,
    is_integer_precision_tag, is_three_decimal_tag,
};
use crate::core::{MetadataMap, TagValue};

// =============================================================================
// MAIN PUBLIC API
// =============================================================================

/// Transforms all values in a MetadataMap to ExifTool-compatible formatted strings.
///
/// This is the main entry point for ExifTool compatibility formatting. It iterates
/// over every tag in the input metadata and applies the appropriate formatting rule
/// based on the tag name. Values that don't match any formatting rule are passed
/// through unchanged.
///
/// # Arguments
///
/// * `metadata` - Reference to the source MetadataMap containing raw parsed values
///
/// # Returns
///
/// A new MetadataMap with all values formatted for ExifTool compatibility.
/// Tag names are preserved exactly as they appear in the input.
///
/// # Performance
///
/// This function creates a new MetadataMap rather than modifying in place to
/// maintain immutability semantics. For large metadata sets, the overhead is
/// minimal compared to the formatting operations themselves.
///
/// # Example
///
/// ```rust,ignore
/// use oxidex::core::{MetadataMap, TagValue};
/// use oxidex::core::exiftool_compat::format_for_exiftool;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:ExposureProgram", TagValue::Integer(2));
/// metadata.insert("GPS:GPSLatitudeRef", TagValue::String("N".to_string()));
///
/// let formatted = format_for_exiftool(&metadata);
///
/// // ExposureProgram 2 -> "Program AE"
/// assert_eq!(formatted.get_string("EXIF:ExposureProgram"), Some("Program AE"));
/// // GPSLatitudeRef "N" -> "North"
/// assert_eq!(formatted.get_string("GPS:GPSLatitudeRef"), Some("North"));
/// ```
pub fn format_for_exiftool(metadata: &MetadataMap) -> MetadataMap {
    let mut result = MetadataMap::with_capacity(metadata.len());

    for (tag_name, value) in metadata.iter() {
        let formatted_value = format_tag_value(tag_name, value);
        result.insert(tag_name.clone(), formatted_value);
    }

    result
}

// =============================================================================
// TAG VALUE DISPATCH
// =============================================================================

/// Formats a single tag value based on the tag name.
///
/// This function implements the priority-ordered dispatch logic that determines
/// which formatter to apply to a given tag value. The dispatch order is designed
/// to handle the most specific cases first, falling back to more general rules.
///
/// # Dispatch Priority
///
/// 1. GPS string references (GPSLatitudeRef, GPSLongitudeRef)
/// 2. GPS direction refs (GPSImgDirectionRef, GPSDestBearingRef, GPSTrackRef)
/// 3. GPS speed/distance refs (GPSSpeedRef, GPSDestDistanceRef)
/// 4. GPS status tags (GPSStatus, GPSMeasureMode, GPSDifferential)
/// 5. GPS altitude ref (GPSAltitudeRef for binary 0x00/0x01)
/// 6. GPS processing method (GPSProcessingMethod for binary data)
/// 7. Binary decoders (CFAPattern, SceneType, version tags)
/// 8. APP14 flags (APP14Flags0, APP14Flags1: 0 -> "(none)")
/// 9. Enum tags (ExposureProgram)
/// 10. ICC_Profile matrix tags (5 decimal precision for color matrices, white points, etc.)
/// 11. Integer precision tags (ReferenceBlackWhite: whole numbers)
/// 12. Three decimal precision tags (YCbCrCoefficients)
/// 13. UserComment (binary data with encoding prefix)
/// 14. ThumbnailImage (binary -> "(Binary data X bytes, use -b option to extract)")
/// 15. Percentage tags (Quality, MeasurementFlare: append %)
/// 16. Unit suffixes (FocalLength, GPSAltitude)
/// 17. Special values (infinity -> "undef", -0 -> "0")
/// 18. Default: return original value unchanged
///
/// # Arguments
///
/// * `tag_name` - The full tag name, optionally with family prefix (e.g., "EXIF:FocalLength")
/// * `value` - The raw TagValue to format
///
/// # Returns
///
/// A new TagValue containing the formatted result. If no formatting rule applies,
/// returns a clone of the original value.
pub fn format_tag_value(tag_name: &str, value: &TagValue) -> TagValue {
    let base_name = strip_family_prefix(tag_name);

    if base_name == "ProfileEmbedPolicy"
        && let Some(value) = value.as_integer()
        && let Some(label) = match value {
            0 => Some("Allow Copying"),
            1 => Some("Embed if Used"),
            2 => Some("Never Embed"),
            3 => Some("No Restrictions"),
            _ => None,
        }
    {
        return TagValue::new_string(label);
    }

    // ---------------------------------------------------------------------
    // Rule 1: GPS Latitude/Longitude References
    // Convert single-character direction codes to full names
    // ---------------------------------------------------------------------
    if is_gps_lat_ref(base_name)
        && let Some(s) = value.as_string()
    {
        return TagValue::String(format_gps_lat_ref(s));
    }

    if is_gps_lon_ref(base_name)
        && let Some(s) = value.as_string()
    {
        return TagValue::String(format_gps_lon_ref(s));
    }

    // ---------------------------------------------------------------------
    // Rule 2: GPS Direction References (True North / Magnetic North)
    // ---------------------------------------------------------------------
    if is_gps_direction_ref(base_name)
        && let Some(s) = value.as_string()
    {
        return TagValue::String(format_gps_direction_ref(s));
    }

    // ---------------------------------------------------------------------
    // Rule 3: GPS Speed and Distance References
    // ---------------------------------------------------------------------
    if is_gps_speed_ref(base_name)
        && let Some(s) = value.as_string()
        && let Some(formatted) = format_gps_speed_ref(s)
    {
        return TagValue::String(formatted);
    }

    if is_gps_dest_distance_ref(base_name)
        && let Some(s) = value.as_string()
    {
        if let Some(formatted) = format_gps_dest_distance_ref(s) {
            return TagValue::String(formatted);
        }
        // GPS.pm's PrintConv falls through to ExifTool's standard unknown
        // representation for a present-but-empty two-byte reference field.
        if s.trim().is_empty() {
            return TagValue::String("Unknown ()".to_string());
        }
    }

    // ---------------------------------------------------------------------
    // Rule 4: GPS Status Tags (GPSStatus, GPSMeasureMode, GPSDifferential)
    // ---------------------------------------------------------------------
    if is_gps_status_tag(base_name) {
        // Handle string values (e.g., "A", "V", "2", "3", "0", "1")
        if let Some(s) = value.as_string() {
            let formatted = match base_name {
                "GPSStatus" => format_gps_status(s),
                "GPSMeasureMode" => format_gps_measure_mode(s),
                "GPSDifferential" => format_gps_differential(s),
                _ => None,
            };
            if let Some(f) = formatted {
                return TagValue::String(f);
            }
        }
        // Handle integer values for GPSDifferential (0 -> "No Correction", 1 -> "Differential Corrected")
        if base_name == "GPSDifferential"
            && let Some(i) = value.as_integer()
        {
            let formatted = match i {
                0 => Some("No Correction".to_string()),
                1 => Some("Differential Corrected".to_string()),
                _ => None,
            };
            if let Some(f) = formatted {
                return TagValue::String(f);
            }
        }
    }

    // ---------------------------------------------------------------------
    // Rule 5: GPS Altitude Reference (binary 0x00/0x01)
    // ---------------------------------------------------------------------
    if is_gps_altitude_ref(base_name) {
        // Handle string values ("0", "1", "\x00", "\x01")
        if let Some(s) = value.as_string()
            && let Some(formatted) = format_gps_altitude_ref(s)
        {
            return TagValue::String(formatted);
        }
        // Handle binary values (single byte)
        if let TagValue::Binary(data) = value
            && !data.is_empty()
        {
            // Convert first byte to string for the formatter
            let byte_str = match data[0] {
                0 => "0",
                1 => "1",
                _ => return value.clone(),
            };
            if let Some(formatted) = format_gps_altitude_ref(byte_str) {
                return TagValue::String(formatted);
            }
        }
        // Handle integer values
        if let Some(i) = value.as_integer() {
            let int_str = match i {
                0 => "0",
                1 => "1",
                _ => return value.clone(),
            };
            if let Some(formatted) = format_gps_altitude_ref(int_str) {
                return TagValue::String(formatted);
            }
        }
    }

    // ---------------------------------------------------------------------
    // Rule 6: GPS Processing Method (binary data with encoding prefix)
    // ---------------------------------------------------------------------
    if is_gps_processing_method(base_name)
        && let TagValue::Binary(data) = value
    {
        let decoded = decode_gps_processing_method(data);
        if !decoded.is_empty() {
            return TagValue::String(decoded);
        }
    }

    // GPS.pm 0x001c uses the same ConvertExifText RawConv as
    // GPSProcessingMethod: remove the eight-byte EXIF character-code header
    // and expose the decoded area text instead of an opaque binary value.
    if is_gps_area_information(base_name)
        && let TagValue::Binary(data) = value
        && let Some(decoded) = decode_gps_area_information(data)
    {
        return TagValue::String(decoded);
    }

    // ---------------------------------------------------------------------
    // Rule 7: Binary Decoders (CFAPattern, SceneType, version bytes)
    // ---------------------------------------------------------------------
    if is_cfa_pattern(base_name)
        && let TagValue::Binary(data) = value
    {
        return TagValue::String(decode_cfa_pattern(data));
    }

    if is_scene_type(base_name) {
        if let TagValue::Binary(data) = value {
            let decoded = decode_scene_type(data);
            if !decoded.is_empty() {
                return TagValue::String(decoded);
            }
        }
        // Also handle integer values for SceneType
        if let Some(i) = value.as_integer() {
            if i == 1 {
                return TagValue::String("Directly photographed".to_string());
            } else {
                return TagValue::String(format!("Unknown ({})", i));
            }
        }
    }

    if is_version_tag(base_name)
        && let TagValue::Binary(data) = value
    {
        let decoded = decode_version_bytes(data);
        if !decoded.is_empty() {
            return TagValue::String(decoded);
        }
    }

    // GPSVersionID uses a different format than the ASCII-digit version tags:
    // the 4 raw bytes are joined as dot-separated decimal values (e.g. "2.2.0.0").
    if is_gps_version_id(base_name)
        && let TagValue::Binary(data) = value
    {
        return TagValue::String(format_gps_version_id(data));
    }

    // ---------------------------------------------------------------------
    // Rule 8: APP14 Flags (APP14Flags0, APP14Flags1)
    // ExifTool shows "(none)" for value 0, otherwise shows the value
    // ---------------------------------------------------------------------
    if is_app14_flags_tag(base_name)
        && let Some(i) = value.as_integer()
        && i == 0
    {
        return TagValue::String("(none)".to_string());
    }
    // Non-zero values are returned as-is (pass through to default)

    // ---------------------------------------------------------------------
    // Rule 9: Enum Tags (ExposureProgram and other EXIF enum tags)
    // Convert integer enum values to human-readable strings
    // ---------------------------------------------------------------------
    if is_exposure_program(base_name)
        && let Some(i) = value.as_integer()
    {
        // ExposureProgram values are typically small positive integers
        // Safe to cast from i64 to u32 for the formatter
        let formatted = format_exposure_program(i as u32);
        return TagValue::String(formatted);
    }

    // ColorSpace enum (1=sRGB, 65535=Uncalibrated)
    if base_name == "ColorSpace"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_color_space(i));
    }

    // MeteringMode enum (0-6, 255)
    if base_name == "MeteringMode"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_metering_mode(i));
    }

    // LightSource enum (0-24, 255)
    if base_name == "LightSource"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_light_source(i));
    }

    // Flash enum (complex bitfield)
    if base_name == "Flash"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_flash(i));
    }

    // ExposureMode enum (0=Auto, 1=Manual, 2=Auto bracket)
    if base_name == "ExposureMode"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_exposure_mode(i));
    }

    // WhiteBalance enum (0=Auto, 1=Manual)
    if base_name == "WhiteBalance"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_white_balance(i));
    }

    // SceneCaptureType enum (0-3)
    if base_name == "SceneCaptureType"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_scene_capture_type(i));
    }

    // Contrast enum (0=Normal, 1=Low, 2=High)
    if base_name == "Contrast"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_contrast(i));
    }

    // Saturation enum (0=Normal, 1=Low, 2=High)
    if base_name == "Saturation"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_saturation(i));
    }

    // Sharpness enum (0=Normal, 1=Soft, 2=Hard)
    if base_name == "Sharpness"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_sharpness(i));
    }

    // GainControl enum (0-4)
    if base_name == "GainControl"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_gain_control(i));
    }

    // TIFF/EXIF enums whose PrintConv lives in tiff_enum_to_string but which
    // no parser decodes at read time, so their integers used to reach the
    // output raw (13.59 prints "Standard Output Sensitivity" where oxidex
    // printed 1; 1017 instances across the sample corpus, 900 of them
    // SensitivityType). Exif.pm sources: SubfileType 0x00fe,
    // PhotometricInterpretation 0x0106, PlanarConfiguration 0x011c,
    // Predictor 0x013d, SensitivityType 0x8830, CompositeImage 0xa460,
    // MakerNoteSafety 0xc635. A value outside the PrintConv map keeps its
    // integer form: no observed carrier file pins ExifTool's "Unknown (N)"
    // fallback rendering, so it is left unmodeled rather than approximated.
    if let Some(tag_id) = match base_name {
        "SubfileType" => Some(0x00FEu16),
        "PhotometricInterpretation" => Some(0x0106),
        "PlanarConfiguration" => Some(0x011C),
        "Predictor" => Some(0x013D),
        "SensitivityType" => Some(0x8830),
        "CompositeImage" => Some(0xA460),
        "MakerNoteSafety" => Some(0xC635),
        _ => None,
    } && let Some(i) = value.as_integer()
        && let Some(label) = crate::parsers::tiff::tiff_enums::tiff_enum_to_string(tag_id, i)
    {
        return TagValue::String(label);
    }

    // SecurityClassification (Exif.pm:2453-2463) is an ASCII PrintConv, not
    // a numeric TIFF enum. Preserve codes not present in ExifTool's table.
    if base_name == "SecurityClassification"
        && let Some(value) = value.as_string()
        && let Some(label) = format_security_classification(value)
    {
        return TagValue::String(label.to_string());
    }

    // FileSource (Exif.pm:2811). `Writable => 'undef'`, so the TIFF reader
    // hands this over as `TagValue::Binary`, not as a number -- and that is why
    // the integer arm below, which has been correct for as long as it has
    // existed, never ran: `as_integer()` is `None` for a blob. 2,874 corpus
    // files printed `(Binary data 1 bytes, use -b option to extract)` past a
    // working decoder, in every output mode.
    //
    // ExifTool resolves the same mismatch one layer earlier: `ProcessExif`
    // rewrites the format of any one-element UNDEFINED value to `int8u`
    // (Exif.pm:6682, "treat single unknown byte as int8u"), which is what lets
    // a PrintConv hash keyed `1, 2, 3` match a stored `"\x03"` at all. The
    // binary arm reproduces that lookup for this tag rather than changing how
    // every UNDEFINED value in the tree is read.
    if base_name == "FileSource" {
        if let Some(i) = value.as_integer() {
            return TagValue::String(format_file_source(i));
        }
        if let TagValue::Binary(data) = value {
            // A count other than 1 stays `undef` in ExifTool too, and its hash
            // holds exactly one such key -- "\3\0\0\0", the four-byte form
            // Sigma writes, which is a *different* label from a bare `3`.
            if let Some(label) = file_source_label_bytes(data) {
                return TagValue::String(label.to_string());
            }
            // One byte the hash does not name still prints its number:
            // `Unknown (0)` is what ExifTool reports for the four corpus files
            // storing a zero here. A longer unnamed blob is left as a blob
            // rather than given an invented label.
            if let [byte] = data.as_slice() {
                return TagValue::String(format_file_source(i64::from(*byte)));
            }
        }
    }

    // SensingMethod enum (1-8)
    if base_name == "SensingMethod"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_sensing_method(i));
    }

    // FocalPlaneResolutionUnit enum (1-5). Exif.pm 0xa210 declares a PrintConv;
    // this path used to print the raw code, so 1,098 corpus files reported `2`
    // and `3` instead of `inches` and `cm`. The composite ScaleFactor35efl
    // already accepts either spelling (`Some("3") | Some("cm")`).
    if base_name == "FocalPlaneResolutionUnit"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_focal_plane_resolution_unit(i));
    }

    // GrayResponseUnit (Exif.pm 13.59 tag 0x0122) maps the stored SHORT code
    // to the density increment used by GrayResponseCurve.
    if base_name == "GrayResponseUnit"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_gray_response_unit(i));
    }

    // Compression enum (1-65535)
    if base_name == "Compression"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_compression(i));
    }

    // Orientation enum (1-8)
    if base_name == "Orientation"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_orientation(i));
    }

    // ResolutionUnit enum (1-3)
    if base_name == "ResolutionUnit"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_resolution_unit(i));
    }

    // FillOrder (Exif.pm 13.59 tag 0x010a) maps the two defined SHORT codes.
    // Keep unnamed values raw: its PrintConv table has no invented fallback.
    if base_name == "FillOrder"
        && let Some(i) = value.as_integer()
        && let Some(label) = match i {
            1 => Some("Normal"),
            2 => Some("Reversed"),
            _ => None,
        }
    {
        return TagValue::String(label.to_string());
    }

    // YCbCrPositioning enum (1=Centered, 2=Co-sited)
    if base_name == "YCbCrPositioning"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_ycbcr_positioning(i));
    }

    // YCbCrSubSampling (Exif.pm:1417-1426,
    // `PrintConv => \%Image::ExifTool::JPEG::yCbCrSubSampling`). The pair of
    // int16u values arrives here as the space-separated string the SHORT-array
    // reader produced, e.g. "2 1"; ExifTool prints `YCbCr4:2:2 (2 1)`. The
    // formatter for it already existed but nothing called it, so
    // AppleQT-200.jpg reported the bare `2 1`.
    if base_name == "YCbCrSubSampling"
        && let Some(s) = value.as_string()
    {
        return TagValue::String(format_ycbcr_subsampling_string(s));
    }

    // CustomRendered enum (0-8)
    if base_name == "CustomRendered"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_custom_rendered(i));
    }

    // SubjectDistanceRange enum (0-3)
    if base_name == "SubjectDistanceRange"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_subject_distance_range(i));
    }

    // InteropIndex (R98=sRGB, THM=thumbnail, R03=Adobe RGB)
    if base_name == "InteropIndex"
        && let Some(s) = value.as_string()
    {
        return TagValue::String(format_interop_index(s));
    }

    // ComponentsConfiguration binary data
    if base_name == "ComponentsConfiguration"
        && let TagValue::Binary(data) = value
    {
        return TagValue::String(format_components_configuration(data));
    }

    // ---------------------------------------------------------------------
    // Rule 10: ICC_Profile Matrix Tags (5 decimal precision)
    // Format float values in ICC profile tags with up to 5 decimal places.
    // MeasurementFlare requires a "%" suffix after formatting.
    //
    // `is_icc_matrix_tag` matches on the bare tag name, but "ColorMatrix1"
    // and "ColorMatrix2" also exist as `PhaseOne::Main` tags (0x0106/0x0226,
    // PhaseOne.pm:51,159), which the makernote parser already formats with
    // ExifTool's real PrintConv for that table -- a fixed `sprintf("%.3f")`
    // that keeps trailing zeros, unlike this rule's 5-place-max/trimmed
    // ICC/DNG formatting. Re-running an already-ExifTool-formatted PhaseOne
    // string through this rule silently corrupted it ("1.280" -> "1.28").
    // PhaseOne's own group prefix is the only thing that disambiguates the
    // two, so check it before falling into the generic name-only rule.
    if is_icc_matrix_tag(base_name) && !tag_name.starts_with("PhaseOne:") {
        // Handle string values that contain space-separated floats
        // (e.g., "0.1491851806640625 0.0632171630859375 0.74456787109375")
        if let Some(s) = value.as_string() {
            let formatted = format_icc_string_values(s, base_name);
            return TagValue::String(formatted);
        }
        // Handle single float values
        if let Some(f) = value.as_float() {
            let formatted = format_icc_value(f);
            // Add "%" suffix for MeasurementFlare
            if base_name == "MeasurementFlare" {
                return TagValue::String(format!("{}%", formatted));
            }
            return TagValue::String(formatted);
        }
    }

    // ---------------------------------------------------------------------
    // Rule 11: Integer Precision Tags (ReferenceBlackWhite)
    // Format whole numbers without decimal places (0, 255, 128 not 0.0, 255.0)
    // ---------------------------------------------------------------------
    if is_integer_precision_tag(base_name)
        && let Some(s) = value.as_string()
    {
        let formatted = format_integer_precision_values(s);
        return TagValue::String(formatted);
    }

    // ---------------------------------------------------------------------
    // Rule 12: Three Decimal Precision Tags (YCbCrCoefficients)
    // Format with 3 decimal places (0.299 0.587 0.114 not 0.2990000000...)
    // ---------------------------------------------------------------------
    if is_three_decimal_tag(base_name)
        && let Some(s) = value.as_string()
    {
        let formatted = format_three_decimal_values(s);
        return TagValue::String(formatted);
    }

    // ---------------------------------------------------------------------
    // Rule 13: UserComment (decode binary text encoding)
    // UserComment has 8-byte encoding prefix (ASCII/UNICODE/JIS) + text
    // ---------------------------------------------------------------------
    if is_user_comment(base_name)
        && let TagValue::Binary(data) = value
        && let Some(decoded) = decode_user_comment(data)
    {
        // ConvertExifText returns the trimmed string unconditionally -- empty
        // included. OlympusD450Z.jpg stores an all-NUL identifier plus 117
        // spaces of padding; ExifTool prints an empty UserComment, so an empty
        // decode must not fall through to the binary placeholder.
        return TagValue::String(decoded);
    }

    // ---------------------------------------------------------------------
    // Rule 14: ThumbnailImage / OtherImage (format binary embedded images)
    // Format embedded image blobs with ExifTool-compatible message.
    // OtherImage is the 0x0201/0x0202 pair's blob from a non-IFD1 directory
    // (see `parse_interop_subifd`).
    // ---------------------------------------------------------------------
    if (is_thumbnail_image(base_name) || base_name == "OtherImage")
        && let TagValue::Binary(data) = value
    {
        return TagValue::String(format!(
            "(Binary data {} bytes, use -b option to extract)",
            data.len()
        ));
    }

    // ---------------------------------------------------------------------
    // Rule 14a: TransferFunction / LinearizationTable (Exif.pm Binary => 1)
    // ---------------------------------------------------------------------
    // ExifTool decodes this int16u array to its space-separated textual form
    // before applying the Binary flag, so its reported byte count is the UTF-8
    // length of that rendered payload rather than the raw TIFF value length.
    if matches!(base_name, "TransferFunction" | "LinearizationTable")
        && let Some(payload) = value.as_string()
    {
        return TagValue::String(format!(
            "(Binary data {} bytes, use -b option to extract)",
            payload.len()
        ));
    }

    // APP10 AROT's int32uRev gain curve carries `Binary => 1` the same way:
    // ExifTool's ordinary output reports the rendered payload length, and
    // oxidex keeps that payload as the identical space-separated decimal
    // string, so its UTF-8 byte length is exactly the size ExifTool displays
    // (13.59 prints "(Binary data 1160 bytes, use -b option to extract)" for
    // Apple_iPadAir_3rd_generation.jpg).
    if base_name == "HDRGainCurve"
        && let Some(payload) = value.as_string()
    {
        return TagValue::String(format!(
            "(Binary data {} bytes, use -b option to extract)",
            payload.len()
        ));
    }

    // Exif.pm `%longBin` leaves ProfileToneCurve's decoded float list visible
    // only when its textual payload is at most 64 bytes.
    if base_name == "ProfileToneCurve"
        && let Some(payload) = value.as_string()
        && payload.len() > 64
    {
        return TagValue::String(format!(
            "(Binary data {} bytes, use -b option to extract)",
            payload.len()
        ));
    }

    // ---------------------------------------------------------------------
    // Rule 15: Percentage Tags (Quality, MeasurementFlare)
    // Append "%" suffix to numeric values representing percentages
    // Note: MeasurementFlare is also handled in the ICC matrix rule above,
    // but Quality (from Ducky segment) is handled here for integer values.
    // ---------------------------------------------------------------------
    // Note: only Ducky's "Quality" tag (or a bare, family-less "Quality")
    // gets a "%" suffix -- other formats that happen to share the tag name
    // (e.g. RIFF/AVI's numeric stream Quality, unrelated to percentages)
    // must NOT be reformatted here.
    let quality_percentage_applies =
        base_name == "MeasurementFlare" || tag_name == "Ducky:Quality" || tag_name == "Quality";
    if quality_percentage_applies && is_percentage_tag(base_name) {
        if let Some(i) = value.as_integer() {
            return TagValue::String(format!("{}%", i));
        }
        if let Some(f) = value.as_float() {
            // Format floats: remove trailing zeros for clean output
            // e.g., 84.0 -> "84%", 84.5 -> "84.5%"
            let formatted = if f.fract() == 0.0 {
                format!("{}%", f as i64)
            } else {
                format!("{}%", f)
            };
            return TagValue::String(formatted);
        }
    }

    // ---------------------------------------------------------------------
    // Rule 16: Unit Suffixes (FocalLength -> mm, GPSAltitude -> m)
    // ---------------------------------------------------------------------
    if is_unit_suffix_tag(base_name) {
        // For string values, apply unit suffix directly
        if let Some(s) = value.as_string() {
            return TagValue::String(format_with_unit(tag_name, s));
        }
        // For numeric values, convert to string first then apply suffix
        if let Some(i) = value.as_integer() {
            let formatted = format_with_unit(tag_name, &i.to_string());
            return TagValue::String(formatted);
        }
        if let Some(f) = value.as_float() {
            // Format floats reasonably - avoid excessive decimal places
            let float_str = if f.fract() == 0.0 {
                format!("{:.0}", f)
            } else {
                format!("{}", f)
            };
            let formatted = format_with_unit(tag_name, &float_str);
            return TagValue::String(formatted);
        }
        // Handle Rational values
        if let TagValue::Rational {
            numerator,
            denominator,
        } = value
        {
            // Exif.pm 0x9206 prints zero-denominator rational values as
            // `inf` (nonzero numerator) or `undef` (zero numerator), without
            // the normal meter suffix.
            if base_name == "SubjectDistance" && *denominator == 0 {
                return TagValue::String(if *numerator == 0 { "undef" } else { "inf" }.into());
            }

            if *denominator != 0 {
                let float_val = *numerator as f64 / *denominator as f64;
                let float_str = if float_val.fract() == 0.0 {
                    format!("{:.0}", float_val)
                } else {
                    format!("{}", float_val)
                };
                let formatted = format_with_unit(tag_name, &float_str);
                return TagValue::String(formatted);
            }
        }
    }

    // ---------------------------------------------------------------------
    // Rule 16b: AmbientTemperature carries a unit, not a number format.
    //
    // Exif.pm:2532-2538 -- `0x9400 => { Name => 'AmbientTemperature',
    // Writable => 'rational64s', PrintConv => '"$val C"' }`. The number is
    // whatever GetRational64s rounded it to, so the sign of a negative zero
    // survives: `exiftool -G1 -s` on Olympus/OlympusOM-1.jpg prints `-0 C`,
    // and oxidex printed `-0` with the unit missing entirely.
    //
    // Only the EXIF tag is meant here. Several maker-note tables define their
    // own temperature tags with different PrintConvs (Olympus's
    // CameraTemperature among them), so this must not key on the bare name.
    // ---------------------------------------------------------------------
    if matches!(
        tag_name,
        "EXIF:AmbientTemperature" | "ExifIFD:AmbientTemperature"
    ) && let TagValue::Rational {
        numerator,
        denominator,
    } = value
        && *denominator != 0
    {
        return TagValue::String(format!(
            "{} C",
            exiftool_rational_number(f64::from(*numerator) / f64::from(*denominator))
        ));
    }

    // ---------------------------------------------------------------------
    // Rule 17: Special Values (infinity -> "undef", -0 -> "0")
    // Handle special float/rational values that result from invalid/undefined data.
    // GPS tags like GPSDestBearing/GPSDestDistance produce infinity when
    // the denominator is 0. ExifTool displays "undef" for these cases.
    // Also handles string representations ("inf", "-0") for values already
    // converted to string.
    // ---------------------------------------------------------------------
    if let Some(f) = value.as_float()
        && let Some(formatted) = format_special_float_values(f)
    {
        return TagValue::String(formatted);
    }
    // Handle Rational values with denominator 0 (would produce infinity)
    if let TagValue::Rational { denominator, .. } = value
        && *denominator == 0
    {
        return TagValue::String("undef".to_string());
    }
    // Also handle string representations of special values.
    //
    // A few tags print the literal "inf" on purpose rather than as the symptom
    // of a divide-by-zero: Minolta's FocusDistance has the PrintConv
    // `$val ? "$val m" : "inf"`, so a focus distance of zero means "focused at
    // infinity" and ExifTool reports exactly "inf". Rewriting those to "undef"
    // would replace a real reading with an error marker. Canon's
    // `%focusDistanceByteSwap` (Canon.pm:1200, backing `%Canon::CameraInfo*`
    // FocusDistanceUpper/Lower) has the same shape the other way around:
    // `$val > 655.345 ? "inf" : "$val m"` -- the raw sentinel 0xffff (655.35 m
    // after the ValueConv) means "not focused / infinity", and is exactly as
    // deliberate as Minolta's zero.
    const DELIBERATE_INFINITY: &[&str] =
        &["FocusDistance", "FocusDistanceUpper", "FocusDistanceLower"];
    if let Some(s) = value.as_string() {
        if (s == "inf" || s == "-inf" || s == "Infinity" || s == "-Infinity")
            && !DELIBERATE_INFINITY.contains(&base_name)
        {
            return TagValue::String("undef".to_string());
        }
        if s == "-0" || s == "-0.0" {
            return TagValue::String("0".to_string());
        }
    }

    // ---------------------------------------------------------------------
    // Rule 18: XMP Boolean Formatting
    //
    // ExifTool capitalizes; this rule used to do the exact opposite, under a
    // comment asserting "ExifTool uses lowercase 'true'/'false'". XMP.pm's
    // `%boolConv` is the authority, and it case-folds *up*:
    //
    //     return 'False' if lc $val eq 'false';
    //     return 'True'  if lc $val eq 'true';
    //     True => 'True', False => 'False',
    //
    // and XMP.pm:3690 warns "Boolean value ... should be capitalized" when a
    // file stores the lowercase form -- ExifTool treats lowercase as the
    // malformed input, not the output.
    //
    // Scope matters as much as direction. `%boolConv` is attached to exactly
    // three tags (XMP-exif Flash Fired/Function/RedEyeMode, XMP.pm:2139-2158);
    // the other 51 `Writable => 'boolean'` XMP tags carry no PrintConv at all,
    // so ExifTool prints whatever literal the file holds. That is why `Marked`
    // reads `True`: PLUS.xmp contains `<xmpRights:Marked>True`, and ExifTool
    // passes it through. Blanket-lowercasing every XMP string therefore did not
    // merely mis-case the three converted tags -- it destroyed the stored value
    // of every boolean tag that was already right, and would corrupt a
    // legitimate `XMP-dc:Description` of "True" for good measure.
    // ---------------------------------------------------------------------
    if is_xmp_bool_conv_tag(tag_name, base_name)
        && let Some(s) = value.as_string()
    {
        if s.eq_ignore_ascii_case("true") {
            return TagValue::String("True".to_string());
        }
        if s.eq_ignore_ascii_case("false") {
            return TagValue::String("False".to_string());
        }
    }

    // ---------------------------------------------------------------------
    // Rule 19: XMP LensInfo Formatting
    // ExifTool formats LensInfo as "45-100mm f/4" instead of raw rationals
    // Format: "{min}-{max}mm f/{f_min}[-{f_max}]" or "{focal}mm f/{f}" for primes
    // ---------------------------------------------------------------------
    if base_name == "LensInfo"
        && tag_name.starts_with("XMP")
        && let Some(s) = value.as_string()
        && let Some(formatted) = format_xmp_lens_info(s)
    {
        return TagValue::String(formatted);
    }

    // ---------------------------------------------------------------------
    // Rule 19a: EXIF/TIFF LensInfo Formatting (Exif.pm:5800 `PrintLensInfo`)
    // Renders the 4 space-separated rational64u values (min focal, max focal,
    // min f-number, max f-number) as "12-20mm f/3.8-4.5" or "50mm f/1.4" for a
    // prime lens. Unlike the XMP variant above, ExifTool's PrintLensInfo works
    // on already-stringified tokens with plain string truthy/equality checks
    // (0 is falsy; "45" ne "15" is a string compare) rather than 0/0-as-unknown
    // rational parsing, so it needs its own implementation.
    // ---------------------------------------------------------------------
    if base_name == "LensInfo"
        && !tag_name.starts_with("XMP")
        && let Some(s) = value.as_string()
        && let Some(formatted) = format_exif_lens_info(s)
    {
        return TagValue::String(formatted);
    }

    // ---------------------------------------------------------------------
    // Rule 19b/19c: APEX-stored tags, which DO have a PrintConv and so must
    // be resolved before the catch-all below turns them into plain numbers.
    // Keep PrintConv here. The Composite layer consumes the raw ValueConv via
    // [`apex_value_conv`] before this output-time rendering step.
    // ---------------------------------------------------------------------
    if let Some(converted) = apex_print_conv(base_name, value) {
        return converted;
    }

    // ---------------------------------------------------------------------
    // Rule 19d: ExposureTime (Exif.pm:1823-1828)
    //     PrintConv => 'Image::ExifTool::Exif::PrintExposureTime($val)'
    //
    // Most parsers hand this over already rendered as a string; the rational
    // form still reaches here from the RAW paths (FujiFilm.raf, CanonRaw.cr3).
    // ---------------------------------------------------------------------
    if base_name == "ExposureTime"
        && let TagValue::Rational {
            numerator,
            denominator,
        } = value
        && *denominator != 0
    {
        return TagValue::String(print_exposure_time(
            f64::from(*numerator) / f64::from(*denominator),
        ));
    }

    // ---------------------------------------------------------------------
    // Rule 19e: ExposureCompensation / ExposureBiasValue (Exif.pm:2342-2349)
    //     PrintConv => 'Image::ExifTool::Exif::PrintFraction($val)'
    //
    // 0x9204 is `ExposureCompensation` to ExifTool and `ExposureBiasValue` to
    // the EXIF spec (Exif.pm:2345 Notes), so both names route here. Without
    // this the rational fell through to Rule 20's plain `%.10g` quotient and
    // `exiftool -a -G1 -s` disagreed on the sign and the rounding alike:
    // `1.326429536` where ExifTool prints `+1.33`, `0.0` where it prints `0`,
    // and `1` where it prints `+1`.
    // ---------------------------------------------------------------------
    if matches!(base_name, "ExposureCompensation" | "ExposureBiasValue")
        && let TagValue::Rational {
            numerator,
            denominator,
        } = value
        && *denominator != 0
    {
        return TagValue::String(print_fraction(
            f64::from(*numerator) / f64::from(*denominator),
        ));
    }

    // ---------------------------------------------------------------------
    // Rule 19f: FNumber (Exif.pm:1853-1858)
    //     0x829d => { Name => 'FNumber', Writable => 'rational64u',
    //                 PrintConv => 'Image::ExifTool::Exif::PrintFNumber($val)',
    //                 PrintConvInv => '$val' }
    //
    // 0x829d had no arm at all, so its rational fell through to Rule 20's
    // `%.10g` quotient. That drops the decimal place ExifTool's `%.1f` keeps
    // (`4` where ExifTool prints `4.0` -- 964 sample-corpus files) and prints
    // the full stored expansion where ExifTool rounds (`2.638671875` for
    // `2.6`, `0.640234375` for `0.64` -- another 71).
    //
    // This is a *display* conversion, and 0x829d has no ValueConv: the
    // Composite `Aperture` (Exif.pm:4782, `ValueConv => '$val[0] || $val[1]'`)
    // reads the raw quotient, not this string, and applies `PrintFNumber`
    // itself. Composites are derived before `format_tag_value` runs at all --
    // `composite::lookup_key` reads the stored `TagValue` -- so this arm
    // cannot reach them, which the corpus run confirms: no `Composite:*` value
    // changes.
    // ---------------------------------------------------------------------
    if base_name == "FNumber"
        && let TagValue::Rational {
            numerator,
            denominator,
        } = value
        && *denominator != 0
    {
        return TagValue::String(print_f_number(
            f64::from(*numerator) / f64::from(*denominator),
        ));
    }

    // ---------------------------------------------------------------------
    // Rule 20: PrintConv-less rationals
    //
    // A rational that reaches this point carries no PrintConv of its own, and
    // ExifTool prints such a value as the quotient its rational reader already
    // rounded: `RoundFloat($ratNumer / $ratDenom, 10)`, i.e. `%.10g`
    // (ExifTool.pm:6081-6097 GetRational64s/GetRational64u, :5937-5941
    // RoundFloat). Leaving the Rational intact instead let every downstream
    // renderer invent its own precision -- nine decimal places in one place, a
    // literal `num/den` in another -- so CanonRaw.cr3's FocalPlaneXResolution
    // printed `6514.657980456` against ExifTool's `6514.65798`.
    //
    // A zero denominator is deliberately not handled here: Rule 17 above has
    // already turned it into "undef".
    // ---------------------------------------------------------------------
    if let TagValue::Rational {
        numerator,
        denominator,
    } = value
        && *denominator != 0
    {
        return TagValue::String(exiftool_rational_number(
            f64::from(*numerator) / f64::from(*denominator),
        ));
    }

    // ---------------------------------------------------------------------
    // Rule 21: Default - Return original value unchanged
    // ---------------------------------------------------------------------
    value.clone()
}

/// Applies ValueConv for an APEX-stored rational without applying PrintConv.
///
/// ApertureValue (Exif.pm:2327-2335) and MaxApertureValue (Exif.pm:2350-2359)
/// are both `ValueConv => '2 ** ($val / 2)', PrintConv => 'sprintf("%.1f",$val)'`
/// -- stored as an APEX value. The stored 3.625 in CanonRaw.cr3 converts to
/// the f-number 3.5 before its one-decimal PrintConv is applied.
///
/// ShutterSpeedValue (Exif.pm:2317-2326) is
/// `ValueConv => 'IsFloat($val) && abs($val)<100 ? 2**(-$val) : 0'`,
/// `PrintConv => 'Image::ExifTool::Exif::PrintExposureTime($val)'`.
///
/// The Composite layer (`src/composite/mod.rs`) consumes this raw numeric
/// value. It must not receive a formatted reciprocal or rounded f-number:
/// ExifTool's Composite table reads post-ValueConv values before PrintConv.
pub(crate) fn apex_value_conv(base_name: &str, value: &TagValue) -> Option<TagValue> {
    let TagValue::Rational {
        numerator,
        denominator,
    } = value
    else {
        return None;
    };
    if *denominator == 0 {
        return None;
    }
    let apex = f64::from(*numerator) / f64::from(*denominator);

    if matches!(base_name, "ApertureValue" | "MaxApertureValue") {
        return Some(TagValue::Float(2f64.powf(apex / 2.0)));
    }

    if base_name == "ShutterSpeedValue" {
        let seconds = if apex.abs() < 100.0 {
            2f64.powf(-apex)
        } else {
            0.0
        };
        return Some(TagValue::Float(seconds));
    }

    None
}

/// Applies the APEX PrintConv after [`apex_value_conv`] has produced the raw
/// value used by dependent Composite tags.
fn apex_print_conv(base_name: &str, value: &TagValue) -> Option<TagValue> {
    let TagValue::Float(converted) = apex_value_conv(base_name, value)? else {
        return None;
    };
    match base_name {
        "ApertureValue" | "MaxApertureValue" => Some(TagValue::String(format!("{converted:.1}"))),
        "ShutterSpeedValue" => Some(TagValue::String(print_exposure_time(converted))),
        _ => None,
    }
}

// =============================================================================
// HELPER FUNCTIONS - Special Value Formatting
// =============================================================================

/// Formats special float values (infinity, negative zero) to match ExifTool output.
///
/// When GPS data has invalid rational values (e.g., denominator = 0), OxiDex
/// computes infinity. ExifTool instead shows "undef" for these cases. This
/// function handles these special cases to maintain ExifTool compatibility.
///
/// # Arguments
///
/// * `value` - The float value to check
///
/// # Returns
///
/// * `Some("undef")` - If the value is positive or negative infinity
/// * `Some("0")` - If the value is negative zero (-0.0)
/// * `None` - If the value is a normal number that should be formatted normally
///
/// # Why This Matters
///
/// GPS tags like GPSDestBearing and GPSDestDistance store values as rational
/// numbers (numerator/denominator). When the denominator is 0, division produces
/// infinity. ExifTool recognizes this as invalid data and displays "undef".
/// Similarly, negative zero can occur in edge cases and should normalize to "0".
///
/// # Examples
///
/// ```rust,ignore
/// assert_eq!(format_special_float_values(f64::INFINITY), Some("undef".to_string()));
/// assert_eq!(format_special_float_values(f64::NEG_INFINITY), Some("undef".to_string()));
/// assert_eq!(format_special_float_values(-0.0), Some("0".to_string()));
/// assert_eq!(format_special_float_values(42.5), None);
/// ```
fn format_special_float_values(value: f64) -> Option<String> {
    // Check for infinity (positive or negative) - indicates invalid rational (div by zero)
    if value.is_infinite() {
        return Some("undef".to_string());
    }

    // Check for negative zero - normalize to "0"
    // Note: -0.0 == 0.0 in Rust, so we use is_sign_negative() to detect it
    if value == 0.0 && value.is_sign_negative() {
        return Some("0".to_string());
    }

    // Normal value - no special formatting needed
    None
}

/// Perl's default number-to-string stringification for a floating-point
/// value (an NV): the C library's `%.15g` -- 15 significant digits, fixed
/// notation for the magnitudes this module deals with (focal lengths and
/// f-numbers never approach `%g`'s scientific-notation cutoffs), with
/// trailing fractional zeros (and a bare trailing '.') trimmed.
///
/// `ConvertRational` (XMP.pm:3400-3412) computes `$1/$2` as a plain Perl
/// division with no `sprintf` in the pipeline before `PrintLensInfo`
/// concatenates it into the display string, so whatever Perl's own NV
/// stringification produces *is* the printed value -- e.g. `807365/524263`
/// (a real Pixel `aux:LensInfo` numerator/denominator pair) becomes
/// `1.53999996185121`, not a value rounded to one or two decimals.
fn format_perl_number(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let magnitude = v.abs().log10().floor() as i32;
    let decimals = (14 - magnitude).clamp(0, 17) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Formats XMP LensInfo from rational string to human-readable format.
///
/// XMP stores LensInfo as space-separated rationals like "4500/100 10000/100 400/100 400/100"
/// which ExifTool formats as "45-100mm f/4" for user-friendly display.
///
/// # Format Rules
/// - Prime lens (same min/max focal): "{focal}mm f/{f}"
/// - Zoom with constant aperture: "{min}-{max}mm f/{f}"
/// - Zoom with variable aperture: "{min}-{max}mm f/{f_min}-{f_max}"
///
/// # Arguments
///
/// * `value` - The raw XMP LensInfo string with space-separated rationals
///
/// # Returns
///
/// * `Some(formatted)` - If parsing succeeds
/// * `None` - If parsing fails (returns original value unchanged)
/// Formats an already-stringified EXIF/TIFF `LensInfo` (tag 0xa432) value the way
/// ExifTool's `Image::ExifTool::Exif::PrintLensInfo` does (Exif.pm:5800):
///
/// ```perl
/// my @vals = split ' ', $val;
/// return $val unless @vals == 4;
/// my $c = 0;
/// foreach (@vals) {
///     Image::ExifTool::IsFloat($_) and ++$c, next;
///     $_ eq 'inf' and $_ = '?', ++$c, next;
///     $_ eq 'undef' and $_ = '?', ++$c, next;
/// }
/// return $val unless $c == 4;
/// $val = $vals[0];
/// $val .= "-$vals[1]" if $vals[1] and $vals[1] ne $vals[0];
/// $val .= "mm f/$vals[2]";
/// $val .= "-$vals[3]" if $vals[3] and $vals[3] ne $vals[2];
/// return $val;
/// ```
///
/// Note this is a *string* truthy/equality check on the original tokens (Perl's `0` is
/// falsy, `ne` is a string compare), not a numeric-tolerance comparison -- distinct from
/// [`format_xmp_lens_info`], which parses raw `num/den` rationals and treats `0/0` as an
/// unknown aperture. `IsFloat` unmatched-but-not-`inf`/`undef` tokens fall through with
/// `$c` unincremented, so `Some`/`None` here mirrors that "leave it alone" behavior.
fn format_exif_lens_info(value: &str) -> Option<String> {
    let vals: Vec<&str> = value.split_whitespace().collect();
    if vals.len() != 4 {
        return None;
    }

    let is_float = |s: &str| -> bool { s.parse::<f64>().is_ok() };

    let mut tokens: Vec<String> = Vec::with_capacity(4);
    let mut ok_count = 0;
    for &v in &vals {
        if is_float(v) {
            tokens.push(v.to_string());
            ok_count += 1;
        } else if v == "inf" || v == "undef" {
            tokens.push("?".to_string());
            ok_count += 1;
        } else {
            tokens.push(v.to_string());
        }
    }
    if ok_count != 4 {
        return None;
    }

    // Perl truthy: only "" and "0" are false among numeric-looking strings.
    let truthy = |s: &str| -> bool { s != "0" && !s.is_empty() };

    let mut result = tokens[0].clone();
    if truthy(&tokens[1]) && tokens[1] != tokens[0] {
        result.push('-');
        result.push_str(&tokens[1]);
    }
    result.push_str("mm f/");
    result.push_str(&tokens[2]);
    if truthy(&tokens[3]) && tokens[3] != tokens[2] {
        result.push('-');
        result.push_str(&tokens[3]);
    }
    Some(result)
}

fn format_xmp_lens_info(value: &str) -> Option<String> {
    // Parse space-separated rationals: "4500/100 10000/100 400/100 400/100"
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() != 4 {
        return None;
    }

    // XMP.pm:3400-3412 `ConvertRational`: `$1/$2` for a nonzero denominator,
    // `'inf'` for N/0, `'undef'` for 0/0. `Exif.pm:5800-5818` `PrintLensInfo`
    // then maps both `inf` and `undef` to `'?'`.
    //
    // Returns `Some(quotient)` for a valid division, or `None` for the two
    // sentinel cases (0/0 "undef" and N/0 "inf") -- both print as `?`, so
    // this function doesn't need to tell them apart.
    let parse_rational = |s: &str| -> Option<f64> {
        let r: Vec<&str> = s.split('/').collect();
        if r.len() == 2 {
            let num: f64 = r[0].parse().ok()?;
            let den: f64 = r[1].parse().ok()?;
            if den != 0.0 {
                return Some(num / den);
            }
        }
        None
    };

    let min_focal = parse_rational(parts[0])?;
    let max_focal = parse_rational(parts[1])?;
    // Apertures can be 0/0 (unknown) or N/0 (inf); both print as "?".
    let f_at_min = parse_rational(parts[2]);
    let f_at_max = parse_rational(parts[3]);

    // `Exif.pm:5800-5818` `PrintLensInfo` does no rounding at all: it prints
    // whatever `ConvertRational`'s `$1/$2` division produced, which is Perl's
    // default number-to-string stringification (%.15g -- e.g. a Pixel's
    // `807365/524263` prints as `1.53999996185121`, not a rounded `1.5`).
    let focal_min_s = format_perl_number(min_focal);
    let focal_max_s = format_perl_number(max_focal);

    // Build result. `$val .= "-$vals[1]" if $vals[1] and $vals[1] ne $vals[0]`:
    // Perl's `$vals[1]` truth test is false for the strings "0" and "" (the
    // comment above the Perl source notes the Pentax Q writes zero for the
    // upper bound of a fixed-focal-length lens), and `ne` is a *string*
    // comparison against the already-stringified value, not a numeric one.
    let focal_str = if focal_max_s == "0" || focal_max_s == focal_min_s {
        focal_min_s
    } else {
        format!("{focal_min_s}-{focal_max_s}")
    };

    // Handle unknown apertures (0/0 "undef" or N/0 "inf" -> "?")
    let f_str = match (f_at_min, f_at_max) {
        (None, _) | (_, None) => "?".to_string(),
        (Some(f_min), Some(f_max)) => {
            let f_min_s = format_perl_number(f_min);
            let f_max_s = format_perl_number(f_max);
            if f_max_s == "0" || f_max_s == f_min_s {
                f_min_s
            } else {
                format!("{f_min_s}-{f_max_s}")
            }
        }
    };

    Some(format!("{}mm f/{}", focal_str, f_str))
}

// =============================================================================
// HELPER FUNCTIONS - Tag Name Classification
// =============================================================================

/// Strips the family/group prefix from a tag name.
///
/// Tag names may include a family prefix separated by a colon (e.g., "EXIF:Make",
/// "GPS:GPSLatitude", "XMP:Creator"). This function extracts just the base tag
/// name for comparison against formatting rules.
///
/// # Arguments
///
/// * `tag_name` - The full tag name, possibly with a family prefix
///
/// # Returns
///
/// The base tag name without any prefix. If there's no colon, returns the
/// original string unchanged.
///
/// # Examples
///
/// ```rust,ignore
/// assert_eq!(strip_family_prefix("EXIF:FocalLength"), "FocalLength");
/// assert_eq!(strip_family_prefix("GPS:GPSLatitude"), "GPSLatitude");
/// assert_eq!(strip_family_prefix("FocalLength"), "FocalLength");
/// ```
pub fn strip_family_prefix(tag_name: &str) -> &str {
    // Find the last colon to handle nested prefixes like "Composite:EXIF:Tag"
    tag_name.rsplit(':').next().unwrap_or(tag_name)
}

/// Checks if the tag is a GPS latitude reference (GPSLatitudeRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a latitude reference
pub fn is_gps_lat_ref(base_name: &str) -> bool {
    // GPSDestLatitudeRef (0x0013) shares GPSLatitudeRef's PrintConv table --
    // both are `PrintConv => \%printConvLatRef` (GPS.pm:74 and GPS.pm:245), so
    // both print `North`/`South`. Only the first was listed here, leaving
    // SamsungL73.jpg's GPSDestLatitudeRef as the raw `N`.
    matches!(base_name, "GPSLatitudeRef" | "GPSDestLatitudeRef")
}

/// Checks if the tag is a GPS longitude reference (GPSLongitudeRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a longitude reference
pub fn is_gps_lon_ref(base_name: &str) -> bool {
    // Same pairing as the latitude refs: GPSDestLongitudeRef (0x0015) uses
    // `PrintConv => \%printConvLonRef` exactly as GPSLongitudeRef does
    // (GPS.pm:91 and GPS.pm:258), printing `East`/`West`.
    matches!(base_name, "GPSLongitudeRef" | "GPSDestLongitudeRef")
}

/// Checks if the tag is a GPS direction reference.
///
/// Direction reference tags indicate whether a direction measurement is relative
/// to True North or Magnetic North. Applicable tags:
/// - GPSImgDirectionRef
/// - GPSDestBearingRef
/// - GPSTrackRef
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a direction reference
pub fn is_gps_direction_ref(base_name: &str) -> bool {
    matches!(
        base_name,
        "GPSImgDirectionRef" | "GPSDestBearingRef" | "GPSTrackRef"
    )
}

/// Checks if the tag is a GPS speed reference (GPSSpeedRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a speed reference
pub fn is_gps_speed_ref(base_name: &str) -> bool {
    base_name == "GPSSpeedRef"
}

/// Checks if the tag is a GPS destination distance reference (GPSDestDistanceRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a distance reference
pub fn is_gps_dest_distance_ref(base_name: &str) -> bool {
    base_name == "GPSDestDistanceRef"
}

/// Checks if the tag is a GPS status-related tag.
///
/// Status tags include:
/// - GPSStatus (measurement active/void)
/// - GPSMeasureMode (2D/3D measurement)
/// - GPSDifferential (differential correction applied)
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a GPS status value
pub fn is_gps_status_tag(base_name: &str) -> bool {
    matches!(
        base_name,
        "GPSStatus" | "GPSMeasureMode" | "GPSDifferential"
    )
}

/// Checks if the tag is a GPS altitude reference (GPSAltitudeRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as an altitude reference
pub fn is_gps_altitude_ref(base_name: &str) -> bool {
    base_name == "GPSAltitudeRef"
}

/// Checks if the tag is GPS processing method (GPSProcessingMethod).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as GPS processing method binary data
pub fn is_gps_processing_method(base_name: &str) -> bool {
    base_name == "GPSProcessingMethod"
}

/// Checks if the tag is GPS area information (GPSAreaInformation).
pub fn is_gps_area_information(base_name: &str) -> bool {
    base_name == "GPSAreaInformation"
}

/// Applies GPS.pm 0x001c's ASCII `ConvertExifText` branch exactly. Other
/// character-code identifiers are left opaque until their byte-order/charset
/// rules can be reproduced without guessing.
fn decode_gps_area_information(data: &[u8]) -> Option<String> {
    let text = data.strip_prefix(b"ASCII\0\0\0")?;
    let text = text.split(|byte| *byte == 0).next().unwrap_or_default();
    let text = std::str::from_utf8(text).ok()?;
    Some(text.trim_end_matches(' ').to_string())
}

/// Checks if the tag is a CFA pattern (CFAPattern).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as CFA pattern binary data
pub fn is_cfa_pattern(base_name: &str) -> bool {
    // Also handle alternate spellings
    matches!(base_name, "CFAPattern" | "CFAPattern2")
}

/// Checks if the tag is scene type (SceneType).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as scene type
pub fn is_scene_type(base_name: &str) -> bool {
    base_name == "SceneType"
}

/// Checks if the tag is a version tag that stores data as ASCII bytes.
///
/// Version tags include:
/// - InteropVersion
/// - ExifVersion
/// - FlashpixVersion
///
/// Note: `GPSVersionID` is *not* one of these -- unlike the tags above, its
/// 4 raw bytes are not ASCII digit characters. ExifTool prints it by joining
/// the 4 raw byte values with dots (e.g. `[2, 2, 0, 0]` -> `"2.2.0.0"`); see
/// [`is_gps_version_id`] / [`format_gps_version_id`] for that formatting.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as version bytes
pub fn is_version_tag(base_name: &str) -> bool {
    matches!(
        base_name,
        "InteropVersion" | "ExifVersion" | "FlashpixVersion"
    )
}

/// Checks if the tag is one ExifTool converts with XMP.pm's `%boolConv`.
///
/// `%boolConv` (XMP.pm:246) case-folds a boolean *up* -- `'True' if lc $val eq
/// 'true'` -- and it is attached to exactly three tags, the XMP-exif `Flash`
/// struct's `Fired`, `Function` and `RedEyeMode` members (XMP.pm:2139-2158),
/// which ExifTool flattens to `FlashFired` / `FlashFunction` /
/// `FlashRedEyeMode`.
///
/// The 51 other `Writable => 'boolean'` XMP tags -- `Marked` among them --
/// declare no `PrintConv`, so ExifTool reports the literal the file stores and
/// this predicate deliberately excludes them. Widening it to "any XMP string
/// that looks boolean" would re-introduce the defect it replaced from the other
/// side: rewriting values ExifTool passes through untouched.
///
/// # Arguments
///
/// * `tag_name` - The full tag name, used to confirm the XMP family
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if ExifTool applies `%boolConv` to this tag
pub fn is_xmp_bool_conv_tag(tag_name: &str, base_name: &str) -> bool {
    tag_name.starts_with("XMP")
        && matches!(
            base_name,
            "FlashFired" | "FlashFunction" | "FlashRedEyeMode"
        )
}

/// Checks if the tag is `GPSVersionID`.
///
/// Unlike the ASCII-digit version tags (`ExifVersion`, `InteropVersion`,
/// `FlashpixVersion`), `GPSVersionID`'s 4 raw bytes are small integers
/// (typically `[2, 2, 0, 0]`) that ExifTool prints by joining the decimal
/// byte values with dots, e.g. `"2.2.0.0"`.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded with [`format_gps_version_id`]
pub fn is_gps_version_id(base_name: &str) -> bool {
    base_name == "GPSVersionID"
}

/// Formats `GPSVersionID` raw bytes as dot-separated decimal values.
///
/// ExifTool prints the 4 raw bytes (e.g. `[2, 2, 0, 0]`) as `"2.2.0.0"`,
/// not as a concatenated ASCII digit string like the other EXIF version tags.
pub fn format_gps_version_id(data: &[u8]) -> String {
    data.iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// Checks if the tag is an APP14 flags tag (APP14Flags0, APP14Flags1).
///
/// These tags are used in JPEG APP14 (Adobe) segments to store processing flags.
/// ExifTool displays "(none)" when the value is 0, indicating no flags are set.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as an APP14 flags value
pub fn is_app14_flags_tag(base_name: &str) -> bool {
    matches!(base_name, "APP14Flags0" | "APP14Flags1")
}

/// Checks if the tag is ExposureProgram.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as an exposure program enum
pub fn is_exposure_program(base_name: &str) -> bool {
    base_name == "ExposureProgram"
}

/// Checks if the tag requires a unit suffix.
///
/// Tags that require unit suffixes:
/// - FocalLength, FocalLengthIn35mmFormat -> "mm"
/// - GPSAltitude, SubjectDistance -> "m"
///
/// Note: GPSAltitude is handled here for the unit suffix, not the reference value.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should have a unit suffix appended
pub fn is_unit_suffix_tag(base_name: &str) -> bool {
    matches!(
        base_name,
        "FocalLength"
            | "FocalLengthIn35mmFormat"
            | "FocalLength35efl"
            | "FocalLengthIn35mmFilm"
            | "GPSAltitude"
            | "SubjectDistance"
            | "HyperfocalDistance"
    )
}

/// Checks if the tag represents a percentage value that needs "%" suffix.
///
/// Percentage tags include:
/// - Quality (from Ducky segment in JPEG files) - image quality setting
/// - MeasurementFlare (from ICC_Profile) - flare measurement percentage
///
/// These tags store numeric values representing percentages, and ExifTool
/// displays them with a "%" suffix for clarity (e.g., "84" becomes "84%").
///
/// Note: MeasurementFlare is also handled by the ICC matrix formatting rule,
/// but is included here for consistency and to handle integer values.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should have a "%" suffix appended to numeric values
pub fn is_percentage_tag(base_name: &str) -> bool {
    // Quality from Ducky segment and MeasurementFlare from ICC_Profile need % suffix.
    // Note: MeasurementFlare strings are handled by ICC matrix rule first,
    // but integers fall through to this rule for the % suffix.
    matches!(base_name, "Quality" | "MeasurementFlare")
}

/// Checks if the tag is UserComment.
///
/// UserComment (tag 0x9286) stores text with an 8-byte encoding prefix
/// (ASCII, UNICODE, JIS) followed by the actual text content. This needs
/// special decoding to extract the human-readable text.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as UserComment
pub fn is_user_comment(base_name: &str) -> bool {
    base_name == "UserComment"
}

/// Checks if a tag name refers to a thumbnail image.
///
/// ThumbnailImage tags should be formatted with the ExifTool-compatible
/// message "(Binary data X bytes, use -b option to extract)" instead of
/// just showing the raw binary data.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a thumbnail image
pub fn is_thumbnail_image(base_name: &str) -> bool {
    base_name == "ThumbnailImage"
}

// =============================================================================
// HELPER FUNCTIONS - Value Formatting
// =============================================================================

/// Formats space-separated float values in an ICC profile string with 5 decimal precision.
///
/// ICC profile matrix tags often contain multiple space-separated float values
/// (e.g., "0.1491851806640625 0.0632171630859375 0.74456787109375"). This function
/// parses each value, formats it with up to 5 decimal places, and reassembles
/// the string with spaces.
///
/// For MeasurementFlare, a "%" suffix is appended to the formatted result.
///
/// # Arguments
///
/// * `value` - The string containing space-separated float values
/// * `base_name` - The base tag name (used to detect MeasurementFlare for % suffix)
///
/// # Returns
///
/// A formatted string with each float value limited to 5 decimal places.
/// If parsing fails for any value, the original token is preserved.
///
/// # Examples
///
/// ```rust,ignore
/// // Matrix column with 3 values
/// let result = format_icc_string_values("0.1491851806640625 0.0632171630859375 0.74456787109375", "BlueMatrixColumn");
/// assert_eq!(result, "0.14919 0.06322 0.74457");
///
/// // MeasurementFlare with % suffix
/// let result = format_icc_string_values("0.01", "MeasurementFlare");
/// assert_eq!(result, "0.01%");
/// ```
fn format_icc_string_values(value: &str, base_name: &str) -> String {
    // Split the string by whitespace and format each numeric value
    let formatted_parts: Vec<String> = value
        .split_whitespace()
        .map(|part| {
            // Try to parse as f64 and format with 5 decimal precision
            if let Ok(f) = part.parse::<f64>() {
                format_icc_value(f)
            } else {
                // If parsing fails, keep the original value
                part.to_string()
            }
        })
        .collect();

    let result = formatted_parts.join(" ");

    // Add "%" suffix for MeasurementFlare
    if base_name == "MeasurementFlare" {
        format!("{}%", result)
    } else {
        result
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// The dispatch, not just the table.
    ///
    /// `format_file_source` has existed for as long as this arm has, and the
    /// arm has always been correct -- it simply never ran, because
    /// `raw_bytes_to_tag_value` handed 0xa300 over as `TagValue::Binary` and
    /// `as_integer()` is `None` for a blob. 2,836 corpus files printed
    /// `(Binary data 1 bytes, use -b option to extract)` past a working
    /// decoder. This asserts both shapes reach a label.
    #[test]
    fn file_source_reaches_the_print_conv_from_the_binary_form() {
        // The shape the TIFF reader actually produces for `Writable => 'undef'`.
        // This is the assertion that fails against the old code: the integer
        // cases below passed before this change and prove nothing on their own.
        for (byte, label) in [
            (1u8, "Film Scanner"),
            (2, "Reflection Print Scanner"),
            (3, "Digital Camera"),
        ] {
            assert_eq!(
                format_tag_value("ExifIFD:FileSource", &TagValue::Binary(vec![byte])),
                TagValue::String(label.to_string()),
                "FileSource {byte} did not reach the PrintConv from its binary form"
            );
        }
        // Sigma writes the same code with a count of 4, and that is a separate
        // PrintConv key -- a different label, not `Digital Camera`.
        assert_eq!(
            format_tag_value("ExifIFD:FileSource", &TagValue::Binary(vec![3, 0, 0, 0])),
            TagValue::String("Sigma Digital Camera".to_string())
        );
        // One byte the hash does not name prints its number, the way ExifTool
        // does for the four corpus files that store a zero here.
        assert_eq!(
            format_tag_value("ExifIFD:FileSource", &TagValue::Binary(vec![0])),
            TagValue::String("Unknown (0)".to_string())
        );
        // A longer unnamed blob is left alone rather than given a label.
        let blob = TagValue::Binary(vec![9, 9, 9, 9]);
        assert_eq!(format_tag_value("ExifIFD:FileSource", &blob), blob);
        // The pre-existing integer path is unchanged.
        assert_eq!(
            format_tag_value("ExifIFD:FileSource", &TagValue::new_integer(3)),
            TagValue::String("Digital Camera".to_string())
        );
    }

    /// `Exif.pm:2453-2463` maps the one-letter stored classification codes;
    /// without this dispatch they escape unchanged as their raw ASCII values.
    #[test]
    fn security_classification_string_values_reach_the_print_conv() {
        for (raw, expected) in [
            ("T", "Top Secret"),
            ("S", "Secret"),
            ("C", "Confidential"),
            ("R", "Restricted"),
            ("U", "Unclassified"),
        ] {
            assert_eq!(
                format_tag_value(
                    "ExifIFD:SecurityClassification",
                    &TagValue::String(raw.to_string()),
                ),
                TagValue::String(expected.to_string()),
                "SecurityClassification {raw} did not reach Exif.pm's PrintConv"
            );
        }

        // The PrintConv hash has no fallback label: ExifTool leaves unknown
        // stored strings as-is rather than inventing a classification.
        let unknown = TagValue::String("X".to_string());
        assert_eq!(
            format_tag_value("ExifIFD:SecurityClassification", &unknown),
            unknown
        );
    }

    /// The dispatch, not just the table.
    ///
    /// `format_focal_plane_resolution_unit` existing proves nothing on its own:
    /// the table already existed in `parsers::pdf` while this chain had no
    /// `FocalPlaneResolutionUnit` arm, so `format_tag_value` returned the raw
    /// integer and 1,098 sample-corpus files reported `2` instead of `inches`.
    /// This asserts the wiring.
    #[test]
    fn focal_plane_resolution_unit_reaches_the_print_conv() {
        for (code, label) in [
            (1i64, "None"),
            (2, "inches"),
            (3, "cm"),
            (4, "mm"),
            (5, "um"),
        ] {
            let got = format_tag_value(
                "ExifIFD:FocalPlaneResolutionUnit",
                &TagValue::new_integer(code),
            );
            assert_eq!(
                got,
                TagValue::String(label.to_string()),
                "FocalPlaneResolutionUnit {code} did not reach the PrintConv"
            );
        }
    }

    /// Exif.pm 13.59 tag 0x0122's exact PrintConv must be reached from the
    /// ordinary TIFF/EXIF formatting path. A formatter that omits this dispatch
    /// silently reports the stored enum code instead of its density increment.
    #[test]
    fn gray_response_unit_reaches_the_print_conv() {
        for (code, expected) in [
            (1i64, "0.1"),
            (2, "0.001"),
            (3, "0.0001"),
            (4, "0.00001"),
            (5, "0.000001"),
            (6, "Unknown (6)"),
        ] {
            assert_eq!(
                format_tag_value("IFD0:GrayResponseUnit", &TagValue::new_integer(code)),
                TagValue::String(expected.to_string()),
                "GrayResponseUnit {code} did not reach Exif.pm's PrintConv"
            );
        }
    }

    /// ExifTool 13.59 `Exif.pm` tag 0x010a maps the stored SHORT values to
    /// `Normal` and `Reversed`. The TIFF enum table already knew these labels,
    /// but this central compatibility path did not send FillOrder through it.
    #[test]
    fn fill_order_reaches_the_print_conv() {
        for (code, expected) in [(1i64, "Normal"), (2, "Reversed")] {
            assert_eq!(
                format_tag_value("IFD0:FillOrder", &TagValue::new_integer(code)),
                TagValue::String(expected.to_string()),
                "FillOrder {code} did not reach Exif.pm's PrintConv"
            );
        }

        let unknown = TagValue::new_integer(3);
        assert_eq!(format_tag_value("IFD0:FillOrder", &unknown), unknown);
    }

    /// Flash reaches the hash, and unnamed codes come back in hex.
    #[test]
    fn flash_reaches_the_print_conv_with_the_printhex_unknown_form() {
        assert_eq!(
            format_tag_value("ExifIFD:Flash", &TagValue::new_integer(0x49)),
            TagValue::String("On, Red-eye reduction".to_string())
        );
        assert_eq!(
            format_tag_value("ExifIFD:Flash", &TagValue::new_integer(0x38)),
            TagValue::String("Unknown (0x38)".to_string())
        );
    }

    /// The dispatch, not just the formatter.
    ///
    /// `print_f_number` being right proves nothing on its own: 0x829d had no
    /// arm in this chain at all, so its rational reached Rule 20 and printed
    /// the `%.10g` quotient. 1,035 sample-corpus files reported `4` where
    /// ExifTool reports `4.0`, and `0.640234375` where it reports `0.64`.
    #[test]
    fn fnumber_reaches_print_fnumber_from_its_rational() {
        let rational = |n: i32, d: i32| TagValue::Rational {
            numerator: n,
            denominator: d,
        };
        // The whole f-stops, which Rule 20 printed without a decimal place.
        for (n, d, want) in [(4, 1, "4.0"), (8, 1, "8.0"), (2, 1, "2.0"), (11, 1, "11.0")] {
            assert_eq!(
                format_tag_value("ExifIFD:FNumber", &rational(n, d)),
                TagValue::String(want.to_string()),
                "FNumber {n}/{d} did not reach PrintFNumber"
            );
        }
        // The rounding cases, which Rule 20 printed in full.
        // FujiFilmFinePixA345.jpg stores 344/100.
        assert_eq!(
            format_tag_value("ExifIFD:FNumber", &rational(344, 100)),
            TagValue::String("3.4".to_string())
        );
        // GPS.jpg stores 3277/5119 -- below 1.0, so two decimal places.
        assert_eq!(
            format_tag_value("ExifIFD:FNumber", &rational(3277, 5119)),
            TagValue::String("0.64".to_string())
        );
        // A stored zero stays `0`; ExifTool never prints `0.0` for this tag.
        assert_eq!(
            format_tag_value("ExifIFD:FNumber", &rational(0, 10)),
            TagValue::String("0".to_string())
        );
        // A zero denominator is Rule 17's, and still is.
        assert_eq!(
            format_tag_value("ExifIFD:FNumber", &rational(4, 0)),
            TagValue::String("undef".to_string())
        );
    }

    /// The Composite input is the raw quotient, not this display string.
    ///
    /// Exif.pm:4782's Composite `Aperture` is `ValueConv => '$val[0] || $val[1]'`
    /// over `Desire => { 0 => 'FNumber', 1 => 'ApertureValue' }`, and applies
    /// `PrintFNumber` itself. 0x829d has no ValueConv, so what the composite
    /// must see is the unrounded rational -- which is why this conversion
    /// belongs here, in the display layer, and not in the value the map holds.
    /// `apex_value_conv` is the list of tags whose *stored* value is not what
    /// a reader wants, and FNumber is deliberately not one of them.
    #[test]
    fn fnumber_has_no_value_conv_so_composites_keep_the_raw_quotient() {
        let stored = TagValue::Rational {
            numerator: 3277,
            denominator: 5119,
        };
        assert_eq!(apex_value_conv("FNumber", &stored), None);
        // ...unlike the APEX-stored aperture tags beside it.
        assert!(apex_value_conv("ApertureValue", &stored).is_some());
        assert!(apex_value_conv("MaxApertureValue", &stored).is_some());
    }

    // -------------------------------------------------------------------------
    #[test]
    fn ambient_temperature_print_conv_is_scoped_to_exif() {
        let negative_zero = TagValue::Rational {
            numerator: 0,
            denominator: -1,
        };
        assert_eq!(
            format_tag_value("ExifIFD:AmbientTemperature", &negative_zero),
            TagValue::String("-0 C".to_string())
        );

        let dji_value = TagValue::Rational {
            numerator: 21,
            denominator: 2,
        };
        assert_eq!(
            format_tag_value("DJI:AmbientTemperature", &dji_value),
            TagValue::String("10.5".to_string())
        );
    }

    // strip_family_prefix tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_strip_family_prefix_with_prefix() {
        assert_eq!(strip_family_prefix("EXIF:Make"), "Make");
        assert_eq!(strip_family_prefix("GPS:GPSLatitude"), "GPSLatitude");
        assert_eq!(strip_family_prefix("XMP:Creator"), "Creator");
        assert_eq!(strip_family_prefix("IPTC:Keywords"), "Keywords");
    }

    #[test]
    fn test_strip_family_prefix_without_prefix() {
        assert_eq!(strip_family_prefix("Make"), "Make");
        assert_eq!(strip_family_prefix("FocalLength"), "FocalLength");
    }

    #[test]
    fn test_strip_family_prefix_nested() {
        // Should take the last segment after the final colon
        assert_eq!(strip_family_prefix("Composite:EXIF:Tag"), "Tag");
    }

    #[test]
    fn test_strip_family_prefix_empty() {
        assert_eq!(strip_family_prefix(""), "");
    }

    // -------------------------------------------------------------------------
    // GPS Latitude/Longitude Reference tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_lat_ref_formatting() {
        let value = TagValue::String("N".to_string());
        let formatted = format_tag_value("GPS:GPSLatitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("North"));

        let value = TagValue::String("S".to_string());
        let formatted = format_tag_value("GPSLatitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("South"));
    }

    #[test]
    fn test_gps_lon_ref_formatting() {
        let value = TagValue::String("E".to_string());
        let formatted = format_tag_value("GPS:GPSLongitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("East"));

        let value = TagValue::String("W".to_string());
        let formatted = format_tag_value("GPSLongitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("West"));
    }

    // -------------------------------------------------------------------------
    // GPS Direction Reference tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_direction_ref_formatting() {
        let value = TagValue::String("T".to_string());
        let formatted = format_tag_value("GPS:GPSImgDirectionRef", &value);
        assert_eq!(formatted.as_string(), Some("True North"));

        let value = TagValue::String("M".to_string());
        let formatted = format_tag_value("GPSTrackRef", &value);
        assert_eq!(formatted.as_string(), Some("Magnetic North"));

        let value = TagValue::String("T".to_string());
        let formatted = format_tag_value("GPSDestBearingRef", &value);
        assert_eq!(formatted.as_string(), Some("True North"));
    }

    // -------------------------------------------------------------------------
    // GPS Speed/Distance Reference tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_speed_ref_formatting() {
        let value = TagValue::String("K".to_string());
        let formatted = format_tag_value("GPS:GPSSpeedRef", &value);
        assert_eq!(formatted.as_string(), Some("km/h"));

        let value = TagValue::String("M".to_string());
        let formatted = format_tag_value("GPSSpeedRef", &value);
        assert_eq!(formatted.as_string(), Some("mph"));

        let value = TagValue::String("N".to_string());
        let formatted = format_tag_value("GPSSpeedRef", &value);
        assert_eq!(formatted.as_string(), Some("knots"));
    }

    #[test]
    fn test_gps_dest_distance_ref_formatting() {
        let value = TagValue::String("K".to_string());
        let formatted = format_tag_value("GPS:GPSDestDistanceRef", &value);
        assert_eq!(formatted.as_string(), Some("Kilometers"));

        let value = TagValue::String("M".to_string());
        let formatted = format_tag_value("GPSDestDistanceRef", &value);
        assert_eq!(formatted.as_string(), Some("Miles"));

        let value = TagValue::String("N".to_string());
        let formatted = format_tag_value("GPSDestDistanceRef", &value);
        assert_eq!(formatted.as_string(), Some("Nautical Miles"));
    }

    // -------------------------------------------------------------------------
    // GPS Status Tags tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_status_formatting() {
        let value = TagValue::String("A".to_string());
        let formatted = format_tag_value("GPS:GPSStatus", &value);
        assert_eq!(formatted.as_string(), Some("Measurement Active"));

        let value = TagValue::String("V".to_string());
        let formatted = format_tag_value("GPSStatus", &value);
        assert_eq!(formatted.as_string(), Some("Measurement Void"));
    }

    #[test]
    fn test_gps_measure_mode_formatting() {
        let value = TagValue::String("2".to_string());
        let formatted = format_tag_value("GPS:GPSMeasureMode", &value);
        assert_eq!(formatted.as_string(), Some("2-Dimensional Measurement"));

        let value = TagValue::String("3".to_string());
        let formatted = format_tag_value("GPSMeasureMode", &value);
        assert_eq!(formatted.as_string(), Some("3-Dimensional Measurement"));
    }

    #[test]
    fn test_gps_differential_formatting() {
        let value = TagValue::String("0".to_string());
        let formatted = format_tag_value("GPS:GPSDifferential", &value);
        assert_eq!(formatted.as_string(), Some("No Correction"));

        let value = TagValue::String("1".to_string());
        let formatted = format_tag_value("GPSDifferential", &value);
        assert_eq!(formatted.as_string(), Some("Differential Corrected"));
    }

    #[test]
    fn test_gps_differential_from_integer() {
        // Test integer 0 -> "No Correction"
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("GPS:GPSDifferential", &value);
        assert_eq!(formatted.as_string(), Some("No Correction"));

        // Test integer 1 -> "Differential Corrected"
        let value = TagValue::Integer(1);
        let formatted = format_tag_value("GPSDifferential", &value);
        assert_eq!(formatted.as_string(), Some("Differential Corrected"));

        // Test unknown integer value - should pass through unchanged
        let value = TagValue::Integer(2);
        let formatted = format_tag_value("GPSDifferential", &value);
        assert_eq!(formatted.as_integer(), Some(2));
    }

    // -------------------------------------------------------------------------
    // GPS Altitude Reference tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_altitude_ref_from_string() {
        let value = TagValue::String("0".to_string());
        let formatted = format_tag_value("GPS:GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Above Sea Level"));

        let value = TagValue::String("1".to_string());
        let formatted = format_tag_value("GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Below Sea Level"));
    }

    #[test]
    fn test_gps_altitude_ref_from_binary() {
        let value = TagValue::Binary(vec![0]);
        let formatted = format_tag_value("GPS:GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Above Sea Level"));

        let value = TagValue::Binary(vec![1]);
        let formatted = format_tag_value("GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Below Sea Level"));
    }

    #[test]
    fn test_gps_altitude_ref_from_integer() {
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("GPS:GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Above Sea Level"));

        let value = TagValue::Integer(1);
        let formatted = format_tag_value("GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Below Sea Level"));
    }

    // -------------------------------------------------------------------------
    // GPS Processing Method tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_processing_method_formatting() {
        // ASCII-encoded "GPS" method
        let data = b"ASCII\0\0\0GPS\0\0\0\0\0".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("GPS:GPSProcessingMethod", &value);
        assert_eq!(formatted.as_string(), Some("GPS"));
    }

    #[test]
    fn test_gps_area_information_formatting() {
        let value = TagValue::Binary(b"ASCII\0\0\0San Francisco".to_vec());
        let formatted = format_tag_value("GPS:GPSAreaInformation", &value);
        assert_eq!(formatted.as_string(), Some("San Francisco"));
    }

    // -------------------------------------------------------------------------
    // Binary Decoder tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_cfa_pattern_formatting() {
        // 2x2 RGGB Bayer pattern
        let data = vec![0, 2, 0, 2, 0, 1, 1, 2];
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:CFAPattern", &value);
        assert_eq!(formatted.as_string(), Some("[Red,Green][Green,Blue]"));
    }

    #[test]
    fn test_scene_type_formatting() {
        // Binary value 1 = "Directly photographed"
        let value = TagValue::Binary(vec![1]);
        let formatted = format_tag_value("EXIF:SceneType", &value);
        assert_eq!(formatted.as_string(), Some("Directly photographed"));
    }

    #[test]
    fn test_scene_type_from_integer() {
        let value = TagValue::Integer(1);
        let formatted = format_tag_value("EXIF:SceneType", &value);
        assert_eq!(formatted.as_string(), Some("Directly photographed"));

        let value = TagValue::Integer(5);
        let formatted = format_tag_value("SceneType", &value);
        assert_eq!(formatted.as_string(), Some("Unknown (5)"));
    }

    #[test]
    fn test_version_bytes_formatting() {
        let data = b"0100".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:InteropVersion", &value);
        assert_eq!(formatted.as_string(), Some("0100"));

        let data = b"0232".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("ExifVersion", &value);
        assert_eq!(formatted.as_string(), Some("0232"));

        let data = b"0100".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("FlashpixVersion", &value);
        assert_eq!(formatted.as_string(), Some("0100"));

        // GPSVersionID is NOT decoded like the ASCII-digit version tags above:
        // its 4 raw bytes are small integers joined with dots (e.g. "2.2.0.0"),
        // not a concatenated ASCII digit string.
        let data = vec![2u8, 2, 0, 0];
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("GPSVersionID", &value);
        assert_eq!(formatted.as_string(), Some("2.2.0.0"));
    }

    // -------------------------------------------------------------------------
    // APP14 Flags tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_app14_flags_zero_returns_none() {
        // APP14Flags0 with value 0 should return "(none)"
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("JPEG:APP14Flags0", &value);
        assert_eq!(formatted.as_string(), Some("(none)"));

        // APP14Flags1 with value 0 should return "(none)"
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("APP14Flags1", &value);
        assert_eq!(formatted.as_string(), Some("(none)"));
    }

    #[test]
    fn test_app14_flags_nonzero_passes_through() {
        // Non-zero APP14Flags0 should pass through unchanged
        let value = TagValue::Integer(1);
        let formatted = format_tag_value("JPEG:APP14Flags0", &value);
        assert_eq!(formatted.as_integer(), Some(1));

        // Non-zero APP14Flags1 should pass through unchanged
        let value = TagValue::Integer(42);
        let formatted = format_tag_value("APP14Flags1", &value);
        assert_eq!(formatted.as_integer(), Some(42));
    }

    #[test]
    fn test_is_app14_flags_tag() {
        assert!(is_app14_flags_tag("APP14Flags0"));
        assert!(is_app14_flags_tag("APP14Flags1"));
        assert!(!is_app14_flags_tag("APP14Flags2"));
        assert!(!is_app14_flags_tag("APP14"));
        assert!(!is_app14_flags_tag("Flags0"));
    }

    // -------------------------------------------------------------------------
    // Enum Tag tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_exposure_program_formatting() {
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("EXIF:ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Not Defined"));

        let value = TagValue::Integer(1);
        let formatted = format_tag_value("ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Manual"));

        let value = TagValue::Integer(2);
        let formatted = format_tag_value("ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Program AE"));

        let value = TagValue::Integer(3);
        let formatted = format_tag_value("ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Aperture-priority AE"));

        let value = TagValue::Integer(99);
        let formatted = format_tag_value("ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Unknown (99)"));
    }

    /// The tiff_enum_to_string-backed arm: enums no parser decodes at read
    /// time. Values quoted from `exiftool -json -G` 13.59 over the sample
    /// corpus (e.g. OlympusSH-1.jpg prints SensitivityType
    /// "Standard Output Sensitivity" where oxidex printed 1).
    #[test]
    fn test_undecoded_tiff_enums_render_via_shared_table() {
        let cases: &[(&str, i64, &str)] = &[
            ("EXIF:SensitivityType", 1, "Standard Output Sensitivity"),
            ("EXIF:CompositeImage", 2, "General Composite Image"),
            ("TIFF:PhotometricInterpretation", 2, "RGB"),
            ("SubIFD0:PlanarConfiguration", 1, "Chunky"),
            ("EXIF:SubfileType", 1, "Reduced-resolution image"),
            ("EXIF:Predictor", 2, "Horizontal differencing"),
            ("EXIF:MakerNoteSafety", 1, "Safe"),
        ];
        for (tag, raw, label) in cases {
            let formatted = format_tag_value(tag, &TagValue::Integer(*raw));
            assert_eq!(
                formatted.as_string(),
                Some(*label),
                "{tag} {raw} did not reach its PrintConv"
            );
        }

        // A value outside the PrintConv map keeps its integer form: ExifTool's
        // "Unknown (N)" fallback is unmodeled here (no carrier file pins it),
        // and a plausible-but-unverified rendering is worse than the raw int.
        let formatted = format_tag_value("EXIF:SensitivityType", &TagValue::Integer(42));
        assert_eq!(formatted, TagValue::Integer(42));
    }

    /// APP10 AROT gain curve: `Binary => 1` semantics over the rendered
    /// payload. 13.59 prints "(Binary data 1160 bytes, use -b option to
    /// extract)" for Apple_iPadAir_3rd_generation.jpg, and oxidex's
    /// space-separated decimal payload for that file is 1160 UTF-8 bytes.
    #[test]
    fn test_hdr_gain_curve_reports_payload_length() {
        let payload = "0 1024 2048 4096";
        let formatted = format_tag_value("APP10:HDRGainCurve", &TagValue::new_string(payload));
        assert_eq!(
            formatted.as_string(),
            Some("(Binary data 16 bytes, use -b option to extract)")
        );
    }

    /// `%boolConv` capitalizes, and applies to three tags.
    ///
    /// The version of this test that shipped with the inverted rule asserted
    /// `XMP:AlreadyApplied` of `"True"` should print `"true"`. It was wrong
    /// twice over: ExifTool's `%boolConv` folds *up*, and `AlreadyApplied`
    /// (XMP.pm:1416) is a bare `Writable => 'boolean'` with no `PrintConv` at
    /// all, so ExifTool reports the file's own literal and converts nothing.
    /// Picking two pass-through tags to demonstrate a conversion is what let
    /// the rule read as tested while it corrupted 10 corpus values.
    #[test]
    fn xmp_bool_conv_capitalizes_its_three_tags() {
        for tag in ["XMP:FlashFired", "XMP:FlashFunction", "XMP:FlashRedEyeMode"] {
            // The form files actually store -- passed through, not folded down.
            let value = TagValue::String("False".to_string());
            assert_eq!(
                format_tag_value(tag, &value).as_string(),
                Some("False"),
                "{tag}"
            );

            // XMP.pm:3690 calls the lowercase form malformed input; `%boolConv`
            // repairs it upward rather than adopting it.
            let value = TagValue::String("true".to_string());
            assert_eq!(
                format_tag_value(tag, &value).as_string(),
                Some("True"),
                "{tag}"
            );
        }
    }

    /// Boolean-looking XMP tags without `%boolConv` keep the stored literal.
    ///
    /// `Marked` is the one the corpus caught: PLUS.xmp stores
    /// `<xmpRights:Marked>True`, ExifTool prints `True`, and the blanket rule
    /// rewrote it to `true` on 4 files.
    #[test]
    fn xmp_booleans_without_bool_conv_are_passed_through() {
        for (tag, stored) in [
            ("XMP-xmpRights:Marked", "True"),
            ("XMP:AlreadyApplied", "True"),
            ("XMP-crs:HasCrop", "False"),
            // A free-text tag that merely happens to read as a boolean.
            ("XMP-dc:Description", "True"),
        ] {
            let value = TagValue::String(stored.to_string());
            assert_eq!(
                format_tag_value(tag, &value).as_string(),
                Some(stored),
                "{tag}"
            );
        }

        // Non-boolean XMP values should be unchanged
        let value = TagValue::String("Normal".to_string());
        let formatted = format_tag_value("XMP:ProcessVersion", &value);
        assert_eq!(formatted.as_string(), Some("Normal"));
    }

    /// `%boolConv` is XMP-only; an EXIF tag of the same base name is untouched.
    #[test]
    fn bool_conv_does_not_reach_outside_xmp() {
        assert!(is_xmp_bool_conv_tag("XMP:FlashFired", "FlashFired"));
        assert!(!is_xmp_bool_conv_tag("EXIF:FlashFired", "FlashFired"));
        assert!(!is_xmp_bool_conv_tag("XMP-xmpRights:Marked", "Marked"));
    }

    #[test]
    fn test_xmp_lens_info_formatting() {
        // Zoom lens with constant aperture: 45-100mm f/4
        let value = TagValue::String("4500/100 10000/100 400/100 400/100".to_string());
        let formatted = format_tag_value("XMP:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("45-100mm f/4"));

        // Prime lens: 50mm f/1.8
        let value = TagValue::String("500/10 500/10 18/10 18/10".to_string());
        let formatted = format_tag_value("XMP:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("50mm f/1.8"));

        // Variable aperture zoom: 18-55mm f/3.5-5.6
        let value = TagValue::String("1800/100 5500/100 350/100 560/100".to_string());
        let formatted = format_tag_value("XMP:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("18-55mm f/3.5-5.6"));

        // Unknown aperture (0/0): 50mm f/?
        let value = TagValue::String("50/1 50/1 0/0 0/0".to_string());
        let formatted = format_tag_value("XMP:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("50mm f/?"));

        // Non-XMP LensInfo should not be formatted
        let value = TagValue::String("24/1 70/1 28/10 28/10".to_string());
        let formatted = format_tag_value("EXIF:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("24/1 70/1 28/10 28/10"));
    }

    // -------------------------------------------------------------------------
    // Unit Suffix tests
    // -------------------------------------------------------------------------

    /// EXIF 0x920a forces one decimal (Exif.pm:2401
    /// `sprintf("%.1f mm",$val)`); FocalLengthIn35mmFormat does not
    /// (Exif.pm:2842 `"$val mm"`).
    #[test]
    fn test_focal_length_unit_suffix() {
        let value = TagValue::String("50".to_string());
        let formatted = format_tag_value("EXIF:FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("50.0 mm"));

        let value = TagValue::String("31".to_string());
        let formatted = format_tag_value("FocalLengthIn35mmFormat", &value);
        assert_eq!(formatted.as_string(), Some("31 mm"));
    }

    #[test]
    fn test_focal_length_from_integer() {
        let value = TagValue::Integer(50);
        let formatted = format_tag_value("EXIF:FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("50.0 mm"));

        // A maker-note FocalLength keeps ExifTool's bare `"$val mm"`
        // (Canon.pm:3138).
        let formatted = format_tag_value("Canon:FocalLength", &TagValue::Integer(34));
        assert_eq!(formatted.as_string(), Some("34 mm"));
    }

    #[test]
    fn test_focal_length_from_float() {
        let value = TagValue::Float(50.0);
        let formatted = format_tag_value("EXIF:FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("50.0 mm"));

        let value = TagValue::Float(35.5);
        let formatted = format_tag_value("FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("35.5 mm"));

        // Minolta.mrw's 7.203125 prints as `7.2 mm` under `exiftool -G1 -s`
        let value = TagValue::Float(7.203125);
        let formatted = format_tag_value("ExifIFD:FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("7.2 mm"));
    }

    #[test]
    fn test_focal_length_from_rational() {
        let value = TagValue::Rational {
            numerator: 500,
            denominator: 10,
        };
        let formatted = format_tag_value("EXIF:FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("50.0 mm"));
    }

    #[test]
    fn test_gps_altitude_unit_suffix() {
        let value = TagValue::String("117".to_string());
        let formatted = format_tag_value("GPS:GPSAltitude", &value);
        assert_eq!(formatted.as_string(), Some("117 m"));
    }

    #[test]
    fn test_subject_distance_unit_suffix() {
        let value = TagValue::String("2.5".to_string());
        let formatted = format_tag_value("EXIF:SubjectDistance", &value);
        assert_eq!(formatted.as_string(), Some("2.5 m"));
    }

    #[test]
    fn subject_distance_preserves_exiftool_rational_sentinels() {
        let infinity = TagValue::Rational {
            numerator: 1,
            denominator: 0,
        };
        let undefined = TagValue::Rational {
            numerator: 0,
            denominator: 0,
        };

        assert_eq!(
            format_tag_value("EXIF:SubjectDistance", &infinity).as_string(),
            Some("inf")
        );
        assert_eq!(
            format_tag_value("EXIF:SubjectDistance", &undefined).as_string(),
            Some("undef")
        );
    }

    // -------------------------------------------------------------------------
    // Default behavior tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_unknown_tag_passes_through() {
        let value = TagValue::String("Canon".to_string());
        let formatted = format_tag_value("EXIF:Make", &value);
        assert_eq!(formatted.as_string(), Some("Canon"));

        let value = TagValue::Integer(400);
        let formatted = format_tag_value("ISO", &value);
        assert_eq!(formatted.as_integer(), Some(400));
    }

    // -------------------------------------------------------------------------
    // format_for_exiftool integration tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_for_exiftool_basic() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::String("Canon".to_string()));
        metadata.insert("EXIF:ExposureProgram", TagValue::Integer(2));
        metadata.insert("GPS:GPSLatitudeRef", TagValue::String("N".to_string()));
        metadata.insert("EXIF:FocalLength", TagValue::String("50".to_string()));

        let formatted = format_for_exiftool(&metadata);

        // Make should pass through unchanged
        assert_eq!(formatted.get_string("EXIF:Make"), Some("Canon"));
        // ExposureProgram should be formatted
        assert_eq!(
            formatted.get_string("EXIF:ExposureProgram"),
            Some("Program AE")
        );
        // GPSLatitudeRef should be formatted
        assert_eq!(formatted.get_string("GPS:GPSLatitudeRef"), Some("North"));
        // FocalLength should have unit suffix
        assert_eq!(formatted.get_string("EXIF:FocalLength"), Some("50.0 mm"));
    }

    #[test]
    fn test_format_for_exiftool_preserves_count() {
        let mut metadata = MetadataMap::new();
        metadata.insert("Tag1", TagValue::String("Value1".to_string()));
        metadata.insert("Tag2", TagValue::Integer(42));
        metadata.insert("Tag3", TagValue::Float(3.14));

        let formatted = format_for_exiftool(&metadata);

        assert_eq!(formatted.len(), 3);
    }

    #[test]
    fn test_format_for_exiftool_empty() {
        let metadata = MetadataMap::new();
        let formatted = format_for_exiftool(&metadata);
        assert!(formatted.is_empty());
    }

    // -------------------------------------------------------------------------
    // Percentage Tag tests (Quality, MeasurementFlare)
    // -------------------------------------------------------------------------

    #[test]
    fn test_quality_percentage_from_integer() {
        // Ducky:Quality with integer value should have "%" suffix
        let value = TagValue::Integer(84);
        let formatted = format_tag_value("Ducky:Quality", &value);
        assert_eq!(formatted.as_string(), Some("84%"));

        // Without family prefix
        let value = TagValue::Integer(100);
        let formatted = format_tag_value("Quality", &value);
        assert_eq!(formatted.as_string(), Some("100%"));

        // Zero value
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("Quality", &value);
        assert_eq!(formatted.as_string(), Some("0%"));
    }

    #[test]
    fn test_quality_percentage_from_float() {
        // Quality with float value should have "%" suffix
        let value = TagValue::Float(84.0);
        let formatted = format_tag_value("Ducky:Quality", &value);
        assert_eq!(formatted.as_string(), Some("84%"));

        // Fractional float value
        let value = TagValue::Float(75.5);
        let formatted = format_tag_value("Quality", &value);
        assert_eq!(formatted.as_string(), Some("75.5%"));
    }

    #[test]
    fn test_measurement_flare_percentage_from_integer() {
        // MeasurementFlare with integer value should have "%" suffix
        let value = TagValue::Integer(1);
        let formatted = format_tag_value("ICC_Profile:MeasurementFlare", &value);
        assert_eq!(formatted.as_string(), Some("1%"));
    }

    #[test]
    fn test_is_percentage_tag() {
        assert!(is_percentage_tag("Quality"));
        // MeasurementFlare is included for integer values (floats handled by ICC matrix rule)
        assert!(is_percentage_tag("MeasurementFlare"));
        assert!(!is_percentage_tag("FocalLength"));
        assert!(!is_percentage_tag("ISO"));
        assert!(!is_percentage_tag("Make"));
    }

    // -------------------------------------------------------------------------
    // Helper function classification tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_is_gps_lat_lon_ref() {
        assert!(is_gps_lat_ref("GPSLatitudeRef"));
        assert!(is_gps_lon_ref("GPSLongitudeRef"));
        assert!(!is_gps_lat_ref("GPSLongitudeRef"));
        assert!(!is_gps_lon_ref("GPSLatitudeRef"));
    }

    #[test]
    fn test_is_gps_direction_ref() {
        assert!(is_gps_direction_ref("GPSImgDirectionRef"));
        assert!(is_gps_direction_ref("GPSDestBearingRef"));
        assert!(is_gps_direction_ref("GPSTrackRef"));
        assert!(!is_gps_direction_ref("GPSLatitudeRef"));
        assert!(!is_gps_direction_ref("GPSSpeedRef"));
    }

    #[test]
    fn test_is_gps_status_tag() {
        assert!(is_gps_status_tag("GPSStatus"));
        assert!(is_gps_status_tag("GPSMeasureMode"));
        assert!(is_gps_status_tag("GPSDifferential"));
        assert!(!is_gps_status_tag("GPSAltitude"));
    }

    #[test]
    fn test_is_version_tag() {
        assert!(is_version_tag("InteropVersion"));
        assert!(is_version_tag("ExifVersion"));
        assert!(is_version_tag("FlashpixVersion"));
        // GPSVersionID uses a distinct dot-separated decimal format; see
        // `is_gps_version_id` / `format_gps_version_id`.
        assert!(!is_version_tag("GPSVersionID"));
        assert!(!is_version_tag("SomeOtherVersion"));
    }

    #[test]
    fn test_is_unit_suffix_tag() {
        assert!(is_unit_suffix_tag("FocalLength"));
        assert!(is_unit_suffix_tag("FocalLengthIn35mmFormat"));
        assert!(is_unit_suffix_tag("GPSAltitude"));
        assert!(is_unit_suffix_tag("SubjectDistance"));
        assert!(!is_unit_suffix_tag("ISO"));
        assert!(!is_unit_suffix_tag("Make"));
    }

    // -------------------------------------------------------------------------
    // Special Float Value tests (infinity, negative zero)
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_special_float_values_infinity() {
        // Positive infinity should return "undef"
        assert_eq!(
            format_special_float_values(f64::INFINITY),
            Some("undef".to_string())
        );

        // Negative infinity should also return "undef"
        assert_eq!(
            format_special_float_values(f64::NEG_INFINITY),
            Some("undef".to_string())
        );
    }

    #[test]
    fn test_format_special_float_values_negative_zero() {
        // Negative zero should return "0"
        assert_eq!(format_special_float_values(-0.0), Some("0".to_string()));
    }

    #[test]
    fn test_format_special_float_values_normal() {
        // Normal values should return None
        assert_eq!(format_special_float_values(0.0), None);
        assert_eq!(format_special_float_values(42.5), None);
        assert_eq!(format_special_float_values(-123.456), None);
        assert_eq!(format_special_float_values(f64::MIN), None);
        assert_eq!(format_special_float_values(f64::MAX), None);
    }

    #[test]
    fn test_infinity_float_formats_to_undef() {
        // Test that TagValue::Float with infinity formats to "undef"
        let value = TagValue::Float(f64::INFINITY);
        let formatted = format_tag_value("EXIF:GPSDestBearing", &value);
        assert_eq!(formatted.as_string(), Some("undef"));

        let value = TagValue::Float(f64::NEG_INFINITY);
        let formatted = format_tag_value("EXIF:GPSDestDistance", &value);
        assert_eq!(formatted.as_string(), Some("undef"));
    }

    #[test]
    fn test_negative_zero_float_formats_to_zero() {
        // Test that TagValue::Float with -0.0 formats to "0"
        let value = TagValue::Float(-0.0);
        let formatted = format_tag_value("EXIF:ExposureIndex", &value);
        assert_eq!(formatted.as_string(), Some("0"));
    }

    // -------------------------------------------------------------------------
    // ICC_Profile Matrix Tag tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_icc_profile_blue_matrix_column_precision() {
        // Test the exact case from the issue: too many decimal places
        // OxiDex was showing: 0.1491851806640625 0.0632171630859375 0.74456787109375
        // ExifTool shows:     0.14919 0.06322 0.74457
        let value =
            TagValue::String("0.1491851806640625 0.0632171630859375 0.74456787109375".to_string());
        let formatted = format_tag_value("ICC_Profile:BlueMatrixColumn", &value);
        assert_eq!(formatted.as_string(), Some("0.14919 0.06322 0.74457"));
    }

    #[test]
    fn test_icc_profile_red_matrix_column() {
        let value = TagValue::String("0.43604 0.22249 0.01392".to_string());
        let formatted = format_tag_value("ICC_Profile:RedMatrixColumn", &value);
        assert_eq!(formatted.as_string(), Some("0.43604 0.22249 0.01392"));
    }

    #[test]
    fn test_icc_profile_green_matrix_column() {
        // Values with trailing zeros should be trimmed
        let value = TagValue::String("0.38512 0.71690 0.09706".to_string());
        let formatted = format_tag_value("GreenMatrixColumn", &value);
        assert_eq!(formatted.as_string(), Some("0.38512 0.7169 0.09706"));
    }

    #[test]
    fn test_icc_profile_media_white_point() {
        let value = TagValue::String("0.95047 1 1.08883".to_string());
        let formatted = format_tag_value("ICC_Profile:MediaWhitePoint", &value);
        assert_eq!(formatted.as_string(), Some("0.95047 1 1.08883"));
    }

    #[test]
    fn test_icc_profile_luminance() {
        let value = TagValue::String("76.03647".to_string());
        let formatted = format_tag_value("ICC_Profile:Luminance", &value);
        assert_eq!(formatted.as_string(), Some("76.03647"));
    }

    #[test]
    fn test_icc_profile_connection_space_illuminant() {
        // Whole number 1.0 should be trimmed to "1"
        let value = TagValue::String("0.9642 1.0 0.82491".to_string());
        let formatted = format_tag_value("ConnectionSpaceIlluminant", &value);
        assert_eq!(formatted.as_string(), Some("0.9642 1 0.82491"));
    }

    #[test]
    fn test_icc_profile_viewing_cond_illuminant() {
        let value = TagValue::String("19.6445 20.3718 16.8089".to_string());
        let formatted = format_tag_value("ViewingCondIlluminant", &value);
        assert_eq!(formatted.as_string(), Some("19.6445 20.3718 16.8089"));
    }

    #[test]
    fn test_format_icc_string_values_helper() {
        // Test the helper function directly
        let result = format_icc_string_values(
            "0.1491851806640625 0.0632171630859375 0.74456787109375",
            "BlueMatrixColumn",
        );
        assert_eq!(result, "0.14919 0.06322 0.74457");

        // Test with non-numeric content (should preserve)
        let result = format_icc_string_values("abc 1.5 def", "SomeTag");
        assert_eq!(result, "abc 1.5 def");
    }

    #[test]
    fn test_icc_matrix_tag_recognition() {
        // Test that is_icc_matrix_tag correctly identifies ICC profile tags
        assert!(is_icc_matrix_tag("BlueMatrixColumn"));
        assert!(is_icc_matrix_tag("RedMatrixColumn"));
        assert!(is_icc_matrix_tag("GreenMatrixColumn"));
        assert!(is_icc_matrix_tag("MediaWhitePoint"));
        assert!(is_icc_matrix_tag("MeasurementFlare"));
        assert!(is_icc_matrix_tag("ICC_Profile:Luminance"));
        assert!(!is_icc_matrix_tag("FocalLength"));
        assert!(!is_icc_matrix_tag("GPSAltitude"));
    }

    // -------------------------------------------------------------------------
    // ReferenceBlackWhite Integer Precision tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_reference_black_white_integer_formatting() {
        // ReferenceBlackWhite should display integers without decimals
        // ExifTool: "0 255 128 255 128 255"
        // OxiDex before fix: "0.0000000000 255.0000000000 128.0000000000..."
        let value = TagValue::String(
            "0.0000000000 255.0000000000 128.0000000000 255.0000000000 128.0000000000 255.0000000000"
                .to_string(),
        );
        let formatted = format_tag_value("EXIF:ReferenceBlackWhite", &value);
        assert_eq!(formatted.as_string(), Some("0 255 128 255 128 255"));

        // Without family prefix
        let value = TagValue::String("0.0 128.0 255.0".to_string());
        let formatted = format_tag_value("ReferenceBlackWhite", &value);
        assert_eq!(formatted.as_string(), Some("0 128 255"));
    }

    #[test]
    fn test_reference_black_white_with_fractional_values() {
        // If any values are non-integer, preserve minimal decimals
        let value = TagValue::String("0.5 255.0 128.25".to_string());
        let formatted = format_tag_value("EXIF:ReferenceBlackWhite", &value);
        assert_eq!(formatted.as_string(), Some("0.5 255 128.25"));
    }

    // -------------------------------------------------------------------------
    // YCbCrCoefficients Three Decimal Precision tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_ycbcr_coefficients_three_decimal_formatting() {
        // YCbCrCoefficients should display with 3 decimal places
        // ExifTool: "0.299 0.587 0.114"
        // OxiDex before fix: "0.2990000000 0.5870000000 0.1140000000"
        let value = TagValue::String("0.2990000000 0.5870000000 0.1140000000".to_string());
        let formatted = format_tag_value("EXIF:YCbCrCoefficients", &value);
        assert_eq!(formatted.as_string(), Some("0.299 0.587 0.114"));

        // Without family prefix
        let formatted = format_tag_value("YCbCrCoefficients", &value);
        assert_eq!(formatted.as_string(), Some("0.299 0.587 0.114"));
    }

    #[test]
    fn test_ycbcr_coefficients_trimmed_zeros() {
        // Values with fewer decimals should still trim trailing zeros
        let value = TagValue::String("0.5 1.0 0.25".to_string());
        let formatted = format_tag_value("EXIF:YCbCrCoefficients", &value);
        assert_eq!(formatted.as_string(), Some("0.5 1 0.25"));
    }

    // -------------------------------------------------------------------------
    // UserComment Decoding tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_user_comment_ascii_decoding() {
        // UserComment with ASCII encoding should be decoded to text
        // ExifTool shows: "GCM_TAG"
        // OxiDex before fix showed: "[Binary data]"
        let data = b"ASCII\0\0\0GCM_TAG".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:UserComment", &value);
        assert_eq!(formatted.as_string(), Some("GCM_TAG"));
    }

    #[test]
    fn test_user_comment_unicode_decoding() {
        // UserComment with UNICODE encoding
        let mut data = b"UNICODE\0".to_vec();
        // "Hi" in UTF-16LE: H=0x48, i=0x69
        data.extend_from_slice(&[0x48, 0x00, 0x69, 0x00, 0x00, 0x00]);
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("UserComment", &value);
        assert_eq!(formatted.as_string(), Some("Hi"));
    }

    #[test]
    fn test_user_comment_empty_renders_empty() {
        // ConvertExifText returns the trimmed string even when it is empty, so
        // a prefix-only UserComment prints as "" -- never as the
        // "(Binary data N bytes...)" placeholder.
        let data = b"ASCII\0\0\0".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:UserComment", &value);
        assert_eq!(formatted.as_string(), Some(""));
    }

    #[test]
    fn test_user_comment_nul_id_space_padding_renders_empty() {
        // OlympusD450Z.jpg: 8 NUL identifier bytes + 117 spaces. The all-NUL
        // identifier selects ConvertExifText's ASCII branch and the trailing
        // blanks trim to nothing; ExifTool 13.59 prints an empty string.
        let mut data = vec![0u8; 8];
        data.extend(std::iter::repeat_n(b' ', 117));
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:UserComment", &value);
        assert_eq!(formatted.as_string(), Some(""));
    }

    #[test]
    fn test_is_user_comment() {
        assert!(is_user_comment("UserComment"));
        assert!(!is_user_comment("Comment"));
        assert!(!is_user_comment("ImageDescription"));
    }

    // -------------------------------------------------------------------------
    // ThumbnailImage Formatting tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_thumbnail_image_binary_formatting() {
        // ThumbnailImage should be formatted with ExifTool-compatible message
        // ExifTool shows: "(Binary data 5448 bytes, use -b option to extract)"
        let data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // Start of JPEG
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:ThumbnailImage", &value);
        assert_eq!(
            formatted.as_string(),
            Some("(Binary data 4 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn test_thumbnail_image_large_binary() {
        // Test with a larger thumbnail
        let data = vec![0u8; 5448]; // 5448 bytes like in the example
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("ThumbnailImage", &value);
        assert_eq!(
            formatted.as_string(),
            Some("(Binary data 5448 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn linearization_table_uses_exiftools_binary_placeholder() {
        let value = TagValue::String("1 2 3".to_string());
        let formatted = format_tag_value("EXIF:LinearizationTable", &value);
        assert_eq!(
            formatted.as_string(),
            Some("(Binary data 5 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn profile_tone_curve_keeps_short_payloads_and_hides_long_ones() {
        assert_eq!(
            format_tag_value("EXIF:ProfileToneCurve", &TagValue::new_string("0 1")).as_string(),
            Some("0 1")
        );
        let payload = "0 0.0625 0.125 0.1875 0.25 0.3125 0.375 0.4375 0.5 0.5625 0.625 0.6875 0.75 0.8125 0.875 0.9375 1";
        assert_eq!(payload.len(), 97);
        assert_eq!(
            format_tag_value("EXIF:ProfileToneCurve", &TagValue::new_string(payload)).as_string(),
            Some("(Binary data 97 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn test_is_thumbnail_image() {
        assert!(is_thumbnail_image("ThumbnailImage"));
        assert!(!is_thumbnail_image("PreviewImage"));
        assert!(!is_thumbnail_image("JpgFromRaw"));
    }

    #[test]
    fn test_other_image_binary_formatting() {
        // OtherImage (the 0x0201/0x0202 blob from a non-IFD1 directory, e.g.
        // an image-carrying InteropIFD) gets the same ExifTool-compatible
        // binary placeholder as ThumbnailImage.
        let value = TagValue::Binary(vec![0u8; 5146]);
        let formatted = format_tag_value("InteropIFD:OtherImage", &value);
        assert_eq!(
            formatted.as_string(),
            Some("(Binary data 5146 bytes, use -b option to extract)")
        );
    }
}
