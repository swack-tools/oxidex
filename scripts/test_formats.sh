#!/bin/bash
# Test script for RW2, RAF, and ORF formats

echo "Testing all RW2, RAF, and ORF files..."
echo "======================================="

success=0
errors=0

# Test all RW2 files (case-insensitive)
while IFS= read -r file; do
    if ./target/release/exiftool-rs "$file" >/dev/null 2>&1; then
        success=$((success + 1))
    else
        errors=$((errors + 1))
        echo "ERROR: $file"
    fi
done < <(find /Users/allen/Documents/git/examples/data.lfs -iname "*.rw2" -type f 2>/dev/null)

echo ""
echo "RW2 Files - Success: $success, Errors: $errors"
success=0
errors=0

# Test all ORF files (case-insensitive)
while IFS= read -r file; do
    if ./target/release/exiftool-rs "$file" >/dev/null 2>&1; then
        success=$((success + 1))
    else
        errors=$((errors + 1))
        echo "ERROR: $file"
    fi
done < <(find /Users/allen/Documents/git/examples/data.lfs -iname "*.orf" -type f 2>/dev/null)

echo "ORF Files - Success: $success, Errors: $errors"
success=0
errors=0

# Test all RAF files (case-insensitive)
while IFS= read -r file; do
    if ./target/release/exiftool-rs "$file" >/dev/null 2>&1; then
        success=$((success + 1))
    else
        errors=$((errors + 1))
        echo "ERROR: $file"
    fi
done < <(find /Users/allen/Documents/git/examples/data.lfs -iname "*.raf" -type f 2>/dev/null)

echo "RAF Files - Success: $success, Errors: $errors"
echo ""
echo "======================================="
echo "Testing complete!"
