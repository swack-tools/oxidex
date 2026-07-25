//! Integration tests for PE file parsing

use oxidex::io::MMapReader;
use oxidex::parsers::pe::parse_pe_metadata;
use std::path::Path;

#[test]
#[ignore] // Ignore until we have real PE test files
fn test_parse_real_pe_file() {
    // This test requires a real PE file
    // You can use any Windows executable or DLL
    let test_file = Path::new("tests/samples/pe/sample.exe");

    if !test_file.exists() {
        eprintln!("Skipping test: sample PE file not found");
        return;
    }

    let reader = MMapReader::new(test_file).expect("Failed to open PE file");
    let metadata = parse_pe_metadata(&reader).expect("Failed to parse PE metadata");

    // Verify basic metadata is present
    assert!(metadata.contains_key("EXE:DOSSignature"));
    assert!(metadata.contains_key("EXE:MachineType"));
    assert!(metadata.contains_key("EXE:NumberOfSections"));
    assert!(metadata.contains_key("EXE:CompileTime"));
    assert!(metadata.contains_key("EXE:FileType"));
    assert!(metadata.contains_key("EXE:Subsystem"));

    // Print all metadata for manual verification
    for (key, value) in metadata.iter() {
        println!("{}: {:?}", key, value);
    }
}

#[test]
fn test_parse_minimal_pe_structure() {
    // Create minimal valid PE structure in memory
    let mut data = Vec::new();

    // DOS Header (64 bytes)
    data.extend_from_slice(&0x5A4Du16.to_le_bytes()); // MZ signature
    data.resize(0x3C, 0x00); // Padding to e_lfanew
    data.extend_from_slice(&0x80u32.to_le_bytes()); // e_lfanew = 0x80

    // DOS Stub (from 0x40 to 0x80)
    data.resize(0x80, 0x00);

    // PE Signature and COFF Header
    data.extend_from_slice(b"PE\0\0"); // PE signature
    data.extend_from_slice(&0x014Cu16.to_le_bytes()); // Machine (i386)
    data.extend_from_slice(&3u16.to_le_bytes()); // Number of sections
    data.extend_from_slice(&1609459200u32.to_le_bytes()); // Timestamp
    data.extend_from_slice(&[0; 4]); // Symbol table ptr
    data.extend_from_slice(&[0; 4]); // Number of symbols
    data.extend_from_slice(&96u16.to_le_bytes()); // Optional header size (just fields we parse)
    data.extend_from_slice(&0x0102u16.to_le_bytes()); // Characteristics

    // Optional Header Standard Fields
    data.extend_from_slice(&0x010Bu16.to_le_bytes()); // Magic (PE32)
    data.push(14); // Major linker version
    data.push(0); // Minor linker version
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Size of code
    data.extend_from_slice(&0x2000u32.to_le_bytes()); // Initialized data
    data.extend_from_slice(&0x0u32.to_le_bytes()); // Uninitialized data
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Entry point
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Base of code

    // Optional Header NT Fields (PE32)
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Base of data
    data.extend_from_slice(&0x00400000u32.to_le_bytes()); // Image base
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Section alignment
    data.extend_from_slice(&0x200u32.to_le_bytes()); // File alignment
    data.extend_from_slice(&6u16.to_le_bytes()); // Major OS version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor OS version
    data.extend_from_slice(&1u16.to_le_bytes()); // Major image version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor image version
    data.extend_from_slice(&6u16.to_le_bytes()); // Major subsystem version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor subsystem version
    data.extend_from_slice(&0u32.to_le_bytes()); // Win32 version
    data.extend_from_slice(&0x10000u32.to_le_bytes()); // Size of image
    data.extend_from_slice(&0x400u32.to_le_bytes()); // Size of headers
    data.extend_from_slice(&0u32.to_le_bytes()); // Checksum
    data.extend_from_slice(&3u16.to_le_bytes()); // Subsystem (CUI)
    data.extend_from_slice(&0u16.to_le_bytes()); // DLL characteristics
    data.extend_from_slice(&0x100000u32.to_le_bytes()); // Stack reserve
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Stack commit
    data.extend_from_slice(&0x100000u32.to_le_bytes()); // Heap reserve
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Heap commit
    data.extend_from_slice(&0u32.to_le_bytes()); // Loader flags
    data.extend_from_slice(&16u32.to_le_bytes()); // Number of RVA/sizes

    // Pad the file to ensure we have enough data for reading
    // The parser reads 512 bytes from PE offset, so ensure file is at least that large
    data.resize(0x80 + 512, 0x00);

    // Write to temp file and parse
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&data).unwrap();
    temp_file.flush().unwrap();

    let reader = MMapReader::new(temp_file.path()).expect("Failed to open temp file");
    let metadata = parse_pe_metadata(&reader).expect("Failed to parse minimal PE");

    // Verify metadata
    assert_eq!(metadata.get_string("EXE:MachineType").unwrap(), "Intel 386");
    assert_eq!(metadata.get_integer("EXE:NumberOfSections").unwrap(), 3);
    assert_eq!(metadata.get_string("EXE:FileType").unwrap(), "Executable");
    assert_eq!(
        metadata.get_string("EXE:Subsystem").unwrap(),
        "Windows Console"
    );
    assert_eq!(metadata.get_string("EXE:ImageFormat").unwrap(), "PE32");
}

#[test]
fn test_pe_header_characteristics_decoded() {
    // Create minimal valid PE structure with specific characteristics
    let mut data = Vec::new();

    // DOS Header (64 bytes)
    data.extend_from_slice(&0x5A4Du16.to_le_bytes()); // MZ signature
    data.resize(0x3C, 0x00); // Padding to e_lfanew
    data.extend_from_slice(&0x80u32.to_le_bytes()); // e_lfanew = 0x80

    // DOS Stub (from 0x40 to 0x80)
    data.resize(0x80, 0x00);

    // PE Signature and COFF Header
    data.extend_from_slice(b"PE\0\0"); // PE signature
    data.extend_from_slice(&0x014Cu16.to_le_bytes()); // Machine (i386)
    data.extend_from_slice(&3u16.to_le_bytes()); // Number of sections
    data.extend_from_slice(&1609459200u32.to_le_bytes()); // Timestamp
    data.extend_from_slice(&[0; 4]); // Symbol table ptr
    data.extend_from_slice(&[0; 4]); // Number of symbols
    data.extend_from_slice(&96u16.to_le_bytes()); // Optional header size
    // Characteristics: Executable (0x0002) + 32-bit (0x0100) = 0x0102
    data.extend_from_slice(&0x0102u16.to_le_bytes());

    // Optional Header Standard Fields
    data.extend_from_slice(&0x010Bu16.to_le_bytes()); // Magic (PE32)
    data.push(14); // Major linker version
    data.push(0); // Minor linker version
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Size of code
    data.extend_from_slice(&0x2000u32.to_le_bytes()); // Initialized data
    data.extend_from_slice(&0x0u32.to_le_bytes()); // Uninitialized data
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Entry point
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Base of code

    // Optional Header NT Fields (PE32)
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Base of data
    data.extend_from_slice(&0x00400000u32.to_le_bytes()); // Image base
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Section alignment
    data.extend_from_slice(&0x200u32.to_le_bytes()); // File alignment
    data.extend_from_slice(&6u16.to_le_bytes()); // Major OS version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor OS version
    data.extend_from_slice(&1u16.to_le_bytes()); // Major image version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor image version
    data.extend_from_slice(&6u16.to_le_bytes()); // Major subsystem version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor subsystem version
    data.extend_from_slice(&0u32.to_le_bytes()); // Win32 version
    data.extend_from_slice(&0x10000u32.to_le_bytes()); // Size of image
    data.extend_from_slice(&0x400u32.to_le_bytes()); // Size of headers
    data.extend_from_slice(&0u32.to_le_bytes()); // Checksum
    data.extend_from_slice(&3u16.to_le_bytes()); // Subsystem (CUI)
    data.extend_from_slice(&0u16.to_le_bytes()); // DLL characteristics
    data.extend_from_slice(&0x100000u32.to_le_bytes()); // Stack reserve
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Stack commit
    data.extend_from_slice(&0x100000u32.to_le_bytes()); // Heap reserve
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Heap commit
    data.extend_from_slice(&0u32.to_le_bytes()); // Loader flags
    data.extend_from_slice(&16u32.to_le_bytes()); // Number of RVA/sizes

    // Pad the file to ensure we have enough data for reading
    data.resize(0x80 + 512, 0x00);

    // Write to temp file and parse
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&data).unwrap();
    temp_file.flush().unwrap();

    let reader = MMapReader::new(temp_file.path()).expect("Failed to open temp file");
    let metadata = parse_pe_metadata(&reader).expect("Failed to parse minimal PE");

    // Should have ImageFileCharacteristics as decoded string
    assert!(metadata.contains_key("EXE:ImageFileCharacteristics"));
    let chars_str = metadata.get_string("EXE:ImageFileCharacteristics").unwrap();
    // Should contain flags like "Executable" and "32-bit"
    assert!(chars_str.contains("Executable"));
    assert!(chars_str.contains("32-bit"));
}

#[test]
fn test_pe_type_tag() {
    // Create minimal valid PE structure
    let mut data = Vec::new();

    // DOS Header (64 bytes)
    data.extend_from_slice(&0x5A4Du16.to_le_bytes()); // MZ signature
    data.resize(0x3C, 0x00); // Padding to e_lfanew
    data.extend_from_slice(&0x80u32.to_le_bytes()); // e_lfanew = 0x80

    // DOS Stub
    data.resize(0x80, 0x00);

    // PE Signature and COFF Header
    data.extend_from_slice(b"PE\0\0");
    data.extend_from_slice(&0x014Cu16.to_le_bytes()); // Machine
    data.extend_from_slice(&3u16.to_le_bytes()); // Number of sections
    data.extend_from_slice(&1609459200u32.to_le_bytes()); // Timestamp
    data.extend_from_slice(&[0; 4]); // Symbol table ptr
    data.extend_from_slice(&[0; 4]); // Number of symbols
    data.extend_from_slice(&96u16.to_le_bytes()); // Optional header size
    data.extend_from_slice(&0x0102u16.to_le_bytes()); // Characteristics

    // Optional Header Standard Fields
    data.extend_from_slice(&0x010Bu16.to_le_bytes()); // Magic (PE32)
    data.push(14); // Major linker version
    data.push(0); // Minor linker version
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Size of code
    data.extend_from_slice(&0x2000u32.to_le_bytes()); // Initialized data
    data.extend_from_slice(&0x0u32.to_le_bytes()); // Uninitialized data
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Entry point
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Base of code

    // Optional Header NT Fields
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Base of data
    data.extend_from_slice(&0x00400000u32.to_le_bytes()); // Image base
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Section alignment
    data.extend_from_slice(&0x200u32.to_le_bytes()); // File alignment
    data.extend_from_slice(&6u16.to_le_bytes()); // Major OS version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor OS version
    data.extend_from_slice(&1u16.to_le_bytes()); // Major image version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor image version
    data.extend_from_slice(&6u16.to_le_bytes()); // Major subsystem version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor subsystem version
    data.extend_from_slice(&0u32.to_le_bytes()); // Win32 version
    data.extend_from_slice(&0x10000u32.to_le_bytes()); // Size of image
    data.extend_from_slice(&0x400u32.to_le_bytes()); // Size of headers
    data.extend_from_slice(&0u32.to_le_bytes()); // Checksum
    data.extend_from_slice(&3u16.to_le_bytes()); // Subsystem
    data.extend_from_slice(&0u16.to_le_bytes()); // DLL characteristics
    data.extend_from_slice(&0x100000u32.to_le_bytes()); // Stack reserve
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Stack commit
    data.extend_from_slice(&0x100000u32.to_le_bytes()); // Heap reserve
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Heap commit
    data.extend_from_slice(&0u32.to_le_bytes()); // Loader flags
    data.extend_from_slice(&16u32.to_le_bytes()); // Number of RVA/sizes

    data.resize(0x80 + 512, 0x00);

    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&data).unwrap();
    temp_file.flush().unwrap();

    let reader = MMapReader::new(temp_file.path()).expect("Failed to open temp file");
    let metadata = parse_pe_metadata(&reader).expect("Failed to parse minimal PE");

    // Should have PEType tag
    assert!(metadata.contains_key("EXE:PEType"));
    let pe_type_str = metadata.get_string("EXE:PEType").unwrap();
    // Should be "PE32" or "PE32+"
    assert!(pe_type_str == "PE32" || pe_type_str == "PE32+");
}

#[test]
fn test_pe_with_exports() {
    // Create PE with export directory
    let mut data = Vec::new();

    // DOS Header (64 bytes)
    data.extend_from_slice(&0x5A4Du16.to_le_bytes()); // MZ signature
    data.resize(0x3C, 0x00);
    data.extend_from_slice(&0x80u32.to_le_bytes()); // e_lfanew

    // DOS Stub
    data.resize(0x80, 0x00);

    // PE Signature and COFF Header
    data.extend_from_slice(b"PE\0\0");
    data.extend_from_slice(&0x014Cu16.to_le_bytes()); // Machine (i386)
    data.extend_from_slice(&1u16.to_le_bytes()); // Number of sections
    data.extend_from_slice(&1609459200u32.to_le_bytes()); // Timestamp
    data.extend_from_slice(&[0; 4]); // Symbol table ptr
    data.extend_from_slice(&[0; 4]); // Number of symbols
    data.extend_from_slice(&224u16.to_le_bytes()); // Optional header size (PE32 with 16 data dirs)
    data.extend_from_slice(&0x2002u16.to_le_bytes()); // Characteristics (Executable, DLL)

    // Optional Header Standard Fields
    data.extend_from_slice(&0x010Bu16.to_le_bytes()); // Magic (PE32)
    data.push(14); // Major linker version
    data.push(0); // Minor linker version
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Size of code
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Initialized data
    data.extend_from_slice(&0x0u32.to_le_bytes()); // Uninitialized data
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Entry point
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Base of code

    // Optional Header NT Fields (PE32)
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Base of data
    data.extend_from_slice(&0x10000000u32.to_le_bytes()); // Image base
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Section alignment
    data.extend_from_slice(&0x200u32.to_le_bytes()); // File alignment
    data.extend_from_slice(&6u16.to_le_bytes()); // Major OS version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor OS version
    data.extend_from_slice(&1u16.to_le_bytes()); // Major image version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor image version
    data.extend_from_slice(&6u16.to_le_bytes()); // Major subsystem version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor subsystem version
    data.extend_from_slice(&0u32.to_le_bytes()); // Win32 version
    data.extend_from_slice(&0x5000u32.to_le_bytes()); // Size of image
    data.extend_from_slice(&0x400u32.to_le_bytes()); // Size of headers
    data.extend_from_slice(&0u32.to_le_bytes()); // Checksum
    data.extend_from_slice(&3u16.to_le_bytes()); // Subsystem (CUI)
    data.extend_from_slice(&0u16.to_le_bytes()); // DLL characteristics
    data.extend_from_slice(&0x100000u32.to_le_bytes()); // Stack reserve
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Stack commit
    data.extend_from_slice(&0x100000u32.to_le_bytes()); // Heap reserve
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Heap commit
    data.extend_from_slice(&0u32.to_le_bytes()); // Loader flags
    data.extend_from_slice(&16u32.to_le_bytes()); // Number of RVA/sizes

    // Data Directories (16 entries, 8 bytes each)
    // Export Directory (index 0)
    data.extend_from_slice(&0x3000u32.to_le_bytes()); // RVA of export directory
    data.extend_from_slice(&0x200u32.to_le_bytes()); // Size of export directory (must cover forwarder at 0x3110)
    // Other directories (zeroed)
    for _ in 1..16 {
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
    }

    // Section Table (1 section: .edata)
    let section_offset = data.len();
    data.extend_from_slice(b".edata\0\0"); // Name (8 bytes)
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Virtual size
    data.extend_from_slice(&0x3000u32.to_le_bytes()); // Virtual address
    data.extend_from_slice(&0x400u32.to_le_bytes()); // Size of raw data
    data.extend_from_slice(&0x400u32.to_le_bytes()); // Pointer to raw data
    data.extend_from_slice(&0u32.to_le_bytes()); // Pointer to relocations
    data.extend_from_slice(&0u32.to_le_bytes()); // Pointer to line numbers
    data.extend_from_slice(&0u16.to_le_bytes()); // Number of relocations
    data.extend_from_slice(&0u16.to_le_bytes()); // Number of line numbers
    data.extend_from_slice(&0x40000040u32.to_le_bytes()); // Characteristics (readable, initialized)

    // Pad to section data offset (0x400)
    data.resize(0x400, 0x00);

    // Export Directory (at file offset 0x400, RVA 0x3000)
    let export_dir_offset = data.len();
    data.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    data.extend_from_slice(&1609459200u32.to_le_bytes()); // Timestamp
    data.extend_from_slice(&0u16.to_le_bytes()); // Major version
    data.extend_from_slice(&0u16.to_le_bytes()); // Minor version
    data.extend_from_slice(&0x3100u32.to_le_bytes()); // Name RVA (points to "test.dll")
    data.extend_from_slice(&1u32.to_le_bytes()); // Base ordinal
    data.extend_from_slice(&3u32.to_le_bytes()); // Number of functions
    data.extend_from_slice(&2u32.to_le_bytes()); // Number of names
    data.extend_from_slice(&0x3050u32.to_le_bytes()); // Address of functions
    data.extend_from_slice(&0x3060u32.to_le_bytes()); // Address of names
    data.extend_from_slice(&0x3070u32.to_le_bytes()); // Address of name ordinals

    // Export Address Table (at file offset 0x450, RVA 0x3050)
    data.resize(0x450, 0x00);
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Function 1 RVA
    data.extend_from_slice(&0x1010u32.to_le_bytes()); // Function 2 RVA
    data.extend_from_slice(&0x3110u32.to_le_bytes()); // Function 3 RVA (forwarded - within export section)

    // Export Name Pointer Table (at file offset 0x460, RVA 0x3060)
    data.resize(0x460, 0x00);
    data.extend_from_slice(&0x3120u32.to_le_bytes()); // Name 1 RVA
    data.extend_from_slice(&0x3130u32.to_le_bytes()); // Name 2 RVA

    // Export Ordinal Table (at file offset 0x470, RVA 0x3070)
    data.resize(0x470, 0x00);
    data.extend_from_slice(&0u16.to_le_bytes()); // Ordinal for name 1
    data.extend_from_slice(&1u16.to_le_bytes()); // Ordinal for name 2

    // DLL Name (at file offset 0x500, RVA 0x3100)
    data.resize(0x500, 0x00);
    data.extend_from_slice(b"test.dll\0");

    // Forwarder string (at file offset 0x510, RVA 0x3110)
    data.resize(0x510, 0x00);
    data.extend_from_slice(b"KERNEL32.GetProcAddress\0");

    // Export names (at file offset 0x520, RVA 0x3120)
    data.resize(0x520, 0x00);
    data.extend_from_slice(b"Function1\0");
    data.resize(0x530, 0x00);
    data.extend_from_slice(b"Function2\0");

    // Pad to reasonable size
    data.resize(0x800, 0x00);

    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&data).unwrap();
    temp_file.flush().unwrap();

    let reader = MMapReader::new(temp_file.path()).expect("Failed to open temp file");
    let metadata = parse_pe_metadata(&reader).expect("Failed to parse PE with exports");

    // Verify export metadata
    assert!(metadata.contains_key("EXE:HasExports"));
    assert_eq!(metadata.get_integer("EXE:HasExports").unwrap(), 1);

    assert_eq!(
        metadata.get_string("EXE:ExportDLLName").unwrap(),
        "test.dll"
    );
    assert_eq!(metadata.get_integer("EXE:ExportCount").unwrap(), 3);
    assert_eq!(metadata.get_integer("EXE:ExportNameCount").unwrap(), 2);
    assert_eq!(metadata.get_integer("EXE:ExportBase").unwrap(), 1);
    assert_eq!(
        metadata.get_integer("EXE:ExportTimestamp").unwrap(),
        1609459200
    );
    assert!(metadata.contains_key("EXE:ExportCreateDate"));

    // Verify forwarded export count
    assert_eq!(metadata.get_integer("EXE:ForwardedExportCount").unwrap(), 1);

    // Verify exported function names
    let exported_functions = metadata.get_string("EXE:ExportedFunctions").unwrap();
    assert!(exported_functions.contains("Function1"));
    assert!(exported_functions.contains("Function2"));
}
