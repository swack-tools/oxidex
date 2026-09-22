use oxidex::core::TagValue;
use oxidex::core::exiftool_compat::format_tag_value;
use oxidex::core::operations::read_metadata;
use oxidex::core::tag_conversion::raw_bytes_to_tag_value;
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use tempfile::NamedTempFile;

const BYTE: u16 = 1;
const SRATIONAL: u16 = 10;
const UNDEFINED: u16 = 7;

fn jpeg_with_exif_entries(entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();

    // IFD0 contains only the ExifIFD pointer. The pointed directory starts at
    // TIFF offset 26: 8-byte header + 2-byte count + 12-byte entry + 4-byte next.
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x8769u16.to_le_bytes());
    tiff.extend_from_slice(&4u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&26u32.to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());

    let directory_len = 2 + entries.len() * 12 + 4;
    let mut directory = Vec::with_capacity(directory_len);
    let mut tail = Vec::new();
    directory.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, field_type, bytes) in entries {
        directory.extend_from_slice(&tag.to_le_bytes());
        directory.extend_from_slice(&field_type.to_le_bytes());
        let element_size = match *field_type {
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => 1,
        };
        assert_eq!(bytes.len() % element_size, 0, "complete TIFF elements");
        directory.extend_from_slice(&((bytes.len() / element_size) as u32).to_le_bytes());
        if bytes.len() <= 4 {
            directory.extend_from_slice(bytes);
            directory.resize(directory.len() + 4 - bytes.len(), 0);
        } else {
            let offset = 26 + directory_len + tail.len();
            directory.extend_from_slice(&(offset as u32).to_le_bytes());
            tail.extend_from_slice(bytes);
        }
    }
    directory.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(&directory);
    tiff.extend_from_slice(&tail);

    let app1_len = 2 + 6 + tiff.len();
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
    jpeg.extend_from_slice(&(app1_len as u16).to_be_bytes());
    jpeg.extend_from_slice(b"Exif\0\0");
    jpeg.extend_from_slice(&tiff);
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg
}

fn tiff_with_ifd0_entries(entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    let directory_len = 2 + entries.len() * 12 + 4;
    let mut directory = Vec::with_capacity(directory_len);
    let mut tail = Vec::new();
    directory.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, field_type, bytes) in entries {
        directory.extend_from_slice(&tag.to_le_bytes());
        directory.extend_from_slice(&field_type.to_le_bytes());
        directory.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        if bytes.len() <= 4 {
            directory.extend_from_slice(bytes);
            directory.resize(directory.len() + 4 - bytes.len(), 0);
        } else {
            let offset = 8 + directory_len + tail.len();
            directory.extend_from_slice(&(offset as u32).to_le_bytes());
            tail.extend_from_slice(bytes);
        }
    }
    directory.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(&directory);
    tiff.extend_from_slice(&tail);
    tiff
}

fn jpeg_with_ifd0_entries(entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let tiff = tiff_with_ifd0_entries(entries);
    let app1_len = 2 + 6 + tiff.len();
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
    jpeg.extend_from_slice(&(app1_len as u16).to_be_bytes());
    jpeg.extend_from_slice(b"Exif\0\0");
    jpeg.extend_from_slice(&tiff);
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg
}

fn opcode_record(opcode: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&opcode.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn assert_single_opcode_list1(path: &std::path::Path) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-a", "-G1", "-s", "-OpcodeList1"])
        .arg(path)
        .output()
        .expect("runs oxidex duplicate projection");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| line.contains("OpcodeList1"))
            .count(),
        1,
        "the engine row must be consumed, not duplicated"
    );
}

fn assert_time_codes_cli_n(path: &std::path::Path, expected: &str) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
        // OxiDex reserves `-n` for dry-run; its ExifTool-compatible value
        // channel is deliberately spelled `--no-print-conv`.
        .args(["--no-print-conv", "-s", "-TimeCodes"])
        .arg(path)
        .output()
        .expect("runs oxidex ValueConv projection");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim().split_once(": ").map(|(_, value)| value),
        Some(expected)
    );
}

#[test]
fn opcode_lists_match_print_opcode() {
    let mut two_records = 2u32.to_be_bytes().to_vec();
    two_records.extend_from_slice(&opcode_record(1, &[]));
    two_records.extend_from_slice(&opcode_record(99, &[0xaa, 0xbb]));
    let expected_raw = two_records.clone();

    let mut truncated = 2u32.to_be_bytes().to_vec();
    truncated.extend_from_slice(&opcode_record(14, &[]));

    let jpeg = jpeg_with_exif_entries(&[
        (0xC740, UNDEFINED, two_records.clone()),
        (0xC741, UNDEFINED, truncated),
        (0xC74E, UNDEFINED, two_records),
    ]);
    let file = NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), jpeg).expect("writes JPEG fixture");
    let metadata = read_metadata(file.path()).expect("reads JPEG fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:OpcodeList1"),
        Some("WarpRectilinear, [opcode 99]")
    );
    assert_eq!(
        metadata.get_string("ExifIFD:OpcodeList2"),
        Some("WarpRectilinear2, <err>")
    );
    assert_eq!(
        metadata.get_string("ExifIFD:OpcodeList3"),
        Some("WarpRectilinear, [opcode 99]")
    );

    let without_print_conv = metadata.without_print_conv();
    assert_eq!(
        without_print_conv.get("ExifIFD:OpcodeList1"),
        Some(&TagValue::Binary(expected_raw)),
        "ConvertBinary must retain the exact UNDEFINED bytes before PrintOpcode"
    );
}

#[test]
fn ifd0_opcode_list_preserves_convert_binary_bytes() {
    let mut raw = 1u32.to_be_bytes().to_vec();
    raw.extend_from_slice(&opcode_record(9, &[0xde, 0xad, 0xbe, 0xef]));
    let jpeg = jpeg_with_ifd0_entries(&[(0xC740, UNDEFINED, raw.clone())]);
    let file = NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), jpeg).expect("writes JPEG fixture");

    let metadata = read_metadata(file.path()).expect("reads JPEG fixture");
    assert_eq!(metadata.get_string("IFD0:OpcodeList1"), Some("GainMap"));
    assert_single_opcode_list1(file.path());
    assert_eq!(
        metadata.without_print_conv().get("IFD0:OpcodeList1"),
        Some(&TagValue::Binary(raw.clone())),
        "JPEG IFD0 must retain the exact ConvertBinary payload"
    );

    let file = NamedTempFile::new().expect("creates TIFF fixture");
    std::fs::write(
        file.path(),
        tiff_with_ifd0_entries(&[(0xC740, UNDEFINED, raw.clone())]),
    )
    .expect("writes TIFF fixture");
    let metadata = read_metadata(file.path()).expect("reads TIFF fixture");
    assert_eq!(metadata.get_string("IFD0:OpcodeList1"), Some("GainMap"));
    assert_single_opcode_list1(file.path());
    assert_eq!(
        metadata.without_print_conv().get("IFD0:OpcodeList1"),
        Some(&TagValue::Binary(raw)),
        "TIFF IFD0 must retain the exact ConvertBinary payload"
    );
}

#[test]
fn time_codes_match_value_and_print_conversions() {
    let raw = vec![0x01, 0x02, 0x03, 0x04, 0, 0, 0, 0];
    let jpeg = jpeg_with_exif_entries(&[(0xC763, BYTE, raw.clone())]);
    let file = NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), jpeg).expect("writes JPEG fixture");
    let metadata = read_metadata(file.path()).expect("reads JPEG fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:TimeCodes"),
        Some("04:03:02.01")
    );
    assert_eq!(
        metadata
            .without_print_conv()
            .get_string("ExifIFD:TimeCodes"),
        Some("01.02.03.04.00.00.00.00")
    );
    assert_time_codes_cli_n(file.path(), "01.02.03.04.00.00.00.00");

    for bytes in [
        jpeg_with_ifd0_entries(&[(0xC763, BYTE, raw.clone())]),
        tiff_with_ifd0_entries(&[(0xC763, BYTE, raw.clone())]),
    ] {
        let file = NamedTempFile::new().expect("creates IFD0 fixture");
        std::fs::write(file.path(), bytes).expect("writes IFD0 fixture");
        let metadata = read_metadata(file.path()).expect("reads IFD0 fixture");
        assert_eq!(metadata.get_string("IFD0:TimeCodes"), Some("04:03:02.01"));
        assert_eq!(
            metadata.without_print_conv().get_string("IFD0:TimeCodes"),
            Some("01.02.03.04.00.00.00.00")
        );
        assert_time_codes_cli_n(file.path(), "01.02.03.04.00.00.00.00");
    }

    // Incomplete groups are ignored by both ValueConv and PrintConv.
    let jpeg = jpeg_with_exif_entries(&[(0xC763, BYTE, vec![1, 2, 3, 4, 5, 6, 7])]);
    let file = NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), jpeg).expect("writes JPEG fixture");
    let metadata = read_metadata(file.path()).expect("reads JPEG fixture");
    assert_eq!(metadata.get_string("ExifIFD:TimeCodes"), Some(""));

    let jpeg = jpeg_with_exif_entries(&[(
        0xC763,
        BYTE,
        vec![0x01, 0x02, 0x03, 0x84, 0x15, 0x09, 0x26, 0x00],
    )]);
    let file = NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), jpeg).expect("writes JPEG fixture");
    let metadata = read_metadata(file.path()).expect("reads JPEG fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:TimeCodes"),
        Some("2026-09-15T04:03:02.01+00:00")
    );

    // BGF2 plus the high timezone bit selects Modified Julian Date. 40587 is
    // the Unix epoch, and timezone code 0x28 is UTC.
    let jpeg = jpeg_with_exif_entries(&[(
        0xC763,
        BYTE,
        vec![0x01, 0x02, 0x03, 0x84, 0x87, 0x05, 0x04, 0xA8],
    )]);
    let file = NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), jpeg).expect("writes JPEG fixture");
    let metadata = read_metadata(file.path()).expect("reads JPEG fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:TimeCodes"),
        Some("1970-01-01T04:03:02.01+00:00")
    );

    // Non-BCD zone 0x0b is -01:30. Applying it to 01:00 at the Unix epoch
    // crosses the MJD day boundary instead of underflowing unsigned math.
    let jpeg = jpeg_with_exif_entries(&[(
        0xC763,
        BYTE,
        vec![0x00, 0x00, 0x00, 0x81, 0x87, 0x05, 0x04, 0x8b],
    )]);
    let file = NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), jpeg).expect("writes JPEG fixture");
    let metadata = read_metadata(file.path()).expect("reads JPEG fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:TimeCodes"),
        Some("1969-12-31T23:30:00.00-01:30")
    );

    // Perl numifies the malformed YY byte text "7a" as 7, not zero.
    let jpeg = jpeg_with_exif_entries(&[(0xC763, BYTE, vec![0, 0, 0, 0x80, 1, 1, 0x7a, 0])]);
    let file = NamedTempFile::new().expect("creates non-BCD year fixture");
    std::fs::write(file.path(), jpeg).expect("writes non-BCD year fixture");
    let metadata = read_metadata(file.path()).expect("reads non-BCD year fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:TimeCodes"),
        Some("2007-01-01T00:00:00.00+00:00")
    );

    // Reverse date bytes form "1e0001", which Perl treats as exponent syntax.
    let jpeg =
        jpeg_with_exif_entries(&[(0xC763, BYTE, vec![0, 0, 0, 0x80, 0x01, 0x00, 0x1e, 0x80])]);
    let file = NamedTempFile::new().expect("creates exponent MJD fixture");
    std::fs::write(file.path(), jpeg).expect("writes exponent MJD fixture");
    let metadata = read_metadata(file.path()).expect("reads exponent MJD fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:TimeCodes"),
        Some("1858-11-27T00:00:00.00+00:00")
    );

    // The source numeric domain is floating point. A finite exponent beyond
    // gmtime's range must reach ExifTool's overflow rendering without integer
    // multiplication overflow or wrapping to a plausible date.
    let jpeg = jpeg_with_exif_entries(&[(0xC763, BYTE, vec![0, 0, 0, 0x80, 0x15, 0, 0x1e, 0x80])]);
    let file = NamedTempFile::new().expect("creates finite-overflow MJD fixture");
    std::fs::write(file.path(), jpeg).expect("writes finite-overflow MJD fixture");
    let metadata = read_metadata(file.path()).expect("reads finite-overflow MJD fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:TimeCodes"),
        Some("1900-01-00T00:00:00.00+00:00")
    );

    // 9e9999 becomes infinity in native Perl; its failed gmtime rendering
    // retains the NaN fractional marker produced by ConvertUnixTime.
    let jpeg =
        jpeg_with_exif_entries(&[(0xC763, BYTE, vec![0, 0, 0, 0x80, 0x99, 0x99, 0x9e, 0x80])]);
    let file = NamedTempFile::new().expect("creates infinite MJD fixture");
    std::fs::write(file.path(), jpeg).expect("writes infinite MJD fixture");
    let metadata = read_metadata(file.path()).expect("reads infinite MJD fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:TimeCodes"),
        Some("1900-01-00T00:00:00NaN.00+00:00")
    );
}

#[test]
fn walker_owned_structural_refusals_are_classified_not_characterized() {
    let worklist = include_str!("../tools/exiftool-tables/exif_main_refusal_worklist.json");
    let value: serde_json::Value = serde_json::from_str(worklist).expect("valid worklist");
    let walker_ids = value["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|row| row["target_owner"] == "walker")
        .map(|row| row["id"].as_str().expect("id"))
        .collect::<Vec<_>>();
    assert_eq!(
        walker_ids,
        [
            "0x0111", "0x0117", "0x014a", "0x0201", "0x0202", "0x927c", "0xc634"
        ]
    );
    for row in value["rows"].as_array().expect("rows") {
        if row["target_owner"] == "walker" {
            assert_eq!(row["verification_status"], "classified_owner");
        }
    }
}

#[test]
fn stateful_refusals_are_explicitly_planned_not_characterized() {
    let worklist = include_str!("../tools/exiftool-tables/exif_main_refusal_worklist.json");
    let value: serde_json::Value = serde_json::from_str(worklist).expect("valid worklist");
    for id in ["0x00fe", "0x00ff", "0x0103"] {
        let row = value["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|row| row["id"] == id)
            .expect("stateful refusal row");
        assert_eq!(row["verification_status"], "planned_residual");
    }
}

#[test]
fn copyright_xp_and_signed_zero_match_residual_behavior() {
    let mut xp_title = "Hi"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    xp_title.extend_from_slice(&[0, 0]);
    let jpeg = jpeg_with_exif_entries(&[
        (0x8298, UNDEFINED, b"Photographer \0Editor\0".to_vec()),
        (0x9C9B, BYTE, xp_title),
    ]);
    let file = NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), jpeg).expect("writes JPEG fixture");
    let metadata = read_metadata(file.path()).expect("reads JPEG fixture");

    assert_eq!(
        metadata.get_string("ExifIFD:Copyright"),
        Some("Photographer\nEditor")
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-a", "-G1", "-s", "-Copyright"])
        .arg(file.path())
        .output()
        .expect("runs oxidex duplicate projection");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| line.contains("Copyright"))
            .count(),
        1,
        "Copyright must be inserted exactly once under -a"
    );
    assert_eq!(metadata.get_string("ExifIFD:XPTitle"), Some("Hi"));

    let negative_zero = [0i32.to_le_bytes(), (-1i32).to_le_bytes()].concat();
    let value = raw_bytes_to_tag_value(
        &negative_zero,
        SRATIONAL,
        1,
        0x9400,
        ByteOrder::LittleEndian,
    );
    assert_eq!(
        format_tag_value("ExifIFD:AmbientTemperature", &value).as_string(),
        Some("-0 C")
    );
}

#[test]
fn composite_image_exposure_times_mixed_layout_matches_exiftool() {
    use oxidex::core::formatters::composite_image_exposure_times::format_composite_image_exposure_times;
    let mut bytes = Vec::new();
    for _ in 0..7 {
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
    }
    bytes.extend_from_slice(&2u16.to_be_bytes());
    bytes.extend_from_slice(&3u16.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    assert_eq!(
        format_composite_image_exposure_times(&bytes, ByteOrder::BigEndian),
        "0 0 0 0 0 0 0 2 3 0.5"
    );
}
