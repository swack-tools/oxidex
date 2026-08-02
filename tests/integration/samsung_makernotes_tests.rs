//! Integration tests for Samsung MakerNotes parser
//!
//! This file was undeclared to Cargo and had never compiled. Seventeen of its
//! twenty-three tests asserted tag names that do not exist in ExifTool's
//! Samsung.pm: `SceneOptimizer`, `SceneType`, `SingleTake`, `ExpertRAW`,
//! `NightMode`, `GalaxyLensType`, `ZoomLevel`, `PortraitEffect`,
//! `DirectorsView`, `ProMode`, `SuperSteady`, `FoodMode`, `ObjectTracking` and
//! `MultiFrameNoiseReduction` (0 hits for `Name => '<tag>'` across all of
//! ExifTool 13.59; `MultiFrameNoiseReduction` exists only in Sony.pm).
//!
//! Two near-misses worth recording, because they look like hits:
//!   * `%Image::ExifTool::Samsung::PortraitEffect` is a real *table* name, but
//!     it is JSON-processed with string keys and yields `PortraitEffectID`,
//!     `PortraitEffectLevel`, ... -- there is no `Samsung:PortraitEffect` tag
//!     and it is not a numeric MakerNote IFD entry.
//!   * `SceneType` exists in FlashPix.pm, Exif.pm and DICOM.pm, not Samsung.pm.
//!
//! What survives is checked against ExifTool 13.59 `Samsung::Type2` by tag id
//! and by PrintConv value.

use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::samsung::SamsungParser;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

#[test]
fn test_samsung_parser_trait() {
    let parser = SamsungParser::new();
    assert_eq!(parser.manufacturer_name(), "Samsung");
    assert_eq!(parser.tag_prefix(), "Samsung:");
}

#[test]
fn test_samsung_makernote_version() {
    // ExifTool Samsung::Type2 0x0001 MakerNoteVersion, Writable => 'undef', Count => 4.
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0x0001, Type 2 (ASCII), Count 4, Value "0100"
    data.extend_from_slice(&[
        0x01, 0x00, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x30, 0x31, 0x30, 0x30,
    ]);

    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .is_ok()
    );
    assert_eq!(
        tags.get("Samsung:MakerNoteVersion"),
        Some(&"0100".to_string())
    );
}

#[test]
fn test_samsung_device_type() {
    // ExifTool Samsung::Type2 0x0002 DeviceType PrintConv: 0x2000 => 'High-end NX Camera'.
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0x0002, Type 4 (LONG), Count 1, Value 0x2000
    data.extend_from_slice(&[
        0x02, 0x00, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00,
    ]);

    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .is_ok()
    );
    assert_eq!(
        tags.get("Samsung:DeviceType"),
        Some(&"High-end NX Camera".to_string())
    );
}

#[test]
fn test_samsung_model_id_nx10() {
    // ExifTool Samsung::Type2 0x0003 SamsungModelID PrintConv: 0x100101c => 'NX10'.
    //
    // This test previously used 0x100123a => "NX1". ExifTool has no entry for
    // 0x100123a at all, and its only 'NX1' is commented out at 0x5001038.
    // 0x100101c => 'NX10' is the one mapping where oxidex's MODEL_ID_DECODER
    // and ExifTool agree; the rest of that table disagrees and is reported
    // separately.
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0x0003, Type 4 (LONG), Count 1, Value 0x0100101c
    data.extend_from_slice(&[
        0x03, 0x00, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1c, 0x10, 0x00, 0x01,
    ]);

    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .is_ok()
    );
    assert_eq!(
        tags.get("Samsung:SamsungModelID"),
        Some(&"NX10".to_string())
    );
}

// ============================================================================
// ColorSpace -- ExifTool Samsung::Type2 0xa011
//
// The PrintConv (0 => 'sRGB', 1 => 'Adobe RGB') is correct in oxidex, but the
// tag id is not: `SAMSUNG_COLOR_SPACE` in registries/samsung.rs is 0x0221, and
// 0x0221 appears nowhere in Samsung.pm. Consequence on a real Samsung file:
// ColorSpace is never emitted (nothing sits at 0x0221), and whatever really
// does live at 0x0221 would be misread as ColorSpace.
//
// These two tests encode ExifTool's id, so they fail until the constant is
// corrected. They are #[ignore]d rather than deleted or bent to 0x0221 --
// deleting would lose the defect, and asserting 0x0221 would pin it.
// ============================================================================

#[test]
#[ignore = "SAMSUNG_COLOR_SPACE is 0x0221; ExifTool Samsung::Type2 puts ColorSpace at 0xa011"]
fn test_samsung_color_space_srgb() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0xa011, Type 3 (SHORT, ExifTool: int16u), Count 1, Value 0 (sRGB)
    data.extend_from_slice(&[
        0x11, 0xa0, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);

    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .is_ok()
    );
    assert_eq!(tags.get("Samsung:ColorSpace"), Some(&"sRGB".to_string()));
}

#[test]
#[ignore = "SAMSUNG_COLOR_SPACE is 0x0221; ExifTool Samsung::Type2 puts ColorSpace at 0xa011"]
fn test_samsung_color_space_adobe_rgb() {
    let parser = SamsungParser::new();
    let mut data = vec![0x01, 0x00]; // 1 entry
    // Tag 0xa011, Type 3 (SHORT), Count 1, Value 1 (Adobe RGB)
    data.extend_from_slice(&[
        0x11, 0xa0, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    ]);

    let mut tags = HashMap::new();
    assert!(
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .is_ok()
    );
    assert_eq!(
        tags.get("Samsung:ColorSpace"),
        Some(&"Adobe RGB".to_string())
    );
}
