# ExifTool Tag Coverage

This document reports two separate things about OxiDex, and does not mix them:
how many tags it has **definitions** for, and how many tags it actually
**extracts** from real files.

::: info Auto-Generated
This document is automatically updated on each push to `main`. Last updated: **2026-08-08**
:::

## Tag Definitions

Counted from the `oxidex-tags-*` YAML databases. This is what OxiDex knows a
tag *exists*; it says nothing about whether any parser reads it.

| Metric | Value |
|--------|-------|
| Total Tags | 16,684 |
| Tag Tables | 931 |
| Domains | 6 |

::: warning Definitions are not coverage
A definition count is documentation, not capability. `src/tag_sync` ingests
`exiftool -f -listx`, which carries `count encoding id index lang name type
version writable` and no layout at all — no `SubDirectory`, `FORMAT`,
`FIRST_ENTRY`, `ValueConv` or `Condition`. It can say a tag exists; it can
never say how to read one. A rising tag count is therefore **not** evidence of
rising extraction coverage. See [Measured Extraction Coverage](#measured-extraction-coverage).
:::

::: tip Empirical JPEG comparison
For JPEG specifically there is a deeper per-tag comparison against ExifTool
covering read *and* write round-trips, regression-gated in CI:
[JPEG Tag Support](/reference/jpeg-tag-support) ·
[JPEG Tag Matrix](/reference/jpeg-tag-matrix)
:::

---

## Definitions by Domain

| Domain | Tables | Tags | Description |
|--------|--------|------|-------------|
| Camera | 580 | 8,246 | MakerNotes from 40+ manufacturers |
| Core | 108 | 1,455 | EXIF, GPS, XMP, IPTC standards |
| Document | 50 | 434 | PDF, Office, HTML metadata |
| Image | 63 | 576 | PNG, GIF, BMP, WebP, etc. |
| Media | 113 | 2,496 | Audio/video containers |
| Specialty | 17 | 3,477 | FLIR, DICOM, DJI, etc. |
| **Total** | **931** | **16,684** | |

---

## Measured Extraction Coverage

Every number in this section comes from running OxiDex and ExifTool
13.59 over the same files and diffing the output tag by tag. It is
a measurement, not an estimate derived from source code.

**Corpus:** tests/fixtures (recursive), media files only

| Column | Meaning |
|--------|---------|
| Match | Same tag name, same value. |
| Rename | OxiDex read the value correctly under a name ExifTool does not use. Value-confirmed, so this is a naming fix, not parsing work. |
| Value | Both emit the tag, values disagree. Usually a `PrintConv` gap. |
| Missing | ExifTool emits a tag OxiDex does not. Real extraction work. |
| Score | Match / (Match + Rename + Value + Missing). |
| Ceiling | Score once every rename is corrected. |

| Format | Files | Match | Rename | Value | Missing | Score | Ceiling |
|--------|------:|------:|-------:|------:|--------:|------:|--------:|
| DNG | 1 | 4 | 0 | 0 | 1 | 80.0% | 80.0% |
| JPEG | 28 | 808 | 0 | 23 | 0 | 97.2% | 97.2% |
| MP4 | 2 | 66 | 0 | 0 | 0 | 100.0% | 100.0% |
| PDF | 2 | 28 | 0 | 0 | 0 | 100.0% | 100.0% |
| PNG | 4 | 138 | 6 | 0 | 16 | 86.2% | 90.0% |
| TIFF | 7 | 143 | 0 | 4 | 0 | 97.3% | 97.3% |
| **Total** | **44** | **1187** | **6** | **27** | **17** | **96.0%** | **96.4%** |

### Renames — free coverage (4)

OxiDex reads these values correctly under the wrong name. The value match is
what makes the mapping safe to act on; name similarity alone would be a guess.

| Format | OxiDex name | ExifTool name | Files |
|--------|-------------|---------------|------:|
| PNG | `tEXt:Description` | `Description` | 2 |
| PNG | `tEXt:Title` | `Title` | 2 |
| PNG | `tEXt:Author` | `Author` | 1 |
| PNG | `tEXt:Software` | `Software` | 1 |

### Top missing tags — real extraction work (6 distinct)

| Format | Tag | Files |
|--------|-----|------:|
| PNG | `Datecreate` | 4 |
| PNG | `Datemodify` | 4 |
| PNG | `Datetimestamp` | 4 |
| PNG | `ExifByteOrder` | 2 |
| PNG | `Warning` | 2 |
| DNG | `Warning` | 1 |

---

## MakerNote Status

::: tip ✅ MakerNote Parsers Active
MakerNote parsers for 30 camera manufacturers are **implemented and connected** to the TIFF parsing pipeline.

This means the dispatcher has an arm for these makes and that the TIFF parser
calls it. It is not a claim about how much of each manufacturer's MakerNote is
extracted — only the conformance table above measures that.
:::

### Dispatched Manufacturers

**Traditional Cameras:** Canon, Nikon, Sony, Panasonic, Fujifilm, Leica

**Smartphones:** Apple

**Specialty Devices:** Dji, Flir, Gopro, Infiray, Nintendo, Parrot, Reconyx, Red

**Legacy Cameras:** Ge, Hp, Jvc, Kodak, Motorola, Ricoh, Sanyo

**Other:** Adobe Indesign, Capture One, Fotostation, Gimp, Leica Camera Ag, Nikon Capture, Photoshop, Scalado


---

## ExifTool Module Reference

Approximate tag counts published by ExifTool for its own modules, for scale.
These describe ExifTool, not OxiDex, and are not used in any calculation above.

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

## Tag Count Notes

### Why definition counts differ from ExifTool's

The OxiDex tag database and ExifTool's documented tag list are not directly
comparable, because OxiDex stores:

1. **Variant definitions**: Tags with multiple format/type variants
2. **Nested structures**: Subtable entries counted separately
3. **Conditional definitions**: Platform or version-specific tags

Dividing one count by the other produces a ratio that moves for reasons
unrelated to capability, which is why this page does not publish one.

### Excluded Tags

Some ExifTool tags are excluded by design:

- **Composite tags**: Calculated values (Aperture from FNumber, etc.)
- **Shortcut tags**: Aliases to other tags
- **Internal tags**: ExifTool operational tags

---

## Related Documentation

- [Tag Database Architecture](/architecture/tag-database) - Implementation details
- [MakerNotes Reference](/reference/makernotes) - Camera manufacturer metadata
