# Troubleshooting

This chapter covers common issues, error messages, performance tips, and debugging strategies for OxiDex.

## Common Errors

### "Error: File not found"

**Cause**: The specified file path doesn't exist or is inaccessible.

**Solutions:**

1. **Verify the path**:
   ```bash
   ls photo.jpg  # Check file exists
   oxidex photo.jpg
   ```

2. **Use absolute paths** if relative paths aren't working:
   ```bash
   oxidex /full/path/to/photo.jpg
   ```

3. **Check file permissions**:
   ```bash
   # Linux/macOS
   ls -l photo.jpg
   chmod 644 photo.jpg  # Make readable
   ```

### "Error: Unsupported file format"

**Cause**: The file format is not yet implemented in OxiDex.

**Solutions:**

1. **Check supported formats** in the [Supported Formats](/reference/formats/) section.

2. **Use the original ExifTool** for unsupported formats:
   ```bash
   exiftool photo.raw  # Use Perl ExifTool for RAW files
   ```

3. **Convert to supported format** (if appropriate):
   ```bash
   # Example: Convert HEIC to JPEG
   sips -s format jpeg photo.heic --out photo.jpg
   oxidex photo.jpg
   ```

4. **Request format support**: [Open a GitHub issue](https://github.com/swack-tools/oxidex/issues) to request the format.

### "Error: File is read-only"

**Cause**: The file has read-only permissions and you're trying to write metadata.

**Solutions:**

1. **Make file writable**:
   ```bash
   # Linux/macOS
   chmod u+w photo.jpg
   oxidex -EXIF:Artist="John Doe" photo.jpg
   ```

   ```powershell
   # Windows PowerShell
   attrib -r photo.jpg
   oxidex -EXIF:Artist="John Doe" photo.jpg
   ```

2. **Use `--readonly` flag** if you only want to read metadata:
   ```bash
   oxidex --readonly photo.jpg
   ```

3. **Work on a copy**:
   ```bash
   cp photo.jpg photo_copy.jpg
   oxidex -EXIF:Artist="John Doe" photo_copy.jpg
   ```

### "Error: Invalid value for TAG"

**Cause**: The value provided doesn't match the expected data type for the tag.

**Solutions:**

1. **Check tag data type**:
   - `EXIF:ISO`: Integer (e.g., `400`, not `"400"`)
   - `EXIF:FNumber`: Float (e.g., `5.6`)
   - `EXIF:Make`: String (e.g., `"Canon"`)
   - `EXIF:DateTime`: Datetime string (e.g., `"2025:01:15 14:30:00"`)

2. **Use correct format**:
   ```bash
   # Wrong
   oxidex -EXIF:ISO="four hundred" photo.jpg  # Error

   # Correct
   oxidex -EXIF:ISO=400 photo.jpg  # Success
   ```

3. **Check date/time format**:
   ```bash
   # Wrong
   oxidex -EXIF:DateTime="2025-01-15 14:30" photo.jpg

   # Correct (EXIF uses ":" separator)
   oxidex -EXIF:DateTime="2025:01:15 14:30:00" photo.jpg
   ```

### "Error: Cannot modify file in read-only mode"

**Cause**: The `--readonly` flag is set but you're attempting a write operation.

**Solution:**

Remove the `--readonly` flag:

```bash
# Wrong
oxidex --readonly -EXIF:Artist="John" photo.jpg  # Error

# Correct
oxidex -EXIF:Artist="John" photo.jpg  # Success
```

### "Error: Failed to create backup file"

**Cause**: Insufficient permissions or disk space when using `--backup` flag.

**Solutions:**

1. **Check disk space**:
   ```bash
   df -h .  # Check available space
   ```

2. **Check write permissions** in the directory:
   ```bash
   ls -ld .
   chmod u+w .  # Make directory writable
   ```

3. **Don't use `--backup`** if backups aren't needed:
   ```bash
   oxidex -EXIF:Artist="John" photo.jpg  # No backup
   ```

### "Error: Parse error at offset X"

**Cause**: The file is corrupted or malformed.

**Solutions:**

1. **Verify file integrity**:
   ```bash
   file photo.jpg  # Check if file is valid JPEG
   ```

2. **Try opening in image viewer** to confirm the file isn't corrupted.

3. **Use original ExifTool** which may have more robust parsing:
   ```bash
   exiftool photo.jpg  # Perl ExifTool
   ```

4. **Report the issue** with the problematic file (if possible):
   - Create a GitHub issue with the file (if not confidential)
   - This helps improve parser robustness

### "warning: Tag generation failed"

**Cause**: Build script couldn't download ExifTool source (network issue).

**Impact**: Build continues with fallback tag registry (reduced tag coverage).

**Solutions:**

1. **Check internet connection** and retry:
   ```bash
   cargo clean
   cargo build --release
   ```

2. **Use proxy** if behind a firewall:
   ```bash
   export HTTP_PROXY=http://proxy:port
   export HTTPS_PROXY=http://proxy:port
   cargo build --release
   ```

3. **Accept fallback**: The build will still succeed with core tags available.

## Performance Issues

### Slow Processing

**Symptoms**: Metadata operations take much longer than expected.

**Solutions:**

#### 1. Use Release Builds

Debug builds are 2-5x slower than release builds:

```bash
# Slow (debug build)
cargo build
./target/debug/oxidex photo.jpg

# Fast (release build)
cargo build --release
./target/release/oxidex photo.jpg
```

**Release Build Optimizations:**
- Link-time optimization (LTO)
- Codegen units = 1
- Optimization level 3
- Dead code elimination

#### 2. Use Batch Processing

Process multiple files in parallel:

```bash
# Slow (sequential)
for file in *.jpg; do
  oxidex "$file"
done

# Fast (parallel with -r flag)
oxidex -r .
```

OxiDex automatically uses all CPU cores for batch processing.

#### 3. Memory-Mapped I/O

OxiDex automatically uses memory-mapped I/O for files > 1MB. No configuration needed!

#### 4. Avoid Unnecessary Operations

```bash
# Slow (reads file twice)
oxidex photo.jpg > metadata.txt
oxidex -j photo.jpg > metadata.json

# Fast (read once, output both formats)
oxidex photo.jpg | tee metadata.txt
oxidex -j photo.jpg > metadata.json
```

### High Memory Usage

**Symptoms**: Process uses excessive RAM during batch processing.

**Causes & Solutions:**

1. **Large files**: Memory-mapped I/O keeps large files in virtual memory
   - **Solution**: Process files in smaller batches
   - Normal for files > 100MB

2. **Many files**: Processing thousands of files in parallel
   - **Solution**: Limit parallelism or process in batches
   - OxiDex automatically limits parallel threads

3. **Memory leaks**: Rare, but possible
   - **Solution**: Report issue with reproduction steps

**Monitoring Memory:**

```bash
# Linux
top -p $(pgrep oxidex)

# macOS
top | grep oxidex

# Windows Task Manager
# Look for oxidex.exe
```

### Slow Compilation

**Symptoms**: `cargo build` takes a very long time (first build).

**Normal Behavior**: First compilation downloads and compiles all dependencies (3-5 minutes).

**Solutions:**

1. **Use incremental builds**: Subsequent builds are much faster (10-30 seconds)

2. **Use `--release` only when needed**:
   ```bash
   # Fast (debug build for development)
   cargo build

   # Slow (release build for performance)
   cargo build --release
   ```

3. **Use `cargo check`** for faster syntax checking:
   ```bash
   cargo check  # Fast, no code generation
   ```

4. **Parallel compilation** (usually automatic):
   ```bash
   cargo build -j$(nproc)  # Use all cores
   ```

## Debugging Strategies

### Enable Verbose Output

OxiDex doesn't have a verbose flag yet, but you can use:

```bash
# Check file exists
ls -lh photo.jpg

# Verify format
file photo.jpg

# Try reading
oxidex photo.jpg
```

### Compare with Original ExifTool

If OxiDex produces unexpected results:

```bash
# Original ExifTool
exiftool photo.jpg > exiftool_output.txt

# OxiDex
oxidex photo.jpg > oxidex_output.txt

# Compare
diff exiftool_output.txt oxidex_output.txt
```

Report differences as issues (this helps improve compatibility).

### Check File Format

```bash
# Linux/macOS
file photo.jpg

# Hexdump first 32 bytes (magic number)
xxd -l 32 photo.jpg
```

Expected magic numbers:
- JPEG: `ff d8 ff`
- PNG: `89 50 4e 47`
- TIFF (LE): `49 49 2a 00`
- PDF: `25 50 44 46` (`%PDF`)

### Test with Sample Files

Use test fixtures from the repository:

```bash
cd oxidex
oxidex tests/fixtures/jpeg/sample_with_exif.jpg
oxidex tests/fixtures/png/sample_with_metadata.png
```

If test files work but your file doesn't, the file may be:
- Corrupted
- Using unsupported variant
- Non-standard format

### Run Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_jpeg_read

# Run with output
cargo test -- --nocapture
```

If tests pass but your file fails, please report an issue with the file (if possible).

## Performance Tips

### Best Practices for Fast Processing

1. **Use Release Builds**:
   ```bash
   cargo build --release
   ./target/release/oxidex
   ```

2. **Batch Process with `-r`**:
   ```bash
   oxidex -r /photos/  # Parallel processing
   ```

3. **Output to File** (avoid terminal I/O overhead):
   ```bash
   oxidex -j photos/ > metadata.json
   ```

4. **Use CSV for Large Datasets**:
   ```bash
   oxidex --csv -r photos/ > metadata.csv
   ```

5. **Process Locally** (avoid network drives):
   ```bash
   # Slow
   oxidex /mnt/network_drive/photos/

   # Fast
   rsync -av /mnt/network_drive/photos/ /local/photos/
   oxidex -r /local/photos/
   ```

### Benchmarking Performance

Compare OxiDex vs original ExifTool:

```bash
# Original ExifTool
time exiftool -r photos/ > /dev/null

# OxiDex
time oxidex -r photos/ > /dev/null
```

Expected results: OxiDex should be 2-5x faster.

If not, report the issue with:
- System info (OS, CPU, RAM)
- File count and sizes
- Performance measurements

### Run Benchmarks

```bash
# Run criterion benchmarks
cargo bench

# View results
open target/criterion/report/index.html  # macOS
xdg-open target/criterion/report/index.html  # Linux
```

Current baseline performance:
- Format detection: ~2.2 ns per operation
- JPEG segment parsing: ~24 ns per operation
- TIFF IFD parsing: ~94 ns per operation
- Full read_metadata: ~9.3 μs per file

## Known Limitations

### Features Not Yet Implemented

The following features are planned but not available in v1.1.0:

1. **Tag-specific reading**: `-TAG` to show only specific tags
2. **Tag deletion**: `-TAG=` to delete tags
3. **Group deletion**: `-all=` to remove all metadata
4. **Conditional edits**: `-if` for conditional operations
5. **Short format output**: `-s` for compact output
6. **Advanced file renaming**: Complex patterns with multiple tags

**Workaround**: Use the original ExifTool for these features.

### Format Limitations

- **RAW formats**: Not yet supported (CR2, NEF, ARW, etc.)
- **Video writing**: MP4/MOV read-only (write support planned)
- **PDF writing**: Read-only (write support planned)
- **Archive formats**: No ZIP, 7z support yet

See [Supported Formats](/reference/formats/) for the complete list.

### Platform-Specific Issues

#### Windows

- **Path separators**: Use forward slashes or escape backslashes:
  ```bash
  oxidex C:/photos/image.jpg  # OK
  oxidex "C:\\photos\\image.jpg"  # OK
  oxidex C:\photos\image.jpg  # May fail
  ```

#### macOS

- **Gatekeeper**: First run may require explicit permission:
  ```bash
  # If blocked by Gatekeeper
  xattr -d com.apple.quarantine ./target/release/oxidex
  ```

#### Linux

- **Permissions**: Ensure execute permission:
  ```bash
  chmod +x ./target/release/oxidex
  ```

## Getting Help

### Community Resources

1. **GitHub Issues**: Report bugs and request features
   - [https://github.com/swack-tools/oxidex/issues](https://github.com/swack-tools/oxidex/issues)

2. **GitHub Discussions**: Ask questions and discuss
   - [https://github.com/swack-tools/oxidex/discussions](https://github.com/swack-tools/oxidex/discussions)

3. **Documentation**: This user guide
   - [Introduction](/guide/)
   - [Command-Line Usage](/guide/cli-usage)
   - [Library API](/guide/library-api)

### Reporting Bugs

When reporting bugs, include:

1. **OxiDex version**:
   ```bash
   oxidex --version
   ```

2. **System information**:
   - OS and version
   - Rust version (`rustc --version`)
   - Architecture (x86_64, ARM64)

3. **File information** (if applicable):
   ```bash
   file photo.jpg
   ls -lh photo.jpg
   ```

4. **Full error message**:
   ```bash
   oxidex photo.jpg 2>&1 | tee error.log
   ```

5. **Steps to reproduce**:
   - Exact command used
   - Expected behavior
   - Actual behavior

6. **Sample file** (if possible and not confidential)

### Feature Requests

When requesting features:

1. **Describe the use case**: What problem does this solve?
2. **Provide examples**: How would you use this feature?
3. **Check existing issues**: Has someone already requested this?
4. **Consider contributing**: PRs welcome!

## Diagnostic Commands

### Check Installation

```bash
# Version
oxidex --version

# Help
oxidex --help

# Test with known good file
oxidex tests/fixtures/jpeg/sample_with_exif.jpg
```

### Check File Integrity

```bash
# File type
file photo.jpg

# File size
ls -lh photo.jpg

# Hex dump (first 32 bytes)
xxd -l 32 photo.jpg

# Try with original ExifTool
exiftool photo.jpg
```

### Check System Resources

```bash
# CPU count (for parallel processing)
nproc  # Linux
sysctl -n hw.ncpu  # macOS

# Available memory
free -h  # Linux
vm_stat  # macOS

# Disk space
df -h .
```

### Check Build Configuration

```bash
# Cargo version
cargo --version

# Rustc version
rustc --version

# Build info
cargo tree  # Show dependency tree
cargo metadata  # Show build metadata
```

## Performance Optimization Guide

### System Tuning

#### Linux

```bash
# Increase file descriptor limit (for batch processing)
ulimit -n 4096

# Check I/O scheduler (for SSD)
cat /sys/block/sda/queue/scheduler
# [mq-deadline] noop  # Good for SSDs
```

#### macOS

```bash
# Increase file descriptor limit
ulimit -n 4096

# Check disk performance
diskutil info /dev/disk0
```

#### Windows

- Disable Windows Defender real-time scanning for the directory (temporarily, for benchmarking)
- Use NVMe SSD for best performance
- Close unnecessary background applications

### Profiling

```bash
# CPU profiling with perf (Linux)
perf record --call-graph dwarf oxidex -r photos/
perf report

# Memory profiling with valgrind
valgrind --tool=massif oxidex photo.jpg
ms_print massif.out.*
```

## Additional Resources

- **[Getting Started](/guide/getting-started)**: Setup and configuration
- **[Command-Line Usage](/guide/cli-usage)**: CLI reference
- **[Supported Formats](/reference/formats/)**: Format compatibility
- **[Library API](/guide/library-api)**: Rust API documentation
- **[Original ExifTool](https://exiftool.org/)**: Perl implementation (for comparison)

## Emergency Fallback

If OxiDex isn't working for your use case, you can always fall back to the original ExifTool:

```bash
# Install original ExifTool
# macOS
brew install exiftool

# Ubuntu/Debian
sudo apt install libimage-exiftool-perl

# Use original ExifTool
exiftool photo.jpg
```

OxiDex aims for compatibility, but the original ExifTool has 20+ years of development and supports 300+ formats. Use the best tool for the job!
