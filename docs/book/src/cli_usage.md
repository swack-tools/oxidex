# Command-Line Usage

This chapter covers how to use ExifTool-RS from the command line to read, write, and manipulate metadata in your files.

## Overview

ExifTool-RS provides a powerful command-line interface for metadata operations. The CLI is designed to be compatible with the original ExifTool's argument syntax, making it easy to migrate existing scripts and workflows.

**Current Status**: The CLI is fully functional with support for reading, writing, batch processing, file renaming, and date shifting operations.

## Basic Syntax

```bash
exiftool-rs [OPTIONS] [TAG_MODIFICATIONS...] FILE|DIRECTORY
```

- **OPTIONS**: Flags and settings (e.g., `-j` for JSON output, `-r` for recursive)
- **TAG_MODIFICATIONS**: Tag assignments in the form `-TAG=VALUE`
- **FILE|DIRECTORY**: Path to a file or directory to process

## Reading Metadata

### Extract All Metadata

Display all metadata tags from a file:

```bash
exiftool-rs photo.jpg
```

**Example Output:**

```
File: photo.jpg
Found 15 metadata tag(s):

EXIF:Make                       : Canon
EXIF:Model                      : Canon EOS 5D Mark IV
EXIF:DateTime                   : 2025:01:15 14:30:00
EXIF:ExposureTime               : 1/125
EXIF:FNumber                    : 5.6
EXIF:ISO                        : 400
EXIF:LensModel                  : EF24-70mm f/2.8L II USM
GPS:GPSLatitude                 : 37.7749
GPS:GPSLongitude                : -122.4194
IPTC:Caption-Abstract           : Golden Gate Bridge at sunset
IPTC:Keywords                   : bridge, sunset, san francisco
XMP:Copyright                   : 2025 John Doe
```

### JSON Output

Export metadata in JSON format (machine-readable):

```bash
exiftool-rs -j photo.jpg
```

**Example Output:**

```json
{
  "EXIF:Make": "Canon",
  "EXIF:Model": "Canon EOS 5D Mark IV",
  "EXIF:DateTime": "2025:01:15 14:30:00",
  "EXIF:ExposureTime": "1/125",
  "EXIF:FNumber": "5.6",
  "EXIF:ISO": "400",
  "GPS:GPSLatitude": 37.7749,
  "GPS:GPSLongitude": -122.4194
}
```

### CSV Output

Export metadata in CSV format (for spreadsheets):

```bash
exiftool-rs --csv photo.jpg
```

**Example Output:**

```csv
tag_name,tag_value
EXIF:Make,Canon
EXIF:Model,Canon EOS 5D Mark IV
EXIF:DateTime,2025:01:15 14:30:00
EXIF:ExposureTime,1/125
EXIF:FNumber,5.6
EXIF:ISO,400
```

## Writing Metadata

### Modify Single Tag

Set the value of a specific metadata tag:

```bash
exiftool-rs -EXIF:Artist="John Doe" photo.jpg
```

**Output:**

```
    1 image files updated
```

### Modify Multiple Tags

Set multiple tags in a single command:

```bash
exiftool-rs \
  -EXIF:Artist="John Doe" \
  -EXIF:Copyright="Copyright 2025 John Doe" \
  -IPTC:Caption-Abstract="Beautiful sunset" \
  photo.jpg
```

### Tag Naming Convention

ExifTool-RS uses the **family:tag** naming convention:

- `EXIF:Make` - Camera manufacturer (EXIF family)
- `GPS:GPSLatitude` - GPS latitude (GPS family)
- `IPTC:Keywords` - Image keywords (IPTC family)
- `XMP:Copyright` - Copyright notice (XMP family)

Common families:
- **EXIF**: Camera settings, image parameters
- **GPS**: Geolocation data
- **IPTC**: Press/media metadata
- **XMP**: Extensible metadata
- **JFIF**: JPEG File Interchange Format
- **PNG**: PNG-specific metadata
- **QuickTime**: Video/audio metadata

### Safety Options

#### Create Backup Before Writing

Always create a backup copy before modifying files:

```bash
exiftool-rs --backup -EXIF:Artist="John Doe" photo.jpg
```

This creates `photo.jpg.bak` with the original file contents.

#### Preserve File Timestamps

Maintain the original file modification time after writing metadata:

```bash
exiftool-rs --preserve-file-times -EXIF:Artist="John Doe" photo.jpg
```

The file's mtime will be restored after metadata is written.

#### Read-Only Mode

Prevent accidental modifications with read-only mode:

```bash
exiftool-rs --readonly photo.jpg
```

This displays metadata but refuses any write operations.

#### Combining Safety Options

Use multiple safety flags together:

```bash
exiftool-rs --backup --preserve-file-times \
  -EXIF:Artist="John Doe" \
  -EXIF:Copyright="Copyright 2025" \
  photo.jpg
```

## Batch Processing

### Process Multiple Files

Process all JPEG files in a directory:

```bash
exiftool-rs -r *.jpg
```

### Recursive Directory Processing

Process all files in a directory and subdirectories:

```bash
exiftool-rs -r /path/to/photos/
```

**Example Output:**

```
Processing: /path/to/photos/2024/vacation/IMG_001.jpg
Processing: /path/to/photos/2024/vacation/IMG_002.jpg
Processing: /path/to/photos/2025/wildlife/IMG_003.jpg

Batch Processing Statistics:
  Total files processed: 3
  Successful: 3
  Errors: 0
```

### Batch Write Operations

Modify metadata for all files in a directory:

```bash
exiftool-rs -r -EXIF:Copyright="Copyright 2025 John Doe" /path/to/photos/
```

### Output Batch Results as JSON

Process multiple files and output results as JSON:

```bash
exiftool-rs -r -j /path/to/photos/ > metadata.json
```

## Copying Metadata

### Copy All Metadata

Copy all metadata from one file to another:

```bash
exiftool-rs --TagsFromFile source.jpg dest.jpg
```

This reads all tags from `source.jpg` and writes them to `dest.jpg`.

### Copy Specific Tags

Copy only specific tags:

```bash
exiftool-rs --TagsFromFile source.jpg \
  -EXIF:Artist \
  -EXIF:Copyright \
  -IPTC:Keywords \
  dest.jpg
```

This copies only the Artist, Copyright, and Keywords tags.

### Copy with Safety Options

Use backup and preserve timestamps when copying:

```bash
exiftool-rs --TagsFromFile source.jpg \
  --backup \
  --preserve-file-times \
  dest.jpg
```

## File Renaming

### Rename Based on Metadata

ExifTool-RS can rename files based on metadata patterns using the `-FileName<PATTERN` syntax.

#### Rename Using Date/Time

Rename files based on the DateTimeOriginal tag:

```bash
exiftool-rs '-FileName<DateTimeOriginal' photo.jpg
```

By default, this uses the format: `YYYYMMDD_HHMMSS.jpg`

**Example:**
- Original: `IMG_1234.jpg`
- Renamed: `20250115_143000.jpg`

#### Custom Date Format

Specify a custom date format with the `-d` option:

```bash
exiftool-rs -d '%Y-%m-%d_%H%M%S' '-FileName<DateTimeOriginal' photo.jpg
```

**Format Specifiers:**
- `%Y` - Year (4 digits)
- `%m` - Month (01-12)
- `%d` - Day (01-31)
- `%H` - Hour (00-23)
- `%M` - Minute (00-59)
- `%S` - Second (00-59)

**Example:**
- Result: `2025-01-15_143000.jpg`

#### Complex Renaming Patterns

Combine multiple metadata tags in the filename pattern:

```bash
exiftool-rs '-FileName<${EXIF:Make}_${EXIF:Model}_${DateTimeOriginal}' photo.jpg
```

**Example:**
- Original: `DSC_5678.jpg`
- Renamed: `Canon_EOS_5D_Mark_IV_20250115_143000.jpg`

#### Dry-Run Mode

Preview renames without actually changing files:

```bash
exiftool-rs -n '-FileName<DateTimeOriginal' photo.jpg
```

**Output:**

```
photo.jpg -> 20250115_143000.jpg
```

No files are actually renamed; you just see what would happen.

#### Batch Rename

Rename all photos in a directory:

```bash
exiftool-rs -r -d '%Y%m%d_%H%M%S' '-FileName<DateTimeOriginal' /path/to/photos/
```

## Date/Time Shifting

ExifTool-RS can shift date/time tags by adding or subtracting time offsets, or setting absolute values.

### Add Time Offset

Add 1 year to all date tags:

```bash
exiftool-rs '-AllDates+=1:0:0 0:0:0' photo.jpg
```

Offset format: `Y:M:D H:M:S` (years:months:days hours:minutes:seconds)

**Examples:**
- Add 1 year: `-AllDates+=1:0:0 0:0:0`
- Add 2 months: `-AllDates+=0:2:0 0:0:0`
- Add 3 hours: `-AllDates+=0:0:0 3:0:0`
- Add 30 minutes: `-AllDates+=0:0:0 0:30:0`

### Subtract Time Offset

Subtract 1 hour from all date tags:

```bash
exiftool-rs '-AllDates-=0:0:0 1:0:0' photo.jpg
```

**Use Case**: Fix incorrect camera clock that was set 1 hour ahead.

### Set Absolute Date/Time

Set a specific date/time value:

```bash
exiftool-rs '-EXIF:DateTime=2025:01:15 10:30:00' photo.jpg
```

Format: `YYYY:MM:DD HH:MM:SS`

### Shift Specific Date Tag

Shift only a specific date tag instead of all dates:

```bash
exiftool-rs '-EXIF:DateTimeOriginal+=0:0:1 0:0:0' photo.jpg
```

This adds 1 day to only the DateTimeOriginal tag.

### Batch Date Shifting

Shift dates for all photos in a directory (useful for timezone corrections):

```bash
exiftool-rs -r '-AllDates+=0:0:0 8:0:0' /path/to/photos/
```

This adds 8 hours to all photos (e.g., UTC to PST conversion).

## Common Options

### `-j, --json`
Output metadata in JSON format (machine-readable).

```bash
exiftool-rs -j photo.jpg
```

### `--csv`
Output metadata in CSV format (for spreadsheets).

```bash
exiftool-rs --csv photo.jpg
```

### `-r`
Enable recursive directory processing.

```bash
exiftool-rs -r /path/to/photos/
```

### `--backup`
Create a backup copy before modifying files (adds `.bak` extension).

```bash
exiftool-rs --backup -EXIF:Artist="John Doe" photo.jpg
```

### `--preserve-file-times`
Restore the original file modification timestamp after writing metadata.

```bash
exiftool-rs --preserve-file-times -EXIF:Copyright="2025" photo.jpg
```

### `--readonly`
Enable read-only mode to prevent any file modifications.

```bash
exiftool-rs --readonly photo.jpg
```

### `-d <FORMAT>`
Specify custom date/time format for filename patterns.

```bash
exiftool-rs -d '%Y-%m-%d' '-FileName<DateTimeOriginal' photo.jpg
```

### `-n`
Dry-run mode: show proposed renames without executing.

```bash
exiftool-rs -n '-FileName<DateTimeOriginal' photo.jpg
```

### `-h, --help`
Display help information with all available options.

```bash
exiftool-rs --help
```

### `-V, --version`
Display version information.

```bash
exiftool-rs --version
```

## Practical Examples

### Photography Workflow

**Add copyright to all photos:**

```bash
exiftool-rs -r --backup \
  -EXIF:Copyright="Copyright 2025 John Doe" \
  -EXIF:Artist="John Doe" \
  /path/to/photos/
```

**Rename photos by capture date:**

```bash
exiftool-rs -r -d '%Y/%m/%Y%m%d_%H%M%S' \
  '-FileName<DateTimeOriginal' \
  /path/to/photos/
```

This organizes photos into year/month folders with dated filenames.

**Fix timezone (camera was set to wrong timezone):**

```bash
exiftool-rs -r --backup '-AllDates-=5:0:0 0:0:0' /path/to/photos/
```

Subtracts 5 hours from all dates (e.g., EST to UTC).

### Metadata Report Generation

**Generate CSV report of all metadata:**

```bash
exiftool-rs -r --csv /path/to/photos/ > metadata_report.csv
```

Open the resulting CSV in Excel or Google Sheets for analysis.

**Generate JSON report:**

```bash
exiftool-rs -r -j /path/to/photos/ > metadata_report.json
```

Process the JSON with tools like `jq` or import into a database.

### Batch Metadata Editing

**Copy metadata from a reference image to all others:**

```bash
for file in *.jpg; do
  exiftool-rs --TagsFromFile reference.jpg \
    -EXIF:Artist \
    -EXIF:Copyright \
    "$file"
done
```

**Remove all metadata (privacy):**

This feature is planned but not yet implemented. For now, use the original ExifTool:

```bash
# Future feature (not yet available)
# exiftool-rs -all= photo.jpg
```

## Performance Tips

### Use Release Builds

For best performance, use release builds of ExifTool-RS:

```bash
cargo build --release
```

Release builds are 2-5x faster than debug builds due to aggressive optimizations.

### Parallel Batch Processing

ExifTool-RS automatically processes multiple files in parallel using all available CPU cores when using the `-r` flag. No additional configuration needed!

### Memory-Mapped I/O

ExifTool-RS uses memory-mapped I/O for efficient processing of large files. This is automatic and requires no user configuration.

## Error Handling

### Common Errors

**"Error: File not found"**

The specified file path doesn't exist. Check the path and try again:

```bash
ls photo.jpg  # Verify file exists
exiftool-rs photo.jpg
```

**"Error: File is read-only"**

The file has read-only permissions. Either change permissions or use `--readonly` if you only want to read metadata:

```bash
chmod u+w photo.jpg  # Make writable
exiftool-rs -EXIF:Artist="John Doe" photo.jpg
```

**"Error: Invalid value for TAG"**

The value provided is not valid for that metadata tag. Check the tag's expected data type:

```bash
# Wrong: FNumber expects a rational number
exiftool-rs -EXIF:FNumber="invalid" photo.jpg  # Error

# Correct:
exiftool-rs -EXIF:FNumber="5.6" photo.jpg  # Success
```

**"Error: Unsupported file format"**

The file format is not yet supported by ExifTool-RS. See [Supported Formats](formats.md) for the current list.

### Verbose Error Messages

ExifTool-RS provides detailed error messages to help diagnose issues. Read the error message carefully—it usually indicates exactly what went wrong.

## Limitations

### Features Not Yet Implemented

The following features are planned but not yet available:

- ⏳ **Specific Tag Extraction**: `-TAG` to show only specific tags (currently shows all tags)
- ⏳ **Tag Deletion**: `-TAG=` to delete a tag
- ⏳ **Group Deletion**: `-all=` to delete all metadata
- ⏳ **Short Format Output**: `-s` for compact output
- ⏳ **Conditional Edits**: `-if` for conditional tag modifications

For these features, use the original ExifTool in the meantime.

## Compatibility with Original ExifTool

ExifTool-RS aims for CLI compatibility with the original ExifTool. Most common commands should work identically:

**Compatible:**
- ✅ Reading metadata: `exiftool-rs file.jpg`
- ✅ JSON output: `exiftool-rs -j file.jpg`
- ✅ Writing tags: `exiftool-rs -TAG=VALUE file.jpg`
- ✅ Recursive processing: `exiftool-rs -r directory/`
- ✅ Backup creation: `exiftool-rs --backup file.jpg`
- ✅ Date shifting: `exiftool-rs '-AllDates+=1:0:0 0:0:0' file.jpg`
- ✅ File renaming: `exiftool-rs '-FileName<DateTimeOriginal' file.jpg`
- ✅ Metadata copying: `exiftool-rs --TagsFromFile src.jpg dest.jpg`

**Not Yet Compatible:**
- ⏳ Tag-specific reading: `exiftool-rs -Make -Model file.jpg`
- ⏳ Tag deletion: `exiftool-rs -TAG= file.jpg`
- ⏳ Conditional operations: `exiftool-rs -if '$Make eq "Canon"' file.jpg`

## Next Steps

- **[Library API](library_api.md)**: Use ExifTool-RS as a Rust library
- **[C FFI Integration](ffi.md)**: Integrate with C, Python, or other languages
- **[Supported Formats](formats.md)**: See what file formats are supported
- **[Troubleshooting](troubleshooting.md)**: Common issues and solutions
