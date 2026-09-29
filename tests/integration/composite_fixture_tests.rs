//! Regression coverage for Composite tags evaluated from real pinned fixtures.
//!
//! These tests require the pinned 13.59 combined-sample corpus. They are
//! explicitly ignored by default; an absent fixture fails when requested.

use crate::fixtures;
use oxidex::core::operations::read_metadata;

const KODAK: &str = "Kodak.jpg";
const FLIR: &str = "FLIR.jpg";
const APPLE_IPHONE_13_PRO: &str = "Apple/Apple_iPhone13Pro.jpg";
const SAMSUNG_L73: &str = "Samsung/SamsungL73.jpg";
const SAMSUNG_A55: &str = "Samsung/SamsungGalaxyA55_5G.jpg";
const SAMSUNG_GT_I8910: &str = "Samsung/SamsungGT-i8910.jpg";
const NIKON_Z7_2: &str = "Nikon/NikonZ7_2.jpg";
const NIKON_P6000: &str = "Nikon/NikonCoolpixP6000.jpg";
const NIKON_D5500: &str = "Nikon/NikonD5500.jpg";
const NIKON_P520: &str = "Nikon/NikonCoolpixP520.jpg";

/// Kodak.pm's DateCreated Composite joins YearCreated and MonthDayCreated.
/// ExifTool 13.59 reports `2002:05:01` for this corpus image.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn kodak_fixture_reports_composite_date_created() {
    let path = fixtures::required_combined_fixture_path(KODAK);
    let metadata = read_metadata(&path).expect("Kodak fixture parses");
    assert_eq!(
        metadata.get_string("Composite:DateCreated"),
        Some("2002:05:01")
    );
}

/// FLIR.pm derives this from PlanckB as `14387.6515 / PlanckB` and formats it
/// to one decimal micrometre. ExifTool 13.59 reports `10.5 um` here.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn flir_fixture_reports_composite_peak_spectral_sensitivity() {
    let path = fixtures::required_combined_fixture_path(FLIR);
    let metadata = read_metadata(&path).expect("FLIR fixture parses");
    assert_eq!(
        metadata.get_string("Composite:PeakSpectralSensitivity"),
        Some("10.5 um")
    );
}

/// The TIFF MakerNote in FLIR.jpg stores these separately from the FLIR APP1
/// binary record as `rational64u[1]`: 308/1, 281/1 and 80/100.  Pinning all
/// three makes the TIFF-relative MakerNote value base observable instead of
/// accidentally relying on the APP1 copy of Emissivity.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn flir_fixture_reports_makernote_rational_measurements() {
    let path = fixtures::required_combined_fixture_path(FLIR);
    let metadata = read_metadata(&path).expect("FLIR fixture parses");
    // These two source numerics are typed floats in the public map, so
    // `get_string` is intentionally None. CLI JSON prints 308 and 281.
    for (name, value) in [("ImageTemperatureMax", 308), ("ImageTemperatureMin", 281)] {
        let key = format!("FLIR:{name}");
        assert_eq!(
            metadata.get(&key),
            Some(&oxidex::core::TagValue::Float(f64::from(value)))
        );
        let rows: Vec<_> = metadata
            .project_occurrences(oxidex::core::tag_occurrence::ValueChannel::Stored)
            .filter(|(name, _, _)| *name == key)
            .map(|(_, row, _)| row)
            .collect();
        assert_eq!(rows.len(), 1, "{key}");
        let row = rows[0];
        assert_eq!(&*row.group0, "MakerNotes");
        assert_eq!(&*row.group1, "FLIR");
        assert_eq!(row.priority, 0);
        assert_eq!(
            row.project(oxidex::core::tag_occurrence::ValueChannel::Stored)
                .as_ref(),
            &oxidex::core::TagValue::new_rational(value, 1),
        );
        for channel in [
            oxidex::core::tag_occurrence::ValueChannel::ValueConv,
            oxidex::core::tag_occurrence::ValueChannel::PrintConv,
        ] {
            assert_eq!(
                row.project(channel).as_ref(),
                &oxidex::core::TagValue::Float(f64::from(value)),
            );
        }
    }
    // APP1 also reports Emissivity. Select the physical MakerNote occurrence,
    // not the same-name APP1 winner, before checking its typed channels.
    let row = metadata
        .project_occurrences(oxidex::core::tag_occurrence::ValueChannel::Stored)
        .filter(|(name, _, _)| *name == "FLIR:Emissivity")
        .map(|(_, row, _)| row)
        .find(|row| row.group0.as_ref() == "MakerNotes")
        .expect("physical FLIR Main occurrence");
    assert_eq!(&*row.group1, "FLIR");
    assert_eq!(row.priority, 0);
    assert_eq!(
        row.project(oxidex::core::tag_occurrence::ValueChannel::Stored)
            .as_ref(),
        &oxidex::core::TagValue::new_rational(80, 100),
    );
    assert_eq!(
        row.project(oxidex::core::tag_occurrence::ValueChannel::ValueConv)
            .as_ref(),
        &oxidex::core::TagValue::Float(0.8),
    );
    assert_eq!(
        row.project(oxidex::core::tag_occurrence::ValueChannel::PrintConv)
            .as_ref(),
        &oxidex::core::TagValue::new_string("0.80"),
    );
}

/// GPS.pm's altitude Composite truncates to one decimal place, while XMP's
/// coordinates furnish the A55 reference composites. These source fixtures
/// also cover the north/east suffix form used by Adobe XMP GPS values.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn gps_composite_fixtures_match_pinned_exiftool() {
    let apple_path = fixtures::required_combined_fixture_path(APPLE_IPHONE_13_PRO);
    let l73_path = fixtures::required_combined_fixture_path(SAMSUNG_L73);
    let a55_path = fixtures::required_combined_fixture_path(SAMSUNG_A55);

    let apple = read_metadata(&apple_path).expect("Apple fixture parses");
    assert_eq!(
        apple.get_string("Composite:GPSAltitude"),
        Some("27.9 m Above Sea Level")
    );

    let l73 = read_metadata(&l73_path).expect("Samsung L73 fixture parses");
    assert_eq!(
        l73.get_string("Composite:GPSDestLatitude"),
        Some("35 deg 48' 8.00\" N")
    );

    let a55 = read_metadata(&a55_path).expect("Samsung A55 fixture parses");
    assert_eq!(a55.get_string("Composite:GPSLatitudeRef"), Some("North"));
    assert_eq!(a55.get_string("Composite:GPSLongitudeRef"), Some("East"));
}

/// Exif.pm's PreviewImageSize Composite joins the APP4 width and height from
/// the GT-i8910 exactly as `"$val[0]x$val[1]"`.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn samsung_gt_i8910_reports_composite_preview_image_size() {
    let path = fixtures::required_combined_fixture_path(SAMSUNG_GT_I8910);
    let metadata = read_metadata(&path).expect("Samsung fixture parses");
    assert_eq!(
        metadata.get_string("Composite:PreviewImageSize"),
        Some("816x459")
    );
}

/// Nikon.pm's Composite table derives these from the parsed AFInfo2 fields.
/// The pinned ExifTool 13.59 corpus reports both as Off for NikonZ7_2.jpg.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn nikon_z7_2_fixture_reports_af_detection_composites() {
    let path = fixtures::required_combined_fixture_path(NIKON_Z7_2);
    let metadata = read_metadata(&path).expect("Nikon Z7 II fixture parses");
    assert_eq!(
        metadata.get_string("Composite:ContrastDetectAF"),
        Some("Off")
    );
    assert_eq!(metadata.get_string("Composite:PhaseDetectAF"), Some("Off"));
}

/// Nikon AFInfo2V0300 retains source coordinates and the integer grid
/// ValueConv separately from the public focus labels.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn nikon_z7_2_focus_positions_keep_typed_channels() {
    let path = fixtures::required_combined_fixture_path(NIKON_Z7_2);
    let metadata = read_metadata(&path).expect("Nikon Z7 II fixture parses");
    for (key, printed, value) in [
        ("Nikon:FocusPositionHorizontal", "1R of Center", 16),
        ("Nikon:FocusPositionVertical", "3D from Center", 12),
    ] {
        assert_eq!(metadata.get_string(key), Some(printed));
        let rows: Vec<_> = metadata
            .project_occurrences(oxidex::core::tag_occurrence::ValueChannel::ValueConv)
            .filter(|(row_key, _, _)| *row_key == key)
            .collect();
        assert_eq!(rows.len(), 1, "{key}");
        assert_eq!(&*rows[0].1.group1, "Nikon");
        assert_eq!(rows[0].2.as_ref(), &oxidex::core::TagValue::Integer(value),);
    }
}

/// Nikon.pm's ShotInfo table maps byte 0x10 to the P6000-only Off/On value.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn nikon_p6000_fixture_reports_distortion_control() {
    let path = fixtures::required_combined_fixture_path(NIKON_P6000);
    let metadata = read_metadata(&path).expect("Nikon P6000 fixture parses");
    assert_eq!(metadata.get_string("Nikon:DistortionControl"), Some("Off"));
}

/// XMP.pm's Flash composite packs the five XMP-exif component fields into the
/// standard EXIF flash bitfield before applying Exif.pm's flash PrintConv.
/// Pinned ExifTool 13.59 reports `Off, Did not fire` for NikonCoolpixP520.jpg.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn nikon_p520_fixture_reports_xmp_flash_composite() {
    let path = fixtures::required_combined_fixture_path(NIKON_P520);
    let metadata = read_metadata(&path).expect("Nikon P520 fixture parses");
    assert_eq!(
        metadata.get_string("Composite:Flash"),
        Some("Off, Did not fire")
    );
}

/// Nikon.pm's LensSpec Composite concatenates the already print-converted
/// MakerNote Lens and LensType values. Pinned ExifTool 13.59 reports this
/// exact value for NikonD5500.jpg.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn nikon_d5500_fixture_reports_composite_lens_spec() {
    let path = fixtures::required_combined_fixture_path(NIKON_D5500);
    let metadata = read_metadata(&path).expect("Nikon D5500 fixture parses");
    assert_eq!(
        metadata.get_string("Composite:LensSpec"),
        Some("18-55mm f/3.5-5.6 G VR")
    );
}
