//! Regression tests for read -> write -> read metadata round-trips.
//!
//! Before the validation fix, reading a real EXIF-bearing file and writing its own
//! metadata back failed PHASE 1 validation: parsers surface "undefined"/Binary,
//! DateTime, and Rational EXIF tags as formatted strings, which the strict
//! exact-type-match validator rejected. These tests pin the coercion behavior so the
//! fundamental round-trip use case keeps working.

use oxidex::core::operations::{read_metadata, write_metadata};
use oxidex::core::tag_value::TagValue;
use std::fs;
use std::path::Path;

fn copy_to_temp(fixture: &str, suffix: &str) -> tempfile::NamedTempFile {
    let temp = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create temp fixture copy");
    fs::copy(fixture, temp.path()).expect("copy fixture");
    temp
}

/// Reading a real file's metadata and writing it straight back must succeed end to end.
fn assert_self_roundtrip(fixture: &str, suffix: &str) {
    let temp = copy_to_temp(fixture, suffix);
    let metadata = read_metadata(temp.path()).unwrap_or_else(|e| panic!("read {fixture}: {e}"));
    assert!(
        !metadata.is_empty(),
        "expected {fixture} to yield some metadata"
    );
    // The regression: this write previously failed PHASE 1 validation on String-typed
    // Binary/DateTime/Rational tags produced by the reader.
    write_metadata(temp.path(), &metadata)
        .unwrap_or_else(|e| panic!("self round-trip write {fixture}: {e}"));
    // And the file must still be readable afterwards.
    read_metadata(temp.path()).unwrap_or_else(|e| panic!("re-read {fixture} after write: {e}"));
}

#[test]
fn png_self_roundtrip_succeeds() {
    assert_self_roundtrip("tests/fixtures/png/sample.png", ".png");
}

#[test]
fn jpeg_self_roundtrip_succeeds() {
    assert_self_roundtrip("tests/fixtures/jpeg/sample_with_exif.jpg", ".jpg");
}

#[test]
fn tiff_self_roundtrip_succeeds() {
    assert_self_roundtrip("tests/fixtures/tiff/sample.tif", ".tif");
}

/// A tag set on read metadata survives a write and is read back unchanged.
/// EXIF tags live under the `IFD0:` namespace for the JPEG/TIFF writer.
#[test]
fn jpeg_set_tag_survives_roundtrip() {
    let temp = copy_to_temp("tests/fixtures/jpeg/sample_with_exif.jpg", ".jpg");
    let mut metadata = read_metadata(temp.path()).expect("read jpeg");
    metadata.insert("IFD0:Artist", TagValue::new_string("OxiDex RoundTrip"));
    write_metadata(temp.path(), &metadata).expect("write jpeg with new Artist");

    let reread = read_metadata(temp.path()).expect("re-read jpeg");
    assert_eq!(
        reread.get_string("IFD0:Artist"),
        Some("OxiDex RoundTrip"),
        "IFD0:Artist should persist through the round-trip"
    );
}

/// write_metadata still rejects genuinely-wrong types (validation strength preserved):
/// a Struct value for a String-typed tag has no scalar representation.
#[test]
fn write_rejects_truly_invalid_type() {
    use std::collections::HashMap;
    let temp = copy_to_temp("tests/fixtures/jpeg/sample_with_exif.jpg", ".jpg");
    let mut metadata = read_metadata(temp.path()).expect("read jpeg");
    let mut s = HashMap::new();
    s.insert("k".to_string(), TagValue::new_string("v"));
    // EXIF:Make is a String descriptor; a Struct value must still be rejected.
    metadata.insert("EXIF:Make", TagValue::new_struct(s));
    assert!(
        write_metadata(temp.path(), &metadata).is_err(),
        "a Struct value for a String tag must still fail validation"
    );
    let _ = Path::new(".");
}
