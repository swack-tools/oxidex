# Command-Line Usage

This chapter covers how to use OxiDex from the command line to read, write, and manipulate metadata in your files.

## Overview

OxiDex provides a powerful command-line interface for metadata operations. The CLI is designed to be compatible with the original ExifTool's argument syntax, making it easy to migrate existing scripts and workflows.

**Current Status**: The CLI is fully functional with support for reading, writing, batch processing, file renaming, and date shifting operations.

## Basic Syntax

```bash
oxidex [OPTIONS] [TAG_MODIFICATIONS...] FILE|DIRECTORY
```

- **OPTIONS**: Flags and settings (e.g., `-j` for JSON output, `-r` for recursive)
- **TAG_MODIFICATIONS**: Tag assignments in the form `-TAG=VALUE`
- **FILE|DIRECTORY**: Path to a file or directory to process

## Reading Metadata

### Extract All Metadata

Display all metadata tags from a file:

```bash
oxidex photo.jpg
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
oxidex -j photo.jpg
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
oxidex --csv photo.jpg
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
oxidex -EXIF:Artist="John Doe" photo.jpg
```

**Output:**

```
    1 image files updated
```

### Modify Multiple Tags

Set multiple tags in a single command:

```bash
oxidex \
  -EXIF:Artist="John Doe" \
  -EXIF:Copyright="Copyright 2025 John Doe" \
  -IPTC:Caption-Abstract="Beautiful sunset" \
  photo.jpg
```

### Tag Naming Convention

OxiDex uses the **family:tag** naming convention:

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
oxidex --backup -EXIF:Artist="John Doe" photo.jpg
```

This creates `photo.jpg.bak` with the original file contents.

#### Preserve File Timestamps

Maintain the original file modification time after writing metadata:

```bash
oxidex --preserve-file-times -EXIF:Artist="John Doe" photo.jpg
```

The file's mtime will be restored after metadata is written.

#### Read-Only Mode

Prevent accidental modifications with read-only mode:

```bash
oxidex --readonly photo.jpg
```

This displays metadata but refuses any write operations.

#### Combining Safety Options

Use multiple safety flags together:

```bash
oxidex --backup --preserve-file-times \
  -EXIF:Artist="John Doe" \
  -EXIF:Copyright="Copyright 2025" \
  photo.jpg
```

## Batch Processing

### Process Multiple Files

Process all JPEG files in a directory:

```bash
oxidex -r *.jpg
```

### Recursive Directory Processing

Process all files in a directory and subdirectories:

```bash
oxidex -r /path/to/photos/
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
oxidex -r -EXIF:Copyright="Copyright 2025 John Doe" /path/to/photos/
```

### Output Batch Results as JSON

Process multiple files and output results as JSON:

```bash
oxidex -r -j /path/to/photos/ > metadata.json
```

## Copying Metadata

### Copy All Metadata

Copy all metadata from one file to another:

```bash
oxidex --TagsFromFile source.jpg dest.jpg
```

This reads all tags from `source.jpg` and writes them to `dest.jpg`.

### Copy Specific Tags

Copy only specific tags:

```bash
oxidex --TagsFromFile source.jpg \
  -EXIF:Artist \
  -EXIF:Copyright \
  -IPTC:Keywords \
  dest.jpg
```

This copies only the Artist, Copyright, and Keywords tags.

### Copy with Safety Options

Use backup and preserve timestamps when copying:

```bash
oxidex --TagsFromFile source.jpg \
  --backup \
  --preserve-file-times \
  dest.jpg
```

## File Renaming

### Rename Based on Metadata

OxiDex can rename files based on metadata patterns using the `-FileName<PATTERN` syntax.

#### Rename Using Date/Time

Rename files based on the DateTimeOriginal tag:

```bash
oxidex '-FileName<DateTimeOriginal' photo.jpg
```

By default, this uses the format: `YYYYMMDD_HHMMSS.jpg`

**Example:**
- Original: `IMG_1234.jpg`
- Renamed: `20250115_143000.jpg`

#### Custom Date Format

Specify a custom date format with the `-d` option:

```bash
oxidex -d '%Y-%m-%d_%H%M%S' '-FileName<DateTimeOriginal' photo.jpg
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
oxidex '-FileName<${EXIF:Make}_${EXIF:Model}_${DateTimeOriginal}' photo.jpg
```

**Example:**
- Original: `DSC_5678.jpg`
- Renamed: `Canon_EOS_5D_Mark_IV_20250115_143000.jpg`

#### Dry-Run Mode

Preview renames without actually changing files:

```bash
oxidex -n '-FileName<DateTimeOriginal' photo.jpg
```

**Output:**

```
photo.jpg -> 20250115_143000.jpg
```

No files are actually renamed; you just see what would happen.

#### Batch Rename

Rename all photos in a directory:

```bash
oxidex -r -d '%Y%m%d_%H%M%S' '-FileName<DateTimeOriginal' /path/to/photos/
```

## Date/Time Shifting

OxiDex can shift date/time tags by adding or subtracting time offsets, or setting absolute values.

### Add Time Offset

Add 1 year to all date tags:

```bash
oxidex '-AllDates+=1:0:0 0:0:0' photo.jpg
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
oxidex '-AllDates-=0:0:0 1:0:0' photo.jpg
```

**Use Case**: Fix incorrect camera clock that was set 1 hour ahead.

### Set Absolute Date/Time

Set a specific date/time value:

```bash
oxidex '-EXIF:DateTime=2025:01:15 10:30:00' photo.jpg
```

Format: `YYYY:MM:DD HH:MM:SS`

### Shift Specific Date Tag

Shift only a specific date tag instead of all dates:

```bash
oxidex '-EXIF:DateTimeOriginal+=0:0:1 0:0:0' photo.jpg
```

This adds 1 day to only the DateTimeOriginal tag.

### Batch Date Shifting

Shift dates for all photos in a directory (useful for timezone corrections):

```bash
oxidex -r '-AllDates+=0:0:0 8:0:0' /path/to/photos/
```

This adds 8 hours to all photos (e.g., UTC to PST conversion).

## Common Options

### `-j, --json`
Output metadata in JSON format (machine-readable).

```bash
oxidex -j photo.jpg
```

### `--csv`
Output metadata in CSV format (for spreadsheets).

```bash
oxidex --csv photo.jpg
```

### `-r`
Enable recursive directory processing.

```bash
oxidex -r /path/to/photos/
```

### `--backup`
Create a backup copy before modifying files (adds `.bak` extension).

```bash
oxidex --backup -EXIF:Artist="John Doe" photo.jpg
```

### `--preserve-file-times`
Restore the original file modification timestamp after writing metadata.

```bash
oxidex --preserve-file-times -EXIF:Copyright="2025" photo.jpg
```

### `--readonly`
Enable read-only mode to prevent any file modifications.

```bash
oxidex --readonly photo.jpg
```

### `-d <FORMAT>`
Specify custom date/time format for filename patterns.

```bash
oxidex -d '%Y-%m-%d' '-FileName<DateTimeOriginal' photo.jpg
```

### `-n`
Dry-run mode: show proposed renames without executing.

```bash
oxidex -n '-FileName<DateTimeOriginal' photo.jpg
```

### `-h, --help`
Display help information with all available options.

```bash
oxidex --help
```

### `-V, --version`
Display version information.

```bash
oxidex --version
```

## Practical Examples

### Photography Workflow

**Add copyright to all photos:**

```bash
oxidex -r --backup \
  -EXIF:Copyright="Copyright 2025 John Doe" \
  -EXIF:Artist="John Doe" \
  /path/to/photos/
```

**Rename photos by capture date:**

```bash
oxidex -r -d '%Y/%m/%Y%m%d_%H%M%S' \
  '-FileName<DateTimeOriginal' \
  /path/to/photos/
```

This organizes photos into year/month folders with dated filenames.

**Fix timezone (camera was set to wrong timezone):**

```bash
oxidex -r --backup '-AllDates-=5:0:0 0:0:0' /path/to/photos/
```

Subtracts 5 hours from all dates (e.g., EST to UTC).

### Metadata Report Generation

**Generate CSV report of all metadata:**

```bash
oxidex -r --csv /path/to/photos/ > metadata_report.csv
```

Open the resulting CSV in Excel or Google Sheets for analysis.

**Generate JSON report:**

```bash
oxidex -r -j /path/to/photos/ > metadata_report.json
```

Process the JSON with tools like `jq` or import into a database.

### Batch Metadata Editing

**Copy metadata from a reference image to all others:**

```bash
for file in *.jpg; do
  oxidex --TagsFromFile reference.jpg \
    -EXIF:Artist \
    -EXIF:Copyright \
    "$file"
done
```

**Remove all metadata (privacy):**

OxiDex supports removing all metadata from a file, which is useful for privacy or cleaning up files.

```bash
oxidex -all= photo.jpg
```


## Performance Tips

### Use Release Builds

For best performance, use release builds of OxiDex:

```bash
cargo build --release
```

Release builds are 2-5x faster than debug builds due to aggressive optimizations.

### Parallel Batch Processing

OxiDex automatically processes multiple files in parallel using all available CPU cores when using the `-r` flag. No additional configuration needed!

### Memory-Mapped I/O

OxiDex uses memory-mapped I/O for efficient processing of large files. This is automatic and requires no user configuration.

## Error Handling

### Common Errors

**"Error: File not found"**

The specified file path doesn't exist. Check the path and try again:

```bash
ls photo.jpg  # Verify file exists
oxidex photo.jpg
```

**"Error: File is read-only"**

The file has read-only permissions. Either change permissions or use `--readonly` if you only want to read metadata:

```bash
chmod u+w photo.jpg  # Make writable
oxidex -EXIF:Artist="John Doe" photo.jpg
```

**"Error: Invalid value for TAG"**

The value provided is not valid for that metadata tag. Check the tag's expected data type:

```bash
# Wrong: FNumber expects a rational number
oxidex -EXIF:FNumber="invalid" photo.jpg  # Error

# Correct:
oxidex -EXIF:FNumber="5.6" photo.jpg  # Success
```

**"Error: Unsupported file format"**

The file format is not yet supported by OxiDex. See [Supported Formats](/reference/formats/) for the current list.

### Verbose Error Messages

OxiDex provides detailed error messages to help diagnose issues. Read the error message carefully—it usually indicates exactly what went wrong.

## Limitations

### Features Not Yet Implemented

The following features are planned but not yet available:

- ⏳ **Conditional Edits**: `-if` for conditional tag modifications

For this feature, use the original ExifTool in the meantime.

## Compatibility with Original ExifTool

OxiDex aims for CLI compatibility with the original ExifTool. Most common commands should work identically:

**Compatible:**
- ✅ Reading metadata: `oxidex file.jpg`
- ✅ JSON output: `oxidex -j file.jpg`
- ✅ Short format: `oxidex -s file.jpg`
- ✅ Writing tags: `oxidex -TAG=VALUE file.jpg`
- ✅ Tag-specific reading: `oxidex -Make -Model file.jpg`
- ✅ Tag deletion: `oxidex -TAG= file.jpg`
- ✅ Clear all metadata: `oxidex -all= file.jpg`
- ✅ Recursive processing: `oxidex -r directory/`
- ✅ Backup creation: `oxidex --backup file.jpg`
- ✅ Date shifting: `oxidex '-AllDates+=1:0:0 0:0:0' file.jpg`
- ✅ File renaming: `oxidex '-FileName<DateTimeOriginal' file.jpg`
- ✅ Metadata copying: `oxidex --TagsFromFile src.jpg dest.jpg`

**Not Yet Compatible:**
- ⏳ Conditional operations: `oxidex -if '$Make eq "Canon"' file.jpg`

## Next Steps

- **[Library API](/guide/library-api)**: Use OxiDex as a Rust library
- **[Troubleshooting](/guide/troubleshooting)**: Common issues and solutions
- **[Supported Formats](/reference/formats/)**: See what file formats are supported
