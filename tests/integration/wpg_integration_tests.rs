use oxidex::core::TagValue;
use oxidex::core::operations::read_metadata;
use std::path::Path;

/// ExifTool 13.59 emits WPG version 1 record types in file order and collapses
/// only adjacent duplicate types. This fails if the WPG parser is not wired
/// into the production read path, if a record length is decoded incorrectly,
/// or if the collapse/print conversion differs from ExifTool's `Records` tag.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn wpg_fixture_reports_records() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/WPG.wpg",
    ))
    .expect("read pinned WPG fixture");

    assert_eq!(
        metadata
            .get("WPG:Records")
            .expect("OxiDex missing WPG:Records"),
        &TagValue::Array(vec![
            TagValue::String("Start WPG (Type 1)".to_string()),
            TagValue::String("Start WPG (Type 2)".to_string()),
            TagValue::String("Fill Attributes".to_string()),
            TagValue::String("Line Attributes".to_string()),
            TagValue::String("Polygon x 5".to_string()),
            TagValue::String("Fill Attributes".to_string()),
            TagValue::String("Line Attributes".to_string()),
            TagValue::String("End WPG".to_string()),
        ])
    );
}
