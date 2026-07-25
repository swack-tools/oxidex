# PE Rich Header Extraction

## Overview

The PE parser has been enhanced to extract the **Rich Header**, an undocumented Microsoft structure embedded in PE files compiled with Visual Studio and related Microsoft toolchains. The Rich Header contains forensically valuable information about the compilers and tools used to build the executable.

## Location and Structure

The Rich Header is located between the DOS stub (typically at 0x80) and the PE signature. It consists of:

1. **Header Start**: Encrypted "DanS" signature (0x536E6144)
2. **Padding**: Three zero DWORDs
3. **Tool Entries**: Pairs of (compid, count) entries
   - `compid = (build_number << 16) | product_id`
   - `count = number of times the tool was used
4. **Header End**: "Rich" signature (0x68636952) followed by XOR key

The entire header is XOR-encrypted with a checksum key that follows the "Rich" marker.

## Extracted Metadata Tags

| Tag Name | Type | Description |
|----------|------|-------------|
| `PE:RichHeaderPresent` | String | "Yes" if Rich Header found |
| `PE:RichHeaderChecksum` | String | XOR key/checksum in hex format |
| `PE:RichHeaderEntries` | Integer | Number of compiler/tool entries |
| `PE:RichCompilerInfo` | String | Formatted list of "ProductID.BuildNumber xCount" |
| `PE:RichProductIDs` | String | Comma-separated unique product IDs |
| `PE:RichHeaderHash` | String | MD5 hash of decrypted header (for forensic comparison) |

## Product ID Mapping

The parser includes comprehensive product ID mapping for Microsoft toolchains:

### Common Product IDs

| ID | Tool |
|----|------|
| 0x01 | Import0 (linker import object) |
| 0x0A | Utc11_Basic |
| 0x0F | AliasObj60 |
| 0x11 | Masm613 |
| 0x5D-0x6A | Visual Studio 2002/2003 tools |
| 0x83-0x91 | Visual Studio 2005 tools |
| 0x92-0xA0 | Visual Studio 2008 tools |
| 0xA1-0xA7 | Visual Studio 2010 tools |
| 0xDB-0xDF | Visual Studio 2013 tools |

Over 60 product IDs are mapped in total.

## Forensic Value

The Rich Header is valuable for:

1. **Build Environment Fingerprinting**: Identify exact compiler versions used
2. **Malware Analysis**: Detect tampered or forged executables
3. **Software Provenance**: Verify claimed build tools
4. **Anomaly Detection**:
   - Missing Rich Header in claimed MSVC binaries
   - Mismatched checksums indicating tampering
   - Unusual tool combinations

## Implementation Details

### File Structure

- **Parser**: `src/parsers/pe/rich_header_parser.rs`
  - Signature detection (forward and backward search)
  - XOR decryption
  - Entry parsing
  - Product ID mapping

- **Metadata Extraction**: `src/parsers/pe/metadata_extractor.rs`
  - `extract_rich_header_metadata()` function

- **Integration**: `src/parsers/pe/mod.rs`
  - Called between DOS header and COFF header parsing
  - Graceful fallback if Rich Header absent

### Decryption Algorithm

```
1. Search for "Rich" signature (0x68636952) between DOS stub and PE header
2. Extract XOR key from 4 bytes following "Rich"
3. Search backwards for encrypted "DanS" (DANS_SIGNATURE ^ xor_key)
4. XOR decrypt all DWORDs between "DanS" and "Rich"
5. Verify decrypted header starts with "DanS" signature
6. Parse tool entries (skip padding zeros)
```

### Tests

Comprehensive test coverage in `tests/pe_rich_header_test.rs`:

- ✓ Rich Header parsing with sample data
- ✓ Compiler info string formatting
- ✓ Product ID extraction and deduplication
- ✓ Product name mapping
- ✓ Missing Rich Header handling
- ✓ MD5 hash calculation

All tests pass successfully.

## Example Output

```json
{
  "PE:RichHeaderPresent": "Yes",
  "PE:RichHeaderChecksum": "0x12345678",
  "PE:RichHeaderEntries": 2,
  "PE:RichCompilerInfo": "149.30729 x5, 154.30729 x1",
  "PE:RichProductIDs": "149, 154",
  "PE:RichHeaderHash": "a1b2c3d4e5f6..."
}
```

## Dependencies

Added `md5 = "0.7"` to Cargo.toml for hash calculation.

## References

- No official Microsoft documentation exists for the Rich Header
- Community research and reverse engineering efforts
- Used by security tools like PEiD, Detect It Easy, and VirusTotal
- Format documented through analysis of Visual Studio linker output

## Future Enhancements

Potential improvements:

1. Expand product ID mapping as new Visual Studio versions release
2. Add anomaly detection heuristics
3. Compare Rich Header against PE timestamp for consistency checks
4. Extract individual tool names in addition to IDs
