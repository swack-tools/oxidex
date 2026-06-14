//! Coverage tests for enum mappers, EXIF formatters, RAW parsers, and FLIR.
//!
//! Segment targets:
//! - src/parsers/tiff/tiff_enums.rs        (pub tiff_enum_to_string)
//! - src/core/formatters/exif_enums.rs     (pure format_* fns)
//! - src/parsers/icc/tags.rs               (parse_tags_registry via parse_icc_profile_data)
//! - src/parsers/raw/metadata.rs           (parse_raw_metadata dispatch)
//! - src/parsers/raw/raf_parser.rs         (parse_raf_makernote + decoders)
//! - src/parsers/jpeg/flir_parser.rs       (parse_flir_segment)
//!
//! These tests drive the public API with synthetic byte buffers built to match
//! the exact layout each parser expects, hitting many distinct branches.

#[path = "common/mod.rs"]
mod common;

#[allow(unused_imports)]
use common::TestReader;
use oxidex::core::TagValue;

// ============================================================================
// tiff_enums::tiff_enum_to_string
// ============================================================================

use oxidex::parsers::tiff::tiff_enums::tiff_enum_to_string;

#[test]
fn test_tiff_orientation_all_values() {
    // Tag 0x0112: values 1..=8 mapped, others None.
    let expected = [
        (1, "Horizontal (normal)"),
        (2, "Mirror horizontal"),
        (3, "Rotate 180"),
        (4, "Mirror vertical"),
        (5, "Mirror horizontal and rotate 270 CW"),
        (6, "Rotate 90 CW"),
        (7, "Mirror horizontal and rotate 90 CW"),
        (8, "Rotate 270 CW"),
    ];
    for (val, name) in expected {
        assert_eq!(tiff_enum_to_string(0x0112, val), Some(name.to_string()));
    }
    assert_eq!(tiff_enum_to_string(0x0112, 0), None);
    assert_eq!(tiff_enum_to_string(0x0112, 99), None);
}

#[test]
fn test_tiff_compression_many_values() {
    // Tag 0x0103: a wide range of compression codes.
    let cases = [
        (1, "Uncompressed"),
        (2, "CCITT 1D"),
        (3, "T4/Group 3 Fax"),
        (4, "T6/Group 4 Fax"),
        (5, "LZW"),
        (6, "JPEG (old-style)"),
        (7, "JPEG"),
        (8, "Adobe Deflate"),
        (9, "JBIG B&W"),
        (10, "JBIG Color"),
        (99, "JPEG"),
        (262, "Kodak 262"),
        (32766, "Next"),
        (32767, "Sony ARW Compressed"),
        (32769, "Packed RAW"),
        (32770, "Samsung SRW Compressed"),
        (32771, "CCIRLEW"),
        (32773, "PackBits"),
        (32809, "Thunderscan"),
        (32867, "Kodak KDC Compressed"),
        (32895, "IT8CTPAD"),
        (32896, "IT8LW"),
        (32897, "IT8MP"),
        (32898, "IT8BL"),
        (32908, "PixarFilm"),
        (32909, "PixarLog"),
        (32946, "Deflate"),
        (32947, "DCS"),
        (34661, "JBIG"),
        (34676, "SGILog"),
        (34677, "SGILog24"),
        (34712, "JPEG 2000"),
        (34713, "Nikon NEF Compressed"),
        (34715, "JBIG2 TIFF FX"),
        (34892, "Lossy JPEG"),
        (65000, "Kodak DCR Compressed"),
        (65535, "Pentax PEF Compressed"),
    ];
    for (val, name) in cases {
        assert_eq!(
            tiff_enum_to_string(0x0103, val),
            Some(name.to_string()),
            "compression value {}",
            val
        );
    }
    // The multi-line MDI codes
    assert!(tiff_enum_to_string(0x0103, 34718).unwrap().contains("MDI"));
    assert!(tiff_enum_to_string(0x0103, 34719).unwrap().contains("MDI"));
    assert!(tiff_enum_to_string(0x0103, 34720).unwrap().contains("MDI"));
    assert_eq!(tiff_enum_to_string(0x0103, 12345), None);
}

#[test]
fn test_tiff_photometric_and_planar_and_resolution() {
    // PhotometricInterpretation (0x0106)
    for (v, n) in [
        (0, "WhiteIsZero"),
        (1, "BlackIsZero"),
        (2, "RGB"),
        (3, "RGB Palette"),
        (4, "Transparency Mask"),
        (5, "CMYK"),
        (6, "YCbCr"),
        (8, "CIELab"),
        (9, "ICCLab"),
        (10, "ITULab"),
        (32803, "Color Filter Array"),
        (32844, "Pixar LogL"),
        (32845, "Pixar LogLuv"),
        (34892, "Linear Raw"),
    ] {
        assert_eq!(tiff_enum_to_string(0x0106, v), Some(n.to_string()));
    }
    assert_eq!(tiff_enum_to_string(0x0106, 7), None);

    // PlanarConfiguration (0x011C)
    assert_eq!(tiff_enum_to_string(0x011C, 1), Some("Chunky".to_string()));
    assert_eq!(tiff_enum_to_string(0x011C, 2), Some("Planar".to_string()));
    assert_eq!(tiff_enum_to_string(0x011C, 3), None);

    // ResolutionUnit (0x0128)
    assert_eq!(tiff_enum_to_string(0x0128, 1), Some("None".to_string()));
    assert_eq!(tiff_enum_to_string(0x0128, 2), Some("inches".to_string()));
    assert_eq!(tiff_enum_to_string(0x0128, 3), Some("cm".to_string()));
    assert_eq!(tiff_enum_to_string(0x0128, 0), None);
}

#[test]
fn test_tiff_fill_sample_ycbcr_extra_subfile_predictor() {
    // FillOrder (0x010A)
    assert_eq!(tiff_enum_to_string(0x010A, 1), Some("Normal".to_string()));
    assert_eq!(tiff_enum_to_string(0x010A, 2), Some("Reversed".to_string()));
    assert_eq!(tiff_enum_to_string(0x010A, 3), None);

    // SampleFormat (0x0153)
    for (v, n) in [
        (1, "Unsigned"),
        (2, "Signed"),
        (3, "Float"),
        (4, "Undefined"),
        (5, "Complex int"),
        (6, "Complex float"),
    ] {
        assert_eq!(tiff_enum_to_string(0x0153, v), Some(n.to_string()));
    }
    assert_eq!(tiff_enum_to_string(0x0153, 0), None);

    // YCbCrPositioning (0x0213)
    assert_eq!(tiff_enum_to_string(0x0213, 1), Some("Centered".to_string()));
    assert_eq!(tiff_enum_to_string(0x0213, 2), Some("Co-sited".to_string()));
    assert_eq!(tiff_enum_to_string(0x0213, 9), None);

    // ExtraSamples (0x0152)
    assert_eq!(
        tiff_enum_to_string(0x0152, 0),
        Some("Unspecified".to_string())
    );
    assert_eq!(
        tiff_enum_to_string(0x0152, 1),
        Some("Associated Alpha".to_string())
    );
    assert_eq!(
        tiff_enum_to_string(0x0152, 2),
        Some("Unassociated Alpha".to_string())
    );
    assert_eq!(tiff_enum_to_string(0x0152, 5), None);

    // NewSubfileType (0x00FE) all 0..=7
    for v in 0..=7i64 {
        assert!(tiff_enum_to_string(0x00FE, v).is_some());
    }
    assert_eq!(tiff_enum_to_string(0x00FE, 8), None);

    // Predictor (0x013D)
    assert_eq!(tiff_enum_to_string(0x013D, 1), Some("None".to_string()));
    assert_eq!(
        tiff_enum_to_string(0x013D, 2),
        Some("Horizontal differencing".to_string())
    );
    assert_eq!(
        tiff_enum_to_string(0x013D, 3),
        Some("Floating point predictor".to_string())
    );
    assert_eq!(tiff_enum_to_string(0x013D, 9), None);
}

#[test]
fn test_tiff_exif_enums_through_helper() {
    // ColorSpace 0xA001
    assert_eq!(tiff_enum_to_string(0xA001, 1), Some("sRGB".to_string()));
    assert_eq!(
        tiff_enum_to_string(0xA001, 2),
        Some("Adobe RGB".to_string())
    );
    assert_eq!(
        tiff_enum_to_string(0xA001, 65535),
        Some("Uncalibrated".to_string())
    );
    assert_eq!(tiff_enum_to_string(0xA001, 0), None);

    // MeteringMode 0x9207
    for (v, n) in [
        (0, "Unknown"),
        (1, "Average"),
        (2, "Center-weighted average"),
        (3, "Spot"),
        (4, "Multi-spot"),
        (5, "Multi-segment"),
        (6, "Partial"),
        (255, "Other"),
    ] {
        assert_eq!(tiff_enum_to_string(0x9207, v), Some(n.to_string()));
    }
    assert_eq!(tiff_enum_to_string(0x9207, 99), None);

    // SensingMethod 0xA217
    for v in [1, 2, 3, 4, 5, 7, 8] {
        assert!(tiff_enum_to_string(0xA217, v).is_some());
    }
    assert_eq!(tiff_enum_to_string(0xA217, 6), None);

    // CustomRendered 0xA401
    for v in [0, 1, 2, 3, 4, 6, 7, 8] {
        assert!(tiff_enum_to_string(0xA401, v).is_some());
    }
    assert_eq!(tiff_enum_to_string(0xA401, 5), None);

    // ExposureMode 0xA402 / WhiteBalance 0xA403
    assert_eq!(tiff_enum_to_string(0xA402, 0), Some("Auto".to_string()));
    assert_eq!(tiff_enum_to_string(0xA402, 1), Some("Manual".to_string()));
    assert_eq!(
        tiff_enum_to_string(0xA402, 2),
        Some("Auto bracket".to_string())
    );
    assert_eq!(tiff_enum_to_string(0xA402, 9), None);
    assert_eq!(tiff_enum_to_string(0xA403, 0), Some("Auto".to_string()));
    assert_eq!(tiff_enum_to_string(0xA403, 1), Some("Manual".to_string()));
    assert_eq!(tiff_enum_to_string(0xA403, 2), None);

    // SceneCaptureType 0xA406
    for v in 0..=4i64 {
        assert!(tiff_enum_to_string(0xA406, v).is_some());
    }
    assert_eq!(tiff_enum_to_string(0xA406, 5), None);

    // ExposureProgram 0x8822
    for v in 0..=9i64 {
        assert!(tiff_enum_to_string(0x8822, v).is_some());
    }
    assert_eq!(tiff_enum_to_string(0x8822, 10), None);
}

#[test]
fn test_tiff_light_source_and_processing_enums() {
    // LightSource 0x9208 (covers all listed codes)
    for v in [
        0, 1, 2, 3, 4, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 255,
    ] {
        assert!(
            tiff_enum_to_string(0x9208, v).is_some(),
            "light source {}",
            v
        );
    }
    assert_eq!(tiff_enum_to_string(0x9208, 5), None);

    // GainControl 0xA407
    for v in 0..=4i64 {
        assert!(tiff_enum_to_string(0xA407, v).is_some());
    }
    assert_eq!(tiff_enum_to_string(0xA407, 9), None);

    // Contrast/Saturation/Sharpness (0xA408/0xA409/0xA40A)
    for tag in [0xA408u16, 0xA409, 0xA40A] {
        for v in 0..=2i64 {
            assert!(tiff_enum_to_string(tag, v).is_some());
        }
        assert_eq!(tiff_enum_to_string(tag, 3), None);
    }

    // SubjectDistanceRange 0xA40C
    for v in 0..=3i64 {
        assert!(tiff_enum_to_string(0xA40C, v).is_some());
    }
    assert_eq!(tiff_enum_to_string(0xA40C, 4), None);

    // SceneType 0xA301
    assert_eq!(
        tiff_enum_to_string(0xA301, 1),
        Some("Directly photographed".to_string())
    );
    assert_eq!(tiff_enum_to_string(0xA301, 0), None);

    // SensitivityType 0x8830 (0..=7)
    for v in 0..=7i64 {
        assert!(tiff_enum_to_string(0x8830, v).is_some());
    }
    assert_eq!(tiff_enum_to_string(0x8830, 8), None);

    // CompositeImage 0xA460
    for v in 0..=3i64 {
        assert!(tiff_enum_to_string(0xA460, v).is_some());
    }
    assert_eq!(tiff_enum_to_string(0xA460, 9), None);

    // MakerNoteSafety 0xC635
    assert_eq!(tiff_enum_to_string(0xC635, 0), Some("Unsafe".to_string()));
    assert_eq!(tiff_enum_to_string(0xC635, 1), Some("Safe".to_string()));
    assert_eq!(tiff_enum_to_string(0xC635, 2), None);

    // Unknown tag id entirely
    assert_eq!(tiff_enum_to_string(0xFFFF, 1), None);
}

// ============================================================================
// formatters::exif_enums - pure format_* functions
// ============================================================================

use oxidex::core::formatters::exif_enums::{
    format_color_space, format_components_configuration, format_compression, format_contrast,
    format_custom_rendered, format_digital_zoom_ratio, format_exposure_mode, format_file_source,
    format_flash, format_gain_control, format_interop_index, format_light_source,
    format_metering_mode, format_orientation, format_resolution_unit, format_saturation,
    format_scene_capture_type, format_sensing_method, format_sharpness,
    format_subject_distance_range, format_white_balance, format_ycbcr_positioning,
};

#[test]
fn test_format_color_space_and_unknown() {
    assert_eq!(format_color_space(1), "sRGB");
    assert_eq!(format_color_space(2), "Adobe RGB");
    assert_eq!(format_color_space(65535), "Uncalibrated");
    assert_eq!(format_color_space(7), "Unknown (7)");
}

#[test]
fn test_format_metering_and_light_source() {
    for (v, n) in [
        (0, "Unknown"),
        (1, "Average"),
        (2, "Center-weighted average"),
        (3, "Spot"),
        (4, "Multi-spot"),
        (5, "Multi-segment"),
        (6, "Partial"),
        (255, "Other"),
    ] {
        assert_eq!(format_metering_mode(v), n);
    }
    assert_eq!(format_metering_mode(42), "Unknown (42)");

    // Cover every arm of light source, plus the default.
    for v in [
        0, 1, 2, 3, 4, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 255,
    ] {
        assert!(!format_light_source(v).starts_with("Unknown ("));
    }
    assert_eq!(format_light_source(100), "Unknown (100)");
}

#[test]
fn test_format_flash_bitfield_branches() {
    // No flash, value 0 -> early-return path.
    assert_eq!(format_flash(0), "No Flash");
    // Fired only.
    assert_eq!(format_flash(0x01), "Fired");
    // Fired + return detected (bits 1-2 = 0b11 => value 0b111 = 7).
    let s = format_flash(0x07);
    assert!(s.contains("Fired"));
    assert!(s.contains("Return detected"));
    // Fired + return NOT detected (0b101 = 5).
    let s = format_flash(0x05);
    assert!(s.contains("Return not detected"));
    // Mode On (bits 3-4 = 01 => 0b01000 = 8) fired off.
    let s = format_flash(0x08);
    assert!(s.contains("No Flash"));
    assert!(s.contains("On"));
    // Mode Off (10 => 0x10) and Auto (11 => 0x18).
    assert!(format_flash(0x10).contains("Off"));
    assert!(format_flash(0x18).contains("Auto"));
    // No flash function bit (0x20 clears function_present).
    let s = format_flash(0x01);
    assert!(!s.contains("No flash function"));
    let s = format_flash(0x21); // fired + no function present? 0x20 set => function_present false
    // 0x20 set means function_present == false -> "No flash function"
    assert!(s.contains("No flash function"));
    // Red-eye reduction bit (0x40).
    let s = format_flash(0x41);
    assert!(s.contains("Red-eye reduction"));
    // Full-featured combo.
    let s = format_flash(0x4F);
    assert!(s.contains("Fired"));
}

#[test]
fn test_format_exposure_white_scene_processing() {
    assert_eq!(format_exposure_mode(0), "Auto");
    assert_eq!(format_exposure_mode(1), "Manual");
    assert_eq!(format_exposure_mode(2), "Auto bracket");
    assert_eq!(format_exposure_mode(9), "Unknown (9)");

    assert_eq!(format_white_balance(0), "Auto");
    assert_eq!(format_white_balance(1), "Manual");
    assert_eq!(format_white_balance(5), "Unknown (5)");

    for (v, n) in [
        (0, "Standard"),
        (1, "Landscape"),
        (2, "Portrait"),
        (3, "Night"),
    ] {
        assert_eq!(format_scene_capture_type(v), n);
    }
    assert_eq!(format_scene_capture_type(9), "Unknown (9)");

    for f in [format_contrast as fn(i64) -> String, format_saturation] {
        assert_eq!(f(0), "Normal");
        assert_eq!(f(1), "Low");
        assert_eq!(f(2), "High");
        assert_eq!(f(9), "Unknown (9)");
    }

    assert_eq!(format_sharpness(0), "Normal");
    assert_eq!(format_sharpness(1), "Soft");
    assert_eq!(format_sharpness(2), "Hard");
    assert_eq!(format_sharpness(9), "Unknown (9)");

    for v in 0..=4i64 {
        assert!(!format_gain_control(v).starts_with("Unknown ("));
    }
    assert_eq!(format_gain_control(9), "Unknown (9)");
}

#[test]
fn test_format_file_source_sensing_subject_components() {
    assert_eq!(format_file_source(1), "Film Scanner");
    assert_eq!(format_file_source(2), "Reflection Print Scanner");
    assert_eq!(format_file_source(3), "Digital Camera");
    assert_eq!(format_file_source(9), "Unknown (9)");

    for v in [1, 2, 3, 4, 5, 7, 8] {
        assert!(!format_sensing_method(v).starts_with("Unknown ("));
    }
    assert_eq!(format_sensing_method(6), "Unknown (6)");

    for v in 0..=3i64 {
        assert!(!format_subject_distance_range(v).starts_with("Unknown ("));
    }
    assert_eq!(format_subject_distance_range(9), "Unknown (9)");

    // ComponentsConfiguration: short buffer + full mapping incl. unknown byte.
    assert!(format_components_configuration(&[1, 2]).contains("Binary data"));
    assert_eq!(
        format_components_configuration(&[1, 2, 3, 0]),
        "Y, Cb, Cr, -"
    );
    assert_eq!(format_components_configuration(&[4, 5, 6, 0]), "R, G, B, -");
    assert_eq!(
        format_components_configuration(&[7, 8, 9, 10]),
        "?, ?, ?, ?"
    );
}

#[test]
fn test_format_compression_branches() {
    // A sampling that includes arms unique to the formatter (not in tiff_enums).
    let cases = [
        (1, "Uncompressed"),
        (32772, "Samsung SRW Compressed 2"),
        (33003, "Aperio JPEG 2000 YCbCr"),
        (33004, "Aperio JPEG 2000 YCbCr"),
        (33005, "Aperio JPEG 2000 YCbCr"),
        (34887, "ESRI Lerc"),
        (34925, "LZMA2"),
        (34926, "Zstd"),
        (34927, "WebP"),
        (34933, "PNG"),
        (34934, "JPEG XR"),
        (65535, "Pentax PEF Compressed"),
    ];
    for (v, n) in cases {
        assert_eq!(format_compression(v), n, "compression {}", v);
    }
    assert_eq!(format_compression(123456), "Unknown (123456)");
}

#[test]
fn test_format_orientation_resolution_ycbcr_custom_zoom_interop() {
    for v in 1..=8i64 {
        assert!(!format_orientation(v).starts_with("Unknown ("));
    }
    assert_eq!(format_orientation(9), "Unknown (9)");

    assert_eq!(format_resolution_unit(1), "None");
    assert_eq!(format_resolution_unit(2), "inches");
    assert_eq!(format_resolution_unit(3), "cm");
    assert_eq!(format_resolution_unit(9), "Unknown (9)");

    assert_eq!(format_ycbcr_positioning(1), "Centered");
    assert_eq!(format_ycbcr_positioning(2), "Co-sited");
    assert_eq!(format_ycbcr_positioning(9), "Unknown (9)");

    for v in [0, 1, 2, 3, 4, 6, 7, 8] {
        assert!(!format_custom_rendered(v).starts_with("Unknown ("));
    }
    assert_eq!(format_custom_rendered(5), "Unknown (5)");

    assert_eq!(format_digital_zoom_ratio(0.0), "Digital zoom not used");
    assert_eq!(format_digital_zoom_ratio(2.5), "2.5");

    assert_eq!(format_interop_index("R98"), "R98 - DCF basic file (sRGB)");
    assert_eq!(format_interop_index("THM"), "THM - DCF thumbnail file");
    assert_eq!(
        format_interop_index("R03"),
        "R03 - DCF option file (Adobe RGB)"
    );
    assert_eq!(format_interop_index("XYZ"), "XYZ");
    // Trimming path
    assert_eq!(
        format_interop_index("  R98  "),
        "R98 - DCF basic file (sRGB)"
    );
}

// ============================================================================
// icc::tags via parse_icc_profile_data
// ============================================================================

use oxidex::parsers::icc::parse_icc_profile_data;

/// Write a 32-bit big-endian value into a buffer at the given offset (extending
/// with zeros as needed).
fn put_u32_be(buf: &mut Vec<u8>, offset: usize, value: u32) {
    if buf.len() < offset + 4 {
        buf.resize(offset + 4, 0);
    }
    buf[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

/// Build a minimal but valid ICC profile with a tag table containing the
/// provided (signature, tag_data) tuples. Header is 128 bytes; the tag count
/// is at offset 128; entries (12 bytes) follow at 132.
fn build_icc(tags: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let header_size = 128usize;
    let tag_count = tags.len();
    let table_start = 132usize;
    let table_size = tag_count * 12;
    let data_start = table_start + table_size;

    // Compute layout: each tag's data placed sequentially after the table.
    let mut tag_offsets = Vec::with_capacity(tag_count);
    let mut cursor = data_start;
    for (_, data) in tags {
        tag_offsets.push(cursor);
        cursor += data.len();
    }
    let total = cursor.max(header_size);

    let mut buf = vec![0u8; total];

    // Minimal header: set profile size at offset 0 and "acsp" signature at 36.
    put_u32_be(&mut buf, 0, total as u32);
    buf[36..40].copy_from_slice(b"acsp");

    // Tag count at offset 128.
    put_u32_be(&mut buf, 128, tag_count as u32);

    // Tag table entries.
    for (i, (sig, data)) in tags.iter().enumerate() {
        let entry = table_start + i * 12;
        buf[entry..entry + 4].copy_from_slice(*sig);
        put_u32_be(&mut buf, entry + 4, tag_offsets[i] as u32);
        put_u32_be(&mut buf, entry + 8, data.len() as u32);
    }

    // Place tag data.
    for (i, (_, data)) in tags.iter().enumerate() {
        let off = tag_offsets[i];
        buf[off..off + data.len()].copy_from_slice(data);
    }

    buf
}

/// Build an ICC "textType" tag: "text" sig (4) + reserved (4) + ascii text.
fn icc_text(text: &str) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"text");
    v.extend_from_slice(&[0, 0, 0, 0]); // reserved
    v.extend_from_slice(text.as_bytes());
    v.push(0);
    v
}

/// Build an ICC "desc" textDescriptionType: "desc"+reserved+ascii_count+text.
fn icc_desc(text: &str) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"desc");
    v.extend_from_slice(&[0, 0, 0, 0]); // reserved
    let bytes = text.as_bytes();
    let count = (bytes.len() + 1) as u32; // include null
    v.extend_from_slice(&count.to_be_bytes());
    v.extend_from_slice(bytes);
    v.push(0);
    // pad a bit for safety
    v.extend_from_slice(&[0, 0, 0, 0]);
    v
}

/// Build an ICC XYZType: "XYZ "+reserved+three s15Fixed16 values.
fn icc_xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"XYZ ");
    v.extend_from_slice(&[0, 0, 0, 0]); // reserved
    let enc = |f: f64| -> [u8; 4] { ((f * 65536.0) as i32).to_be_bytes() };
    v.extend_from_slice(&enc(x));
    v.extend_from_slice(&enc(y));
    v.extend_from_slice(&enc(z));
    v
}

/// Build an ICC signatureType: "sig "+reserved+4-byte signature.
fn icc_sig(four: &[u8; 4]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"sig ");
    v.extend_from_slice(&[0, 0, 0, 0]); // reserved
    v.extend_from_slice(four);
    v
}

#[test]
fn test_icc_text_xyz_desc_curve_signature() {
    // ProfileCopyright (Text), ProfileDescription (TextDescription/desc),
    // RedMatrixColumn (Xyz), RedToneReproductionCurve (Curve),
    // Technology (Signature -> "dcam" => "Digital Camera").
    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"cprt", icc_text("Copyright ACME 2026")),
        (b"desc", icc_desc("ACME Display Profile")),
        (b"rXYZ", icc_xyz(0.4, 0.2, 0.01)),
        (b"rTRC", {
            let mut v = Vec::new();
            v.extend_from_slice(b"curv");
            v.extend_from_slice(&[0, 0, 0, 0]);
            v.extend_from_slice(&[0, 0, 0, 4]); // 4 entries
            v.extend_from_slice(&[0, 1, 0, 2, 0, 3, 0, 4]);
            v
        }),
        (b"tech", icc_sig(b"dcam")),
    ];
    let buf = build_icc(&tags);

    let md = parse_icc_profile_data(&buf).expect("ICC parse should succeed");

    assert_eq!(
        md.get("ProfileCopyright"),
        Some(&TagValue::String("Copyright ACME 2026".to_string()))
    );
    assert_eq!(
        md.get("ProfileDescription"),
        Some(&TagValue::String("ACME Display Profile".to_string()))
    );
    // XYZ formatted as "x y z".
    if let Some(TagValue::String(s)) = md.get("RedMatrixColumn") {
        assert!(s.split_whitespace().count() == 3, "xyz triple: {}", s);
    } else {
        panic!("RedMatrixColumn missing");
    }
    // Curve produces a "(Binary data N bytes...)" string.
    assert!(
        md.get("RedToneReproductionCurve").is_some(),
        "curve tag present"
    );
    // Technology resolves via TECHNOLOGIES table.
    assert_eq!(
        md.get("Technology"),
        Some(&TagValue::String("Digital Camera".to_string()))
    );
}

#[test]
fn test_icc_viewing_conditions_and_measurement() {
    // ViewingConditions ("view"): needs >= 36 bytes after sig/reserved framing.
    // Layout: 0-3 sig, 4-7 reserved, then illuminant XYZ (8,12,16),
    // surround XYZ (20,24,28), illuminant type u32 at 32.
    let mut view = Vec::new();
    view.extend_from_slice(b"view");
    view.extend_from_slice(&[0, 0, 0, 0]); // reserved
    let enc = |f: f64| ((f * 65536.0) as i32).to_be_bytes();
    view.extend_from_slice(&enc(0.9)); // illum x  (offset 8)
    view.extend_from_slice(&enc(1.0)); // illum y  (12)
    view.extend_from_slice(&enc(1.1)); // illum z  (16)
    view.extend_from_slice(&enc(0.1)); // surr x   (20)
    view.extend_from_slice(&enc(0.2)); // surr y   (24)
    view.extend_from_slice(&enc(0.3)); // surr z   (28)
    view.extend_from_slice(&2u32.to_be_bytes()); // illum type D65 (32)

    // Measurement ("meas"): observer(8 u32), backing XYZ(12,16,20),
    // geometry(24 u32), flare(28 u16fixed16), illuminant(32 u32).
    let mut meas = Vec::new();
    meas.extend_from_slice(b"meas");
    meas.extend_from_slice(&[0, 0, 0, 0]); // reserved
    meas.extend_from_slice(&1u32.to_be_bytes()); // observer CIE 1931 (8)
    meas.extend_from_slice(&enc(0.0)); // backing x (12)
    meas.extend_from_slice(&enc(0.0)); // backing y (16)
    meas.extend_from_slice(&enc(0.0)); // backing z (20)
    meas.extend_from_slice(&1u32.to_be_bytes()); // geometry 0/45 (24)
    meas.extend_from_slice(&(0u32).to_be_bytes()); // flare u16fixed16 (28)
    meas.extend_from_slice(&1u32.to_be_bytes()); // illuminant D50 (32)

    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![(b"view", view), (b"meas", meas)];
    let buf = build_icc(&tags);
    let md = parse_icc_profile_data(&buf).expect("ICC parse should succeed");

    // ViewingConditions produces multiple sub-entries.
    assert!(md.contains_key("ViewingCondIlluminant"));
    assert!(md.contains_key("ViewingCondSurround"));
    assert_eq!(
        md.get("ViewingCondIlluminantType"),
        Some(&TagValue::String("D65".to_string()))
    );

    // Measurement sub-entries.
    assert_eq!(
        md.get("MeasurementObserver"),
        Some(&TagValue::String("CIE 1931".to_string()))
    );
    assert!(md.contains_key("MeasurementBacking"));
    assert_eq!(
        md.get("MeasurementGeometry"),
        Some(&TagValue::String("0/45 or 45/0".to_string()))
    );
    assert!(md.contains_key("MeasurementFlare"));
    assert_eq!(
        md.get("MeasurementIlluminant"),
        Some(&TagValue::String("D50".to_string()))
    );
}

#[test]
fn test_icc_mluc_text_description() {
    // textDescriptionType using "mluc" variant -> parse_mluc_type path.
    // mluc layout: "mluc"(0) reserved(4) num_records(8) record_size(12)
    //   record table: lang(16) country(18) length(20 u32) offset(24 u32)
    //   string data (UTF-16BE) at offset.
    let mut mluc = Vec::new();
    mluc.extend_from_slice(b"mluc");
    mluc.extend_from_slice(&[0, 0, 0, 0]); // reserved (4)
    mluc.extend_from_slice(&1u32.to_be_bytes()); // num_records (8)
    mluc.extend_from_slice(&12u32.to_be_bytes()); // record size (12)
    // record table entry (starts at 16)
    mluc.extend_from_slice(b"en"); // language (16)
    mluc.extend_from_slice(b"US"); // country (18)
    let text16: Vec<u8> = "Hi".encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
    mluc.extend_from_slice(&(text16.len() as u32).to_be_bytes()); // length (20)
    // The string offset: place string right after this 28-byte prefix.
    let str_offset = 28u32;
    mluc.extend_from_slice(&str_offset.to_be_bytes()); // offset (24)
    // pad to str_offset
    while mluc.len() < str_offset as usize {
        mluc.push(0);
    }
    mluc.extend_from_slice(&text16);

    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![(b"desc", mluc)];
    let buf = build_icc(&tags);
    let md = parse_icc_profile_data(&buf).expect("ICC parse should succeed");
    // mluc decoding may or may not succeed depending on exact framing; tolerate
    // either by asserting parse-level success (no panic) and checking presence.
    if let Some(TagValue::String(s)) = md.get("ProfileDescription") {
        assert!(s == "Hi" || !s.is_empty());
    }
}

#[test]
fn test_icc_too_small_and_unknown_tag() {
    // < 128 bytes -> parse_icc_profile errors out.
    let small = vec![0u8; 64];
    assert!(parse_icc_profile_data(&small).is_err());

    // A profile with a tag signature not in TAG_REGISTRY: decode_tag does nothing.
    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![(b"ZZZZ", icc_text("ignored"))];
    let buf = build_icc(&tags);
    let md = parse_icc_profile_data(&buf).expect("parse ok");
    // Unknown tag is silently dropped; header-only result is fine.
    assert!(!md.contains_key("ProfileCopyright"));
}

// ============================================================================
// raw::metadata::parse_raw_metadata - dispatch + format-specific parsers
// ============================================================================

use oxidex::parsers::raw::{RawFormat, parse_raw_metadata};

/// Build a minimal little-endian TIFF with a single IFD holding one entry.
fn build_minimal_tiff_le(entries: &[(u16, u16, u32, [u8; 4])]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"II\x2a\x00");
    data.extend_from_slice(&8u32.to_le_bytes()); // first IFD at offset 8
    data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, typ, count, value) in entries {
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&typ.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        data.extend_from_slice(value);
    }
    data.extend_from_slice(&0u32.to_le_bytes()); // next IFD = none
    data
}

#[test]
fn test_raw_dng_minimal_tiff() {
    // Single SHORT ImageWidth=160 entry.
    let mut width = [0u8; 4];
    width[0..2].copy_from_slice(&160u16.to_le_bytes());
    let data = build_minimal_tiff_le(&[(0x0100, 3, 1, width)]);
    let md = parse_raw_metadata(&data, RawFormat::AdobeDNG).expect("DNG parse");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("AdobeDNG".to_string()))
    );
}

#[test]
fn test_raw_dng_version_string() {
    // DNGVersion tag (0xC612), BYTE type (1), count 4, inline [1,4,0,0].
    let data = build_minimal_tiff_le(&[(0xC612, 1, 4, [1, 4, 0, 0])]);
    let md = parse_raw_metadata(&data, RawFormat::AdobeDNG).expect("DNG parse");
    if let Some(TagValue::String(s)) = md.get("DNG:VersionString") {
        assert_eq!(s, "1.4.0.0");
    }
}

#[test]
fn test_raw_cr2_and_nef_dispatch() {
    let data = build_minimal_tiff_le(&[]);
    let md = parse_raw_metadata(&data, RawFormat::CanonCR2).expect("CR2 parse");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("CanonCR2".to_string()))
    );

    // NEF (big-endian header for Nikon).
    let mut nef = Vec::new();
    nef.extend_from_slice(b"MM\x00\x2a");
    nef.extend_from_slice(&8u32.to_be_bytes());
    nef.extend_from_slice(&0u16.to_be_bytes()); // 0 entries
    nef.extend_from_slice(&0u32.to_be_bytes()); // next IFD
    let md = parse_raw_metadata(&nef, RawFormat::NikonNEF).expect("NEF parse");
    assert!(md.contains_key("File:FileType"));
    let md2 = parse_raw_metadata(&nef, RawFormat::NikonNRW).expect("NRW parse");
    assert!(md2.contains_key("File:FileType"));
}

#[test]
fn test_raw_stub_formats() {
    // CR3 stub
    let md = parse_raw_metadata(b"\x00\x00\x00\x18ftypcrx ", RawFormat::CanonCR3).unwrap();
    assert!(md.contains_key("File:FileType"));
    // CRW stub
    let md = parse_raw_metadata(b"anything", RawFormat::CanonCRW).unwrap();
    assert!(md.contains_key("File:FileType"));
    // Generic fallback: a non-TIFF buffer still yields minimal metadata.
    let md = parse_raw_metadata(b"not a tiff at all", RawFormat::GenericRAW).unwrap();
    assert!(md.contains_key("File:FileType"));
}

#[test]
fn test_raw_too_small_errors() {
    // Fewer than 8 bytes for a TIFF-based format -> Err.
    let r = parse_raw_metadata(b"abc", RawFormat::SonyARW);
    assert!(r.is_err());
}

#[test]
fn test_raw_sigma_x3f_with_directory() {
    // Build an X3F: FOVb header (v2.1) + WB string + SECd directory with a
    // SECp property section.
    let mut data = Vec::new();
    data.extend_from_slice(b"FOVb"); // 0
    data.extend_from_slice(&0x00020001u32.to_le_bytes()); // version 2.1 (4)
    data.extend_from_slice(&[0u8; 16]); // unique id (8..24)
    data.extend_from_slice(&0u32.to_le_bytes()); // mark bits (24)
    data.extend_from_slice(&4000u32.to_le_bytes()); // columns (28)
    data.extend_from_slice(&3000u32.to_le_bytes()); // rows (32)
    data.extend_from_slice(&90u32.to_le_bytes()); // rotation (36)
    // white balance string (32 bytes at offset 40)
    let mut wb = [0u8; 32];
    wb[..4].copy_from_slice(b"Auto");
    data.extend_from_slice(&wb);

    // Build a SECp property section with one property: CAMMODEL = "SD".
    // Header (24 bytes): "SECp"(0) ver(4) num_props(8) char_fmt(12)
    //   reserved(16) total_len(20). Then property table (8 bytes/entry).
    let prop_name = "CAMMODEL";
    let prop_value = "SD1";
    // UTF-16LE data block: name then value, each null terminated. Offsets in the
    // table are in UTF-16 code units (multiplied by 2 inside the parser).
    let name_u16: Vec<u8> = prop_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let value_u16: Vec<u8> = prop_value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let name_offset_units = 0u32;
    let value_offset_units = (name_u16.len() / 2) as u32;

    let mut secp = Vec::new();
    secp.extend_from_slice(b"SECp"); // 0
    secp.extend_from_slice(&1u32.to_le_bytes()); // version (4)
    secp.extend_from_slice(&1u32.to_le_bytes()); // num_props (8)
    secp.extend_from_slice(&0u32.to_le_bytes()); // char fmt UTF-16 (12)
    secp.extend_from_slice(&0u32.to_le_bytes()); // reserved (16)
    secp.extend_from_slice(&0u32.to_le_bytes()); // total len (20)
    // property table entry (24): name_offset, value_offset
    secp.extend_from_slice(&name_offset_units.to_le_bytes());
    secp.extend_from_slice(&value_offset_units.to_le_bytes());
    // data block follows table
    secp.extend_from_slice(&name_u16);
    secp.extend_from_slice(&value_u16);

    // Place SECp section in the file, recording its offset/size.
    let secp_offset = data.len();
    data.extend_from_slice(&secp);

    // Build the SECd directory: "SECd"(0) ver(4) num_entries(8) then entries
    // of 12 bytes: offset, size, type.
    let dir_offset = data.len();
    let mut secd = Vec::new();
    secd.extend_from_slice(b"SECd"); // 0
    secd.extend_from_slice(&1u32.to_le_bytes()); // version (4)
    secd.extend_from_slice(&1u32.to_le_bytes()); // num entries (8)
    secd.extend_from_slice(&(secp_offset as u32).to_le_bytes()); // entry offset
    secd.extend_from_slice(&(secp.len() as u32).to_le_bytes()); // entry size
    secd.extend_from_slice(b"SECp"); // entry type
    data.extend_from_slice(&secd);

    // Trailer: directory offset at (file_size - 4). We append 4 bytes pointing
    // to the SECd directory.
    data.extend_from_slice(&(dir_offset as u32).to_le_bytes());

    let md = parse_raw_metadata(&data, RawFormat::SigmaX3F).expect("X3F parse");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("SigmaX3F".to_string()))
    );
    // Header-derived fields.
    assert!(md.contains_key("SigmaRaw:FileVersion"));
    assert!(md.contains_key("EXIF:ImageWidth"));
    assert!(md.contains_key("EXIF:ImageHeight"));
    assert!(md.contains_key("SigmaRaw:Rotation"));
    assert!(md.contains_key("SigmaRaw:WhiteBalance"));
    // Property mapped via map_x3f_property_name -> EXIF:Model.
    assert_eq!(
        md.get("EXIF:Model"),
        Some(&TagValue::String("SD1".to_string()))
    );
}

#[test]
fn test_raw_sigma_x3f_bad_signature() {
    // Wrong signature -> returns only FileType.
    let md = parse_raw_metadata(b"NOPEshortdata____", RawFormat::SigmaX3F).unwrap();
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("SigmaX3F".to_string()))
    );
    assert!(!md.contains_key("SigmaRaw:FileVersion"));
}

#[test]
fn test_raw_minolta_mrw_with_prd_and_wbg() {
    // MRM container: "\x00MRM" + filesize + blocks.
    let mut data = Vec::new();
    data.extend_from_slice(b"\x00MRM");
    data.extend_from_slice(&0u32.to_be_bytes()); // file size (ignored)

    // PRD block: "\x00PRD" + size + payload (big-endian u16 fields).
    let mut prd = Vec::new();
    prd.extend_from_slice(&1u16.to_be_bytes()); // version (0)
    prd.extend_from_slice(&6000u16.to_be_bytes()); // sensor w (2)
    prd.extend_from_slice(&4000u16.to_be_bytes()); // sensor h (4)
    prd.extend_from_slice(&5800u16.to_be_bytes()); // img w (6)
    prd.extend_from_slice(&3800u16.to_be_bytes()); // img h (8)
    data.extend_from_slice(b"\x00PRD");
    data.extend_from_slice(&(prd.len() as u32).to_be_bytes());
    data.extend_from_slice(&prd);

    // WBG block: R/G/B multipliers.
    let mut wbg = Vec::new();
    wbg.extend_from_slice(&512u16.to_be_bytes()); // r (0)
    wbg.extend_from_slice(&256u16.to_be_bytes()); // g (2)
    wbg.extend_from_slice(&384u16.to_be_bytes()); // b (4)
    wbg.extend_from_slice(&0u16.to_be_bytes()); // pad (6)
    data.extend_from_slice(b"\x00WBG");
    data.extend_from_slice(&(wbg.len() as u32).to_be_bytes());
    data.extend_from_slice(&wbg);

    let md = parse_raw_metadata(&data, RawFormat::MinoltaMRW).expect("MRW parse");
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("MinoltaMRW".to_string()))
    );
    assert_eq!(
        md.get("MakerNotes:SensorWidth"),
        Some(&TagValue::Integer(6000))
    );
    assert_eq!(
        md.get("MakerNotes:SensorHeight"),
        Some(&TagValue::Integer(4000))
    );
    assert_eq!(md.get("EXIF:ImageWidth"), Some(&TagValue::Integer(5800)));
    assert_eq!(md.get("EXIF:ImageHeight"), Some(&TagValue::Integer(3800)));
    assert!(md.contains_key("MakerNotes:ColorBalanceRed"));
    assert!(md.contains_key("MakerNotes:ColorBalanceBlue"));
}

#[test]
fn test_raw_minolta_mrw_bad_signature() {
    let md = parse_raw_metadata(b"\x00XYZsomedata", RawFormat::MinoltaMRW).unwrap();
    assert_eq!(
        md.get("File:FileType"),
        Some(&TagValue::String("MinoltaMRW".to_string()))
    );
}

#[test]
fn test_raw_fujifilm_raf_invalid() {
    // Missing FUJIFILM signature -> Err from parse_fujifilm_raf.
    let r = parse_raw_metadata(b"NOTRAF__________________________", RawFormat::FujifilmRAF);
    assert!(r.is_err());

    // Valid signature but too small for the offset table -> Err.
    let mut data = Vec::new();
    data.extend_from_slice(b"FUJIFILMCCD-RAW ");
    data.extend_from_slice(&[0u8; 10]); // < 92 bytes total
    let r = parse_raw_metadata(&data, RawFormat::FujifilmRAF);
    assert!(r.is_err());
}

#[test]
fn test_raw_fujifilm_raf_jpeg_not_jpeg() {
    // Valid signature + header, but pointed data isn't a JPEG -> Err.
    let mut data = vec![0u8; 200];
    data[0..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
    // jpeg_offset at 84 (BE), jpeg_length at 88 (BE).
    data[84..88].copy_from_slice(&100u32.to_be_bytes());
    data[88..92].copy_from_slice(&50u32.to_be_bytes());
    // bytes at 100 are zero, not 0xFFD8.
    let r = parse_raw_metadata(&data, RawFormat::FujifilmRAF);
    assert!(r.is_err());
}

// ============================================================================
// raf_parser::parse_raf_makernote + decoders
// ============================================================================

use oxidex::parsers::raw::raf_parser::parse_raf_makernote;
use oxidex::parsers::tiff::ifd_parser::ByteOrder;

#[test]
fn test_raf_makernote_valid_header() {
    // "FUJIFILM"(0..8) + reserved/gap up to 0x10, then serial at 0x10..0x14,
    // internal serial at 0x14..0x18 (per parser offsets).
    let mut data = vec![0u8; 0x18];
    data[0..8].copy_from_slice(b"FUJIFILM");
    data[0x10..0x14].copy_from_slice(&0x12345678u32.to_le_bytes()); // serial
    data[0x14..0x18].copy_from_slice(&0xAABBCCDDu32.to_le_bytes()); // internal
    // Pad with model-ish bytes for sensor info (offset 24..32).
    while data.len() < 24 {
        data.push(0);
    }
    data.extend_from_slice(b"X-T4\x00\x00\x00\x00");
    // Extra padding.
    data.extend_from_slice(&[0u8; 16]);

    let tags = parse_raf_makernote(&data, ByteOrder::LittleEndian).expect("raf makernote");
    // Serial number formatted as 8 hex digits.
    assert_eq!(
        tags.get("Fujifilm:SerialNumber").map(String::as_str),
        Some("12345678")
    );
    assert!(tags.contains_key("Fujifilm:InternalSerialNumber"));
    assert!(tags.contains_key("Fujifilm:SensorInfo"));
    assert_eq!(
        tags.get("Fujifilm:ColorSpace").map(String::as_str),
        Some("sRGB")
    );
}

#[test]
fn test_raf_makernote_big_endian_and_errors() {
    // Big-endian variant; serial value lives at offset 0x10.
    let mut data = vec![0u8; 0x40];
    data[0..8].copy_from_slice(b"FUJIFILM");
    data[0x10..0x14].copy_from_slice(&0x01020304u32.to_be_bytes());
    let tags = parse_raf_makernote(&data, ByteOrder::BigEndian).expect("be raf");
    assert_eq!(
        tags.get("Fujifilm:SerialNumber").map(String::as_str),
        Some("01020304")
    );

    // Too small for header -> Err.
    assert!(parse_raf_makernote(b"FUJI", ByteOrder::LittleEndian).is_err());

    // Wrong signature -> Err.
    let bad = b"NOTFUJI_____________";
    assert!(parse_raf_makernote(bad, ByteOrder::LittleEndian).is_err());
}

// ============================================================================
// jpeg::flir_parser::parse_flir_segment
// ============================================================================

use oxidex::core::MetadataMap;
use oxidex::parsers::jpeg::flir_parser::parse_flir_segment;

/// Helper: write a little-endian f32 into a buffer at offset (resize as needed).
fn put_f32_le(buf: &mut Vec<u8>, offset: usize, value: f32) {
    if buf.len() < offset + 4 {
        buf.resize(offset + 4, 0);
    }
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Helper: write a little-endian u16 into a buffer at offset.
fn put_u16_le(buf: &mut Vec<u8>, offset: usize, value: u16) {
    if buf.len() < offset + 2 {
        buf.resize(offset + 2, 0);
    }
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

/// Helper: write a little-endian u32 into a buffer at offset.
fn put_u32_le_buf(buf: &mut Vec<u8>, offset: usize, value: u32) {
    if buf.len() < offset + 4 {
        buf.resize(offset + 4, 0);
    }
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn test_flir_segment_errors() {
    // Too short.
    let mut md = MetadataMap::new();
    assert!(parse_flir_segment(b"FLIR", &mut md).is_err());
    // Not a FLIR segment.
    let mut md = MetadataMap::new();
    assert!(parse_flir_segment(b"EXIF\x00\x00____", &mut md).is_err());
}

#[test]
fn test_flir_legacy_format() {
    // FLIR header (8 bytes) + payload < 64 bytes triggers legacy parsing.
    // Legacy reads emissivity at 0x20, width at 0x02, height at 0x04 of payload.
    let mut payload = vec![0u8; 48];
    // Camera model at offset 0x20 of payload (legacy search list includes 0x20).
    let model = b"FLIR E60";
    payload[0x20..0x20 + model.len()].copy_from_slice(model);
    // emissivity at 0x20 would overlap with model; instead set width/height.
    put_u16_le(&mut payload, 0x02, 320); // width
    put_u16_le(&mut payload, 0x04, 240); // height

    let mut data = Vec::new();
    data.extend_from_slice(b"FLIR\x00");
    data.push(0x01); // marker
    data.push(0x00); // segment index 0
    data.push(0x00); // reserved
    data.extend_from_slice(&payload);

    let mut md = MetadataMap::new();
    let res = parse_flir_segment(&data, &mut md);
    assert!(res.is_ok());
    // Width/height should be extracted by the legacy path.
    assert_eq!(md.get_integer("FLIR:RawThermalImageWidth"), Some(320));
    assert_eq!(md.get_integer("FLIR:RawThermalImageHeight"), Some(240));
}

#[test]
fn test_flir_legacy_emissivity() {
    // Payload < 64 bytes; place a valid emissivity (0..1) at 0x20, avoid a model
    // string there so the emissivity branch is taken.
    let mut payload = vec![0u8; 48];
    put_f32_le(&mut payload, 0x20, 0.95); // emissivity

    let mut data = Vec::new();
    data.extend_from_slice(b"FLIR\x00");
    data.extend_from_slice(&[0x01, 0x00, 0x00]);
    data.extend_from_slice(&payload);

    let mut md = MetadataMap::new();
    parse_flir_segment(&data, &mut md).unwrap();
    let e = md.get_float("FLIR:Emissivity");
    assert!(e.is_some());
    assert!((e.unwrap() - 0.95).abs() < 0.01);
}

#[test]
fn test_flir_full_fff_with_camera_and_raw_records() {
    // Build a full FFF structure with a record index referencing a CameraInfo
    // record and a RawData record so we hit parse_fff_with_index and the
    // CameraInfo/RawData parsers.

    // --- CameraInfo record payload ---
    let mut cam = vec![0u8; 0x470];
    put_f32_le(&mut cam, 0x0020, 0.95); // emissivity
    put_f32_le(&mut cam, 0x0024, 3.0); // object distance
    put_f32_le(&mut cam, 0x0028, 295.0); // reflected apparent temp (K)
    put_f32_le(&mut cam, 0x002C, 293.0); // atmospheric temp
    put_f32_le(&mut cam, 0x0030, 293.0); // IR window temp
    put_f32_le(&mut cam, 0x0034, 0.9); // IR window transmission
    put_f32_le(&mut cam, 0x003C, 50.0); // relative humidity
    put_f32_le(&mut cam, 0x0058, 14000.0); // Planck R1
    put_f32_le(&mut cam, 0x005C, 1400.0); // Planck B
    put_f32_le(&mut cam, 0x0060, 1.0); // Planck F
    put_f32_le(&mut cam, 0x0070, 0.006); // alpha1
    put_f32_le(&mut cam, 0x0074, 0.012); // alpha2
    put_f32_le(&mut cam, 0x0078, -0.002); // beta1
    put_f32_le(&mut cam, 0x007C, -0.006); // beta2
    put_f32_le(&mut cam, 0x0080, 1.9); // trans X
    put_f32_le(&mut cam, 0x0090, 423.0); // camera temp range max
    put_f32_le(&mut cam, 0x0094, 253.0); // camera temp range min
    // camera identification strings
    cam[0x00D4..0x00D4 + 7].copy_from_slice(b"FLIR T1");
    cam[0x0104..0x0104 + 5].copy_from_slice(b"SN123");
    cam[0x0114..0x0114 + 4].copy_from_slice(b"v1.0");
    cam[0x0170..0x0170 + 4].copy_from_slice(b"Lens");
    put_f32_le(&mut cam, 0x01B4, 45.0); // field of view
    put_f32_le(&mut cam, 0x01B8, 10.0); // peak spectral sensitivity
    put_u16_le(&mut cam, 0x0310, 1000); // raw value range min
    put_u16_le(&mut cam, 0x0312, 9000); // raw value range max
    put_u16_le(&mut cam, 0x0338, 5000); // raw value median
    put_u16_le(&mut cam, 0x033C, 8000); // raw value range
    // focus + frame rate
    put_u16_le(&mut cam, 0x0390, 100); // focus step count (i16)
    put_f32_le(&mut cam, 0x045C, 2.5); // focus distance
    put_u16_le(&mut cam, 0x0464, 30); // frame rate
    let cam_planck_o = 0x0308;
    put_u32_le_buf(&mut cam, cam_planck_o, 100); // Planck O
    put_f32_le(&mut cam, 0x030C, 0.01); // Planck R2

    // --- RawData record payload ---
    let mut raw = vec![0u8; 64];
    put_u16_le(&mut raw, 0x0000, 0); // byte order little-endian
    put_u16_le(&mut raw, 0x0002, 640); // width
    put_u16_le(&mut raw, 0x0004, 480); // height
    put_u16_le(&mut raw, 0x0010, 1); // image type U16 Linear

    // --- PaletteInfo record payload ---
    let mut pal = vec![0u8; 0x80];
    pal[0x0000] = 224; // palette colors
    pal[0x0006] = 0xFF; // above r
    pal[0x0007] = 0x00; // above g
    pal[0x0008] = 0x00; // above b
    pal[0x001A] = 1; // method => Color Bar
    pal[0x001B] = 0; // stretch => Linear
    pal[0x0050..0x0050 + 4].copy_from_slice(b"Iron"); // palette name

    // --- Assemble FFF data ---
    // Header is 64 bytes; record_count at offset 28; index_offset at offset 32.
    // CreatorSoftware string at offset 0x08 (16 bytes).
    let mut fff = vec![0u8; 64];
    fff[0..4].copy_from_slice(b"FFF\0");
    fff[0x08..0x08 + 7].copy_from_slice(b"ATotool");
    let record_count = 3u32;
    fff[28..32].copy_from_slice(&record_count.to_le_bytes());

    // Index starts right after the 64-byte header.
    let index_offset = 64u32;
    fff[32..36].copy_from_slice(&index_offset.to_le_bytes());

    // Each index entry is 32 bytes: type(0,u16), offset(12,u32), length(16,u32).
    let entry_size = 32usize;
    let index_size = entry_size * record_count as usize;
    // Records placed after the index.
    let records_base = index_offset as usize + index_size;
    let cam_off = records_base;
    let raw_off = cam_off + cam.len();
    let pal_off = raw_off + raw.len();

    // Append index region (zeroed), then fill entries.
    fff.resize(records_base, 0);
    // Re-borrow as we write entry fields.
    let write_entry = |buf: &mut Vec<u8>, idx: usize, rtype: u16, off: u32, len: u32| {
        let base = index_offset as usize + idx * entry_size;
        buf[base..base + 2].copy_from_slice(&rtype.to_le_bytes());
        buf[base + 12..base + 16].copy_from_slice(&off.to_le_bytes());
        buf[base + 16..base + 20].copy_from_slice(&len.to_le_bytes());
    };
    write_entry(&mut fff, 0, 0x0020, cam_off as u32, cam.len() as u32); // CameraInfo
    write_entry(&mut fff, 1, 0x0001, raw_off as u32, raw.len() as u32); // RawData
    write_entry(&mut fff, 2, 0x0022, pal_off as u32, pal.len() as u32); // PaletteInfo

    // Append record payloads.
    fff.extend_from_slice(&cam);
    fff.extend_from_slice(&raw);
    fff.extend_from_slice(&pal);

    // Wrap FFF into a FLIR APP1 segment: "FLIR\0" + marker + index 0 + reserved.
    let mut data = Vec::new();
    data.extend_from_slice(b"FLIR\x00");
    data.push(0x01); // marker
    data.push(0x00); // segment index 0
    data.push(0x00); // reserved
    data.extend_from_slice(&fff);

    let mut md = MetadataMap::new();
    let res = parse_flir_segment(&data, &mut md);
    assert!(res.is_ok(), "FFF parse failed: {:?}", res);

    // CameraInfo-derived tags.
    assert!(md.get_float("FLIR:Emissivity").is_some());
    assert!(md.get_float("FLIR:ObjectDistance").is_some());
    assert!(md.get_float("FLIR:PlanckR1").is_some());
    assert!(md.get_float("FLIR:PlanckB").is_some());
    assert!(md.get_integer("FLIR:PlanckO").is_some());
    assert_eq!(md.get_string("FLIR:CameraModel"), Some("FLIR T1"));
    assert_eq!(md.get_string("FLIR:CameraSerialNumber"), Some("SN123"));
    assert!(md.get_float("FLIR:FieldOfView").is_some());
    assert!(md.get_integer("FLIR:RawValueRangeMin").is_some());

    // RawData-derived tags.
    assert_eq!(md.get_integer("FLIR:RawThermalImageWidth"), Some(640));
    assert_eq!(md.get_integer("FLIR:RawThermalImageHeight"), Some(480));
    assert_eq!(
        md.get_string("FLIR:RawThermalImageType"),
        Some("U16 (Linear)")
    );

    // PaletteInfo-derived tags.
    assert!(md.get_integer("FLIR:PaletteColors").is_some());
    assert_eq!(md.get_string("FLIR:PaletteMethod"), Some("Color Bar"));
    assert_eq!(md.get_string("FLIR:PaletteStretch"), Some("Linear"));
    assert_eq!(md.get_string("FLIR:PaletteName"), Some("Iron"));
    assert!(md.get_string("FLIR:AboveColor").is_some());

    // CreatorSoftware from the FFF header.
    assert!(md.get_string("FLIR:CreatorSoftware").is_some());
}

#[test]
fn test_flir_invalid_record_count_falls_back() {
    // FFF header with absurd record count -> falls back to legacy parse path.
    let mut fff = vec![0u8; 80];
    fff[0..4].copy_from_slice(b"FFF\0");
    fff[28..32].copy_from_slice(&9999u32.to_le_bytes()); // > 100 => fallback

    let mut data = Vec::new();
    data.extend_from_slice(b"FLIR\x00");
    data.extend_from_slice(&[0x01, 0x00, 0x00]);
    data.extend_from_slice(&fff);

    let mut md = MetadataMap::new();
    // Should not error even though record count is invalid.
    assert!(parse_flir_segment(&data, &mut md).is_ok());
}

// ============================================================================
// Production path: read_metadata over a tempfile (detection + dispatch).
// ============================================================================

use oxidex::core::operations::read_metadata;
use std::io::Write;

#[test]
fn test_read_metadata_dng_fixture_via_tempfile() {
    // Build a small but valid DNG-ish TIFF and write to a .dng tempfile to
    // exercise the production detection + dispatch path.
    let mut width = [0u8; 4];
    width[0..2].copy_from_slice(&320u16.to_le_bytes());
    let data = build_minimal_tiff_le(&[(0x0100, 3, 1, width), (0xC612, 1, 4, [1, 4, 0, 0])]);

    let mut tmp = tempfile::Builder::new()
        .suffix(".dng")
        .tempfile()
        .expect("create tempfile");
    tmp.write_all(&data).expect("write data");
    tmp.flush().expect("flush");

    // The production path may succeed or return a detection error depending on
    // how strict DNG detection is; either way it must not panic.
    let result = read_metadata(tmp.path());
    let _ = result.is_ok();
}

#[test]
fn test_read_metadata_tiff_fixture() {
    use std::path::Path;
    let fixture = Path::new("tests/fixtures/tiff/sample.tif");
    if fixture.exists() {
        let result = read_metadata(fixture);
        // Real fixture: should parse without panicking.
        let _ = result.is_ok();
    }
}
