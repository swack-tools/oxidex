//! Regression coverage for Composite tags evaluated from real pinned fixtures.
//!
//! The files are from the same comparison corpus used for the compatibility
//! report.  The guards intentionally keep a source checkout usable without
//! that optional cache while pinning exact ExifTool 13.59 output in CI.

use oxidex::core::operations::read_metadata;
use std::path::Path;

const KODAK: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Kodak.jpg";
const FLIR: &str = "/tmp/oxidex-exiftool-cache/combined-samples/FLIR.jpg";
const APPLE_IPHONE_13_PRO: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Apple/Apple_iPhone13Pro.jpg";
const SAMSUNG_L73: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Samsung/SamsungL73.jpg";
const SAMSUNG_A55: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Samsung/SamsungGalaxyA55_5G.jpg";
const SAMSUNG_GT_I8910: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Samsung/SamsungGT-i8910.jpg";
const NIKON_Z7_2: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Nikon/NikonZ7_2.jpg";
const NIKON_P6000: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Nikon/NikonCoolpixP6000.jpg";
const NIKON_D5500: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Nikon/NikonD5500.jpg";
const NIKON_P520: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Nikon/NikonCoolpixP520.jpg";

/// Kodak.pm's DateCreated Composite joins YearCreated and MonthDayCreated.
/// ExifTool 13.59 reports `2002:05:01` for this corpus image.
#[test]
fn kodak_fixture_reports_composite_date_created() {
    if !Path::new(KODAK).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(KODAK)).expect("Kodak fixture parses");
    assert_eq!(
        metadata.get_string("Composite:DateCreated"),
        Some("2002:05:01")
    );
}

/// FLIR.pm derives this from PlanckB as `14387.6515 / PlanckB` and formats it
/// to one decimal micrometre. ExifTool 13.59 reports `10.5 um` here.
#[test]
fn flir_fixture_reports_composite_peak_spectral_sensitivity() {
    if !Path::new(FLIR).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(FLIR)).expect("FLIR fixture parses");
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
fn flir_fixture_reports_makernote_rational_measurements() {
    if !Path::new(FLIR).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(FLIR)).expect("FLIR fixture parses");
    assert_eq!(
        metadata.get_string("MakerNotes:ImageTemperatureMax"),
        Some("308")
    );
    assert_eq!(
        metadata.get_string("MakerNotes:ImageTemperatureMin"),
        Some("281")
    );
    assert_eq!(metadata.get_string("MakerNotes:Emissivity"), Some("0.80"));
}

/// GPS.pm's altitude Composite truncates to one decimal place, while XMP's
/// coordinates furnish the A55 reference composites. These source fixtures
/// also cover the north/east suffix form used by Adobe XMP GPS values.
#[test]
fn gps_composite_fixtures_match_pinned_exiftool() {
    for path in [APPLE_IPHONE_13_PRO, SAMSUNG_L73, SAMSUNG_A55] {
        if !Path::new(path).is_file() {
            return;
        }
    }

    let apple = read_metadata(Path::new(APPLE_IPHONE_13_PRO)).expect("Apple fixture parses");
    assert_eq!(
        apple.get_string("Composite:GPSAltitude"),
        Some("27.9 m Above Sea Level")
    );

    let l73 = read_metadata(Path::new(SAMSUNG_L73)).expect("Samsung L73 fixture parses");
    assert_eq!(
        l73.get_string("Composite:GPSDestLatitude"),
        Some("35 deg 48' 8.00\" N")
    );

    let a55 = read_metadata(Path::new(SAMSUNG_A55)).expect("Samsung A55 fixture parses");
    assert_eq!(a55.get_string("Composite:GPSLatitudeRef"), Some("North"));
    assert_eq!(a55.get_string("Composite:GPSLongitudeRef"), Some("East"));
}

/// Exif.pm's PreviewImageSize Composite joins the APP4 width and height from
/// the GT-i8910 exactly as `"$val[0]x$val[1]"`.
#[test]
fn samsung_gt_i8910_reports_composite_preview_image_size() {
    if !Path::new(SAMSUNG_GT_I8910).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(SAMSUNG_GT_I8910)).expect("Samsung fixture parses");
    assert_eq!(
        metadata.get_string("Composite:PreviewImageSize"),
        Some("816x459")
    );
}

/// Nikon.pm's Composite table derives these from the parsed AFInfo2 fields.
/// The pinned ExifTool 13.59 corpus reports both as Off for NikonZ7_2.jpg.
#[test]
fn nikon_z7_2_fixture_reports_af_detection_composites() {
    if !Path::new(NIKON_Z7_2).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(NIKON_Z7_2)).expect("Nikon Z7 II fixture parses");
    assert_eq!(
        metadata.get_string("Composite:ContrastDetectAF"),
        Some("Off")
    );
    assert_eq!(metadata.get_string("Composite:PhaseDetectAF"), Some("Off"));
}

/// Nikon.pm's ShotInfo table maps byte 0x10 to the P6000-only Off/On value.
#[test]
fn nikon_p6000_fixture_reports_distortion_control() {
    if !Path::new(NIKON_P6000).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(NIKON_P6000)).expect("Nikon P6000 fixture parses");
    assert_eq!(metadata.get_string("Nikon:DistortionControl"), Some("Off"));
}

/// XMP.pm's Flash composite packs the five XMP-exif component fields into the
/// standard EXIF flash bitfield before applying Exif.pm's flash PrintConv.
/// Pinned ExifTool 13.59 reports `Off, Did not fire` for NikonCoolpixP520.jpg.
#[test]
fn nikon_p520_fixture_reports_xmp_flash_composite() {
    if !Path::new(NIKON_P520).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(NIKON_P520)).expect("Nikon P520 fixture parses");
    assert_eq!(
        metadata.get_string("Composite:Flash"),
        Some("Off, Did not fire")
    );
}

/// Nikon.pm's LensSpec Composite concatenates the already print-converted
/// MakerNote Lens and LensType values. Pinned ExifTool 13.59 reports this
/// exact value for NikonD5500.jpg.
#[test]
fn nikon_d5500_fixture_reports_composite_lens_spec() {
    if !Path::new(NIKON_D5500).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(NIKON_D5500)).expect("Nikon D5500 fixture parses");
    assert_eq!(
        metadata.get_string("Composite:LensSpec"),
        Some("18-55mm f/3.5-5.6 G VR")
    );
}
