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

**Corpus:** ExifTool 13.59 `t/images` + `tests/fixtures`

See [Measuring Extraction Coverage](/contributing/measuring-coverage) for how to
reproduce this, and what to do when a new file type is added.

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
| AA | 1 | 3 | 0 | 0 | 29 | 9.4% | 9.4% |
| AAC | 1 | 7 | 0 | 0 | 0 | 100.0% | 100.0% |
| AAE | 1 | 3 | 0 | 0 | 15 | 16.7% | 16.7% |
| AFM | 1 | 2 | 0 | 1 | 13 | 12.5% | 12.5% |
| AIFF | 1 | 3 | 0 | 0 | 22 | 12.0% | 12.0% |
| APE | 1 | 22 | 0 | 0 | 0 | 100.0% | 100.0% |
| AVI | 2 | 158 | 0 | 3 | 2 | 96.9% | 96.9% |
| BMP | 1 | 16 | 0 | 0 | 0 | 100.0% | 100.0% |
| BPG | 1 | 70 | 0 | 12 | 2 | 83.3% | 83.3% |
| BTF | 1 | 3 | 0 | 0 | 10 | 23.1% | 23.1% |
| CR2 | 1 | 202 | 0 | 11 | 1 | 94.4% | 94.4% |
| CR3 | 1 | 313 | 0 | 15 | 5 | 94.0% | 94.0% |
| CRW | 1 | 3 | 0 | 0 | 156 | 1.9% | 1.9% |
| CSV | 2 | 10 | 0 | 0 | 8 | 55.6% | 55.6% |
| CZI | 1 | 3 | 0 | 0 | 31 | 8.8% | 8.8% |
| DFONT | 1 | 1 | 0 | 2 | 25 | 3.6% | 3.6% |
| DICOM | 1 | 3 | 0 | 0 | 98 | 3.0% | 3.0% |
| DJVU (MULTI-PAGE) | 1 | 36 | 1 | 0 | 0 | 97.3% | 100.0% |
| DNG | 2 | 283 | 0 | 9 | 4 | 95.6% | 95.6% |
| DOCX | 1 | 68 | 0 | 1 | 1 | 97.1% | 97.1% |
| DPX | 1 | 21 | 0 | 0 | 13 | 61.8% | 61.8% |
| DR4 | 1 | 96 | 0 | 0 | 0 | 100.0% | 100.0% |
| DSS | 1 | 4 | 0 | 0 | 3 | 57.1% | 57.1% |
| DV | 1 | 3 | 0 | 0 | 15 | 16.7% | 16.7% |
| EIP | 1 | 11 | 0 | 0 | 81 | 12.0% | 12.0% |
| ELF EXECUTABLE | 1 | 5 | 0 | 2 | 0 | 71.4% | 71.4% |
| ELF SHARED LIBRARY | 1 | 6 | 0 | 1 | 0 | 85.7% | 85.7% |
| EPS | 1 | 53 | 0 | 0 | 5 | 91.4% | 91.4% |
| EXR | 1 | 16 | 0 | 1 | 0 | 94.1% | 94.1% |
| EXTENDED WEBP | 1 | 20 | 0 | 1 | 1 | 90.9% | 90.9% |
| FIT | 1 | 0 | 0 | 0 | 78 | 0.0% | 0.0% |
| FITS | 1 | 34 | 0 | 0 | 0 | 100.0% | 100.0% |
| FLAC | 1 | 19 | 0 | 0 | 0 | 100.0% | 100.0% |
| FLIF | 1 | 40 | 0 | 3 | 0 | 93.0% | 93.0% |
| FLV | 1 | 44 | 0 | 2 | 0 | 95.7% | 95.7% |
| FPF | 1 | 41 | 0 | 0 | 0 | 100.0% | 100.0% |
| GIF | 1 | 37 | 0 | 4 | 0 | 90.2% | 90.2% |
| GZIP | 1 | 9 | 0 | 1 | 0 | 90.0% | 90.0% |
| HDR | 1 | 3 | 0 | 0 | 10 | 23.1% | 23.1% |
| HEIF | 1 | 30 | 0 | 1 | 0 | 96.8% | 96.8% |
| HTML | 1 | 58 | 0 | 0 | 0 | 100.0% | 100.0% |
| ICC | 1 | 25 | 0 | 3 | 0 | 89.3% | 89.3% |
| ICO | 1 | 12 | 0 | 0 | 0 | 100.0% | 100.0% |
| ICS | 1 | 9 | 0 | 0 | 40 | 18.4% | 18.4% |
| IIQ | 1 | 71 | 0 | 4 | 2 | 92.2% | 92.2% |
| INDD | 1 | 3 | 0 | 0 | 9 | 25.0% | 25.0% |
| INX | 1 | 3 | 0 | 0 | 34 | 8.1% | 8.1% |
| ISO | 1 | 18 | 0 | 0 | 0 | 100.0% | 100.0% |
| ITC | 1 | 3 | 0 | 0 | 10 | 23.1% | 23.1% |
| J2C | 1 | 3 | 0 | 0 | 5 | 37.5% | 37.5% |
| JP2 | 1 | 3 | 0 | 0 | 55 | 5.2% | 5.2% |
| JPEG | 69 | 4461 | 0 | 342 | 226 | 88.7% | 88.7% |
| JPS | 1 | 11 | 0 | 0 | 5 | 68.8% | 68.8% |
| JXL | 1 | 5 | 0 | 5 | 1 | 45.5% | 45.5% |
| JXL CODESTREAM | 1 | 2 | 0 | 5 | 0 | 28.6% | 28.6% |
| LFP | 1 | 98 | 0 | 0 | 0 | 100.0% | 100.0% |
| LNK | 1 | 46 | 0 | 0 | 0 | 100.0% | 100.0% |
| M2TS | 1 | 22 | 0 | 0 | 1 | 95.7% | 95.7% |
| M4A | 1 | 56 | 0 | 6 | 0 | 90.3% | 90.3% |
| MACH-O DYNAMIC LINK LIBRARY | 1 | 8 | 0 | 1 | 0 | 88.9% | 88.9% |
| MACH-O EXECUTABLE | 1 | 7 | 0 | 2 | 0 | 77.8% | 77.8% |
| MACH-O STATIC LIBRARY | 1 | 2 | 0 | 1 | 5 | 25.0% | 25.0% |
| MACOS | 1 | 2 | 0 | 1 | 8 | 18.2% | 18.2% |
| MIE | 1 | 2 | 0 | 1 | 60 | 3.2% | 3.2% |
| MIFF | 1 | 13 | 0 | 0 | 89 | 12.7% | 12.7% |
| MKV | 1 | 33 | 0 | 0 | 0 | 100.0% | 100.0% |
| MOBI | 1 | 3 | 0 | 0 | 21 | 12.5% | 12.5% |
| MOI | 1 | 3 | 0 | 0 | 7 | 30.0% | 30.0% |
| MOV | 1 | 77 | 0 | 8 | 6 | 84.6% | 84.6% |
| MP3 | 1 | 31 | 0 | 0 | 1 | 96.9% | 96.9% |
| MP4 | 2 | 66 | 0 | 0 | 0 | 100.0% | 100.0% |
| MPC | 1 | 16 | 0 | 10 | 17 | 37.2% | 37.2% |
| MRC | 1 | 3 | 0 | 0 | 88 | 3.3% | 3.3% |
| MRW | 1 | 126 | 0 | 3 | 2 | 96.2% | 96.2% |
| MXF | 1 | 34 | 0 | 0 | 0 | 100.0% | 100.0% |
| NEF | 1 | 213 | 0 | 7 | 4 | 95.1% | 95.1% |
| NUMBERS | 1 | 11 | 0 | 0 | 7 | 61.1% | 61.1% |
| ODS | 1 | 3 | 0 | 0 | 18 | 14.3% | 14.3% |
| OGG | 2 | 33 | 0 | 0 | 1 | 97.1% | 97.1% |
| OPUS | 1 | 18 | 0 | 0 | 0 | 100.0% | 100.0% |
| PCAPNG | 1 | 6 | 1 | 2 | 3 | 50.0% | 58.3% |
| PCD | 1 | 3 | 0 | 0 | 26 | 10.3% | 10.3% |
| PCX | 1 | 3 | 0 | 0 | 15 | 16.7% | 16.7% |
| PDF | 4 | 149 | 0 | 2 | 0 | 98.7% | 98.7% |
| PFA | 1 | 6 | 1 | 0 | 12 | 31.6% | 36.8% |
| PFB | 1 | 3 | 0 | 0 | 16 | 15.8% | 15.8% |
| PFM | 2 | 11 | 0 | 1 | 26 | 28.9% | 28.9% |
| PGF | 1 | 3 | 0 | 0 | 24 | 11.1% | 11.1% |
| PICT | 1 | 3 | 0 | 0 | 6 | 33.3% | 33.3% |
| PLIST | 2 | 5 | 0 | 1 | 20 | 19.2% | 19.2% |
| PMP | 1 | 3 | 0 | 0 | 23 | 11.5% | 11.5% |
| PNG | 5 | 153 | 7 | 0 | 17 | 86.4% | 90.4% |
| PPM | 1 | 3 | 0 | 0 | 6 | 33.3% | 33.3% |
| PPT | 1 | 40 | 0 | 0 | 0 | 100.0% | 100.0% |
| PSD | 1 | 93 | 0 | 3 | 2 | 94.9% | 94.9% |
| PSP | 1 | 3 | 0 | 0 | 23 | 11.5% | 11.5% |
| R3D | 1 | 3 | 0 | 0 | 34 | 8.1% | 8.1% |
| RA | 1 | 3 | 0 | 0 | 8 | 27.3% | 27.3% |
| RAF | 1 | 97 | 0 | 18 | 10 | 77.6% | 77.6% |
| RAM | 1 | 4 | 0 | 0 | 0 | 100.0% | 100.0% |
| RAR | 1 | 8 | 0 | 0 | 0 | 100.0% | 100.0% |
| RAW | 1 | 3 | 0 | 0 | 17 | 15.0% | 15.0% |
| RM | 1 | 2 | 0 | 1 | 49 | 3.8% | 3.8% |
| RTF | 1 | 3 | 0 | 0 | 10 | 23.1% | 23.1% |
| RW2 | 1 | 165 | 0 | 3 | 10 | 92.7% | 92.7% |
| SVG | 1 | 25 | 0 | 1 | 0 | 96.2% | 96.2% |
| SWF | 1 | 3 | 0 | 0 | 11 | 21.4% | 21.4% |
| TIFF | 9 | 243 | 0 | 16 | 1 | 93.5% | 93.5% |
| TNEF | 1 | 0 | 0 | 0 | 37 | 0.0% | 0.0% |
| TORRENT | 1 | 3 | 0 | 0 | 21 | 12.5% | 12.5% |
| TTF | 1 | 29 | 0 | 0 | 0 | 100.0% | 100.0% |
| TXT | 9 | 59 | 0 | 4 | 0 | 93.7% | 93.7% |
| URL | 1 | 1 | 0 | 2 | 13 | 6.2% | 6.2% |
| VCARD | 1 | 8 | 4 | 3 | 24 | 20.5% | 30.8% |
| VRD | 1 | 111 | 0 | 0 | 0 | 100.0% | 100.0% |
| WAV | 1 | 15 | 0 | 1 | 1 | 88.2% | 88.2% |
| WIN32 EXE | 1 | 33 | 0 | 1 | 0 | 97.1% | 97.1% |
| WMV | 1 | 41 | 0 | 1 | 0 | 97.6% | 97.6% |
| WPG | 1 | 4 | 0 | 0 | 3 | 57.1% | 57.1% |
| WTV | 1 | 3 | 0 | 0 | 69 | 4.2% | 4.2% |
| X3F | 2 | 190 | 0 | 11 | 9 | 90.5% | 90.5% |
| XCF | 1 | 25 | 0 | 3 | 31 | 42.4% | 42.4% |
| XISF | 1 | 3 | 0 | 0 | 22 | 12.0% | 12.0% |
| XML | 4 | 2 | 0 | 10 | 43 | 3.6% | 3.6% |
| XMP | 11 | 212 | 1 | 11 | 81 | 69.5% | 69.8% |
| ZIP | 1 | 11 | 0 | 0 | 0 | 100.0% | 100.0% |
| **Total** | **238** | **9110** | **15** | **564** | **2073** | **77.5%** | **77.6%** |

### Renames — free coverage (13)

OxiDex reads these values correctly under the wrong name. The value match is
what makes the mapping safe to act on; name similarity alone would be a guess.

| Format | OxiDex name | ExifTool name | Files |
|--------|-------------|---------------|------:|
| PNG | `tEXt:Description` | `Description` | 2 |
| PNG | `tEXt:Title` | `Title` | 2 |
| DJVU (MULTI-PAGE) | `note` | `Note` | 1 |
| PCAPNG | `Application` | `UserApplication` | 1 |
| PFA | `PostScript:Title` | `FontName` | 1 |
| PNG | `tEXt:Author` | `Author` | 1 |
| PNG | `tEXt:Software` | `Software` | 1 |
| PNG | `tEXt:comment` | `Comment` | 1 |
| VCARD | `Address` | `AddressOther` | 1 |
| VCARD | `Email` | `EmailInternetWork` | 1 |
| VCARD | `Sound` | `SoundOgg` | 1 |
| VCARD | `Telephone` | `TelephoneOtherVoice` | 1 |
| XMP | `XmpmetaXmptk` | `XMPToolkit` | 1 |

### Top missing tags — real extraction work (1981 distinct)

| Format | Tag | Files |
|--------|-----|------:|
| JPEG | `LensID` | 7 |
| JPEG | `Quality` | 5 |
| JPEG | `Warning` | 5 |
| JPEG | `Composite:GPSAltitude` | 4 |
| JPEG | `DOF` | 4 |
| JPEG | `PreviewImageStart` | 4 |
| JPEG | `Sharpness` | 4 |
| PNG | `Datecreate` | 4 |
| PNG | `Datemodify` | 4 |
| PNG | `Datetimestamp` | 4 |
| JPEG | `AFPoint` | 3 |
| JPEG | `ColorMode` | 3 |
| JPEG | `DigitalZoom` | 3 |
| JPEG | `FocusMode` | 3 |
| JPEG | `ISO` | 3 |
| JPEG | `LightValue` | 3 |
| JPEG | `MakerNotes:Sharpness` | 3 |
| PNG | `Warning` | 3 |
| XMP | `Flash` | 3 |
| CSV | `ColumnCount` | 2 |
| CSV | `Delimiter` | 2 |
| CSV | `Quoting` | 2 |
| CSV | `RowCount` | 2 |
| JPEG | `DataDump` | 2 |
| JPEG | `FirmwareVersion` | 2 |

_1956 further missing tags omitted from this table._

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

- [Measuring Extraction Coverage](/contributing/measuring-coverage) - How this page is produced, and how to extend it for a new file type
- [Tag Database Architecture](/architecture/tag-database) - Implementation details
- [MakerNotes Reference](/reference/makernotes) - Camera manufacturer metadata
