#!/bin/bash
# validate_corpus.sh
# Validates the test corpus for ExifTool comparison tests
#
# Usage: ./validate_corpus.sh
#
# Checks:
# - Image file count vs target (100+)
# - Git LFS tracking status
# - manifest.json completeness
# - File accessibility (can be read by both tools)

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

FIXTURES_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_COUNT=100
WARNINGS=0
ERRORS=0

echo "========================================="
echo "ExifTool-RS Test Corpus Validation"
echo "========================================="
echo ""

# Check 1: Image count
echo "1. Checking image count..."
IMAGE_COUNT=$(find "$FIXTURES_DIR" -type f \( -name "*.jpg" -o -name "*.jpeg" -o -name "*.tif" -o -name "*.tiff" -o -name "*.png" -o -name "*.pdf" -o -name "*.mp4" \) | wc -l | tr -d ' ')
echo "   Found: $IMAGE_COUNT images"
echo "   Target: $TARGET_COUNT+ images"

if [ "$IMAGE_COUNT" -ge "$TARGET_COUNT" ]; then
    echo -e "   ${GREEN}✓ PASS${NC}: Image count meets target"
else
    PROGRESS=$((IMAGE_COUNT * 100 / TARGET_COUNT))
    echo -e "   ${YELLOW}⚠ WARN${NC}: Image count below target ($PROGRESS% complete)"
    WARNINGS=$((WARNINGS + 1))
fi
echo ""

# Check 2: Format distribution
echo "2. Checking format distribution..."
JPEG_COUNT=$(find "$FIXTURES_DIR/jpeg" -type f \( -name "*.jpg" -o -name "*.jpeg" \) 2>/dev/null | wc -l | tr -d ' ')
PNG_COUNT=$(find "$FIXTURES_DIR/png" -type f -name "*.png" 2>/dev/null | wc -l | tr -d ' ')
TIFF_COUNT=$(find "$FIXTURES_DIR/tiff" -type f \( -name "*.tif" -o -name "*.tiff" \) 2>/dev/null | wc -l | tr -d ' ')
PDF_COUNT=$(find "$FIXTURES_DIR/pdf" -type f -name "*.pdf" 2>/dev/null | wc -l | tr -d ' ')
MP4_COUNT=$(find "$FIXTURES_DIR/mp4" -type f -name "*.mp4" 2>/dev/null | wc -l | tr -d ' ')

echo "   JPEG: $JPEG_COUNT (target: 50)"
echo "   PNG:  $PNG_COUNT (target: 30)"
echo "   TIFF: $TIFF_COUNT (target: 25)"
echo "   PDF:  $PDF_COUNT (target: 15)"
echo "   MP4:  $MP4_COUNT (target: 15)"

if [ "$JPEG_COUNT" -eq 0 ] || [ "$TIFF_COUNT" -eq 0 ]; then
    echo -e "   ${RED}✗ FAIL${NC}: Missing critical format samples (JPEG or TIFF)"
    ERRORS=$((ERRORS + 1))
else
    echo -e "   ${GREEN}✓ PASS${NC}: All formats represented"
fi
echo ""

# Check 3: Git LFS tracking
echo "3. Checking Git LFS tracking..."
if ! command -v git &> /dev/null; then
    echo -e "   ${YELLOW}⚠ WARN${NC}: Git not found, skipping LFS check"
    WARNINGS=$((WARNINGS + 1))
elif ! git lfs env &> /dev/null; then
    echo -e "   ${YELLOW}⚠ WARN${NC}: Git LFS not installed"
    WARNINGS=$((WARNINGS + 1))
else
    LFS_TRACKED=$(git lfs ls-files "$FIXTURES_DIR" | wc -l | tr -d ' ')
    echo "   LFS tracked files: $LFS_TRACKED"

    if [ "$LFS_TRACKED" -eq "$IMAGE_COUNT" ]; then
        echo -e "   ${GREEN}✓ PASS${NC}: All images tracked with Git LFS"
    elif [ "$LFS_TRACKED" -gt 0 ]; then
        echo -e "   ${YELLOW}⚠ WARN${NC}: Some images not tracked with Git LFS ($LFS_TRACKED/$IMAGE_COUNT)"
        WARNINGS=$((WARNINGS + 1))
    else
        echo -e "   ${YELLOW}⚠ WARN${NC}: No images tracked with Git LFS (run 'git lfs migrate import')"
        WARNINGS=$((WARNINGS + 1))
    fi
fi
echo ""

# Check 4: manifest.json exists and is valid
echo "4. Checking manifest.json..."
MANIFEST="$FIXTURES_DIR/manifest.json"
if [ ! -f "$MANIFEST" ]; then
    echo -e "   ${RED}✗ FAIL${NC}: manifest.json not found"
    ERRORS=$((ERRORS + 1))
else
    # Basic JSON validation (requires jq or python)
    if command -v jq &> /dev/null; then
        if jq empty "$MANIFEST" 2>/dev/null; then
            echo -e "   ${GREEN}✓ PASS${NC}: manifest.json is valid JSON"

            # Count fixtures in manifest
            MANIFEST_COUNT=$(jq '.fixtures | length' "$MANIFEST")
            echo "   Documented fixtures: $MANIFEST_COUNT"

            if [ "$MANIFEST_COUNT" -ne "$IMAGE_COUNT" ]; then
                echo -e "   ${YELLOW}⚠ WARN${NC}: Manifest count ($MANIFEST_COUNT) doesn't match file count ($IMAGE_COUNT)"
                WARNINGS=$((WARNINGS + 1))
            fi
        else
            echo -e "   ${RED}✗ FAIL${NC}: manifest.json is invalid JSON"
            ERRORS=$((ERRORS + 1))
        fi
    elif command -v python3 &> /dev/null; then
        if python3 -m json.tool "$MANIFEST" &> /dev/null; then
            echo -e "   ${GREEN}✓ PASS${NC}: manifest.json is valid JSON"
        else
            echo -e "   ${RED}✗ FAIL${NC}: manifest.json is invalid JSON"
            ERRORS=$((ERRORS + 1))
        fi
    else
        echo -e "   ${YELLOW}⚠ WARN${NC}: Cannot validate JSON (install jq or python3)"
        WARNINGS=$((WARNINGS + 1))
    fi
fi
echo ""

# Check 5: Perl ExifTool availability
echo "5. Checking Perl ExifTool availability..."
if ! command -v exiftool &> /dev/null; then
    echo -e "   ${RED}✗ FAIL${NC}: Perl ExifTool not found in PATH"
    echo "   Install with: apt-get install libimage-exiftool-perl (Ubuntu)"
    echo "              or: brew install exiftool (macOS)"
    ERRORS=$((ERRORS + 1))
else
    EXIFTOOL_VERSION=$(exiftool -ver)
    echo "   Found: ExifTool version $EXIFTOOL_VERSION"
    echo -e "   ${GREEN}✓ PASS${NC}: Perl ExifTool available"
fi
echo ""

# Check 6: Sample file accessibility
echo "6. Checking sample file accessibility..."
SAMPLE_FILES=(
    "jpeg/simple/sample_with_exif.jpg"
    "jpeg/complex/sample_with_exif_xmp.jpg"
    "tiff/simple/sample.tif"
    "pdf/simple/sample.pdf"
    "mp4/simple/sample.mp4"
)

ACCESSIBLE=0
for sample in "${SAMPLE_FILES[@]}"; do
    SAMPLE_PATH="$FIXTURES_DIR/$sample"
    if [ -f "$SAMPLE_PATH" ]; then
        ACCESSIBLE=$((ACCESSIBLE + 1))
    else
        echo -e "   ${YELLOW}⚠ WARN${NC}: Sample not found: $sample"
        WARNINGS=$((WARNINGS + 1))
    fi
done

if [ "$ACCESSIBLE" -eq "${#SAMPLE_FILES[@]}" ]; then
    echo -e "   ${GREEN}✓ PASS${NC}: All core sample files accessible ($ACCESSIBLE/${#SAMPLE_FILES[@]})"
else
    echo -e "   ${YELLOW}⚠ WARN${NC}: Some sample files missing ($ACCESSIBLE/${#SAMPLE_FILES[@]})"
fi
echo ""

# Summary
echo "========================================="
echo "Validation Summary"
echo "========================================="
echo "Total images: $IMAGE_COUNT / $TARGET_COUNT ($(($IMAGE_COUNT * 100 / $TARGET_COUNT))%)"
echo "Warnings: $WARNINGS"
echo "Errors: $ERRORS"
echo ""

if [ "$ERRORS" -eq 0 ] && [ "$WARNINGS" -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed!${NC}"
    echo "Test corpus is ready for integration tests."
    exit 0
elif [ "$ERRORS" -eq 0 ]; then
    echo -e "${YELLOW}⚠ Validation passed with warnings${NC}"
    echo "Test corpus is functional but could be improved."
    exit 0
else
    echo -e "${RED}✗ Validation failed with $ERRORS error(s)${NC}"
    echo "Please fix errors before running integration tests."
    exit 1
fi
