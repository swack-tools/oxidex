use oxidex::core::operations::read_metadata;
use std::path::Path;

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
