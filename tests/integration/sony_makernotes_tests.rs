//! Integration tests for Sony MakerNotes parser
//!
//! Tests the Sony MakerNotes parsing functionality including:
//! - Lens database lookups (A-mount and E-mount)
//! - MakerNote header validation
//! - Tag extraction from synthetic test data
//! - Drive mode, white balance, focus mode decoding
//! - Array tag parsing (CameraSettings, AFInfo, ShotInfo)

#[test]
fn test_sony_lens_database_a_mount() {
    use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;

    // Test Minolta AF legacy lens
    assert_eq!(
        lookup_lens_name(11),
        Some("Minolta AF 50mm f/1.4".to_string())
    );

    // Test Sony A-mount standard zoom
    assert_eq!(
        lookup_lens_name(151),
        Some("Sony SAL 24-70mm f/2.8 ZA SSM".to_string())
    );

    // Test Sony A-mount professional telephoto
    assert_eq!(
        lookup_lens_name(164),
        Some("Sony SAL 300mm f/2.8 G SSM".to_string())
    );

    // Test Sony A-mount prime
    assert_eq!(
        lookup_lens_name(129),
        Some("Sony SAL 85mm f/1.4 ZA".to_string())
    );
}

#[test]
fn test_sony_lens_database_e_mount_fe() {
    use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;

    // Test FE prime lens
    assert_eq!(
        lookup_lens_name(266),
        Some("Sony FE 50mm f/1.2 GM".to_string())
    );

    // Test FE standard zoom
    assert_eq!(
        lookup_lens_name(281),
        Some("Sony FE 24-70mm f/2.8 GM".to_string())
    );

    // Test FE telephoto zoom
    assert_eq!(
        lookup_lens_name(287),
        Some("Sony FE 70-200mm f/2.8 GM OSS".to_string())
    );

    // Test FE macro lens
    assert_eq!(
        lookup_lens_name(277),
        Some("Sony FE 90mm f/2.8 Macro G OSS".to_string())
    );
}

#[test]
fn test_sony_lens_database_e_mount_aps_c() {
    use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;

    // Test E-mount APS-C wide angle
    assert_eq!(
        lookup_lens_name(320),
        Some("Sony E 10-18mm f/4 OSS".to_string())
    );

    // Test E-mount APS-C standard zoom
    assert_eq!(
        lookup_lens_name(323),
        Some("Sony E 16-55mm f/2.8 G".to_string())
    );

    // Test E-mount APS-C telephoto
    assert_eq!(
        lookup_lens_name(337),
        Some("Sony E 70-350mm f/4.5-6.3 G OSS".to_string())
    );
}

#[test]
fn test_sony_lens_database_g_master() {
    use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;

    // Test G Master lenses (premium line)
    assert_eq!(
        lookup_lens_name(398),
        Some("Sony FE 50mm f/1.2 GM".to_string())
    );

    assert_eq!(
        lookup_lens_name(402),
        Some("Sony FE 85mm f/1.4 GM II".to_string())
    );

    assert_eq!(
        lookup_lens_name(390),
        Some("Sony FE 12-24mm f/2.8 GM".to_string())
    );
}

#[test]
fn test_sony_lens_database_zeiss() {
    use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;

    // Test Zeiss Batis
    assert_eq!(
        lookup_lens_name(451),
        Some("Zeiss Batis 85mm f/1.8".to_string())
    );

    // Test Zeiss Loxia
    assert_eq!(
        lookup_lens_name(456),
        Some("Zeiss Loxia 50mm f/2".to_string())
    );

    // Test Sony-Zeiss collaboration
    assert_eq!(
        lookup_lens_name(468),
        Some("Sony FE 55mm f/1.8 ZA".to_string())
    );
}

#[test]
fn test_sony_lens_database_third_party() {
    use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;

    // Test Sigma Contemporary
    assert_eq!(
        lookup_lens_name(513),
        Some("Sigma 30mm f/1.4 DC DN Contemporary".to_string())
    );

    // Test Tamron lens
    assert_eq!(
        lookup_lens_name(520),
        Some("Tamron 28-75mm f/2.8 Di III RXD".to_string())
    );

    // Test Sigma Art lens
    assert_eq!(
        lookup_lens_name(517),
        Some("Sigma 85mm f/1.4 DG DN Art".to_string())
    );
}

#[test]
fn test_sony_lens_database_unknown() {
    use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;

    // Unknown lens ID should return None
    assert_eq!(lookup_lens_name(65000), None);
    assert_eq!(lookup_lens_name(9999), None);
}

#[test]
fn test_sony_is_sony_makernote() {
    use oxidex::parsers::tiff::makernotes::sony::is_sony_makernote;

    // Valid Sony header with signature
    assert!(is_sony_makernote(b"SONY\x05\x00"));

    // Valid IFD without signature (5 entries, little-endian)
    assert!(is_sony_makernote(b"\x05\x00"));

    // Valid IFD without signature (5 entries, big-endian)
    assert!(is_sony_makernote(b"\x00\x05"));

    // Invalid - too many entries
    assert!(!is_sony_makernote(b"\xFF\xFF"));

    // Invalid - too short
    assert!(!is_sony_makernote(b"\x01"));

    // Invalid - zero entries
    assert!(!is_sony_makernote(b"\x00\x00"));
}

#[test]
fn test_sony_parse_basic_tags() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::sony::parse_sony_makernote;
    use std::collections::HashMap;

    // Create minimal Sony MakerNote
    let mut data = Vec::new();

    // No signature - start directly with IFD
    // IFD: entry count (little-endian)
    data.extend_from_slice(&[0x03, 0x00]); // 3 entries

    // Entry 1: ImageQuality (tag 0x0102)
    data.extend_from_slice(&[0x02, 0x01]); // Tag ID
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Fine)

    // Entry 2: SequenceNumber (tag 0xB04B)
    data.extend_from_slice(&[0x4B, 0xB0]); // Tag ID
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x05, 0x00, 0x00, 0x00]); // Value: 5

    // Entry 3: ShutterCount (tag 0xB05A)
    data.extend_from_slice(&[0x5A, 0xB0]); // Tag ID
    data.extend_from_slice(&[0x04, 0x00]); // Type: LONG
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0xE8, 0x03, 0x00, 0x00]); // Value: 1000

    // Next IFD offset
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut result = HashMap::new();
    parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut result);

    // Verify extracted tags
    assert!(result.contains_key("Sony:ImageQuality"));
    assert_eq!(result.get("Sony:ImageQuality"), Some(&"2".to_string()));

    assert!(result.contains_key("Sony:SequenceNumber"));
    assert_eq!(result.get("Sony:SequenceNumber"), Some(&"5".to_string()));

    assert!(result.contains_key("Sony:ShutterCount"));
    assert_eq!(result.get("Sony:ShutterCount"), Some(&"1000".to_string()));
}

#[test]
fn test_sony_parse_lens_id_lookup() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::sony::parse_sony_makernote;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // IFD: 1 entry
    data.extend_from_slice(&[0x01, 0x00]);

    // Entry: LensID (tag 0xB027) = 281 (Sony FE 24-70mm f/2.8 GM)
    data.extend_from_slice(&[0x27, 0xB0]); // Tag ID
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
    data.extend_from_slice(&[0x19, 0x01, 0x00, 0x00]); // Value: 281

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    let mut result = HashMap::new();
    parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut result);

    // Should have looked up lens name from database
    assert!(result.contains_key("Sony:LensType"));
    assert_eq!(
        result.get("Sony:LensType"),
        Some(&"Sony FE 24-70mm f/2.8 GM".to_string())
    );
}

#[test]
fn test_sony_parse_camera_settings_array() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::sony::parse_sony_makernote;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // IFD: 1 entry
    data.extend_from_slice(&[0x01, 0x00]);

    // CameraSettings tag (0x0114)
    data.extend_from_slice(&[0x14, 0x01]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x11, 0x00, 0x00, 0x00]); // Count: 17
    data.extend_from_slice(&[0x12, 0x00, 0x00, 0x00]); // Offset: 18 (2 + 12 + 4)

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    // CameraSettings array (17 values)
    let settings: Vec<i16> = vec![
        1,   // [0] Drive mode: Continuous High
        0,   // [1] White balance mode: Auto
        2,   // [2] Focus mode: AF-C (Continuous)
        3,   // [3] AF area mode: Flexible Spot
        0,   // [4] Local AF area point
        0,   // [5] Metering mode: Multi-segment
        100, // [6] ISO setting: 100
        1,   // [7] Dynamic range optimizer: DRO Auto
        1,   // [8] Image stabilization: On
        1,   // [9] Color mode: Vivid
        0,   // [10] Color space
        2,   // [11] Long exposure NR: Normal
        3,   // [12] High ISO NR: High
        0,   // [13] Picture effect
        0,   // [14] Soft skin effect
        0,   // [15] Vignetting correction
        1,   // [16] Auto HDR: Auto
    ];

    for value in settings {
        data.extend_from_slice(&value.to_le_bytes());
    }

    let mut result = HashMap::new();
    parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut result);

    // Verify decoded settings
    assert_eq!(
        result.get("Sony:CameraSettings:DriveMode"),
        Some(&"Continuous High".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:WhiteBalanceMode"),
        Some(&"Auto".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:FocusMode"),
        Some(&"AF-C (Continuous)".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:AFAreaMode"),
        Some(&"Flexible Spot".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:MeteringMode"),
        Some(&"Multi-segment".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:ISO"),
        Some(&"100".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:DynamicRangeOptimizer"),
        Some(&"DRO Auto".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:ImageStabilization"),
        Some(&"On".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:ColorMode"),
        Some(&"Vivid".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:LongExposureNoiseReduction"),
        Some(&"Normal".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:HighISONoiseReduction"),
        Some(&"High".to_string())
    );
    assert_eq!(
        result.get("Sony:CameraSettings:AutoHDR"),
        Some(&"Auto".to_string())
    );
}

#[test]
fn test_sony_parse_af_info_array() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::sony::parse_sony_makernote;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // IFD: 1 entry
    data.extend_from_slice(&[0x01, 0x00]);

    // AFInfo tag (0x9400)
    data.extend_from_slice(&[0x00, 0x94]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]); // Count: 8
    data.extend_from_slice(&[0x12, 0x00, 0x00, 0x00]); // Offset: 18 (2 + 12 + 4)

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    // AFInfo array
    let af_info: Vec<i16> = vec![
        5, // [0] AF point selected: center
        3, // [1] AF points in focus: 3 points
        1, // [2] AF tracking status: active
        1, // [3] Face detection: enabled
        2, // [4] Num faces detected: 2
        0, 0, 0, // [5-7] unused
    ];

    for value in af_info {
        data.extend_from_slice(&value.to_le_bytes());
    }

    let mut result = HashMap::new();
    parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut result);

    assert_eq!(
        result.get("Sony:AFInfo:AFPointSelected"),
        Some(&"5".to_string())
    );
    assert_eq!(
        result.get("Sony:AFInfo:AFPointsInFocus"),
        Some(&"3".to_string())
    );
    assert_eq!(
        result.get("Sony:AFInfo:FaceDetection"),
        Some(&"On".to_string())
    );
    assert_eq!(
        result.get("Sony:AFInfo:NumFacesDetected"),
        Some(&"2".to_string())
    );
}

#[test]
fn test_sony_parse_shot_info_array() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::sony::parse_sony_makernote;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // IFD: 1 entry
    data.extend_from_slice(&[0x01, 0x00]);

    // ShotInfo tag (0x3000)
    data.extend_from_slice(&[0x00, 0x30]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x0A, 0x00, 0x00, 0x00]); // Count: 10
    data.extend_from_slice(&[0x12, 0x00, 0x00, 0x00]); // Offset: 18 (2 + 12 + 4)

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    // ShotInfo array
    let shot_info: Vec<i16> = vec![
        5,    // [0] White balance: Daylight
        0,    // [1] WB fine tune
        5500, // [2] Color temperature: 5500K
        0,    // [3] Color compensation filter
        0,    // [4] Saturation
        0,    // [5] Contrast
        0,    // [6] Sharpness
        0,    // [7] Brightness
        1,    // [8] Flash mode: Fill-flash
        0,    // [9] Flash exposure comp
    ];

    for value in shot_info {
        data.extend_from_slice(&value.to_le_bytes());
    }

    let mut result = HashMap::new();
    parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut result);

    assert_eq!(
        result.get("Sony:ShotInfo:WhiteBalance"),
        Some(&"Daylight".to_string())
    );
    assert_eq!(
        result.get("Sony:ShotInfo:ColorTemperature"),
        Some(&"5500".to_string())
    );
    assert_eq!(
        result.get("Sony:ShotInfo:FlashMode"),
        Some(&"Fill-flash".to_string())
    );
}

#[test]
fn test_sony_parse_empty_data() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::sony::parse_sony_makernote;
    use std::collections::HashMap;

    // Empty data should not crash
    let mut result = HashMap::new();
    parse_sony_makernote(&[], ByteOrder::LittleEndian, &mut result);
    assert!(result.is_empty());

    // Too short data should not crash
    let short_data = b"\x01";
    let mut result2 = HashMap::new();
    parse_sony_makernote(short_data, ByteOrder::LittleEndian, &mut result2);
    assert!(result2.is_empty());
}

#[test]
fn test_sony_parse_with_signature() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::sony::parse_sony_makernote;
    use std::collections::HashMap;

    let mut data = Vec::new();

    // Sony signature
    data.extend_from_slice(b"SONY");

    // IFD: 1 entry
    data.extend_from_slice(&[0x01, 0x00]);

    // Entry: SequenceNumber (tag 0xB04B)
    data.extend_from_slice(&[0x4B, 0xB0]);
    data.extend_from_slice(&[0x03, 0x00]);
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x0A, 0x00, 0x00, 0x00]); // Value: 10

    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let mut result = HashMap::new();
    parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut result);

    // Should skip signature and parse IFD correctly
    assert_eq!(result.get("Sony:SequenceNumber"), Some(&"10".to_string()));
}

#[test]
fn test_sony_lens_database_coverage() {
    use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;

    // Count how many lenses we have in database
    let mut count = 0;
    for id in 0..=600 {
        if lookup_lens_name(id).is_some() {
            count += 1;
        }
    }

    // Should have at least 100 lenses
    assert!(
        count >= 100,
        "Expected at least 100 lenses in database, found {}",
        count
    );
}
