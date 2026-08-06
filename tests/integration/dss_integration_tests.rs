use oxidex::core::operations::read_metadata;
use std::io::Write;
use std::path::Path;
use tempfile::Builder;

/// ExifTool 13.59 reads the twelve ASCII digits at offset 50 in Olympus DSS
/// files and applies its declared date conversion before printing the tag.
/// This fails if DSS is not routed to that layout or if the conversion format
/// differs from ExifTool.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn dss_fixture_reports_end_time() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/Olympus.dss",
    ))
    .expect("read pinned DSS fixture");

    assert_eq!(
        metadata
            .get_string("Olympus:EndTime")
            .expect("OxiDex missing Olympus:EndTime"),
        "2005:11:16 13:52:53"
    );
}

/// ExifTool routes the DS2 signature through the same Olympus DSS binary
/// table. This exercises detection and parser validation as well as the shared
/// EndTime conversion without enabling any adjacent table fields.
#[test]
fn ds2_signature_reports_end_time() {
    let mut bytes = vec![0_u8; 69];
    bytes[..4].copy_from_slice(b"\x03ds2");
    bytes[50..62].copy_from_slice(b"260806123456");

    let mut file = Builder::new()
        .suffix(".ds2")
        .tempfile()
        .expect("create synthetic DS2 file");
    file.write_all(&bytes).expect("write synthetic DS2 file");

    let metadata = read_metadata(file.path()).expect("read synthetic DS2 file");
    assert_eq!(
        metadata.get_string("Olympus:EndTime"),
        Some("2026:08:06 12:34:56")
    );
}
