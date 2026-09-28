//! GitHub #957 review: mixed file/directory inputs.
use oxidex::exiftool_oracle;
use std::process::{Command, Output};
const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn every_input_is_read_when_files_and_directories_are_mixed() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("plain.jpg");
    std::fs::copy(JPEG, &plain).unwrap();
    let images = dir.path().join("images");
    std::fs::create_dir(&images).unwrap();
    std::fs::copy(JPEG, images.join("nested.jpg")).unwrap();
    let deep = images.join("deep");
    std::fs::create_dir(&deep).unwrap();
    std::fs::copy(JPEG, deep.join("deep.jpg")).unwrap();
    for recursive in [false, true] {
        for directory_last in [false, true] {
            let (first, last) = if directory_last {
                (&plain, &images)
            } else {
                (&images, &plain)
            };
            let mut args = vec!["-j"];
            if recursive {
                args.push("-r");
            }
            args.extend([first.to_str().unwrap(), last.to_str().unwrap()]);
            let expected = oracle.command().args(&args).output().unwrap();
            assert!(expected.status.success(), "{expected:?}");
            let expected: Vec<serde_json::Value> =
                serde_json::from_slice(&expected.stdout).unwrap();
            let actual = run(&args);
            assert!(actual.status.success(), "{actual:?}");
            let actual: Vec<serde_json::Value> = serde_json::from_slice(&actual.stdout).unwrap();
            let paths = |rows: &[serde_json::Value]| {
                let mut names = rows
                    .iter()
                    .map(|row| row["SourceFile"].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>();
                names.sort();
                names
            };
            assert_eq!(expected.len(), if recursive { 3 } else { 2 });
            assert_eq!(
                paths(&actual),
                paths(&expected),
                "recursive={recursive}, directory_last={directory_last}"
            );
        }
    }
}
