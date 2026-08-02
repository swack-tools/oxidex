//! EXIF enum value formatters for ExifTool-compatible output
//!
//! This module contains formatters for common EXIF enum tags that convert
//! integer values to human-readable strings matching ExifTool's output.

/// Formats ColorSpace (EXIF tag 0xA001).
///
/// ExifTool's `%Exif::Main{0xa001}` carries `PrintHex => 1`, so a value the
/// table does not name prints its code in hex -- `Unknown (0x0)`, not
/// `Unknown (0)`. Thirteen files in the sample corpus report exactly that.
pub fn format_color_space(value: i64) -> String {
    match value {
        1 => "sRGB".to_string(),
        2 => "Adobe RGB".to_string(),
        65533 => "Wide Gamut RGB".to_string(),
        65534 => "ICC Profile".to_string(),
        65535 => "Uncalibrated".to_string(),
        _ => format!("Unknown (0x{:x})", value),
    }
}

/// Format MeteringMode enum value
/// EXIF tag 0x9207
pub fn format_metering_mode(value: i64) -> String {
    match value {
        0 => "Unknown".to_string(),
        1 => "Average".to_string(),
        2 => "Center-weighted average".to_string(),
        3 => "Spot".to_string(),
        4 => "Multi-spot".to_string(),
        5 => "Multi-segment".to_string(),
        6 => "Partial".to_string(),
        255 => "Other".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format LightSource enum value
/// EXIF tag 0x9208
pub fn format_light_source(value: i64) -> String {
    match value {
        0 => "Unknown".to_string(),
        1 => "Daylight".to_string(),
        2 => "Fluorescent".to_string(),
        3 => "Tungsten (Incandescent)".to_string(),
        4 => "Flash".to_string(),
        9 => "Fine Weather".to_string(),
        10 => "Cloudy".to_string(),
        11 => "Shade".to_string(),
        12 => "Daylight Fluorescent".to_string(),
        13 => "Day White Fluorescent".to_string(),
        14 => "Cool White Fluorescent".to_string(),
        15 => "White Fluorescent".to_string(),
        16 => "Warm White Fluorescent".to_string(),
        17 => "Standard Light A".to_string(),
        18 => "Standard Light B".to_string(),
        19 => "Standard Light C".to_string(),
        20 => "D55".to_string(),
        21 => "D65".to_string(),
        22 => "D75".to_string(),
        23 => "D50".to_string(),
        24 => "ISO Studio Tungsten".to_string(),
        255 => "Other".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format Flash enum value (complex bitfield)
/// EXIF tag 0x9209
///
/// Delegates to [`crate::core::exif_enums::decode_flash`], which correctly
/// orders the flash mode ("On"/"Off"/"Auto") before the fired status (e.g.
/// "Off, Did not fire") to match ExifTool's output. A previous, independent
/// implementation here produced the wrong word order (e.g. "No Flash, Off"
/// instead of "Off, Did not fire") for compulsory-suppression mode.
pub fn format_flash(value: i64) -> String {
    crate::core::exif_enums::decode_flash(value.max(0) as u32)
}

/// Format ExposureMode enum value
/// EXIF tag 0xA402
pub fn format_exposure_mode(value: i64) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Manual".to_string(),
        2 => "Auto bracket".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format WhiteBalance enum value
/// EXIF tag 0xA403
pub fn format_white_balance(value: i64) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Manual".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format SceneCaptureType enum value
/// EXIF tag 0xA406
pub fn format_scene_capture_type(value: i64) -> String {
    match value {
        0 => "Standard".to_string(),
        1 => "Landscape".to_string(),
        2 => "Portrait".to_string(),
        3 => "Night".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format Contrast enum value
/// EXIF tag 0xA408
pub fn format_contrast(value: i64) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Low".to_string(),
        2 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format Saturation enum value
/// EXIF tag 0xA409
pub fn format_saturation(value: i64) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Low".to_string(),
        2 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format Sharpness enum value
/// EXIF tag 0xA40A
pub fn format_sharpness(value: i64) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Soft".to_string(),
        2 => "Hard".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format GainControl enum value
/// EXIF tag 0xA407
pub fn format_gain_control(value: i64) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Low gain up".to_string(),
        2 => "High gain up".to_string(),
        3 => "Low gain down".to_string(),
        4 => "High gain down".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// `0xa300 FileSource`'s PrintConv (Exif.pm:2811), transcribed verbatim:
///
/// ```text
///         PrintConv => {
///             1 => 'Film Scanner',
///             2 => 'Reflection Print Scanner',
///             3 => 'Digital Camera',
///             # handle the case where Sigma incorrectly gives this tag a count of 4
///             "\3\0\0\0" => 'Sigma Digital Camera',
///         },
/// ```
///
/// This is the single copy. Four divergent transcriptions used to exist -- this
/// one, `core::binary_decoders::decode_file_source`, `FILE_SOURCE_LABELS` in
/// `parsers::pdf`, and the `0xA300` arm of `parsers::raw::metadata`'s
/// `format_exif_display_value` -- and only the RAW one carried the Sigma key,
/// so `decode_file_source(&[3, 0, 0, 0])` answered `Digital Camera` where
/// ExifTool answers `Sigma Digital Camera`. The corpus holds exactly one file
/// in that shape, `Sigma.jpg` (`undef[4]` = `03 00 00 00`).
const FILE_SOURCE: &[(i64, &str)] = &[
    (1, "Film Scanner"),
    (2, "Reflection Print Scanner"),
    (3, "Digital Camera"),
];

/// The one non-integer key in ExifTool's `%{0xa300}{PrintConv}`.
///
/// `"\3\0\0\0"` is looked up as a raw 4-byte string, not as a number, because
/// `ProcessExif` only rewrites a format-7 value to `int8u` when its count is 1
/// (Exif.pm:6682); a count of 4 stays `undef` and reaches the PrintConv as the
/// literal bytes.
const FILE_SOURCE_SIGMA: (&[u8], &str) = (&[3, 0, 0, 0], "Sigma Digital Camera");

/// Looks up EXIF `FileSource` (0xa300) by its decoded integer code.
///
/// Returns `None` for a code ExifTool does not name, so callers keep their own
/// behaviour for it -- `format_file_source` renders `Unknown (N)` the way
/// `PrintConv` does, while the RAW and PDF paths leave the tag alone.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::exif_enums::file_source_label;
///
/// assert_eq!(file_source_label(1), Some("Film Scanner"));
/// assert_eq!(file_source_label(3), Some("Digital Camera"));
/// assert_eq!(file_source_label(0), None);
/// ```
pub fn file_source_label(value: i64) -> Option<&'static str> {
    FILE_SOURCE
        .iter()
        .find(|&&(id, _)| id == value)
        .map(|&(_, label)| label)
}

/// Looks up EXIF `FileSource` (0xa300) from the raw bytes of an `undef` value.
///
/// ExifTool reads a format-7 value whose count is 1 as `int8u` (Exif.pm:6682,
/// "treat single unknown byte as int8u"), so a lone byte is looked up by its
/// numeric value; any other length is matched against the hash as a raw string,
/// and `"\3\0\0\0"` is the only such key ExifTool holds.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::exif_enums::file_source_label_bytes;
///
/// assert_eq!(file_source_label_bytes(&[3]), Some("Digital Camera"));
/// // Sigma writes the same code with a count of 4 -- a different label
/// assert_eq!(file_source_label_bytes(&[3, 0, 0, 0]), Some("Sigma Digital Camera"));
/// assert_eq!(file_source_label_bytes(&[0]), None);
/// assert_eq!(file_source_label_bytes(&[]), None);
/// ```
pub fn file_source_label_bytes(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [b] => file_source_label(i64::from(*b)),
        other if other == FILE_SOURCE_SIGMA.0 => Some(FILE_SOURCE_SIGMA.1),
        _ => None,
    }
}

/// Format FileSource enum value
/// EXIF tag 0xA300
///
/// No `PrintHex` on 0xa300, so an unnamed code prints in decimal: `Unknown (0)`
/// is what ExifTool reports for the four corpus files that store a zero byte.
pub fn format_file_source(value: i64) -> String {
    match file_source_label(value) {
        Some(label) => label.to_string(),
        None => format!("Unknown ({})", value),
    }
}

/// Format SensingMethod enum value
/// EXIF tag 0xA217
pub fn format_sensing_method(value: i64) -> String {
    match value {
        1 => "Not defined".to_string(),
        2 => "One-chip color area".to_string(),
        3 => "Two-chip color area".to_string(),
        4 => "Three-chip color area".to_string(),
        5 => "Color sequential area".to_string(),
        7 => "Trilinear".to_string(),
        8 => "Color sequential linear".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format Compression enum value
/// EXIF/TIFF tag 0x0103
pub fn format_compression(value: i64) -> String {
    match value {
        1 => "Uncompressed".to_string(),
        2 => "CCITT 1D".to_string(),
        3 => "T4/Group 3 Fax".to_string(),
        4 => "T6/Group 4 Fax".to_string(),
        5 => "LZW".to_string(),
        6 => "JPEG (old-style)".to_string(),
        7 => "JPEG".to_string(),
        8 => "Adobe Deflate".to_string(),
        9 => "JBIG B&W".to_string(),
        10 => "JBIG Color".to_string(),
        99 => "JPEG".to_string(),
        262 => "Kodak 262".to_string(),
        32766 => "Next".to_string(),
        32767 => "Sony ARW Compressed".to_string(),
        32769 => "Packed RAW".to_string(),
        32770 => "Samsung SRW Compressed".to_string(),
        32771 => "CCIRLEW".to_string(),
        32772 => "Samsung SRW Compressed 2".to_string(),
        32773 => "PackBits".to_string(),
        32809 => "Thunderscan".to_string(),
        32867 => "Kodak KDC Compressed".to_string(),
        32895 => "IT8CTPAD".to_string(),
        32896 => "IT8LW".to_string(),
        32897 => "IT8MP".to_string(),
        32898 => "IT8BL".to_string(),
        32908 => "PixarFilm".to_string(),
        32909 => "PixarLog".to_string(),
        32946 => "Deflate".to_string(),
        32947 => "DCS".to_string(),
        33003 | 33004 | 33005 => "Aperio JPEG 2000 YCbCr".to_string(),
        34661 => "JBIG".to_string(),
        34676 => "SGILog".to_string(),
        34677 => "SGILog24".to_string(),
        34712 => "JPEG 2000".to_string(),
        34713 => "Nikon NEF Compressed".to_string(),
        34715 => "JBIG2 TIFF FX".to_string(),
        34718 => "Microsoft Document Imaging (MDI) Binary Level Codec".to_string(),
        34719 => "Microsoft Document Imaging (MDI) Progressive Transform Codec".to_string(),
        34720 => "Microsoft Document Imaging (MDI) Vector".to_string(),
        34887 => "ESRI Lerc".to_string(),
        34892 => "Lossy JPEG".to_string(),
        34925 => "LZMA2".to_string(),
        34926 => "Zstd".to_string(),
        34927 => "WebP".to_string(),
        34933 => "PNG".to_string(),
        34934 => "JPEG XR".to_string(),
        65000 => "Kodak DCR Compressed".to_string(),
        65535 => "Pentax PEF Compressed".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format Orientation enum value
/// EXIF/TIFF tag 0x0112
pub fn format_orientation(value: i64) -> String {
    match value {
        1 => "Horizontal (normal)".to_string(),
        2 => "Mirror horizontal".to_string(),
        3 => "Rotate 180".to_string(),
        4 => "Mirror vertical".to_string(),
        5 => "Mirror horizontal and rotate 270 CW".to_string(),
        6 => "Rotate 90 CW".to_string(),
        7 => "Mirror horizontal and rotate 90 CW".to_string(),
        8 => "Rotate 270 CW".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format ResolutionUnit enum value
/// EXIF/TIFF tag 0x0128
pub fn format_resolution_unit(value: i64) -> String {
    match value {
        1 => "None".to_string(),
        2 => "inches".to_string(),
        3 => "cm".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format YCbCrPositioning enum value
/// EXIF tag 0x0213
pub fn format_ycbcr_positioning(value: i64) -> String {
    match value {
        1 => "Centered".to_string(),
        2 => "Co-sited".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format ComponentsConfiguration binary data
/// EXIF tag 0x9101 - 4 bytes representing Y, Cb, Cr, - or R, G, B, -
pub fn format_components_configuration(data: &[u8]) -> String {
    if data.len() < 4 {
        return format!("(Binary data {} bytes)", data.len());
    }

    let component_names: Vec<&str> = data
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
        .collect();

    component_names.join(", ")
}

/// Format CustomRendered enum value
/// EXIF tag 0xA401
pub fn format_custom_rendered(value: i64) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Custom".to_string(),
        2 => "HDR (no original saved)".to_string(),
        3 => "HDR (original saved)".to_string(),
        4 => "Original (for HDR)".to_string(),
        6 => "Panorama".to_string(),
        7 => "Portrait HDR".to_string(),
        8 => "Portrait".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format DigitalZoomRatio - converts 0 to "Digital zoom not used"
pub fn format_digital_zoom_ratio(value: f64) -> String {
    if value == 0.0 {
        "Digital zoom not used".to_string()
    } else {
        format!("{}", value)
    }
}

/// Format SubjectDistanceRange enum value
/// EXIF tag 0xA40C
pub fn format_subject_distance_range(value: i64) -> String {
    match value {
        0 => "Unknown".to_string(),
        1 => "Macro".to_string(),
        2 => "Close".to_string(),
        3 => "Distant".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format InteropIndex value
/// EXIF Interop tag 0x0001
pub fn format_interop_index(value: &str) -> String {
    match value.trim() {
        "R98" => "R98 - DCF basic file (sRGB)".to_string(),
        "THM" => "THM - DCF thumbnail file".to_string(),
        "R03" => "R03 - DCF option file (Adobe RGB)".to_string(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_space() {
        assert_eq!(format_color_space(1), "sRGB");
        assert_eq!(format_color_space(65535), "Uncalibrated");
    }

    #[test]
    fn test_metering_mode() {
        assert_eq!(format_metering_mode(5), "Multi-segment");
        assert_eq!(format_metering_mode(1), "Average");
    }

    #[test]
    fn test_flash() {
        assert_eq!(format_flash(0), "No Flash");
        assert_eq!(format_flash(1), "Fired");
    }

    #[test]
    fn test_orientation() {
        assert_eq!(format_orientation(1), "Horizontal (normal)");
        assert_eq!(format_orientation(6), "Rotate 90 CW");
    }

    #[test]
    fn test_compression() {
        assert_eq!(format_compression(1), "Uncompressed");
        assert_eq!(format_compression(6), "JPEG (old-style)");
        assert_eq!(format_compression(7), "JPEG");
    }

    #[test]
    fn test_components_configuration() {
        assert_eq!(
            format_components_configuration(&[1, 2, 3, 0]),
            "Y, Cb, Cr, -"
        );
        assert_eq!(format_components_configuration(&[4, 5, 6, 0]), "R, G, B, -");
    }

    #[test]
    fn test_custom_rendered() {
        assert_eq!(format_custom_rendered(0), "Normal");
        assert_eq!(format_custom_rendered(1), "Custom");
    }

    #[test]
    fn test_interop_index() {
        assert_eq!(format_interop_index("R98"), "R98 - DCF basic file (sRGB)");
    }

    /// The whole 0xa300 hash, including the key that is not a number.
    ///
    /// Three of the four transcriptions this table replaced held only 1, 2 and
    /// 3, so a test that asserts those alone passes against every one of them
    /// and proves nothing. `"\3\0\0\0"` is the key that told them apart:
    /// `binary_decoders::decode_file_source` read `data[0]` and answered
    /// `Digital Camera` for it.
    #[test]
    fn file_source_matches_exiftool_print_conv_including_the_sigma_key() {
        assert_eq!(file_source_label(1), Some("Film Scanner"));
        assert_eq!(file_source_label(2), Some("Reflection Print Scanner"));
        assert_eq!(file_source_label(3), Some("Digital Camera"));
        assert_eq!(
            file_source_label_bytes(&[3, 0, 0, 0]),
            Some("Sigma Digital Camera")
        );
        // The same code with a count of 1 is the ordinary camera label.
        assert_eq!(file_source_label_bytes(&[3]), Some("Digital Camera"));
    }

    /// Exactly three of the 256 byte values are FileSource codes.
    ///
    /// `Sigma.jpg` is the only corpus file in the four-byte shape, so an
    /// implementation that folded `"\3\0\0\0"` into `3` would still look right
    /// on 2,847 of the 2,848 `Digital Camera` files.
    #[test]
    fn file_source_names_exactly_three_of_two_hundred_fifty_six_bytes() {
        let named = (0u8..=255).filter(|&b| file_source_label_bytes(&[b]).is_some());
        assert_eq!(named.count(), 3);
    }

    /// A code the hash does not name prints its number, and is not guessed at.
    ///
    /// `Unknown (0)` is a value ExifTool really reports: four corpus files
    /// (`GPS.jpg`, `CanonMP610series.jpg`, `NikonSUPER_COOLSCAN9000ED.jpg`,
    /// `SamsungHMX-H300.jpg`) store a zero byte here.
    #[test]
    fn file_source_unnamed_codes_print_their_number() {
        assert_eq!(format_file_source(0), "Unknown (0)");
        assert_eq!(format_file_source(4), "Unknown (4)");
        assert_eq!(format_file_source(255), "Unknown (255)");
        assert_eq!(file_source_label(0), None);
        // Neither an empty value nor an unrecognised multi-byte one is named.
        assert_eq!(file_source_label_bytes(&[]), None);
        assert_eq!(file_source_label_bytes(&[3, 0, 0]), None);
        assert_eq!(file_source_label_bytes(&[1, 0, 0, 0]), None);
    }
}
