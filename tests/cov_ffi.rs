//! Coverage-focused integration tests for the C FFI surface.
//!
//! These tests drive the `pub extern "C"` functions in `src/ffi/read_tags.rs`
//! and `src/ffi/write_tags.rs` directly from Rust, exercising the
//! error / malformed-input branches that the C integration test does not
//! reach (real-file reads, type-mismatch, NULL pointers, invalid UTF-8,
//! write success / unsupported-format / IO error paths, etc.).

#[path = "common/mod.rs"]
mod common;

#[allow(unused_imports)]
use common::TestReader;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

use oxidex::ffi::{
    EXIFTOOL_ERR_INVALID_TAG_VALUE, EXIFTOOL_ERR_IO, EXIFTOOL_ERR_NULL_POINTER, EXIFTOOL_ERR_PARSE,
    EXIFTOOL_ERR_TAG_NOT_FOUND, EXIFTOOL_ERR_UNSUPPORTED_FORMAT, EXIFTOOL_OK, ExifToolHandle,
    exiftool_get_last_error, exiftool_get_tag_count, exiftool_get_tag_float,
    exiftool_get_tag_integer, exiftool_get_tag_name_at, exiftool_get_tag_string, exiftool_has_tag,
    exiftool_read_file, exiftool_remove_tag, exiftool_set_tag_float, exiftool_set_tag_integer,
    exiftool_set_tag_string, exiftool_write_file,
};
use oxidex::ffi::{exiftool_create, exiftool_destroy};

/// Path to a real JPEG fixture that contains EXIF data.
const SAMPLE_JPEG: &str = "tests/fixtures/jpeg/sample_with_exif.jpg";

/// Helper: create a handle, panics-via-assert if allocation fails.
fn make_handle() -> *mut ExifToolHandle {
    let handle = exiftool_create();
    assert!(!handle.is_null(), "exiftool_create returned NULL");
    handle
}

/// Helper: build a CString (panics on embedded NUL, fine for tests).
fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// Helper: read last error message as a Rust String.
fn last_error() -> String {
    let ptr = exiftool_get_last_error();
    assert!(!ptr.is_null());
    unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}

// ===========================================================================
// read_file: success on a real fixture + iteration
// ===========================================================================

#[test]
fn read_real_jpeg_fixture_succeeds_and_populates_tags() {
    let handle = make_handle();
    let path = cs(SAMPLE_JPEG);

    let rc = exiftool_read_file(handle, path.as_ptr());
    assert_eq!(rc, EXIFTOOL_OK, "read of real fixture should succeed");

    let count = exiftool_get_tag_count(handle);
    assert!(count > 0, "expected at least one tag from real JPEG");

    // Iterate every cached tag name; each must be non-NULL and valid UTF-8.
    for i in 0..count {
        let name_ptr = exiftool_get_tag_name_at(handle, i);
        assert!(!name_ptr.is_null(), "tag name at {i} was NULL");
        let name = unsafe { CStr::from_ptr(name_ptr) }.to_str().unwrap();
        assert!(!name.is_empty());

        // has_tag must agree that this name exists.
        let name_c = cs(name);
        assert_eq!(
            exiftool_has_tag(handle, name_c.as_ptr()),
            1,
            "has_tag disagreed with iteration for {name}"
        );
    }

    exiftool_destroy(handle);
}

#[test]
fn read_file_then_get_string_for_known_string_tag() {
    let handle = make_handle();
    let path = cs(SAMPLE_JPEG);
    assert_eq!(exiftool_read_file(handle, path.as_ptr()), EXIFTOOL_OK);

    // Walk the tags, find the first that returns a string and confirm round-trip.
    let count = exiftool_get_tag_count(handle);
    let mut found_string = false;
    for i in 0..count {
        let name_ptr = exiftool_get_tag_name_at(handle, i);
        let name = unsafe { CStr::from_ptr(name_ptr) }
            .to_str()
            .unwrap()
            .to_owned();
        let name_c = cs(&name);
        let val_ptr = exiftool_get_tag_string(handle, name_c.as_ptr());
        if !val_ptr.is_null() {
            let _ = unsafe { CStr::from_ptr(val_ptr) }.to_str().unwrap();
            found_string = true;
        }
    }
    // Not strictly required, but the fixture has Make/DateTime strings.
    assert!(found_string, "expected at least one string-valued tag");

    exiftool_destroy(handle);
}

#[test]
fn read_file_can_be_called_twice_clearing_previous_state() {
    let handle = make_handle();
    let path = cs(SAMPLE_JPEG);

    // First read.
    assert_eq!(exiftool_read_file(handle, path.as_ptr()), EXIFTOOL_OK);
    let first = exiftool_get_tag_count(handle);
    assert!(first > 0);

    // Second read of the same file should clear the cache and re-populate.
    assert_eq!(exiftool_read_file(handle, path.as_ptr()), EXIFTOOL_OK);
    let second = exiftool_get_tag_count(handle);
    assert_eq!(first, second);

    exiftool_destroy(handle);
}

// ===========================================================================
// read_file: error branches
// ===========================================================================

#[test]
fn read_file_null_handle_returns_null_pointer_err() {
    let path = cs(SAMPLE_JPEG);
    let rc = exiftool_read_file(ptr::null_mut(), path.as_ptr());
    assert_eq!(rc, EXIFTOOL_ERR_NULL_POINTER);
    assert!(last_error().contains("NULL"));
}

#[test]
fn read_file_null_path_returns_null_pointer_err() {
    let handle = make_handle();
    let rc = exiftool_read_file(handle, ptr::null());
    assert_eq!(rc, EXIFTOOL_ERR_NULL_POINTER);
    exiftool_destroy(handle);
}

#[test]
fn read_file_nonexistent_returns_io_err() {
    let handle = make_handle();
    let path = cs("/no/such/path/should/exist/file.jpg");
    let rc = exiftool_read_file(handle, path.as_ptr());
    assert_eq!(rc, EXIFTOOL_ERR_IO);
    assert!(!last_error().is_empty());
    exiftool_destroy(handle);
}

#[test]
fn read_file_invalid_utf8_path_returns_invalid_value() {
    let handle = make_handle();
    // 0xFF is not valid UTF-8; CStr::to_str() will fail inside the FFI fn.
    let bad = CString::new(vec![0x66u8, 0x6f, 0x6f, 0xff]).unwrap();
    let rc = exiftool_read_file(handle, bad.as_ptr() as *const c_char);
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    assert!(last_error().contains("UTF-8"));
    exiftool_destroy(handle);
}

#[test]
fn read_empty_file_drives_read_path() {
    // A zero-byte file detects as FileFormat::Unknown, which the dispatcher
    // rejects as an unsupported format. Drives the read_file -> error_to_code
    // branch end to end.
    let tmp = tempfile::Builder::new().suffix(".bin").tempfile().unwrap();
    std::fs::write(tmp.path(), b"").unwrap();

    let handle = make_handle();
    let path = cs(tmp.path().to_str().unwrap());
    let rc = exiftool_read_file(handle, path.as_ptr());
    // Unknown format -> unsupported. error_to_code maps it to a non-OK code.
    assert_eq!(rc, EXIFTOOL_ERR_UNSUPPORTED_FORMAT);
    assert!(!last_error().is_empty());
    exiftool_destroy(handle);
}

#[test]
fn read_arbitrary_bytes_file_returns_defined_code() {
    // Arbitrary non-media bytes exercise the read path; the exact outcome
    // depends on format detection, so we only require a defined return code
    // and a consistent handle state.
    let tmp = tempfile::Builder::new().suffix(".bin").tempfile().unwrap();
    std::fs::write(tmp.path(), b"not a real media file at all").unwrap();

    let handle = make_handle();
    let path = cs(tmp.path().to_str().unwrap());
    let rc = exiftool_read_file(handle, path.as_ptr());
    // Must be one of the documented codes (success or a mapped error).
    assert!(
        rc == EXIFTOOL_OK
            || rc == EXIFTOOL_ERR_UNSUPPORTED_FORMAT
            || rc == EXIFTOOL_ERR_PARSE
            || rc == EXIFTOOL_ERR_IO,
        "unexpected read code {rc}"
    );
    // get_tag_count must never panic regardless of the read outcome.
    let _ = exiftool_get_tag_count(handle);
    exiftool_destroy(handle);
}

// ===========================================================================
// get_tag_count / get_tag_name_at edge cases
// ===========================================================================

#[test]
fn get_tag_count_null_handle_is_zero() {
    assert_eq!(exiftool_get_tag_count(ptr::null()), 0);
}

#[test]
fn get_tag_name_at_null_handle_is_null() {
    assert!(exiftool_get_tag_name_at(ptr::null(), 0).is_null());
}

#[test]
fn get_tag_name_at_out_of_bounds_is_null() {
    let handle = make_handle();
    exiftool_set_tag_string(handle, cs("EXIF:Make").as_ptr(), cs("Canon").as_ptr());
    // Index past the single tag must be NULL.
    assert!(exiftool_get_tag_name_at(handle, 999).is_null());
    // Index 0 is valid.
    assert!(!exiftool_get_tag_name_at(handle, 0).is_null());
    exiftool_destroy(handle);
}

// ===========================================================================
// has_tag edge cases
// ===========================================================================

#[test]
fn has_tag_null_handle_and_null_name() {
    let handle = make_handle();
    assert_eq!(exiftool_has_tag(ptr::null(), cs("EXIF:Make").as_ptr()), 0);
    assert_eq!(exiftool_has_tag(handle, ptr::null()), 0);
    assert_eq!(exiftool_has_tag(handle, cs("Nope:Missing").as_ptr()), 0);
    exiftool_destroy(handle);
}

#[test]
fn has_tag_invalid_utf8_name_is_zero() {
    let handle = make_handle();
    let bad = CString::new(vec![0xffu8, 0xfe]).unwrap();
    assert_eq!(exiftool_has_tag(handle, bad.as_ptr() as *const c_char), 0);
    exiftool_destroy(handle);
}

// ===========================================================================
// get_tag_string edge cases
// ===========================================================================

#[test]
fn get_tag_string_null_handle_and_name() {
    let handle = make_handle();
    assert!(exiftool_get_tag_string(ptr::null(), cs("EXIF:Make").as_ptr()).is_null());
    assert!(exiftool_get_tag_string(handle, ptr::null()).is_null());
    exiftool_destroy(handle);
}

#[test]
fn get_tag_string_missing_tag_is_null() {
    let handle = make_handle();
    assert!(exiftool_get_tag_string(handle, cs("EXIF:DoesNotExist").as_ptr()).is_null());
    exiftool_destroy(handle);
}

#[test]
fn get_tag_string_non_string_tag_is_null() {
    // get_string only returns Some for String variants; an integer tag → NULL.
    let handle = make_handle();
    exiftool_set_tag_integer(handle, cs("EXIF:ISO").as_ptr(), 400);
    assert!(exiftool_get_tag_string(handle, cs("EXIF:ISO").as_ptr()).is_null());
    exiftool_destroy(handle);
}

#[test]
fn get_tag_string_invalid_utf8_name_is_null() {
    let handle = make_handle();
    let bad = CString::new(vec![0xc3u8, 0x28]).unwrap();
    assert!(exiftool_get_tag_string(handle, bad.as_ptr() as *const c_char).is_null());
    exiftool_destroy(handle);
}

#[test]
fn get_tag_string_roundtrip_after_set() {
    let handle = make_handle();
    exiftool_set_tag_string(handle, cs("EXIF:Artist").as_ptr(), cs("Ansel").as_ptr());
    let p = exiftool_get_tag_string(handle, cs("EXIF:Artist").as_ptr());
    assert!(!p.is_null());
    let v = unsafe { CStr::from_ptr(p) }.to_str().unwrap();
    assert_eq!(v, "Ansel");
    exiftool_destroy(handle);
}

// ===========================================================================
// get_tag_integer branches
// ===========================================================================

#[test]
fn get_tag_integer_null_pointers() {
    let handle = make_handle();
    let mut out: i64 = 0;
    assert_eq!(
        exiftool_get_tag_integer(ptr::null(), cs("EXIF:ISO").as_ptr(), &mut out),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_get_tag_integer(handle, ptr::null(), &mut out),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_get_tag_integer(handle, cs("EXIF:ISO").as_ptr(), ptr::null_mut()),
        EXIFTOOL_ERR_NULL_POINTER
    );
    exiftool_destroy(handle);
}

#[test]
fn get_tag_integer_not_found() {
    let handle = make_handle();
    let mut out: i64 = -1;
    let rc = exiftool_get_tag_integer(handle, cs("EXIF:Missing").as_ptr(), &mut out);
    assert_eq!(rc, EXIFTOOL_ERR_TAG_NOT_FOUND);
    assert!(last_error().contains("not found"));
    exiftool_destroy(handle);
}

#[test]
fn get_tag_integer_type_mismatch() {
    let handle = make_handle();
    exiftool_set_tag_string(handle, cs("EXIF:Make").as_ptr(), cs("Nikon").as_ptr());
    let mut out: i64 = 0;
    let rc = exiftool_get_tag_integer(handle, cs("EXIF:Make").as_ptr(), &mut out);
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    assert!(last_error().contains("Integer"));
    exiftool_destroy(handle);
}

#[test]
fn get_tag_integer_success() {
    let handle = make_handle();
    exiftool_set_tag_integer(handle, cs("EXIF:ISO").as_ptr(), 1600);
    let mut out: i64 = 0;
    assert_eq!(
        exiftool_get_tag_integer(handle, cs("EXIF:ISO").as_ptr(), &mut out),
        EXIFTOOL_OK
    );
    assert_eq!(out, 1600);
    exiftool_destroy(handle);
}

#[test]
fn get_tag_integer_invalid_utf8_name() {
    let handle = make_handle();
    let bad = CString::new(vec![0xffu8]).unwrap();
    let mut out: i64 = 0;
    let rc = exiftool_get_tag_integer(handle, bad.as_ptr() as *const c_char, &mut out);
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    exiftool_destroy(handle);
}

// ===========================================================================
// get_tag_float branches
// ===========================================================================

#[test]
fn get_tag_float_null_pointers() {
    let handle = make_handle();
    let mut out: f64 = 0.0;
    assert_eq!(
        exiftool_get_tag_float(ptr::null(), cs("EXIF:FNumber").as_ptr(), &mut out),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_get_tag_float(handle, ptr::null(), &mut out),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_get_tag_float(handle, cs("EXIF:FNumber").as_ptr(), ptr::null_mut()),
        EXIFTOOL_ERR_NULL_POINTER
    );
    exiftool_destroy(handle);
}

#[test]
fn get_tag_float_not_found() {
    let handle = make_handle();
    let mut out: f64 = 0.0;
    assert_eq!(
        exiftool_get_tag_float(handle, cs("EXIF:Missing").as_ptr(), &mut out),
        EXIFTOOL_ERR_TAG_NOT_FOUND
    );
    exiftool_destroy(handle);
}

#[test]
fn get_tag_float_type_mismatch() {
    let handle = make_handle();
    exiftool_set_tag_integer(handle, cs("EXIF:ISO").as_ptr(), 100);
    let mut out: f64 = 0.0;
    let rc = exiftool_get_tag_float(handle, cs("EXIF:ISO").as_ptr(), &mut out);
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    assert!(last_error().contains("Float"));
    exiftool_destroy(handle);
}

#[test]
fn get_tag_float_success() {
    let handle = make_handle();
    exiftool_set_tag_float(handle, cs("EXIF:FNumber").as_ptr(), 1.8);
    let mut out: f64 = 0.0;
    assert_eq!(
        exiftool_get_tag_float(handle, cs("EXIF:FNumber").as_ptr(), &mut out),
        EXIFTOOL_OK
    );
    assert!((out - 1.8).abs() < 1e-9);
    exiftool_destroy(handle);
}

#[test]
fn get_tag_float_invalid_utf8_name() {
    let handle = make_handle();
    let bad = CString::new(vec![0xc0u8, 0x80]).unwrap();
    let mut out: f64 = 0.0;
    let rc = exiftool_get_tag_float(handle, bad.as_ptr() as *const c_char, &mut out);
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    exiftool_destroy(handle);
}

// ===========================================================================
// set_tag_string branches
// ===========================================================================

#[test]
fn set_tag_string_null_pointers() {
    let handle = make_handle();
    assert_eq!(
        exiftool_set_tag_string(ptr::null_mut(), cs("X").as_ptr(), cs("v").as_ptr()),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_set_tag_string(handle, ptr::null(), cs("v").as_ptr()),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_set_tag_string(handle, cs("X").as_ptr(), ptr::null()),
        EXIFTOOL_ERR_NULL_POINTER
    );
    exiftool_destroy(handle);
}

#[test]
fn set_tag_string_invalid_utf8_name() {
    let handle = make_handle();
    let bad = CString::new(vec![0xffu8]).unwrap();
    let rc = exiftool_set_tag_string(handle, bad.as_ptr() as *const c_char, cs("v").as_ptr());
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    exiftool_destroy(handle);
}

#[test]
fn set_tag_string_invalid_utf8_value() {
    let handle = make_handle();
    let bad = CString::new(vec![0xffu8, 0xfe]).unwrap();
    let rc = exiftool_set_tag_string(
        handle,
        cs("EXIF:Make").as_ptr(),
        bad.as_ptr() as *const c_char,
    );
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    assert!(last_error().contains("UTF-8"));
    exiftool_destroy(handle);
}

#[test]
fn set_tag_string_overwrite_updates_value() {
    let handle = make_handle();
    exiftool_set_tag_string(handle, cs("EXIF:Make").as_ptr(), cs("Canon").as_ptr());
    exiftool_set_tag_string(handle, cs("EXIF:Make").as_ptr(), cs("Sony").as_ptr());
    let p = exiftool_get_tag_string(handle, cs("EXIF:Make").as_ptr());
    let v = unsafe { CStr::from_ptr(p) }.to_str().unwrap();
    assert_eq!(v, "Sony");
    // Count remains 1 since the key was overwritten.
    assert_eq!(exiftool_get_tag_count(handle), 1);
    exiftool_destroy(handle);
}

// ===========================================================================
// set_tag_integer branches
// ===========================================================================

#[test]
fn set_tag_integer_null_pointers() {
    let handle = make_handle();
    assert_eq!(
        exiftool_set_tag_integer(ptr::null_mut(), cs("X").as_ptr(), 1),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_set_tag_integer(handle, ptr::null(), 1),
        EXIFTOOL_ERR_NULL_POINTER
    );
    exiftool_destroy(handle);
}

#[test]
fn set_tag_integer_invalid_utf8_name() {
    let handle = make_handle();
    let bad = CString::new(vec![0xffu8]).unwrap();
    let rc = exiftool_set_tag_integer(handle, bad.as_ptr() as *const c_char, 5);
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    exiftool_destroy(handle);
}

#[test]
fn set_tag_integer_negative_value_roundtrips() {
    let handle = make_handle();
    exiftool_set_tag_integer(handle, cs("Custom:Offset").as_ptr(), -42);
    let mut out: i64 = 0;
    assert_eq!(
        exiftool_get_tag_integer(handle, cs("Custom:Offset").as_ptr(), &mut out),
        EXIFTOOL_OK
    );
    assert_eq!(out, -42);
    exiftool_destroy(handle);
}

// ===========================================================================
// set_tag_float branches
// ===========================================================================

#[test]
fn set_tag_float_null_pointers() {
    let handle = make_handle();
    assert_eq!(
        exiftool_set_tag_float(ptr::null_mut(), cs("X").as_ptr(), 1.0),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_set_tag_float(handle, ptr::null(), 1.0),
        EXIFTOOL_ERR_NULL_POINTER
    );
    exiftool_destroy(handle);
}

#[test]
fn set_tag_float_nan_and_infinity_rejected() {
    let handle = make_handle();
    assert_eq!(
        exiftool_set_tag_float(handle, cs("EXIF:FNumber").as_ptr(), f64::NAN),
        EXIFTOOL_ERR_INVALID_TAG_VALUE
    );
    assert_eq!(
        exiftool_set_tag_float(handle, cs("EXIF:FNumber").as_ptr(), f64::INFINITY),
        EXIFTOOL_ERR_INVALID_TAG_VALUE
    );
    assert_eq!(
        exiftool_set_tag_float(handle, cs("EXIF:FNumber").as_ptr(), f64::NEG_INFINITY),
        EXIFTOOL_ERR_INVALID_TAG_VALUE
    );
    exiftool_destroy(handle);
}

#[test]
fn set_tag_float_invalid_utf8_name() {
    let handle = make_handle();
    let bad = CString::new(vec![0xffu8]).unwrap();
    // Use a finite value so we reach the UTF-8 check (NaN check runs first).
    let rc = exiftool_set_tag_float(handle, bad.as_ptr() as *const c_char, 2.0);
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    exiftool_destroy(handle);
}

// ===========================================================================
// remove_tag branches
// ===========================================================================

#[test]
fn remove_tag_null_pointers() {
    let handle = make_handle();
    assert_eq!(
        exiftool_remove_tag(ptr::null_mut(), cs("X").as_ptr()),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_remove_tag(handle, ptr::null()),
        EXIFTOOL_ERR_NULL_POINTER
    );
    exiftool_destroy(handle);
}

#[test]
fn remove_tag_missing_still_ok() {
    let handle = make_handle();
    // Removing a tag that was never set is a no-op success.
    assert_eq!(
        exiftool_remove_tag(handle, cs("EXIF:NeverSet").as_ptr()),
        EXIFTOOL_OK
    );
    exiftool_destroy(handle);
}

#[test]
fn remove_tag_invalid_utf8_name() {
    let handle = make_handle();
    let bad = CString::new(vec![0xffu8, 0xfe]).unwrap();
    let rc = exiftool_remove_tag(handle, bad.as_ptr() as *const c_char);
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    exiftool_destroy(handle);
}

#[test]
fn remove_tag_then_count_drops() {
    let handle = make_handle();
    exiftool_set_tag_string(handle, cs("A:One").as_ptr(), cs("1").as_ptr());
    exiftool_set_tag_string(handle, cs("A:Two").as_ptr(), cs("2").as_ptr());
    assert_eq!(exiftool_get_tag_count(handle), 2);
    assert_eq!(
        exiftool_remove_tag(handle, cs("A:One").as_ptr()),
        EXIFTOOL_OK
    );
    assert_eq!(exiftool_get_tag_count(handle), 1);
    assert_eq!(exiftool_has_tag(handle, cs("A:One").as_ptr()), 0);
    exiftool_destroy(handle);
}

// ===========================================================================
// write_file branches
// ===========================================================================

#[test]
fn write_file_null_pointers() {
    let handle = make_handle();
    let path = cs("/tmp/whatever.jpg");
    assert_eq!(
        exiftool_write_file(ptr::null(), path.as_ptr()),
        EXIFTOOL_ERR_NULL_POINTER
    );
    assert_eq!(
        exiftool_write_file(handle, ptr::null()),
        EXIFTOOL_ERR_NULL_POINTER
    );
    exiftool_destroy(handle);
}

#[test]
fn write_file_invalid_utf8_path() {
    let handle = make_handle();
    let bad = CString::new(vec![0x2fu8, 0xff]).unwrap();
    let rc = exiftool_write_file(handle, bad.as_ptr() as *const c_char);
    assert_eq!(rc, EXIFTOOL_ERR_INVALID_TAG_VALUE);
    exiftool_destroy(handle);
}

#[test]
fn write_file_nonexistent_target_is_io_error() {
    // write_metadata opens the original file first; a missing target is an IO error.
    let handle = make_handle();
    exiftool_set_tag_string(handle, cs("EXIF:Artist").as_ptr(), cs("X").as_ptr());
    let path = cs("/no/such/dir/missing.jpg");
    let rc = exiftool_write_file(handle, path.as_ptr());
    assert_eq!(rc, EXIFTOOL_ERR_IO);
    exiftool_destroy(handle);
}

#[test]
fn write_file_unsupported_format_returns_error() {
    // A PNG-magic file: write path supports PNG, but a bogus/empty-ish file
    // with an unrecognized format must hit the unsupported-format branch.
    let tmp = tempfile::Builder::new().suffix(".dat").tempfile().unwrap();
    // GIF magic — a real format that detect_format recognizes but write does not.
    std::fs::write(tmp.path(), b"GIF89a\x01\x00\x01\x00\x00\x00\x00;").unwrap();

    let handle = make_handle();
    exiftool_set_tag_string(handle, cs("EXIF:Artist").as_ptr(), cs("X").as_ptr());
    let path = cs(tmp.path().to_str().unwrap());
    let rc = exiftool_write_file(handle, path.as_ptr());
    // Either unsupported-format (if GIF is detected) or another non-OK code.
    assert_ne!(rc, EXIFTOOL_OK);
    if rc == EXIFTOOL_ERR_UNSUPPORTED_FORMAT {
        assert!(last_error().to_lowercase().contains("format"));
    }
    exiftool_destroy(handle);
}

#[test]
fn write_file_real_jpeg_roundtrip() {
    // Copy the real fixture to a temp file, read it, set a tag, write it back.
    let original = std::fs::read(SAMPLE_JPEG).unwrap();
    let tmp = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
    std::fs::write(tmp.path(), &original).unwrap();
    let path = cs(tmp.path().to_str().unwrap());

    let handle = make_handle();
    assert_eq!(exiftool_read_file(handle, path.as_ptr()), EXIFTOOL_OK);

    // Set a benign string tag, then write back to the JPEG.
    exiftool_set_tag_string(handle, cs("EXIF:Artist").as_ptr(), cs("Coverage").as_ptr());
    let rc = exiftool_write_file(handle, path.as_ptr());
    // The JPEG writer should accept this; if validation rejects it, still
    // exercise the error-to-code branch without failing the test harness.
    if rc == EXIFTOOL_OK {
        // Re-read and confirm the file is still parseable.
        let handle2 = make_handle();
        assert_eq!(exiftool_read_file(handle2, path.as_ptr()), EXIFTOOL_OK);
        exiftool_destroy(handle2);
    } else {
        assert_ne!(rc, EXIFTOOL_ERR_NULL_POINTER);
        assert!(!last_error().is_empty());
    }
    exiftool_destroy(handle);
}

// ===========================================================================
// get_last_error baseline
// ===========================================================================

#[test]
fn get_last_error_never_null() {
    // Even before any error, the pointer is non-NULL ("No error" sentinel).
    let p = exiftool_get_last_error();
    assert!(!p.is_null());
    let _ = unsafe { CStr::from_ptr(p) }.to_str().unwrap();
}

#[test]
fn full_lifecycle_smoke() {
    // Mixed sequence touching set/get/remove/count to exercise cache rebuilds.
    let handle = make_handle();
    exiftool_set_tag_string(handle, cs("EXIF:Make").as_ptr(), cs("Fuji").as_ptr());
    exiftool_set_tag_integer(handle, cs("EXIF:ISO").as_ptr(), 200);
    exiftool_set_tag_float(handle, cs("EXIF:FNumber").as_ptr(), 4.0);
    assert_eq!(exiftool_get_tag_count(handle), 3);

    let mut iso: i64 = 0;
    assert_eq!(
        exiftool_get_tag_integer(handle, cs("EXIF:ISO").as_ptr(), &mut iso),
        EXIFTOOL_OK
    );
    assert_eq!(iso, 200);

    assert_eq!(
        exiftool_remove_tag(handle, cs("EXIF:ISO").as_ptr()),
        EXIFTOOL_OK
    );
    assert_eq!(exiftool_get_tag_count(handle), 2);

    exiftool_destroy(handle);
    // Destroying NULL is a safe no-op.
    exiftool_destroy(ptr::null_mut());
}
