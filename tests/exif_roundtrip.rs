//! Round-trip regression suite for issue #20: read -> write must never
//! corrupt or silently drop EXIF data.

use oxidex::core::operations::{modify_tag, read_metadata, remove_tag, write_metadata};
use oxidex::core::tag_value::TagValue;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jpeg")
        .join(name)
}

fn temp_copy(src: &Path, label: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join(label);
    std::fs::copy(src, &dst).unwrap();
    (dir, dst)
}

/// Semantic parity: every key in `before` must exist in `after` with an
/// equal value (except keys in `except`). File:* pseudo-tags are ignored.
fn assert_parity(
    before: &oxidex::core::MetadataMap,
    after: &oxidex::core::MetadataMap,
    except: &[&str],
) {
    for (key, value) in before.iter() {
        if key.starts_with("File:") || except.contains(&key.as_str()) {
            continue;
        }
        assert_eq!(
            after.get(key),
            Some(value),
            "tag {} was dropped or changed by the rewrite",
            key
        );
    }
}

#[test]
fn noop_write_preserves_everything_gps_fixture() {
    let (_d, path) = temp_copy(&fixture("complex/synthetic_gps_001.jpg"), "noop_gps.jpg");
    let before = read_metadata(&path).unwrap();
    write_metadata(&path, &before).unwrap(); // was: hard validation failure
    let after = read_metadata(&path).unwrap();
    assert_parity(&before, &after, &[]);
    assert_parity(&after, &before, &[]); // and nothing appeared from nowhere
}

#[test]
fn noop_write_preserves_makernotes() {
    let (_d, path) = temp_copy(&fixture("makernotes/canon_sample.jpg"), "noop_canon.jpg");
    let before = read_metadata(&path).unwrap();
    let canon_before: Vec<(String, TagValue)> = before
        .iter()
        .filter(|(k, _)| k.starts_with("Canon:"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert!(
        !canon_before.is_empty(),
        "fixture must have Canon MakerNote tags"
    );
    write_metadata(&path, &before).unwrap();
    let after = read_metadata(&path).unwrap();
    for (key, value) in &canon_before {
        assert_eq!(after.get(key), Some(value), "MakerNote tag {} lost", key);
    }
}

#[test]
fn modify_tag_leaves_binary_canaries_byte_identical() {
    let src = fixture("complex/synthetic_gps_001.jpg");
    let (_d, path) = temp_copy(&src, "modify.jpg");
    modify_tag(&path, "IFD0:Artist", TagValue::new_string("Round Tripper")).unwrap();

    let after = read_metadata(&path).unwrap();
    assert_eq!(
        after.get("IFD0:Artist").and_then(|v| v.as_string()),
        Some("Round Tripper")
    );
    // The canaries that used to be corrupted/rejected
    let before = read_metadata(&src).unwrap();
    for canary in ["ExifIFD:ComponentsConfiguration", "GPS:GPSVersionID"] {
        assert_eq!(before.get(canary), after.get(canary), "{} damaged", canary);
    }
}

#[test]
fn remove_tag_removes_only_that_tag() {
    let (_d, path) = temp_copy(&fixture("complex/synthetic_gps_001.jpg"), "remove.jpg");
    let before = read_metadata(&path).unwrap();
    assert!(before.get("ExifIFD:DateTimeOriginal").is_some());
    remove_tag(&path, "ExifIFD:DateTimeOriginal").unwrap();
    let after = read_metadata(&path).unwrap();
    assert!(after.get("ExifIFD:DateTimeOriginal").is_none());
    assert_parity(&before, &after, &["ExifIFD:DateTimeOriginal"]);
}

#[test]
fn changed_binary_display_string_still_rejected() {
    let (_d, path) = temp_copy(&fixture("complex/synthetic_gps_001.jpg"), "reject.jpg");
    let mut map = read_metadata(&path).unwrap();
    map.insert(
        "ExifIFD:ComponentsConfiguration",
        TagValue::new_string("R, G, B, -"),
    );
    let err = write_metadata(&path, &map).unwrap_err();
    assert!(err.to_string().contains("Type mismatch"), "got: {}", err);
    // And the file was not touched
    let orig = std::fs::read(fixture("complex/synthetic_gps_001.jpg")).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), orig);
}
