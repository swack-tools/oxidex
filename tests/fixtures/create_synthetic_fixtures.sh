#!/bin/bash
# create_synthetic_fixtures.sh
# Generates synthetic test images with known metadata for ExifTool-RS integration testing
# License: GPL-3.0

set -euo pipefail

echo "Creating synthetic test fixtures..."

# Define base directory
FIXTURES_DIR="$(cd "$(dirname "$0")" && pwd)"

# Create directories if they don't exist
mkdir -p "$FIXTURES_DIR/jpeg/simple"
mkdir -p "$FIXTURES_DIR/jpeg/complex"
mkdir -p "$FIXTURES_DIR/jpeg/edge_cases"
mkdir -p "$FIXTURES_DIR/png/simple"
mkdir -p "$FIXTURES_DIR/png/complex"
mkdir -p "$FIXTURES_DIR/png/edge_cases"
mkdir -p "$FIXTURES_DIR/tiff/simple"
mkdir -p "$FIXTURES_DIR/tiff/complex"
mkdir -p "$FIXTURES_DIR/tiff/edge_cases"
mkdir -p "$FIXTURES_DIR/pdf/simple"
mkdir -p "$FIXTURES_DIR/pdf/complex"
mkdir -p "$FIXTURES_DIR/mp4/simple"
mkdir -p "$FIXTURES_DIR/mp4/complex"

# Counter for tracking
jpeg_count=0
png_count=0
tiff_count=0
pdf_count=0
mp4_count=0

echo "=== Generating JPEG Simple Images (10 images) ==="
for i in {1..10}; do
  file="$FIXTURES_DIR/jpeg/simple/synthetic_$(printf %03d $i).jpg"
  convert -size 800x600 xc:blue "$file"
  exiftool -Artist="Synthetic Artist $i" \
           -Make="Synthetic Camera Co" \
           -Model="TestCam $i" \
           -DateTimeOriginal="2024:01:$(printf %02d $i) 12:00:00" \
           -overwrite_original \
           "$file"
  ((jpeg_count++))
  echo "Created: $file"
done

echo "=== Generating JPEG Complex Images with GPS (10 images) ==="
for i in {1..10}; do
  file="$FIXTURES_DIR/jpeg/complex/synthetic_gps_$(printf %03d $i).jpg"
  convert -size 1024x768 gradient:blue-red "$file"

  # Calculate GPS coordinates with high precision
  lat=$(echo "37.7749 + $i * 0.001" | bc -l)
  lon=$(echo "-122.4194 + $i * 0.001" | bc -l)

  exiftool -Artist="GPS Test Artist $i" \
           -Make="GPS Test Camera" \
           -Model="GPS TestCam $i" \
           -DateTimeOriginal="2024:02:$(printf %02d $i) 14:30:00" \
           -GPSLatitude="$lat" \
           -GPSLongitude="$lon" \
           -GPSAltitude="$(( 100 + i * 10 ))" \
           -overwrite_original \
           "$file"
  ((jpeg_count++))
  echo "Created: $file with GPS ($lat, $lon)"
done

echo "=== Generating JPEG Edge Cases (8 images) ==="
# Large dimensions
convert -size 4000x3000 xc:white "$FIXTURES_DIR/jpeg/edge_cases/large_dimension.jpg"
exiftool -Artist="Large Image" -Make="Edge Case" -overwrite_original "$FIXTURES_DIR/jpeg/edge_cases/large_dimension.jpg"
((jpeg_count++))

# All 8 EXIF orientations
for orientation in {1..8}; do
  file="$FIXTURES_DIR/jpeg/edge_cases/orientation_$orientation.jpg"
  magick -size 640x480 -gravity center -pointsize 72 -fill black -annotate +0+0 "Orient $orientation" xc:white "$file"
  exiftool -Orientation#="$orientation" -overwrite_original "$file" 2>/dev/null || true
  ((jpeg_count++))
  echo "Created: $file (Orientation=$orientation)"
done

echo "=== Generating PNG Simple Images with Text Chunks (10 images) ==="
for i in {1..10}; do
  file="$FIXTURES_DIR/png/simple/synthetic_text_$(printf %03d $i).png"
  convert -size 640x480 xc:green "$file"
  exiftool -Title="PNG Title $i" \
           -Author="PNG Author $i" \
           -Description="PNG test image with text chunks number $i" \
           -overwrite_original \
           "$file"
  ((png_count++))
  echo "Created: $file"
done

echo "=== Generating PNG Complex Images with eXIf Chunk (10 images) ==="
# First create JPEG with EXIF, then convert to PNG to get eXIf chunk
for i in {1..10}; do
  temp_jpg="/tmp/temp_exif_$i.jpg"
  file="$FIXTURES_DIR/png/complex/synthetic_exif_$(printf %03d $i).png"

  # Create JPEG with EXIF
  convert -size 800x600 gradient:green-yellow "$temp_jpg"
  exiftool -Make="PNG EXIF Test" \
           -Model="PNG TestCam $i" \
           -Artist="PNG EXIF Artist $i" \
           -DateTimeOriginal="2024:03:$(printf %02d $i) 10:00:00" \
           -overwrite_original \
           "$temp_jpg"

  # Convert to PNG (ImageMagick 7+ preserves EXIF as eXIf chunk)
  convert "$temp_jpg" "$file"
  rm "$temp_jpg"
  ((png_count++))
  echo "Created: $file with eXIf chunk"
done

echo "=== Generating PNG Edge Cases (5 images) ==="
# Large PNG
convert -size 3000x2000 plasma:blue-pink "$FIXTURES_DIR/png/edge_cases/large_plasma.png"
exiftool -Title="Large Plasma" -overwrite_original "$FIXTURES_DIR/png/edge_cases/large_plasma.png"
((png_count++))

# Interlaced PNG
for i in {1..4}; do
  file="$FIXTURES_DIR/png/edge_cases/interlaced_$(printf %03d $i).png"
  convert -size 640x480 -interlace PNG xc:blue "$file"
  exiftool -Title="Interlaced PNG $i" -overwrite_original "$file"
  ((png_count++))
  echo "Created: $file (interlaced)"
done

echo "=== Generating TIFF Simple Images (8 images) ==="
for i in {1..8}; do
  file="$FIXTURES_DIR/tiff/simple/synthetic_$(printf %03d $i).tif"
  convert -size 800x600 xc:yellow "$file"
  exiftool -Artist="TIFF Artist $i" \
           -Make="TIFF Camera" \
           -Model="TIFF Cam $i" \
           -DateTimeOriginal="2024:04:$(printf %02d $i) 09:00:00" \
           -overwrite_original \
           "$file"
  ((tiff_count++))
  echo "Created: $file"
done

echo "=== Generating TIFF Complex Images (5 images) ==="
# Multi-page TIFF
echo "Creating multi-page TIFF..."
temp_files=()
for i in {1..3}; do
  temp_file="/tmp/tiff_page_$i.tif"
  convert -size 640x480 -gravity center -pointsize 72 -annotate +0+0 "Page $i" xc:cyan "$temp_file"
  temp_files+=("$temp_file")
done
convert "${temp_files[@]}" "$FIXTURES_DIR/tiff/complex/multipage.tif"
exiftool -Artist="Multipage TIFF" -overwrite_original "$FIXTURES_DIR/tiff/complex/multipage.tif"
rm "${temp_files[@]}"
((tiff_count++))

# Big-endian TIFF (4 images)
for i in {1..4}; do
  file="$FIXTURES_DIR/tiff/complex/big_endian_$(printf %03d $i).tif"
  convert -size 800x600 -endian MSB xc:magenta "$file"
  exiftool -Artist="Big Endian $i" -Make="TIFF BE" -overwrite_original "$file"
  ((tiff_count++))
  echo "Created: $file (big-endian)"
done

echo "=== Generating TIFF Edge Cases (3 images) ==="
# Very large TIFF
convert -size 6000x4000 plasma:red-blue "$FIXTURES_DIR/tiff/edge_cases/very_large.tif"
exiftool -Artist="Very Large TIFF" -overwrite_original "$FIXTURES_DIR/tiff/edge_cases/very_large.tif"
((tiff_count++))

# Different compressions
convert -size 640x480 -compress LZW xc:orange "$FIXTURES_DIR/tiff/edge_cases/lzw_compressed.tif"
exiftool -Artist="LZW Compressed" -overwrite_original "$FIXTURES_DIR/tiff/edge_cases/lzw_compressed.tif"
((tiff_count++))

convert -size 640x480 -compress Zip xc:purple "$FIXTURES_DIR/tiff/edge_cases/zip_compressed.tif"
exiftool -Artist="ZIP Compressed" -overwrite_original "$FIXTURES_DIR/tiff/edge_cases/zip_compressed.tif"
((tiff_count++))

echo "=== Generating PDF Simple Images (4 images) ==="
for i in {1..4}; do
  temp_jpg="/tmp/pdf_source_$i.jpg"
  file="$FIXTURES_DIR/pdf/simple/synthetic_$(printf %03d $i).pdf"

  # Create source image
  convert -size 800x600 -gravity center -pointsize 48 -annotate +0+0 "PDF $i" xc:lightblue "$temp_jpg"

  # Convert to PDF
  convert "$temp_jpg" "$file"

  # Add PDF metadata
  exiftool -Title="PDF Title $i" \
           -Author="PDF Author $i" \
           -Creator="Synthetic PDF Generator" \
           -Subject="Test PDF $i" \
           -overwrite_original \
           "$file"

  rm "$temp_jpg"
  ((pdf_count++))
  echo "Created: $file"
done

echo "=== Generating PDF Complex Images with XMP (3 images) ==="
for i in {1..3}; do
  temp_jpg="/tmp/pdf_xmp_source_$i.jpg"
  file="$FIXTURES_DIR/pdf/complex/synthetic_xmp_$(printf %03d $i).pdf"

  # Create source with XMP
  convert -size 1024x768 gradient:red-green "$temp_jpg"
  exiftool -Title="XMP PDF $i" \
           -Artist="XMP Artist $i" \
           -XMP:Creator="XMP Creator $i" \
           -XMP:Rights="CC0 Public Domain" \
           -overwrite_original \
           "$temp_jpg"

  # Convert to PDF
  convert "$temp_jpg" "$file"

  rm "$temp_jpg"
  ((pdf_count++))
  echo "Created: $file with XMP"
done

echo "=== Generating MP4 Simple Videos (4 files) ==="
for i in {1..4}; do
  file="$FIXTURES_DIR/mp4/simple/synthetic_$(printf %03d $i).mp4"

  # Generate 3-second test video
  ffmpeg -f lavfi -i testsrc=duration=3:size=640x480:rate=1 \
         -metadata title="Test Video $i" \
         -metadata artist="Video Artist $i" \
         -metadata date="2024-05-$(printf %02d $i)" \
         -y \
         "$file" 2>/dev/null

  ((mp4_count++))
  echo "Created: $file"
done

echo "=== Generating MP4 Complex Videos with GPS (3 files) ==="
for i in {1..3}; do
  file="$FIXTURES_DIR/mp4/complex/synthetic_gps_$(printf %03d $i).mp4"

  # Calculate GPS coordinates
  lat=$(echo "34.0522 + $i * 0.01" | bc -l)
  lon=$(echo "-118.2437 + $i * 0.01" | bc -l)

  # Generate video with GPS metadata
  ffmpeg -f lavfi -i testsrc=duration=3:size=800x600:rate=1 \
         -metadata title="GPS Video $i" \
         -metadata location="$lat,$lon" \
         -metadata com.apple.quicktime.location.ISO6709="+${lat}-${lon}/" \
         -y \
         "$file" 2>/dev/null

  ((mp4_count++))
  echo "Created: $file with GPS ($lat, $lon)"
done

echo ""
echo "=== Summary ==="
echo "JPEG images created: $jpeg_count"
echo "PNG images created: $png_count"
echo "TIFF images created: $tiff_count"
echo "PDF files created: $pdf_count"
echo "MP4 files created: $mp4_count"
echo "Total synthetic fixtures: $((jpeg_count + png_count + tiff_count + pdf_count + mp4_count))"
echo ""
echo "All synthetic fixtures created successfully!"
echo "License: All generated images are GPL-3.0 (synthetic/automatically generated)"
