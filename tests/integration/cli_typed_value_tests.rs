//! `-TAG=VALUE` must reach the writer as the tag's *declared* type.
//!
//! Regression cover for the defect where the CLI wrapped every command-line
//! value as `TagValue::String` (single-file path) or guessed a type from the
//! value's own shape (batch path). The write path validates against the type
//! the registry declares, so no Integer, Rational or DateTime tag could be set
//! from the command line on any format — every attempt died with
//! `Type mismatch: expected Integer but got String`.
//!
//! These tests drive the real `oxidex` binary, so they cover the argument
//! parsing, the type resolution and the writer together. Values are proven by
//! the TIFF field type actually serialized (RATIONAL vs ASCII vs SHORT), not
//! only by the reader's rendering — a string "300" and a rational 300/1 print
//! differently but a type-blind assertion could still pass on the wrong bytes.

use oxidex::writers::exif_surgical::{IfdKind, RawEntry, scan_exif_entries};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::{NamedTempFile, TempDir};

const JPEG_FIXTURE: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const TIFF_FIXTURE: &str = "tests/fixtures/tiff/complex/big_endian_001.tif";

// TIFF field types (TIFF 6.0 §2)
const ASCII: u16 = 2;
const SHORT: u16 = 3;
const RATIONAL: u16 = 5;

// Tag ids
const ORIENTATION: u16 = 0x0112;
const X_RESOLUTION: u16 = 0x011A;
const ARTIST: u16 = 0x013B;
const GPS_DOP: u16 = 0x000B;
const GPS_MEASURE_MODE: u16 = 0x000A;
const GPS_DIFFERENTIAL: u16 = 0x001E;

fn oxidex(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

fn copy_fixture(fixture: &str, suffix: &str) -> NamedTempFile {
    let temp = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create temp fixture copy");
    fs::copy(fixture, temp.path()).expect("copy fixture");
    let mut perms = fs::metadata(temp.path()).expect("stat copy").permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(temp.path(), perms).expect("make copy writable");
    temp
}

/// Applies one `-TAG=VALUE` to a fresh copy of `fixture` and returns the path.
fn write_to_copy(fixture: &str, suffix: &str, spec: &str) -> NamedTempFile {
    let temp = copy_fixture(fixture, suffix);
    let path = temp.path().to_str().expect("utf-8 temp path").to_string();
    let output = oxidex(&[spec, &path]);
    assert!(
        output.status.success(),
        "`oxidex {spec}` failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    temp
}

/// The value as the CLI's own reader renders it, e.g. "ExifIFD:ISO: 1600".
fn read_tag(path: &Path, key: &str) -> Option<String> {
    let output = oxidex(&[path.to_str().expect("utf-8 path")]);
    assert!(output.status.success(), "read-back failed");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let prefix = format!("{key}: ");
    stdout
        .lines()
        .find(|line| line.starts_with(&prefix))
        .map(|line| line[prefix.len()..].to_string())
}

/// The raw TIFF entry a *TIFF* file carries for `tag_id`. (A TIFF file is its
/// own EXIF structure, so the whole file can be scanned directly.)
fn tiff_entry(path: &Path, ifd: IfdKind, tag_id: u16) -> RawEntry {
    let bytes = fs::read(path).expect("read written tiff");
    let scan = scan_exif_entries(&bytes).expect("scan written tiff");
    scan.entries
        .into_iter()
        .find(|entry| entry.ifd == ifd && entry.tag_id == tag_id)
        .unwrap_or_else(|| panic!("no {ifd:?} entry for tag 0x{tag_id:04x}"))
}

/// Returns an EXIF entry from a JPEG's APP1 TIFF payload.
fn jpeg_entry(path: &Path, ifd: IfdKind, tag_id: u16) -> RawEntry {
    let bytes = fs::read(path).expect("read written jpeg");
    let exif_start = bytes
        .windows(b"Exif\0\0".len())
        .position(|window| window == b"Exif\0\0")
        .expect("written jpeg must contain EXIF APP1 data");
    let scan = scan_exif_entries(&bytes[exif_start + b"Exif\0\0".len()..])
        .expect("scan written EXIF TIFF");
    scan.entries
        .into_iter()
        .find(|entry| entry.ifd == ifd && entry.tag_id == tag_id)
        .unwrap_or_else(|| panic!("no {ifd:?} entry for tag 0x{tag_id:04x}"))
}

// ---------------------------------------------------------------------------
// JPEG — one tag of each declared type
// ---------------------------------------------------------------------------

#[test]
fn jpeg_integer_tag_is_settable() {
    let file = write_to_copy(JPEG_FIXTURE, ".jpg", "-ExifIFD:ISO=1600");
    assert_eq!(
        read_tag(file.path(), "ExifIFD:ISO").as_deref(),
        Some("1600")
    );
}

#[test]
fn jpeg_rational_tag_is_settable_from_a_fraction() {
    let file = write_to_copy(JPEG_FIXTURE, ".jpg", "-ExifIFD:ExposureTime=1/250");
    assert_eq!(
        read_tag(file.path(), "ExifIFD:ExposureTime").as_deref(),
        Some("1/250")
    );
}

#[test]
fn jpeg_rational_tag_is_settable_from_a_decimal() {
    // ExifTool's Rationalize (Writer.pl:5200-5228) turns 5.6 into 28/5 and
    // 0.004 into 1/250; a decimal and its fraction must agree.
    let by_decimal = write_to_copy(JPEG_FIXTURE, ".jpg", "-ExifIFD:FNumber=5.6");
    assert_eq!(
        read_tag(by_decimal.path(), "ExifIFD:FNumber").as_deref(),
        Some("28/5")
    );

    let by_decimal = write_to_copy(JPEG_FIXTURE, ".jpg", "-ExifIFD:ExposureTime=0.004");
    let by_fraction = write_to_copy(JPEG_FIXTURE, ".jpg", "-ExifIFD:ExposureTime=1/250");
    assert_eq!(
        read_tag(by_decimal.path(), "ExifIFD:ExposureTime"),
        read_tag(by_fraction.path(), "ExifIFD:ExposureTime"),
    );
}

#[test]
fn jpeg_datetime_tag_is_settable() {
    let file = write_to_copy(
        JPEG_FIXTURE,
        ".jpg",
        "-ExifIFD:DateTimeOriginal=2024-01-15T10:30:00",
    );
    assert_eq!(
        read_tag(file.path(), "ExifIFD:DateTimeOriginal").as_deref(),
        Some("2024:01:15 10:30:00"),
    );
}

#[test]
fn jpeg_datetime_accepts_the_canonical_exif_form() {
    let file = write_to_copy(
        JPEG_FIXTURE,
        ".jpg",
        "-ExifIFD:DateTimeOriginal=2024:01:15 10:30:00",
    );
    assert_eq!(
        read_tag(file.path(), "ExifIFD:DateTimeOriginal").as_deref(),
        Some("2024:01:15 10:30:00"),
    );
}

#[test]
fn jpeg_string_tag_is_still_settable() {
    let file = write_to_copy(JPEG_FIXTURE, ".jpg", "-IFD0:Artist=Grace Hopper");
    assert_eq!(
        read_tag(file.path(), "IFD0:Artist").as_deref(),
        Some("Grace Hopper")
    );
}

#[test]
fn jpeg_gps_measure_mode_display_value_is_serialized_as_its_exif_code() {
    // GPS.pm 0x000a's PrintConv maps raw ASCII "2" to this display value.
    // Storing the label itself makes ExifTool report an unknown raw value.
    let file = write_to_copy(
        JPEG_FIXTURE,
        ".jpg",
        "-GPS:GPSMeasureMode=2-Dimensional Measurement",
    );
    let entry = jpeg_entry(file.path(), IfdKind::Gps, GPS_MEASURE_MODE);
    assert_eq!(entry.field_type, ASCII);
    assert_eq!(entry.count, 2);
    assert_eq!(entry.value, b"2\0");
}

#[test]
fn jpeg_gps_differential_display_value_is_serialized_as_its_exif_code() {
    // GPS.pm 0x001e's PrintConv maps raw int16u 0 to this display value.
    // The writer must store the tag's numeric value rather than trying to
    // parse the label as a generic integer.
    let file = write_to_copy(JPEG_FIXTURE, ".jpg", "-GPS:GPSDifferential=No Correction");
    let entry = jpeg_entry(file.path(), IfdKind::Gps, GPS_DIFFERENTIAL);
    assert_eq!(entry.field_type, SHORT);
    assert_eq!(entry.count, 1);
    assert_eq!(entry.value, &[0, 0]);
}

#[test]
fn jpeg_gps_differential_corrected_is_serialized_as_its_exif_code() {
    // GPS.pm 0x001e's PrintConv maps raw int16u 1 to this display value.
    let file = write_to_copy(
        JPEG_FIXTURE,
        ".jpg",
        "-GPS:GPSDifferential=Differential Corrected",
    );
    let entry = jpeg_entry(file.path(), IfdKind::Gps, GPS_DIFFERENTIAL);
    assert_eq!(entry.field_type, SHORT);
    assert_eq!(entry.count, 1);
    assert_eq!(entry.value, &[0, 1]);
}

#[test]
fn jpeg_gps_dop_is_a_rational_and_reads_as_exiftool_decimal() {
    // GPS.pm 13.59 0x000b is rational64u with no PrintConv. ExifTool's
    // rational reader displays the quotient (1.5), not the storage pair
    // (3/2), while the TIFF entry itself must remain an unsigned RATIONAL.
    let file = write_to_copy(JPEG_FIXTURE, ".jpg", "-GPS:GPSDOP=1.5");
    let entry = jpeg_entry(file.path(), IfdKind::Gps, GPS_DOP);
    assert_eq!(entry.field_type, RATIONAL);
    assert_eq!(entry.count, 1);
    assert_eq!(read_tag(file.path(), "GPS:GPSDOP").as_deref(), Some("1.5"));
}

#[test]
fn jpeg_write_leaves_every_other_tag_alone() {
    let before = copy_fixture(JPEG_FIXTURE, ".jpg");
    let baseline = oxidex(&[before.path().to_str().unwrap()]);
    let baseline = String::from_utf8_lossy(&baseline.stdout).into_owned();

    let after = write_to_copy(JPEG_FIXTURE, ".jpg", "-ExifIFD:ISO=1600");
    let updated = oxidex(&[after.path().to_str().unwrap()]);
    let updated = String::from_utf8_lossy(&updated.stdout).into_owned();

    // Only the ISO line may differ (and the added tag raises the tag count).
    let changed: Vec<_> = baseline
        .lines()
        .filter(|line| line.starts_with("ExifIFD:") || line.starts_with("IFD0:"))
        .filter(|line| !updated.lines().any(|other| other == *line))
        .collect();
    assert!(
        changed.is_empty(),
        "setting ISO disturbed unrelated tags: {changed:?}"
    );
}

// ---------------------------------------------------------------------------
// TIFF (non-JPEG) — same, plus the serialized field type
// ---------------------------------------------------------------------------

#[test]
fn tiff_integer_tag_is_settable_and_serialized_as_an_integer() {
    let file = write_to_copy(TIFF_FIXTURE, ".tif", "-IFD0:Orientation=6");
    let entry = tiff_entry(file.path(), IfdKind::Ifd0, ORIENTATION);
    assert_eq!(entry.field_type, SHORT, "Orientation must stay a SHORT");
    assert_eq!(entry.count, 1);
    assert_eq!(
        read_tag(file.path(), "IFD0:Orientation").as_deref(),
        Some("Rotate 90 CW"),
    );
}

#[test]
fn tiff_rational_tag_is_settable_and_serialized_as_a_rational() {
    let file = write_to_copy(TIFF_FIXTURE, ".tif", "-IFD0:XResolution=300");
    let entry = tiff_entry(file.path(), IfdKind::Ifd0, X_RESOLUTION);
    assert_eq!(
        entry.field_type, RATIONAL,
        "XResolution must be a RATIONAL, not the ASCII string \"300\""
    );
    assert_eq!(entry.count, 1);
    assert_eq!(
        read_tag(file.path(), "IFD0:XResolution").as_deref(),
        Some("300/1")
    );
}

#[test]
fn tiff_string_tag_is_settable_and_serialized_as_ascii() {
    let file = write_to_copy(TIFF_FIXTURE, ".tif", "-IFD0:Artist=Grace Hopper");
    let entry = tiff_entry(file.path(), IfdKind::Ifd0, ARTIST);
    assert_eq!(entry.field_type, ASCII);
    assert_eq!(
        read_tag(file.path(), "IFD0:Artist").as_deref(),
        Some("Grace Hopper")
    );
}

#[test]
fn tiff_write_leaves_every_other_tag_alone() {
    let before = copy_fixture(TIFF_FIXTURE, ".tif");
    let baseline = scan_exif_entries(&fs::read(before.path()).unwrap()).unwrap();

    // Orientation is already present in this fixture, so nothing is added and
    // the entry count must be identical.
    let after = write_to_copy(TIFF_FIXTURE, ".tif", "-IFD0:Orientation=6");
    let updated = scan_exif_entries(&fs::read(after.path()).unwrap()).unwrap();

    assert_eq!(baseline.entries.len(), updated.entries.len());
    for original in &baseline.entries {
        if original.tag_id == ORIENTATION {
            continue;
        }
        let same = updated
            .entries
            .iter()
            .any(|entry| entry.ifd == original.ifd && entry.tag_id == original.tag_id);
        assert!(
            same,
            "tag 0x{:04x} in {:?} disappeared",
            original.tag_id, original.ifd
        );
    }
}

// ---------------------------------------------------------------------------
// The type comes from the tag, not from the value's shape
// ---------------------------------------------------------------------------

#[test]
fn an_integer_looking_value_on_a_string_tag_stays_a_string() {
    // The old batch-mode heuristic typed by the value: "800" became
    // TagValue::Integer regardless of the tag, which the type check then
    // rejected as "expected String but got Integer".
    let file = write_to_copy(TIFF_FIXTURE, ".tif", "-IFD0:Artist=800");
    let entry = tiff_entry(file.path(), IfdKind::Ifd0, ARTIST);
    assert_eq!(entry.field_type, ASCII);
    assert_eq!(read_tag(file.path(), "IFD0:Artist").as_deref(), Some("800"));
}

#[test]
fn batch_mode_types_by_the_tag_too() {
    let dir = TempDir::new().expect("temp dir");
    let file = dir.path().join("copy.tif");
    fs::copy(TIFF_FIXTURE, &file).expect("copy fixture into batch dir");
    let mut perms = fs::metadata(&file).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&file, perms).unwrap();

    let output = oxidex(&["-IFD0:XResolution=300", dir.path().to_str().unwrap()]);
    assert!(
        output.status.success(),
        "batch write failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entry = tiff_entry(&file, IfdKind::Ifd0, X_RESOLUTION);
    assert_eq!(entry.field_type, RATIONAL);
}

// ---------------------------------------------------------------------------
// Unparseable values fail loudly and change nothing
// ---------------------------------------------------------------------------

#[test]
fn an_unparseable_value_is_refused_and_the_file_is_untouched() {
    let cases: [(&str, &str, &str); 3] = [
        ("-ExifIFD:ISO=hello world", "ExifIFD:ISO", "Not an integer"),
        (
            "-ExifIFD:FNumber=wide open",
            "ExifIFD:FNumber",
            "Not a floating point number",
        ),
        (
            "-ExifIFD:DateTimeOriginal=yesterday",
            "ExifIFD:DateTimeOriginal",
            "Invalid date/time",
        ),
    ];

    let pristine = fs::read(JPEG_FIXTURE).expect("read fixture");
    for (spec, tag, reason) in cases {
        let temp = copy_fixture(JPEG_FIXTURE, ".jpg");
        let output = oxidex(&[spec, temp.path().to_str().unwrap()]);
        assert!(
            !output.status.success(),
            "`oxidex {spec}` should have failed but reported: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(tag), "error should name {tag}: {stderr}");
        assert!(
            stderr.contains(reason),
            "error should explain '{reason}': {stderr}"
        );
        assert_eq!(
            fs::read(temp.path()).expect("read after failed write"),
            pristine,
            "`oxidex {spec}` modified the file despite failing"
        );
    }
}

#[test]
fn an_unparseable_value_never_silently_becomes_a_string() {
    // The old behaviour's friendlier face: falling back to TagValue::String
    // would have written the literal bytes "hello world" into an Integer tag.
    let temp = copy_fixture(JPEG_FIXTURE, ".jpg");
    let path = temp.path().to_str().unwrap().to_string();
    let _ = oxidex(&["-ExifIFD:ISO=hello world", &path]);
    assert_eq!(
        read_tag(temp.path(), "ExifIFD:ISO"),
        None,
        "a refused value must not be stored in any form"
    );
}

#[test]
fn out_of_range_date_components_are_refused_with_exiftools_wording() {
    // The `T` forms; a date-shaped value containing a space is claimed by the
    // separate `-DateTag=YYYY:mm:dd HH:MM:SS` date-shift parser
    // (`cli::args::parse_date_shift`), which refuses these with its own
    // wording. Either way the write is refused.
    let cases = [
        ("2024-13-15T10:30:00", "Month '13' out of range 1..12"),
        ("2024-01-32T10:30:00", "Day '32' out of range 1..31"),
        ("2024-01-15T25:30:00", "Hour '25' out of range 0..24"),
    ];
    for (value, reason) in cases {
        let temp = copy_fixture(JPEG_FIXTURE, ".jpg");
        let spec = format!("-ExifIFD:DateTimeOriginal={value}");
        let output = oxidex(&[&spec, temp.path().to_str().unwrap()]);
        assert!(!output.status.success(), "{value} should have been refused");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(reason),
            "{value}: expected '{reason}', got {stderr}"
        );
    }
}

// ---------------------------------------------------------------------------
// The exact symptom from the bug report
// ---------------------------------------------------------------------------

#[test]
fn the_reported_type_mismatch_no_longer_happens() {
    for spec in [
        "-IFD0:BitsPerSample=8",
        "-ExifIFD:ISO=800",
        "-ExifIFD:FNumber=5.6",
        "-ExifIFD:ExposureTime=1/250",
    ] {
        let temp = copy_fixture(JPEG_FIXTURE, ".jpg");
        let output = oxidex(&[spec, temp.path().to_str().unwrap()]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("Type mismatch"),
            "`oxidex {spec}` still reports a type mismatch: {stderr}"
        );
        assert!(output.status.success(), "`oxidex {spec}` failed: {stderr}");
    }
}

// ---------------------------------------------------------------------------
// Every declared type has a settable path (or a stated reason it does not)
// ---------------------------------------------------------------------------

#[test]
fn integer_hex_and_rounded_forms_match_checkvalue() {
    // Writer.pl:6873-6883 — IsInt, then IsHex, then a rounded IsFloat.
    for (value, expected) in [("800", "800"), ("0x320", "800"), ("800.5", "801")] {
        let spec = format!("-ExifIFD:ISO={value}");
        let file = write_to_copy(JPEG_FIXTURE, ".jpg", &spec);
        assert_eq!(
            read_tag(file.path(), "ExifIFD:ISO").as_deref(),
            Some(expected),
            "input {value}"
        );
    }
}
