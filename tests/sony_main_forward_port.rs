//! Sony MakerNote fields forward-ported from `origin/main` (P5 Sony in
//! docs/reference/main-divergence-2026-09-18.md): 9b215f03 and badda311
//! (`MoreSettings`), ed2982e1 (`MoreSettings` later-body arms), c8887915 and
//! badda311 (`SONY PIC` text blocks), badda311 (`CameraInfo3`) and 12a2b7d7
//! (`PixelShiftInfo`, `HiddenInfo`). The census that found them counted 119
//! `Sony:*` occurrences matched on main and MISSING on the tip across 48
//! combined-samples/Sony files.
//!
//! Expected values are the pinned oracle's (ExifTool 13.59 under perl 5.38.2,
//! `-ver` 13.59 and the OOXML.docx probe `DOCX` asserted first):
//!
//! ```text
//! $ et.sh -G1 -a -s -Sony:<tags> SonyDSC-H300.jpg SonyDSC-W370.jpg ...
//! ======== SonyDSC-H300.jpg   Barcode A0D9P7016135, TextInfo1 769 bytes, TextInfo2 671 bytes
//! ======== SonyDSC-W370.jpg   Barcode A0A2LU014804, TextInfo1 660, TextInfo2 780, BoardTemperature 28 C
//! ======== SonyMHS-TS20.jpg   Barcode -1066078184, TextInfo1 650, TextInfo2 867
//! ======== SonyZV-E10M2.jpg   PixelShiftInfo n/a, HiddenDataOffset 13938688, HiddenDataLength 53248
//! ======== SonyDSLR-A550.jpg  CustomWB_RBLevels 0 0, ExposureCompensation2 0,
//!                             FlashExposureCompSet2 0, FocalLength2 19.2 mm,
//!                             FocusPosition2 190, Orientation2 Horizontal (normal)
//! ======== SonyDSLR-A580.jpg  FocalLengthTeleZoom 70.0 mm, FlashActionExternal Did not fire,
//!                             LiveViewAFMethod Phase-detect AF
//! ======== SonyNEX-C3.jpg     FocalLengthTeleZoom 18.0 mm, FlashActionExternal Did not fire,
//!                             LiveViewAFMethod Contrast AF
//! ```

use oxidex::core::operations::read_metadata;
use std::path::Path;

const CORPUS: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Sony";

fn assert_fields(file: &str, expected: &[(&str, &str)]) {
    let path = Path::new(CORPUS).join(file);
    let metadata = read_metadata(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    for (tag, value) in expected {
        assert_eq!(
            metadata.get_string(&format!("Sony:{tag}")),
            Some(*value),
            "{file} Sony:{tag}"
        );
    }
}

#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Sony"]
fn sony_pic_text_blocks_match_pinned_exiftool() {
    assert_fields(
        "SonyDSC-H300.jpg",
        &[
            ("Barcode", "A0D9P7016135"),
            (
                "TextInfo1",
                "(Binary data 769 bytes, use -b option to extract)",
            ),
            (
                "TextInfo2",
                "(Binary data 671 bytes, use -b option to extract)",
            ),
        ],
    );
    assert_fields(
        "SonyDSC-W370.jpg",
        &[
            ("Barcode", "A0A2LU014804"),
            ("BoardTemperature", "28 C"),
            (
                "TextInfo1",
                "(Binary data 660 bytes, use -b option to extract)",
            ),
            (
                "TextInfo2",
                "(Binary data 780 bytes, use -b option to extract)",
            ),
        ],
    );
    assert_fields(
        "SonyMHS-TS20.jpg",
        &[
            ("Barcode", "-1066078184"),
            (
                "TextInfo1",
                "(Binary data 650 bytes, use -b option to extract)",
            ),
        ],
    );
}

#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Sony"]
fn sony_hidden_and_pixel_shift_info_match_pinned_exiftool() {
    assert_fields(
        "SonyZV-E10M2.jpg",
        &[
            ("PixelShiftInfo", "n/a"),
            ("HiddenDataOffset", "13938688"),
            ("HiddenDataLength", "53248"),
        ],
    );
}

#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Sony"]
fn sony_more_settings_and_camera_info3_match_pinned_exiftool() {
    assert_fields(
        "SonyDSLR-A550.jpg",
        &[
            ("CustomWB_RBLevels", "0 0"),
            ("ExposureCompensation2", "0"),
            ("FlashExposureCompSet2", "0"),
            ("FocalLength2", "19.2 mm"),
            ("FocusPosition2", "190"),
            ("Orientation2", "Horizontal (normal)"),
        ],
    );
    assert_fields(
        "SonyDSLR-A580.jpg",
        &[
            ("FocalLengthTeleZoom", "70.0 mm"),
            ("FlashActionExternal", "Did not fire"),
            ("LiveViewAFMethod", "Phase-detect AF"),
        ],
    );
    assert_fields(
        "SonyNEX-C3.jpg",
        &[
            ("FocalLengthTeleZoom", "18.0 mm"),
            ("FlashActionExternal", "Did not fire"),
            ("LiveViewAFMethod", "Contrast AF"),
        ],
    );
}
