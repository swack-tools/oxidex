# Forensic & Cybersecurity Tag Coverage Implementation Plan

**Date**: 2025-12-03
**Goal**: Increase tag coverage for digital forensics and cybersecurity use cases
**Priority Focus**: Timeline reconstruction, device identification, tamper detection, executable analysis

---

## Executive Summary

This plan prioritizes metadata tags critical for:
- **Timeline Reconstruction**: Timestamps across all formats with timezone support
- **Device Identification**: Camera/device serial numbers, hardware fingerprints
- **Tamper Detection**: Edit history, software provenance, manipulation indicators
- **Executable Analysis**: Code signing, certificates, compilation artifacts
- **Geolocation Evidence**: GPS coordinates, location metadata

---

## Current State Analysis

### Strong Coverage (75%+)
| Format | Tags | Forensic Value |
|--------|------|----------------|
| PE | ~70 tags | Timestamps, certificates, debug info, Rich header |
| ELF | ~40 tags | Build info, symbols, dynamic linking |
| Mach-O | ~45 tags | UUID, code signing, version info |
| PDF | ~38 tags | Encryption, signatures, permissions |
| Audio | 100% | ID3, FLAC, Vorbis complete |

### Needs Improvement (20-60%)
| Format | Current | Gap | Priority |
|--------|---------|-----|----------|
| EXIF/TIFF | 90% | Missing timezone offsets, subsecond times | **Critical** |
| GPS | ~60% | Missing track/speed/direction tags | **Critical** |
| XMP | 60% | Missing edit history (photoshop:History) | **High** |
| QuickTime | 40% | Missing forensic timestamps, location | **High** |
| Office | 60% | Missing revision history, hidden data | **Medium** |

---

## Phase 1: Critical Forensic Tags (Week 1-2)

### 1.1 EXIF Timezone & Subsecond Precision

**Files**: `src/parsers/tiff/tag_parser.rs`, `oxidex-tags-core/src/exif.rs`

| Tag | Tag ID | Description | Forensic Value |
|-----|--------|-------------|----------------|
| OffsetTime | 0x9010 | Timezone for ModifyDate | Timeline correlation |
| OffsetTimeOriginal | 0x9011 | Timezone for DateTimeOriginal | Capture location inference |
| OffsetTimeDigitized | 0x9012 | Timezone for CreateDate | Digitization location |
| SubSecTime | 0x9290 | Subsecond for ModifyDate | Precise sequencing |
| SubSecTimeOriginal | 0x9291 | Subsecond for DateTimeOriginal | Burst photo ordering |
| SubSecTimeDigitized | 0x9292 | Subsecond for CreateDate | Frame-accurate timing |

**Implementation**:
```rust
// In tag_parser.rs - add these EXIF tags
0x9010 => "EXIF:OffsetTime",
0x9011 => "EXIF:OffsetTimeOriginal",
0x9012 => "EXIF:OffsetTimeDigitized",
0x9290 => "EXIF:SubSecTime",
0x9291 => "EXIF:SubSecTimeOriginal",
0x9292 => "EXIF:SubSecTimeDigitized",
```

### 1.2 GPS Movement & Tracking Tags

**Files**: `src/parsers/tiff/tag_parser.rs`, `oxidex-tags-core/src/gps.rs`

| Tag | Tag ID | Description | Forensic Value |
|-----|--------|-------------|----------------|
| GPSTrack | 0x000F | Direction of movement | Vehicle/person tracking |
| GPSTrackRef | 0x000E | True/Magnetic north | Direction accuracy |
| GPSSpeed | 0x000D | Speed of movement | Velocity analysis |
| GPSSpeedRef | 0x000C | Speed unit (K/M/N) | Unit conversion |
| GPSImgDirection | 0x0011 | Camera pointing direction | Field of view |
| GPSImgDirectionRef | 0x0010 | True/Magnetic reference | Direction accuracy |
| GPSDestBearing | 0x0018 | Bearing to destination | Navigation analysis |
| GPSDestDistance | 0x001A | Distance to destination | Proximity analysis |
| GPSHPositioningError | 0x001F | Horizontal accuracy (m) | Location reliability |

### 1.3 Device Serial Numbers

**Files**: `src/parsers/tiff/tag_parser.rs`, MakerNote parsers

| Tag | Location | Description | Forensic Value |
|-----|----------|-------------|----------------|
| SerialNumber | EXIF 0xC62F | Camera body serial | Device attribution |
| LensSerialNumber | EXIF 0xA435 | Lens serial number | Equipment tracking |
| BodySerialNumber | MakerNotes | Alternative body serial | Cross-reference |
| InternalSerialNumber | Canon/Nikon | Internal device ID | Hidden identifier |
| ImageUniqueID | EXIF 0xA420 | Unique image hash | Image tracking |
| OwnerName | EXIF 0xA430 | Camera owner | Attribution |

---

## Phase 2: Tamper Detection & Edit History (Week 2-3)

### 2.1 XMP Edit History

**New File**: `src/parsers/xmp/history_parser.rs`

| Tag | XMP Path | Description | Forensic Value |
|-----|----------|-------------|----------------|
| HistoryAction | xmpMM:History/stEvt:action | Edit action type | Modification tracking |
| HistoryWhen | xmpMM:History/stEvt:when | Edit timestamp | Timeline reconstruction |
| HistorySoftwareAgent | xmpMM:History/stEvt:softwareAgent | Software used | Tool identification |
| HistoryChanged | xmpMM:History/stEvt:changed | What was changed | Change scope |
| HistoryInstanceID | xmpMM:History/stEvt:instanceID | Version identifier | Version tracking |
| HistoryParameters | xmpMM:History/stEvt:parameters | Action parameters | Detailed changes |
| DerivedFromDocumentID | xmpMM:DerivedFrom/stRef:documentID | Source document | Provenance chain |
| DerivedFromInstanceID | xmpMM:DerivedFrom/stRef:instanceID | Source version | Version lineage |
| DerivedFromOriginalDocumentID | xmpMM:DerivedFrom/stRef:originalDocumentID | Original source | Full provenance |
| DocumentAncestors | xmpMM:Pantry | Document history | Complete lineage |

**Implementation**:
```rust
// New struct for XMP history entries
pub struct XmpHistoryEntry {
    pub action: String,           // saved, created, converted, derived
    pub when: Option<String>,     // ISO 8601 timestamp
    pub software_agent: Option<String>,
    pub changed: Option<String>,  // /metadata, /content
    pub instance_id: Option<String>,
    pub parameters: Option<String>,
}

// Parse xmpMM:History array
fn parse_xmp_history(xmp_data: &str) -> Vec<XmpHistoryEntry> {
    // Parse RDF/XML structure for stEvt: elements
}
```

### 2.2 Software Provenance Tags

| Tag | Format | Description | Forensic Value |
|-----|--------|-------------|----------------|
| ProcessingSoftware | EXIF 0x000B | Post-processing software | Edit detection |
| CreatorTool | XMP | Creating application | Origin verification |
| HistorySoftwareAgent | XMP | Each edit's software | Full edit chain |
| Producer | PDF | PDF generator | Document origin |
| Application | Office | Office application | Document source |
| AppVersion | Office | Application version | Tool fingerprint |

### 2.3 Thumbnail Mismatch Detection

**New File**: `src/parsers/jpeg/thumbnail_analyzer.rs`

| Check | Description | Forensic Value |
|-------|-------------|----------------|
| ThumbnailDimensionMismatch | Thumbnail vs main image ratio | Cropping detection |
| ThumbnailHashMismatch | Visual similarity check | Content replacement |
| ThumbnailMetadataMismatch | EXIF in thumbnail vs main | Metadata tampering |
| PreviewImageMismatch | Preview vs main comparison | Larger tampering |

---

## Phase 3: QuickTime/Video Forensics (Week 3-4)

### 3.1 QuickTime Timestamp Tags

**Files**: `src/parsers/quicktime/metadata_extractor.rs`

| Tag | Atom Path | Description | Forensic Value |
|-----|-----------|-------------|----------------|
| MediaCreateDate | moov/mvhd creation_time | Media creation UTC | Timeline anchor |
| MediaModifyDate | moov/mvhd modification_time | Last modification UTC | Edit detection |
| TrackCreateDate | moov/trak/tkhd creation_time | Track creation | Multi-track analysis |
| TrackModifyDate | moov/trak/tkhd modification_time | Track modification | Splice detection |
| ContentCreateDate | moov/udta/©day | Content creation | User-set date |
| CreationDate | moov/meta/keys/mdta | iPhone creation date | iOS specifics |

### 3.2 QuickTime Location Tags

| Tag | Atom Path | Description | Forensic Value |
|-----|-----------|-------------|----------------|
| GPSCoordinates | moov/meta/keys/com.apple.quicktime.location.ISO6709 | Full GPS string | Location evidence |
| LocationAccuracyHorizontal | moov/meta/keys/com.apple.quicktime.location.accuracy.horizontal | GPS accuracy | Reliability |
| LocationRole | moov/meta/keys/com.apple.quicktime.location.role | Location purpose | Context |
| LocationBody | moov/meta/keys/com.apple.quicktime.location.body | Planet (Earth) | Validation |
| CreationLocationName | moov/meta/keys/com.apple.quicktime.creationLocation.name | Reverse geocode | Human-readable |

### 3.3 Video Metadata

| Tag | Description | Forensic Value |
|-----|-------------|----------------|
| Duration | Video length | Timeline bounds |
| FrameRate | Frames per second | Frame analysis |
| VideoCodec | Encoding method | Re-encoding detection |
| AudioCodec | Audio encoding | Audio tampering |
| BitRate | Encoding quality | Quality analysis |

---

## Phase 4: Document Forensics (Week 4-5)

### 4.1 Office Document Hidden Metadata

**Files**: `src/parsers/document/office_parser.rs`

| Tag | Location | Description | Forensic Value |
|-----|----------|-------------|----------------|
| RevisionNumber | core.xml | Save count | Edit frequency |
| TotalEditTime | app.xml | Total edit time | Creation effort |
| LastModifiedBy | core.xml | Last editor | Attribution |
| Company | app.xml | Organization | Source identification |
| Manager | app.xml | Manager name | Organizational context |
| Template | app.xml | Template used | Document origin |
| HyperlinkBase | app.xml | Base URL | Network context |
| HiddenSlides | app.xml | Hidden slide count | Concealed content |
| Notes | slides/*.xml | Speaker notes | Hidden information |
| Comments | comments.xml | Review comments | Collaboration trail |

### 4.2 PDF Enhanced Metadata

| Tag | Description | Forensic Value |
|-----|-------------|----------------|
| ModDate | Modification date | Edit detection |
| MetadataDate | XMP metadata date | Metadata tampering |
| DocumentHistory | XMP history | Full edit trail |
| AccessPermissions | Full permission flags | Security analysis |

---

## Phase 5: Enhanced Executable Analysis (Week 5-6)

### 5.1 PE Rich Header Deep Analysis

**Files**: `src/parsers/pe/rich_header_parser.rs`

| Tag | Description | Forensic Value |
|-----|-------------|----------------|
| RichHeaderHash | Checksum of Rich data | Build fingerprint |
| RichHeaderEntries | All @comp.id entries | Compiler/linker versions |
| RichHeaderVSVersion | Visual Studio version | Development environment |
| RichHeaderObjectCount | Objects linked | Build complexity |
| RichHeaderImportCount | Import library count | Dependencies |

### 5.2 PE Anomaly Detection

**Files**: `src/parsers/pe/anomaly_detector.rs` (enhance existing)

| Check | Description | Forensic Value |
|-------|-------------|----------------|
| TimestampInFuture | Compilation date > now | Timestamp manipulation |
| TimestampTooOld | Compilation < 1990 | Invalid timestamp |
| SectionEntropy | High entropy sections | Packed/encrypted code |
| ImportHashMismatch | Import hash anomalies | IAT hooking |
| CertificateChainIncomplete | Missing CA certs | Unsigned effectively |
| DebugPathLeakage | PDB path disclosure | Build environment leak |

### 5.3 ELF/Mach-O Enhancements

| Tag | Format | Description | Forensic Value |
|-----|--------|-------------|----------------|
| BuildID | ELF | GNU Build ID | Binary identification |
| BuildTimestamp | ELF | Build time if available | Timeline |
| GoVersion | ELF/Mach-O | Go compiler version | Language detection |
| RustVersion | ELF/Mach-O | Rust compiler version | Language detection |
| CodeSigningFlags | Mach-O | Signature details | Security analysis |
| TeamIdentifier | Mach-O | Developer team ID | Attribution |
| SigningIdentity | Mach-O | Signing certificate | Code origin |

---

## Phase 6: Communication Metadata (Week 6)

### 6.1 Email/TNEF Parser (New)

**New Files**: `src/parsers/email/mod.rs`, `src/parsers/tnef/mod.rs`

| Tag | Description | Forensic Value |
|-----|-------------|----------------|
| From | Sender address | Attribution |
| To | Recipient(s) | Distribution |
| Subject | Email subject | Context |
| SentDate | When sent | Timeline |
| ReceivedDate | When received | Delivery confirmation |
| MessageID | Unique message ID | Tracking |
| XOriginatingIP | Sender's IP | Source location |
| ReceivedHeaders | Full receive chain | Path analysis |
| AttachmentHashes | Attachment checksums | Content verification |

---

## Implementation Priority Matrix

| Priority | Category | Tags | Impact | Effort |
|----------|----------|------|--------|--------|
| **P0** | EXIF Timezone | 6 tags | Critical for timeline | Low |
| **P0** | GPS Movement | 9 tags | Critical for tracking | Low |
| **P0** | Device Serials | 6 tags | Critical for attribution | Low |
| **P1** | XMP History | 10 tags | High for tamper detection | Medium |
| **P1** | QuickTime Times | 6 tags | High for video forensics | Medium |
| **P1** | QuickTime GPS | 5 tags | High for location evidence | Medium |
| **P2** | Office Hidden | 10 tags | Medium for doc forensics | Medium |
| **P2** | PE Anomalies | 6 checks | Medium for malware analysis | Medium |
| **P3** | Email/TNEF | 10 tags | Lower priority new format | High |

---

## Success Metrics

### Coverage Goals
| Format | Current | Target | Increase |
|--------|---------|--------|----------|
| EXIF | 90% | 98% | +8% |
| GPS | 60% | 95% | +35% |
| XMP | 60% | 85% | +25% |
| QuickTime | 40% | 70% | +30% |
| Office | 60% | 80% | +20% |
| PE | 75% | 90% | +15% |

### Forensic Capability Goals
- [ ] Full timezone support for all timestamp tags
- [ ] Subsecond precision for photo sequencing
- [ ] GPS movement tracking (speed, direction, bearing)
- [ ] Device serial number extraction (camera + lens)
- [ ] XMP edit history parsing
- [ ] QuickTime forensic timestamps
- [ ] Office document hidden metadata
- [ ] Enhanced PE anomaly detection

---

## Testing Strategy

### Unit Tests
- Each new tag has parsing test
- Timezone offset parsing validation
- GPS coordinate format tests
- XMP history array parsing

### Integration Tests
- Real-world forensic image samples
- QuickTime files with GPS
- Signed executables with full chains
- Office documents with revision history

### Forensic Validation
- Compare output with ExifTool for same files
- Validate timezone handling against known samples
- Verify GPS accuracy against mapping tools

---

## Appendix: Tag ID Quick Reference

### EXIF Timezone Tags (IFD0/SubIFD)
```
0x9010 = OffsetTime
0x9011 = OffsetTimeOriginal
0x9012 = OffsetTimeDigitized
0x9290 = SubSecTime
0x9291 = SubSecTimeOriginal
0x9292 = SubSecTimeDigitized
```

### GPS Movement Tags (GPS IFD)
```
0x000C = GPSSpeedRef
0x000D = GPSSpeed
0x000E = GPSTrackRef
0x000F = GPSTrack
0x0010 = GPSImgDirectionRef
0x0011 = GPSImgDirection
0x0018 = GPSDestBearing
0x001A = GPSDestDistance
0x001F = GPSHPositioningError
```

### Device ID Tags
```
0xA420 = ImageUniqueID
0xA430 = OwnerName
0xA431 = BodySerialNumber (EXIF 2.3)
0xA432 = LensInfo
0xA433 = LensMake
0xA434 = LensModel
0xA435 = LensSerialNumber
0xC62F = SerialNumber (DNG/Adobe)
```

---

## Next Steps

1. **Immediate**: Implement Phase 1 (P0 tags) - EXIF timezone, GPS movement, device serials
2. **Week 2**: Implement Phase 2 (XMP history, software provenance)
3. **Week 3-4**: Implement Phase 3 (QuickTime forensics)
4. **Week 5-6**: Implement Phases 4-5 (Document & executable enhancements)
5. **Ongoing**: Add forensic validation tests

---

*This plan focuses on tags with highest forensic value per implementation effort.*
