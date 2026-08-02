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

#[test]
fn lowercase_tag_filter_matches_case_insensitively() {
    // ExifTool tag-name arguments are case-insensitive: `-make` must match IFD0:Make.
    let output = oxidex(&["-make", "tests/fixtures/jpeg/sample_with_exif.jpg"]);
    assert!(
        output.status.success(),
        "expected -make to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("IFD0:Make: TestCamera"));
    assert!(!stdout.contains("IFD0:Model"));
}

#[test]
fn assignment_args_with_date_option_prefix_reach_the_write_path() {
    // Regression: an assignment whose tag starts with the `-d` date-format
    // short option (as an attached value) was silently falling through to read
    // mode. Use a group-qualified, resolvable tag so the test verifies the
    // value actually lands, not just that the banner printed.
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let temp_file = temp_dir.path().join("write_target.jpg");
    std::fs::copy("tests/fixtures/jpeg/sample_with_exif.jpg", &temp_file).expect("copy fixture");

    let output = oxidex(&[
        "-IFD0:ImageDescription=OxiDex QA",
        temp_file.to_str().expect("temp path utf-8"),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("updated"),
        "assignment arg must reach the tag-write path, got stdout: {stdout}"
    );
    assert!(
        !stdout.contains("Found "),
        "assignment arg must not fall through to a metadata dump, got stdout: {stdout}"
    );

    // Re-read: the written tag must actually be present, not silently dropped.
    let reread = oxidex(&[temp_file.to_str().expect("temp path utf-8")]);
    let reread_stdout = String::from_utf8_lossy(&reread.stdout);
    assert!(
        reread_stdout.contains("ImageDescription: OxiDex QA"),
        "written tag must survive a round-trip, got: {reread_stdout}"
    );
}

#[test]
fn multiple_explicit_files_json_produces_one_object_per_file() {
    // Regression: `oxidex -j a.jpg b.jpg` used to print a single JSON
    // document -- the *last* file's tags with no SourceFile key at all --
    // because CliArgs::file() only ever looked at the final positional
    // argument. Every earlier file was silently dropped.
    let output = oxidex(&[
        "-json",
        "tests/fixtures/jpeg/sample_with_exif.jpg",
        "tests/fixtures/jpeg/sample_with_exif_xmp.jpg",
    ]);
    assert!(
        output.status.success(),
        "expected multi-file -json to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let values: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("multi-file JSON stdout must contain only parseable JSON");
    let items = values
        .as_array()
        .expect("multi-file JSON output must be an array");
    assert_eq!(items.len(), 2, "expected exactly one object per input file");

    let source_files: Vec<&str> = items
        .iter()
        .map(|item| {
            item.get("SourceFile")
                .and_then(|v| v.as_str())
                .expect("every object must carry SourceFile")
        })
        .collect();
    assert!(source_files[0].ends_with("sample_with_exif.jpg"));
    assert!(source_files[1].ends_with("sample_with_exif_xmp.jpg"));
    assert_ne!(
        source_files[0], source_files[1],
        "each object must be attributed to a distinct file"
    );

    let tag_sets: Vec<std::collections::BTreeSet<&str>> = items
        .iter()
        .map(|item| {
            item.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect()
        })
        .collect();
    assert_ne!(
        tag_sets[0], tag_sets[1],
        "the two files' tag sets must not collapse into an identical result"
    );

    // serde_json's Map has no insertion-order tracking here (this crate
    // doesn't enable the "preserve_order" feature), so re-parsing into
    // `serde_json::Value` and reading `.keys()` would re-sort alphabetically
    // and could never observe emission order either way. Check the raw text
    // instead: within each object, "SourceFile" must be the first key line.
    let stdout = String::from_utf8_lossy(&output.stdout);
    for object_text in stdout.split("  {").skip(1) {
        let first_key_line = object_text
            .lines()
            .find(|line| line.trim_start().starts_with('"'))
            .expect("object must have at least one key line");
        assert!(
            first_key_line.contains("\"SourceFile\""),
            "SourceFile must be the first key, matching ExifTool's -j ordering; got: {first_key_line}"
        );
    }
}

#[test]
fn multiple_explicit_files_csv_has_one_row_group_per_file() {
    let output = oxidex(&[
        "-csv",
        "tests/fixtures/jpeg/sample_with_exif.jpg",
        "tests/fixtures/jpeg/sample_with_exif_xmp.jpg",
    ]);
    assert!(
        output.status.success(),
        "expected multi-file -csv to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("tests/fixtures/jpeg/sample_with_exif.jpg,"))
    );
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("tests/fixtures/jpeg/sample_with_exif_xmp.jpg,"))
    );
}

#[test]
fn multiple_explicit_files_human_readable_has_one_header_per_file() {
    let output = oxidex(&[
        "tests/fixtures/jpeg/sample_with_exif.jpg",
        "tests/fixtures/jpeg/sample_with_exif_xmp.jpg",
    ]);
    assert!(
        output.status.success(),
        "expected multi-file human-readable read to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("File: tests/fixtures/jpeg/sample_with_exif.jpg"));
    assert!(stdout.contains("File: tests/fixtures/jpeg/sample_with_exif_xmp.jpg"));
}

#[test]
fn bare_tag_filter_starting_with_d_is_not_swallowed_by_date_option() {
    // Regression: `-datetimeoriginal` (a lowercase tag filter) was parsed as
    // the `-d` date-format option with an attached value, dumping all tags.
    let output = oxidex(&[
        "-datetimeoriginal",
        "tests/fixtures/jpeg/sample_with_exif.jpg",
    ]);
    assert!(
        output.status.success(),
        "expected -datetimeoriginal filter to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // A filter narrows output to matching tags; it must not dump unrelated ones.
    assert!(
        !stdout.contains("IFD0:Make"),
        "filter must not fall through to a full dump, got: {stdout}"
    );
}
