# Supported Formats

This chapter lists the file formats and metadata standards currently supported by ExifTool-RS.

## Implementation Status

ExifTool-RS is actively being developed. The current implementation (v0.1.0) focuses on core formats with comprehensive metadata support. Additional formats will be added in future releases.

**Legend:**
- ✅ **Fully Implemented**: Read and write support with comprehensive tag coverage
- 🔄 **Partial Support**: Read support implemented, write support in progress or limited
- ⏳ **Planned**: On the roadmap for future implementation
- ❌ **Not Supported**: Not currently planned

## Image Formats

### JPEG (Joint Photographic Experts Group)

**Status**: ✅ Fully Implemented

**File Extensions**: `.jpg`, `.jpeg`, `.jpe`

**Metadata Types Supported:**
- ✅ EXIF (Exchangeable Image File Format)
- ✅ XMP (Extensible Metadata Platform)
- ✅ IPTC (International Press Telecommunications Council)
- ✅ JFIF (JPEG File Interchange Format)
- ✅ GPS (Geolocation data)
- ✅ ICC Profile (Color management)
- ✅ Photoshop metadata
- ✅ Thumbnail extraction

**Read Operations**: ✅ Full support
**Write Operations**: ✅ Full support

**Available Tags**: 244 EXIF tags + 122 IPTC tags + XMP support

**Common Use Cases:**
- Digital camera photos
- Web images
- Social media uploads
- Scanned documents

### TIFF (Tagged Image File Format)

**Status**: ✅ Fully Implemented

**File Extensions**: `.tif`, `.tiff`

**Metadata Types Supported:**
- ✅ EXIF
- ✅ XMP
- ✅ IPTC
- ✅ GPS
- ✅ ICC Profile
- ✅ Photoshop
- ✅ Multi-page/multi-image support

**Read Operations**: ✅ Full support
**Write Operations**: ✅ Full support

**Available Tags**: 244 EXIF tags + additional TIFF-specific tags

**Common Use Cases:**
- Professional photography
- Archival images
- Scientific imaging
- Medical imaging (DICOM-TIFF)

### PNG (Portable Network Graphics)

**Status**: ✅ Fully Implemented

**File Extensions**: `.png`

**Metadata Types Supported:**
- ✅ PNG text chunks (tEXt, zTXt, iTXt)
- ✅ XMP
- ✅ EXIF (embedded via PNG chunks)
- ✅ ICC Profile
- ✅ Creation time (tIME chunk)
- ✅ Physical dimensions (pHYs chunk)

**Read Operations**: ✅ Full support
**Write Operations**: ✅ Full support

**Available Tags**: 30 PNG-specific tags + EXIF/XMP support

**Common Use Cases:**
- Web graphics
- Screenshots
- Lossless image archiving
- Images with transparency

## Document Formats

### PDF (Portable Document Format)

**Status**: 🔄 Partial Support

**File Extensions**: `.pdf`

**Metadata Types Supported:**
- ✅ PDF Info Dictionary (Title, Author, Subject, Keywords, Creator, Producer)
- ✅ Creation/Modification dates
- ✅ XMP metadata packets
- 🔄 Embedded image metadata (read-only)

**Read Operations**: ✅ Supported
**Write Operations**: ⏳ Planned for future release

**Available Tags**: PDF Info keys + XMP support

**Common Use Cases:**
- Document metadata extraction
- PDF library management
- Compliance and archiving

## Video Formats

### MP4/QuickTime

**Status**: ✅ Fully Implemented

**File Extensions**: `.mp4`, `.m4v`, `.mov`, `.3gp`, `.3g2`

**Metadata Types Supported:**
- ✅ QuickTime atoms (moov, udta, meta)
- ✅ Creation/modification times
- ✅ Duration, dimensions
- ✅ GPS coordinates (from video metadata)
- ✅ Camera make/model
- ✅ XMP packets

**Read Operations**: ✅ Full support
**Write Operations**: ⏳ Planned for future release

**Available Tags**: 143 QuickTime-specific tags

**Common Use Cases:**
- Video library management
- Smartphone video metadata
- Media asset databases
- GPS-tagged videos

## Metadata Standards

### EXIF (Exchangeable Image File Format)

**Status**: ✅ Comprehensive Support

**Supported in Formats**: JPEG, TIFF, PNG (embedded), RAW formats

**Tag Categories:**
- Image structure (width, height, color space)
- Camera settings (ISO, aperture, shutter speed, focal length)
- Camera identification (make, model, serial number)
- Date/time stamps (original, digitized, modified)
- Image processing (white balance, exposure compensation, flash)
- Thumbnail images
- Copyright and author information

**Available Tags**: 244 tags from ExifTool spec

**Standards Compliance**: EXIF 2.3 specification

### XMP (Extensible Metadata Platform)

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF, PNG, PDF, MP4

**XMP Namespaces Supported:**
- `dc` (Dublin Core): Title, Creator, Rights, Description
- `xmp`: Base XMP properties
- `xmpRights`: Copyright management
- `photoshop`: Adobe Photoshop metadata
- `exif`: EXIF properties in XMP format
- `tiff`: TIFF properties in XMP format
- `aux`: Additional camera metadata

**Read Operations**: ✅ Full XML parsing
**Write Operations**: ✅ Full XML serialization

**Available Tags**: 7 base XMP tags + namespace-specific tags

### IPTC (International Press Telecommunications Council)

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF

**IPTC Categories:**
- Descriptive metadata (Caption, Keywords, Headline)
- Administrative metadata (Credit, Source, Copyright Notice)
- People and locations (City, Province, Country, Creator)
- Rights information (Usage Terms, Copyright Notice)
- Technical metadata (Date Created, Digital Creation Date)

**Read Operations**: ✅ Full support
**Write Operations**: ✅ Full support

**Available Tags**: 122 IPTC tags

**Standards Compliance**: IPTC Core 1.3, IPTC Extension

### GPS Metadata

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF, MP4/MOV

**GPS Tags Supported:**
- Latitude, Longitude (decimal degrees)
- Altitude (meters above sea level)
- Timestamp (UTC)
- Speed, Track (direction of movement)
- Satellites used, DOP (dilution of precision)
- Map Datum (coordinate system)
- Differential correction

**Available Tags**: 32 GPS-specific tags

**Coordinate Formats**: Decimal degrees, degrees/minutes/seconds (DMS)

## Additional Metadata Families

### ICC Profile (Color Management)

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF, PNG

**Profile Information:**
- Profile description
- Color space (RGB, CMYK, Lab)
- Rendering intent
- White point, primaries
- Gamma/transfer curve

**Available Tags**: 42 ICC Profile tags

### Photoshop Metadata

**Status**: ✅ Fully Implemented

**Supported in Formats**: JPEG, TIFF, PNG

**Photoshop Resources:**
- Image resources (layers, paths)
- Copyright flag
- URL
- Credit, Source
- Caption Writer

**Available Tags**: 35 Photoshop-specific tags

### RIFF (Resource Interchange File Format)

**Status**: 🔄 Parser Implemented

**Supported in Formats**: AVI, WAV (planned)

**Available Tags**: 46 RIFF tags

**Status**: Tag database generated, parser infrastructure ready

## Tag Database Statistics

ExifTool-RS automatically generates its tag database from the official ExifTool source during build:

**Current Tag Count**: 731 tags (v0.1.0)

**Tags by Format Family:**
- EXIF: 244 tags
- QuickTime: 143 tags
- IPTC: 122 tags
- RIFF: 46 tags
- ICC_Profile: 42 tags
- Photoshop: 35 tags
- GPS: 32 tags
- PNG: 30 tags
- JPEG: 30 tags
- XMP: 7 tags (base module)

**Total Coverage**: ~700+ unique metadata tags across 10+ format families

## Planned Format Support

The following formats are on the roadmap for future releases:

### High Priority

- **RAW Formats** (⏳ Planned):
  - CR2, CR3 (Canon)
  - NEF (Nikon)
  - ARW (Sony)
  - DNG (Adobe Digital Negative)
  - ORF (Olympus)
  - RAF (Fujifilm)

- **Additional Video Formats** (⏳ Planned):
  - AVI (Audio Video Interleave)
  - MKV (Matroska)
  - WebM

### Medium Priority

- **Document Formats** (⏳ Planned):
  - DOCX (Microsoft Word)
  - XLSX (Microsoft Excel)
  - PPTX (Microsoft PowerPoint)
  - ODT, ODS, ODP (OpenDocument)

- **Audio Formats** (⏳ Planned):
  - MP3 (ID3 tags)
  - FLAC
  - M4A/AAC
  - WAV (RIFF metadata)
  - OGG Vorbis

### Lower Priority

- **Archive Formats** (⏳ Planned):
  - ZIP (embedded metadata)
  - 7z

- **Vector Formats** (⏳ Planned):
  - SVG (XML metadata)
  - AI (Adobe Illustrator)
  - EPS (Encapsulated PostScript)

## Checking Format Support

To check if a file format is supported, use the CLI:

```bash
exiftool-rs photo.unknown
```

If the format is not supported, you'll see:

```
Error: Unsupported file format: unknown
```

Supported formats will display metadata or indicate no metadata was found.

## Format Detection

ExifTool-RS uses **magic number detection** to identify file formats:

1. Reads the first few bytes of the file (magic number)
2. Matches against known format signatures
3. Falls back to file extension if magic number is ambiguous

**Format Signatures:**

| Format | Magic Bytes | Offset |
|--------|-------------|--------|
| JPEG | `FF D8 FF` | 0 |
| PNG | `89 50 4E 47 0D 0A 1A 0A` | 0 |
| TIFF (LE) | `49 49 2A 00` | 0 |
| TIFF (BE) | `4D 4D 00 2A` | 0 |
| PDF | `25 50 44 46` (`%PDF`) | 0 |
| MP4/MOV | `66 74 79 70` (`ftyp`) | 4 |

This ensures robust format detection even when files have incorrect extensions.

## Performance by Format

**Relative Performance** (compared to baseline JPEG parsing):

| Format | Read Speed | Write Speed | Notes |
|--------|-----------|-------------|-------|
| JPEG | 1.0x (baseline) | 1.0x | Optimized segment parsing |
| TIFF | 0.9x | 0.9x | IFD chain traversal |
| PNG | 1.1x | 1.1x | Simple chunk-based format |
| PDF | 0.5x | N/A | Complex object parsing |
| MP4/MOV | 0.7x | N/A | Atom tree traversal |

All formats process typical files in < 10ms on modern hardware.

## Contributing New Format Support

Interested in adding support for a new format? See the [Contributing Guide](https://github.com/exiftool-rs/exiftool-rs/blob/main/CONTRIBUTING.md) for:

- Parser implementation guidelines
- Format-specific testing requirements
- Tag database integration
- Documentation standards

## Additional Resources

- **[ExifTool Format Support](https://exiftool.org/#supported)**: Official ExifTool format list (300+ formats)
- **[Command-Line Usage](cli_usage.md)**: How to use the CLI with different formats
- **[Library API](library_api.md)**: Programmatic format detection and parsing
- **[Installation](installation.md)**: Tag database generation from ExifTool source

## Future Roadmap

**v1.0 Goal**: 10+ core formats with full read/write support (DONE)
**v2.0 Goal**: 50+ formats including RAW, video, audio
**v3.0 Goal**: 200+ formats approaching ExifTool feature parity
**Long-term**: 300+ formats with 28,000+ tags (full ExifTool compatibility)

We welcome contributions to accelerate format support development!
