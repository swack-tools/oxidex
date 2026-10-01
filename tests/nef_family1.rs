//! ExifTool 13.59 `t/images/Nikon.nef`: the TIFF directories are distinct
//! occurrences even when family 0 and the default values already agree.

#[path = "common/fixtures.rs"]
mod fixtures;

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn linked_three_ifd_nef(big_endian: bool) -> Vec<u8> {
    let mut data = vec![0u8; 1024];
    let u16_bytes = |value: u16| {
        if big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    };
    let u32_bytes = |value: u32| {
        if big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    };
    data[..2].copy_from_slice(if big_endian { b"MM" } else { b"II" });
    data[2..4].copy_from_slice(&u16_bytes(42));
    data[4..8].copy_from_slice(&u32_bytes(8));
    for (index, (offset, width, next)) in [(8, 100, 128), (128, 200, 256), (256, 300, 0)]
        .into_iter()
        .enumerate()
    {
        let mut entries = vec![
            (0x00fe, 4, 1, 1),
            (0x0100, 4, 1, width),
            (0x0101, 4, 1, width / 2),
        ];
        if index == 0 {
            entries.push((0x010f, 2, 6, 512));
        }
        data[offset..offset + 2].copy_from_slice(&u16_bytes(entries.len() as u16));
        for (entry_index, (tag, field_type, count, value)) in entries.into_iter().enumerate() {
            let at = offset + 2 + entry_index * 12;
            data[at..at + 2].copy_from_slice(&u16_bytes(tag));
            data[at + 2..at + 4].copy_from_slice(&u16_bytes(field_type));
            data[at + 4..at + 8].copy_from_slice(&u32_bytes(count));
            data[at + 8..at + 12].copy_from_slice(&u32_bytes(value));
        }
        let next_at = offset + 2 + (3 + usize::from(index == 0)) * 12;
        data[next_at..next_at + 4].copy_from_slice(&u32_bytes(next));
    }
    data[512..518].copy_from_slice(b"Nikon\0");
    data
}

fn selected(file: &Path, mode: &[&str]) -> Value {
    selected_tags(
        file,
        mode,
        &[
            "-ImageWidth",
            "-ImageHeight",
            "-SubfileType",
            "-TIFF-EPStandardID",
        ],
    )
}

fn selected_tags(file: &Path, mode: &[&str], tags: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-j")
        .args(mode)
        .args(tags)
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

#[test]
fn linked_nef_ifd2_keeps_physical_group_in_duplicate_views() {
    for big_endian in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("linked.nef");
        std::fs::write(&file, linked_three_ifd_nef(big_endian)).unwrap();
        let expected_g1 = json!({
            "IFD0:ImageWidth": 100, "IFD1:ImageWidth": 200, "IFD2:ImageWidth": 300,
            "IFD0:ImageHeight": 50, "IFD1:ImageHeight": 100, "IFD2:ImageHeight": 150,
        });
        assert_eq!(
            selected_tags(&file, &["-G1", "-a"], &["-ImageWidth", "-ImageHeight"]),
            expected_g1
        );
        let expected_g01 = json!({
            "EXIF:IFD0:ImageWidth": 100, "EXIF:IFD1:ImageWidth": 200,
            "EXIF:IFD2:ImageWidth": 300, "EXIF:IFD0:ImageHeight": 50,
            "EXIF:IFD1:ImageHeight": 100, "EXIF:IFD2:ImageHeight": 150,
        });
        assert_eq!(
            selected_tags(
                &file,
                &["-G0:1", "-a", "--no-print-conv"],
                &["-ImageWidth", "-ImageHeight"]
            ),
            expected_g01
        );
    }
}
