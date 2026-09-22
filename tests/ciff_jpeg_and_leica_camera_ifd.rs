//! CIFF records embedded in JPEG APP0 (`HEAPJPGM`) and Leica Q3/SL3
//! `PanasonicRaw::CameraIFD`, forward-ported from `origin/main` 9b215f03
//! (#696) and 5cd52a0e (#701). The census that found them
//! (docs/reference/main-divergence-2026-09-18.md) counted these tags as
//! matched on main and MISSING on the tip.
//!
//! Expected values are the pinned oracle's (13.59, both probes asserted):
//!
//! ```text
//! $ exiftool-pinned.sh -G1 -a -s -CIFF:all CanonPowerShotPro70.jpg CanonPowerShot600.jpg
//! $ exiftool-pinned.sh -G1 -a -s -CameraIFD:all -OriginalDirectory LeicaQ3.jpg
//! ```

use oxidex::core::operations::read_metadata;
#[path = "common/fixtures.rs"]
mod fixtures;

fn assert_tags(file: &str, group: &str, expected: &[(&str, &str)]) {
    let path = fixtures::required_combined_fixture_path(file);
    let metadata = read_metadata(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    for (tag, value) in expected {
        assert_eq!(
            metadata.get_string(&format!("{group}:{tag}")),
            Some(*value),
            "{file} {group}:{tag}"
        );
    }
}

/// A little-endian (`II`) APP0 CIFF, including a `%Canon::FocalLength`
/// record (`[CIFF]`, not `[Canon]`: ExifTool.pm:7734 sets `SET_GROUP1`).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Canon"]
fn little_endian_app0_ciff_matches_pinned_exiftool() {
    assert_tags(
        "Canon/CanonPowerShotPro70.jpg",
        "CIFF",
        &[
            ("FileFormat", "JPEG (lossy)"),
            ("ImageWidth", "768"),
            ("ImageHeight", "512"),
            ("FileNumber", "43"),
            ("DateTimeOriginal", "1998:10:23 10:56:08"),
            ("OriginalFileName", "AUT_0043.JPG"),
            ("ShutterSpeedValue", "1/166"),
            ("ApertureValue", "2.4"),
            ("MeasuredEV", "14.90625"),
            ("CanonFileDescription", "Full automatic mode"),
            ("Model", "Canon PowerShot Pro70"),
            ("FocalType", "Zoom"),
            ("FocalLength", "419 mm"),
            ("FocalPlaneXSize", "7.85 mm"),
        ],
    );
}

/// A big-endian (`MM`) APP0 CIFF (ExifTool.pm:7730 accepts `(II|MM)`).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Canon"]
fn big_endian_app0_ciff_matches_pinned_exiftool() {
    assert_tags(
        "Canon/CanonPowerShot600.jpg",
        "CIFF",
        &[
            ("FileFormat", "JPEG (lossy/non-quantization toggled)"),
            ("ImageWidth", "832"),
            ("ImageHeight", "608"),
            ("ColorBW", "257"),
            ("RecordID", "58"),
            ("FileNumber", "3"),
            ("DateTimeOriginal", "1970:01:01 15:11:20"),
            ("TimeZoneCode", "-9"),
            ("ShutterSpeedValue", "1/128"),
            ("ApertureValue", "8.6"),
            ("FlashThreshold", "8.5"),
            ("OwnerName", "111"),
            ("ComponentVersion", "Component version 1.00"),
        ],
    );
}

/// Leica5/Leica8 0x05ff `CameraIFD` and 0x0408 `OriginalDirectory`.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Leica"]
fn leica_q3_camera_ifd_matches_pinned_exiftool() {
    assert_tags(
        "Leica/LeicaQ3.jpg",
        "PanasonicRaw",
        &[
            ("MultishotOn", "No"),
            ("FocusStepNear", "280"),
            ("FocusStepCount", "296"),
            ("LensTypeModel", "01 f0"),
            ("FocalLengthIn35mmFormat", "28 mm"),
            ("ApertureValue", "1.8"),
            ("ShutterSpeedValue", "1/56"),
            ("SensitivityValue", "2.54296875"),
            ("WB_RedLevelAuto", "1385"),
            ("Orientation", "Horizontal (normal)"),
            ("WhiteBalanceDetected", "Auto"),
        ],
    );
    assert_tags(
        "Leica/LeicaQ3.jpg",
        "Leica",
        &[("OriginalDirectory", "100LEICA")],
    );
}
