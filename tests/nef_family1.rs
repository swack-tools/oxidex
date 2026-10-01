//! ExifTool 13.59 `t/images/Nikon.nef`: the TIFF directories are distinct
//! occurrences even when family 0 and the default values already agree.

#[path = "common/fixtures.rs"]
mod fixtures;

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn selected(file: &Path, mode: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-j")
        .args(mode)
        .args([
            "-ImageWidth",
            "-ImageHeight",
            "-SubfileType",
            "-TIFF-EPStandardID",
        ])
        .arg(file)
        .output()
        .expect("run oxidex");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut row = serde_json::from_slice::<Value>(&output.stdout).expect("JSON output");
    let row = row.as_array_mut().unwrap().remove(0);
    let mut tags = row.as_object().unwrap().clone();
    tags.remove("SourceFile");
    Value::Object(tags)
}

#[test]
fn nikon_nef_group_and_numeric_views_keep_source_directories() {
    let Some(file) = fixtures::pinned_t_images_fixture_path("Nikon.nef") else {
        return;
    };
    assert_eq!(
        selected(&file, &["-G0"]),
        json!({
            "EXIF:ImageWidth": 3040,
            "EXIF:ImageHeight": 2014,
            "EXIF:SubfileType": "Full-resolution image",
            "EXIF:TIFF-EPStandardID": "1.0.0.0",
        })
    );
    assert_eq!(
        selected(&file, &["-G1"]),
        json!({
            "IFD0:ImageWidth": 160,
            "SubIFD1:ImageWidth": 3040,
            "IFD0:ImageHeight": 106,
            "SubIFD1:ImageHeight": 2014,
            "IFD0:SubfileType": "Reduced-resolution image",
            "SubIFD:SubfileType": "Reduced-resolution image",
            "SubIFD1:SubfileType": "Full-resolution image",
            "IFD0:TIFF-EPStandardID": "1.0.0.0",
        })
    );
    assert_eq!(
        // OxiDex reserves -n for dry-run; this is its ExifTool -n mode.
        selected(&file, &["-G0:1", "-a", "--no-print-conv"]),
        json!({
            "EXIF:IFD0:ImageWidth": 160,
            "EXIF:SubIFD1:ImageWidth": 3040,
            "EXIF:IFD0:ImageHeight": 106,
            "EXIF:SubIFD1:ImageHeight": 2014,
            "EXIF:IFD0:SubfileType": 1,
            "EXIF:SubIFD:SubfileType": 1,
            "EXIF:SubIFD1:SubfileType": 0,
            "EXIF:IFD0:TIFF-EPStandardID": "1 0 0 0",
        })
    );
}
