//! Native-graded regressions for the #997 review batch. Each source is a
//! pinned ExifTool 13.59 t/images fixture; the two writers receive identical
//! bytes and their physical EXIF rows are read by the same pinned oracle.
#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle;
use std::path::Path;
use std::process::Command;

fn run(command: &mut Command) -> std::process::Output {
    command.output().unwrap()
}

fn oracle_rows(oracle: &exiftool_oracle::Oracle, path: &Path, tag: &str) -> String {
    let out = run(oracle
        .command()
        .args(["-a", "-G1", "-s", "-n", tag])
        .arg(path));
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn compare(
    oracle: &exiftool_oracle::Oracle,
    source: &Path,
    suffix: &str,
    seed: &[&str],
    args: &[&str],
    tag: &str,
) {
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().join(format!("native.{suffix}"));
    let ours = dir.path().join(format!("ours.{suffix}"));
    std::fs::copy(source, &native).unwrap();
    if !seed.is_empty() {
        let out = run(oracle
            .command()
            .arg("-overwrite_original")
            .args(seed)
            .arg(&native));
        assert!(out.status.success(), "seed {seed:?}: {out:?}");
    }
    std::fs::copy(&native, &ours).unwrap();
    let native_out = run(oracle
        .command()
        .arg("-overwrite_original")
        .args(args)
        .arg(&native));
    assert!(
        native_out.status.success(),
        "native {args:?}: {native_out:?}"
    );
    let ours_out = run(Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(&ours));
    assert!(ours_out.status.success(), "ours {args:?}: {ours_out:?}");
    assert_eq!(
        oracle_rows(oracle, &ours, tag),
        oracle_rows(oracle, &native, tag),
        "{suffix} {args:?}"
    );
}

#[test]
fn tiff_raw_cross_directory_sets_and_shift() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for (name, suffix) in [("DNG.dng", "dng"), ("CanonRaw.cr2", "cr2")] {
        let Some(source) = fixtures::pinned_t_images_fixture_path(name) else {
            return;
        };
        let seed = [
            "-IFD0:CreateDate=",
            "-ExifIFD:CreateDate=2020:01:02 03:04:05",
        ];
        compare(
            oracle,
            &source,
            suffix,
            &seed,
            &["-IFD0:CreateDate=2022:03:04 05:06:07"],
            "-CreateDate",
        );
        // DNG's more complex directory graph is deliberately refused by the
        // in-place scanner; CR2 is the supported classic-TIFF shift control.
        if suffix == "cr2" {
            compare(
                oracle,
                &source,
                suffix,
                &seed,
                &["-IFD0:CreateDate+=0:0:1 0:0:0"],
                "-CreateDate",
            );
        }
    }
}

#[test]
fn tiff_raw_absolute_family_date_set_moves_the_other_copy() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let Some(source) = fixtures::pinned_t_images_fixture_path("CanonRaw.cr2") else {
        return;
    };
    let seed = [
        "-ExifIFD:CreateDate=",
        "-IFD0:CreateDate=2020:01:02 03:04:05",
    ];
    for arg in [
        "-CreateDate=2022:03:04 05:06:07",
        "-EXIF:CreateDate=2022:03:04 05:06:07",
    ] {
        compare(oracle, &source, "cr2", &seed, &[arg], "-CreateDate");
    }
}

#[test]
fn family_delete_and_explicit_destination_preserve_native_ordering() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for (name, suffix) in [("ExifTool.tif", "tif"), ("Writer.jpg", "jpg")] {
        let Some(source) = fixtures::pinned_t_images_fixture_path(name) else {
            return;
        };
        let seed = ["-IFD0:FocalLength=25", "-ExifIFD:FocalLength=35"];
        for family in ["-FocalLength=", "-EXIF:FocalLength="] {
            for set in ["-IFD0:FocalLength=50", "-ExifIFD:FocalLength=50"] {
                compare(
                    oracle,
                    &source,
                    suffix,
                    &seed,
                    &[family, set],
                    "-FocalLength",
                );
                compare(
                    oracle,
                    &source,
                    suffix,
                    &seed,
                    &[set, family],
                    "-FocalLength",
                );
            }
        }
    }
}

fn exif_ids(path: &Path) -> Vec<u16> {
    let bytes = std::fs::read(path).unwrap();
    let little = &bytes[..2] == b"II";
    assert!(little || &bytes[..2] == b"MM");
    let u16_at = |at: usize| {
        let raw: [u8; 2] = bytes[at..at + 2].try_into().unwrap();
        if little {
            u16::from_le_bytes(raw)
        } else {
            u16::from_be_bytes(raw)
        }
    };
    let u32_at = |at: usize| {
        let raw: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        (if little {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        }) as usize
    };
    let root = u32_at(4);
    let pointer = (0..u16_at(root) as usize)
        .map(|i| root + 2 + i * 12)
        .find(|at| u16_at(*at) == 0x8769)
        .map(|at| u32_at(at + 8))
        .unwrap();
    (0..u16_at(pointer) as usize)
        .map(|i| u16_at(pointer + 2 + i * 12))
        .collect()
}

#[test]
fn first_entry_in_existing_empty_exif_ifd_seeds_mandatory_tags() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let Some(source) = fixtures::pinned_t_images_fixture_path("ExifTool.tif") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let seeded = dir.path().join("seeded.tif");
    std::fs::copy(source, &seeded).unwrap();
    let out = run(oracle
        .command()
        .args([
            "-overwrite_original",
            "-ExifIFD:CreateDate=2020:01:02 03:04:05",
        ])
        .arg(&seeded));
    assert!(out.status.success(), "{out:?}");
    let mut bytes = std::fs::read(&seeded).unwrap();
    let little = &bytes[..2] == b"II";
    assert!(little || &bytes[..2] == b"MM");
    let u16_at = |at: usize| {
        let raw: [u8; 2] = bytes[at..at + 2].try_into().unwrap();
        if little {
            u16::from_le_bytes(raw)
        } else {
            u16::from_be_bytes(raw)
        }
    };
    let u32_at = |at: usize| {
        let raw: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        (if little {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        }) as usize
    };
    let root = u32_at(4);
    let count = u16_at(root) as usize;
    let pointer = (0..count)
        .map(|i| root + 2 + i * 12)
        .find(|at| u16_at(*at) == 0x8769)
        .map(|at| u32_at(at + 8))
        .unwrap();
    bytes[pointer..pointer + 6].fill(0);
    let native = dir.path().join("native.tif");
    let ours = dir.path().join("ours.tif");
    std::fs::write(&native, &bytes).unwrap();
    std::fs::write(&ours, &bytes).unwrap();
    let arg = "-ExifIFD:CreateDate=2024:02:03 04:05:06";
    let out = run(oracle
        .command()
        .arg("-overwrite_original")
        .arg(arg)
        .arg(&native));
    assert!(out.status.success(), "{out:?}");
    let out = run(Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg(arg)
        .arg(&ours));
    assert!(out.status.success(), "{out:?}");
    let expected = [0x9000, 0x9004, 0x9101, 0xa001];
    assert_eq!(exif_ids(&native), expected);
    assert_eq!(exif_ids(&ours), expected);
}

#[test]
fn rejected_date_alias_shift_does_not_conflict_with_real_date_set() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let Some(alias_source) = fixtures::pinned_t_images_fixture_path("ExifTool.jpg") else {
        return;
    };
    for (alias, read) in [
        ("-DateTime+=1", "-ModifyDate"),
        ("-DateTimeDigitized+=1", "-CreateDate"),
    ] {
        compare(oracle, &alias_source, "jpg", &[], &[alias], read);
    }
    let Some(source) = fixtures::pinned_t_images_fixture_path("Writer.jpg") else {
        return;
    };
    compare(
        oracle,
        &source,
        "jpg",
        &[],
        &["-DateTime+=1", "-ModifyDate=2025:01:02 03:04:05"],
        "-ModifyDate",
    );
    compare(
        oracle,
        &source,
        "jpg",
        &[],
        &["-DateTimeDigitized+=1", "-CreateDate=2025:01:02 03:04:05"],
        "-CreateDate",
    );
}

#[test]
fn family_deletion_across_copy_preserves_surviving_ifd0_assignments() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for (name, suffix) in [("ExifTool.tif", "tif"), ("Writer.jpg", "jpg")] {
        let Some(source) = fixtures::pinned_t_images_fixture_path(name) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let copy_source = dir.path().join(format!("source.{suffix}"));
        std::fs::copy(&source, &copy_source).unwrap();
        let setup = run(oracle
            .command()
            .args([
                "-overwrite_original",
                "-Make=source maker",
                "-IFD0:FocalLength=40",
            ])
            .arg(&copy_source));
        assert!(setup.status.success(), "{setup:?}");
        for directory in ["IFD0", "ifd0", "ExifIFD"] {
            for pre_first in [false, true] {
                let native = dir
                    .path()
                    .join(format!("native-{directory}-{pre_first}.{suffix}"));
                let ours = dir
                    .path()
                    .join(format!("ours-{directory}-{pre_first}.{suffix}"));
                std::fs::copy(&source, &native).unwrap();
                let setup = run(oracle
                    .command()
                    .args([
                        "-overwrite_original",
                        "-IFD0:FocalLength=25",
                        "-ExifIFD:FocalLength=35",
                    ])
                    .arg(&native));
                assert!(setup.status.success(), "{setup:?}");
                std::fs::copy(&native, &ours).unwrap();
                let assignment = format!("-{directory}:FocalLength=50");
                let mut args = Vec::new();
                if pre_first {
                    args.push(assignment.as_str());
                } else {
                    args.push("-EXIF:FocalLength=");
                }
                args.push("-TagsFromFile");
                let source_name = copy_source.to_str().unwrap();
                args.push(source_name);
                args.push("-Make");
                if pre_first {
                    args.push("-EXIF:FocalLength=");
                } else {
                    args.push(assignment.as_str());
                }
                let native_out = run(oracle
                    .command()
                    .arg("-overwrite_original")
                    .args(&args)
                    .arg(&native));
                assert!(
                    native_out.status.success(),
                    "native {args:?}: {native_out:?}"
                );
                let ours_out = run(Command::new(env!("CARGO_BIN_EXE_oxidex"))
                    .args(&args)
                    .arg(&ours));
                assert!(ours_out.status.success(), "ours {args:?}: {ours_out:?}");
                assert_eq!(
                    oracle_rows(oracle, &ours, "-FocalLength"),
                    oracle_rows(oracle, &native, "-FocalLength"),
                    "{name} {args:?}"
                );
                assert_eq!(
                    oracle_rows(oracle, &ours, "-Make"),
                    oracle_rows(oracle, &native, "-Make"),
                    "{name} {args:?}"
                );
            }
        }
        // The copied IFD0 assignment is also a pending destination. A
        // following family deletion removes other copies but keeps it.
        let native = dir.path().join(format!("copied-native.{suffix}"));
        let ours = dir.path().join(format!("copied-ours.{suffix}"));
        std::fs::copy(&source, &native).unwrap();
        std::fs::copy(&native, &ours).unwrap();
        let args = [
            "-TagsFromFile",
            copy_source.to_str().unwrap(),
            "-IFD0:FocalLength",
            "-EXIF:FocalLength=",
        ];
        let native_out = run(oracle
            .command()
            .arg("-overwrite_original")
            .args(args)
            .arg(&native));
        assert!(native_out.status.success(), "{native_out:?}");
        let ours_out = run(Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(args)
            .arg(&ours));
        assert!(ours_out.status.success(), "{ours_out:?}");
        assert_eq!(
            oracle_rows(oracle, &ours, "-FocalLength"),
            oracle_rows(oracle, &native, "-FocalLength"),
            "{name} copied IFD0"
        );
        // A copied value for the same physical destination replaces the
        // earlier typed value before the later family deletion is evaluated.
        let native = dir.path().join(format!("replaced-native.{suffix}"));
        let ours = dir.path().join(format!("replaced-ours.{suffix}"));
        std::fs::copy(&source, &native).unwrap();
        std::fs::copy(&native, &ours).unwrap();
        let args = [
            "-IFD0:FocalLength=50",
            "-TagsFromFile",
            copy_source.to_str().unwrap(),
            "-IFD0:FocalLength",
            "-EXIF:FocalLength=",
        ];
        let native_out = run(oracle
            .command()
            .arg("-overwrite_original")
            .args(args)
            .arg(&native));
        assert!(native_out.status.success(), "{native_out:?}");
        let ours_out = run(Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(args)
            .arg(&ours));
        assert!(ours_out.status.success(), "{ours_out:?}");
        assert_eq!(
            oracle_rows(oracle, &ours, "-FocalLength"),
            oracle_rows(oracle, &native, "-FocalLength"),
            "{name} replaced IFD0"
        );
    }
}

#[test]
fn family_set_across_copy_uses_its_canonical_exif_ifd_destination() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for (name, suffix) in [("ExifTool.tif", "tif"), ("Writer.jpg", "jpg")] {
        let Some(source) = fixtures::pinned_t_images_fixture_path(name) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let copy_source = dir.path().join(format!("source.{suffix}"));
        std::fs::copy(&source, &copy_source).unwrap();
        let setup = run(oracle
            .command()
            .args([
                "-overwrite_original",
                "-Make=source maker",
                "-IFD0:FocalLength=40",
            ])
            .arg(&copy_source));
        assert!(setup.status.success(), "{setup:?}");
        for copied in [false, true] {
            let native = dir.path().join(format!("native-{copied}.{suffix}"));
            let ours = dir.path().join(format!("ours-{copied}.{suffix}"));
            std::fs::copy(&source, &native).unwrap();
            std::fs::copy(&native, &ours).unwrap();
            let source_name = copy_source.to_str().unwrap();
            let args = if copied {
                vec![
                    "-TagsFromFile",
                    source_name,
                    "-IFD0:FocalLength",
                    "-EXIF:FocalLength=60",
                ]
            } else {
                vec![
                    "-IFD0:FocalLength=50",
                    "-TagsFromFile",
                    source_name,
                    "-Make",
                    "-EXIF:FocalLength=60",
                ]
            };
            let native_out = run(oracle
                .command()
                .arg("-overwrite_original")
                .args(&args)
                .arg(&native));
            assert!(
                native_out.status.success(),
                "native {args:?}: {native_out:?}"
            );
            let ours_out = run(Command::new(env!("CARGO_BIN_EXE_oxidex"))
                .args(&args)
                .arg(&ours));
            assert!(ours_out.status.success(), "ours {args:?}: {ours_out:?}");
            assert_eq!(
                oracle_rows(oracle, &ours, "-FocalLength"),
                oracle_rows(oracle, &native, "-FocalLength"),
                "{name} {args:?}"
            );
        }
    }
}

#[test]
fn absolute_date_set_after_copy_preserves_other_directory_assignment() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for (name, suffix) in [("ExifTool.tif", "tif"), ("Writer.jpg", "jpg")] {
        let Some(source) = fixtures::pinned_t_images_fixture_path(name) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let copy_source = dir.path().join(format!("source.{suffix}"));
        std::fs::copy(&source, &copy_source).unwrap();
        let setup = run(oracle
            .command()
            .args([
                "-overwrite_original",
                "-IFD0:CreateDate=2020:01:02 03:04:05",
                "-Make=source maker",
            ])
            .arg(&copy_source));
        assert!(setup.status.success(), "{setup:?}");
        for copied in [false, true] {
            for set in [
                "-CreateDate=2022:03:04 05:06:07",
                "-AllDates=2022:03:04 05:06:07",
            ] {
                let native = dir
                    .path()
                    .join(format!("native-{copied}-{}.{suffix}", set.len()));
                let ours = dir
                    .path()
                    .join(format!("ours-{copied}-{}.{suffix}", set.len()));
                std::fs::copy(&source, &native).unwrap();
                std::fs::copy(&native, &ours).unwrap();
                let source_name = copy_source.to_str().unwrap();
                let args = if copied {
                    vec!["-TagsFromFile", source_name, "-IFD0:CreateDate", set]
                } else {
                    vec![
                        "-IFD0:CreateDate=2020:01:02 03:04:05",
                        "-TagsFromFile",
                        source_name,
                        "-Make",
                        set,
                    ]
                };
                let native_out = run(oracle
                    .command()
                    .arg("-overwrite_original")
                    .args(&args)
                    .arg(&native));
                assert!(
                    native_out.status.success(),
                    "native {args:?}: {native_out:?}"
                );
                let ours_out = run(Command::new(env!("CARGO_BIN_EXE_oxidex"))
                    .args(&args)
                    .arg(&ours));
                assert!(ours_out.status.success(), "ours {args:?}: {ours_out:?}");
                assert_eq!(
                    oracle_rows(oracle, &ours, "-CreateDate"),
                    oracle_rows(oracle, &native, "-CreateDate"),
                    "{name} {args:?}"
                );
            }
        }
    }
}
