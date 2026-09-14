# Native creation of a JPEG EXIF block

September 14, 2026. Instrument: `native_write_matrix.py` native API calls,
followed by raw TIFF/JPEG inspection. This is native behavior evidence, not
OxiDex writer conformance or proof of a complete version upgrade.

The first inspector rejected TIFF RATIONAL (type 5) fields added by native
ExifTool. The raw native outputs and call records were retained. Regrading with
classic TIFF wire widths recovered all 27 cases: nine generated scalar names
across releases 11.78, 12.64 and 13.59. A regression checks signed, rational and
floating-point storage in both byte orders, truncated values and unknown types.

## Observed defaults

The fixture is `tests/fixtures/jpeg/tag_matrix_base.jpg`. Its EXIF block was
removed with explicitly selected 13.59 before native insertion. The JFIF header
remained. Each of the nine current final-scalar fields was inserted separately.
All 27 files preserve the JPEG scan and bytes outside EXIF. They all add:

| Physical ID | Native field | Stored value |
| --- | --- | --- |
| 282 | XResolution | RATIONAL 72/1 |
| 283 | YResolution | RATIONAL 72/1 |
| 296 | ResolutionUnit | SHORT 1 |
| 531 | YCbCrPositioning | SHORT 1 |

A second three-file probe removed JFIF too, then inserted `IFD0:HostComputer`.
Releases 11.78 and 12.64 add IDs 282, 283, 296 and 531, with ResolutionUnit 2.
Release 13.59 adds only ID 531 beside the requested field. All three preserve
the scan and bytes outside EXIF. Both older outputs are byte-identical for
this input; the 13.59 output differs as expected from the recorded native rules.

The selected random pair remains 11.78/12.64. Version 13.59 is an additional
current-pin regression case, not a replacement for either selected release.

## Implementation consequence

The new-block writer must derive directory defaults from WriteExif's lexical
`%mandatory` data and its JFIF-dependent selection logic. Copying this observed
table into Rust would preserve old behavior across upgrades and is not the
implementation. A native-only observation also does not admit the generated
writer: generated new-block output must reproduce each selected release's own
result, including preserved bytes and the absence of obsolete defaults.

Evidence under the external write-upgrade continuation root:
`new-jpeg-exif-native-20260914/report.json` (first inspector failure),
`regrade-rational-inspector/report.json` and `without-jfif/report.json` below
that directory, plus retained inputs, outputs and exact native call records.
