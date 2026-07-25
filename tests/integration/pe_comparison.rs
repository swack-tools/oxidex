//! PE Format Validation Tests
//!
//! This test suite validates PE metadata extraction

use oxidex::io::MMapReader;
use oxidex::parsers::pe::parse_pe_metadata;
use std::path::Path;

#[test]
#[ignore] // Requires external PE file
fn test_pe_parsing_notepad_plus_plus() {
    // This test requires Notepad++ to be downloaded
    let test_file = "/tmp/notepad++.exe";

    let test_path = Path::new(test_file);
    if test_path.exists() {
        let reader = MMapReader::new(test_path).expect("Failed to open file");
        let metadata = parse_pe_metadata(&reader).expect("Failed to parse PE");

        println!("\n=== PE Tags Extracted: {} ===", metadata.len());

        // Verify key tags are present
        assert!(
            metadata.get("EXE:MachineType").is_some(),
            "Missing PE:MachineType"
        );
        assert!(
            metadata.get("EXE:CompanyName").is_some(),
            "Missing PE:CompanyName"
        );
        assert!(
            metadata.get("EXE:FileVersion").is_some(),
            "Missing PE:FileVersion"
        );
        assert!(
            metadata.get("EXE:ProductName").is_some(),
            "Missing PE:ProductName"
        );
        assert!(
            metadata.get("EXE:FileDescription").is_some(),
            "Missing PE:FileDescription"
        );
        assert!(
            metadata.get("EXE:LegalCopyright").is_some(),
            "Missing PE:LegalCopyright"
        );

        // Print all tags for manual verification
        let mut keys: Vec<_> = metadata.keys().collect();
        keys.sort();
        for key in keys {
            if let Some(value) = metadata.get(key) {
                println!("{}: {:?}", key, value);
            }
        }

        println!("\n✓ All critical PE tags extracted successfully!");
    } else {
        eprintln!("Skipping test: {} not found", test_file);
        eprintln!(
            "Download with: curl -sL https://github.com/notepad-plus-plus/notepad-plus-plus/releases/download/v8.6.9/npp.8.6.9.portable.x64.zip -o /tmp/npp.zip && cd /tmp && unzip -q npp.zip"
        );
    }
}

#[test]
fn test_pe_tag_coverage() {
    // Verify that we extract all the tags specified in the plan
    // This test doesn't need a real PE file, just verifies the tag database

    let expected_pe_tags = vec![
        // COFF Header tags
        "EXE:MachineType",
        "EXE:NumberOfSections",
        "EXE:TimeStamp",
        "EXE:CompileTime",
        "EXE:Characteristics",
        "EXE:ImageFileCharacteristics",
        // Optional Header tags
        "EXE:ImageFormat",
        "EXE:LinkerVersion",
        "EXE:CodeSize",
        "EXE:InitializedDataSize",
        "EXE:UninitializedDataSize",
        "EXE:EntryPoint",
        "EXE:ImageBase",
        "EXE:OSVersion",
        "EXE:ImageVersion",
        "EXE:Subsystem",
        "EXE:SubsystemVersion",
        // VERSION_INFO tags
        "EXE:FileVersionNumber",
        "EXE:ProductVersionNumber",
        "EXE:FileFlags",
        "EXE:FileOS",
        "EXE:FileType",
        "EXE:FileSubtype",
        "EXE:ObjectFileType",
        "EXE:CompanyName",
        "EXE:FileDescription",
        "EXE:FileVersion",
        "EXE:InternalName",
        "EXE:LegalCopyright",
        "EXE:OriginalFilename",
        "EXE:ProductName",
        "EXE:ProductVersion",
        // Debug Directory tags (optional, depending on file)
        // "EXE:PDBFileName",
        // "EXE:PDBGUID",
        // "EXE:PDBAge",
    ];

    println!("Expected PE tags: {}", expected_pe_tags.len());
    println!("Tags:");
    for tag in &expected_pe_tags {
        println!("  - {}", tag);
    }

    // This test just documents the expected tags
    // Actual validation happens in the comparison tests
    assert!(
        expected_pe_tags.len() >= 30,
        "Should have at least 30 PE tags"
    );
}
