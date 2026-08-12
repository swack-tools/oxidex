# Known Discrepancies Between ExifTool-RS and Perl ExifTool

This document tracks acceptable differences in metadata extraction between ExifTool-RS and the reference Perl ExifTool implementation.

## Format: Tag Differences

### JPEG Format

**Maker Notes**
- **Status**: Partial support
- **Reason**: Maker notes are proprietary binary formats that vary by camera manufacturer. ExifTool-RS currently extracts maker note blocks but may not decode all vendor-specific tags.
- **Impact**: Lower match rates for images from Canon, Nikon, Sony cameras with extensive maker notes
- **Mitigation**: Documented in test corpus manifest; considered acceptable for v1.0

**EXIF UserComment Encoding**
- **Status**: Under investigation
- **Reason**: UserComment tag uses variable encoding (ASCII, JIS, Unicode). Character set detection may differ.
- **Impact**: Rare mismatches in comment tag values
- **Mitigation**: Added tolerance for string encoding variations

### TIFF Format

**Big-Endian vs Little-Endian**
- **Status**: Resolved
- **Reason**: Early implementation had byte-order bugs
- **Impact**: None (fixed in I2.T2)

### PNG Format

**Text Chunk Encoding**
- **Status**: Pending implementation
- **Reason**: PNG text chunks support Latin-1, UTF-8, and compressed formats. Full support planned for I6.
- **Impact**: May miss or incorrectly decode international text
- **Mitigation**: Tests will document expected encoding behavior

### PDF Format

**XMP Extraction**
- **Status**: Partial support
- **Reason**: PDF XMP packets can be deeply nested in object streams. Basic extraction works; complex scenarios may differ.
- **Impact**: Potential mismatches in PDFs with compressed object streams
- **Mitigation**: Test corpus focuses on simple XMP in PDF catalog

### MP4/QuickTime Format

**iTunes Metadata Atoms**
- **Status**: In progress
- **Reason**: iTunes uses custom atom types (©nam, ©ART, etc.) with variable encodings
- **Impact**: Possible mismatches in tag names or text encoding
- **Mitigation**: Tests use known-good MP4 files with standard iTunes tags

## Format: Value Representation Differences

### Floating-Point Numbers

**GPS Coordinates**
- **Tolerance**: ±0.0001 degrees (~11 meters)
- **Reason**: Floating-point rounding in rational-to-decimal conversion
- **Status**: Acceptable (GPS consumer accuracy is ~5-10 meters)

**Camera Settings (Aperture, Focal Length, etc.)**
- **Tolerance**: ±0.01
- **Reason**: Different rounding strategies in rational math
- **Status**: Acceptable (no practical impact)

### Date/Time Formats

**Timezone Handling**
- **Status**: Under investigation
- **Reason**: EXIF timestamps lack timezone; interpretation may vary
- **Impact**: Potential discrepancies in DateTimeOriginal rendering
- **Mitigation**: Tests compare raw values, not formatted strings

## Format: Structural Differences

### JSON Output Format

**TagValue Enum Serialization**
- **Status**: By design
- **Reason**: ExifTool-RS uses strongly-typed TagValue enum (String, Integer, Float, etc.). Perl ExifTool outputs bare values.
- **Example**:
  - Perl: `{"Make": "Canon"}`
  - Rust: `{"Make": {"String": "Canon"}}`
- **Impact**: Comparison logic unwraps enum wrappers
- **Mitigation**: `extract_value()` function handles unwrapping

### Group Names

**Group Prefixes**
- **Status**: Aligned
- **Reason**: Both tools use `-G1` flag for group prefixes (EXIF:, GPS:, XMP:)
- **Impact**: None (both outputs compatible)

## JPEG COM / DQT wiring (2026-07-19)

- **File:JPEGQualityEstimate is request-gated as of Step 21 (2026-08-12),
  superseding the "always emitted" note this replaces.** ExifTool computes
  this tag only when explicitly requested (`-JPEGQualityEstimate` or
  `RequestAll > 2`, `ExifTool.pm:7682-7692`); oxidex now models the same
  `REQ_TAG_LOOKUP` check (`core::read_options::ReadOptions`) and only
  computes/emits the tag when specifically requested or under
  `--extended-output`. Values still match ExifTool's algorithm exactly
  (JPEGDigest.pm EstimateQuality) -- confirmed against the pinned oracle:
  `-s -JPEGQualityEstimate` on `t/images/Canon.jpg` returns `61`, matching
  `oxidex -s -JPEGQualityEstimate` on the same file.
- **Multiple COM segments collapse to one File:Comment (last wins).** ExifTool
  reports each COM segment as a duplicate Comment tag under `-a`; MetadataMap
  stores one value per key.
- **Non-UTF-8 COM comments are stored as a binary blob.** When a COM segment's
  bytes are not valid UTF-8, oxidex stores `File:Comment` as a
  `TagValue::Binary` blob; ExifTool Latin-1-decodes the bytes into a string
  instead.
- **Duplicate "1 of 1" ICC profiles keep the first, not the last.** Files
  carrying multiple APP2 ICC_PROFILE segments each marked chunk 1 of 1 warn
  and keep the first profile's tags, matching ExifTool's behavior; the
  previous oxidex release silently kept the last.

## OxiDex's extended-output namespace (Step 21, 2026-08-12)

Prior to Step 21, oxidex emitted several tags in its *default* read mode
that real ExifTool never emits under any option -- not even `-u`
(Unknown). That made a precision comparison against the pinned oracle
unscoreable (AGENTS.md's output-contract/8.4): `tools/exiftool-tables/
conformance.py`'s EXTRA axis on `t/images/Canon.jpg` alone counted 13 such
tags before this step, 0 after.

These tags still exist -- they are useful for debugging oxidex itself --
but now require the CLI's `--extended-output` flag (or, for
`JPEGQualityEstimate` specifically, an explicit `-JPEGQualityEstimate`
request; see the DQT note above). The namespace:

| Tags | Source |
|---|---|
| `JPEG:Width`, `JPEG:Height`, `JPEG:ColorComponents` (a duplicate of `File:ColorComponents`), `JPEG:ComponentID_1..3`, `JPEG:YCbCrSubSampling_1..3`, `JPEG:SamplingFactors` | `src/parsers/jpeg/app_parsers.rs::parse_sof_segment_with_options` |
| The undecoded-MakerNote hex-fallback tag (`ExifIFD:0x927C` when no vendor parser or `MakerNoteUnknown*` condition claims the note) and, generally, any tag whose name is `tag_db::lookup_tag_name`'s bare-hex fallback (an id with no name in the generated database, in any IFD) | `src/core/tiff_helpers.rs` (source-computed unconditionally, for `src/writers/exif_surgical.rs`'s round-trip fidelity; filtered at the CLI display boundary by `core::read_options::ReadOptions::strip_extended_only`) |
| ZIP's per-entry forensic tags (`ZIP:File<N>:Filename`, `:CRC32`, `:CompressedSize`, ...) -- real ExifTool's `ZIP.pm` reports the unnumbered `Zip*` fields for the archive's first local file header only, never a per-entry breakdown | `src/parsers/archive/zip.rs`, filtered the same way as the hex-fallback tags above |

ZIP's *archive-level* forensic summary fields (`ZIP:TotalCompressedSize`,
`ZIP:CompressionRatio`, `ZIP:CreationDate`, `ZIP:CompressionMethod`, ...)
are a related but explicitly out-of-scope EXTRA source Step 21 did not
touch -- confirmed still present via `conformance.py --only ZIP.zip`. A
future step should fold them into the same namespace.

See `src/core/read_options.rs`'s module doc comment for the full design
(including why the hex-fallback/ZIP categories are filtered at the CLI
display boundary rather than gated at the source, unlike
`JPEGQualityEstimate` and the JPEG SOF tags).

## Testing Strategy

### Match Rate Thresholds

Per integration test plan (Section 4.1.1):

| Category | Threshold | Rationale |
|----------|-----------|-----------|
| Simple | 99% | Well-formed files with standard metadata |
| Complex | 99% | Multiple metadata formats (EXIF+XMP+IPTC) |
| Edge Cases | 95% | Unusual encodings, large files, rare tags |
| Malformed | 90% | Corrupted files; best-effort extraction |

### Exclusion Criteria

Tags excluded from comparison:
- `SourceFile`: Path-dependent
- `File:FileName`: Path-dependent
- `File:Directory`: Path-dependent
- `File:FileModifyDate`: Filesystem-dependent
- `File:FileAccessDate`: Filesystem-dependent
- `File:FileInodeChangeDate`: Filesystem-dependent

### Reporting

When tests fail due to known discrepancies:
1. Document the discrepancy in this file
2. Add `// KNOWN DISCREPANCY: <tag> - see KNOWN_DISCREPANCIES.md` comment in test
3. Adjust threshold if discrepancy is acceptable
4. Create GitHub issue if discrepancy should be fixed

## Version Tracking

| ExifTool-RS Version | Perl ExifTool Version | Overall Match Rate |
|---------------------|----------------------|-------------------|
| 0.1.0 | 12.70 | ~95% (3 test files) |
| 0.2.0 (planned) | 12.70 | Target: 98%+ (100+ files) |

## References

- [ExifTool Tag Names](https://exiftool.org/TagNames/index.html)
- [Integration Test Plan](../../docs/testing/integration_test_plan.md)
- [ExifTool JSON Format](https://exiftool.org/faq.html#Q10)

## Changelog

### 2025-10-30
- Initial document created for I5.T9
- Documented TagValue enum serialization difference
- Defined match rate thresholds
- Listed tag exclusions for path-dependent fields

---

**Note**: This is a living document. Update when new discrepancies are discovered or resolved.
