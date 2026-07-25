# PDF Parser ExifTool Parity Implementation Plan

**Date:** 2025-12-03
**Goal:** Add all 26 missing PDF tags to achieve ExifTool parity

## Current State

The PDF parser currently extracts 12 tags:
- PDFVersion, Linearized
- Title, Author, Subject, Keywords, Creator, Producer
- CreateDate, ModifyDate
- PageCount, MediaBox

## Tags to Implement (26 total)

### Module 1: Info Dictionary Extensions (2 tags)
File: `src/parsers/pdf/info_parser.rs`

| Tag | Description | Implementation |
|-----|-------------|----------------|
| `PDF:SourceModified` | Source modification date | Parse `/SourceModified` from Info dict |
| `PDF:Trapped` | Trapping status (True/False/Unknown) | Parse `/Trapped` from Info dict |

### Module 2: Root Dictionary Parser (3 tags)
File: `src/parsers/pdf/root_parser.rs` (NEW)

| Tag | Description | Implementation |
|-----|-------------|----------------|
| `PDF:Language` | Document language code | Parse `/Lang` from Root/Catalog |
| `PDF:PageLayout` | Page display layout | Parse `/PageLayout` from Root |
| `PDF:PageMode` | Initial view mode | Parse `/PageMode` from Root |

### Module 3: Encryption Parser (2 tags)
File: `src/parsers/pdf/encryption_parser.rs` (NEW)

| Tag | Description | Implementation |
|-----|-------------|----------------|
| `PDF:Encryption` | "Standard V2 128-bit" format | Parse `/Filter`, `/V`, `/Length` from Encrypt dict |
| `PDF:UserAccess` | Comma-separated permissions | Decode `/P` permission flags |

Permission flags to decode:
- Bit 3: Print
- Bit 4: Modify
- Bit 5: Copy
- Bit 6: Annotate
- Bit 9: Fill Forms
- Bit 10: Extract
- Bit 11: Assemble
- Bit 12: Print High-res

### Module 4: Structure Parser (3 tags)
File: `src/parsers/pdf/structure_parser.rs` (NEW)

| Tag | Description | Implementation |
|-----|-------------|----------------|
| `PDF:TaggedPDF` | Accessibility tagging | Check `/MarkInfo` -> `/Marked` in Root |
| `PDF:HasXFA` | XML Forms present | Check `/AcroForm` -> `/XFA` existence |
| `PDF:HasAcroForm` | Interactive forms | Check `/AcroForm` existence in Root |

### Module 5: Signature Parser (6 tags)
File: `src/parsers/pdf/signature_parser.rs` (NEW)

| Tag | Description | Implementation |
|-----|-------------|----------------|
| `PDF:SignerContactInfo` | Signer contact | Parse from signature dict |
| `PDF:SigningLocation` | Where signed | Parse `/Location` |
| `PDF:SigningDate` | When signed | Parse `/M` (modification date) |
| `PDF:SigningAuthority` | Signing authority | Parse `/Name` |
| `PDF:SigningReason` | Reason for signing | Parse `/Reason` |
| `PDF:AuthenticationTime` | Auth timestamp | Parse from signature reference |

### Module 6: Embedded Resources Parser (5 tags)
File: `src/parsers/pdf/resources_parser.rs` (NEW)

| Tag | Description | Implementation |
|-----|-------------|----------------|
| `PDF:EmbeddedImageWidth` | Image width | Parse `/Width` from XObject streams |
| `PDF:EmbeddedImageHeight` | Image height | Parse `/Height` from XObject streams |
| `PDF:EmbeddedImageFilter` | Compression filter | Parse `/Filter` (DCTDecode, etc.) |
| `PDF:EmbeddedImageColorSpace` | Color space | Parse `/ColorSpace` |
| `PDF:EmbeddedImage` | Binary image data | Extract stream data |

### Module 7: Permissions Parser (3 tags)
File: `src/parsers/pdf/permissions_parser.rs` (NEW)

| Tag | Description | Implementation |
|-----|-------------|----------------|
| `PDF:DocMDP` | Doc modification prevention | Parse `/Perms` -> `/DocMDP` |
| `PDF:FieldMDP` | Field modification prevention | Parse `/Perms` -> `/FieldMDP` |
| `PDF:UR3` | Usage rights signature | Parse `/Perms` -> `/UR3` |

## Implementation Tasks

### Task 1: Shared PDF Object Navigation (Foundation)
- Refactor `PdfContext` to be reusable across all parsers
- Add helper functions for navigating object references
- Add dictionary key extraction utilities

### Task 2: Info Dictionary Extensions
- Add SourceModified and Trapped parsing to existing info_parser.rs
- Simple extension of current functionality

### Task 3: Root Dictionary Parser
- Create root_parser.rs
- Parse Language, PageLayout, PageMode from catalog

### Task 4: Encryption Parser
- Create encryption_parser.rs
- Parse Encrypt dictionary
- Decode permission flags into human-readable list

### Task 5: Structure Detection Parser
- Create structure_parser.rs
- Check for TaggedPDF, XFA, AcroForm presence

### Task 6: Digital Signature Parser
- Create signature_parser.rs
- Navigate to signature dictionaries
- Extract signer info, dates, reasons

### Task 7: Embedded Resources Parser
- Create resources_parser.rs
- Find XObject image streams
- Extract image metadata (not binary data initially)

### Task 8: Permissions Parser
- Create permissions_parser.rs
- Parse DocMDP, FieldMDP, UR3 from Perms dictionary

### Task 9: Integration
- Update mod.rs to call all new parsers
- Merge all metadata into single MetadataMap
- Add comprehensive tests

## File Structure After Implementation

```
src/parsers/pdf/
├── mod.rs                  # Main entry point (updated)
├── info_parser.rs          # Info dict (updated)
├── xmp_extractor.rs        # XMP (existing)
├── root_parser.rs          # NEW: Root/Catalog parsing
├── encryption_parser.rs    # NEW: Encryption & permissions
├── structure_parser.rs     # NEW: Tagged, XFA, AcroForm
├── signature_parser.rs     # NEW: Digital signatures
├── resources_parser.rs     # NEW: Embedded images
├── permissions_parser.rs   # NEW: DocMDP, FieldMDP, UR3
└── shared.rs               # NEW: Shared utilities
```

## Parallel Agent Assignment

| Agent | Task | Files |
|-------|------|-------|
| Agent 1 | Shared utilities + Info extensions | shared.rs, info_parser.rs |
| Agent 2 | Root + Structure parsers | root_parser.rs, structure_parser.rs |
| Agent 3 | Encryption parser | encryption_parser.rs |
| Agent 4 | Signature + Permissions parsers | signature_parser.rs, permissions_parser.rs |
| Agent 5 | Resources parser + Integration | resources_parser.rs, mod.rs |

## Testing Strategy

Each module should include:
1. Unit tests with synthetic PDF data
2. Test helper for creating minimal PDF structures
3. Edge case handling (missing dictionaries, malformed data)

## Success Criteria

- All 26 new tags extractable from appropriate PDFs
- No external PDF parsing libraries used
- All existing tests continue to pass
- Coverage report shows PDF at 60%+ (up from 20%)
