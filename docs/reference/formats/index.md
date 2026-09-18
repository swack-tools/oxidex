# Supported formats

A format can be supported in three different ways, and they mean very
different things:

| State | What you get | How to tell |
| --- | --- | --- |
| **Parsed** | The format's own tags, from a parser for that format | Tags beyond `File:` and `System:`; JSON has no `Status` key |
| **Parsed generically** | Tags from a container or text reader, not the format's own tags | For example, an ODS spreadsheet gives `ZIP:` tags, and an RTF file gives line and word counts |
| **Identified only** | `File:FileType`, `FileTypeExtension`, `MIMEType` and filesystem tags. Nothing else. | For a single-file read, JSON `"Status": "IdentifiedOnly"`; `ParseStatus::IdentifiedOnly` in the library |

"Parsed" says a parser exists and runs. It does not say that the parser
extracts everything ExifTool does. How much of each format matches ExifTool
is measured, not assumed; see [ExifTool parity](/guide/exiftool-parity) and
the per-format [comparison report](/reference/comparison/).

::: info How these lists were made
The format lists come from the code at the `refactor/tag-machinery` tip: the
`FileFormat` enum in `src/core/file_format.rs`, the dispatch in
`src/core/format_dispatch.rs`, and the RAW detection in
`src/parsers/raw/format_detection.rs`. The examples of generic and
identity-only reads were produced by running a release build on the pinned
ExifTool 13.59 `t/images` corpus, and on synthetic headers for the types that
corpus lacks.
:::

## Parsed

Detection can map a file to one of 131 `FileFormat` variants. 129 of them
dispatch to a parser; the other two are `Unknown` and an unused `RAW`
placeholder. One of the 129, camera RAW, covers 36 RAW sub-formats.

| Family | Formats |
| --- | --- |
| **Images** | JPEG, TIFF, BigTIFF, PNG, GIF, BMP, WebP, HEIF, AVIF, JPEG XL, JPEG 2000, BPG, OpenEXR, PFM, Radiance HDR, FLIF, GIMP XCF, MIFF, SVG, ICO, Photoshop PSD, Paint Shop Pro, WordPerfect Graphics, DjVu, DPX, Photo CD, PCX, PGF, PICT, PPM, Sony PMP, Casio CAM, XISF |
| **Camera RAW** | [36 formats from 20+ makers](/reference/formats/camera-raw) |
| **Video and containers** | QuickTime/MP4 family (MOV, MP4, M4A, 3GP, …), Matroska MKV, WebM (only with `--detector magika`; signature detection reports WebM files as MKV), FLV, SWF, AVI, MPEG-2 TS (MTS/M2TS), ASF/WMV, MXF, DV, WTV, RealMedia, RED R3D, MOI |
| **Audio** | MP3, FLAC, AAC, WAV, AIFF, Ogg Vorbis, Opus, Monkey's Audio (APE), Musepack (MPC), RealAudio (RA, RAM), Olympus DSS, Audible AA |
| **Documents and text** | PDF, EPS/PostScript, DOCX, XLSX, PPTX, Apple Pages/Numbers/Keynote, EPUB, OLE compound documents (DOC, XLS, PPT, FlashPix), InDesign, HTML, plain text, CSV, vCard, iCalendar, email (EML), TNEF, Palm database (PDB, MOBI), XMP, XML, property lists |
| **Archives** | ZIP, Capture One EIP, RAR, 7z, ISO 9660, TAR, `ar`, gzip, BitTorrent |
| **Fonts** | TrueType, OpenType, WOFF, WOFF2, AFM, PFB, PFM (printer font metrics), Mac resource fonts (dfont) |
| **Executables** | Windows PE (EXE/DLL; [details](/reference/formats/pe-executable), [Rich header](/features/pe-rich-header)), ELF, Mach-O |
| **Other** | ICC profiles, X.509 certificates, Canon VRD/DR4, MIE, macOS sidecars, Windows shortcuts (LNK), Lytro LFP, SQLite, Windows Prefetch, Windows Registry hives, Windows event logs (EVTX), PCAP/PCAPNG, DWG, DXF, STL, OBJ, glTF, Garmin FIT, FITS, HDF5, DICOM, MRC, Zeiss CZI, iTunes ITC, FLIR FPF |

### MakerNotes

Maker notes are dispatched by signature or by `Make`
(`src/parsers/tiff/makernote_dispatcher.rs`). The dispatcher covers Canon,
Nikon, Sony, Panasonic, FujiFilm, Olympus/OM Digital, Pentax/Asahi/Ricoh
Imaging, Samsung, Leica, Minolta/Konica Minolta, Apple, DJI, FLIR, GoPro,
InfiRay, Nintendo, Parrot, Reconyx, RED, Casio, GE/General Imaging, HP, JVC,
Kodak, Motorola, Ricoh, Sanyo, Phase One and Sigma. It also covers
maker-note blocks written by software: Capture One, FotoStation, GIMP,
InDesign, Nikon Capture, Photoshop and Scalado. See
[MakerNotes](/reference/makernotes).

## Parsed generically

These are identified correctly, but read by a generic reader:

- **Text read as plain text**: RTF, JSON, `.url` files and InDesign INX.
  They get `File:LineCount`, `WordCount`, `Newlines` and `MIMEEncoding`, not
  ExifTool's tags for the format.
- **ZIP-based documents read as ZIP**: OpenDocument (ODS, ODT, …), IDML,
  Sketch and VSDX. They get `ZIP:` tags, not their document properties.

## Identified only

OxiDex's type identification is generated from ExifTool's own extension and
magic-number tables. It knows 299 FileTypes, far more than have parsers. A
file of a type with no parser is returned with its identity and filesystem
tags only. On a release build, WMF and JPEG XR (JXR) files, for example,
come back as `"Status": "IdentifiedOnly"`.

Reading the detection code at the tip, these ExifTool types have no parser
route, so expect them to be identified at best: EXV, BZ2, MNG, JNG, TTC,
CUR, CHM, DEX, JUMBF/C2PA, DWF, DSF, LA, OFR, PAC, WavPack, FLIR SEQ/FFF,
WMF, JXR/HDP/WDP, DCX, LIF, RWZ, NKA, AVC, QTIF, MPEG program streams
(MPEG, M2V, VOB) and LRI. This list comes from reading code, not from
running sample files. Treat it as a guide, and check a file with `oxidex -j`.

## Writable formats

| Format | Written |
| --- | --- |
| JPEG | EXIF (APP1) only |
| TIFF, and TIFF-structured RAW (NEF, CR2, ARW, DNG, PEF, RW2, IIQ, …) | IFD entries, surgically in place |
| PNG | `tEXt`, `iTXt`, `zTXt`, `eXIf` |
| PDF | an appended Info dictionary |

Nothing else can be written, including BigTIFF, ORF, RAF, MRW, X3F, CR3,
CRW, video and HEIC. Most writable tags are **not yet proven** against
ExifTool; see [Writing metadata](/guide/writing).

## Tag definitions

The tag database holds 16,684 tag definitions, synced from ExifTool's
documentation view (`exiftool -listx`). They are browsable by domain under
[Tag domains](/tag-domains/). A definition says ExifTool knows a tag, not
that OxiDex reads it.

## Implementation notes

- [Camera RAW](/reference/formats/camera-raw)
- [PE executables](/reference/formats/pe-executable) and [PE Rich header](/features/pe-rich-header)
- [GPS movement tags](/features/GPS_MOVEMENT_TRACKING)
- [Office forensic metadata](/features/OFFICE_FORENSIC_METADATA)
