//! Pinned SetNewValue command-phase and file-phase conversion outcomes.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Clone, Copy)]
enum Mode {
    Single,
    List,
    Directory,
}

type Case<'a> = (&'a str, &'a Path, &'a [&'a str], i32, bool, bool);

fn files(dir: &Path, source: &Path, mode: Mode) -> Vec<PathBuf> {
    std::fs::create_dir(dir).unwrap();
    let count = if matches!(mode, Mode::Single) { 1 } else { 2 };
    (0..count)
        .map(|index| {
            let path = dir.join(format!("{index}.jpg"));
            std::fs::copy(source, &path).unwrap();
            path
        })
        .collect()
}

fn run(command: &mut Command, args: &[&str], dir: &Path, paths: &[PathBuf], mode: Mode) -> Output {
    command.args(args);
    match mode {
        Mode::Directory => {
            command.arg(dir);
        }
        Mode::Single | Mode::List => {
            command.args(paths);
        }
    }
    command.output().unwrap()
}

#[test]
fn invalid_enum_candidates_follow_pinned_command_and_file_phases() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let synthetic = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jpeg/simple/synthetic_001.jpg");
    let canon = fixtures::required_t_images_fixture_path("Canon.jpg");
    let cases: &[Case<'_>] = &[
        ("canon-only", &canon, &["-ColorSpace=bogus"], 0, true, false),
        (
            "canon-sibling",
            &canon,
            &["-ColorSpace=bogus", "-IFD0:Artist=conversion-phase"],
            0,
            false,
            true,
        ),
        (
            "orientation",
            &synthetic,
            &["-Orientation=bogus"],
            1,
            true,
            false,
        ),
        (
            "orientation-mixed-case",
            &synthetic,
            &["-oRiEnTaTiOn=definitely_invalid"],
            1,
            true,
            false,
        ),
        (
            "orientation-nonbreaking-space",
            &synthetic,
            &["-Orientation=Rotate 90 CW\u{a0}"],
            1,
            true,
            false,
        ),
        (
            "orientation-em-space",
            &synthetic,
            &["-Orientation=Rotate 90 CW\u{2003}"],
            1,
            true,
            false,
        ),
        (
            "orientation-unknown-nonbreaking-space",
            &synthetic,
            &["-Orientation=Unknown\u{a0}(6)"],
            1,
            true,
            false,
        ),
        (
            "orientation-ambiguous-substring",
            &synthetic,
            &["-Orientation=Rotate"],
            1,
            true,
            false,
        ),
        (
            "metering",
            &synthetic,
            &["-MeteringMode=xyzzy"],
            1,
            true,
            false,
        ),
        (
            "program",
            &synthetic,
            &["-ExposureProgram=bogus"],
            1,
            true,
            false,
        ),
        (
            "sensing",
            &synthetic,
            &["-SensingMethod=bogus"],
            1,
            true,
            false,
        ),
        (
            "gain",
            &synthetic,
            &["-GainControl=not_a_gain"],
            1,
            true,
            false,
        ),
        (
            "distance-range",
            &synthetic,
            &["-SubjectDistanceRange=bogus"],
            1,
            true,
            false,
        ),
        (
            "orientation-raw-code-without-hash",
            &synthetic,
            &["-Orientation=6"],
            1,
            true,
            false,
        ),
        (
            "orientation-ambiguous-numeric",
            &synthetic,
            &["-Orientation=0"],
            1,
            true,
            false,
        ),
        (
            "sony-candidate-survives-exif-conversion",
            &synthetic,
            &["-ExposureProgram=3"],
            0,
            true,
            false,
        ),
        (
            "grouped-color-space",
            &synthetic,
            &["-ExifIFD:ColorSpace=bogus"],
            1,
            true,
            false,
        ),
        (
            "light-source",
            &synthetic,
            &["-LightSource=bogus"],
            0,
            true,
            false,
        ),
        (
            "custom-rendered",
            &synthetic,
            &["-CustomRendered=bogus"],
            0,
            true,
            false,
        ),
        (
            "white-balance",
            &synthetic,
            &["-WhiteBalance=bogus"],
            0,
            true,
            false,
        ),
    ];
    for &(name, source, args, exit, unchanged, artist) in cases {
        for (mode_name, mode) in [
            ("single", Mode::Single),
            ("list", Mode::List),
            ("directory", Mode::Directory),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let native_dir = temp.path().join("native");
            let oxidex_dir = temp.path().join("oxidex");
            let native_files = files(&native_dir, source, mode);
            let oxidex_files = files(&oxidex_dir, source, mode);
            let native = run(
                oracle.command().arg("-overwrite_original"),
                args,
                &native_dir,
                &native_files,
                mode,
            );
            let oxidex = run(
                &mut Command::new(env!("CARGO_BIN_EXE_oxidex")),
                args,
                &oxidex_dir,
                &oxidex_files,
                mode,
            );
            let context = format!("{name}/{mode_name}");
            assert_eq!(
                native.status.code(),
                Some(exit),
                "native {context}: {native:?}"
            );
            assert_eq!(
                oxidex.status.code(),
                Some(exit),
                "oxidex {context}: {oxidex:?}"
            );
            for (label, output) in [("native", &native), ("oxidex", &oxidex)] {
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(
                    stderr.contains("Warning: Can't convert "),
                    "{label} {context}: {stderr}"
                );
                assert_eq!(
                    stderr.contains("Nothing to do."),
                    exit == 1,
                    "{label} {context}: {stderr}"
                );
            }
            let warning_count = |output: &Output| {
                String::from_utf8_lossy(&output.stderr)
                    .matches("Warning: Can't convert ")
                    .count()
            };
            assert_eq!(
                warning_count(&oxidex),
                warning_count(&native),
                "warning multiplicity {context}: native={native:?}, oxidex={oxidex:?}"
            );
            for (native_path, oxidex_path) in native_files.iter().zip(&oxidex_files) {
                let source_bytes = std::fs::read(source).unwrap();
                for (label, path) in [("native", native_path), ("oxidex", oxidex_path)] {
                    let bytes = std::fs::read(path).unwrap();
                    assert_eq!(bytes == source_bytes, unchanged, "{label} {context}");
                    if artist {
                        let read = oracle
                            .command()
                            .args(["-s3", "-IFD0:Artist"])
                            .arg(path)
                            .output()
                            .unwrap();
                        assert!(read.status.success());
                        assert_eq!(
                            String::from_utf8_lossy(&read.stdout).trim(),
                            "conversion-phase",
                            "{label} {context}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn unknown_and_raw_orientation_values_keep_their_write_paths() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jpeg/simple/synthetic_001.jpg");
    for (name, native_arg, oxidex_arg) in [
        (
            "unknown",
            "-Orientation=Unknown (6)",
            "-Orientation=Unknown (6)",
        ),
        (
            "unknown-final-newline",
            "-Orientation=Unknown (6)\n",
            "-Orientation=Unknown (6)\n",
        ),
        (
            "unknown-hex-final-newline",
            "-Orientation=Unknown (0x6)\n",
            "-Orientation=Unknown (0x6)\n",
        ),
        ("raw", "-Orientation#=6", "-Orientation#=6"),
        (
            "label",
            "-Orientation=Rotate 90 CW",
            "-Orientation=Rotate 90 CW",
        ),
        ("prefix", "-Orientation=Horiz", "-Orientation=Horiz"),
        (
            "label-ascii-whitespace",
            "-Orientation=Rotate 90 CW \t\r\n\u{b}\u{c}",
            "-Orientation=Rotate 90 CW \t\r\n\u{b}\u{c}",
        ),
        (
            "unknown-vertical-tab",
            "-Orientation=Unknown\u{b}(6)",
            "-Orientation=Unknown\u{b}(6)",
        ),
        ("numeric-substring", "-Orientation=1", "-Orientation=1"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let native = temp.path().join("native.jpg");
        let oxidex = temp.path().join("oxidex.jpg");
        std::fs::copy(&source, &native).unwrap();
        std::fs::copy(&source, &oxidex).unwrap();
        let native_result = oracle
            .command()
            .args(["-overwrite_original", native_arg])
            .arg(&native)
            .output()
            .unwrap();
        let oxidex_result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .arg(oxidex_arg)
            .arg(&oxidex)
            .output()
            .unwrap();
        assert!(
            native_result.status.success(),
            "native {name}: {native_result:?}"
        );
        assert!(
            oxidex_result.status.success(),
            "oxidex {name}: {oxidex_result:?}"
        );
        let read = |path: &Path| {
            let output = oracle
                .command()
                .args(["-s3", "-n", "-Orientation"])
                .arg(path)
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        let expected = read(&native);
        assert!(
            !expected.is_empty(),
            "native {name} produced no Orientation"
        );
        assert_eq!(read(&oxidex), expected, "{name}");
    }
}
