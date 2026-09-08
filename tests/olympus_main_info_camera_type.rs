//! `Olympus::Main` 0x0201 `Quality`, 0x0207 `CameraType` and 0x0208 `TextInfo`
//! inside the 0x4000 `MainInfo` directory.
//!
//! The `OLYMPUS\0II` bodies from the FE/SP/u generations on write these
//! three entries ONLY in the second, later-walked `MainInfo` directory --
//! OlympusFE4010.jpg's top level holds 0x0200, 0x0209 and four sub-directory
//! pointers, and MainInfo (note offset 0x3c0) holds Quality, CameraType and
//! TextInfo. Until 2026-09-08 the hand pass scanned the top level alone, and
//! the corpus census at `d4d6528b` (conformance.py over
//! combined-samples/Olympus against the pinned 13.59 oracle) counted 51
//! files with no CameraType and no Quality and 44 with no `Resolution` (a
//! TextInfo row) for exactly that reason.
//!
//! Expected values are the pinned oracle's:
//!
//! ```text
//! $ exiftool-pinned.sh -j -G1 OlympusFE4010.jpg
//! "Olympus:CameraType2": "FE4010,X930",
//! "Olympus:Quality": "HQ (Normal)",
//! "Olympus:Resolution": 2,
//! "Olympus:CameraType": "FE4010,X930",
//! $ exiftool-pinned.sh -j -G1 Olympus_u9010.jpg
//! "Olympus:Quality": "HQ (Normal)", "Olympus:Resolution": 2,
//! "Olympus:CameraType": "u9010,S9010",
//! $ exiftool-pinned.sh -j -G1 OlympusFE370.jpg
//! "Olympus:Quality": "HQ (Normal)", "Olympus:Resolution": 2,
//! "Olympus:CameraType": "FE370,X880,C575",
//! $ exiftool-pinned.sh -j -G1 OlympusC160.jpg
//! "Olympus:Quality": "Medium-Fine", "Olympus:CameraType": "C160,D395",
//! ```
//!
//! OlympusC160.jpg's CameraType is deliberately NOT asserted: its `OLYMP\0`
//! note (TIFF-relative offsets) stores the value at TIFF offset 0x02e0,
//! before the note at 0x037e, which the window this parser receives cannot
//! reach (3 corpus files; a dispatcher question, tracked in the plan doc).

use oxidex::core::MetadataMap;
use oxidex::core::operations::read_metadata;
use std::path::Path;

const CORPUS: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Olympus";

fn shown(metadata: &MetadataMap, key: &str) -> Option<String> {
    let value = metadata.get(key)?;
    value
        .as_string()
        .map(str::to_string)
        .or_else(|| value.as_integer().map(|i| i.to_string()))
}

fn assert_tags(file: &str, expected: &[(&str, &str)]) {
    let path = Path::new(CORPUS).join(file);
    let metadata = read_metadata(&path).unwrap_or_else(|e| panic!("{file} parses: {e}"));
    for (key, want) in expected {
        assert_eq!(
            shown(&metadata, key).as_deref(),
            Some(*want),
            "{file}: {key}"
        );
    }
}

#[test]
#[ignore = "requires the combined-samples corpus"]
fn main_info_directory_supplies_camera_type_quality_and_text_info() {
    assert_tags(
        "OlympusFE4010.jpg",
        &[
            ("Olympus:CameraType", "FE4010,X930"),
            ("Olympus:CameraType2", "FE4010,X930"),
            ("Olympus:Quality", "HQ (Normal)"),
            ("Olympus:Resolution", "2"),
        ],
    );
    assert_tags(
        "Olympus_u9010.jpg",
        &[
            ("Olympus:CameraType", "u9010,S9010"),
            ("Olympus:Quality", "HQ (Normal)"),
            ("Olympus:Resolution", "2"),
        ],
    );
    assert_tags(
        "OlympusFE370.jpg",
        &[
            ("Olympus:CameraType", "FE370,X880,C575"),
            ("Olympus:Quality", "HQ (Normal)"),
            ("Olympus:Resolution", "2"),
        ],
    );
    // A top-level carrier keeps its values (the member and the last Quality
    // value seen are carried across both directories, not reset).
    assert_tags("OlympusC160.jpg", &[("Olympus:Quality", "Medium-Fine")]);
}
