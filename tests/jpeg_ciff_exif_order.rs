//! A late Canon CIFF APP0 must not move ahead of a newly written EXIF APP1.
//! Pinned ExifTool 13.59 reads the CIFF custom-function row only when the
//! Canon EOS 10D Model in EXIF precedes this particular APP0.

use oxidex::exiftool_oracle::{self, Oracle};
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const CARRIER: &[u8] = include_bytes!("fixtures/jpeg/ciff_order/late-ciff-app0.jpg");

fn without_first_exif(jpeg: &[u8]) -> Vec<u8> {
    assert!(jpeg.starts_with(&[0xff, 0xd8, 0xff, 0xe1]));
    let length = u16::from_be_bytes([jpeg[4], jpeg[5]]) as usize;
    assert!(jpeg[6..].starts_with(b"Exif\0\0"));
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&jpeg[4 + length..]);
    out
}

fn exif_and_ciff_offsets(jpeg: &[u8]) -> (usize, usize) {
    assert!(jpeg.starts_with(&[0xff, 0xd8]));
    let (mut exif, mut ciff) = (None, None);
    let mut offset = 2;
    while offset + 4 <= jpeg.len() && jpeg[offset] == 0xff {
        let marker = jpeg[offset + 1];
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        let length = u16::from_be_bytes([jpeg[offset + 2], jpeg[offset + 3]]) as usize;
        assert!(length >= 2 && offset + 2 + length <= jpeg.len());
        let payload = &jpeg[offset + 4..offset + 2 + length];
        if marker == 0xe1 && payload.starts_with(b"Exif\0\0") {
            assert!(exif.replace(offset).is_none(), "multiple EXIF APP1s");
        }
        if marker == 0xe0 && payload.windows(8).any(|part| part == b"HEAPJPGM") {
            assert!(ciff.replace(offset).is_none(), "multiple CIFF APP0s");
        }
        offset += 2 + length;
    }
    (exif.expect("EXIF APP1"), ciff.expect("CIFF APP0"))
}

fn native_rows(oracle: &Oracle, path: &Path) -> Value {
    let output = oracle
        .command()
        .args(["-j", "-a", "-G1", "-n", "-SetButtonWhenShooting", "-Model"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice::<Vec<Value>>(&output.stdout)
        .unwrap()
        .remove(0)
}

fn compare_with_native(oracle: &Oracle, original: &[u8], args: &[&str]) {
    let dir = TempDir::new().unwrap();
    let native = dir.path().join("native.jpg");
    let ours = dir.path().join("ours.jpg");
    fs::write(&native, original).unwrap();
    fs::write(&ours, original).unwrap();

    let expected = oracle
        .command()
        .args(["-m", "-overwrite_original"])
        .args(args)
        .arg(&native)
        .output()
        .unwrap();
    assert!(expected.status.success(), "native {args:?}: {expected:?}");
    let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(&ours)
        .output()
        .unwrap();
    assert!(result.status.success(), "oxidex {args:?}: {result:?}");

    let native_bytes = fs::read(&native).unwrap();
    let ours_bytes = fs::read(&ours).unwrap();
    // The pinned 11.78/12.64 native writes place CIFF at 346; 13.59 places
    // it at 294. All three still require EXIF to precede the late CIFF.
    let expected_offsets = match exiftool_oracle::repo_pin() {
        "11.78" | "12.64" => (2, 346),
        "13.59" => (2, 294),
        other => panic!("unreviewed ExifTool release {other}"),
    };
    assert_eq!(exif_and_ciff_offsets(&native_bytes), expected_offsets);
    assert_eq!(exif_and_ciff_offsets(&ours_bytes), expected_offsets);
    // Only the EXIF segment may change: the late CIFF and image bytes remain.
    let preserved = if original.starts_with(&[0xff, 0xd8, 0xff, 0xe1]) {
        without_first_exif(original)
    } else {
        original.to_vec()
    };
    assert_eq!(without_first_exif(&native_bytes), preserved);
    assert_eq!(without_first_exif(&ours_bytes), preserved);

    let native_tags = native_rows(oracle, &native);
    let ours_tags = native_rows(oracle, &ours);
    assert_eq!(native_tags["IFD0:Model"], "Canon EOS 10D");
    assert_eq!(native_tags["CIFF:SetButtonWhenShooting"], 0);
    assert_eq!(ours_tags["IFD0:Model"], native_tags["IFD0:Model"]);
    assert_eq!(
        ours_tags["CIFF:SetButtonWhenShooting"],
        native_tags["CIFF:SetButtonWhenShooting"]
    );
}

#[test]
fn fresh_exif_precedes_late_ciff_and_preserves_its_raw_readback() {
    let oracle = exiftool_oracle::graded().expect("pinned ExifTool oracle required");
    compare_with_native(
        oracle,
        &without_first_exif(CARRIER),
        &["-IFD0:Model=Canon EOS 10D"],
    );
}

#[test]
fn clear_then_recreate_exif_preserves_late_ciff_readback() {
    let oracle = exiftool_oracle::graded().expect("pinned ExifTool oracle required");
    compare_with_native(
        oracle,
        CARRIER,
        &["-EXIF:All=", "-IFD0:Model=Canon EOS 10D"],
    );
}
