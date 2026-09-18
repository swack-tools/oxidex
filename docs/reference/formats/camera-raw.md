# Camera RAW

Camera RAW files are one `FileFormat` variant, `CameraRaw`, that covers 36
RAW sub-formats (`RawFormat` in `src/parsers/raw/format_detection.rs`). Most
of them are TIFF containers, so they share the TIFF/IFD walker and the
MakerNote dispatcher. A few have their own parser.

How much of each format's output matches ExifTool 13.59 is measured per
format in the [ExifTool comparison report](/reference/comparison/). This
page covers structure and routing only.

## Formats

| Maker | Formats |
| --- | --- |
| Canon | CR2, CR3, CRW |
| Nikon | NEF, NRW |
| Sony | ARW, SR2, SRF, ARQ |
| ARRI | ARI |
| Samsung | SRW |
| Fujifilm | RAF |
| Olympus / OM Digital | ORF, ORI |
| Pentax | PEF |
| Panasonic | RW2, RWL |
| Hasselblad | 3FR, FFF |
| Phase One | IIQ |
| Mamiya | MEF |
| Leaf | MOS |
| Kodak | DCR, KDC |
| Minolta | MDC, MRW |
| Epson | ERF |
| Sigma | X3F |
| GoPro | GPR |
| Adobe | DNG |
| Light | LRI |
| Sinar | STI |
| HEIF-based | HIF |
| Generic | RAW (including Kyocera), CAM, REV |

Detection combines magic bytes with the file name, because many RAW formats
share TIFF's magic.

## How each is read

`parse_raw_metadata` (`src/parsers/raw/metadata.rs`) routes by format:

- **CR3** (ISO base media), **X3F**, **MRW** and **CRW** have their own
  parsers.
- **RAF** is read through the JPEG embedded in the file.
- **Everything else** goes through the TIFF-based RAW path: the IFD chain,
  EXIF, GPS, sub-IFDs, then the vendor's MakerNote via
  `src/parsers/tiff/makernote_dispatcher.rs`. Many MakerNote tables come
  from the generated tables in `src/exiftool_tables/`. See
  [Architecture](/architecture/).

## Writing

TIFF-structured RAW files can be written in place by the surgical TIFF
writer, but only when the header is a classic `II`/`MM` TIFF with magic 42
or 85. That covers NEF, CR2, ARW, DNG, PEF, RW2, IIQ and similar formats.
BigTIFF, ORF (its `IIRO`/`IIRS`/`MMOR` magic), RAF, MRW, X3F, CR3 and CRW
cannot be written. Which tags are proven against ExifTool is covered in
[Writing metadata](/guide/writing).

## Example

```bash
oxidex -j -a -G1 photo.nef                      # everything, family-1 groups
oxidex -IFD0:Make -IFD0:Model -ExifIFD:ISO photo.cr2
oxidex -r -j /path/to/raw/                      # a directory, in parallel
```

Tags are keyed by family-1 group: `IFD0`, `ExifIFD`, `SubIFD`, `GPS`, and
the maker group (`Canon`, `Nikon`, `Sony`, `FujiFilm`, …). Use
`oxidex -j file` to see the exact keys a file produces.
