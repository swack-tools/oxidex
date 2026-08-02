use oxidex::core::TagValue;
use oxidex::exiftool_oracle;
use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::audio::mp3::parse_mp3_metadata;
use serde_json::Value;
use std::path::Path;

/// The value as it reaches output, for the variants these parsers emit.
///
/// Anything else is reported verbatim so a value stored in an unexpected shape
/// fails the comparison instead of quietly reading as equal.
fn printed(value: &TagValue) -> String {
    match value {
        TagValue::String(s) => s.clone(),
        TagValue::Integer(n) => n.to_string(),
        TagValue::Float(f) => f.to_string(),
        other => format!("<unexpected TagValue variant: {:?}>", other),
    }
}

#[test]
#[ignore] // Requires ExifTool to be installed
fn test_mp3_metadata_parity_with_exiftool() {
    let test_file = "test_data/audio/sample.mp3";

    // Check if test file exists
    if !std::path::Path::new(test_file).exists() {
        eprintln!("Warning: {} not found, skipping test", test_file);
        return;
    }

    // Run ExifTool
    // -G0 is required: the tags compared below are group-qualified
    // ("ID3:Title"), and a plain `-json` emits bare tag names, so every
    // lookup would miss and the comparison would silently pass on nothing.
    let oracle =
        exiftool_oracle::shared().unwrap_or_else(|e| panic!("No usable ExifTool oracle: {e}"));
    let exiftool_output = oracle
        .command()
        .arg("-G0")
        .arg("-json")
        .arg(test_file)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "Failed to run ExifTool at {} - is it installed? {e}",
                oracle.display()
            )
        });

    assert!(exiftool_output.status.success(), "ExifTool failed");

    let exiftool_json: Vec<Value> =
        serde_json::from_slice(&exiftool_output.stdout).expect("Failed to parse ExifTool JSON");

    // Run OxiDex
    let reader = BufferedReader::new(Path::new(test_file)).expect("Failed to open MP3 file");
    let oxidex_metadata = parse_mp3_metadata(&reader).expect("Failed to parse MP3 file");

    // Compare key tags
    let tags_to_compare = ["ID3:Title", "ID3:Artist", "ID3:Album"];

    for tag in &tags_to_compare {
        let exiftool_value = &exiftool_json[0][tag];
        if exiftool_value.is_null() {
            continue; // Skip tags not present in test file
        }

        let oxidex_value = oxidex_metadata.get(tag);

        assert!(oxidex_value.is_some(), "OxiDex missing tag: {}", tag);

        // Compare values (convert to strings for comparison)
        let exiftool_str = exiftool_value.to_string().trim_matches('"').to_string();
        let oxidex_str = printed(oxidex_value.unwrap());

        assert_eq!(
            exiftool_str, oxidex_str,
            "Mismatch for tag {}: ExifTool={}, OxiDex={}",
            tag, exiftool_str, oxidex_str
        );
    }
}
