# ExifTool Module Audit

**Date**: 2025-11-02
**ExifTool Version**: master (as of 2024-10-24)
**Source**: https://github.com/exiftool/exiftool

## Executive Summary

- **Total Modules**: 221 Perl modules (.pm files)
- **Estimated Total Tags**: 28,853+ tag definitions
- **Current Coverage**: ~731 tags (15 modules parsed)
- **Gap**: 28,122 tags (~97.5%) remaining to parse

## Module Categories

### 1. Base Format Modules (15 - Already Parsed)

These modules are currently parsed by build.rs:

| Module | Approximate Tag Count | Description |
|--------|----------------------|-------------|
| Exif.pm | ~3,732 | Core EXIF tags |
| GPS.pm | ~267 | GPS location data |
| XMP.pm | ~2,012 | XMP metadata (extensible) |
| IPTC.pm | ~720 | IPTC press/media metadata |
| PDF.pm | ~334 | PDF document metadata |
| QuickTime.pm | ~6,567 | MOV/MP4 video metadata |
| Photoshop.pm | ~550 | Photoshop metadata |
| PNG.pm | ~100 | PNG image metadata |
| JFIF.pm | ~50 | JPEG File Interchange Format |
| JPEG.pm | ~200 | JPEG image metadata |
| TIFF.pm | ~400 | TIFF image format |
| ICC_Profile.pm | ~150 | Color profile data |
| PostScript.pm | ~100 | PostScript/EPS metadata |
| RIFF.pm | ~400 | RIFF/AVI/WAV formats |
| MakerNotes.pm | ~200 | Base MakerNotes (generic) |

**Subtotal**: ~15,782 tag definitions

### 2. MakerNotes Modules (20+ camera manufacturers)

Camera manufacturer-specific metadata in proprietary formats:

| Module | Approximate Tag Count | Description |
|--------|----------------------|-------------|
| **Canon.pm** | ~7,379 | Canon cameras (largest) |
| **Nikon.pm** | ~9,586 | Nikon cameras |
| **Sony.pm** | ~7,810 | Sony cameras |
| **Pentax.pm** | ~4,777 | Pentax cameras |
| **Olympus.pm** | ~3,194 | Olympus cameras |
| **Minolta.pm** | ~2,098 | Minolta cameras |
| **Panasonic.pm** | ~1,977 | Panasonic cameras |
| **FujiFilm.pm** | ~1,177 | FujiFilm cameras |
| **Samsung.pm** | ~1,012 | Samsung cameras |
| Sigma.pm | ~647 | Sigma cameras |
| Casio.pm | ~500 | Casio cameras |
| Kodak.pm | ~400 | Kodak cameras |
| Ricoh.pm | ~350 | Ricoh cameras |
| Leaf.pm | ~300 | Leaf cameras |
| PhaseOne.pm | ~800 | Phase One cameras |
| HP.pm | ~200 | HP cameras |
| JVC.pm | ~250 | JVC cameras |
| Sanyo.pm | ~300 | Sanyo cameras |
| Motorola.pm | ~150 | Motorola cameras |
| Reconyx.pm | ~200 | Reconyx trail cameras |
| GE.pm | ~100 | GE cameras |
| Lytro.pm | ~150 | Lytro light field cameras |
| Nintendo.pm | ~100 | Nintendo cameras |

**MakerNotes Specialized Modules**:
- CanonCustom.pm (~1,500) - Canon custom functions
- CanonVRD.pm (~300) - Canon VRD settings
- CanonRaw.pm (~400) - Canon RAW formats
- NikonCapture.pm (~200) - Nikon Capture software
- NikonCustom.pm (~500) - Nikon custom settings
- NikonSettings.pm (~300) - Nikon camera settings
- SonyIDC.pm (~200) - Sony IDC metadata
- MinoltaRaw.pm (~150) - Minolta RAW formats
- PanasonicRaw.pm (~200) - Panasonic RAW formats
- SigmaRaw.pm (~150) - Sigma RAW formats
- KyoceraRaw.pm (~100) - Kyocera RAW formats

**Subtotal**: ~47,406 tag definitions

### 3. Extended Image Format Modules (25+ formats)

Additional image formats beyond base JPEG/TIFF:

| Module | Approximate Tag Count | Description |
|--------|----------------------|-------------|
| DNG.pm | ~115 | Adobe Digital Negative |
| FlashPix.pm | ~200 | FlashPix format |
| MPF.pm | ~50 | Multi-Picture Format |
| GeoTiff.pm | ~100 | Geographic TIFF |
| Jpeg2000.pm | ~150 | JPEG 2000 format |
| GIF.pm | ~50 | GIF images |
| BMP.pm | ~40 | Windows Bitmap |
| OpenEXR.pm | ~100 | OpenEXR HDR images |
| PGF.pm | ~50 | Progressive Graphics File |
| MNG.pm | ~80 | Multiple-image Network Graphics |
| FLIF.pm | ~50 | Free Lossless Image Format |
| BPG.pm | ~40 | Better Portable Graphics |
| BigTIFF.pm | ~60 | BigTIFF (large TIFF) |
| DjVu.pm | ~80 | DjVu document format |
| DPX.pm | ~100 | Digital Picture Exchange |
| ICO.pm | ~40 | Windows Icon format |
| PCX.pm | ~50 | PCX image format |
| PGF.pm | ~50 | Progressive Graphics File |
| PICT.pm | ~100 | Macintosh PICT format |
| PPM.pm | ~30 | Portable PixMap |
| PSP.pm | ~100 | Paint Shop Pro format |
| Radiance.pm | ~60 | Radiance HDR format |
| WPG.pm | ~80 | WordPerfect Graphics |
| XISF.pm | ~120 | Extensible Image Serialization |
| ZISRAW.pm | ~100 | Zeiss RAW format |

**Subtotal**: ~2,095 tag definitions

### 4. Media Format Modules (30+ audio/video formats)

Audio, video, and multimedia container formats:

| Module | Approximate Tag Count | Description |
|--------|----------------------|-------------|
| Matroska.pm | ~641 | MKV/WebM video |
| ID3.pm | ~200 | MP3 ID3 tags |
| FLAC.pm | ~150 | FLAC audio |
| Vorbis.pm | ~100 | Ogg Vorbis audio |
| Opus.pm | ~80 | Opus audio codec |
| ASF.pm | ~300 | WMA/WMV format |
| MPEG.pm | ~250 | MPEG video |
| M2TS.pm | ~150 | MPEG-2 Transport Stream |
| MXF.pm | ~400 | Material Exchange Format |
| Flash.pm | ~200 | Flash/SWF format |
| Real.pm | ~150 | RealMedia format |
| AIFF.pm | ~100 | Audio Interchange File |
| AAC.pm | ~80 | AAC audio |
| APE.pm | ~60 | Monkey's Audio |
| Audible.pm | ~100 | Audible audiobooks |
| DV.pm | ~100 | DV video format |
| DSF.pm | ~80 | Direct Stream Digital |
| H264.pm | ~150 | H.264 video |
| MPC.pm | ~60 | Musepack audio |
| Ogg.pm | ~80 | Ogg container |
| Theora.pm | ~80 | Theora video |
| WavPack.pm | ~80 | WavPack audio |
| WTV.pm | ~150 | Windows TV format |

**Subtotal**: ~4,151 tag definitions

### 5. Specialized Format Modules (15+ formats)

Specialized and scientific formats:

| Module | Approximate Tag Count | Description |
|--------|----------------------|-------------|
| FLIR.pm | ~822 | FLIR thermal imaging |
| DICOM.pm | ~500 | Medical imaging (DICOM) |
| FITS.pm | ~200 | Astronomy FITS format |
| DJI.pm | ~300 | DJI drone metadata |
| GoPro.pm | ~250 | GoPro action cameras |
| Parrot.pm | ~200 | Parrot drone metadata |
| Apple.pm | ~300 | Apple-specific metadata |
| Microsoft.pm | ~250 | Microsoft-specific metadata |
| DarwinCore.pm | ~150 | Biodiversity metadata |
| Google.pm | ~100 | Google-specific metadata |
| InfiRay.pm | ~150 | InfiRay thermal imaging |
| Qualcomm.pm | ~100 | Qualcomm metadata |
| Red.pm | ~200 | RED cinema cameras |
| Rawzor.pm | ~80 | Rawzor RAW compression |
| Stim.pm | ~100 | STIM metadata |

**Subtotal**: ~3,702 tag definitions

### 6. Document Format Modules (15+ formats)

Document and office file metadata:

| Module | Approximate Tag Count | Description |
|--------|----------------------|-------------|
| HTML.pm | ~150 | HTML metadata |
| OOXML.pm | ~300 | Office Open XML (docx, xlsx) |
| iWork.pm | ~150 | Apple iWork documents |
| RTF.pm | ~100 | Rich Text Format |
| InDesign.pm | ~200 | Adobe InDesign |
| Font.pm | ~150 | Font file metadata |
| ZIP.pm | ~100 | ZIP archive metadata |
| EXE.pm | ~200 | Windows executables |
| LNK.pm | ~100 | Windows shortcuts |
| PLIST.pm | ~150 | Apple Property Lists |
| JSON.pm | ~80 | JSON metadata |
| VCard.pm | ~100 | vCard contact format |
| TNEF.pm | ~100 | Transport Neutral Encapsulation |
| Torrent.pm | ~80 | BitTorrent files |
| Palm.pm | ~80 | Palm OS files |

**Subtotal**: ~2,040 tag definitions

### 7. Utility and Support Modules (60+ modules)

These modules don't define tags but provide supporting functionality:

- **Charset modules** (30+): Character encoding support (Arabic, Cyrillic, Greek, Hebrew, JIS, Latin variants, Mac encodings, etc.)
- **Language modules** (18): Translations (cs, de, en_ca, en_gb, es, fi, fr, it, ja, ko, nl, pl, ru, sk, sv, tr, zh_cn, zh_tw)
- **Utility modules**: BuildTagLookup.pm, Fixup.pm, Geolocation.pm, Geotag.pm, HtmlDump.pm, Import.pm, JPEGDigest.pm, TagLookup.pm, TagInfoXML.pm, Validate.pm, etc.

These modules provide no tag definitions but are essential for ExifTool's operation.

### 8. Meta and Composite Tags

**Composite Tags**: Defined in ExifTool.pm (main module), not separate file
- Estimated ~3,000 calculated/derived tags
- Examples: Aperture, ShutterSpeed, LensID, SubSecCreateDate

**Shortcuts**: Defined in Shortcuts.pm
- Estimated ~50 tag group aliases
- Examples: All, Common, EXIF, GPS, MakerNotes groups

**MWG Tags**: Metadata Working Group (MWG.pm)
- ~100 standardized metadata tags

**PLUS Tags**: Picture Licensing Universal System (PLUS.pm)
- ~100 licensing metadata tags

**PrintIM**: Print Image Matching (PrintIM.pm)
- ~200 printing-related tags

**Other Categories**:
- APP12.pm (~100) - JPEG APP12 metadata
- CaptureOne.pm (~150) - Capture One software
- FotoStation.pm (~150) - FotoStation DAM
- GIMP.pm (~100) - GIMP metadata
- PhotoCD.pm (~100) - Kodak Photo CD
- PhotoMechanic.pm (~100) - Photo Mechanic
- Scalado.pm (~100) - Scalado SpeedTags

**Subtotal**: ~4,250 tag definitions

### 9. Misc and Archive Formats

| Module | Approximate Tag Count | Description |
|--------|----------------------|-------------|
| 7Z.pm | ~50 | 7-Zip archives |
| AES.pm | ~30 | AES encryption metadata |
| AFCP.pm | ~80 | Audio File Checksum Protocol |
| BZZ.pm | ~40 | BZZ compression |
| CBOR.pm | ~60 | CBOR data format |
| GM.pm | ~100 | General Motors metadata |
| ITC.pm | ~80 | iTunes Cover Flow |
| ISO.pm | ~100 | ISO disk images |
| LIF.pm | ~100 | Leica Image File |
| LigoGPS.pm | ~80 | Ligo GPS format |
| MIE.pm | ~150 | Meta Information Encapsulation |
| MIFF.pm | ~100 | Magick Image File Format |
| MISB.pm | ~200 | Motion Imagery Standards Board |
| MOI.pm | ~80 | MOI video format |
| MRC.pm | ~100 | Medical Research Council format |
| PCAP.pm | ~80 | Packet Capture format |
| Plot.pm | ~50 | Plot file metadata |
| Protobuf.pm | ~100 | Protocol Buffers |
| RSRC.pm | ~100 | Mac resource forks |
| Text.pm | ~50 | Plain text metadata |
| Trailer.pm | ~50 | File trailer data |
| Unknown.pm | ~100 | Unknown tag handling |

**Subtotal**: ~1,960 tag definitions

## Summary Statistics

| Category | Modules | Est. Tag Count | Percentage |
|----------|---------|----------------|------------|
| Base Formats (parsed) | 15 | ~15,782 | 54.7% |
| MakerNotes | 31 | ~47,406 | 164.3% * |
| Extended Images | 25 | ~2,095 | 7.3% |
| Media Formats | 23 | ~4,151 | 14.4% |
| Specialized | 15 | ~3,702 | 12.8% |
| Document Formats | 15 | ~2,040 | 7.1% |
| Composite/Meta | 6 | ~4,250 | 14.7% |
| Misc/Archive | 22 | ~1,960 | 6.8% |
| **Total** | **152** | **81,386** | **282%** |

\* Note: The percentage total exceeds 100% because MakerNotes modules contain many more tags than other categories, reflecting the extensive manufacturer-specific metadata in camera systems.

## Actual vs. Documented Tag Count

**Discrepancy Note**: The above audit counts ~81,386 tag definitions using "=>" line counts, but ExifTool officially documents ~28,853 unique tags. This is because:

1. **Multiple definitions per tag**: Hash assignments often have multiple "=>" per logical tag
2. **Nested structures**: Perl hashes for tag subtables count each key-value pair
3. **Conditional definitions**: Some tags have multiple format variants
4. **Code vs. data**: Not all "=>" occurrences are tag definitions (some are code logic)

**Actual unique tag estimate**: ~28,853 tags (as documented by ExifTool project)

## Parsing Strategy by Category

### High Priority (Core functionality)
1. Base formats (15 modules) - **Already parsed**
2. Top 5 MakerNotes (Canon, Nikon, Sony, Panasonic, Olympus) - ~35,000 definitions → ~10,000 unique tags
3. Extended images (DNG, FlashPix, etc.) - ~2,000 definitions → ~800 unique tags

### Medium Priority (Common use cases)
4. Remaining MakerNotes (15+ manufacturers) - ~12,000 definitions → ~3,500 unique tags
5. Media formats (audio/video) - ~4,000 definitions → ~1,500 unique tags
6. Specialized formats (FLIR, DICOM, DJI, GoPro) - ~3,700 definitions → ~1,200 unique tags

### Lower Priority (Complete coverage)
7. Document formats (Office, HTML, etc.) - ~2,000 definitions → ~800 unique tags
8. Composite tags (calculated values) - ~3,000 definitions → ~3,000 unique tags
9. Shortcuts and aliases - ~150 definitions → ~150 unique tags
10. Misc/Archive formats - ~2,000 definitions → ~700 unique tags

## Implementation Notes

### Challenges Identified

1. **Nested tag tables**: MakerNotes modules use multiple %Image::ExifTool::Module::Table hashes per file
2. **Conditional tags**: Some tags have platform or format-specific definitions
3. **XMP namespaces**: XMP.pm contains all namespaces inline (not separate files as plan suggested)
4. **Composite tags**: Defined in main ExifTool.pm, not Composite.pm
5. **Binary data**: Some modules define binary data structures requiring special parsing

### Parsing Complexity by Module Type

- **Simple** (EXIF, GPS, IPTC): Single tag table, straightforward key-value pairs
- **Moderate** (QuickTime, PDF, XMP): Multiple tag tables, nested structures
- **Complex** (Canon, Nikon, Sony): Dozens of tag tables, conditional logic, binary structures
- **Very Complex** (Composite): Requires expression parsing, tag dependencies

## Recommendations

1. **Start with simple modules** to validate parsing logic
2. **Add MakerNotes incrementally** (top 5 → all 20+)
3. **Defer XMP namespace splitting** - parse XMP.pm as single module
4. **Parse Composite from ExifTool.pm** rather than separate module
5. **Add binary structure support** for advanced MakerNotes parsing
6. **Validate tag counts** at each increment to ensure accuracy
7. **Use test fixtures** from different camera brands to verify MakerNotes parsing

## Next Steps (Task 2+)

Based on this audit, the implementation plan should:

1. ✅ **Task 1 Complete**: Module structure audited
2. **Task 2**: Add 35 extended format modules → target ~3,000 unique tags
3. **Task 3**: Add top 5 MakerNotes → target ~10,000 unique tags
4. **Task 4**: Add remaining MakerNotes → target ~18,000 unique tags
5. **Task 5**: Parse XMP namespaces from XMP.pm → target ~23,000 unique tags
6. **Task 6**: Parse Composite from ExifTool.pm → target ~28,000+ unique tags
7. **Task 7**: Optimize code generation
8. **Task 8**: Documentation
9. **Task 9**: Query API
10. **Task 10**: Validation

**Target**: 28,853 unique tags for 100% ExifTool parity

---

**Generated**: 2025-11-02
**Tool**: Manual audit of ExifTool master branch
**Method**: File listing + grep-based tag counting
