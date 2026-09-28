//! EXIF-table MakerNotes and an unrelated GPS 0x927c have distinct semantics.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle::{self, Oracle};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Command, Output};

fn native(oracle: &Oracle, args: &[&str], path: &Path) -> Output {
    oracle.command().args(args).arg(path).output().unwrap()
}

fn oxidex(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .unwrap()
}

fn rows(oracle: &Oracle, path: &Path) -> String {
    let out = native(oracle, &["-a", "-G1", "-s", "-WhiteBalance"], path);
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn be_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn be_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn source_nikon_note() -> Vec<u8> {
    let source = std::fs::read(fixtures::required_t_images_fixture_path("NikonD70.jpg")).unwrap();
    let tiff = &source[source.windows(6).position(|w| w == b"Exif\0\0").unwrap() + 6..];
    assert_eq!(&tiff[..2], b"MM");
    let ifd0 = be_u32(tiff, 4) as usize;
    let exif_record = (0..be_u16(tiff, ifd0) as usize)
        .map(|i| ifd0 + 2 + 12 * i)
        .find(|&at| be_u16(tiff, at) == 0x8769)
        .unwrap();
    let exif = be_u32(tiff, exif_record + 8) as usize;
    let record = (0..be_u16(tiff, exif) as usize)
        .map(|i| exif + 2 + 12 * i)
        .find(|&at| be_u16(tiff, at) == 0x927c)
        .unwrap();
    let at = be_u32(tiff, record + 8) as usize;
    let len = be_u32(tiff, record + 4) as usize;
    let note = tiff[at..at + len].to_vec();
    assert!(note.starts_with(b"Nikon\0\x02"));
    note
}

enum Value {
    Bytes(Vec<u8>),
    Pointer(&'static str),
}

struct Directory {
    name: &'static str,
    entries: Vec<(u16, u16, Value)>,
    next: Option<&'static str>,
}

/// These directories use Exif::Main in pinned 13.59 except GPS::Main.
/// ACME hides the Nikon row from OxiDex; native still decodes the signature.
fn carrier(oracle: &Oracle, dir: &Path, placement: &str, note: &[u8]) -> Vec<u8> {
    let mut dirs = vec![Directory {
        name: "IFD0",
        entries: vec![
            (0x010f, 2, Value::Bytes(b"ACME CAMERA\0".to_vec())),
            (0x0110, 2, Value::Bytes(b"NIKON D70\0".to_vec())),
        ],
        next: None,
    }];
    if matches!(placement, "ExifIFD" | "Interop" | "GPS+ExifIFD") {
        dirs[0].entries.push((0x8769, 4, Value::Pointer("ExifIFD")));
        dirs.push(Directory {
            name: "ExifIFD",
            entries: Vec::new(),
            next: None,
        });
    }
    if matches!(placement, "GPS" | "GPS-duplicate" | "GPS+ExifIFD") {
        dirs[0].entries.push((0x8825, 4, Value::Pointer("GPS")));
        dirs.push(Directory {
            name: "GPS",
            entries: Vec::new(),
            next: None,
        });
    }
    if placement == "Interop" {
        dirs.iter_mut()
            .find(|d| d.name == "ExifIFD")
            .unwrap()
            .entries
            .push((0xa005, 4, Value::Pointer("Interop")));
        dirs.push(Directory {
            name: "Interop",
            entries: Vec::new(),
            next: None,
        });
    }
    if matches!(placement, "IFD1" | "IFD2" | "IFD0+IFD1") {
        dirs[0].next = Some("IFD1");
        dirs.push(Directory {
            name: "IFD1",
            entries: Vec::new(),
            next: (placement == "IFD2").then_some("IFD2"),
        });
        if placement == "IFD2" {
            dirs.push(Directory {
                name: "IFD2",
                entries: Vec::new(),
                next: None,
            });
        }
    }
    if placement == "SubIFD" {
        dirs[0].entries.push((0x014a, 4, Value::Pointer("SubIFD")));
        dirs.push(Directory {
            name: "SubIFD",
            entries: Vec::new(),
            next: None,
        });
    }
    for target in match placement {
        "GPS+ExifIFD" => vec!["GPS", "ExifIFD"],
        "GPS-duplicate" => vec!["GPS", "GPS"],
        "IFD0+IFD1" => vec!["IFD0", "IFD1"],
        _ => vec![placement],
    } {
        dirs.iter_mut()
            .find(|d| d.name == target)
            .unwrap()
            .entries
            .push((0x927c, 7, Value::Bytes(note.to_vec())));
    }
    let mut offsets = std::collections::BTreeMap::new();
    let mut end = 8;
    for d in &dirs {
        offsets.insert(d.name, end);
        end += 2 + 12 * d.entries.len() + 4;
    }
    let mut tiff = b"MM\0*\0\0\0\x08".to_vec();
    let mut values = Vec::new();
    for d in &dirs {
        tiff.extend_from_slice(&(d.entries.len() as u16).to_be_bytes());
        for (id, ty, value) in &d.entries {
            tiff.extend_from_slice(&id.to_be_bytes());
            tiff.extend_from_slice(&ty.to_be_bytes());
            match value {
                Value::Pointer(target) => {
                    tiff.extend_from_slice(&1_u32.to_be_bytes());
                    tiff.extend_from_slice(&(*offsets.get(target).unwrap() as u32).to_be_bytes());
                }
                Value::Bytes(bytes) => {
                    tiff.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                    if bytes.len() <= 4 {
                        tiff.extend_from_slice(bytes);
                        tiff.resize(tiff.len() + 4 - bytes.len(), 0);
                    } else {
                        tiff.extend_from_slice(&((end + values.len()) as u32).to_be_bytes());
                        values.extend_from_slice(bytes);
                    }
                }
            }
        }
        tiff.extend_from_slice(
            &(d.next.map_or(0, |n| *offsets.get(n).unwrap()) as u32).to_be_bytes(),
        );
    }
    tiff.extend(values);
    let clean = dir.join("clean.jpg");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg/simple/synthetic_001.jpg"),
        &clean,
    )
    .unwrap();
    let clear = native(oracle, &["-overwrite_original", "-all="], &clean);
    assert!(
        clear.status.success(),
        "{}",
        String::from_utf8_lossy(&clear.stderr)
    );
    let jpeg = std::fs::read(clean).unwrap();
    let payload = [b"Exif\0\0".as_slice(), &tiff].concat();
    let len = u16::try_from(payload.len() + 2).unwrap().to_be_bytes();
    [&jpeg[..2], b"\xff\xe1", &len, &payload, &jpeg[2..]].concat()
}

fn copies_of(bytes: &[u8], note: &[u8]) -> usize {
    bytes.windows(note.len()).filter(|w| *w == note).count()
}

#[test]
fn gps_927c_is_opaque_and_survives_bare_and_group_clear_writes() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let note = source_nikon_note();
    let original = carrier(oracle, dir.path(), "GPS", &note);
    let original_path = dir.path().join("original.jpg");
    std::fs::write(&original_path, &original).unwrap();
    assert!(!rows(oracle, &original_path).contains("WhiteBalance"));
    for args in [
        vec!["-WhiteBalance#=1"],
        vec!["-ExifIFD:All=", "-WhiteBalance#=1"],
        vec!["-MakerNotes:All=", "-WhiteBalance#=1"],
    ] {
        for (tool, path) in [(true, "native.jpg"), (false, "oxidex.jpg")] {
            let path = dir.path().join(path);
            std::fs::write(&path, &original).unwrap();
            let out = if tool {
                native(
                    oracle,
                    &[&["-overwrite_original"][..], args.as_slice()].concat(),
                    &path,
                )
            } else {
                oxidex(&args, &path)
            };
            assert!(
                out.status.success(),
                "{args:?} {tool}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                copies_of(&std::fs::read(&path).unwrap(), &note),
                1,
                "{args:?} {tool}"
            );
            let result = rows(oracle, &path);
            assert!(
                result.contains("[ExifIFD]       WhiteBalance                    : Manual"),
                "{args:?} {tool}: {result}"
            );
            assert!(!result.contains("[Nikon]"), "{args:?} {tool}: {result}");
        }
    }
}

#[test]
fn gps_and_exififd_927c_remain_independent() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let note = source_nikon_note();
    let original = carrier(oracle, dir.path(), "GPS+ExifIFD", &note);
    let path = dir.path().join("original.jpg");
    std::fs::write(&path, &original).unwrap();
    assert_eq!(copies_of(&original, &note), 2);
    assert!(rows(oracle, &path).contains("[Nikon]"));
    for args in [
        vec!["-ExifIFD:All=", "-WhiteBalance#=1"],
        vec!["-MakerNotes:All=", "-WhiteBalance#=1"],
    ] {
        for (tool, name) in [(true, "native.jpg"), (false, "oxidex.jpg")] {
            let path = dir.path().join(name);
            std::fs::write(&path, &original).unwrap();
            let out = if tool {
                native(
                    oracle,
                    &[&["-overwrite_original"][..], args.as_slice()].concat(),
                    &path,
                )
            } else {
                oxidex(&args, &path)
            };
            assert!(
                out.status.success(),
                "{args:?} {tool}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                copies_of(&std::fs::read(&path).unwrap(), &note),
                1,
                "{args:?} {tool}"
            );
            let result = rows(oracle, &path);
            assert!(
                result.contains("[ExifIFD]       WhiteBalance                    : Manual"),
                "{args:?} {tool}: {result}"
            );
            assert!(!result.contains("[Nikon]"), "{args:?} {tool}: {result}");
        }
    }
}

#[test]
fn duplicate_opaque_gps_927c_values_are_kept_independently() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let note = source_nikon_note();
    let original = carrier(oracle, dir.path(), "GPS-duplicate", &note);
    assert_eq!(copies_of(&original, &note), 2);
    for (args, expected_copies) in [
        (vec!["-WhiteBalance#=1"], 2),
        (vec!["-ExifIFD:All=", "-WhiteBalance#=1"], 2),
        (vec!["-MakerNotes:All=", "-WhiteBalance#=1"], 2),
        (vec!["-all=", "-WhiteBalance#=1"], 0),
    ] {
        for (tool, name) in [(true, "native.jpg"), (false, "oxidex.jpg")] {
            let path = dir.path().join(name);
            std::fs::write(&path, &original).unwrap();
            let out = if tool {
                native(
                    oracle,
                    &[&["-overwrite_original"][..], args.as_slice()].concat(),
                    &path,
                )
            } else {
                oxidex(&args, &path)
            };
            assert!(
                out.status.success(),
                "{args:?} {tool}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                copies_of(&std::fs::read(&path).unwrap(), &note),
                expected_copies,
                "{args:?} {tool}"
            );
            let result = rows(oracle, &path);
            assert!(
                result.contains("[ExifIFD]       WhiteBalance                    : Manual"),
                "{args:?} {tool}: {result}"
            );
            assert!(!result.contains("[Nikon]"), "{args:?} {tool}: {result}");
        }
    }
}

#[test]
fn real_notes_in_other_directories_keep_atomic_backstops() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let note = source_nikon_note();
    for placement in ["Interop", "IFD1", "IFD2", "SubIFD"] {
        let original = carrier(oracle, dir.path(), placement, &note);
        let path = dir.path().join("original.jpg");
        std::fs::write(&path, &original).unwrap();
        assert!(rows(oracle, &path).contains("[Nikon]"), "{placement}");
        for args in [
            vec!["-WhiteBalance#=1"],
            vec!["-ExifIFD:All=", "-WhiteBalance#=1"],
        ] {
            let native_path = dir.path().join("native.jpg");
            let ox_path = dir.path().join("oxidex.jpg");
            std::fs::write(&native_path, &original).unwrap();
            std::fs::write(&ox_path, &original).unwrap();
            let native_out = native(
                oracle,
                &[&["-overwrite_original"][..], args.as_slice()].concat(),
                &native_path,
            );
            assert!(
                native_out.status.success(),
                "{placement} {args:?}: {}",
                String::from_utf8_lossy(&native_out.stderr)
            );
            let native_rows = rows(oracle, &native_path);
            assert_eq!(
                native_rows.contains("[Nikon]"),
                placement != "Interop" || args.len() == 1,
                "{placement} {args:?}: {native_rows}"
            );
            #[cfg(unix)]
            let inode = std::fs::metadata(&ox_path).unwrap().ino();
            let ox_out = oxidex(&args, &ox_path);
            if placement == "Interop" && args.len() == 2 {
                assert!(
                    ox_out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&ox_out.stderr)
                );
                assert!(!rows(oracle, &ox_path).contains("[Nikon]"));
            } else {
                assert!(!ox_out.status.success(), "{placement} {args:?}");
                assert_eq!(
                    std::fs::read(&ox_path).unwrap(),
                    original,
                    "{placement} {args:?}"
                );
                #[cfg(unix)]
                assert_eq!(std::fs::metadata(&ox_path).unwrap().ino(), inode);
            }
        }
    }
}

#[test]
fn ifd1_clear_removes_its_direct_note_before_bare_resolution() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let note = source_nikon_note();
    let original = carrier(oracle, dir.path(), "IFD1", &note);
    for args in [
        vec!["-IFD1:All=", "-WhiteBalance#=1"],
        vec!["-ExifIFD:All=", "-IFD1:All=", "-WhiteBalance#=1"],
        vec!["-IFD1:All=", "-ExifIFD:All=", "-WhiteBalance#=1"],
    ] {
        for (tool, name) in [(true, "native.jpg"), (false, "oxidex.jpg")] {
            let path = dir.path().join(name);
            std::fs::write(&path, &original).unwrap();
            let out = if tool {
                native(
                    oracle,
                    &[&["-overwrite_original"][..], args.as_slice()].concat(),
                    &path,
                )
            } else {
                oxidex(&args, &path)
            };
            assert!(
                out.status.success(),
                "{args:?} {tool}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                copies_of(&std::fs::read(&path).unwrap(), &note),
                0,
                "{args:?} {tool}"
            );
            let result = rows(oracle, &path);
            assert!(
                result.contains("[ExifIFD]       WhiteBalance                    : Manual"),
                "{args:?} {tool}: {result}"
            );
            assert!(!result.contains("[Nikon]"), "{args:?} {tool}: {result}");
        }
    }
}

#[test]
fn ifd1_clear_does_not_hide_a_surviving_ifd0_note() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let note = source_nikon_note();
    let original = carrier(oracle, dir.path(), "IFD0+IFD1", &note);
    assert_eq!(copies_of(&original, &note), 2);
    for args in [
        vec!["-IFD1:All=", "-WhiteBalance#=1"],
        vec!["-ExifIFD:All=", "-IFD1:All=", "-WhiteBalance#=1"],
    ] {
        let native_path = dir.path().join("native.jpg");
        let ox_path = dir.path().join("oxidex.jpg");
        std::fs::write(&native_path, &original).unwrap();
        std::fs::write(&ox_path, &original).unwrap();
        let native_out = native(
            oracle,
            &[&["-overwrite_original"][..], args.as_slice()].concat(),
            &native_path,
        );
        assert!(
            native_out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&native_out.stderr)
        );
        let native_rows = rows(oracle, &native_path);
        assert!(native_rows.contains("[Nikon]"), "{args:?}: {native_rows}");
        #[cfg(unix)]
        let inode = std::fs::metadata(&ox_path).unwrap().ino();
        let ox_out = oxidex(&args, &ox_path);
        assert!(!ox_out.status.success(), "{args:?}");
        assert_eq!(std::fs::read(&ox_path).unwrap(), original, "{args:?}");
        #[cfg(unix)]
        assert_eq!(std::fs::metadata(&ox_path).unwrap().ino(), inode);
    }
}
