# Implementation Roadmap - exiftool-rs

> **Last Updated:** November 16, 2025
> **Current Version:** v1.0.6
> **Overall Parity:** 62.5% tag group coverage (20/32 groups)

## Executive Summary

exiftool-rs has achieved **excellent parity** with Perl ExifTool for basic use cases (web, video, documents, hobbyist photography). However, **two critical features** are blocking professional adoption:

1. **IPTC Metadata** - Required for journalism and stock photography
2. **MakerNotes** - Required for professional photography (camera-specific data)

With these features implemented, exiftool-rs will be **production-ready for professional workflows**.

---

## Current Status

### ✅ What Works Excellently

| Feature | Coverage | Status | Notes |
|---------|----------|--------|-------|
| **Core EXIF** | 100% | ✅ Complete | All IFD0, IFD1, ExifIFD, GPS tags |
| **XMP** | 95%+ | ✅ Complete | 10 namespaces supported |
| **PDF** | 100% | ✅ Complete | Info dict + ICC profiles |
| **PNG** | 95% | ✅ Complete | All major chunks |
| **MP4/QuickTime** | 76% | ✅ Good | Basic + ItemList metadata |
| **ICC Profiles** | 100% | ✅ Complete | Full header + tag parsing |
| **File Metadata** | 100% | ✅ Complete | All OS-level attributes |

### ❌ Critical Gaps

| Feature | Impact | Blocks | Priority |
|---------|--------|--------|----------|
| **IPTC** | CRITICAL | Journalism, stock photography | P0 |
| **MakerNotes** | CRITICAL | Professional photography | P0 |
| **Composite Tags** | MEDIUM | User experience | P1 |
| **RAW Formats** | HIGH | Professional workflows | P2 |

### ⚠️ Minor Gaps

- InteropIFD tags (rarely used)
- PrintIM tags (niche use case)
- Some video codec-specific metadata
- JFIF thumbnail extraction

---

## Priority 0: IPTC Metadata Support

### Overview

IPTC (International Press Telecommunications Council) is the industry standard for photojournalism and stock photography metadata. Used by news agencies, stock photo sites, and media organizations worldwide.

### Use Cases

- **Journalism:** Captions, credits, copyright, locations
- **Stock Photography:** Keywords, categories, licensing
- **Media Asset Management:** Searchability, rights management
- **Publishing:** Automated workflows

### Technical Implementation

#### IPTC Binary Format (IIM - Information Interchange Model)

**Location in JPEG:**
- Stored in APP13 segment (Photoshop IRB - Image Resource Block)
- Marker: `0xFFED`
- Signature: "Photoshop 3.0\0"
- Contains 8BIM resources, including IPTC IIM data

**Binary Structure:**
```
APP13 Segment:
  [0xFF, 0xED] - Marker
  [length] - 2 bytes, big-endian
  "Photoshop 3.0\0" - 14 bytes

  Image Resource Block(s):
    Type: "8BIM" - 4 bytes
    ID: 0x0404 (IPTC) - 2 bytes
    Name: Pascal string (padded to even)
    Size: 4 bytes
    Data: IPTC IIM records
```

**IPTC IIM Record Format:**
```
Each record:
  Tag marker: 0x1C - 1 byte
  Record number: 1 or 2 - 1 byte
  Dataset number: varies - 1 byte
  Data length: 2 bytes (or extended)
  Data: variable length
```

#### Common IPTC Tags to Implement

**Record 2 (Application Record):**
- Dataset 5: ObjectName (Title)
- Dataset 25: Keywords (array)
- Dataset 40: SpecialInstructions
- Dataset 55: DateCreated
- Dataset 60: TimeCreated
- Dataset 80: ByLine (Creator)
- Dataset 85: ByLineTitle (Creator's job title)
- Dataset 90: City
- Dataset 95: Province/State
- Dataset 100: Country/PrimaryLocationCode
- Dataset 101: Country/PrimaryLocationName
- Dataset 103: OriginalTransmissionReference
- Dataset 105: Headline
- Dataset 110: Credit
- Dataset 115: Source
- Dataset 116: CopyrightNotice
- Dataset 120: Caption/Abstract
- Dataset 122: Writer/Editor

#### Implementation Files

**Create:**
- `src/parsers/jpeg/iptc_parser.rs` - IPTC IIM parser

**Modify:**
- `src/parsers/jpeg/mod.rs` - Call IPTC parser from APP13 segment
- `src/parsers/jpeg/segment_parser.rs` - Extract APP13 segments

#### Implementation Steps

1. **Parse APP13 Segment** (1-2 days)
   - Locate APP13 markers in JPEG
   - Verify "Photoshop 3.0" signature
   - Extract 8BIM resources

2. **Parse Image Resource Blocks** (1-2 days)
   - Find 8BIM resource with ID 0x0404 (IPTC)
   - Extract IPTC IIM data block

3. **Parse IPTC IIM Records** (2-3 days)
   - Implement record/dataset parser
   - Handle variable-length encoding
   - Support extended length format (>32767 bytes)

4. **Map to Metadata Tags** (1-2 days)
   - Map datasets to tag names
   - Handle arrays (Keywords)
   - Convert encodings (Latin1, UTF-8)

5. **Testing** (2-3 days)
   - Test with journalism samples
   - Test with stock photo samples
   - Verify against Perl ExifTool output

#### Test Files

- Download IPTC samples from: https://www.iptc.org/std/photometadata/examples/
- Use journalism photos with captions
- Test with stock photo metadata

#### Effort Estimate

- **Development:** 7-10 days
- **Testing:** 2-3 days
- **Total:** 2-3 weeks

#### Expected Impact

- Unlocks journalism workflows ✅
- Unlocks stock photography ✅
- Enables media asset management ✅
- Supports publishing workflows ✅

---

## Priority 0: MakerNotes Support (Canon)

### Status: Phase 1 Complete ✅

**Completed:** Basic Canon MakerNotes tag extraction (ImageType, FirmwareVersion, OwnerName, SerialNumber, ModelID, FileNumber)
**Next:** Phase 2 - Complex array tags (CameraSettings, ShotInfo, AFInfo, LensInfo)

### Overview

MakerNotes contain camera-specific metadata that manufacturers embed in proprietary formats. Canon is the largest professional camera manufacturer, making Canon MakerNotes the highest priority.

### Use Cases

- **Professional Photography:** Detailed camera settings for review
- **Lens Information:** Auto-detected lens model, serial numbers
- **AF System:** Focus points used, AF mode
- **Image Processing:** Picture style, white balance fine-tuning
- **Firmware:** Camera firmware version

### Technical Implementation

#### Canon MakerNotes Format

**Location in JPEG:**
- Stored in EXIF MakerNote tag (0x927C) in ExifIFD
- Canon uses standard TIFF IFD structure (easier than Nikon!)

**Structure:**
```
MakerNote Tag (0x927C):
  Byte order: Same as EXIF (usually little-endian)
  IFD structure:
    Entry count: 2 bytes
    Entries: 12 bytes each (tag, type, count, value/offset)
    Next IFD offset: 4 bytes (usually 0)
```

**Canon-Specific Tags:**
- 0x0001: CameraSettings (array of settings)
- 0x0002: FocalLength
- 0x0003: FlashInfo
- 0x0004: ShotInfo (ISO, focus, flash, etc.)
- 0x0005: Panorama
- 0x0006: ImageType
- 0x0007: FirmwareVersion
- 0x0008: FileNumber
- 0x0009: OwnerName
- 0x000A: UnknownD30
- 0x000C: CameraSerialNumber
- 0x000D: CameraInfo (varies by model)
- 0x000E: FileLength
- 0x0010: ModelID
- 0x0013: ThumbnailImageValidArea
- 0x0015: SerialNumberFormat
- 0x001A: SuperMacro
- 0x0026: AFInfoArray
- 0x0083: OriginalDecisionDataOffset
- 0x0095: LensModel
- 0x0096: InternalSerialNumber
- 0x0097: DustRemovalData
- 0x0098: CropInfo
- 0x009A: AspectInfo
- 0x00A0: ProcessingInfo
- 0x00AA: MeasuredColor
- 0x00B4: ColorSpace
- 0x00E0: SensorInfo
- 0x4001: ColorData (varies by model, complex)

#### Implementation Files

**Create:**
- `src/parsers/tiff/makernotes/mod.rs` - MakerNotes dispatcher
- `src/parsers/tiff/makernotes/canon.rs` - Canon-specific parser
- `src/parsers/tiff/makernotes/canon_tags.rs` - Canon tag definitions

**Modify:**
- `src/parsers/tiff/ifd_parser.rs` - Detect and parse MakerNote tag
- Add Canon tag database (YAML or hardcoded)

#### Implementation Steps

1. **MakerNote Detection** (1 day)
   - Detect MakerNote tag (0x927C) in ExifIFD
   - Identify manufacturer from EXIF Make tag
   - Route to appropriate parser

2. **Canon IFD Parsing** (2-3 days)
   - Parse Canon MakerNote as TIFF IFD
   - Handle byte order (same as parent EXIF)
   - Extract tag entries

3. **Canon Tag Decoding** (5-7 days)
   - Implement decoders for major tags:
     - CameraSettings (0x0001) - Array of 46+ values
     - ShotInfo (0x0004) - Array of settings
     - CameraInfo (0x000D) - Model-specific, complex
     - AFInfo (0x0026) - Focus point data
     - ColorData (0x4001) - Color calibration (model-specific)

4. **Lens Database** (2-3 days)
   - Create Canon lens ID → name mapping
   - ~400+ Canon lenses to map
   - Use ExifTool's Canon lens database

5. **Testing** (3-5 days)
   - Test with Canon EOS cameras (5D, 6D, 7D, R5, R6, etc.)
   - Verify against Perl ExifTool
   - Test different Canon models (tags vary by model!)

#### Resources

- ExifTool Canon.pm source: https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/Canon.pm
- Canon MakerNote documentation: Various reverse-engineering sources
- Sample images: https://raw.pixls.us/ (filter by Canon)

#### Effort Estimate

- **Development:** 10-15 days
- **Lens database creation:** 2-3 days
- **Testing:** 3-5 days
- **Total:** 4-6 weeks

#### Phased Approach

**Phase 1 (2 weeks):** Basic Canon support
- Parse MakerNote IFD
- Extract simple tags (firmware, serial, lens model)
- Proof of concept

**Phase 2 (2 weeks):** Complex tags
- CameraSettings array decoding
- ShotInfo array decoding
- AFInfo parsing

**Phase 3 (2 weeks):** Model-specific
- CameraInfo (varies by model)
- ColorData (varies by model)
- Lens database integration

#### Expected Impact

- Unlocks professional Canon users ✅
- Provides detailed camera settings ✅
- Enables lens identification ✅
- Shows AF system data ✅

---

## Priority 1: Composite Tags

### Overview

Composite tags are calculated/derived values that make metadata more user-friendly. Perl ExifTool calculates these on-the-fly for better UX.

### Examples

**Instead of raw values:**
```
FNumber: 71/10
ShutterSpeed: 1/500
FocalLength: 50/1
Flash: 9
```

**Show human-readable:**
```
Aperture: f/7.1
ShutterSpeed: 1/500
FocalLength: 50mm
Flash: On, Fired
```

### Common Composite Tags

1. **Aperture** = sqrt(2^FNumber) → "f/7.1"
2. **ShutterSpeed** = Rational to fraction → "1/500"
3. **FocalLength** = Add "mm" unit
4. **Flash** = Decode bitmask → "On, Fired, Red-eye reduction"
5. **ImageSize** = Width × Height → "6000x4000"
6. **Megapixels** = (Width × Height) / 1,000,000 → "24.0"
7. **CircleOfConfusion** = Based on sensor size
8. **DOF** = Depth of field calculation
9. **HyperfocalDistance** = Based on aperture/focal length
10. **LightValue** = EV calculation

### Implementation

#### Simple Composites (1 week)

Create `src/core/composite_tags.rs`:

```rust
pub fn calculate_aperture(fnumber: f64) -> String {
    format!("f/{:.1}", fnumber)
}

pub fn format_shutter_speed(numerator: u32, denominator: u32) -> String {
    if numerator == 1 {
        format!("1/{}", denominator)
    } else {
        format!("{}/{}", numerator, denominator)
    }
}

pub fn decode_flash(value: u16) -> String {
    let mut parts = Vec::new();
    if value & 0x01 != 0 { parts.push("Fired"); }
    if value & 0x02 != 0 { parts.push("Return detected"); }
    if value & 0x04 != 0 { parts.push("Fill-in"); }
    // ... etc
    parts.join(", ")
}
```

#### Complex Composites (1 week)

Calculations requiring multiple inputs:
```rust
pub fn calculate_megapixels(width: u32, height: u32) -> f64 {
    (width as f64 * height as f64) / 1_000_000.0
}

pub fn calculate_dof(
    focal_length: f64,
    aperture: f64,
    distance: f64,
    coc: f64
) -> (f64, f64) {
    // Near/far DOF calculation
}
```

### Effort Estimate

- **Simple composites:** 1 week
- **Complex composites:** 1 week
- **Testing:** 3-5 days
- **Total:** 2-3 weeks

### Expected Impact

- Better user experience ✅
- Matches Perl ExifTool output format ✅
- Easier to read metadata ✅

---

## Priority 2: Additional MakerNotes

After Canon, implement in this order:

### Nikon (3-4 weeks)
- Second largest pro market
- More complex than Canon (encrypted sections!)
- ~300+ lens database entries

### Sony (2-3 weeks)
- Growing pro market
- Simpler than Nikon
- ~200+ lens database entries

### Panasonic (1-2 weeks)
- Strong in video/hybrid cameras
- Relatively simple format

---

## Priority 3: RAW Format Support

### Overview

RAW formats are essentially TIFF files with proprietary extensions. Most metadata can be extracted using existing TIFF parser with minor modifications.

### Common RAW Formats

| Format | Manufacturer | Extension | Difficulty |
|--------|--------------|-----------|------------|
| CR2/CR3 | Canon | .cr2, .cr3 | Medium |
| NEF | Nikon | .nef | Easy (TIFF-based) |
| ARW | Sony | .arw | Easy (TIFF-based) |
| DNG | Adobe (standard) | .dng | Easy (TIFF-based) |
| ORF | Olympus | .orf | Medium |
| RAF | Fujifilm | .raf | Medium |
| RW2 | Panasonic | .rw2 | Easy (TIFF-based) |

### Implementation Approach

1. **DNG (Adobe Digital Negative)** - Start here (1 week)
   - Open standard, well-documented
   - Used by many cameras as native format
   - TIFF-based with known extensions

2. **NEF (Nikon)** - Next (1 week)
   - TIFF-based
   - Straightforward IFD structure

3. **ARW (Sony)** - Next (1 week)
   - TIFF-based
   - Similar to NEF

4. **CR2 (Canon)** - More complex (2 weeks)
   - TIFF-based but with Canon-specific structures
   - Multiple IFDs with different purposes

### Effort Estimate

- **DNG + NEF + ARW:** 3-4 weeks
- **CR2/CR3:** 2-3 weeks
- **Others:** 1-2 weeks each
- **Total for major formats:** 2-3 months

---

## Testing Strategy

### Test File Sources

1. **Sample Images:**
   - https://raw.pixls.us/ - Professional RAW samples
   - https://www.iptc.org/std/photometadata/examples/ - IPTC samples
   - https://sample-videos.com/ - Video samples
   - https://filesamples.com/ - Various format samples

2. **Camera-Specific:**
   - Request samples from community
   - Use manufacturer sample galleries
   - Download from photography forums

3. **Automated Testing:**
   - Create test fixtures for each format
   - Compare against Perl ExifTool output
   - Regression tests for all features

### Continuous Integration

Add to CI pipeline:
```yaml
- name: Compare with Perl ExifTool
  run: |
    for file in tests/fixtures/**/*; do
      ./scripts/compare_with_perl.sh "$file"
    done
```

### Regression Detection

Script to detect regressions:
```bash
#!/bin/bash
# scripts/compare_with_perl.sh

FILE=$1
PERL_OUT=$(exiftool -a -G1 "$FILE")
RUST_OUT=$(target/release/exiftool-rs "$FILE")

# Compare field counts
PERL_COUNT=$(echo "$PERL_OUT" | wc -l)
RUST_COUNT=$(echo "$RUST_OUT" | wc -l)

if [ $RUST_COUNT -lt $(($PERL_COUNT * 80 / 100)) ]; then
    echo "❌ REGRESSION: $FILE has <80% parity"
    exit 1
fi
```

---

## Success Metrics

### Quantitative Goals

| Metric | Current | Target (3 months) | Target (6 months) |
|--------|---------|-------------------|-------------------|
| Tag group coverage | 62.5% | 80% | 90% |
| Professional readiness | 40% | 80% | 95% |
| JPEG tag extraction | 36-53% | 75% | 85% |
| Supported formats | 6 | 10 | 15 |

### Qualitative Goals

- [ ] Used in production by journalism outlets
- [ ] Used by stock photography platforms
- [ ] Recommended by professional photographers
- [ ] Feature parity for 80% of use cases
- [ ] Performance 2-5x faster than Perl ExifTool

---

## Resource Requirements

### Development Time (Full-time equivalent)

- **IPTC:** 2-3 weeks
- **Canon MakerNotes:** 4-6 weeks
- **Composite Tags:** 2-3 weeks
- **Nikon MakerNotes:** 3-4 weeks
- **RAW Formats:** 2-3 months
- **Total:** 4-5 months for professional readiness

### Expertise Needed

- Rust development (intermediate)
- Binary format parsing
- Photography/metadata domain knowledge
- Reverse engineering (for MakerNotes)
- Testing/QA

---

## Risk Assessment

### Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Canon MakerNotes complexity | Medium | High | Phased approach, start with simple tags |
| Encrypted MakerNotes (Nikon) | High | Medium | Skip encrypted sections initially |
| Format documentation gaps | Medium | Medium | Community reverse engineering, ExifTool source |
| Performance regression | Low | Medium | Continuous benchmarking |
| Breaking changes | Low | High | Comprehensive test suite |

### Market Risks

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Perl ExifTool remains dominant | Medium | High | Focus on performance + ease of use |
| Manufacturer format changes | Low | Medium | Active maintenance |
| Professional adoption slow | Medium | Medium | Community engagement, marketing |

---

## Community Engagement

### Contribution Opportunities

Label GitHub issues with:
- `good-first-issue` - Composite tag calculations
- `help-wanted` - MakerNotes reverse engineering
- `documentation` - Tag database expansion

### Documentation Needed

1. **Contributor Guide** - How to add support for new formats
2. **Tag Database Guide** - How to add tags to YAML database
3. **Testing Guide** - How to create test fixtures
4. **MakerNotes Guide** - How to reverse engineer camera-specific data

---

## Next Actions

### Immediate (This Week)

1. ✅ Create this implementation roadmap
2. ✅ Implement IPTC parser
3. ✅ Canon MakerNotes Phase 1 implementation
4. Create GitHub issues for:
   - Canon MakerNotes Phase 2 (#6)
   - Composite tags (#7)
5. Set up project board with milestones
6. Recruit contributors for lens database creation

### This Month

1. ✅ Implement IPTC parser
2. ✅ Create test fixtures for IPTC samples
3. ✅ Canon MakerNotes Phase 1 - Basic tag support (ImageType, FirmwareVersion, OwnerName, SerialNumber, ModelID, FileNumber)
4. Begin Canon MakerNotes Phase 2 (complex arrays and camera settings)
5. Create lens database schema

### Next 3 Months

1. ✅ Complete IPTC support
2. Complete Canon MakerNotes (all phases)
3. Implement Composite tags
4. Begin Nikon MakerNotes
5. Release v2.0.0 (professional ready)

### Next 6 Months

1. Complete Nikon + Sony MakerNotes
2. Add RAW format support (DNG, NEF, ARW, CR2)
3. Expand lens databases
4. Professional workflow integration guides
5. Release v3.0.0 (comprehensive pro support)

---

## Appendix: Reference Documentation

### ExifTool Source Code

- Canon.pm: https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/Canon.pm
- IPTC.pm: https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/IPTC.pm
- Nikon.pm: https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/Nikon.pm
- Composite.pm: https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/Composite.pm

### Format Specifications

- EXIF 2.32: https://www.cipa.jp/std/documents/e/DC-008-Translation-2019-E.pdf
- IPTC IIM: https://www.iptc.org/std/IIM/4.2/specification/IIMV4.2.pdf
- TIFF 6.0: https://www.adobe.io/content/dam/udp/en/open/standards/tiff/TIFF6.pdf
- XMP Specification: https://www.adobe.com/devnet/xmp.html
- ICC Profile Format: https://www.color.org/specification/ICC.1-2022-05.pdf

### Sample Files & Testing

- Raw Samples: https://raw.pixls.us/
- IPTC Examples: https://www.iptc.org/std/photometadata/examples/
- ExifTool Test Files: https://github.com/exiftool/exiftool/tree/master/t/images

---

**Document Version:** 1.0
**Author:** Auto-generated from parity analysis
**Next Review:** After IPTC implementation
