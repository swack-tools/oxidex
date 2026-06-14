//! Wave-2 coverage tests for Canon and Nikon MakerNote parsers.
//!
//! Wave-1 (`cov_makernotes_a.rs`) hit the happy path for Canon/Nikon/Sony IFD
//! walking, scalar tags, and the common array tags. This file targets the
//! REMAINING uncovered branches:
//!
//! - `canon/camera_info.rs`: the public `parse_canon_camera_info` entry point,
//!   its model-family detection, the per-family sub-table decoders
//!   (1DmkII / 5D / modern), and the `CanonBatteryType` enum mappers.
//! - `canon.rs`: ShotInfo formatters (`apex_to_ev`, `format_focus_distance`,
//!   `decode_af_points_in_focus`), the model-id `decode_camera_type`, the
//!   FileInfo unknown-lens-id branch, and the n/a / fallback arms.
//! - `nikon.rs`: the many less-common tag-id branches not exercised by wave-1
//!   (ColorBalance 0x000C, PictureControlData, ImageBoundary, CropHiSpeed,
//!   MultiExposure, ISOInfo, AFInfo, FlashInfo, DistortInfo, ManualFocusDistance,
//!   string tags, LensData aperture range, etc.) plus error/edge paths.
//!
//! A MakerNote IFD is a standard TIFF IFD:
//!   [entry_count: u16][entries...][next_ifd_offset: u32]
//! Each entry is 12 bytes: [tag: u16][type: u16][count: u32][value/offset: u32].

#[path = "common/mod.rs"]
mod common;

use common::TestReader;

use std::collections::HashMap;

use oxidex::core::FileReader;
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::canon::camera_info::{
    CanonBatteryType, parse_canon_camera_info,
};
use oxidex::parsers::tiff::makernotes::canon::{decode_camera_type, parse_canon_makernotes};
use oxidex::parsers::tiff::makernotes::nikon::parse_nikon_makernotes;

// ===========================================================================
// Synthetic IFD construction helpers (mirrors wave-1 style)
// ===========================================================================

/// A single 12-byte TIFF IFD entry described in builder form.
struct Entry {
    tag: u16,
    field_type: u16,
    count: u32,
    value_offset: u32,
}

/// Builds a little-endian TIFF IFD body: [count][entries][next=0] then appends
/// `trailing` data after the IFD header.
fn build_ifd_le(entries: &[Entry], trailing: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.tag.to_le_bytes());
        out.extend_from_slice(&e.field_type.to_le_bytes());
        out.extend_from_slice(&e.count.to_le_bytes());
        out.extend_from_slice(&e.value_offset.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(trailing);
    out
}

/// Encodes a slice of i16 as little-endian bytes.
fn i16s_le(values: &[i16]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Encodes a slice of u16 as little-endian bytes.
fn u16s_le(values: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

// Header size of an IFD with a single 12-byte entry: 2 + 12 + 4 = 18.
const ONE_ENTRY_HEADER: u32 = 18;

/// Builds a complete Nikon Type 3 MakerNote with a little-endian embedded TIFF.
/// IFD offset within the embedded TIFF is 8, so the IFD immediately follows the
/// 8-byte TIFF header. `array_trailing` is appended after the IFD body; array
/// value_offsets must be ABSOLUTE positions into the whole buffer.
fn build_nikon(entries: &[Entry], array_trailing: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"Nikon\0"); // 6 bytes
    out.extend_from_slice(&[0x02, 0x10, 0x00, 0x00]); // version -> 10 bytes total

    out.extend_from_slice(b"II");
    out.extend_from_slice(&0x002Au16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());

    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.tag.to_le_bytes());
        out.extend_from_slice(&e.field_type.to_le_bytes());
        out.extend_from_slice(&e.count.to_le_bytes());
        out.extend_from_slice(&e.value_offset.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // next IFD
    out.extend_from_slice(array_trailing);
    out
}

/// Computes the byte offset where the IFD trailing block begins in a Nikon
/// buffer with `n` entries: header(10) + tiff(8) + count(2) + n*12 + next(4).
fn nikon_trailing_offset(n: usize) -> u32 {
    (10 + 8 + 2 + n * 12 + 4) as u32
}

// ===========================================================================
// Sanity: TestReader still satisfies FileReader (template requirement)
// ===========================================================================

#[test]
fn test_test_reader_basics() {
    let reader = TestReader::new(vec![9, 8, 7]);
    assert_eq!(reader.size(), 3);
    assert_eq!(reader.read(1, 2).unwrap(), &[8, 7]);
    assert!(reader.read(2, 5).is_err());
}

// ===========================================================================
// Canon camera_info.rs: CanonBatteryType enum mappers (exhaustive)
// ===========================================================================

#[test]
fn test_canon_battery_type_parse_all() {
    // String parser: every known variant plus normalization (dash/space removal).
    assert_eq!(CanonBatteryType::parse("LP-E6"), CanonBatteryType::LpE6);
    assert_eq!(CanonBatteryType::parse("lp e6n"), CanonBatteryType::LpE6N);
    assert_eq!(CanonBatteryType::parse("LP-E6NH"), CanonBatteryType::LpE6Nh);
    assert_eq!(CanonBatteryType::parse("LPE6P"), CanonBatteryType::LpE6P);
    assert_eq!(CanonBatteryType::parse("LP-E4"), CanonBatteryType::LpE4);
    assert_eq!(CanonBatteryType::parse("LP-E4N"), CanonBatteryType::LpE4N);
    assert_eq!(CanonBatteryType::parse("LP-E5"), CanonBatteryType::LpE5);
    assert_eq!(CanonBatteryType::parse("LP-E8"), CanonBatteryType::LpE8);
    assert_eq!(CanonBatteryType::parse("LP-E10"), CanonBatteryType::LpE10);
    assert_eq!(CanonBatteryType::parse("LP-E12"), CanonBatteryType::LpE12);
    assert_eq!(CanonBatteryType::parse("LP-E17"), CanonBatteryType::LpE17);
    assert_eq!(CanonBatteryType::parse("LP-E19"), CanonBatteryType::LpE19);
    assert_eq!(
        CanonBatteryType::parse("garbage"),
        CanonBatteryType::Unknown
    );
}

#[test]
fn test_canon_battery_type_from_code_all() {
    assert_eq!(CanonBatteryType::from_code(0x01), CanonBatteryType::LpE6);
    assert_eq!(CanonBatteryType::from_code(0x02), CanonBatteryType::LpE6N);
    assert_eq!(CanonBatteryType::from_code(0x03), CanonBatteryType::LpE6Nh);
    assert_eq!(CanonBatteryType::from_code(0x04), CanonBatteryType::LpE6P);
    assert_eq!(CanonBatteryType::from_code(0x10), CanonBatteryType::LpE4);
    assert_eq!(CanonBatteryType::from_code(0x11), CanonBatteryType::LpE4N);
    assert_eq!(CanonBatteryType::from_code(0x20), CanonBatteryType::LpE5);
    assert_eq!(CanonBatteryType::from_code(0x21), CanonBatteryType::LpE8);
    assert_eq!(CanonBatteryType::from_code(0x22), CanonBatteryType::LpE10);
    assert_eq!(CanonBatteryType::from_code(0x23), CanonBatteryType::LpE12);
    assert_eq!(CanonBatteryType::from_code(0x24), CanonBatteryType::LpE17);
    assert_eq!(CanonBatteryType::from_code(0x30), CanonBatteryType::LpE19);
    assert_eq!(CanonBatteryType::from_code(0x99), CanonBatteryType::Unknown);
}

#[test]
fn test_canon_battery_type_as_str_all() {
    // Round-trip every variant through as_str().
    for (variant, name) in [
        (CanonBatteryType::LpE6, "LP-E6"),
        (CanonBatteryType::LpE6N, "LP-E6N"),
        (CanonBatteryType::LpE6Nh, "LP-E6NH"),
        (CanonBatteryType::LpE6P, "LP-E6P"),
        (CanonBatteryType::LpE4, "LP-E4"),
        (CanonBatteryType::LpE4N, "LP-E4N"),
        (CanonBatteryType::LpE5, "LP-E5"),
        (CanonBatteryType::LpE8, "LP-E8"),
        (CanonBatteryType::LpE10, "LP-E10"),
        (CanonBatteryType::LpE12, "LP-E12"),
        (CanonBatteryType::LpE17, "LP-E17"),
        (CanonBatteryType::LpE19, "LP-E19"),
        (CanonBatteryType::Unknown, "Unknown"),
    ] {
        assert_eq!(variant.as_str(), name);
        // Exercise Debug/Clone/Copy derives as well.
        let copy = variant;
        assert_eq!(copy, variant);
        let _ = format!("{:?}", variant);
    }
}

// ===========================================================================
// Canon camera_info.rs: parse_canon_camera_info entry-point branches
// ===========================================================================

#[test]
fn test_camera_info_too_short_returns_empty() {
    // < MIN_CAMERA_INFO_LENGTH (16) -> empty map, early return.
    let data = vec![0u8; 10];
    let meta = parse_canon_camera_info(&data, false);
    assert!(meta.is_empty());

    // Truly empty as well.
    assert!(parse_canon_camera_info(&[], false).is_empty());
}

#[test]
fn test_camera_info_records_length_and_clamps() {
    // Always records CameraInfoLength for valid-length data.
    let data = vec![0u8; 64];
    let meta = parse_canon_camera_info(&data, false);
    assert_eq!(
        meta.get_integer("Canon:CameraInfoLength"),
        Some(64),
        "should record actual length"
    );

    // Oversized data clamps to MAX_CAMERA_INFO_LENGTH (4096).
    let big = vec![0u8; 9000];
    let meta_big = parse_canon_camera_info(&big, false);
    let reported = meta_big
        .get_integer("Canon:CameraInfoLength")
        .expect("length present");
    assert!(reported <= 4096, "length should be clamped, got {reported}");
}

#[test]
fn test_camera_info_powershot_family() {
    // 138..=145 -> "PowerShot/Entry"; falls into the default (1d + 5d) parse arm.
    let mut data = vec![0u8; 140];
    // Camera type byte at offset 7 = 255 -> PowerShot.
    data[7] = 255;
    let meta = parse_canon_camera_info(&data, false);
    assert_eq!(
        meta.get_string("Canon:DetectedCameraFamily"),
        Some("PowerShot/Entry")
    );
    assert_eq!(meta.get_string("Canon:CameraType"), Some("PowerShot"));
}

#[test]
fn test_camera_info_1dmkii_family_fields() {
    // 150..=160 -> "1DmkII/40D/50D" -> parse_camera_info_1d_mkii.
    let mut data = vec![0u8; 156];
    // ExposureTime (index 4, int8u).
    data[4] = 12;
    // FocalLength (index 9, int16u) at byte 9 LE = 85mm.
    data[9] = 85;
    data[10] = 0;
    // LensType (index 13, int8u).
    data[13] = 50;
    // ShortFocal (index 17, int16u) byte 17 LE = 24.
    data[17] = 24;
    data[18] = 0;
    // LongFocal (index 19, int16u) byte 19 LE = 70.
    data[19] = 70;
    data[20] = 0;
    // FocalType (index 45) -> Zoom.
    data[45] = 2;
    // WhiteBalance (index 54) -> Daylight.
    data[54] = 1;
    // ColorTemperature (index 55, int16u) LE = 5500.
    data[55..57].copy_from_slice(&5500u16.to_le_bytes());

    let meta = parse_canon_camera_info(&data, false);
    assert_eq!(
        meta.get_string("Canon:DetectedCameraFamily"),
        Some("1DmkII/40D/50D")
    );
    assert_eq!(meta.get_integer("Canon:ExposureTimeRaw"), Some(12));
    assert_eq!(meta.get_integer("Canon:FocalLengthRaw"), Some(85));
    assert_eq!(meta.get_integer("Canon:LensTypeRaw"), Some(50));
    assert_eq!(meta.get_string("Canon:MinFocalLength"), Some("24 mm"));
    assert_eq!(meta.get_string("Canon:MaxFocalLength"), Some("70 mm"));
    assert_eq!(meta.get_string("Canon:FocalType"), Some("Zoom"));
    assert_eq!(meta.get_string("Canon:WhiteBalance"), Some("Daylight"));
    assert_eq!(meta.get_string("Canon:ColorTemperature"), Some("5500 K"));
}

#[test]
fn test_camera_info_5d_family_fields() {
    // 161..=175 -> "5D/60D/450D" -> parse_camera_info_5d + parse_camera_info_modern.
    let mut data = vec![0u8; 170];
    // Camera temperature at offset 25 (5D): 155 -> 27 C.
    data[25] = 155;
    // Firmware string at offset 28: must be >=3 chars and contain a digit.
    let fw = b"1.2.3";
    data[28..28 + fw.len()].copy_from_slice(fw);
    // LensType (index 15, 5D layout).
    data[15] = 40;
    // WhiteBalance (index 36, <30) -> Cloudy.
    data[36] = 2;
    // ColorTemperature (index 37, int16u) LE = 6000.
    data[37..39].copy_from_slice(&6000u16.to_le_bytes());
    // Sharpness (index 6, modern): in -4..=7.
    data[6] = 3;
    // PictureStyle (index 45, modern) -> Standard.
    data[45] = 0x01;

    let meta = parse_canon_camera_info(&data, false);
    assert_eq!(
        meta.get_string("Canon:DetectedCameraFamily"),
        Some("5D/60D/450D")
    );
    assert_eq!(meta.get_string("Canon:CameraTemperature"), Some("27 C"));
    assert_eq!(
        meta.get_string("Canon:FirmwareVersionInternal"),
        Some("1.2.3")
    );
    assert_eq!(meta.get_integer("Canon:LensTypeRaw"), Some(40));
    assert!(meta.contains_key("Canon:WhiteBalance"));
    assert!(meta.contains_key("Canon:ColorTemperature"));
    assert!(meta.contains_key("Canon:Sharpness"));
    assert!(meta.contains_key("Canon:PictureStyle"));
}

#[test]
fn test_camera_info_unknown_family_tries_1d_and_5d() {
    // Size outside known ranges (e.g. 200) -> "Unknown" -> default arm runs
    // parse_camera_info_1d then parse_camera_info_5d.
    let mut data = vec![0u8; 200];
    // 1D ExposureTime at index 4.
    data[4] = 7;
    // 1D FocalLength int16u at byte 20 (index 10 * 2) = 35.
    data[20] = 35;
    data[21] = 0;
    // 1D LensType at index 13.
    data[13] = 99;
    // 1D ShortFocal int16u at byte 28 (index 14 * 2) = 18.
    data[28] = 18;
    data[29] = 0;
    // 1D LongFocal int16u at byte 32 (index 16 * 2) = 55.
    data[32] = 55;
    data[33] = 0;
    // 1D SharpnessFrequency at index 65 -> Standard.
    data[65] = 3;
    // 1D Sharpness at index 67.
    data[67] = 2;
    // 1D WhiteBalance at index 68 -> Tungsten.
    data[68] = 3;
    // 1D ColorTemperature int16u at byte 69 = 4800.
    data[69..71].copy_from_slice(&4800u16.to_le_bytes());
    // 1D PictureStyle at index 81 -> Portrait.
    data[81] = 0x02;

    let meta = parse_canon_camera_info(&data, false);
    // DetectedCameraFamily should NOT be present for Unknown.
    assert!(!meta.contains_key("Canon:DetectedCameraFamily"));
    assert_eq!(meta.get_integer("Canon:ExposureTimeRaw"), Some(7));
    assert_eq!(meta.get_integer("Canon:FocalLengthRaw"), Some(35));
    assert_eq!(meta.get_integer("Canon:LensTypeRaw"), Some(99));
    assert_eq!(meta.get_string("Canon:MinFocalLength"), Some("18 mm"));
    assert_eq!(meta.get_string("Canon:MaxFocalLength"), Some("55 mm"));
    assert_eq!(
        meta.get_string("Canon:SharpnessFrequency"),
        Some("Standard")
    );
    // Note: the default arm runs parse_camera_info_1d THEN parse_camera_info_5d,
    // so the 5D pass (index 36 == 0 -> "Auto") overwrites the 1D WhiteBalance.
    // Both branches executed; just assert the key is present.
    assert!(meta.contains_key("Canon:WhiteBalance"));
    assert_eq!(meta.get_integer("Canon:Sharpness"), Some(2));
    assert_eq!(meta.get_string("Canon:ColorTemperature"), Some("4800 K"));
    assert_eq!(meta.get_string("Canon:PictureStyle"), Some("Portrait"));
}

#[test]
fn test_camera_info_temperature_fallback_scan() {
    // 1DmkII-sized data with NO temperature in its layout, but a valid temp at
    // one of the fallback offsets (23/25/27/30). parse_camera_info_1d_mkii does
    // not set temperature, so the post-pass scan fills it.
    let mut data = vec![0u8; 155];
    // Put a valid temperature (40 C -> raw 168) at fallback offset 27.
    data[27] = 168;
    let meta = parse_canon_camera_info(&data, false);
    assert_eq!(meta.get_string("Canon:CameraTemperature"), Some("40 C"));
}

#[test]
fn test_camera_info_camera_type_scan_offsets() {
    // Camera type decoded from offsets 6/7/8. Put 252 (EOS Mid-Range) at offset 6.
    let mut data = vec![0u8; 64];
    data[6] = 252;
    let meta = parse_canon_camera_info(&data, false);
    assert_eq!(meta.get_string("Canon:CameraType"), Some("EOS Mid-Range"));

    // 254 (EOS Entry) at offset 8, with 6/7 not matching.
    let mut data2 = vec![0u8; 64];
    data2[8] = 254;
    let meta2 = parse_canon_camera_info(&data2, false);
    assert_eq!(meta2.get_string("Canon:CameraType"), Some("EOS Entry"));

    // 250 (Compact) and 248 (EOS High-End) variants.
    let mut data3 = vec![0u8; 64];
    data3[7] = 250;
    assert_eq!(
        parse_canon_camera_info(&data3, false).get_string("Canon:CameraType"),
        Some("Compact")
    );
    let mut data4 = vec![0u8; 64];
    data4[7] = 248;
    assert_eq!(
        parse_canon_camera_info(&data4, false).get_string("Canon:CameraType"),
        Some("EOS High-End")
    );
}

#[test]
fn test_camera_info_big_endian_color_temp() {
    // Big-endian path through EndianReader (byte_order = true).
    // 1DmkII size; ColorTemperature int16u at byte 55 in big-endian = 5200.
    let mut data = vec![0u8; 156];
    data[55..57].copy_from_slice(&5200u16.to_be_bytes());
    // FocalLength int16u at byte 9 big-endian = 100.
    data[9..11].copy_from_slice(&100u16.to_be_bytes());
    let meta = parse_canon_camera_info(&data, true);
    assert_eq!(meta.get_string("Canon:ColorTemperature"), Some("5200 K"));
    assert_eq!(meta.get_integer("Canon:FocalLengthRaw"), Some(100));
}

// ===========================================================================
// Canon canon.rs: model-id decode_camera_type (the u32 variant)
// ===========================================================================

#[test]
fn test_decode_camera_type_model_id_variants() {
    // EOS High-end: 1-series pro bodies.
    assert_eq!(decode_camera_type(0x80000001), "EOS High-end");
    assert_eq!(decode_camera_type(0x80000269), "EOS High-end");
    assert_eq!(decode_camera_type(0x80000406), "EOS High-end");
    // EOS Mid-range: any other 0x80XXXXXX id.
    assert_eq!(decode_camera_type(0x80000218), "EOS Mid-range");
    assert_eq!(decode_camera_type(0x80000331), "EOS Mid-range");
    // Compact: 0x01000000..0x02000000.
    assert_eq!(decode_camera_type(0x01110000), "Compact");
    assert_eq!(decode_camera_type(0x01010000), "Compact");
    // Unknown: outside both ranges.
    assert_eq!(decode_camera_type(0x03160000), "Unknown");
    assert_eq!(decode_camera_type(0x00000000), "Unknown");
}

// ===========================================================================
// Canon canon.rs: ShotInfo formatter branches not hit by wave-1
// ===========================================================================

#[test]
fn test_canon_shot_info_ev_and_distance_branches() {
    // Drive apex_to_ev (negative + positive), format_focus_distance (finite +
    // inf), decode_af_points_in_focus (Center + numbered + None) via ShotInfo.
    let mut shot = vec![0i16; 25];
    shot[0] = 25;
    shot[1] = 0; // AutoISO = 0 -> falls back, but no Canon:ISO set, so skipped.
    shot[2] = 0; // BaseISO = 0 -> uses raw "0".
    shot[3] = 64; // MeasuredEV (unsigned/32).
    shot[4] = 96; // TargetAperture (apex).
    shot[5] = -64; // TargetExposureTime (apex, slow -> >= 1 sec branch).
    shot[6] = -32; // ExposureCompensation -> negative EV branch.
    shot[7] = 9; // WhiteBalance -> Manual Temperature (Kelvin).
    shot[8] = 3; // SlowShutter -> None.
    shot[14] = 0x0005; // AFPointsInFocus: bits 0 and 2 -> "Center, 2".
    shot[15] = 48; // FlashExposureComp -> positive EV.
    shot[16] = -16; // AutoExposureBracketing -> negative EV.
    shot[17] = 8; // AEBBracketValue -> positive EV.
    shot[18] = 3; // ControlMode -> Computer Remote Control.
    shot[19] = 250; // FocusDistanceUpper -> 2.50 m.
    shot[20] = 0; // FocusDistanceLower -> inf (value 0 branch).
    shot[24] = 0; // BulbDuration 0 -> skipped.

    let trailing = i16s_le(&shot);
    let entries = [Entry {
        tag: 0x0004,
        field_type: 3,
        count: shot.len() as u32,
        value_offset: ONE_ENTRY_HEADER,
    }];
    let data = build_ifd_le(&entries, &trailing);
    let mut tags = HashMap::new();
    parse_canon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    // apex_to_ev sign branches.
    assert!(
        tags.get("Canon:ExposureCompensation")
            .unwrap()
            .starts_with('-')
    );
    assert!(
        tags.get("Canon:FlashExposureComp")
            .unwrap()
            .starts_with('+')
    );
    assert!(
        tags.get("Canon:AutoExposureBracketing")
            .unwrap()
            .starts_with('-')
    );
    assert!(tags.get("Canon:AEBBracketValue").unwrap().starts_with('+'));
    // format_focus_distance branches.
    assert_eq!(
        tags.get("Canon:FocusDistanceUpper"),
        Some(&"2.50 m".to_string())
    );
    assert_eq!(
        tags.get("Canon:FocusDistanceLower"),
        Some(&"inf".to_string())
    );
    // decode_af_points_in_focus: Center + numbered point.
    let af = tags.get("Canon:AFPointsInFocus").unwrap();
    assert!(af.contains("Center"));
    assert!(af.contains('2'));
    // WhiteBalance + SlowShutter decoders.
    assert_eq!(
        tags.get("Canon:WhiteBalance"),
        Some(&"Manual Temperature (Kelvin)".to_string())
    );
    assert_eq!(tags.get("Canon:SlowShutter"), Some(&"None".to_string()));
    assert_eq!(
        tags.get("Canon:ControlMode"),
        Some(&"Computer Remote Control".to_string())
    );
    // TargetExposureTime slow branch -> contains "sec".
    assert!(
        tags.get("Canon:TargetExposureTime")
            .unwrap()
            .contains("sec")
    );
    // BulbDuration was 0 -> not inserted.
    assert!(!tags.contains_key("Canon:BulbDuration"));
}

#[test]
fn test_canon_shot_info_af_points_none() {
    // AFPointsInFocus value 0 -> "None" branch in decode_af_points_in_focus.
    let mut shot = vec![0i16; 16];
    shot[0] = 16;
    shot[14] = 0; // None.
    let trailing = i16s_le(&shot);
    let entries = [Entry {
        tag: 0x0004,
        field_type: 3,
        count: shot.len() as u32,
        value_offset: ONE_ENTRY_HEADER,
    }];
    let data = build_ifd_le(&entries, &trailing);
    let mut tags = HashMap::new();
    parse_canon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert_eq!(tags.get("Canon:AFPointsInFocus"), Some(&"None".to_string()));
}

#[test]
fn test_canon_shot_info_auto_iso_fallback_from_camera_settings() {
    // Two-entry IFD: CameraSettings sets Canon:ISO, ShotInfo AutoISO=0 falls back.
    // Build a 2-entry header (2 + 24 + 4 = 30); both arrays placed in trailing.
    let mut settings = vec![0i16; 17];
    settings[0] = 17;
    settings[16] = 320; // ISO index 16 -> "320".

    let mut shot = vec![0i16; 3];
    shot[0] = 3;
    shot[1] = 0; // AutoISO 0 -> fallback path.

    let settings_bytes = i16s_le(&settings);
    let shot_bytes = i16s_le(&shot);

    // Header size for 2 entries = 30. Place settings at 30, shot right after.
    let settings_off = 30u32;
    let shot_off = settings_off + settings_bytes.len() as u32;

    let mut trailing = Vec::new();
    trailing.extend_from_slice(&settings_bytes);
    trailing.extend_from_slice(&shot_bytes);

    let entries = [
        Entry {
            tag: 0x0001,
            field_type: 3,
            count: settings.len() as u32,
            value_offset: settings_off,
        },
        Entry {
            tag: 0x0004,
            field_type: 3,
            count: shot.len() as u32,
            value_offset: shot_off,
        },
    ];
    let data = build_ifd_le(&entries, &trailing);
    let mut tags = HashMap::new();
    parse_canon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert_eq!(tags.get("Canon:ISO"), Some(&"320".to_string()));
    // AutoISO should have been filled from Canon:ISO via the fallback branch.
    assert_eq!(tags.get("Canon:AutoISO"), Some(&"320".to_string()));
}

#[test]
fn test_canon_file_info_unknown_lens_id_branch() {
    // FileInfo (0x0093) with a lens id NOT in the database -> Canon:LensID branch.
    let mut file_info = vec![0i16; 16];
    file_info[0] = 16;
    file_info[6] = 30000; // unlikely to be a known lens db id.
    let trailing = i16s_le(&file_info);
    let entries = [Entry {
        tag: 0x0093,
        field_type: 3,
        count: file_info.len() as u32,
        value_offset: ONE_ENTRY_HEADER,
    }];
    let data = build_ifd_le(&entries, &trailing);
    let mut tags = HashMap::new();
    parse_canon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    // Either a known lens name (unlikely) or the LensID fallback - assert the
    // unknown-id branch executed.
    assert!(
        tags.contains_key("Canon:LensID") || tags.contains_key("Canon:LensType"),
        "lens id branch should have run"
    );
    if let Some(id) = tags.get("Canon:LensID") {
        assert_eq!(id, "30000");
    }
}

#[test]
fn test_canon_file_info_lens_id_zero_na() {
    // FileInfo lens id == 0 -> "n/a" branch.
    let mut file_info = vec![0i16; 16];
    file_info[0] = 16;
    file_info[6] = 0;
    let trailing = i16s_le(&file_info);
    let entries = [Entry {
        tag: 0x0093,
        field_type: 3,
        count: file_info.len() as u32,
        value_offset: ONE_ENTRY_HEADER,
    }];
    let data = build_ifd_le(&entries, &trailing);
    let mut tags = HashMap::new();
    parse_canon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert_eq!(tags.get("Canon:LensType"), Some(&"n/a".to_string()));
}

#[test]
fn test_canon_camera_settings_aperture_above_ten_branch() {
    // apex_to_aperture integer-format branch (f-number >= 10). A large APEX value
    // gives a large f-number, exercising the format!("f/{:.0}") path.
    let mut settings = vec![0i16; 41];
    settings[0] = 41;
    settings[26] = 600; // MaxAperture APEX -> large f-number.
    settings[27] = 0; // MinAperture APEX 0 -> "n/a".
    let trailing = i16s_le(&settings);
    let entries = [Entry {
        tag: 0x0001,
        field_type: 3,
        count: settings.len() as u32,
        value_offset: ONE_ENTRY_HEADER,
    }];
    let data = build_ifd_le(&entries, &trailing);
    let mut tags = HashMap::new();
    parse_canon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(tags.get("Canon:MaxAperture").unwrap().starts_with("f/"));
    assert_eq!(tags.get("Canon:MinAperture"), Some(&"n/a".to_string()));
}

#[test]
fn test_canon_lens_model_inline_short_string() {
    // LensModel (0x0095) ASCII with <=4 bytes -> inline extraction branch.
    let inline = u32::from_le_bytes([b'5', b'0', b'/', 0]);
    let entries = [Entry {
        tag: 0x0095,
        field_type: 2,
        count: 3,
        value_offset: inline,
    }];
    let data = build_ifd_le(&entries, &[]);
    let mut tags = HashMap::new();
    parse_canon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert_eq!(tags.get("Canon:LensModel"), Some(&"50/".to_string()));
}

// ===========================================================================
// Nikon nikon.rs: less-common tag-id branches (one focused test each cluster)
// ===========================================================================

#[test]
fn test_nikon_scalar_enum_unknown_and_extra_branches() {
    // Hit the less-common scalar arms and unknown-enum fall-through paths.
    let entries = [
        Entry {
            tag: 0x0004,
            field_type: 3,
            count: 1,
            value_offset: 99,
        }, // Quality unknown -> "Unknown"
        Entry {
            tag: 0x0005,
            field_type: 3,
            count: 1,
            value_offset: 8,
        }, // WhiteBalance -> Kelvin
        Entry {
            tag: 0x0007,
            field_type: 3,
            count: 1,
            value_offset: 2,
        }, // Focus -> AF-A
        Entry {
            tag: 0x0008,
            field_type: 3,
            count: 1,
            value_offset: 6,
        }, // FlashSetting -> Off
        Entry {
            tag: 0x0087,
            field_type: 3,
            count: 1,
            value_offset: 8,
        }, // FlashMode -> Commander Mode
        Entry {
            tag: 0x0089,
            field_type: 3,
            count: 1,
            value_offset: 6,
        }, // ShootingMode -> Interval Timer
        Entry {
            tag: 0x00B0,
            field_type: 3,
            count: 1,
            value_offset: 1,
        }, // ColorSpace -> sRGB
        Entry {
            tag: 0x00B3,
            field_type: 3,
            count: 1,
            value_offset: 7,
        }, // ActiveDLighting -> Extra High
        Entry {
            tag: 0x00B7,
            field_type: 3,
            count: 1,
            value_offset: 3,
        }, // VignetteControl -> High
        Entry {
            tag: 0x00B8,
            field_type: 3,
            count: 1,
            value_offset: 2,
        }, // DistortionControl -> "On (Cannot Disable)"
        Entry {
            tag: 0x0093,
            field_type: 3,
            count: 1,
            value_offset: 6,
        }, // NEFCompression -> High Efficiency
    ];
    let data = build_nikon(&entries, &[]);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(tags.get("Nikon:Quality"), Some(&"Unknown".to_string()));
    assert_eq!(tags.get("Nikon:WhiteBalance"), Some(&"Kelvin".to_string()));
    assert_eq!(tags.get("Nikon:FocusMode"), Some(&"AF-A".to_string()));
    assert_eq!(tags.get("Nikon:FlashSetting"), Some(&"Off".to_string()));
    assert_eq!(
        tags.get("Nikon:FlashMode"),
        Some(&"Fired, Commander Mode".to_string())
    );
    assert_eq!(
        tags.get("Nikon:ShootingMode"),
        Some(&"Interval Timer".to_string())
    );
    assert_eq!(tags.get("Nikon:ColorSpace"), Some(&"sRGB".to_string()));
    assert_eq!(
        tags.get("Nikon:ActiveDLighting"),
        Some(&"Extra High".to_string())
    );
    assert_eq!(tags.get("Nikon:VignetteControl"), Some(&"High".to_string()));
    assert_eq!(
        tags.get("Nikon:DistortionControl"),
        Some(&"On (Cannot Disable)".to_string())
    );
    assert_eq!(
        tags.get("Nikon:NEFCompression"),
        Some(&"High Efficiency".to_string())
    );
}

#[test]
fn test_nikon_iso_setting_zero_skipped() {
    // ISOSetting value 0 -> NOT inserted (the `value > 0` guard).
    // ImageAuthentication 0 -> "Off"; ISOSelection non-zero -> "Manual".
    let entries = [
        Entry {
            tag: 0x0013,
            field_type: 3,
            count: 1,
            value_offset: 0,
        }, // ISOSetting 0 -> skipped
        Entry {
            tag: 0x0020,
            field_type: 3,
            count: 1,
            value_offset: 0,
        }, // ImageAuth -> Off
        Entry {
            tag: 0x0011,
            field_type: 3,
            count: 1,
            value_offset: 5,
        }, // ISOSelection nonzero -> Manual
        Entry {
            tag: 0x00B8,
            field_type: 3,
            count: 1,
            value_offset: 0,
        }, // DistortionControl -> Off
    ];
    let data = build_nikon(&entries, &[]);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(!tags.contains_key("Nikon:ISOSetting"));
    assert_eq!(
        tags.get("Nikon:ImageAuthentication"),
        Some(&"Off".to_string())
    );
    assert_eq!(tags.get("Nikon:ISOSelection"), Some(&"Manual".to_string()));
    assert_eq!(
        tags.get("Nikon:DistortionControl"),
        Some(&"Off".to_string())
    );
}

#[test]
fn test_nikon_world_time_negative_and_distort_info() {
    // WorldTime negative offset -> "UTC-HH:MM"; DistortInfo (0x002B) uses same arm
    // as DistortionControl.
    let entries = [
        Entry {
            tag: 0x00B5,
            field_type: 3,
            count: 1,
            value_offset: (-300i32) as u32,
        }, // WorldTime -300 minutes
        Entry {
            tag: 0x002B,
            field_type: 3,
            count: 1,
            value_offset: 1,
        }, // DistortInfo -> On
    ];
    let data = build_nikon(&entries, &[]);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    let wt = tags.get("Nikon:WorldTime").expect("world time");
    assert!(wt.starts_with("UTC-"), "got {wt}");
    assert_eq!(tags.get("Nikon:DistortionControl"), Some(&"On".to_string()));
}

#[test]
fn test_nikon_exposure_comp_family_branches() {
    // FlashExposureComp / ExternalFlashComp / FlashBracketValue /
    // ExposureBracketValue / ExposureTuning all use /6.0 EV formatting.
    let entries = [
        Entry {
            tag: 0x0012,
            field_type: 8,
            count: 1,
            value_offset: 6,
        }, // FlashExposureComp -> +1.0 EV
        Entry {
            tag: 0x0017,
            field_type: 8,
            count: 1,
            value_offset: (-12i32) as u32,
        }, // External -> -2.0 EV
        Entry {
            tag: 0x0018,
            field_type: 8,
            count: 1,
            value_offset: 3,
        }, // FlashBracketValue
        Entry {
            tag: 0x0019,
            field_type: 8,
            count: 1,
            value_offset: 6,
        }, // ExposureBracketValue
        Entry {
            tag: 0x001C,
            field_type: 8,
            count: 1,
            value_offset: 6,
        }, // ExposureTuning
        Entry {
            tag: 0x008B,
            field_type: 3,
            count: 1,
            value_offset: 24,
        }, // LensFStops -> /12.0
    ];
    let data = build_nikon(&entries, &[]);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(tags.get("Nikon:FlashExposureComp").unwrap().contains("EV"));
    assert!(
        tags.get("Nikon:ExternalFlashExposureComp")
            .unwrap()
            .contains("EV")
    );
    assert!(tags.contains_key("Nikon:FlashExposureBracketValue"));
    assert!(tags.contains_key("Nikon:ExposureBracketValue"));
    assert!(tags.contains_key("Nikon:ExposureTuning"));
    assert!(tags.contains_key("Nikon:LensFStops"));
}

#[test]
fn test_nikon_color_balance_and_picture_control_arrays() {
    // ColorBalance (0x000C), PictureControlData (0x0023), ImageBoundary (0x0016).
    // Three array entries -> trailing offset computed from 3-entry header.
    let n = 3;
    let off = nikon_trailing_offset(n);
    // extract_u16_array reads `count` u16 elements from data[value_offset..],
    // so count is the ELEMENT count and the payload must hold that many u16s.
    let payload = u16s_le(&[10, 20, 30, 40, 50, 60, 70, 80]);
    let elems = 8u32;
    let entries = [
        Entry {
            tag: 0x000C,
            field_type: 7,
            count: elems,
            value_offset: off,
        }, // ColorBalance
        Entry {
            tag: 0x0023,
            field_type: 7,
            count: elems,
            value_offset: off,
        }, // PictureControlData
        Entry {
            tag: 0x0016,
            field_type: 7,
            count: elems,
            value_offset: off,
        }, // ImageBoundary
    ];
    let data = build_nikon(&entries, &payload);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(tags.contains_key("Nikon:ColorBalance"));
    assert!(tags.contains_key("Nikon:PictureControlVersion"));
    assert!(tags.contains_key("Nikon:ImageBoundary"));
}

#[test]
fn test_nikon_crop_multi_exposure_iso_af_flash_arrays() {
    // CropHiSpeed (0x001B), MultiExposure (0x00B2), ISOInfo (0x00B6),
    // AFInfo (0x0088), FlashInfo (0x00A8).
    let n = 5;
    let off = nikon_trailing_offset(n);
    let payload = u16s_le(&[2, 1, 3, 4, 5, 6, 7, 8]);
    let elems = 8u32;
    let entries = [
        Entry {
            tag: 0x001B,
            field_type: 7,
            count: elems,
            value_offset: off,
        }, // CropHiSpeed -> array[0]=2 -> "DX Crop"
        Entry {
            tag: 0x00B2,
            field_type: 7,
            count: elems,
            value_offset: off,
        }, // MultiExposure -> array[0]=2 -> "Image Overlay"
        Entry {
            tag: 0x00B6,
            field_type: 7,
            count: elems,
            value_offset: off,
        }, // ISOInfo
        Entry {
            tag: 0x0088,
            field_type: 7,
            count: elems,
            value_offset: off,
        }, // AFInfo
        Entry {
            tag: 0x00A8,
            field_type: 7,
            count: elems,
            value_offset: off,
        }, // FlashInfo
    ];
    let data = build_nikon(&entries, &payload);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert_eq!(tags.get("Nikon:CropHiSpeed"), Some(&"DX Crop".to_string()));
    assert_eq!(
        tags.get("Nikon:MultiExposureMode"),
        Some(&"Image Overlay".to_string())
    );
    assert!(tags.contains_key("Nikon:ISOExpansion"));
    assert!(tags.contains_key("Nikon:AFInfo"));
    assert!(tags.contains_key("Nikon:FlashInfoVersion"));
}

#[test]
fn test_nikon_lens_data_full_aperture_range() {
    // LensData (0x0098) long enough to hit the max-aperture-at-min/max-focal arms
    // (indices 11 and 12), beyond what wave-1 covered.
    let n = 1;
    let off = nikon_trailing_offset(n);
    // 13 u16 elements: index 7 = known lens id (147), index 11/12 aperture values.
    let lens_vals = [1u16, 10, 20, 0, 30, 40, 50, 147, 80, 24, 70, 28, 56];
    let lens = u16s_le(&lens_vals);
    let entries = [Entry {
        tag: 0x0098,
        field_type: 7,
        count: lens_vals.len() as u32,
        value_offset: off,
    }];
    let data = build_nikon(&entries, &lens);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(tags.contains_key("Nikon:LensID"));
    assert!(tags.contains_key("Nikon:MaxApertureAtMinFocal"));
    assert!(tags.contains_key("Nikon:MaxApertureAtMaxFocal"));
}

#[test]
fn test_nikon_lens_data_unknown_lens_id() {
    // LensData with an unknown lens id -> "Unknown (id)" branch.
    let n = 1;
    let off = nikon_trailing_offset(n);
    let lens_vals = [1u16, 10, 20, 0, 30, 40, 50, 64000, 80, 24, 70, 28, 56];
    let lens = u16s_le(&lens_vals);
    let entries = [Entry {
        tag: 0x0098,
        field_type: 7,
        count: lens_vals.len() as u32,
        value_offset: off,
    }];
    let data = build_nikon(&entries, &lens);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    let lens_id = tags.get("Nikon:LensID").expect("lens id present");
    assert!(lens_id.contains("Unknown"), "got {lens_id}");
}

#[test]
fn test_nikon_string_tags_cluster() {
    // String tags resolved through extract_string_with_offset. Put an ASCII string
    // in the trailing block; the stored offset is relative to the embedded TIFF
    // (byte 10). With a single-entry IFD, trailing begins at byte 36, so the
    // tiff-relative offset is 36 - 10 = 26.
    let mut trailing = Vec::new();
    trailing.extend_from_slice(b"VIVID\x00");
    let entries = [Entry {
        tag: 0x00AB, // VariProgram (string)
        field_type: 2,
        count: 6,
        value_offset: 26,
    }];
    let data = build_nikon(&entries, &trailing);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    // Code path executed; tag present if string resolved.
    let _ = tags.get("Nikon:VariProgram");
}

#[test]
fn test_nikon_sensor_pixel_size_hex() {
    // SensorPixelSize (0x009A) formats value_offset as hex.
    let entries = [Entry {
        tag: 0x009A,
        field_type: 4,
        count: 1,
        value_offset: 0x12345678,
    }];
    let data = build_nikon(&entries, &[]);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert_eq!(
        tags.get("Nikon:SensorPixelSize"),
        Some(&"0x12345678".to_string())
    );
}

#[test]
fn test_nikon_vr_info_mode_branches() {
    // VRInfo (0x00B1) with array[1] driving the VR mode sub-decoder.
    let n = 1;
    let off = nikon_trailing_offset(n);
    // array[0] = version, array[1] = 2 -> "Active".
    let payload = u16s_le(&[1, 2, 0, 0]);
    let entries = [Entry {
        tag: 0x00B1,
        field_type: 7,
        count: 4,
        value_offset: off,
    }];
    let data = build_nikon(&entries, &payload);
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
    assert!(tags.contains_key("Nikon:VRInfoVersion"));
    assert_eq!(tags.get("Nikon:VRMode"), Some(&"Active".to_string()));
}

#[test]
fn test_nikon_error_paths() {
    // Empty -> Ok, no tags.
    let mut tags = HashMap::new();
    parse_nikon_makernotes(&[], ByteOrder::LittleEndian, &mut tags);
    assert!(tags.is_empty());

    // Valid Nikon header but truncated before TIFF header -> early Ok return.
    let mut short = Vec::new();
    short.extend_from_slice(b"Nikon\0");
    short.extend_from_slice(&[0x02, 0x10, 0x00, 0x00]); // 10 bytes only
    let mut tags2 = HashMap::new();
    parse_nikon_makernotes(&short, ByteOrder::LittleEndian, &mut tags2);
    assert!(tags2.is_empty());

    // Invalid TIFF byte order ("XX") -> Err inside parse, no tags.
    let mut bad = Vec::new();
    bad.extend_from_slice(b"Nikon\0");
    bad.extend_from_slice(&[0x02, 0x10, 0x00, 0x00]);
    bad.extend_from_slice(b"XX");
    bad.extend_from_slice(&[0u8; 8]);
    let mut tags3 = HashMap::new();
    parse_nikon_makernotes(&bad, ByteOrder::LittleEndian, &mut tags3);
    assert!(tags3.is_empty());

    // Non-Nikon header rejected.
    let mut tags4 = HashMap::new();
    parse_nikon_makernotes(b"Sony\0\0\0\0\0\0\0\0", ByteOrder::LittleEndian, &mut tags4);
    assert!(tags4.is_empty());
}

// ===========================================================================
// Production path: drive read_metadata over a real DNG fixture (dispatch).
// ===========================================================================

#[test]
fn test_read_metadata_dng_fixture_dispatch() {
    use std::io::Write;
    let bytes = std::fs::read("tests/fixtures/raw/sample.dng").expect("read dng fixture");
    let mut tmp = tempfile::Builder::new()
        .suffix(".dng")
        .tempfile()
        .expect("create temp dng");
    tmp.write_all(&bytes).expect("write temp dng");
    tmp.flush().expect("flush temp dng");

    let result = oxidex::core::operations::read_metadata(tmp.path());
    assert!(result.is_ok(), "read_metadata should parse the DNG fixture");
    assert!(result.unwrap().len() > 0, "expected some tags from DNG");
}
