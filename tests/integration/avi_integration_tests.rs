use oxidex::core::TagValue;
use oxidex::exiftool_oracle;
use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::video::parse_avi_metadata;
use serde_json::Value;
use std::path::Path;

const CORPUS_ROOT: &str = "/tmp/oxidex-exiftool-cache/combined-samples";

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

/// ExifTool 13.59's `RIFF.pm:338` Main table owns AVI files and has no AVI
/// family-1 override. These real corpus carriers previously emitted eight
/// same-chunk `AVI:` aliases alongside their RIFF values.
#[test]
fn real_avi_carriers_keep_riff_values_without_an_avi_group() {
    for (file, expected) in [
        (
            "Pentax.avi",
            [
                ("RIFF:FrameRate", TagValue::String("24".to_string())),
                ("RIFF:ImageWidth", TagValue::Integer(1280)),
                ("RIFF:ImageHeight", TagValue::Integer(720)),
                ("RIFF:Duration", TagValue::String("25.00".to_string())),
                ("RIFF:VideoCodec", TagValue::String("mjpg".to_string())),
                ("RIFF:NumChannels", TagValue::Integer(1)),
                ("RIFF:SampleRate", TagValue::Integer(32000)),
                ("RIFF:AvgBytesPerSec", TagValue::Integer(64000)),
            ],
        ),
        (
            "RIFF.avi",
            [
                ("RIFF:FrameRate", TagValue::String("15".to_string())),
                ("RIFF:ImageWidth", TagValue::Integer(320)),
                ("RIFF:ImageHeight", TagValue::Integer(240)),
                ("RIFF:Duration", TagValue::String("15.53".to_string())),
                ("RIFF:VideoCodec", TagValue::String("mjpg".to_string())),
                ("RIFF:NumChannels", TagValue::Integer(1)),
                ("RIFF:SampleRate", TagValue::Integer(11024)),
                ("RIFF:AvgBytesPerSec", TagValue::Integer(11024)),
            ],
        ),
    ] {
        let path = Path::new(CORPUS_ROOT).join(file);
        let reader = BufferedReader::new(&path).expect("real AVI carrier must be available");
        let metadata = parse_avi_metadata(&reader).expect("real AVI carrier must parse");

        for (tag, value) in expected {
            assert_eq!(metadata.get(tag), Some(&value), "{file}: {tag}");
        }
        assert!(
            metadata.iter().all(|(key, _)| !key.starts_with("AVI:")),
            "{file}: fabricated AVI family-1 group returned"
        );
    }
}

#[test]
#[ignore] // Requires ExifTool to be installed
fn test_avi_metadata_parity_with_exiftool() {
    let test_file = "test_data/video/sample.avi";

    // Check if test file exists
    if !std::path::Path::new(test_file).exists() {
        eprintln!("Warning: {} not found, skipping test", test_file);
        return;
    }

    // Run ExifTool
    // -G0 is required: the tags compared below are group-qualified
    // ("RIFF:FrameRate"), and a plain `-json` emits bare tag names, so every
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
    let reader = BufferedReader::new(Path::new(test_file)).expect("Failed to open AVI file");
    let oxidex_metadata = parse_avi_metadata(&reader).expect("Failed to parse AVI file");

    // Compare key tags
    let tags_to_compare = ["RIFF:FrameRate", "RIFF:ImageWidth", "RIFF:ImageHeight"];

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
