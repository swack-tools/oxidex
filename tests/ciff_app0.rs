use oxidex::core::operations::read_metadata;
use std::path::Path;

const PRO70: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonPowerShotPro70.jpg";
const EXIFTOOL_JPEG: &str = "/tmp/oxidex-exiftool-cache/combined-samples/ExifTool.jpg";
const POWERSHOT_600: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonPowerShot600.jpg";

#[test]
fn canon_pro70_app0_ciff_matches_exiftool() {
    if !Path::new(PRO70).is_file() {
        eprintln!("skipping: corpus fixture not present at {PRO70}");
        return;
    }
    let metadata = read_metadata(Path::new(PRO70)).expect("Canon Pro70 JPEG parses");
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
fn exiftool_jpeg_app0_ciff_matches_exiftool() {
    if !Path::new(EXIFTOOL_JPEG).is_file() {
        eprintln!("skipping: corpus fixture not present at {EXIFTOOL_JPEG}");
        return;
    }
    let metadata = read_metadata(Path::new(EXIFTOOL_JPEG)).expect("ExifTool JPEG parses");
    assert_eq!(metadata.get_string("CIFF:FileFormat"), Some("JPEG (lossy)"));
    assert_eq!(
        metadata.get_string("CIFF:Model"),
        Some("Canon PowerShot A5")
    );
    assert_eq!(metadata.get_string("CIFF:FocalLength"), Some("5 mm"));
}

#[test]
fn powershot_600_app0_ciff_reports_component_version() {
    if !Path::new(POWERSHOT_600).is_file() {
        eprintln!("skipping: corpus fixture not present at {POWERSHOT_600}");
        return;
    }
    let metadata =
        read_metadata(Path::new(POWERSHOT_600)).expect("Canon PowerShot 600 JPEG parses");
    assert_eq!(
        metadata.get_string("CIFF:ComponentVersion"),
        Some("Component version 1.00")
    );
}
