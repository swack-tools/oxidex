use oxidex::exiftool_oracle;
use std::path::Path;
use std::process::Command;

#[test]
fn pdf_keyword_scalar_binary_output_matches_pinned_exiftool() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping PDF keyword binary parity: pinned oracle unavailable");
        return;
    };
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdf/sample.pdf");
    let extract = |mut command: Command| {
        let output = command
            .args(["-b", "-Keywords"])
            .arg(&file)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    let expected = extract(oracle.command());
    assert_eq!(expected, b"exiftool, rust, pdf, metadata, test");
    assert_eq!(
        extract(Command::new(env!("CARGO_BIN_EXE_oxidex"))),
        expected
    );
}
