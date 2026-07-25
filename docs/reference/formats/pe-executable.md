# PE (Portable Executable) Format Support

## Overview

OxiDex supports extracting metadata from Windows PE (Portable Executable) files including executables (.exe), dynamic libraries (.dll), and system drivers (.sys).

## Supported Metadata

### DOS Header
- `PE:DOSSignature` - DOS signature (should be "MZ")
- `PE:PEHeaderOffset` - Offset to PE header (e_lfanew)

### COFF Header
- `PE:MachineType` - Target CPU architecture (e.g., "x64 (AMD64)", "Intel 386", "ARM64")
- `PE:MachineTypeRaw` - Raw machine type value
- `PE:NumberOfSections` - Number of sections in the executable
- `PE:TimeStamp` - Compilation timestamp (Unix epoch)
- `PE:CompileTime` - Human-readable compilation date/time
- `PE:Characteristics` - File characteristics flags
- `PE:FileType` - File type (Executable, DLL, Object)

### Optional Header
- `PE:ImageFormat` - Image format (PE32 or PE32+)
- `PE:LinkerVersion` - Linker version used to build the file
- `PE:EntryPoint` - Entry point address (RVA)
- `PE:ImageBase` - Preferred load address
- `PE:OSVersion` - Target OS version
- `PE:ImageVersion` - Application version
- `PE:Subsystem` - Subsystem type (Windows GUI, Console, Native, EFI, etc.)
- `PE:SubsystemRaw` - Raw subsystem value
- `PE:SubsystemVersion` - Subsystem version requirement
- `PE:Checksum` - File checksum (if present)

## Usage Examples

### CLI

```bash
# Extract all metadata from an executable
oxidex program.exe

# Extract specific PE tags
oxidex -PE:MachineType -PE:CompileTime -PE:Subsystem program.exe

# JSON output
oxidex -json program.exe
```

### Library API

```rust
use oxidex::Metadata;
use std::path::Path;

let metadata = Metadata::from_path("program.exe")?;

println!("Machine: {}", metadata.get_string("PE:MachineType").unwrap_or("Unknown"));
println!("Compiled: {}", metadata.get_string("PE:CompileTime").unwrap_or("Unknown"));
println!("Subsystem: {}", metadata.get_string("PE:Subsystem").unwrap_or("Unknown"));
```

## Technical Details

PE files are detected by:
1. DOS signature "MZ" (0x4D 0x5A) at file offset 0
2. PE signature "PE\0\0" (0x50 0x45 0x00 0x00) at offset specified by e_lfanew field

The parser extracts metadata from:
- DOS Header (64 bytes)
- COFF File Header (20 bytes after PE signature)
- Optional Header (variable size, contains detailed metadata)

Both PE32 (32-bit) and PE32+ (64-bit) formats are supported.

## Limitations

- Section headers and data directories are not currently parsed
- Resource information is not extracted
- Digital signatures are not validated
- Import/Export tables are not processed
