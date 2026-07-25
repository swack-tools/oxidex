//! Integration test for PE import parsing

use oxidex::io::MMapReader;
use oxidex::parsers::pe::parse_pe_metadata;

#[test]
fn test_pe_with_imports() {
    // Create a minimal PE file with import directory
    let mut data = Vec::new();

    // DOS Header (64 bytes)
    data.extend_from_slice(&0x5A4Du16.to_le_bytes()); // MZ signature
    data.resize(0x3C, 0x00); // Padding to e_lfanew
    data.extend_from_slice(&0x80u32.to_le_bytes()); // e_lfanew = 0x80
    data.resize(0x80, 0x00); // DOS stub

    // PE Signature and COFF Header
    data.extend_from_slice(b"PE\0\0"); // PE signature
    data.extend_from_slice(&0x014Cu16.to_le_bytes()); // Machine (i386)
    data.extend_from_slice(&1u16.to_le_bytes()); // Number of sections
    data.extend_from_slice(&1609459200u32.to_le_bytes()); // Timestamp
    data.extend_from_slice(&0u32.to_le_bytes()); // Symbol table ptr
    data.extend_from_slice(&0u32.to_le_bytes()); // Number of symbols
    data.extend_from_slice(&0xE0u16.to_le_bytes()); // Optional header size (224 bytes for 16 dirs)
    data.extend_from_slice(&0x0102u16.to_le_bytes()); // Characteristics

    // Optional Header Standard Fields (28 bytes)
    data.extend_from_slice(&0x010Bu16.to_le_bytes()); // Magic (PE32)
    data.push(14); // Major linker version
    data.push(0); // Minor linker version
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Size of code
    data.extend_from_slice(&0x2000u32.to_le_bytes()); // Initialized data
    data.extend_from_slice(&0x0u32.to_le_bytes()); // Uninitialized data
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Entry point
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Base of code
    data.extend_from_slice(&0x2000u32.to_le_bytes()); // Base of data

    // Optional Header NT Fields (68 bytes for base fields)
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

    // Data Directories (16 directories * 8 bytes = 128 bytes)
    // Directory 0: Export Table
    data.extend_from_slice(&0u32.to_le_bytes()); // RVA
    data.extend_from_slice(&0u32.to_le_bytes()); // Size
    // Directory 1: Import Table (RVA 0x3000, size doesn't matter much)
    data.extend_from_slice(&0x3000u32.to_le_bytes()); // RVA
    data.extend_from_slice(&0x100u32.to_le_bytes()); // Size
    // Directories 2-15: Empty
    for _ in 2..16 {
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
    }

    // Section Table (1 section: .idata)
    data.extend_from_slice(b".idata\0\0"); // Name (8 bytes)
    data.extend_from_slice(&0x1000u32.to_le_bytes()); // Virtual size
    data.extend_from_slice(&0x3000u32.to_le_bytes()); // Virtual address
    data.extend_from_slice(&0x600u32.to_le_bytes()); // Size of raw data
    data.extend_from_slice(&0x400u32.to_le_bytes()); // Pointer to raw data
    data.extend_from_slice(&0u32.to_le_bytes()); // Pointer to relocations
    data.extend_from_slice(&0u32.to_le_bytes()); // Pointer to line numbers
    data.extend_from_slice(&0u16.to_le_bytes()); // Number of relocations
    data.extend_from_slice(&0u16.to_le_bytes()); // Number of line numbers
    data.extend_from_slice(&0xC0000040u32.to_le_bytes()); // Characteristics

    // Pad to import directory offset (0x400)
    data.resize(0x400, 0x00);

    // Import Directory (at file offset 0x400, RVA 0x3000)
    // Import Descriptor 1: kernel32.dll
    data.extend_from_slice(&0x3050u32.to_le_bytes()); // OriginalFirstThunk (ILT)
    data.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
    data.extend_from_slice(&0u32.to_le_bytes()); // ForwarderChain
    data.extend_from_slice(&0x3100u32.to_le_bytes()); // Name (RVA to "kernel32.dll")
    data.extend_from_slice(&0x3050u32.to_le_bytes()); // FirstThunk (IAT, same as ILT for simplicity)

    // Import Descriptor 2: user32.dll
    data.extend_from_slice(&0x3070u32.to_le_bytes()); // OriginalFirstThunk
    data.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
    data.extend_from_slice(&0u32.to_le_bytes()); // ForwarderChain
    data.extend_from_slice(&0x3110u32.to_le_bytes()); // Name (RVA to "user32.dll")
    data.extend_from_slice(&0x3070u32.to_le_bytes()); // FirstThunk

    // Null descriptor (end of import table)
    data.extend_from_slice(&[0u8; 20]);

    // Import Lookup Table for kernel32.dll (at file offset 0x450, RVA 0x3050)
    data.resize(0x450, 0x00);
    data.extend_from_slice(&0x3120u32.to_le_bytes()); // RVA to "VirtualAlloc" import by name
    data.extend_from_slice(&0x3140u32.to_le_bytes()); // RVA to "CreateFileW" import by name
    data.extend_from_slice(&0u32.to_le_bytes()); // Null terminator

    // Import Lookup Table for user32.dll (at file offset 0x470, RVA 0x3070)
    data.resize(0x470, 0x00);
    data.extend_from_slice(&0x3160u32.to_le_bytes()); // RVA to "MessageBoxW" import by name
    data.extend_from_slice(&0u32.to_le_bytes()); // Null terminator

    // DLL Names
    // kernel32.dll (at file offset 0x500, RVA 0x3100)
    data.resize(0x500, 0x00);
    data.extend_from_slice(b"kernel32.dll\0");

    // user32.dll (at file offset 0x510, RVA 0x3110)
    data.resize(0x510, 0x00);
    data.extend_from_slice(b"user32.dll\0");

    // Import by name structures
    // VirtualAlloc (at file offset 0x520, RVA 0x3120)
    data.resize(0x520, 0x00);
    data.extend_from_slice(&0u16.to_le_bytes()); // Hint
    data.extend_from_slice(b"VirtualAlloc\0");

    // CreateFileW (at file offset 0x540, RVA 0x3140)
    data.resize(0x540, 0x00);
    data.extend_from_slice(&0u16.to_le_bytes()); // Hint
    data.extend_from_slice(b"CreateFileW\0");

    // MessageBoxW (at file offset 0x560, RVA 0x3160)
    data.resize(0x560, 0x00);
    data.extend_from_slice(&0u16.to_le_bytes()); // Hint
    data.extend_from_slice(b"MessageBoxW\0");

    // Pad to reasonable size
    data.resize(0x800, 0x00);

    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&data).unwrap();
    temp_file.flush().unwrap();

    let reader = MMapReader::new(temp_file.path()).expect("Failed to open temp file");
    let metadata = parse_pe_metadata(&reader).expect("Failed to parse PE with imports");

    // Debug: Print all available keys
    println!("Available metadata keys:");
    for (key, value) in metadata.iter() {
        println!("  {}: {:?}", key, value);
    }

    // Verify import metadata
    let imported_dlls = metadata
        .get_string("EXE:ImportedDLLs")
        .expect("EXE:ImportedDLLs not found");
    assert!(imported_dlls.contains("kernel32.dll"));
    assert!(imported_dlls.contains("user32.dll"));

    // Verify import count (3 functions total)
    assert_eq!(metadata.get_integer("EXE:ImportCount").unwrap(), 3);

    // Verify DLL count
    assert_eq!(metadata.get_integer("EXE:ImportedDLLCount").unwrap(), 2);

    // Verify imported functions
    let imported_functions = metadata.get_string("EXE:ImportedFunctions").unwrap();
    assert!(imported_functions.contains("kernel32.dll:VirtualAlloc"));
    assert!(imported_functions.contains("kernel32.dll:CreateFileW"));
    assert!(imported_functions.contains("user32.dll:MessageBoxW"));

    // Verify suspicious imports flag (VirtualAlloc is suspicious)
    assert_eq!(metadata.get_integer("EXE:HasSuspiciousImports").unwrap(), 1);
}
