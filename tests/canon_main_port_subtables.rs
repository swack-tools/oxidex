//! Canon sub-table fixes forward-ported from `origin/main` (#699, #703, #705,
//! #707, #708, #709, #718) onto `refactor/tag-machinery`, pinned per tag to
//! real carriers.
//!
//! main's versions of these tests returned early when the fixture was absent
//! (a silent pass); here each is `#[ignore = "needs ..."]` and panics on a
//! missing fixture, so run them with
//! `cargo test --test canon_main_port_subtables -- --include-ignored`.
//!
//! Every expectation is the pinned ExifTool 13.59 oracle's output, both probes
//! asserted first as `AGENTS.md` requires:
//!
//! ```text
//! $ exiftool-pinned -ver                                   -> 13.59
//! $ exiftool-pinned -s3 -FileType t/images/OOXML.docx      -> DOCX
//! $ exiftool-pinned -a -G1 -s -Canon:<tag> <carrier>
//! ```

use oxidex::core::MetadataMap;
use oxidex::core::operations::read_metadata;
#[path = "common/fixtures.rs"]
mod fixtures;

fn carrier(relative: &str) -> MetadataMap {
    let path = fixtures::pinned_combined_fixture_path(relative)
        .unwrap_or_else(|| panic!("{relative} is part of the pinned corpus"));
    assert!(
        path.is_file(),
        "{} is part of the pinned corpus",
        path.display()
    );
    read_metadata(&path).unwrap_or_else(|error| panic!("{relative}: {error}"))
}

/// The value as `-j` prints it: a string, or an integer (`FaceWidth`, ...).
fn shown(metadata: &MetadataMap, key: &str) -> Option<String> {
    let value = metadata.get(key)?;
    value
        .as_string()
        .map(str::to_string)
        .or_else(|| value.as_integer().map(|i| i.to_string()))
}

fn assert_tags(relative: &str, expected: &[(&str, &str)]) {
    let metadata = carrier(relative);
    for (key, want) in expected {
        assert_eq!(
            shown(&metadata, key).as_deref(),
            Some(*want),
            "{relative}: {key}"
        );
    }
}

/// `%Canon::AFConfig` (hand `binary_tables`): key 1's `$val + 1` ValueConv
/// and the model-conditioned keys 7, 10, 18 and 19. From main c8887915,
/// 8e2af335 and 8b91de9f; plus `%Canon::CameraSettings` key 52 (badda311).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Canon"]
fn eos_r6m2_af_config_and_hdr_pq() {
    assert_tags(
        "Canon/CanonEOS_R6m2.jpg",
        &[
            ("Canon:AFConfigTool", "Case A"),
            ("Canon:USMLensElectronicMF", "One-Shot -> Enabled (magnify)"),
            ("Canon:AFStatusViewfinder", "Show in Field of View"),
            ("Canon:InitialAFPointInServo", "Auto"),
            ("Canon:AutoAFPointSelEOSiTRAF", "Enable"),
            ("Canon:HDR-PQ", "Off"),
        ],
    );
}

/// `%Canon::FileInfo` key 6, `RawConv => '$val<=0 ? undef : $val'` then
/// `%canonQuality` (badda311).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Canon"]
fn eos_m10_raw_jpg_quality() {
    assert_tags("Canon/CanonEOS_M10.jpg", &[("Canon:RawJpgQuality", "Fine")]);
}

/// `%Canon::FaceDetect2` through the generated table (95d16186).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Canon"]
fn powershot_a560_face_detect2() {
    assert_tags(
        "Canon/CanonPowerShotA560.jpg",
        &[("Canon:FaceWidth", "35"), ("Canon:FacesDetected", "1")],
    );
}

/// EOS-1D: Main 0x0094 and the six `%longBin` residual rows (8b91de9f), and
/// the two generated `%Canon::ModifiedInfo` fields (12a2b7d7).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Canon"]
fn eos_1d_residual_rows_and_modified_info() {
    assert_tags(
        "Canon/CanonEOS-1D.jpg",
        &[
            ("Canon:AFPointsInFocus1D", "Auto (B6,B7,C6,C7,C8,D6,D7,D8)"),
            (
                "Canon:ToneCurveTable",
                "(Binary data 1679 bytes, use -b option to extract)",
            ),
            ("Canon:SharpnessTable", "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"),
            (
                "Canon:SharpnessFreqTable",
                "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
            ),
            (
                "Canon:WhiteBalanceTable",
                "(Binary data 2217 bytes, use -b option to extract)",
            ),
            (
                "Canon:ToneCurveMatching",
                "(Binary data 95 bytes, use -b option to extract)",
            ),
            (
                "Canon:WhiteBalanceMatching",
                "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
            ),
            ("Canon:ModifiedSharpness", "0"),
            ("Canon:ModifiedDigitalGain", "0"),
        ],
    );
}

/// `%Canon::SerialInfo`, the `/EOS 5D/` alternative of Main 0x0096 (95d16186).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Canon"]
fn eos_5d_mark_iii_serial_info() {
    let metadata = carrier("Canon/CanonEOS5D_MarkIII.jpg");
    assert_eq!(
        shown(&metadata, "Canon:InternalSerialNumber2").as_deref(),
        Some("AD0010003")
    );
    // The oracle prints no InternalSerialNumber for this body: key 9 fails
    // its `/^\w{6}/` RawConv, and the Main string alternative lost.
    assert_eq!(shown(&metadata, "Canon:InternalSerialNumber"), None);
}

/// `%Canon::ColorBalance` through the generated table, both sides of the
/// key-29 `/EOS D60\b/` pair (95d16186).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples"]
fn color_balance_both_key_29_alternatives() {
    assert_tags(
        "Canon.jpg",
        &[
            ("Canon:WB_RGGBLevelsAuto", "1719 832 831 990"),
            ("Canon:WB_RGGBLevelsTungsten", "1228 913 912 1668"),
            ("Canon:WB_RGGBLevelsCustom", "1722 832 831 989"),
            ("Canon:WB_RGGBLevelsKelvin", "1722 832 831 988"),
            ("Canon:WB_RGGBBlackLevels", "124 123 124 123"),
        ],
    );
    let d60 = carrier("Canon/CanonEOS_D60.jpg");
    assert_eq!(
        shown(&d60, "Canon:BlackLevels").as_deref(),
        Some("128 128 128 128")
    );
    assert_eq!(
        shown(&d60, "Canon:WB_RGGBLevelsFlash").as_deref(),
        Some("1895 826 819 1066")
    );
    assert_eq!(shown(&d60, "Canon:WB_RGGBLevelsCustom"), None);
}

/// `Canon::ReadODD` behind `Composite:OriginalDecisionData` (bb326810).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples"]
fn canon_1d_mark_iii_original_decision_data() {
    let metadata = carrier("Canon1DmkIII.jpg");
    let Some(oxidex::core::TagValue::Binary(block)) =
        metadata.get("Composite:OriginalDecisionData")
    else {
        panic!("Composite:OriginalDecisionData is a 512-byte binary block");
    };
    assert_eq!(block.len(), 512);
    assert_eq!(block[..4], [0xff; 4]);
}
