use std::process::Command;

fn oxidex(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

#[test]
fn single_dash_json_is_accepted() {
    let output = oxidex(&["-json", "tests/fixtures/jpeg/sample_with_exif.jpg"]);
    assert!(
        output.status.success(),
        "expected -json to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .trim_start()
            .starts_with('[')
    );
}

#[test]
fn single_dash_short_tag_filter_is_accepted() {
    let output = oxidex(&["-Make", "tests/fixtures/jpeg/sample_with_exif.jpg"]);
    assert!(
        output.status.success(),
        "expected -Make to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("IFD0:Make: TestCamera"));
    assert!(!stdout.contains("IFD0:Model"));
}

#[test]
fn batch_directory_honors_short_format() {
    let output = oxidex(&["-s", "tests/fixtures/jpeg/simple"]);
    assert!(
        output.status.success(),
        "expected batch -s to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Make:") || stdout.contains("Model:"));
    assert!(stdout.contains("SourceFile: tests/fixtures/jpeg/simple/"));
    assert!(!stdout.contains("IFD0:"));
    assert!(!stdout.contains("EXIF:"));
    assert!(!stdout.contains("========"));
    assert!(!stdout.contains("Found "));
}

#[test]
fn single_dash_short_option_cluster_still_reaches_lexopt() {
    let output = oxidex(&["-sr", "tests/fixtures/jpeg/simple"]);
    assert!(
        output.status.success(),
        "expected -sr to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("SourceFile: tests/fixtures/jpeg/simple/"));
    assert!(!stdout.contains("image files read"));
    assert!(!stdout.lines().any(|line| line.starts_with("File: ")));
}

#[test]
fn attached_date_format_option_still_reaches_lexopt() {
    let output = oxidex(&[
        "-d%Y%m%d",
        "-FileName<IFD0:ModifyDate",
        "-n",
        "tests/fixtures/jpeg/sample_with_exif.jpg",
    ]);
    assert!(
        output.status.success(),
        "expected attached -d format to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-> tests/fixtures/jpeg/20250115"));
}

#[test]
fn dash_leading_date_format_value_stays_with_date_option() {
    let output = oxidex(&[
        "-d",
        "-%Y%m%d",
        "-FileName<IFD0:ModifyDate",
        "-n",
        "tests/fixtures/jpeg/sample_with_exif.jpg",
    ]);
    assert!(
        output.status.success(),
        "expected dash-leading -d value to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-> tests/fixtures/jpeg/-20250115"));
}

#[test]
fn date_format_value_that_looks_like_an_option_is_not_normalized() {
    let output = oxidex(&[
        "-d",
        "-json",
        "-FileName<IFD0:ModifyDate",
        "-n",
        "tests/fixtures/jpeg/sample_with_exif.jpg",
    ]);
    assert!(
        output.status.success(),
        "expected -json date format value to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-> tests/fixtures/jpeg/-json"));
    assert!(!stdout.trim_start().starts_with('['));
}

#[test]
fn batch_directory_json_is_parseable_and_includes_source_file() {
    let output = oxidex(&["-json", "tests/fixtures/jpeg/simple"]);
    assert!(
        output.status.success(),
        "expected batch -json to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let values: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("batch JSON stdout must contain only parseable JSON");
    let items = values
        .as_array()
        .expect("batch JSON output must be an array");
    assert!(items.len() > 1, "expected multiple files in batch JSON");
    assert!(items.iter().all(|item| item.get("SourceFile").is_some()));
}

#[test]
fn batch_directory_csv_has_single_header_and_source_file_column() {
    let output = oxidex(&["-csv", "tests/fixtures/jpeg/simple"]);
    assert!(
        output.status.success(),
        "expected batch -csv to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("SourceFile,Tag,Value").count(), 1);
    assert!(!stdout.contains("image files read"));
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("tests/fixtures/jpeg/simple/") && line.contains(","))
    );
}
