//! Real JPEG parser and CLI regression for binary display versus payload state.
//! Expected output is pinned ExifTool 13.59's `-s`, `-n -s`, and `-b` output
//! on these same synthetic MPF and XMP APP segments.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const BASE_JPEG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/jpeg/sample_with_exif.jpg"
);

fn with_segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let base = fs::read(BASE_JPEG).unwrap();
    let mut jpeg = base[..2].to_vec();
    jpeg.extend([0xff, marker]);
    jpeg.extend(u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
    jpeg.extend_from_slice(payload);
    jpeg.extend_from_slice(&base[2..]);
    jpeg
}

fn mpf_uid_jpeg(count: usize) -> (Vec<u8>, Vec<u8>) {
    let uid: Vec<u8> = (0..count).map(|i| i as u8).collect();
    let mut app2 = b"MPF\0II\x2a\0\x08\0\0\0".to_vec();
    app2.extend_from_slice(&1u16.to_le_bytes());
    app2.extend_from_slice(&0xb003u16.to_le_bytes());
    app2.extend_from_slice(&7u16.to_le_bytes());
    app2.extend_from_slice(&(count as u32).to_le_bytes());
    app2.extend_from_slice(&26u32.to_le_bytes());
    app2.extend_from_slice(&0u32.to_le_bytes());
    app2.extend_from_slice(&uid);
    (with_segment(0xe2, &app2), uid)
}

fn run(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .unwrap()
}

#[test]
fn large_mpf_uid_stays_visible_and_extracts_exact_bytes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("uid-264.jpg");
    let (jpeg, uid) = mpf_uid_jpeg(264);
    fs::write(&path, jpeg).unwrap();

    for args in [
        &["-s", "-ImageUIDList"][..],
        &["-s", "--no-print-conv", "-ImageUIDList"][..],
    ] {
        let output = run(args, &path);
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "ImageUIDList                    : (Binary data 264 bytes, use -b option to extract)\n"
        );
    }
    let binary = run(&["-b", "-ImageUIDList"], &path);
    assert!(binary.status.success(), "{:?}", binary);
    assert_eq!(binary.stdout, uid);
}

#[test]
fn xmp_literal_binary_summary_is_extractable_text() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("literal.jpg");
    const LITERAL: &str = "(Binary data 123 bytes, use -b option to extract)";
    let xmp = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" dc:description=\"{LITERAL}\"/></rdf:RDF></x:xmpmeta>"
    );
    let mut app1 = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    app1.extend_from_slice(xmp.as_bytes());
    fs::write(&path, with_segment(0xe1, &app1)).unwrap();

    let short = run(&["-s3", "-Description"], &path);
    assert!(short.status.success(), "{:?}", short);
    assert_eq!(short.stdout, format!("{LITERAL}\n").as_bytes());
    let binary = run(&["-b", "-Description"], &path);
    assert!(binary.status.success(), "{:?}", binary);
    assert_eq!(binary.stdout, LITERAL.as_bytes());
}

#[test]
fn binary_orientation_uses_value_conversion() {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/jpeg/edge_cases/orientation_1.jpg"
    ));
    let output = run(&["-b", "-Orientation"], path);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"3");
}

#[test]
fn empty_directory_binary_scan_keeps_stdout_empty() {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-b", "-Make"])
        .arg(first.path())
        .arg(second.path())
        .output()
        .unwrap();
    assert!(output.stdout.is_empty(), "{:?}", output);
}

#[test]
fn icc_red_trc_extracts_retained_curve_bytes() {
    // The 536-byte ICC profile was extracted from pinned ExifTool 13.59's
    // Apple_iPadAir_3rd_generation.jpg corpus sample. The 32-byte expected
    // payload is that same native oracle's `-b -RedTRC` output.
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/icc/red_trc_apple.icc"
    ));
    let ordinary = run(&["-s", "-RedTRC"], path);
    assert!(ordinary.status.success(), "{ordinary:?}");
    assert!(
        String::from_utf8_lossy(&ordinary.stdout)
            .contains("(Binary data 32 bytes, use -b option to extract)")
    );

    let binary = run(&["-b", "-RedTRC"], path);
    assert!(binary.status.success(), "{binary:?}");
    assert_eq!(
        binary.stdout,
        include_bytes!("fixtures/icc/red_trc_apple.bin")
    );

    // The same profile through JPEG APP2 exercises the shared ICC insertion
    // and container merge path that the original review finding reached.
    let dir = TempDir::new().unwrap();
    let jpeg_path = dir.path().join("embedded-redtrc.jpg");
    let mut app2 = b"ICC_PROFILE\0\x01\x01".to_vec();
    app2.extend_from_slice(include_bytes!("fixtures/icc/red_trc_apple.icc"));
    fs::write(&jpeg_path, with_segment(0xe2, &app2)).unwrap();
    let embedded = run(&["-b", "-RedTRC"], &jpeg_path);
    assert!(embedded.status.success(), "{embedded:?}");
    assert_eq!(
        embedded.stdout,
        include_bytes!("fixtures/icc/red_trc_apple.bin")
    );
}
