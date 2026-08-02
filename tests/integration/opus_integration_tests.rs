use oxidex::core::MetadataMap;
use oxidex::exiftool_oracle;
use serde_json::Value;

#[test]
#[ignore] // Requires ExifTool to be installed
fn test_opus_metadata_parity_with_exiftool() {
    let test_file = "test_data/audio/sample.opus";

    // Check if test file exists
    if !std::path::Path::new(test_file).exists() {
        eprintln!("Warning: {} not found, skipping test", test_file);
        return;
    }

    // Run ExifTool
    let oracle =
        exiftool_oracle::shared().unwrap_or_else(|e| panic!("No usable ExifTool oracle: {e}"));
    let exiftool_output = oracle
        .command()
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

    let exiftool_json: Vec<Value> = serde_json::from_slice(&exiftool_output.stdout)
        .expect("Failed to parse ExifTool JSON");

    // Run OxiDex
    let oxidex_metadata = MetadataMap::from_file(test_file)
        .expect("Failed to parse Opus file");

    // Compare key tags
    let tags_to_compare = [
        "Opus:OpusVersion",
        "Opus:Channels",
    ];

    for tag in &tags_to_compare {
        let exiftool_value = &exiftool_json[0][tag];
        if exiftool_value.is_null() {
            continue; // Skip tags not present in test file
        }

        let oxidex_value = oxidex_metadata.get(tag);

        assert!(
            oxidex_value.is_some(),
            "OxiDex missing tag: {}",
            tag
        );

        // Compare values (convert to strings for comparison)
        let exiftool_str = exiftool_value.to_string().trim_matches('"').to_string();
        let oxidex_str = oxidex_value.unwrap().to_string();

        assert_eq!(
            exiftool_str, oxidex_str,
            "Mismatch for tag {}: ExifTool={}, OxiDex={}",
            tag, exiftool_str, oxidex_str
        );
    }
}
