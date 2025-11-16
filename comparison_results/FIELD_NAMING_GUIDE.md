# Field Naming Differences: Perl vs Rust

## Overview
exiftool-rs uses structured field names with prefixes to indicate the metadata source, while Perl ExifTool uses simplified "friendly" names. The actual data extracted is identical.

## Naming Patterns

### 1. Space Removal
Perl uses spaces in field names, Rust uses camelCase or concatenation:
- `Bits Per Sample` → `BitsPerSample`
- `Image Width` → `ImageWidth`
- `Strip Byte Counts` → `StripByteCounts`

### 2. Prefixes
Rust adds prefixes to show metadata source:

#### JPEG/TIFF Prefixes
- `IFD0:` - Main image metadata
- `ExifIFD:` - EXIF-specific data
- `XMP-dc:` - XMP Dublin Core
- `XMP-xmp:` - XMP basic schema

**Examples:**
```
Perl: Make
Rust: IFD0:Make

Perl: Creator
Rust: XMP-xmp:Creator

Perl: Color Space
Rust: ExifIFD:ColorSpace
```

#### PNG Prefixes
- `PNG:` - PNG chunk metadata
- `PNG-pHYs:` - Physical pixel dimensions
- `PNG:tEXt:` - Text metadata chunks
- `IFD0:` - Embedded EXIF metadata

**Examples:**
```
Perl: Image Width
Rust: PNG:ImageWidth

Perl: Pixels Per Unit X
Rust: PNG-pHYs:PixelsPerUnitX

Perl: Datecreate
Rust: PNG:tEXt:date:create
```

#### PDF Prefixes
- `PDF:` - PDF document metadata
- `Profile:` - ICC color profile data

**Examples:**
```
Perl: PDF Version
Rust: PDF:PDFVersion

Perl: Profile Description
Rust: Profile:ProfileDescription
```

#### MP4/QuickTime Prefixes
- `QuickTime:` - QuickTime container metadata
- `ItemList:` - Metadata item list (user data)
- `UserData:` - User data atoms

**Examples:**
```
Perl: Major Brand
Rust: QuickTime:MajorBrand

Perl: Artist
Rust: ItemList:Artist
```

### 3. Friendly Name Differences

Some fields have completely different names:

| Perl ExifTool | exiftool-rs | Notes |
|---------------|-------------|-------|
| Camera Model Name | Model | Perl adds "Camera" prefix |
| Primary Chromaticities | PrimaryChromaticities | Space removed |
| Photometric Interpretation | PhotometricInterpretation | Space removed |
| Y Cb Cr Positioning | YCbCrPositioning | Space removed |

## Computed Fields (Perl Only)

These fields are NOT in exiftool-rs because they're computed from other metadata:

### Image Dimensions
- **Image Size** - Computed as "Width×Height" (e.g., "800×600")
- **Megapixels** - Computed as (Width × Height) / 1,000,000

Both Width and Height are available separately in Rust.

### Video Fields
- **Avg Bitrate** - Computed as File Size / Duration

### Diagnostic Fields
- **Warning** - Validation messages (e.g., "Text/EXIF chunk(s) found after PNG IDAT")

### Internal Metadata
- **Exif Byte Order** - Indicates little-endian vs big-endian encoding
- **File Type**, **File Type Extension**, **MIME Type** - File classification

## Quick Reference: Common Fields

### JPEG/TIFF
```
Perl                    → Rust
----------------------------------------
Make                    → IFD0:Make
Camera Model Name       → IFD0:Model
Orientation             → IFD0:Orientation
X Resolution            → IFD0:XResolution
Y Resolution            → IFD0:YResolution
Resolution Unit         → IFD0:ResolutionUnit
Artist                  → IFD0:Artist
Date/Time Original      → ExifIFD:DateTimeOriginal
Color Space             → ExifIFD:ColorSpace
Exif Version            → ExifIFD:ExifVersion
```

### PNG
```
Perl                    → Rust
----------------------------------------
Image Width             → PNG:ImageWidth
Image Height            → PNG:ImageHeight
Bit Depth               → PNG:BitDepth
Color Type              → PNG:ColorType
Compression             → PNG:Compression
Interlace               → PNG:Interlace
Modify Date             → PNG:ModifyDate
```

### PDF
```
Perl                    → Rust
----------------------------------------
PDF Version             → PDF:PDFVersion
Page Count              → PDF:PageCount
Producer                → PDF:Producer
Create Date             → PDF:CreateDate
Modify Date             → PDF:ModifyDate
Linearized              → PDF:Linearized
```

### MP4/QuickTime
```
Perl                    → Rust
----------------------------------------
Major Brand             → QuickTime:MajorBrand
Compatible Brands       → QuickTime:CompatibleBrands
Duration                → QuickTime:Duration
Time Scale              → QuickTime:TimeScale
Title                   → ItemList:Title
Artist                  → ItemList:Artist
Album                   → ItemList:Album
Genre                   → ItemList:Genre
Copyright               → ItemList:Copyright
```

## Benefits of Rust Naming Convention

1. **Source Clarity**: Immediately know where metadata came from (IFD0, EXIF, XMP, etc.)
2. **Namespace Prevention**: Avoid conflicts when same field name exists in multiple sources
3. **Programmatic Parsing**: Easier to filter/group by prefix in code
4. **Standard Compliance**: Matches actual metadata structure specifications

## Backward Compatibility

If you have scripts expecting Perl field names, you can:
1. Strip prefixes: `IFD0:Make` → `Make`
2. Add friendly aliases in your application layer
3. Use the raw field name for lookups, Rust will find it

