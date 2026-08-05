# ExifTool Tag Coverage

This document details OxiDex's coverage of ExifTool's tag database and analyzes parser implementation status.

::: info Auto-Generated
This document is automatically updated on each push to `main`. Last updated: **2026-08-05**
:::

## Summary

| Metric | Value |
|--------|-------|
| Total Tags | 16,677 |
| Tag Tables | 931 |
| Domains | 6 |
| Format Parsers | 38 |
| ExifTool Parity | 58%* |

*ExifTool officially documents ~28,853 unique tags. OxiDex defines 16,677 tags (including variant definitions).

::: tip Empirical JPEG comparison
The counts on this page are derived statically from the tag database. For JPEG
specifically, an empirical per-tag comparison against ExifTool (read + write
round-trips, regression-gated in CI) is available:
[JPEG Tag Support](/reference/jpeg-tag-support) ·
[JPEG Tag Matrix](/reference/jpeg-tag-matrix)
:::

---

## Coverage by Domain

| Domain | Tables | Tags | Description |
|--------|--------|------|-------------|
| Camera | 580 | 8,246 | MakerNotes from 40+ manufacturers |
| Core | 108 | 1,448 | EXIF, GPS, XMP, IPTC standards |
| Document | 50 | 434 | PDF, Office, HTML metadata |
| Image | 63 | 576 | PNG, GIF, BMP, WebP, etc. |
| Media | 113 | 2,496 | Audio/video containers |
| Specialty | 17 | 3,477 | FLIR, DICOM, DJI, etc. |
| **Total** | **931** | **16,677** | |

---

## MakerNote Status

::: tip ✅ MakerNote Parsers Active
MakerNote parsers for 30+ camera manufacturers are **fully implemented and connected** to the TIFF parsing pipeline.
:::

### Supported Manufacturers

**Traditional Cameras:** Canon, Nikon, Sony, Panasonic, Fujifilm, Leica

**Smartphones:** Apple

**Specialty Devices:** Dji, Flir, Gopro, Infiray, Nintendo, Parrot, Reconyx, Red

**Legacy Cameras:** Ge, Hp, Jvc, Kodak, Motorola, Ricoh, Sanyo


---

## Coverage by Use Case

| Use Case | Coverage | Formats |
|----------|----------|---------|
| JPEG photos | ⚠️ 67% | EXIF, XMP, IPTC, MakerNotes |
| RAW photos | ✅ 90% | DNG, CR2, NEF, ARW, etc. |
| Video files | ✅ 90% | QuickTime, Matroska, RIFF |
| Audio files | ✅ 100% | ID3, FLAC, Vorbis, AAC |
| PDF documents | ✅ 90% | Info dict, XMP |
| Office docs | ✅ 75% | OOXML, iWork |
| Executables | ✅ 80% | PE, ELF, Mach-O |

---

## Parser Coverage by Format

### ✅ Strong Coverage (>50%)

| Format | Coverage | Status |
|--------|----------|--------|
| FLAC | 100% | ✅ Complete |
| MP3 | 100% | ✅ Complete |
| AAC | 100% | ✅ Complete |
| APE | 100% | ✅ Complete |
| Opus | 100% | ✅ Complete |
| OGG | 100% | ✅ Complete |
| WAV | 100% | ✅ Complete |
| BMP | 90% | ✅ Good |
| GIF | 90% | ✅ Good |
| WebP | 90% | ✅ Good |
| ZIP | 90% | ✅ Good |
| TTF | 90% | ✅ Good |
| OTF | 90% | ✅ Good |
| PE | 90% | ✅ Good |
| TIFF | 90% | ✅ Good |
| EXIF | 90% | ✅ Good |
| DNG | 90% | ✅ Good |
| CR2 | 90% | ✅ Good |
| NEF | 90% | ✅ Good |
| PDF | 90% | ✅ Good |
| MKV | 90% | ✅ Good |
| AVI | 90% | ✅ Good |
| RIFF | 90% | ✅ Good |
| QuickTime | 90% | ✅ Good |
| MP4 | 90% | ✅ Good |
| MOV | 90% | ✅ Good |
| ICC | 90% | ✅ Good |
| SPECIALIZED | 90% | ✅ Good |
| Mach-O | 75% | ✅ Good |
| ELF | 75% | ✅ Good |
| DOCX | 75% | ✅ Good |
| XLSX | 75% | ✅ Good |
| TEXT | 75% | ✅ Good |
| JPEG | 60% | ✅ Good |
| PNG | 60% | ✅ Good |
| IPTC | 60% | ✅ Good |
| XMP | 60% | ✅ Good |

### ⚠️ Partial Coverage (10-50%)

| Format | Coverage | Priority |
|--------|----------|----------|
| CANON_VRD | 20% | Medium |

---

## ExifTool Module Reference

### Base Format Modules

| Module | Tags | Description |
|--------|------|-------------|
| Exif.pm | ~3,732 | Core EXIF tags |
| GPS.pm | ~267 | GPS location data |
| XMP.pm | ~2,012 | XMP metadata |
| IPTC.pm | ~720 | Press/media metadata |
| PDF.pm | ~334 | PDF documents |
| QuickTime.pm | ~6,567 | MOV/MP4 video |
| Photoshop.pm | ~550 | Photoshop metadata |
| PNG.pm | ~100 | PNG images |
| TIFF.pm | ~400 | TIFF format |
| ICC_Profile.pm | ~150 | Color profiles |
| RIFF.pm | ~400 | RIFF/AVI/WAV |

### MakerNotes Modules

| Module | Tags | Description |
|--------|------|-------------|
| Canon.pm | ~7,379 | Canon cameras |
| Nikon.pm | ~9,586 | Nikon cameras |
| Sony.pm | ~7,810 | Sony cameras |
| Pentax.pm | ~4,777 | Pentax cameras |
| Olympus.pm | ~3,194 | Olympus cameras |
| Panasonic.pm | ~1,977 | Panasonic cameras |
| FujiFilm.pm | ~1,177 | FujiFilm cameras |
| Samsung.pm | ~1,012 | Samsung cameras |

### Media Format Modules

| Module | Tags | Description |
|--------|------|-------------|
| Matroska.pm | ~641 | MKV/WebM |
| ID3.pm | ~200 | MP3 ID3 tags |
| FLAC.pm | ~150 | FLAC audio |
| Vorbis.pm | ~100 | Ogg Vorbis |
| ASF.pm | ~300 | WMA/WMV |
| MPEG.pm | ~250 | MPEG video |

### Specialized Modules

| Module | Tags | Description |
|--------|------|-------------|
| FLIR.pm | ~822 | Thermal imaging |
| DICOM.pm | ~500 | Medical imaging |
| DJI.pm | ~300 | DJI drones |
| GoPro.pm | ~250 | Action cameras |
| EXE.pm | ~200 | Executables |

---

## Recommendations

### Formats Needing Enhancement

- **CANON_VRD** (20% coverage)

---

## Tag Count Notes

### Why Counts Differ from ExifTool

ExifTool officially documents ~28,853 unique tags, but our database contains more because:

1. **Variant definitions**: Tags with multiple format/type variants
2. **Nested structures**: Subtable entries counted separately
3. **Conditional definitions**: Platform or version-specific tags

### Excluded Tags

Some ExifTool tags are excluded by design:

- **Composite tags**: Calculated values (Aperture from FNumber, etc.)
- **Shortcut tags**: Aliases to other tags
- **Internal tags**: ExifTool operational tags

---

## Related Documentation

- [Tag Database Architecture](/architecture/tag-database) - Implementation details
- [MakerNotes Reference](/reference/makernotes) - Camera manufacturer metadata
