//! Real JPEG parser and CLI regression for binary display versus payload state.
//! Expected output is pinned ExifTool 13.59's `-s`, `-n -s`, and `-b` output
//! on these same synthetic MPF and XMP APP segments.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

#[path = "common/fixtures.rs"]
mod fixtures;

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
fn native_iptc_list_keeps_newline_separator_in_binary_output() {
    let Some(iptc) = fixtures::pinned_t_images_fixture_path("IPTC.jpg") else {
        return;
    };
    // Pinned ExifTool 13.59: `-b -Keywords t/images/IPTC.jpg`.
    let extracted = run(&["-b", "-Keywords"], &iptc);
    assert!(extracted.status.success(), "{extracted:?}");
    assert_eq!(extracted.stdout, b"ExifTool\nTest\nIPTC");
}

#[test]
fn native_numeric_tuple_keeps_space_separator_in_binary_output() {
    let Some(pgf) = fixtures::pinned_t_images_fixture_path("PGF.pgf") else {
        return;
    };
    // Pinned ExifTool 13.59: `-b -BackgroundColor t/images/PGF.pgf` is `0 0 0`.
    let extracted = run(&["-b", "-BackgroundColor"], &pgf);
    assert!(extracted.status.success(), "{extracted:?}");
    assert_eq!(extracted.stdout, b"0 0 0");
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
fn xmp_declared_binary_summary_refuses_without_payload() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("depth.xmp");
    fs::write(&path, r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:GDepth="http://ns.google.com/photos/1.0/depthmap/"><rdf:Description GDepth:DepthImage="AQIDBA=="/></rdf:RDF></x:xmpmeta>"#).unwrap();
    let binary = run(&["-b", "-DepthImage"], &path);
    assert!(!binary.status.success());
    assert!(binary.stdout.is_empty());
    assert!(String::from_utf8_lossy(&binary.stderr).contains("binary payload is unavailable"));
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

#[test]
fn png_palette_extracts_chunk_payload_instead_of_display_summary() {
    // Expected bytes are pinned ExifTool 13.59's `-b -Palette` result for
    // this existing PNG fixture (741 bytes, SHA-256 44fd7ac15e4400e1...).
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/png/sample.png"
    ));
    let output = run(&["-b", "-Palette"], path);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        include_bytes!("fixtures/png/palette_sample.bin")
    );
}

#[test]
fn png_short_palette_extracts_source_value_conversion() {
    // PNG.pm's PLTE ValueConv emits decimal components for up to three
    // source bytes; pinned ExifTool 13.59 outputs "255 0 0" under -b.
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/png/palette_one_red.png"
    ));
    let output = run(&["-b", "-Palette"], path);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"255 0 0");
}

#[test]
fn xisf_binary_tags_extract_native_source_bytes() {
    // Fixture and expected payloads come from pinned ExifTool 13.59's
    // t/images/XISF.xisf. Both XML and ImageData have ordinary summaries.
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/xisf/pinned.xisf"
    ));
    for (name, expected) in [
        ("-XML", &include_bytes!("fixtures/xisf/xml_header.bin")[..]),
        (
            "-ImageData",
            &include_bytes!("fixtures/xisf/image_data.bin")[..],
        ),
    ] {
        let output = run(&["-b", name], path);
        assert!(output.status.success(), "{name}: {output:?}");
        assert_eq!(output.stdout, expected, "{name}");
    }
}

#[test]
fn czi_xml_extracts_native_source_block() {
    // Pinned ExifTool 13.59's t/images/ZISRAW.czi and -b -XML result.
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/czi/pinned.czi"
    ));
    let has_table = oxidex::exiftool_tables::find_table("ZISRAW", "Main").is_some();
    match oxidex::exiftool_tables::EXIFTOOL_VERSION {
        "11.78" => assert!(!has_table, "11.78 has no native ZISRAW source"),
        "12.64" | "13.59" => assert!(has_table, "capable pin lost ZISRAW::Main"),
        pin => panic!("unverified CZI source capability for ExifTool {pin}"),
    }
    if !has_table {
        let identity = run(&["-b", "-FileType"], path);
        assert!(identity.status.success(), "{identity:?}");
        assert_eq!(identity.stdout, b"Unknown");
        let property = run(&["-b", "-XML:MicroscopeName"], path);
        assert!(!property.status.success(), "{property:?}");
        assert!(property.stdout.is_empty(), "{property:?}");
        assert!(
            String::from_utf8_lossy(&property.stderr).contains("missing ZISRAW::Main table"),
            "{property:?}"
        );
        let unknown = run(&["-b", "-NoSuchTag"], path);
        assert!(unknown.status.success(), "{unknown:?}");
        assert!(unknown.stdout.is_empty(), "{unknown:?}");
        let wildcard = run(&["-b", "-NoSuch*"], path);
        assert!(!wildcard.status.success(), "{wildcard:?}");
        assert!(wildcard.stdout.is_empty(), "{wildcard:?}");
        assert!(
            String::from_utf8_lossy(&wildcard.stderr)
                .contains("-b with wildcard tag requests is not supported"),
            "{wildcard:?}"
        );
    }
    let output = run(&["-b", "-XML"], path);
    if has_table {
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, include_bytes!("fixtures/czi/xml_header.bin"));
    } else {
        assert!(!output.status.success(), "missing CZI table must refuse -b");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("missing ZISRAW::Main table"),
            "{output:?}"
        );
    }
}

#[test]
fn czi_missing_table_refuses_non_fixture_xml_property() {
    let mut czi = fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/czi/pinned.czi"
    ))
    .unwrap();
    let section = u64::from_le_bytes(czi[92..100].try_into().unwrap()) as usize;
    let xml_len = u32::from_le_bytes(czi[section + 32..section + 36].try_into().unwrap()) as usize;
    let xml_start = section + 288;
    let xml_end = xml_start + xml_len;
    let xml = std::str::from_utf8(&czi[xml_start..xml_end]).unwrap();
    let expanded = xml.replace(
        "    </HardwareSetting>",
        "      <Detectors><Detector Name=\"Detector-1\"/></Detectors>\n    </HardwareSetting>",
    );
    assert_ne!(expanded, xml, "fixture lost HardwareSetting XML section");
    czi.splice(xml_start..xml_end, expanded.bytes());
    czi[section + 32..section + 36].copy_from_slice(&(expanded.len() as u32).to_le_bytes());
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("other-property.czi");
    fs::write(&path, czi).unwrap();

    let has_table = oxidex::exiftool_tables::find_table("ZISRAW", "Main").is_some();
    for tag in ["-XML:DetectorName", "-DetectorName"] {
        let output = run(&["-b", tag], &path);
        if has_table {
            assert!(output.status.success(), "{tag}: {output:?}");
            assert_eq!(output.stdout, b"Detector-1", "{tag}");
        } else {
            assert!(!output.status.success(), "{tag}: {output:?}");
            assert!(output.stdout.is_empty(), "{tag}: {output:?}");
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            assert!(
                diagnostic.contains("missing ZISRAW::Main table"),
                "{tag}: {output:?}"
            );
            assert!(diagnostic.contains(&tag[1..]), "{tag}: {output:?}");
        }
    }
    let unknown = run(&["-b", "-XML:NoSuchTag"], &path);
    assert!(unknown.status.success(), "{unknown:?}");
    assert!(unknown.stdout.is_empty(), "{unknown:?}");
}

#[test]
fn czi_all_occurrences_refuses_missing_xml_collision() {
    let mut czi = fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/czi/pinned.czi"
    ))
    .unwrap();
    let section = u64::from_le_bytes(czi[92..100].try_into().unwrap()) as usize;
    let xml_len = u32::from_le_bytes(czi[section + 32..section + 36].try_into().unwrap()) as usize;
    let xml_start = section + 288;
    let xml_end = xml_start + xml_len;
    let xml = std::str::from_utf8(&czi[xml_start..xml_end]).unwrap();
    let expanded = xml.replace("<Metadata>", "<Metadata>\n    <FileType>camera</FileType>");
    assert_ne!(expanded, xml, "fixture lost Metadata XML section");
    czi.splice(xml_start..xml_end, expanded.bytes());
    czi[section + 32..section + 36].copy_from_slice(&(expanded.len() as u32).to_le_bytes());
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("colliding-filetype.czi");
    fs::write(&path, czi).unwrap();

    let output = run(&["-a", "-b", "-FileType"], &path);
    if oxidex::exiftool_tables::find_table("ZISRAW", "Main").is_some() {
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"CZIcamera", "{output:?}");
    } else {
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains("missing ZISRAW::Main table"),
            "{output:?}"
        );
        assert!(diagnostic.contains("FileType"), "{output:?}");
    }
}

#[test]
fn czi_missing_table_never_leaves_partial_multi_file_binary_output() {
    if oxidex::exiftool_tables::EXIFTOOL_VERSION != "11.78" {
        return;
    }
    let valid = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/icc/red_trc_apple.icc"
    ));
    let czi = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/czi/pinned.czi"
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-b", "-RedTRC", "-XML"])
        .arg(valid)
        .arg(czi)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.contains("missing ZISRAW::Main table"),
        "{output:?}"
    );
    assert!(diagnostic.contains("XML"), "{output:?}");
}

#[test]
fn printed_scalar_requests_extract_valueconv_not_the_label() {
    let png = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/png/sample.png"
    ));
    for (name, expected) in [
        ("-PNG:ColorType", "3"),
        ("-PNG:Compression", "0"),
        ("-PNG:Filter", "0"),
        ("-PNG:Interlace", "0"),
        ("-PNG-pHYs:PixelUnits", "0"),
    ] {
        let output = run(&["-b", name], png);
        assert!(output.status.success(), "{name}: {output:?}");
        assert_eq!(output.stdout, expected.as_bytes(), "{name}");
    }
    let Some(mp3) = fixtures::pinned_t_images_fixture_path("MP3.mp3") else {
        return;
    };
    for (name, expected) in [
        ("-MPEG:MPEGAudioVersion", "3"),
        ("-MPEG:AudioLayer", "1"),
        ("-MPEG:AudioBitrate", "128000"),
        ("-MPEG:SampleRate", "0"),
        ("-MPEG:ChannelMode", "1"),
        ("-MPEG:MSStereo", "1"),
        ("-MPEG:IntensityStereo", "0"),
        ("-MPEG:CopyrightFlag", "1"),
        ("-MPEG:OriginalMedia", "1"),
        ("-MPEG:Emphasis", "0"),
    ] {
        let output = run(&["-b", name], &mp3);
        assert!(output.status.success(), "{name}: {output:?}");
        assert_eq!(output.stdout, expected.as_bytes(), "{name}");
    }
}

#[test]
fn nikon_declared_binary_curve_never_extracts_its_summary_as_payload() {
    let Some(nef) = fixtures::pinned_t_images_fixture_path("Nikon.nef") else {
        return;
    };
    let output = run(&["-b", "-Nikon:ContrastCurve"], &nef);
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("binary payload is unavailable"));

    if let Some(rw2) = fixtures::pinned_t_images_fixture_path("Panasonic.rw2") {
        let data_dump = run(&["-b", "-Panasonic:DataDump"], &rw2);
        assert!(!data_dump.status.success(), "{data_dump:?}");
        assert!(data_dump.stdout.is_empty(), "{data_dump:?}");
        assert!(
            String::from_utf8_lossy(&data_dump.stderr).contains("binary payload is unavailable")
        );
    }
}

#[test]
fn invalid_cr3_preview_keeps_default_summary_but_refuses_binary_request() {
    let Some(cr3) = fixtures::pinned_t_images_fixture_path("CanonRaw.cr3") else {
        return;
    };
    let ordinary = run(&["-s"], &cr3);
    assert!(ordinary.status.success(), "{ordinary:?}");
    assert!(
        String::from_utf8_lossy(&ordinary.stdout)
            .lines()
            .any(|line| line.starts_with("PreviewImage") && line.contains("Binary data 26 bytes"))
    );
    let output = run(&["-b", "-PreviewImage"], &cr3);
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("binary payload is unavailable"));
}

#[test]
fn pdf_icc_curve_retains_the_embedded_source_bytes() {
    // A minimal ICCBased PDF embedding red_trc_apple.icc. Pinned ExifTool
    // 13.59 reports ICC_Profile:RedTRC and extracts the exact 32 bytes below.
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/pdf/icc_redtrc.pdf"
    ));
    let ordinary = run(&["-s", "-ICC_Profile:RedTRC"], path);
    assert!(ordinary.status.success(), "{ordinary:?}");
    assert!(
        String::from_utf8_lossy(&ordinary.stdout).contains("Binary data 32 bytes"),
        "{ordinary:?}"
    );
    let extracted = run(&["-b", "-ICC_Profile:RedTRC"], path);
    assert!(extracted.status.success(), "{extracted:?}");
    assert_eq!(
        extracted.stdout,
        include_bytes!("fixtures/icc/red_trc_apple.bin")
    );
}
