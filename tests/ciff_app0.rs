use oxidex::core::operations::read_metadata;
#[path = "common/fixtures.rs"]
mod fixtures;

const PRO70: &str = "Canon/CanonPowerShotPro70.jpg";
const EXIFTOOL_JPEG: &str = "ExifTool.jpg";
const POWERSHOT_600: &str = "Canon/CanonPowerShot600.jpg";

#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn canon_pro70_app0_ciff_matches_exiftool() {
    let path = fixtures::required_combined_fixture_path(PRO70);
    let metadata = read_metadata(&path).expect("Canon Pro70 JPEG parses");
    assert_eq!(metadata.get_string("CIFF:FileFormat"), Some("JPEG (lossy)"));
    assert_eq!(metadata.get_integer("CIFF:ImageWidth"), Some(768));
    assert_eq!(metadata.get_integer("CIFF:ImageHeight"), Some(512));
    assert_eq!(
        metadata.get_string("CIFF:DateTimeOriginal"),
        Some("1998:10:23 10:56:08")
    );
    assert_eq!(metadata.get_string("CIFF:Make"), Some("Canon"));
    assert_eq!(
        metadata.get_string("CIFF:Model"),
        Some("Canon PowerShot Pro70")
    );
    assert_eq!(metadata.get_integer("CIFF:BaseISO"), Some(100));
    assert_eq!(metadata.get_string("CIFF:FocalType"), Some("Zoom"));
    assert_eq!(metadata.get_string("CIFF:FocalLength"), Some("419 mm"));
}

#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn exiftool_jpeg_app0_ciff_matches_exiftool() {
    let path = fixtures::required_combined_fixture_path(EXIFTOOL_JPEG);
    let metadata = read_metadata(&path).expect("ExifTool JPEG parses");
    assert_eq!(metadata.get_string("CIFF:FileFormat"), Some("JPEG (lossy)"));
    assert_eq!(
        metadata.get_string("CIFF:Model"),
        Some("Canon PowerShot A5")
    );
    assert_eq!(metadata.get_string("CIFF:FocalLength"), Some("5 mm"));
}

#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn powershot_600_app0_ciff_reports_component_version() {
    let path = fixtures::required_combined_fixture_path(POWERSHOT_600);
    let metadata = read_metadata(&path).expect("Canon PowerShot 600 JPEG parses");
    assert_eq!(
        metadata.get_string("CIFF:ComponentVersion"),
        Some("Component version 1.00")
    );
}
