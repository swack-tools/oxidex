# Supported Formats

OxiDex supports 140+ file format families with comprehensive metadata extraction and writing capabilities.

**Legend:**
- ✅ **Fully Implemented** - Read and write support with comprehensive tag coverage
- 🔄 **Partial Support** - Read support implemented, write support limited
- ⏳ **Planned** - On the roadmap for future implementation

## Image Formats

### JPEG (Joint Photographic Experts Group)

**Status:** ✅ Fully Implemented

**File Extensions:** `.jpg`, `.jpeg`, `.jpe`

**Metadata Types:**
- ✅ EXIF (Exchangeable Image File Format)
- ✅ XMP (Extensible Metadata Platform)
- ✅ IPTC (International Press Telecommunications Council)
- ✅ JFIF (JPEG File Interchange Format)
- ✅ GPS (Geolocation data)
- ✅ ICC Profile (Color management)
- ✅ Photoshop metadata
- ✅ Thumbnail extraction

**Available Tags:** 244 EXIF tags + 122 IPTC tags + XMP support

**Common Use Cases:**
- Digital camera photos
- Web images
- Social media uploads
- Scanned documents

### TIFF (Tagged Image File Format)

**Status:** ✅ Fully Implemented

**File Extensions:** `.tif`, `.tiff`

**Metadata Types:**
- ✅ EXIF
- ✅ XMP
- ✅ IPTC
- ✅ GPS
- ✅ ICC Profile
- ✅ Photoshop
- ✅ Multi-page/multi-image support

**Available Tags:** 244 EXIF tags + TIFF-specific tags

**Common Use Cases:**
- Professional photography
- Archival images
- Scientific imaging
- Medical imaging (DICOM-TIFF)

### PNG (Portable Network Graphics)

**Status:** ✅ Fully Implemented

**File Extensions:** `.png`

**Metadata Types:**
- ✅ PNG text chunks (tEXt, zTXt, iTXt)
- ✅ XMP
- ✅ EXIF (embedded via PNG chunks)
- ✅ ICC Profile
- ✅ Creation time (tIME chunk)
- ✅ Physical dimensions (pHYs chunk)

**Available Tags:** 30 PNG-specific tags + EXIF/XMP support

**Common Use Cases:**
- Web graphics
- Screenshots
- Lossless image archiving
- Images with transparency

### RAW Camera Formats

**Status:** ✅ Fully Implemented (40+ formats)

**Supported Formats:**

- **Canon:** CR2, CR3, CRW
- **Nikon:** NEF, NRW
- **Sony:** ARW, SR2, SRF, SRW, ARQ, ARI
- **Fujifilm:** RAF
- **Olympus:** ORF, ORI
- **Pentax:** PEF
- **Panasonic:** RW2, RWL
- **Hasselblad:** 3FR, FFF
- **Phase One:** IIQ
- **Mamiya:** MEF
- **Leaf:** MOS
- **Kodak:** DCR, KDC
- **Minolta:** MDC, MRW
- **Epson:** ERF
- **Sigma:** X3F
- **GoPro:** GPR
- **Adobe:** DNG (Digital Negative)
- **HEIF:** HIF
- **Light:** LRI
- **Sinar:** STI
- **Generic:** RAW, CAM, REV

**Metadata Support:** EXIF, XMP, maker notes for each manufacturer

## Video Formats

### MP4/QuickTime

**Status:** ✅ Fully Implemented

**File Extensions:** `.mp4`, `.m4v`, `.mov`, `.3gp`, `.3g2`

**Metadata Types:**
- ✅ QuickTime atoms (moov, udta, meta)
- ✅ Creation/modification times
- ✅ Duration, dimensions
- ✅ GPS coordinates
- ✅ Camera make/model
- ✅ XMP packets

**Available Tags:** 143 QuickTime-specific tags

**Common Use Cases:**
- Video library management
- Smartphone video metadata
- Media asset databases
- GPS-tagged videos

### Additional Video Formats

**Status:** ✅ Phase 1 Complete

- **MKV/WebM** (Matroska) - ✅ Complete
- **FLV** (Flash Video) - ✅ Complete
- **AVI** (Audio Video Interleave) - ✅ Complete
- **MTS/M2TS** (AVCHD) - ✅ Complete

## Audio Formats

**Status:** ✅ Phase 1 Complete

- **MP3** (ID3 tags) - ✅ Complete
- **FLAC** (Free Lossless Audio Codec) - ✅ Complete
- **AAC** (Advanced Audio Coding) - ✅ Complete
- **WAV** (Waveform Audio) - ✅ Complete
- **OGG Vorbis** - ✅ Complete
- **Opus** - ✅ Complete
- **APE** (Monkey's Audio) - ✅ Complete

## Document Formats

### PDF (Portable Document Format)

**Status:** ✅ Fully Implemented

**File Extensions:** `.pdf`

**Metadata Types:**
- ✅ PDF Info Dictionary (Title, Author, Subject, Keywords)
- ✅ Creation/Modification dates
- ✅ XMP metadata packets
- ✅ ICC profiles

**Common Use Cases:**
- Document metadata extraction
- PDF library management
- Compliance and archiving

### PE (Portable Executable)

**Status:** ✅ Fully Implemented

**File Extensions:** `.exe`, `.dll`, `.sys`

**Metadata Types:**
- ✅ Version information
- ✅ File properties
- ✅ Digital signatures
- ✅ Resource metadata

**Common Use Cases:**
- Windows executable analysis
- Software inventory
- Security auditing

## Metadata Standards

### EXIF (Exchangeable Image File Format)

**Status:** ✅ Comprehensive Support

**Supported in Formats:** JPEG, TIFF, PNG, RAW formats

**Tag Categories:**
- Image structure (width, height, color space)
- Camera settings (ISO, aperture, shutter speed, focal length)
- Camera identification (make, model, serial number)
- Date/time stamps (original, digitized, modified)
- Image processing (white balance, exposure compensation, flash)
- Thumbnail images
- Copyright and author information

**Available Tags:** 718 tags from EXIF specification

**Standards Compliance:** EXIF 2.3 specification

### XMP (Extensible Metadata Platform)

**Status:** ✅ Fully Implemented

**Supported in Formats:** JPEG, TIFF, PNG, PDF, MP4

**XMP Namespaces:**
- `dc` (Dublin Core) - Title, Creator, Rights, Description
- `xmp` - Base XMP properties
- `xmpRights` - Copyright management
- `photoshop` - Adobe Photoshop metadata
- `exif` - EXIF properties in XMP format
- `tiff` - TIFF properties in XMP format
- `aux` - Additional camera metadata
- `iptcCore` - IPTC Core metadata
- `iptcExt` - IPTC Extension metadata
- `plus` - Picture Licensing Universal System

**Read Operations:** ✅ Full XML parsing
**Write Operations:** ✅ Full XML serialization

### IPTC (International Press Telecommunications Council)

**Status:** ✅ Fully Implemented

**Supported in Formats:** JPEG, TIFF

**IPTC Categories:**
- Descriptive metadata (Caption, Keywords, Headline)
- Administrative metadata (Credit, Source, Copyright Notice)
- People and locations (City, Province, Country, Creator)
- Rights information (Usage Terms, Copyright Notice)
- Technical metadata (Date Created, Digital Creation Date)

**Available Tags:** 122 IPTC tags

**Standards Compliance:** IPTC Core 1.3, IPTC Extension

### GPS Metadata

**Status:** ✅ Fully Implemented

**Supported in Formats:** JPEG, TIFF, MP4/MOV, RAW

**GPS Tags:**
- Latitude, Longitude (decimal degrees)
- Altitude (meters above sea level)
- Timestamp (UTC)
- Speed, Track (direction of movement)
- Satellites used, DOP (dilution of precision)
- Map Datum (coordinate system)
- Differential correction

**Available Tags:** 32 GPS-specific tags

**Coordinate Formats:** Decimal degrees, degrees/minutes/seconds (DMS)

## Additional Metadata

### ICC Profile (Color Management)

**Status:** ✅ Fully Implemented

**Supported in Formats:** JPEG, TIFF, PNG, PDF

**Profile Information:**
- Profile description
- Color space (RGB, CMYK, Lab)
- Rendering intent
- White point, primaries
- Gamma/transfer curve

**Available Tags:** 90 ICC Profile tags

### Photoshop Metadata

**Status:** ✅ Fully Implemented

**Supported in Formats:** JPEG, TIFF, PNG

**Photoshop Resources:**
- Image resources (layers, paths)
- Copyright flag
- URL
- Credit, Source
- Caption Writer

**Available Tags:** 136 Photoshop-specific tags

### Maker Notes

**Status:** ✅ Comprehensive Support (30+ manufacturers)

**Canon MakerNotes:** 930 tags
- Phase 1: Basic tags (ImageType, SerialNumber, ModelID)
- Phase 2: Array tags (CameraSettings, ShotInfo, FocalLength)
- Phase 3: Advanced (Lens database, AFInfo, FileInfo)

**Nikon MakerNotes:** 2,398 tags (main) + 3,512 tags (NikonCustom)
**Sony MakerNotes:** 1,148 tags
**Pentax MakerNotes:** 876 tags

## Format Detection

OxiDex uses **magic number detection** to identify file formats:

1. Reads the first few bytes (magic number)
2. Matches against known format signatures
3. Falls back to file extension if ambiguous

**Format Signatures:**

| Format | Magic Bytes | Offset |
|--------|-------------|--------|
| JPEG | `FF D8 FF` | 0 |
| PNG | `89 50 4E 47 0D 0A 1A 0A` | 0 |
| TIFF (LE) | `49 49 2A 00` | 0 |
| TIFF (BE) | `4D 4D 00 2A` | 0 |
| PDF | `25 50 44 46` (`%PDF`) | 0 |
| MP4/MOV | `66 74 79 70` (`ftyp`) | 4 |

This ensures robust format detection even with incorrect extensions.

## Performance by Format

**Relative Performance** (compared to baseline JPEG parsing):

| Format | Read Speed | Write Speed | Notes |
|--------|-----------|-------------|-------|
| JPEG | 1.0x (baseline) | 1.0x | Optimized segment parsing |
| TIFF | 0.9x | 0.9x | IFD chain traversal |
| PNG | 1.1x | 1.1x | Simple chunk-based format |
| PDF | 0.8x | 0.8x | Object parsing |
| MP4/MOV | 0.7x | N/A | Atom tree traversal |

All formats process typical files in < 50ms on modern hardware.

## Tag Database Statistics

**Total Tags:** 32,677 (113% of ExifTool's 28,853 tags)

**Tags by Format Family:**
- NikonCustom: 3,512 tags
- DICOM: 3,149 tags
- Nikon: 2,398 tags
- Sony: 1,148 tags
- QuickTime: 1,069 tags
- Canon: 930 tags
- Casio: 930 tags
- Pentax: 876 tags
- EXIF: 718 tags
- And 131+ more format families

## Checking Format Support

Use the CLI to check format support:

```bash
oxidex photo.jpg
```

Supported formats will display metadata. Unsupported formats show:

```
Error: Unsupported file format: unknown
```

## Future Roadmap

**v1.1 (Current):** 140+ format families with full read/write support ✅
**v2.0 Goal:** Enhanced maker notes support for all major camera vendors
**v3.0 Goal:** Complete ExifTool tag parity (28,853+ tags)

## Additional Resources

- [Tag Database](/reference/tag-database) - Complete tag reference
- [API Reference](/reference/api-reference) - Using formats in code
- [CLI Usage](/guide/cli-usage) - Command-line examples
- [ExifTool Format Support](https://exiftool.org/#supported) - Original format list
