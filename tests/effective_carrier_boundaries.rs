//! Physical maker-note placement and post-clear conversion against pinned 13.59.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle::{self, Oracle};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
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

fn rows(oracle: &Oracle, name: &str, path: &Path) -> String {
    let out = native(oracle, &["-a", "-G1", "-s", &format!("-{name}")], path);
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn u16_at(bytes: &[u8], at: usize, be: bool) -> u16 {
    let word = bytes[at..at + 2].try_into().unwrap();
    if be {
        u16::from_be_bytes(word)
    } else {
        u16::from_le_bytes(word)
    }
}

fn u32_at(bytes: &[u8], at: usize, be: bool) -> u32 {
    let word = bytes[at..at + 4].try_into().unwrap();
    if be {
        u32::from_be_bytes(word)
    } else {
        u32::from_le_bytes(word)
    }
}

fn entry(bytes: &[u8], ifd: usize, id: u16, be: bool) -> usize {
    (0..u16_at(bytes, ifd, be) as usize)
        .map(|index| ifd + 2 + 12 * index)
        .find(|&at| u16_at(bytes, at, be) == id)
        .unwrap()
}

/// A source-derived Nikon Type 3 note, moved from its ExifIFD into IFD0.
/// An unrecognized Make hides its fields from OxiDex's reader while pinned
/// ExifTool still identifies the note by its Nikon signature and edits it.
fn direct_ifd0_nikon_jpeg(oracle: &Oracle, dir: &Path) -> Vec<u8> {
    let source = std::fs::read(fixtures::required_t_images_fixture_path("NikonD70.jpg")).unwrap();
    let marker = source
        .windows(6)
        .position(|part| part == b"Exif\0\0")
        .unwrap();
    let tiff = &source[marker + 6..];
    let be = &tiff[..2] == b"MM";
    let ifd0 = u32_at(tiff, 4, be) as usize;
    let exif = u32_at(tiff, entry(tiff, ifd0, 0x8769, be) + 8, be) as usize;
    let note_entry = entry(tiff, exif, 0x927c, be);
    let offset = u32_at(tiff, note_entry + 8, be) as usize;
    let count = u32_at(tiff, note_entry + 4, be) as usize;
    let note = &tiff[offset..offset + count];
    assert!(note.starts_with(b"Nikon\0\x02"));

    let make = b"ACME CAMERA\0";
    let model = b"NIKON D70\0";
    let first_value = 8 + 2 + 3 * 12 + 4;
    let mut direct = b"MM\0*\0\0\0\x08\0\x03".to_vec();
    let mut data = Vec::new();
    for (tag, ty, value) in [
        (0x010f_u16, 2_u16, make.as_slice()),
        (0x0110, 2, model.as_slice()),
        (0x927c, 7, note),
    ] {
        direct.extend_from_slice(&tag.to_be_bytes());
        direct.extend_from_slice(&ty.to_be_bytes());
        direct.extend_from_slice(&(value.len() as u32).to_be_bytes());
        direct.extend_from_slice(&((first_value + data.len()) as u32).to_be_bytes());
        data.extend_from_slice(value);
    }
    direct.extend_from_slice(&0_u32.to_be_bytes());
    direct.extend(data);

    let clean = dir.join("clean.jpg");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/jpeg/simple/synthetic_001.jpg"),
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
    assert_eq!(&jpeg[..2], b"\xff\xd8");
    let payload = [b"Exif\0\0".as_slice(), &direct].concat();
    let length = u16::try_from(payload.len() + 2).unwrap().to_be_bytes();
    [&jpeg[..2], b"\xff\xe1", &length, &payload, &jpeg[2..]].concat()
}

#[test]
fn direct_ifd0_note_survives_exififd_clear_and_never_silently_loses_hidden_write() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let carrier = direct_ifd0_nikon_jpeg(oracle, dir.path());
    let source = dir.path().join("source.jpg");
    std::fs::write(&source, &carrier).unwrap();
    assert!(rows(oracle, "WhiteBalance", &source).contains("[Nikon]"));
    let ox_rows = oxidex(&["-a", "-G1", "-s", "-WhiteBalance"], &source);
    assert!(ox_rows.status.success());
    assert!(!String::from_utf8_lossy(&ox_rows.stdout).contains("WhiteBalance"));

    // Give ExifIFD:All a real directory to remove. The note is in IFD0,
    // so pinned ExifTool still writes its hidden Nikon copy afterwards.
    let setup = native(
        oracle,
        &["-overwrite_original", "-ExifIFD:ColorSpace#=1"],
        &source,
    );
    assert!(
        setup.status.success(),
        "{}",
        String::from_utf8_lossy(&setup.stderr)
    );
    let original = std::fs::read(&source).unwrap();
    for args in [
        vec!["-WhiteBalance#=1"],
        vec!["-ExifIFD:All=", "-WhiteBalance#=1"],
        vec!["-ExifIFD:All=", "-Nikon:WhiteBalance#=1"],
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
        assert!(
            rows(oracle, "WhiteBalance", &native_path)
                .contains("[Nikon]         WhiteBalance                    : 1")
        );
        #[cfg(unix)]
        let inode = std::fs::metadata(&ox_path).unwrap().ino();
        let out = oxidex(&args, &ox_path);
        assert!(
            !out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(error.contains("WhiteBalance"), "{args:?}: {error}");
        if args.iter().any(|arg| arg.starts_with("-WhiteBalance")) {
            assert!(error.contains("maker note"), "{args:?}: {error}");
        }
        assert_eq!(std::fs::read(&ox_path).unwrap(), original, "{args:?}");
        #[cfg(unix)]
        assert_eq!(std::fs::metadata(&ox_path).unwrap().ino(), inode);
    }
}

#[test]
fn clear_all_classifies_bare_set_against_cleared_carrier() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let source = fixtures::required_t_images_fixture_path("NikonD70.jpg");
    for args in [
        vec!["-all=", "-ColorSpace=BT.2100"],
        vec!["-all=", "-ColorSpace=BT.2100", "-IFD0:Artist=survivor"],
    ] {
        let native_path = dir.path().join("native.jpg");
        let ox_path = dir.path().join("oxidex.jpg");
        std::fs::copy(&source, &native_path).unwrap();
        std::fs::copy(&source, &ox_path).unwrap();
        let native_out = native(
            oracle,
            &[&["-overwrite_original"][..], args.as_slice()].concat(),
            &native_path,
        );
        let ox_out = oxidex(&args, &ox_path);
        assert_eq!(
            ox_out.status.code(),
            native_out.status.code(),
            "{args:?}: {}",
            String::from_utf8_lossy(&ox_out.stderr)
        );
        for out in [&native_out, &ox_out] {
            assert!(
                String::from_utf8_lossy(&out.stderr)
                    .contains("Can't convert ExifIFD:ColorSpace (not in PrintConv)"),
                "{args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        assert_eq!(
            rows(oracle, "ColorSpace", &ox_path),
            rows(oracle, "ColorSpace", &native_path)
        );
        assert_eq!(
            rows(oracle, "Artist", &ox_path),
            rows(oracle, "Artist", &native_path)
        );
        assert_ne!(
            std::fs::read(&ox_path).unwrap(),
            std::fs::read(&source).unwrap()
        );
    }
}
