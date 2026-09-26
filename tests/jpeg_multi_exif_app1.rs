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
//! Fixtures: tests/fixtures/jpeg/multi_exif_app1 (PROVENANCE.txt);
//! `fixtures_rebuild_from_their_pinned_sources` re-derives them.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{modify_tag, read_metadata, remove_tag, write_metadata};
use oxidex::core::tag_value::TagValue;
use oxidex::exiftool_oracle;
use oxidex::ffi::{
    exiftool_create, exiftool_destroy, exiftool_get_last_error, exiftool_read_file,
    exiftool_remove_tag, exiftool_set_tag_string, exiftool_write_file,
};
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURES: [&str; 2] = ["multi-app1-nikon-lsi1.jpg", "multi-app1-lsi1-nikon.jpg"];

/// `oxidex::ffi::error::EXIFTOOL_ERR_TAG_NOT_WRITTEN`.
const ERR_TAG_NOT_WRITTEN: i32 = 7;

const REASON: &str =
    "the file has 2 EXIF APP1 blocks; ExifTool writes every one, oxidex writes one";

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
            "ExifIFD:WhiteBalance",
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
            "IFD0:Software",
            Remove("Software"),
            "IFD0:Software",
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

/// `message` is the multi-block refusal of `key`, and `debug` shows the
/// typed `TagsNotWritten` variant.
fn assert_refusal(label: &str, message: &str, debug: Option<&str>, key: &str) {
    let expected = format!("Cannot write tag '{key}': {REASON}");
    assert!(
        message.contains(&expected),
        "{label}: expected the refusal {expected:?}, got {message:?}"
    );
    if let Some(debug) = debug {
        assert!(
            debug.starts_with("TagsNotWritten"),
            "{label}: expected a TagsNotWritten error, got {debug}"
        );
    }
}

#[test]
fn library_refuses_every_exif_write_and_leaves_the_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    for name in FIXTURES {
        let original = std::fs::read(fixture(name)).unwrap();
        for case in cases() {
            let path = copy(dir.path(), name);
            let label = format!("{name} {:?}", case.lib);
            let result = match case.lib {
                Lib::Set(key, value) => modify_tag(&path, key, value()),
                Lib::Remove(key) => remove_tag(&path, key),
            };
            let err = result.expect_err(&format!("{label}: expected a refusal"));
            assert_refusal(
                &label,
                &err.to_string(),
                Some(&format!("{err:?}")),
                case.lib_names,
            );
            assert_eq!(
                std::fs::read(&path).unwrap(),
                original,
                "{label}: the file changed"
            );
        }
    }
}

#[test]
fn a_read_map_write_names_each_changed_exif_key() {
    let dir = tempfile::tempdir().unwrap();
    for name in FIXTURES {
        let original = std::fs::read(fixture(name)).unwrap();
        let path = copy(dir.path(), name);
        let mut map = read_metadata(&path).unwrap();
        // The unchanged read map is a no-op, as on any file.
        write_metadata(&path, &map).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original, "{name}: no-op");
        map.insert("IFD0:Artist", TagValue::new_string("x"));
        map.remove("IFD0:Software");
        let err = write_metadata(&path, &map).unwrap_err();
        let message = err.to_string();
        assert_refusal(name, &message, Some(&format!("{err:?}")), "IFD0:Artist");
        assert_refusal(name, &message, None, "IFD0:Software");
        assert_eq!(std::fs::read(&path).unwrap(), original, "{name}: changed");
    }
}

#[test]
fn cli_refuses_every_exif_write_and_leaves_the_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    for name in FIXTURES {
        let original = std::fs::read(fixture(name)).unwrap();
        for case in cases() {
            let path = copy(dir.path(), name);
            let label = format!("{name} {}", case.arg);
            let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
                .arg(case.arg)
                .arg(&path)
                .output()
                .unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                !out.status.success(),
                "{label}: exit {:?}, stdout {stdout:?}",
                out.status
            );
            assert!(
                !stdout.contains("image files updated"),
                "{label}: reported an update: {stdout:?}"
            );
            assert_refusal(&label, &stderr, None, case.cli_names);
            assert_eq!(
                std::fs::read(&path).unwrap(),
                original,
                "{label}: the file changed"
            );
        }
    }
}

#[test]
fn c_abi_refuses_an_exif_set_and_a_deletion() {
    let dir = tempfile::tempdir().unwrap();
    for name in FIXTURES {
        let original = std::fs::read(fixture(name)).unwrap();
        for (label, key, value) in [
            ("set", "IFD0:Artist", Some("x")),
            ("remove", "IFD0:Software", None),
        ] {
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
                code, ERR_TAG_NOT_WRITTEN,
                "{name} {label}: code {code}, {message}"
            );
            assert_refusal(&format!("{name} {label}"), &message, None, key);
            assert_eq!(std::fs::read(&path).unwrap(), original, "{name} {label}");
        }
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
    for name in FIXTURES {
        let original = std::fs::read(fixture(name)).unwrap();
        let before = exif_app1_payloads(&original);
        assert_eq!(before.len(), 2, "{name}: fixture shape");
        for case in cases() {
            let path = copy(dir.path(), name);
            let label = format!("{name} {} (oracle {})", case.arg, oracle.display());
            let out = oracle
                .command()
                .args(["-m", "-overwrite_original", case.arg])
                .arg(&path)
                .output()
                .unwrap();
            assert!(out.status.success(), "{label}: {out:?}");
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
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
            }
        }
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
    let (Some(nikon), Some(lsi)) = (
        fixtures::pinned_t_images_fixture_path("Nikon.jpg"),
        fixtures::pinned_combined_fixture_path("Nikon/NikonLS-50.jpg"),
    ) else {
        eprintln!("skipping: pinned Nikon.jpg / Nikon/NikonLS-50.jpg absent");
        return;
    };
    let nikon = std::fs::read(nikon).unwrap();
    let lsi = std::fs::read(lsi).unwrap();
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
}
