# Test Corpus Acquisition Guide

This guide provides instructions for expanding the test corpus to meet the 100+ image requirement for comprehensive ExifTool comparison testing.

## Current Status

- **Current Count**: 5 images
- **Target Count**: 100+ images
- **Progress**: 5% complete

## Acquisition Strategy

### Phase 1: Public Test Suites (40-50 images)

#### Exiv2 Test Suite
- **URL**: https://github.com/Exiv2/exiv2/tree/main/test/data
- **License**: GPL-2.0+ (compatible with our GPL-3.0)
- **Estimated Count**: 30-40 images
- **Formats**: JPEG, TIFF, PNG with diverse EXIF/IPTC/XMP
- **Quality**: High - professionally curated test suite

**Acquisition Steps**:
```bash
# Clone Exiv2 repository (sparse checkout for test data only)
git clone --depth 1 --filter=blob:none --sparse https://github.com/Exiv2/exiv2.git
cd exiv2
git sparse-checkout set test/data

# Copy relevant images to ExifTool-RS fixtures
cp test/data/*.jpg ../exiftools/tests/fixtures/jpeg/complex/
cp test/data/*.tif ../exiftools/tests/fixtures/tiff/complex/
cp test/data/*.png ../exiftools/tests/fixtures/png/complex/

# Document sources in manifest.json
```

**Recommended Files** (select diverse subset):
- `test/data/exiv2-bug*.jpg` - JPEG with edge cases
- `test/data/iptc-*.jpg` - JPEG with IPTC metadata
- `test/data/Reagan*.jpg` - JPEG with comprehensive metadata
- `test/data/*.tif` - Various TIFF variants

#### ExifTool Test Files
- **URL**: https://exiftool.org/sample_images.html
- **License**: Public domain samples
- **Estimated Count**: 10-15 images
- **Formats**: All major formats with manufacturer examples

**Acquisition Steps**:
```bash
# Download sample images
wget https://exiftool.org/samples/Canon.jpg -O tests/fixtures/jpeg/complex/canon_sample.jpg
wget https://exiftool.org/samples/Nikon.jpg -O tests/fixtures/jpeg/complex/nikon_sample.jpg
wget https://exiftool.org/samples/Sony.jpg -O tests/fixtures/jpeg/complex/sony_sample.jpg
# ... add more samples
```

### Phase 2: Public Domain Images (20-30 images)

#### Unsplash (CC0 License)
- **URL**: https://unsplash.com
- **License**: CC0 (public domain)
- **Estimated Count**: 20-30 images
- **Focus**: Real-world photos with GPS metadata

**Acquisition Steps**:
```bash
# Use Unsplash API or download manually
# Select images with:
# - GPS coordinates (outdoor/landscape photos)
# - Modern camera EXIF (Canon, Nikon, Sony, Fuji)
# - Diverse scenes (portraits, landscapes, urban, nature)

# Example downloads (replace with actual URLs)
wget "https://unsplash.com/photos/[ID]/download?force=true" -O tests/fixtures/jpeg/simple/unsplash_001.jpg
# ... document sources in manifest.json
```

**Search Criteria**:
- Keywords: "landscape", "travel", "architecture"
- Filters: High resolution (3000x2000+), outdoor shots (likely GPS)
- Cameras: Canon EOS, Nikon D-series, Sony Alpha

#### Wikimedia Commons
- **URL**: https://commons.wikimedia.org
- **License**: CC0 or CC-BY (verify each file)
- **Estimated Count**: 10-15 images
- **Focus**: Historical images, diverse sources

### Phase 3: Synthetic Test Images (20-30 images)

Create synthetic images with known metadata for edge cases and testing.

#### Using ImageMagick + ExifTool

```bash
# Generate base image
convert -size 800x600 xc:blue tests/fixtures/jpeg/edge_cases/synthetic_001.jpg

# Add metadata with Perl ExifTool
exiftool -Artist="Test Artist" \
         -Copyright="CC0 Public Domain" \
         -Make="Synthetic Camera" \
         -Model="Test Model 1" \
         -DateTimeOriginal="2024:01:01 12:00:00" \
         -GPSLatitude="37.7749" \
         -GPSLongitude="-122.4194" \
         -overwrite_original \
         tests/fixtures/jpeg/edge_cases/synthetic_001.jpg
```

#### Synthetic Test Scenarios

1. **Large Dimensions** (edge case)
   - 8000x6000 pixels
   - Tests memory handling

2. **GPS Coordinates** (tolerance validation)
   - Known coordinates with high precision
   - Validates ±0.0001° tolerance

3. **Unusual Orientations** (rotation metadata)
   - Orientation: 1-8 (all EXIF orientations)

4. **Multi-Format Metadata** (EXIF+XMP+IPTC)
   - Same tags in multiple formats
   - Tests priority handling

5. **Unicode in Tags** (internationalization)
   - Chinese, Arabic, Cyrillic characters
   - Tests UTF-8 handling

6. **Very Long Strings** (boundary testing)
   - 256+ character descriptions
   - Tests buffer limits

**Synthetic Image Script**:
```bash
#!/bin/bash
# create_synthetic_fixtures.sh

for i in {1..10}; do
  convert -size 800x600 xc:blue "tests/fixtures/jpeg/edge_cases/synthetic_$(printf %03d $i).jpg"

  # Add unique metadata
  exiftool -Artist="Synthetic Artist $i" \
           -DateTimeOriginal="2024:01:$i 12:00:00" \
           -GPSLatitude="$((37 + i * 0.001))" \
           -GPSLongitude="$((122 + i * 0.001))" \
           -overwrite_original \
           "tests/fixtures/jpeg/edge_cases/synthetic_$(printf %03d $i).jpg"
done
```

### Phase 4: Format-Specific Test Cases (10-20 images)

#### PNG Test Cases
```bash
# PNG with text chunks
convert -size 640x480 xc:white tests/fixtures/png/simple/text_chunk.png
exiftool -Title="PNG Title" -Author="PNG Author" tests/fixtures/png/simple/text_chunk.png

# PNG with eXIf chunk (requires ImageMagick 7+)
convert tests/fixtures/jpeg/simple/sample_with_exif.jpg tests/fixtures/png/complex/with_exif.png
```

#### TIFF Test Cases
```bash
# Multi-page TIFF
convert tests/fixtures/jpeg/simple/*.jpg tests/fixtures/tiff/complex/multipage.tif

# Big-endian TIFF
convert -endian MSB tests/fixtures/jpeg/simple/sample_with_exif.jpg tests/fixtures/tiff/edge_cases/big_endian.tif
```

#### PDF Test Cases
```bash
# Create PDF with metadata
convert tests/fixtures/jpeg/simple/sample_with_exif.jpg tests/fixtures/pdf/simple/from_jpeg.pdf
exiftool -Title="PDF Title" -Author="PDF Author" tests/fixtures/pdf/simple/from_jpeg.pdf
```

#### MP4 Test Cases
```bash
# Generate test video with ffmpeg
ffmpeg -f lavfi -i testsrc=duration=5:size=640x480:rate=1 \
       -metadata title="Test Video" \
       -metadata artist="Test Artist" \
       tests/fixtures/mp4/simple/synthetic_video.mp4
```

## Malformed/Corrupted Test Cases

Create intentionally malformed files for robustness testing:

```bash
# Truncated JPEG (cut at 50% size)
head -c $(($(stat -f%z tests/fixtures/jpeg/simple/sample_with_exif.jpg) / 2)) \
     tests/fixtures/jpeg/simple/sample_with_exif.jpg > tests/fixtures/jpeg/malformed/truncated.jpg

# Corrupted EXIF segment (overwrite random bytes)
cp tests/fixtures/jpeg/simple/sample_with_exif.jpg tests/fixtures/jpeg/malformed/corrupted_exif.jpg
dd if=/dev/urandom of=tests/fixtures/jpeg/malformed/corrupted_exif.jpg bs=1 count=100 seek=100 conv=notrunc
```

## Directory Organization

Place acquired images in the appropriate category:

```
tests/fixtures/
├── jpeg/
│   ├── simple/          (15+ images: basic EXIF)
│   ├── complex/         (15+ images: EXIF+XMP+GPS+maker notes)
│   ├── edge_cases/      (10+ images: large, unusual, synthetic)
│   └── malformed/       (10+ images: truncated, corrupted)
├── png/
│   ├── simple/          (10+ images: text chunks)
│   ├── complex/         (10+ images: eXIf, ICC profile)
│   └── edge_cases/      (10+ images: interlaced, APNG)
├── tiff/
│   ├── simple/          (10+ images: single-page, little-endian)
│   ├── complex/         (10+ images: multi-page, big-endian)
│   └── edge_cases/      (5+ images: large, unusual bit depth)
├── pdf/
│   ├── simple/          (5+ images: Info dictionary)
│   └── complex/         (10+ images: XMP, embedded images)
└── mp4/
    ├── simple/          (5+ images: basic iTunes metadata)
    └── complex/         (10+ images: GPS track, multi-stream)
```

## Metadata Documentation

For each acquired image, update `manifest.json`:

```json
{
  "path": "jpeg/complex/canon_eos_5d.jpg",
  "format": "JPEG",
  "category": "complex",
  "source": "exiftool.org samples",
  "source_url": "https://exiftool.org/samples/Canon.jpg",
  "license": "Public Domain",
  "metadata_types": ["EXIF", "MakerNotes", "GPS"],
  "description": "Canon EOS 5D sample with maker notes and GPS",
  "expected_tags": [
    "EXIF:Make",
    "EXIF:Model",
    "GPS:GPSLatitude",
    "MakerNotes:CanonModelID"
  ],
  "known_issues": []
}
```

## Git LFS Setup

Before adding images, ensure Git LFS is tracking them:

```bash
# Initialize Git LFS (one-time setup)
git lfs install

# Verify .gitattributes is configured
cat .gitattributes | grep "tests/fixtures"

# Add and commit images (LFS will handle storage)
git add tests/fixtures/
git commit -m "feat(tests): add test corpus images from [source]"
```

## Validation Checklist

Before finalizing the test corpus:

- [ ] 100+ total images acquired
- [ ] All 5 formats represented (JPEG, PNG, TIFF, PDF, MP4)
- [ ] Each category has target count (see manifest.json)
- [ ] All images properly licensed (GPL-compatible or CC0)
- [ ] All images documented in manifest.json
- [ ] Git LFS tracking configured
- [ ] Test suite runs successfully: `cargo test --features exiftool-comparison`
- [ ] Match rate meets 98%+ threshold
- [ ] Known discrepancies documented in KNOWN_DISCREPANCIES.md

## Automation Scripts

### Bulk Download Script
```bash
#!/bin/bash
# download_test_corpus.sh
# Downloads curated test corpus from public sources

# Set up directories
mkdir -p tests/fixtures/{jpeg,png,tiff,pdf,mp4}/{simple,complex,edge_cases}

# Download Exiv2 samples
# ... (implement based on sources above)

# Validate downloads
find tests/fixtures -type f | wc -l  # Should be 100+
```

### Manifest Generator Script
```bash
#!/bin/bash
# generate_manifest.sh
# Auto-generates manifest.json entries for new fixtures

for img in tests/fixtures/**/*.jpg; do
  echo "Processing: $img"
  # Extract metadata with exiftool
  # Generate JSON entry
  # Append to manifest.json
done
```

## License Compliance

All test images MUST be:
- GPL-3.0 compatible (GPL-2.0+, LGPL, MIT, BSD, CC0, Public Domain)
- OR created synthetically by us (automatically GPL-3.0)
- Properly attributed in manifest.json

**Forbidden Licenses**:
- ❌ Proprietary/All Rights Reserved
- ❌ Non-commercial licenses (incompatible with GPL)
- ❌ ShareAlike licenses requiring different terms

## References

- [Exiv2 Test Data](https://github.com/Exiv2/exiv2/tree/main/test/data)
- [ExifTool Sample Images](https://exiftool.org/sample_images.html)
- [Unsplash License](https://unsplash.com/license)
- [ImageMagick Documentation](https://imagemagick.org/script/command-line-processing.php)
- [Git LFS Documentation](https://git-lfs.github.com/)

---

**Last Updated**: 2025-10-30
**Maintainer**: ExifTool-RS Integration Test Team
