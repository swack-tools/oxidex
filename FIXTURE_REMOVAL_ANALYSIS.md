# Test Fixture Removal Analysis

## Summary

**Total test fixtures**: 108 files (216 MB)
**Explicitly used in tests**: 20 files
**Used by batch benchmarks**: All files in `jpeg/simple/` directory
**Safe to remove**: 69 files (~38.9 MB)
**Recommended to replace**: 2 large files (save 159 MB)

**Total potential savings**: ~198 MB (92% reduction, down to 18 MB)

---

## Usage Analysis

### Files Directly Referenced in Code (MUST KEEP)
```
tests/fixtures/jpeg/complex/sample_with_exif_xmp.jpg
tests/fixtures/jpeg/complex/synthetic_gps_001.jpg
tests/fixtures/jpeg/complex/synthetic_gps_002.jpg
tests/fixtures/jpeg/complex/synthetic_gps_003.jpg
tests/fixtures/jpeg/edge_cases/large_dimension.jpg
tests/fixtures/jpeg/simple/sample_with_exif.jpg
tests/fixtures/jpeg/simple/synthetic_001.jpg
tests/fixtures/mp4/simple/sample.mp4
tests/fixtures/pdf/simple/sample.pdf
tests/fixtures/png/complex/synthetic_exif_001.png
tests/fixtures/png/simple/synthetic_text_001.png
tests/fixtures/tiff/complex/big_endian_001.tif
tests/fixtures/tiff/complex/multipage.tif
tests/fixtures/tiff/simple/sample.tif
```

### Dynamically Loaded (KEEP ALL)
- **`tests/fixtures/jpeg/simple/*.jpg`** - Used by `benches/integration_benchmarks.rs::bench_batch_processing()`
  - Loads all JPEG files in directory for batch processing benchmarks
  - Keep all 15 JPEG files (including synthetic_001-015.jpg)

---

## Safe to Remove

### 1. Duplicate Files (~3 KB)
These are exact duplicates of files in subdirectories:

```bash
# SAFE TO REMOVE
rm tests/fixtures/jpeg/sample_with_exif.jpg          # duplicate of simple/
rm tests/fixtures/jpeg/sample_with_exif_xmp.jpg      # duplicate of complex/
rm tests/fixtures/tiff/sample.tif                    # duplicate of simple/
rm tests/fixtures/mp4/sample.mp4                     # duplicate of simple/
rm tests/fixtures/pdf/sample.pdf                     # duplicate of simple/
rm tests/fixtures/png/sample.png                     # duplicate of simple/
```

### 2. Unused JPEG Complex (~190 KB)
Only synthetic_gps_001-003 are used. Remove 004-010:

```bash
# SAFE TO REMOVE (7 files × 27 KB = 190 KB)
rm tests/fixtures/jpeg/complex/synthetic_gps_004.jpg
rm tests/fixtures/jpeg/complex/synthetic_gps_005.jpg
rm tests/fixtures/jpeg/complex/synthetic_gps_006.jpg
rm tests/fixtures/jpeg/complex/synthetic_gps_007.jpg
rm tests/fixtures/jpeg/complex/synthetic_gps_008.jpg
rm tests/fixtures/jpeg/complex/synthetic_gps_009.jpg
rm tests/fixtures/jpeg/complex/synthetic_gps_010.jpg
```

### 3. Unused PNG Complex (~38 KB)
Only synthetic_exif_001 is used. Remove 002-012:

```bash
# SAFE TO REMOVE (11 files × 3.4 KB = 38 KB)
rm tests/fixtures/png/complex/synthetic_exif_002.png
rm tests/fixtures/png/complex/synthetic_exif_003.png
rm tests/fixtures/png/complex/synthetic_exif_004.png
rm tests/fixtures/png/complex/synthetic_exif_005.png
rm tests/fixtures/png/complex/synthetic_exif_006.png
rm tests/fixtures/png/complex/synthetic_exif_007.png
rm tests/fixtures/png/complex/synthetic_exif_008.png
rm tests/fixtures/png/complex/synthetic_exif_009.png
rm tests/fixtures/png/complex/synthetic_exif_010.png
rm tests/fixtures/png/complex/synthetic_exif_011.png
rm tests/fixtures/png/complex/synthetic_exif_012.png
```

### 4. Unused PNG Simple (~6 KB)
Only synthetic_text_001 is used. Remove 002-015:

```bash
# SAFE TO REMOVE (14 files × 465 B = 6 KB)
rm tests/fixtures/png/simple/synthetic_text_002.png
rm tests/fixtures/png/simple/synthetic_text_003.png
rm tests/fixtures/png/simple/synthetic_text_004.png
rm tests/fixtures/png/simple/synthetic_text_005.png
rm tests/fixtures/png/simple/synthetic_text_006.png
rm tests/fixtures/png/simple/synthetic_text_007.png
rm tests/fixtures/png/simple/synthetic_text_008.png
rm tests/fixtures/png/simple/synthetic_text_009.png
rm tests/fixtures/png/simple/synthetic_text_010.png
rm tests/fixtures/png/simple/synthetic_text_011.png
rm tests/fixtures/png/simple/synthetic_text_012.png
rm tests/fixtures/png/simple/synthetic_text_013.png
rm tests/fixtures/png/simple/synthetic_text_014.png
rm tests/fixtures/png/simple/synthetic_text_015.png
```

### 5. Unused PNG Edge Cases (~2 KB)
None of these interlaced files are referenced:

```bash
# SAFE TO REMOVE (5 files × 396 B = 2 KB)
rm tests/fixtures/png/edge_cases/interlaced_001.png
rm tests/fixtures/png/edge_cases/interlaced_002.png
rm tests/fixtures/png/edge_cases/interlaced_003.png
rm tests/fixtures/png/edge_cases/interlaced_004.png
rm tests/fixtures/png/edge_cases/interlaced_005.png
```

### 6. Unused TIFF Simple (~27.5 MB) ⚠️
**WARNING**: None directly referenced, but might be needed for comprehensive testing

```bash
# CONSIDER REMOVING (10 files × 2.7 MB = 27.5 MB)
rm tests/fixtures/tiff/simple/synthetic_001.tif
rm tests/fixtures/tiff/simple/synthetic_002.tif
rm tests/fixtures/tiff/simple/synthetic_003.tif
rm tests/fixtures/tiff/simple/synthetic_004.tif
rm tests/fixtures/tiff/simple/synthetic_005.tif
rm tests/fixtures/tiff/simple/synthetic_006.tif
rm tests/fixtures/tiff/simple/synthetic_007.tif
rm tests/fixtures/tiff/simple/synthetic_008.tif
rm tests/fixtures/tiff/simple/synthetic_009.tif
rm tests/fixtures/tiff/simple/synthetic_010.tif
```

### 7. Unused TIFF Complex (~11 MB)
Only big_endian_001 is used. Remove 002-005:

```bash
# SAFE TO REMOVE (4 files × 2.7 MB = 11 MB)
rm tests/fixtures/tiff/complex/big_endian_002.tif
rm tests/fixtures/tiff/complex/big_endian_003.tif
rm tests/fixtures/tiff/complex/big_endian_004.tif
rm tests/fixtures/tiff/complex/big_endian_005.tif
```

### 8. Unused PDF Files (~180 KB)
Only simple/sample.pdf is used. Remove all synthetic PDFs:

```bash
# SAFE TO REMOVE (9 files = 180 KB)
rm tests/fixtures/pdf/simple/synthetic_001.pdf
rm tests/fixtures/pdf/simple/synthetic_002.pdf
rm tests/fixtures/pdf/simple/synthetic_003.pdf
rm tests/fixtures/pdf/simple/synthetic_004.pdf
rm tests/fixtures/pdf/simple/synthetic_005.pdf
rm tests/fixtures/pdf/complex/synthetic_xmp_001.pdf
rm tests/fixtures/pdf/complex/synthetic_xmp_002.pdf
rm tests/fixtures/pdf/complex/synthetic_xmp_003.pdf
rm tests/fixtures/pdf/complex/synthetic_xmp_004.pdf
```

### 9. Unused MP4 Files (~81 KB)
Only simple/sample.mp4 is used. Remove all synthetic MP4s:

```bash
# SAFE TO REMOVE (8 files = 81 KB)
rm tests/fixtures/mp4/simple/synthetic_001.mp4
rm tests/fixtures/mp4/simple/synthetic_002.mp4
rm tests/fixtures/mp4/simple/synthetic_003.mp4
rm tests/fixtures/mp4/simple/synthetic_004.mp4
rm tests/fixtures/mp4/simple/synthetic_005.mp4
rm tests/fixtures/mp4/complex/synthetic_gps_001.mp4
rm tests/fixtures/mp4/complex/synthetic_gps_002.mp4
rm tests/fixtures/mp4/complex/synthetic_gps_003.mp4
```

---

## Recommended to Replace (Not Remove)

### Large Edge Case Files (~159 MB savings)

These files test edge cases and should be kept, but replaced with smaller versions:

#### 1. very_large.tif (137 MB → 5 MB) - Save 132 MB
```bash
# Current: 137 MB
# Replace with: 5 MB version that still tests large file handling
# Used by: benches/integration_benchmarks.rs::bench_large_file_handling()
```

#### 2. large_plasma.png (30 MB → 3 MB) - Save 27 MB
```bash
# Current: 30 MB
# Replace with: 3 MB version that still tests large PNG files
# Not currently referenced, but useful for edge case testing
```

---

## Space Savings Summary

### Conservative Removal (Skip TIFF Simple)
```
Duplicates:           3 KB
JPEG complex:       190 KB
PNG complex:         38 KB
PNG simple:           6 KB
PNG edge cases:       2 KB
TIFF complex:        11 MB
PDF:                180 KB
MP4:                 81 KB
----------------------------
Subtotal:          ~11.5 MB

With large file replacements:
very_large.tif:    132 MB
large_plasma.png:   27 MB
----------------------------
Total:            ~170 MB (79% reduction)
```

### Aggressive Removal (Include TIFF Simple)
```
Conservative:      11.5 MB
TIFF simple:       27.5 MB
----------------------------
Subtotal:          ~39 MB

With large file replacements:
very_large.tif:    132 MB
large_plasma.png:   27 MB
----------------------------
Total:            ~198 MB (92% reduction)
Final size:        ~18 MB
```

---

## Important Notes

1. **JPEG Simple Directory**: Keep ALL files (synthetic_001-015.jpg) because they're loaded dynamically by batch benchmarks

2. **TIFF Simple Synthetic Files**: Consider keeping 3-5 for comprehensive testing even though not directly referenced

3. **Edge Case Files**: Keep compressed TIFF files (lzw_compressed.tif, zip_compressed.tif) as they test important compression formats

4. **Test Coverage**: After removal, verify tests still pass:
   ```bash
   cargo test --all-features
   cargo bench --no-run
   ```

---

## Execution Commands

### Generate removal script:
```bash
# Conservative removal (skip TIFF simple)
cat > /tmp/remove_fixtures.sh << 'EOF'
#!/bin/bash
set -e

# Duplicates
rm tests/fixtures/jpeg/sample_with_exif.jpg
rm tests/fixtures/jpeg/sample_with_exif_xmp.jpg
rm tests/fixtures/tiff/sample.tif
rm tests/fixtures/mp4/sample.mp4
rm tests/fixtures/pdf/sample.pdf
rm tests/fixtures/png/sample.png

# JPEG complex 004-010
rm tests/fixtures/jpeg/complex/synthetic_gps_{004..010}.jpg

# PNG complex 002-012
rm tests/fixtures/png/complex/synthetic_exif_{002..012}.png

# PNG simple 002-015
rm tests/fixtures/png/simple/synthetic_text_{002..015}.png

# PNG edge cases
rm tests/fixtures/png/edge_cases/interlaced_*.png

# TIFF complex 002-005
rm tests/fixtures/tiff/complex/big_endian_{002..005}.tif

# PDF all synthetic
rm tests/fixtures/pdf/simple/synthetic_*.pdf
rm tests/fixtures/pdf/complex/synthetic_*.pdf

# MP4 all synthetic
rm tests/fixtures/mp4/simple/synthetic_*.mp4
rm tests/fixtures/mp4/complex/synthetic_*.mp4

echo "Removed unused test fixtures"
echo "Run: git status"
EOF

chmod +x /tmp/remove_fixtures.sh
```

### Run tests after removal:
```bash
./tmp/remove_fixtures.sh
cargo test --all-features
cargo bench --no-run
```
