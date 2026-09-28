//! A JPEG with two EXIF APP1 records: every EXIF-family set or deletion is
//! refused by name (`TagsNotWritten`), through the library, the CLI and the
//! C ABI alike, and the file is left byte-identical.
//!
//! Pinned ExifTool 13.59 writes every EXIF APP1 of such a file (it warns
//! `Multiple APP1 EXIF records`, then edits each block, or drops both on
//! `-EXIF:All=`); oxidex writes one. Before, a grouped set on
//! `multi-app1-nikon-lsi1.jpg` edited the first block alone -- planting the
//! second block's IFD0 ImageWidth/ImageHeight in it -- and reported
//! `1 image files updated`; `-IFD0:Artist=x` failed "Ambiguous multiple JPEG
//! EXIF blocks"; and on `multi-app1-lsi1-nikon.jpg` a set failed
//! `Invalid value for tag 'IFD0:Orientation'`, the second block's row diffed
//! against the first block. `oracle_writes_every_exif_app1` pins what the
//! oracle does with each request, which is why each is refused.
//!
//! The same holds for a bare maker-note name (`-Quality=Normal`, which the
//! oracle writes into both maker notes of `multi-app1-canon-nikon.jpg`) and
//! for the IFD chain past IFD1 (`IFD2:*` on `multi-app1-leicatl2-nikon.jpg`,
//! which the oracle leaves unwritten): before, each reported
//! `1 image files updated` with the file byte-identical.
//!
//! Fixtures: tests/fixtures/jpeg/multi_exif_app1 (PROVENANCE.txt);
//! `fixtures_rebuild_from_their_pinned_sources` re-derives them.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::read_metadata_with_detector_and_options;
use oxidex::core::operations::{
    copy_metadata, modify_tag, read_metadata, remove_tag, write_metadata,
};
use oxidex::core::read_options::ReadOptions;
use oxidex::core::tag_value::TagValue;
use oxidex::exiftool_oracle;
use oxidex::ffi::{
    EXIFTOOL_ERR_TAG_NOT_WRITTEN, EXIFTOOL_OK, exiftool_create, exiftool_destroy,
    exiftool_get_last_error, exiftool_get_tag_string, exiftool_has_tag, exiftool_read_file,
    exiftool_remove_tag, exiftool_set_tag_string, exiftool_write_file,
};
use oxidex::parsers::DetectorMode;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURES: [&str; 2] = ["multi-app1-nikon-lsi1.jpg", "multi-app1-lsi1-nikon.jpg"];

/// Canon.jpg's first block, whose maker note the reader decodes
/// (`Canon:Quality`), then Nikon.jpg's.
const CANON: &str = "multi-app1-canon-nikon.jpg";

/// LeicaTL2.jpg's first block, whose chain reaches IFD2 (the reader's
/// `IFD2:PreviewImageStart` ...), then Nikon.jpg's.
const LEICA: &str = "multi-app1-leicatl2-nikon.jpg";

/// Nikon.jpg's block, then DJI_XT2.jpg's, whose ExifIFD ApplicationNotes
/// the default read surfaces no row for.
const DJI: &str = "multi-app1-nikon-djixt2.jpg";

const REASON: &str =
    "the file has 2 EXIF APP1 blocks; ExifTool writes every one, oxidex writes one";

/// The same refusal of a bare name by #960's request resolver.
const BARE_NAME_REASON: &str = "the file carries 2 EXIF blocks, each of which ExifTool writes";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jpeg/multi_exif_app1")
        .join(name)
}

/// A private copy of the fixture `name` in `dir`.
fn copy(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::copy(fixture(name), &path).unwrap();
    path
}

/// The TIFF payload of every `Exif\0\0` APP1 in the JPEG header.
fn exif_app1_payloads(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 2;
    while i + 4 <= data.len() && data[i] == 0xFF {
        let marker = data[i + 1];
        if matches!(marker, 0xD9 | 0xDA) {
            break;
        }
        let length = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        let segment = &data[i + 4..i + 2 + length];
        if marker == 0xE1 && segment.starts_with(b"Exif\0\0") {
            out.push(segment[6..].to_vec());
        }
        i += 2 + length;
    }
    out
}

/// What pinned ExifTool 13.59 does with a request on either fixture.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Oracle {
    /// Warns `Multiple APP1 EXIF records` and rewrites both blocks.
    EveryBlock,
    /// Drops both EXIF APP1 records (no warning).
    NoBlockLeft,
    /// Warns, and leaves the file unchanged (the tag is in neither block).
    Unchanged,
    /// Leaves the file unchanged without the warning: no EXIF block is
    /// rewritten, since the tag's group is no group it writes (the IFD chain
    /// past IFD1). oxidex cannot say so -- its EXIF writers reported the
    /// file updated -- so it refuses by name.
    NotWritten,
    /// Fails ("Error reading JpgFromRaw data in IFD2": LeicaTL2.jpg is a
    /// truncated sample) and leaves the file unchanged.
    Failed,
}

/// The library call a CLI argument corresponds to.
#[derive(Clone, Copy, Debug)]
enum Lib {
    Set(&'static str, fn() -> TagValue),
    Remove(&'static str),
}

struct Case {
    /// One CLI argument, as given to both tools.
    arg: &'static str,
    /// The key the CLI's refusal names.
    cli_names: &'static str,
    lib: Lib,
    /// The key the library's refusal names.
    lib_names: &'static str,
    oracle: Oracle,
}

fn cases() -> Vec<Case> {
    use Lib::*;
    use Oracle::*;
    let c = |arg, cli_names, lib, lib_names, oracle| Case {
        arg,
        cli_names,
        lib,
        lib_names,
        oracle,
    };
    vec![
        c(
            "-ExifIFD:WhiteBalance#=1",
            "ExifIFD:WhiteBalance#",
            Set("ExifIFD:WhiteBalance", || TagValue::Integer(1)),
            "ExifIFD:WhiteBalance",
            EveryBlock,
        ),
        c(
            "-ExifIFD:WhiteBalance=Manual",
            "ExifIFD:WhiteBalance",
            Set("ExifIFD:WhiteBalance", || TagValue::new_string("Manual")),
            "ExifIFD:WhiteBalance",
            EveryBlock,
        ),
        c(
            "-IFD0:Artist=x",
            "IFD0:Artist",
            Set("IFD0:Artist", || TagValue::new_string("x")),
            "IFD0:Artist",
            EveryBlock,
        ),
        c(
            "-GPS:GPSAltitude=10",
            "GPS:GPSAltitude",
            Set("GPS:GPSAltitude", || TagValue::new_string("10")),
            "GPS:GPSAltitude",
            EveryBlock,
        ),
        c(
            "-IFD1:XResolution=72",
            "IFD1:XResolution",
            Set("IFD1:XResolution", || TagValue::Integer(72)),
            "IFD1:XResolution",
            EveryBlock,
        ),
        c(
            "-ExifIFD:CreateDate=2020:01:01 00:00:00",
            "ExifIFD:CreateDate",
            Set("ExifIFD:CreateDate", || {
                TagValue::new_string("2020:01:01 00:00:00")
            }),
            "ExifIFD:CreateDate",
            EveryBlock,
        ),
        c(
            "-Artist=x",
            "Artist",
            Set("Artist", || TagValue::new_string("x")),
            "Artist",
            EveryBlock,
        ),
        c(
            "-WhiteBalance#=1",
            "WhiteBalance#",
            Set("WhiteBalance", || TagValue::Integer(1)),
            "WhiteBalance",
            EveryBlock,
        ),
        c(
            "-ExifIFD:CreateDate=",
            "ExifIFD:CreateDate",
            Remove("ExifIFD:CreateDate"),
            "ExifIFD:CreateDate",
            EveryBlock,
        ),
        c(
            "-IFD0:Software=",
            "IFD0:Software",
            Remove("IFD0:Software"),
            "IFD0:Software",
            EveryBlock,
        ),
        c(
            "-Software=",
            "Software",
            Remove("Software"),
            "Software",
            EveryBlock,
        ),
        c(
            "-ExifIFD:WhiteBalance=",
            "ExifIFD:WhiteBalance",
            Remove("ExifIFD:WhiteBalance"),
            "ExifIFD:WhiteBalance",
            Unchanged,
        ),
        c(
            "-ExifIFD:All=",
            "ExifIFD:All",
            Remove("ExifIFD:All"),
            "ExifIFD:All",
            EveryBlock,
        ),
        c(
            "-GPS:All=",
            "GPS:All",
            Remove("GPS:All"),
            "GPS:All",
            Unchanged,
        ),
        c(
            "-MakerNotes:All=",
            "MakerNotes:All",
            Remove("MakerNotes:All"),
            "MakerNotes:All",
            EveryBlock,
        ),
        c(
            "-EXIF:All=",
            "EXIF:All",
            Remove("EXIF:All"),
            "EXIF:All",
            NoBlockLeft,
        ),
        c(
            "-IFD0:All=",
            "IFD0:All",
            Remove("IFD0:All"),
            "IFD0:All",
            NoBlockLeft,
        ),
    ]
}

/// Every request on the fixture it is made of: each of [`cases`] on both
/// Nikon fixtures, then a bare maker-note name on [`CANON`] and the IFD chain
/// past IFD1 on [`LEICA`].
fn all_cases() -> Vec<(&'static str, Case)> {
    use Lib::*;
    use Oracle::*;
    let c = |arg, cli_names, lib, lib_names, oracle| Case {
        arg,
        cli_names,
        lib,
        lib_names,
        oracle,
    };
    let mut all: Vec<(&'static str, Case)> = FIXTURES
        .iter()
        .flat_map(|name| cases().into_iter().map(move |case| (*name, case)))
        .collect();
    all.extend([
        (
            CANON,
            c(
                "-Quality=Normal",
                "Quality",
                Set("Quality", || TagValue::new_string("Normal")),
                "Quality",
                EveryBlock,
            ),
        ),
        (
            CANON,
            c(
                "-IFD0:Artist=x",
                "IFD0:Artist",
                Set("IFD0:Artist", || TagValue::new_string("x")),
                "IFD0:Artist",
                EveryBlock,
            ),
        ),
        (
            LEICA,
            c(
                "-IFD2:XResolution=300",
                "IFD2:XResolution",
                Set("IFD2:XResolution", || TagValue::new_string("300")),
                "IFD2:XResolution",
                NotWritten,
            ),
        ),
        (
            LEICA,
            c(
                "-IFD2:XResolution=",
                "IFD2:XResolution",
                Remove("IFD2:XResolution"),
                "IFD2:XResolution",
                NotWritten,
            ),
        ),
        (
            LEICA,
            c(
                "-OriginalDirectory=999LEICA",
                "OriginalDirectory",
                Set("OriginalDirectory", || TagValue::new_string("999LEICA")),
                "OriginalDirectory",
                Failed,
            ),
        ),
        (
            LEICA,
            c(
                "-IFD0:Artist=x",
                "IFD0:Artist",
                Set("IFD0:Artist", || TagValue::new_string("x")),
                "IFD0:Artist",
                Failed,
            ),
        ),
    ]);
    all
}

/// Why `message` is not the multi-block refusal of `key` (and, given
/// `debug`, not the typed `TagsNotWritten` variant), or `None` when it is.
/// #960's request resolver refuses a bare name on such a file before the
/// transaction, in its own wording ([`BARE_NAME_REASON`]) and under the name
/// as the request spelled it; its CLI also drops a trailing `#` before
/// naming the key. Either is this refusal.
fn refusal_problem(label: &str, message: &str, debug: Option<&str>, key: &str) -> Option<String> {
    let plain = key.trim_end_matches('#');
    let bare = plain.rsplit_once(':').map_or(plain, |(_, name)| name);
    let named =
        |key: &str, reason: &str| message.contains(&format!("Cannot write tag '{key}': {reason}"));
    let refused = named(key, REASON)
        || named(plain, REASON)
        || [key, plain, bare]
            .iter()
            .any(|key| named(key, BARE_NAME_REASON));
    if !refused {
        return Some(format!(
            "{label}: expected the refusal of {key:?} ({REASON:?}), got {message:?}"
        ));
    }
    match debug {
        Some(debug) if !debug.starts_with("TagsNotWritten") => Some(format!(
            "{label}: expected a TagsNotWritten error, got {debug}"
        )),
        _ => None,
    }
}

/// Every problem found, reported together.
fn assert_no_problems(problems: Vec<String>) {
    assert!(
        problems.is_empty(),
        "{} problem(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn library_refuses_every_exif_write_and_leaves_the_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut problems = Vec::new();
    for (name, case) in all_cases() {
        let original = std::fs::read(fixture(name)).unwrap();
        let path = copy(dir.path(), name);
        let label = format!("{name} {:?}", case.lib);
        let result = match case.lib {
            Lib::Set(key, value) => modify_tag(&path, key, value()),
            Lib::Remove(key) => remove_tag(&path, key),
        };
        match result {
            // A deletion of a tag in neither block: ExifTool leaves the
            // file unchanged; #960 does too, before the transaction.
            Ok(_) if case.oracle == Oracle::Unchanged => {}
            Ok(_) => problems.push(format!("{label}: expected a refusal, got Ok")),
            Err(err) => problems.extend(refusal_problem(
                &label,
                &err.to_string(),
                Some(&format!("{err:?}")),
                case.lib_names,
            )),
        }
        if std::fs::read(&path).unwrap() != original {
            problems.push(format!("{label}: the file changed"));
        }
    }
    assert_no_problems(problems);
}

#[test]
fn public_byte_writers_refuse_partial_multi_app1_rewrites() {
    use oxidex::core::metadata_map::MetadataMap;
    use oxidex::core::operations::clear_all_metadata;
    use oxidex::io::MMapReader;
    use oxidex::writers::exif_surgical::rewrite_jpeg_exif;
    use oxidex::writers::jpeg_writer::write_exif_to_jpeg;

    let dir = tempfile::tempdir().unwrap();
    let path = copy(dir.path(), FIXTURES[0]);
    let original = std::fs::read(&path).unwrap();
    let reader = MMapReader::new(&path).unwrap();
    let mut desired = MetadataMap::new();
    desired.insert("IFD0:Artist", TagValue::new_string("new"));
    for result in [
        rewrite_jpeg_exif(&original, &desired),
        write_exif_to_jpeg(&reader, &desired),
    ] {
        let error = result.expect_err("a public byte writer must refuse partial EXIF output");
        assert!(error.to_string().contains("IFD0:Artist"), "{error}");
        assert!(error.to_string().contains(REASON), "{error}");
    }
    // At this level an empty desired map would delete only the first EXIF
    // record. The whole-file clear below has the carrier-wide semantics.
    assert!(rewrite_jpeg_exif(&original, &MetadataMap::new()).is_err());
    assert!(write_exif_to_jpeg(&reader, &MetadataMap::new()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);

    clear_all_metadata(&path).unwrap();
    assert!(exif_app1_payloads(&std::fs::read(&path).unwrap()).is_empty());
}

#[test]
fn absent_bare_exif_deletion_stays_unchanged_across_two_app1_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy(dir.path(), CANON);
    let original = std::fs::read(&path).unwrap();
    remove_tag(&path, "CalibrationIlluminant1").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);

    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-CalibrationIlluminant1=")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 image files unchanged"),
        "{output:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn two_unknown_makernotes_are_an_unchanged_group_deletion() {
    // The committed LSI/Nikon fixture supplies both physical APP1 segments.
    // Repeat its first (LSI1) segment so this case needs no external fixture.
    let source = std::fs::read(fixture("multi-app1-lsi1-nikon.jpg")).unwrap();
    let mut segments = Vec::new();
    let mut at = 2;
    while at + 4 <= source.len() && source[at] == 0xff {
        if matches!(source[at + 1], 0xd9 | 0xda) {
            break;
        }
        let length = u16::from_be_bytes([source[at + 2], source[at + 3]]) as usize;
        let end = at + 2 + length;
        if source[at + 1] == 0xe1 && source[at + 4..end].starts_with(b"Exif\0\0") {
            segments.push((at, end));
        }
        at = end;
    }
    assert_eq!(segments.len(), 2);
    let (first, first_end) = segments[0];
    let (second, second_end) = segments[1];
    let original = [
        &source[..second],
        &source[first..first_end],
        &source[second_end..],
    ]
    .concat();
    assert_eq!(exif_app1_payloads(&original).len(), 2);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two-lsi1.jpg");
    std::fs::write(&path, &original).unwrap();
    remove_tag(&path, "MakerNotes:All").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);

    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-MakerNotes:All=")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 image files unchanged"),
        "{output:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);

    let handle = exiftool_create();
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let c_key = CString::new("MakerNotes:All").unwrap();
    assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
    assert_eq!(exiftool_remove_tag(handle, c_key.as_ptr()), EXIFTOOL_OK);
    let code = exiftool_write_file(handle, c_path.as_ptr());
    let message = unsafe { CStr::from_ptr(exiftool_get_last_error()) }
        .to_string_lossy()
        .into_owned();
    exiftool_destroy(handle);
    assert_eq!(code, EXIFTOOL_OK, "{message}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn absent_grouped_deletion_uses_canonical_address_and_keeps_request_spelling() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy(dir.path(), CANON);
    let original = std::fs::read(&path).unwrap();
    for key in ["IFD0:CalibrationIlluminant1", "ifd0:CalibrationIlluminant1"] {
        remove_tag(&path, key).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original, "{key}");

        let handle = exiftool_create();
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let c_key = CString::new(key).unwrap();
        assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
        assert_eq!(exiftool_remove_tag(handle, c_key.as_ptr()), EXIFTOOL_OK);
        let code = exiftool_write_file(handle, c_path.as_ptr());
        let message = unsafe { CStr::from_ptr(exiftool_get_last_error()) }
            .to_string_lossy()
            .into_owned();
        exiftool_destroy(handle);
        assert_eq!(code, EXIFTOOL_OK, "{key}: {message}");
        assert_eq!(std::fs::read(&path).unwrap(), original, "{key}");
    }

    let key = "ifd0:Software";
    let err = remove_tag(&path, key).unwrap_err();
    assert_no_problems(
        refusal_problem(key, &err.to_string(), Some(&format!("{err:?}")), key)
            .into_iter()
            .collect(),
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);

    let handle = exiftool_create();
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let c_key = CString::new(key).unwrap();
    assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
    assert_eq!(exiftool_remove_tag(handle, c_key.as_ptr()), EXIFTOOL_OK);
    let code = exiftool_write_file(handle, c_path.as_ptr());
    let message = unsafe { CStr::from_ptr(exiftool_get_last_error()) }
        .to_string_lossy()
        .into_owned();
    exiftool_destroy(handle);
    assert_eq!(code, EXIFTOOL_ERR_TAG_NOT_WRITTEN, "{message}");
    assert_no_problems(
        refusal_problem(key, &message, None, key)
            .into_iter()
            .collect(),
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn a_read_map_write_names_each_changed_exif_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut problems = Vec::new();
    for name in FIXTURES {
        let original = std::fs::read(fixture(name)).unwrap();
        let path = copy(dir.path(), name);
        let mut map = read_metadata(&path).unwrap();
        // The unchanged read map is a no-op, as on any file.
        write_metadata(&path, &map).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original, "{name}: no-op");
        map.insert("IFD0:Artist", TagValue::new_string("x"));
        map.remove("IFD0:Software");
        match write_metadata(&path, &map) {
            Ok(_) => problems.push(format!("{name}: expected a refusal, got Ok")),
            Err(err) => {
                let message = err.to_string();
                let debug = format!("{err:?}");
                problems.extend(refusal_problem(name, &message, Some(&debug), "IFD0:Artist"));
                problems.extend(refusal_problem(name, &message, None, "IFD0:Software"));
            }
        }
        if std::fs::read(&path).unwrap() != original {
            problems.push(format!("{name}: the file changed"));
        }
    }
    assert_no_problems(problems);
}

/// A row of the IFD chain past IFD1 dropped from the read map is a deletion
/// the EXIF writers do not make: refused by name, the file unchanged.
#[test]
fn a_read_map_write_that_drops_an_ifd2_row_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let original = std::fs::read(fixture(LEICA)).unwrap();
    let path = copy(dir.path(), LEICA);
    let mut map = read_metadata(&path).unwrap();
    assert!(
        map.contains_key("IFD2:JpgFromRawLength"),
        "fixture shape: the reader files LeicaTL2.jpg's preview rows under IFD2"
    );
    map.remove("IFD2:JpgFromRawLength");
    match write_metadata(&path, &map) {
        Ok(_) => panic!("{LEICA}: expected a refusal, got Ok"),
        Err(err) => assert_no_problems(
            refusal_problem(
                LEICA,
                &err.to_string(),
                Some(&format!("{err:?}")),
                "IFD2:JpgFromRawLength",
            )
            .into_iter()
            .collect(),
        ),
    }
    assert_eq!(std::fs::read(&path).unwrap(), original, "{LEICA}");
}

#[test]
fn cli_refuses_every_exif_write_and_leaves_the_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut problems = Vec::new();
    for (name, case) in all_cases() {
        let original = std::fs::read(fixture(name)).unwrap();
        let path = copy(dir.path(), name);
        let label = format!("{name} {}", case.arg);
        let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .arg(case.arg)
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("1 image files updated") {
            problems.push(format!("{label}: reported an update: {stdout:?}"));
        }
        // A deletion of a tag in neither block may instead be ExifTool's
        // own no-op ("0 image files updated", "1 image files unchanged").
        let unchanged = case.oracle == Oracle::Unchanged
            && out.status.success()
            && stdout.contains("1 image files unchanged");
        if !unchanged {
            if out.status.success() {
                problems.push(format!("{label}: exit {:?}, stdout {stdout:?}", out.status));
            }
            problems.extend(refusal_problem(&label, &stderr, None, case.cli_names));
        }
        if std::fs::read(&path).unwrap() != original {
            problems.push(format!("{label}: the file changed"));
        }
    }
    assert_no_problems(problems);
}

#[test]
fn c_abi_refuses_an_exif_set_and_a_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let requests = FIXTURES
        .iter()
        .flat_map(|name| {
            [
                (*name, "set", "IFD0:Artist", Some("x")),
                (*name, "remove", "IFD0:Software", None),
            ]
        })
        .chain([
            (CANON, "set", "Quality", Some("Normal")),
            (LEICA, "set", "IFD2:XResolution", Some("300")),
        ]);
    for (name, label, key, value) in requests {
        let original = std::fs::read(fixture(name)).unwrap();
        let path = copy(dir.path(), name);
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let c_key = CString::new(key).unwrap();
        let handle = exiftool_create();
        assert!(!handle.is_null());
        assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), 0);
        let edit = match value {
            Some(value) => {
                let c_value = CString::new(value).unwrap();
                exiftool_set_tag_string(handle, c_key.as_ptr(), c_value.as_ptr())
            }
            None => exiftool_remove_tag(handle, c_key.as_ptr()),
        };
        assert_eq!(edit, 0, "{name} {label}: handle edit");
        let code = exiftool_write_file(handle, c_path.as_ptr());
        let message = unsafe { CStr::from_ptr(exiftool_get_last_error()) }
            .to_string_lossy()
            .into_owned();
        exiftool_destroy(handle);
        assert_eq!(
            code, EXIFTOOL_ERR_TAG_NOT_WRITTEN,
            "{name} {label}: code {code}, {message}"
        );
        assert_no_problems(
            refusal_problem(&format!("{name} {label}"), &message, None, key)
                .into_iter()
                .collect(),
        );
        assert_eq!(std::fs::read(&path).unwrap(), original, "{name} {label}");
    }
}

/// `multi-app1-nikon-lsi1.jpg` with its second EXIF APP1 spelled as each
/// header pinned ExifTool 13.59 still counts as a second record: a 0xFF fill
/// byte before it, an `Exif\0\x01` identifier, a lower-case `exif\0\0`.
fn header_variants() -> Vec<(&'static str, Vec<u8>)> {
    let data = std::fs::read(fixture(FIXTURES[0])).unwrap();
    let mut starts = Vec::new();
    let mut i = 2;
    while data[i + 1] != 0xDA {
        let length = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if data[i + 1] == 0xE1 && data[i + 4..].starts_with(b"Exif\0\0") {
            starts.push(i);
        }
        i += 2 + length;
    }
    assert_eq!(starts.len(), 2, "fixture shape");
    let second = starts[1];
    let respelled =
        |identifier: &[u8]| [&data[..second + 4], identifier, &data[second + 10..]].concat();
    vec![
        (
            "fill byte",
            [&data[..second], &[0xFF][..], &data[second..]].concat(),
        ),
        ("Exif\\0\\x01", respelled(b"Exif\0\x01")),
        ("exif\\0\\0", respelled(b"exif\0\0")),
        (
            "identifier only",
            [
                &data[..second],
                &b"\xff\xe1\0\x08Exif\0\0"[..],
                &data[second
                    + 2
                    + u16::from_be_bytes([data[second + 2], data[second + 3]]) as usize..],
            ]
            .concat(),
        ),
    ]
}

/// A second EXIF APP1 the reader's segment parser does not find, but
/// pinned ExifTool 13.59 edits (`oracle_counts_every_header_variant`), is
/// counted: the write is refused and the file left byte-identical. Before,
/// the fill-byte variant was written as one block and truncated (2010 ->
/// 1472 bytes, "JPEG format error"), and the identifier variants had their
/// first block edited alone.
#[test]
fn header_variants_are_refused_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut problems = Vec::new();
    for (variant, bytes) in header_variants() {
        let path = dir.path().join("variant.jpg");
        std::fs::write(&path, &bytes).unwrap();
        match modify_tag(
            &path,
            "ExifIFD:WhiteBalance",
            TagValue::new_string("Manual"),
        ) {
            Ok(_) => problems.push(format!("{variant}: expected a refusal, got Ok")),
            Err(err) => problems.extend(refusal_problem(
                variant,
                &err.to_string(),
                Some(&format!("{err:?}")),
                "ExifIFD:WhiteBalance",
            )),
        }
        if std::fs::read(&path).unwrap() != bytes {
            problems.push(format!("{variant}: the library changed the file"));
        }
        let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .arg("-ExifIFD:WhiteBalance=Manual")
            .arg(&path)
            .output()
            .unwrap();
        if out.status.success() {
            problems.push(format!("{variant}: CLI exit {:?}: {out:?}", out.status));
        }
        problems.extend(refusal_problem(
            variant,
            &String::from_utf8_lossy(&out.stderr),
            None,
            "ExifIFD:WhiteBalance",
        ));
        if std::fs::read(&path).unwrap() != bytes {
            problems.push(format!("{variant}: the CLI changed the file"));
        }
    }
    assert_no_problems(problems);
}

/// Pinned ExifTool 13.59 counts the second record of every variant (it
/// warns `Multiple APP1 EXIF records`): it writes both blocks of the
/// fill-byte and `Exif\0\x01` variants, and refuses the lower-case one
/// (`[minor] Incorrect EXIF segment identifier`, file unchanged).
#[test]
fn oracle_counts_every_header_variant() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for (variant, bytes) in header_variants() {
        let path = dir.path().join("variant.jpg");
        std::fs::write(&path, &bytes).unwrap();
        let label = format!("{variant} (oracle {})", oracle.display());
        let out = oracle
            .command()
            .args(["-overwrite_original", "-ExifIFD:WhiteBalance=Manual"])
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stderr.contains("Warning: Multiple APP1 EXIF records"),
            "{label}: {stderr}"
        );
        if variant.starts_with("exif") {
            assert!(
                stderr.contains("Incorrect EXIF segment identifier"),
                "{label}: {stderr}"
            );
            assert!(stdout.contains("0 image files updated"), "{label}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{label}");
            continue;
        }
        assert!(stdout.contains("1 image files updated"), "{label}");
        let rows = oracle
            .command()
            .args(["-a", "-s3", "-ExifIFD:WhiteBalance"])
            .arg(&path)
            .output()
            .unwrap();
        let rows: Vec<String> = String::from_utf8_lossy(&rows.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(rows, ["Manual", "Manual"], "{label}");
    }
}

/// Through the C ABI: read `path`, set `key` to the value the handle read
/// back for it, write. Returns the write's code, its last error, and the
/// value that was set.
fn c_abi_same_value_set(path: &Path, key: &str) -> (i32, String, String) {
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let c_key = CString::new(key).unwrap();
    let handle = exiftool_create();
    assert!(!handle.is_null());
    assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
    let read = exiftool_get_tag_string(handle, c_key.as_ptr());
    assert!(!read.is_null(), "{}: no {key} row", path.display());
    let value = unsafe { CStr::from_ptr(read) }.to_owned();
    assert_eq!(
        exiftool_set_tag_string(handle, c_key.as_ptr(), value.as_ptr()),
        EXIFTOOL_OK
    );
    let code = exiftool_write_file(handle, c_path.as_ptr());
    let message = unsafe { CStr::from_ptr(exiftool_get_last_error()) }
        .to_string_lossy()
        .into_owned();
    exiftool_destroy(handle);
    (code, message, value.to_string_lossy().into_owned())
}

/// A C-ABI set to the value the handle read back is still a set: pinned
/// ExifTool 13.59 rewrites the block holding the other value
/// (`oracle_rewrites_the_other_block_on_a_same_value_set`), so on a file
/// with two EXIF APP1 blocks it is refused like any other set, not
/// reported done with the blocks still disagreeing. On a file with one
/// block it writes as before.
#[test]
fn c_abi_refuses_a_same_value_set() {
    let dir = tempfile::tempdir().unwrap();
    for name in FIXTURES {
        let original = std::fs::read(fixture(name)).unwrap();
        let path = copy(dir.path(), name);
        let (code, message, _) = c_abi_same_value_set(&path, "IFD0:Make");
        assert_eq!(
            code, EXIFTOOL_ERR_TAG_NOT_WRITTEN,
            "{name}: code {code}, {message}"
        );
        assert_no_problems(
            refusal_problem(name, &message, None, "IFD0:Make")
                .into_iter()
                .collect(),
        );
        assert_eq!(std::fs::read(&path).unwrap(), original, "{name}");
    }
    let single =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/sample_with_exif.jpg");
    let path = dir.path().join("sample_with_exif.jpg");
    std::fs::copy(&single, &path).unwrap();
    let (code, message, _) = c_abi_same_value_set(&path, "IFD0:Make");
    assert_eq!(code, EXIFTOOL_OK, "one EXIF APP1: {message}");
}

/// Through the C ABI: read `path`, remove `key` -- which the handle's map
/// must not hold -- and write. Returns the write's code and last error.
fn c_abi_remove_rowless(path: &Path, key: &str) -> (i32, String) {
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let c_key = CString::new(key).unwrap();
    let handle = exiftool_create();
    assert!(!handle.is_null());
    assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
    assert_eq!(
        exiftool_has_tag(handle, c_key.as_ptr()),
        0,
        "{}: the default read surfaces no {key} row",
        path.display()
    );
    assert_eq!(exiftool_remove_tag(handle, c_key.as_ptr()), EXIFTOOL_OK);
    let code = exiftool_write_file(handle, c_path.as_ptr());
    let message = unsafe { CStr::from_ptr(exiftool_get_last_error()) }
        .to_string_lossy()
        .into_owned();
    exiftool_destroy(handle);
    (code, message)
}

/// `ExifIFD:ApplicationNotes` read on request (the default read withholds
/// it).
fn requested_application_notes(path: &Path) -> Option<TagValue> {
    let options = ReadOptions::new(&["ExifIFD:ApplicationNotes".to_string()], false);
    read_metadata_with_detector_and_options(path, DetectorMode::Signature, &options)
        .unwrap()
        .get("ExifIFD:ApplicationNotes")
        .cloned()
}

/// A C-ABI removal of an entry the handle's map has no row for is still a
/// removal: pinned ExifTool 13.59 deletes DJI_XT2.jpg's ApplicationNotes
/// (`oracle_deletes_a_rowless_entry`), so on a file with two EXIF APP1
/// blocks it is refused like `remove_tag`'s, not reported done with the
/// entry still there; on DJI_XT2.jpg itself it is deleted, as `remove_tag`
/// deletes it.
#[test]
fn c_abi_forwards_a_removal_of_a_rowless_entry() {
    let dir = tempfile::tempdir().unwrap();
    let original = std::fs::read(fixture(DJI)).unwrap();
    let path = copy(dir.path(), DJI);
    assert!(
        requested_application_notes(&path).is_some(),
        "fixture shape"
    );
    let (code, message) = c_abi_remove_rowless(&path, "ExifIFD:ApplicationNotes");
    assert_eq!(
        code, EXIFTOOL_ERR_TAG_NOT_WRITTEN,
        "{DJI}: code {code}, {message}"
    );
    assert_no_problems(
        refusal_problem(DJI, &message, None, "ExifIFD:ApplicationNotes")
            .into_iter()
            .collect(),
    );
    assert_eq!(std::fs::read(&path).unwrap(), original, "{DJI}");

    let Some(single) = fixtures::pinned_combined_fixture_path("DJI/DJI_XT2.jpg") else {
        eprintln!("skipping the one-block half: pinned DJI/DJI_XT2.jpg absent");
        return;
    };
    let path = dir.path().join("DJI_XT2.jpg");
    std::fs::copy(single, &path).unwrap();
    assert!(
        requested_application_notes(&path).is_some(),
        "DJI_XT2.jpg shape"
    );
    let (code, message) = c_abi_remove_rowless(&path, "ExifIFD:ApplicationNotes");
    assert_eq!(code, EXIFTOOL_OK, "DJI_XT2.jpg: {message}");
    assert_eq!(requested_application_notes(&path), None, "DJI_XT2.jpg");
}

/// Pinned ExifTool 13.59 deletes the ApplicationNotes entry of the DJI
/// block (warning `Multiple APP1 EXIF records`), which oxidex cannot.
#[test]
fn oracle_deletes_a_rowless_entry() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = copy(dir.path(), DJI);
    let label = format!("{DJI} (oracle {})", oracle.display());
    let rows = |path: &Path| {
        let out = oracle
            .command()
            .args(["-a", "-s3", "-ExifIFD:ApplicationNotes"])
            .arg(path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).lines().count()
    };
    assert_eq!(rows(&path), 1, "{label}");
    let out = oracle
        .command()
        .args(["-overwrite_original", "-ExifIFD:ApplicationNotes="])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{label}: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Multiple APP1 EXIF records"),
        "{label}: {out:?}"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1 image files updated"),
        "{label}: {out:?}"
    );
    assert_eq!(rows(&path), 0, "{label}");
}

/// Pinned ExifTool 13.59 on a set of IFD0:Make to the value oxidex reads
/// back (the later block's): it rewrites the earlier block, whose Make
/// differs, so both read back the value set.
#[test]
fn oracle_rewrites_the_other_block_on_a_same_value_set() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for name in FIXTURES {
        let path = copy(dir.path(), name);
        let winner = read_metadata(&path)
            .unwrap()
            .get_string("IFD0:Make")
            .map(str::to_string)
            .unwrap();
        let label = format!("{name} (oracle {})", oracle.display());
        let before = oracle
            .command()
            .args(["-a", "-s3", "-IFD0:Make"])
            .arg(&path)
            .output()
            .unwrap();
        let before: Vec<String> = String::from_utf8_lossy(&before.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(before.len(), 2, "{label}: {before:?}");
        assert!(
            before.contains(&winner) && before.iter().any(|make| *make != winner),
            "{label}: the blocks' Make must differ: {before:?}"
        );
        let out = oracle
            .command()
            .args(["-overwrite_original", &format!("-IFD0:Make={winner}")])
            .arg(&path)
            .output()
            .unwrap();
        assert!(out.status.success(), "{label}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("1 image files updated"),
            "{label}: {out:?}"
        );
        let after = oracle
            .command()
            .args(["-a", "-s3", "-IFD0:Make"])
            .arg(&path)
            .output()
            .unwrap();
        let after: Vec<String> = String::from_utf8_lossy(&after.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(after, [winner.clone(), winner], "{label}");
    }
}

/// Why each request is refused: pinned ExifTool 13.59 writes (or deletes
/// from) both EXIF APP1 records, which oxidex cannot, or -- for a tag in
/// neither -- warns and leaves the file as it is.
#[test]
fn oracle_writes_every_exif_app1() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for (name, case) in all_cases() {
        let original = std::fs::read(fixture(name)).unwrap();
        let before = exif_app1_payloads(&original);
        assert_eq!(before.len(), 2, "{name}: fixture shape");
        let path = copy(dir.path(), name);
        let label = format!("{name} {} (oracle {})", case.arg, oracle.display());
        let out = oracle
            .command()
            .args(["-m", "-overwrite_original", case.arg])
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            out.status.success(),
            case.oracle != Oracle::Failed,
            "{label}: {out:?}"
        );
        let warned = stderr.contains("Warning: Multiple APP1 EXIF records");
        let after = std::fs::read(&path).unwrap();
        let blocks = exif_app1_payloads(&after);
        match case.oracle {
            Oracle::EveryBlock => {
                assert!(warned, "{label}: {stderr}");
                assert!(stdout.contains("1 image files updated"), "{label}");
                assert_eq!(blocks.len(), 2, "{label}");
                assert!(
                    blocks[0] != before[0] && blocks[1] != before[1],
                    "{label}: the oracle left a block as it was"
                );
            }
            Oracle::NoBlockLeft => {
                assert!(!warned, "{label}: {stderr}");
                assert!(stdout.contains("1 image files updated"), "{label}");
                assert!(blocks.is_empty(), "{label}: {} left", blocks.len());
            }
            Oracle::Unchanged => {
                assert!(warned, "{label}: {stderr}");
                assert!(stdout.contains("1 image files unchanged"), "{label}");
                assert_eq!(after, original, "{label}");
            }
            Oracle::NotWritten => {
                assert!(!warned, "{label}: {stderr}");
                assert!(stdout.contains("1 image files unchanged"), "{label}");
                assert_eq!(after, original, "{label}");
            }
            Oracle::Failed => {
                assert!(
                    stderr.contains("Error reading JpgFromRaw data in IFD2"),
                    "{label}: {stderr}"
                );
                assert!(stdout.contains("0 image files updated"), "{label}");
                assert_eq!(after, original, "{label}");
            }
        }
    }
    for name in FIXTURES {
        // The set the refusal is about, read back: one row per block.
        let path = copy(dir.path(), name);
        let status = oracle
            .command()
            .args([
                "-q",
                "-q",
                "-overwrite_original",
                "-ExifIFD:WhiteBalance#=1",
            ])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let out = oracle
            .command()
            .args(["-a", "-G1", "-s", "-n", "-ExifIFD:WhiteBalance"])
            .arg(&path)
            .output()
            .unwrap();
        let rows: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(
            rows,
            ["[ExifIFD] WhiteBalance : 1", "[ExifIFD] WhiteBalance : 1"],
            "{name}"
        );
    }
}

/// The committed fixtures are the documented construction over the pinned
/// sources (PROVENANCE.txt, make_fixtures.py).
#[test]
fn fixtures_rebuild_from_their_pinned_sources() {
    let (Some(nikon), Some(lsi), Some(canon), Some(leica), Some(dji)) = (
        fixtures::pinned_t_images_fixture_path("Nikon.jpg"),
        fixtures::pinned_combined_fixture_path("Nikon/NikonLS-50.jpg"),
        fixtures::pinned_t_images_fixture_path("Canon.jpg"),
        fixtures::pinned_combined_fixture_path("Leica/LeicaTL2.jpg"),
        fixtures::pinned_combined_fixture_path("DJI/DJI_XT2.jpg"),
    ) else {
        eprintln!(
            "skipping: pinned Nikon.jpg / Canon.jpg / Nikon/NikonLS-50.jpg / \
             Leica/LeicaTL2.jpg / DJI/DJI_XT2.jpg absent"
        );
        return;
    };
    let nikon = std::fs::read(nikon).unwrap();
    let lsi = std::fs::read(lsi).unwrap();
    let canon = std::fs::read(canon).unwrap();
    let leica = std::fs::read(leica).unwrap();
    let dji = std::fs::read(dji).unwrap();
    let first_exif_app1 = |data: &[u8]| -> (usize, usize) {
        let mut i = 2;
        loop {
            let length = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            if data[i + 1] == 0xE1 && data[i + 4..].starts_with(b"Exif\0\0") {
                return (i, i + 2 + length);
            }
            i += 2 + length;
        }
    };
    let insert_second = |host: &[u8], donor: &[u8]| -> Vec<u8> {
        let (_, end) = first_exif_app1(host);
        let (start, stop) = first_exif_app1(donor);
        [&host[..end], &donor[start..stop], &host[end..]].concat()
    };
    assert_eq!(
        std::fs::read(fixture("multi-app1-nikon-lsi1.jpg")).unwrap(),
        insert_second(&nikon, &lsi)
    );
    assert_eq!(
        std::fs::read(fixture("multi-app1-lsi1-nikon.jpg")).unwrap(),
        insert_second(&lsi, &nikon)
    );
    assert_eq!(
        std::fs::read(fixture(CANON)).unwrap(),
        insert_second(&canon, &nikon)
    );
    assert_eq!(
        std::fs::read(fixture(LEICA)).unwrap(),
        insert_second(&leica, &nikon)
    );
    assert_eq!(
        std::fs::read(fixture(DJI)).unwrap(),
        insert_second(&nikon, &dji)
    );
}

/// A copied key is an explicit write even when the visible winner already
/// equals the source. The oracle updates the other APP1 block; the public
/// copy API must refuse this request before editing either block.
#[test]
fn same_value_copy_is_an_explicit_multi_app1_write() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output");
        return;
    };
    let source =
        fixtures::pinned_t_images_fixture_path("Nikon.jpg").expect("pinned Nikon.jpg fixture");
    let dir = tempfile::tempdir().unwrap();
    let original = std::fs::read(fixture(FIXTURES[1])).unwrap();
    let ours = copy(dir.path(), FIXTURES[1]);
    let theirs = dir.path().join("oracle.jpg");
    std::fs::write(&theirs, &original).unwrap();
    let key = "IFD0:Make";
    assert_eq!(
        read_metadata(&source).unwrap().get(key),
        read_metadata(&ours).unwrap().get(key),
        "source and destination visible winners must already match"
    );
    let out = oracle
        .command()
        .args(["-overwrite_original", "-TagsFromFile"])
        .arg(&source)
        .arg("-IFD0:Make")
        .arg(&theirs)
        .output()
        .unwrap();
    assert!(out.status.success(), "oracle copy: {out:?}");
    assert_ne!(
        std::fs::read(&theirs).unwrap(),
        original,
        "the oracle updates the other block despite the equal visible winner"
    );
    let rows = oracle
        .command()
        .args(["-a", "-s3", "-IFD0:Make"])
        .arg(&theirs)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&rows.stdout)
            .lines()
            .collect::<Vec<_>>(),
        ["NIKON", "NIKON"]
    );
    let error = copy_metadata(&source, &ours, Some(&[key.to_string()]))
        .expect_err("a same-value copy must be refused on multi-APP1");
    assert_no_problems(
        refusal_problem(
            FIXTURES[1],
            &error.to_string(),
            Some(&format!("{error:?}")),
            key,
        )
        .into_iter()
        .collect(),
    );
    assert_eq!(std::fs::read(&ours).unwrap(), original);
}

/// Builder mutations retain named write requests instead of relying on a
/// difference between the visible map and the original file.
#[test]
fn builder_requests_keep_their_multi_app1_provenance() {
    for operation in ["set_tag", "insert", "copy", "write_to"] {
        let dir = tempfile::tempdir().unwrap();
        let original = std::fs::read(fixture(FIXTURES[1])).unwrap();
        let path = copy(dir.path(), FIXTURES[1]);
        let key = "IFD0:Make";
        let mut metadata = oxidex::Metadata::from_path(&path).unwrap();
        let value = metadata.get(key).unwrap().clone();
        let result = match operation {
            "set_tag" => metadata.set_tag(key, value).save(),
            "insert" => {
                metadata.insert(key, value);
                metadata.save()
            }
            "copy" => metadata
                .copy_to(&path)
                .unwrap()
                .with_tags(&[key])
                .unwrap()
                .execute(),
            "write_to" => metadata.set_tag(key, value).write_to(&path),
            _ => unreachable!(),
        };
        let error = result.expect_err(operation);
        assert!(
            matches!(error, oxidex::error::ExifToolError::TagsNotWritten { .. }),
            "{operation}: {error:?}"
        );
        assert!(error.to_string().contains(key), "{operation}: {error}");
        assert_eq!(std::fs::read(&path).unwrap(), original, "{operation}");
    }
}

/// The builder forwards a removal even when the default read map has no
/// row for the named EXIF entry, just as the direct API and C ABI do.
#[test]
fn builder_forwards_a_rowless_multi_app1_removal() {
    let dir = tempfile::tempdir().unwrap();
    let original = std::fs::read(fixture(DJI)).unwrap();
    let path = copy(dir.path(), DJI);
    let mut metadata = oxidex::Metadata::from_path(&path).unwrap();
    assert!(metadata.remove("ExifIFD:ApplicationNotes").is_none());
    let error = metadata
        .save()
        .expect_err("rowless removal must be refused");
    assert_no_problems(
        refusal_problem(
            DJI,
            &error.to_string(),
            Some(&format!("{error:?}")),
            "ExifIFD:ApplicationNotes",
        )
        .into_iter()
        .collect(),
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

/// A later C-ABI set replaces a pending single-tag deletion through its
/// family alias, matching the oracle's delete-then-set order.
#[test]
fn c_abi_alias_set_supersedes_a_named_removal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("single.jpg");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/sample_with_exif.jpg"),
        &path,
    )
    .unwrap();
    modify_tag(&path, "IFD0:Artist", TagValue::new_string("old")).unwrap();
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let remove = CString::new("EXIF:Artist").unwrap();
    let set = CString::new("IFD0:Artist").unwrap();
    let value = CString::new("new").unwrap();
    let handle = exiftool_create();
    assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
    assert_eq!(exiftool_remove_tag(handle, remove.as_ptr()), EXIFTOOL_OK);
    assert_eq!(
        exiftool_set_tag_string(handle, set.as_ptr(), value.as_ptr()),
        EXIFTOOL_OK
    );
    let code = exiftool_write_file(handle, c_path.as_ptr());
    let message = unsafe { CStr::from_ptr(exiftool_get_last_error()) }
        .to_string_lossy()
        .into_owned();
    exiftool_destroy(handle);
    assert_eq!(code, EXIFTOOL_OK, "{message}");
    assert_eq!(
        read_metadata(&path).unwrap().get("IFD0:Artist"),
        Some(&TagValue::new_string("new"))
    );
}

/// A successful group deletion remains deleted when the same C handle is
/// written again. Its retained map must not resurrect the removed rows.
#[test]
fn c_abi_repeated_group_removal_does_not_restore_exif() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("single.jpg");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/sample_with_exif.jpg"),
        &path,
    )
    .unwrap();
    assert!(read_metadata(&path).unwrap().contains_key("IFD0:Make"));
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let key = CString::new("EXIF:All").unwrap();
    let handle = exiftool_create();
    assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
    assert_eq!(exiftool_remove_tag(handle, key.as_ptr()), EXIFTOOL_OK);
    for pass in [1, 2] {
        let code = exiftool_write_file(handle, c_path.as_ptr());
        let message = unsafe { CStr::from_ptr(exiftool_get_last_error()) }
            .to_string_lossy()
            .into_owned();
        assert_eq!(code, EXIFTOOL_OK, "pass {pass}: {message}");
        assert!(
            !read_metadata(&path).unwrap().contains_key("IFD0:Make"),
            "pass {pass} restored a removed row"
        );
    }
    exiftool_destroy(handle);
}

/// Date editing uses an in-place EXIF writer, which must perform the same
/// multi-APP1 check as metadata-map writes before touching the first block.
#[test]
fn date_writes_refuse_multi_app1_and_preserve_every_byte() {
    use oxidex::core::date_shift::{ShiftOperation, shift_metadata_dates};
    for (offset, operation, arg) in [
        ("1:00:00", ShiftOperation::Add, "-IFD0:ModifyDate+=1:00:00"),
        (
            "2020:01:01 00:00:00",
            ShiftOperation::Set,
            "-IFD0:ModifyDate=2020:01:01 00:00:00",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let original = std::fs::read(fixture(FIXTURES[0])).unwrap();
        let path = copy(dir.path(), FIXTURES[0]);
        let error = shift_metadata_dates(&path, "IFD0:ModifyDate", offset, operation)
            .expect_err("in-place date writes must refuse multi-APP1");
        assert_no_problems(
            refusal_problem(
                FIXTURES[0],
                &error.to_string(),
                Some(&format!("{error:?}")),
                "IFD0:ModifyDate",
            )
            .into_iter()
            .collect(),
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .arg(arg)
            .arg(&path)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{arg}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(REASON),
            "{arg}: {out:?}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

/// A later group deletion cancels values authored earlier on the C handle.
#[test]
fn c_abi_group_removal_supersedes_an_earlier_set() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("single.jpg");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/sample_with_exif.jpg"),
        &path,
    )
    .unwrap();
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let all = CString::new("EXIF:All").unwrap();
    let artist = CString::new("IFD0:Artist").unwrap();
    let value = CString::new("new").unwrap();
    let handle = exiftool_create();
    assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
    assert_eq!(
        exiftool_set_tag_string(handle, artist.as_ptr(), value.as_ptr()),
        EXIFTOOL_OK
    );
    assert_eq!(exiftool_remove_tag(handle, all.as_ptr()), EXIFTOOL_OK);
    let code = exiftool_write_file(handle, c_path.as_ptr());
    let message = unsafe { CStr::from_ptr(exiftool_get_last_error()) }
        .to_string_lossy()
        .into_owned();
    exiftool_destroy(handle);
    assert_eq!(code, EXIFTOOL_OK, "{message}");
    assert!(
        !read_metadata(&path).unwrap().contains_key("IFD0:Artist"),
        "group deletion retained the earlier set"
    );
}

#[test]
fn builder_tiff_carrier_removal_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("noop.tif");
    let mut bytes = b"II\x2a\0\x08\0\0\0\x01\0".to_vec();
    bytes.extend_from_slice(&0x013bu16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(b"old\0");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();
    let mut metadata = oxidex::Metadata::from_path(&path).unwrap();
    metadata.remove("IFD0:All");
    metadata.save().expect("TIFF IFD0 deletion is a no-op");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn builder_legacy_alias_set_cancels_earlier_removal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("single.jpg");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/sample_with_exif.jpg"),
        &path,
    )
    .unwrap();
    modify_tag(&path, "ExifIFD:WhiteBalance", TagValue::new_integer(0)).unwrap();
    let mut metadata = oxidex::Metadata::from_path(&path).unwrap();
    metadata.remove("EXIF:WhiteBalance");
    metadata
        .set_tag("ExifIFD:WhiteBalance", 1i64)
        .save()
        .expect("later native set supersedes family deletion");
    assert_eq!(
        read_metadata(&path)
            .unwrap()
            .get("ExifIFD:WhiteBalance")
            .and_then(TagValue::as_string),
        Some("Manual")
    );
}

#[test]
fn builder_group_removal_cancels_bare_and_family_sets() {
    for (key, group, value, native) in [
        ("Artist", "EXIF:All", "new", "IFD0:Artist"),
        (
            "EXIF:DateTimeOriginal",
            "ExifIFD:All",
            "2020:01:01 00:00:00",
            "ExifIFD:DateTimeOriginal",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("single.jpg");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/sample_with_exif.jpg"),
            &path,
        )
        .unwrap();
        let mut metadata = oxidex::Metadata::from_path(&path)
            .unwrap()
            .set_tag(key, value);
        metadata.remove(group);
        metadata
            .save()
            .unwrap_or_else(|e| panic!("{key} then {group}: {e}"));
        assert!(
            !read_metadata(&path).unwrap().contains_key(native),
            "{key} survived {group}"
        );
    }
}

/// Carrying the reader's map contains no explicit EXIF mutation.
#[test]
fn untouched_builder_write_is_unchanged_on_multi_app1() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy(dir.path(), FIXTURES[0]);
    let original = std::fs::read(&path).unwrap();
    let metadata = oxidex::Metadata::from_path(&path).unwrap();
    assert_eq!(
        metadata.save().unwrap(),
        oxidex::core::write_transaction::WriteOutcome::Unchanged
    );
    assert_eq!(
        metadata.write_to(&path).unwrap(),
        oxidex::core::write_transaction::WriteOutcome::Unchanged
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}
