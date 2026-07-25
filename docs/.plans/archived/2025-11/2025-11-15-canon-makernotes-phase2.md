# Canon MakerNotes Phase 2: Complex Array Tags Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extend Canon MakerNote parser to decode complex array tags (CameraSettings, ShotInfo, FocalLength) and extract meaningful camera settings.

**Architecture:** Build array decoder on top of Phase 1's IFD parser. Add tag-specific decoders that interpret array data based on Canon's proprietary formats. Use lookup tables for enumerated values (macro modes, flash modes, etc.).

**Tech Stack:** Rust, nom parser combinators, existing TIFF/EXIF infrastructure from Phase 1

**Estimated Time:** 6-8 hours total

**Prerequisites:** Canon MakerNotes Phase 1 complete (simple tags working)

---

## Background

Phase 1 implemented basic Canon MakerNote parsing for simple string/integer tags. Phase 2 adds support for Canon's complex array tags, which contain multiple camera settings packed into integer arrays.

**Phase 2 Target Tags:**
- **CameraSettings (0x0001):** Array of ~50 values including macro mode, self-timer, quality, flash mode, drive mode, focus mode, image size, etc.
- **ShotInfo (0x0004):** Array of shooting information including auto ISO, base ISO, measured EV, target aperture, exposure compensation, white balance, etc.
- **FocalLength (0x0002):** Array containing focal length info and focal units
- **Panorama (0x0005):** Panorama information (if present)
- **CustomFunctions (0x000F):** Custom function settings (camera-dependent)

**Reference:** ExifTool's Canon.pm provides the canonical tag definitions and array indices.

---

## Task 1: Add Array Value Extraction Infrastructure

**Files:**
- Modify: `src/parsers/tiff/makernotes/canon.rs` (add array extraction helpers)
- Test: Within `src/parsers/tiff/makernotes/canon.rs` (#[cfg(test)] module)

### Step 1: Write test for i16 array extraction

Add to the `#[cfg(test)]` module in `canon.rs`:

```rust
#[test]
fn test_extract_i16_array_inline() {
    // Test inline array (count * 2 <= 4 bytes)
    let entry = IfdEntry {
        tag: CANON_FOCAL_LENGTH,
        field_type: 3, // SHORT
        count: 2,
        value_offset: 0x0064_0032, // Two shorts: 50, 100 (little-endian)
    };

    let result = extract_i16_array(&entry, &[], ByteOrder::LittleEndian);
    assert_eq!(result, Some(vec![50, 100]));
}

#[test]
fn test_extract_i16_array_offset() {
    // Test offset-based array (count * 2 > 4 bytes)
    let entry = IfdEntry {
        tag: CANON_CAMERA_SETTINGS,
        field_type: 3, // SHORT
        count: 4,
        value_offset: 0, // Offset to data
    };

    // Data at offset 0: [1, 2, 3, 4] as little-endian shorts
    let data = vec![
        0x01, 0x00, // 1
        0x02, 0x00, // 2
        0x03, 0x00, // 3
        0x04, 0x00, // 4
    ];

    let result = extract_i16_array(&entry, &data, ByteOrder::LittleEndian);
    assert_eq!(result, Some(vec![1, 2, 3, 4]));
}

#[test]
fn test_extract_i16_array_big_endian() {
    let entry = IfdEntry {
        tag: CANON_CAMERA_SETTINGS,
        field_type: 3,
        count: 2,
        value_offset: 0,
    };

    // Big-endian data: [256, 512]
    let data = vec![
        0x01, 0x00, // 256
        0x02, 0x00, // 512
    ];

    let result = extract_i16_array(&entry, &data, ByteOrder::BigEndian);
    assert_eq!(result, Some(vec![256, 512]));
}
```

### Step 2: Run test to verify it fails

Run: `cargo test test_extract_i16_array --lib -- --nocapture`
Expected: FAIL - "cannot find function `extract_i16_array`"

### Step 3: Implement extract_i16_array helper function

Add to `canon.rs` after the existing `extract_integer_value` function (around line 430):

```rust
/// Extracts an array of signed 16-bit integers from an IFD entry.
///
/// Handles both inline arrays (≤2 values fitting in 4-byte value_offset)
/// and offset-based arrays (>2 values stored elsewhere in data).
fn extract_i16_array(entry: &IfdEntry, ifd_data: &[u8], byte_order: ByteOrder) -> Option<Vec<i16>> {
    // Canon array tags use SHORT type (field_type = 3)
    if entry.field_type != 3 {
        return None;
    }

    let count = entry.count as usize;
    let bytes_needed = count * 2; // 2 bytes per i16

    // Inline: ≤2 shorts fit in 4-byte value_offset field
    if bytes_needed <= 4 {
        let mut result = Vec::with_capacity(count);
        let bytes = entry.value_offset.to_le_bytes();

        for i in 0..count {
            let offset = i * 2;
            let value = match byte_order {
                ByteOrder::LittleEndian => i16::from_le_bytes([bytes[offset], bytes[offset + 1]]),
                ByteOrder::BigEndian => i16::from_be_bytes([bytes[offset], bytes[offset + 1]]),
            };
            result.push(value);
        }

        return Some(result);
    }

    // Offset-based: read from ifd_data at specified offset
    let offset = entry.value_offset as usize;

    // Bounds check
    if offset + bytes_needed > ifd_data.len() {
        return None;
    }

    let mut result = Vec::with_capacity(count);
    let array_data = &ifd_data[offset..offset + bytes_needed];

    for i in 0..count {
        let byte_offset = i * 2;
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                i16::from_le_bytes([array_data[byte_offset], array_data[byte_offset + 1]])
            }
            ByteOrder::BigEndian => {
                i16::from_be_bytes([array_data[byte_offset], array_data[byte_offset + 1]])
            }
        };
        result.push(value);
    }

    Some(result)
}
```

### Step 4: Run test to verify it passes

Run: `cargo test test_extract_i16_array --lib -- --nocapture`
Expected: PASS (3 tests)

### Step 5: Commit

```bash
git add src/parsers/tiff/makernotes/canon.rs
git commit -m "feat(canon): add i16 array extraction helper

Add extract_i16_array() to handle Canon's array tags.
Supports both inline (≤2 values) and offset-based arrays.
Handles both little-endian and big-endian byte order.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 2: Add CameraSettings Tag Constants and Indices

**Files:**
- Modify: `src/parsers/tiff/makernotes/canon.rs` (add CameraSettings constants)
- Test: Within `src/parsers/tiff/makernotes/canon.rs` (#[cfg(test)] module)

### Step 1: Write test for CameraSettings indices

Add to the `#[cfg(test)]` module:

```rust
#[test]
fn test_camera_settings_indices() {
    // Verify key CameraSettings array indices are defined correctly
    assert_eq!(CAMERA_SETTINGS_MACRO_MODE, 1);
    assert_eq!(CAMERA_SETTINGS_SELF_TIMER, 2);
    assert_eq!(CAMERA_SETTINGS_QUALITY, 3);
    assert_eq!(CAMERA_SETTINGS_FLASH_MODE, 4);
    assert_eq!(CAMERA_SETTINGS_DRIVE_MODE, 5);
    assert_eq!(CAMERA_SETTINGS_FOCUS_MODE, 7);
    assert_eq!(CAMERA_SETTINGS_IMAGE_SIZE, 10);
    assert_eq!(CAMERA_SETTINGS_EASY_MODE, 11);
    assert_eq!(CAMERA_SETTINGS_CONTRAST, 13);
    assert_eq!(CAMERA_SETTINGS_SATURATION, 14);
    assert_eq!(CAMERA_SETTINGS_SHARPNESS, 15);
    assert_eq!(CAMERA_SETTINGS_ISO, 16);
    assert_eq!(CAMERA_SETTINGS_METERING_MODE, 17);
    assert_eq!(CAMERA_SETTINGS_FOCUS_TYPE, 18);
    assert_eq!(CAMERA_SETTINGS_AF_POINT, 19);
    assert_eq!(CAMERA_SETTINGS_EXPOSURE_MODE, 20);
    assert_eq!(CAMERA_SETTINGS_FLASH_ACTIVITY, 28);
    assert_eq!(CAMERA_SETTINGS_FOCUS_CONTINUOUS, 32);
}
```

### Step 2: Run test to verify it fails

Run: `cargo test test_camera_settings_indices --lib -- --nocapture`
Expected: FAIL - "cannot find value `CAMERA_SETTINGS_MACRO_MODE`"

### Step 3: Add CameraSettings array index constants

Add after the Canon tag ID constants (around line 30):

```rust
// CameraSettings array (tag 0x0001) indices
// Array contains ~50 values with camera settings
// Reference: ExifTool Canon.pm CameraSettings table
const CAMERA_SETTINGS_MACRO_MODE: usize = 1;
const CAMERA_SETTINGS_SELF_TIMER: usize = 2;
const CAMERA_SETTINGS_QUALITY: usize = 3;
const CAMERA_SETTINGS_FLASH_MODE: usize = 4;
const CAMERA_SETTINGS_DRIVE_MODE: usize = 5;
const CAMERA_SETTINGS_FOCUS_MODE: usize = 7;
const CAMERA_SETTINGS_IMAGE_SIZE: usize = 10;
const CAMERA_SETTINGS_EASY_MODE: usize = 11;
const CAMERA_SETTINGS_CONTRAST: usize = 13;
const CAMERA_SETTINGS_SATURATION: usize = 14;
const CAMERA_SETTINGS_SHARPNESS: usize = 15;
const CAMERA_SETTINGS_ISO: usize = 16;
const CAMERA_SETTINGS_METERING_MODE: usize = 17;
const CAMERA_SETTINGS_FOCUS_TYPE: usize = 18;
const CAMERA_SETTINGS_AF_POINT: usize = 19;
const CAMERA_SETTINGS_EXPOSURE_MODE: usize = 20;
const CAMERA_SETTINGS_FLASH_ACTIVITY: usize = 28;
const CAMERA_SETTINGS_FOCUS_CONTINUOUS: usize = 32;
```

### Step 4: Run test to verify it passes

Run: `cargo test test_camera_settings_indices --lib -- --nocapture`
Expected: PASS

### Step 5: Commit

```bash
git add src/parsers/tiff/makernotes/canon.rs
git commit -m "feat(canon): add CameraSettings array index constants

Define array indices for Canon CameraSettings tag (0x0001).
Based on ExifTool's Canon.pm reference implementation.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 3: Add CameraSettings Value Decoders

**Files:**
- Modify: `src/parsers/tiff/makernotes/canon.rs` (add decoder functions)
- Test: Within `src/parsers/tiff/makernotes/canon.rs` (#[cfg(test)] module)

### Step 1: Write test for macro mode decoder

Add to the `#[cfg(test)]` module:

```rust
#[test]
fn test_decode_macro_mode() {
    assert_eq!(decode_macro_mode(1), "Macro");
    assert_eq!(decode_macro_mode(2), "Normal");
    assert_eq!(decode_macro_mode(99), "Unknown (99)");
}

#[test]
fn test_decode_quality() {
    assert_eq!(decode_quality(2), "Normal");
    assert_eq!(decode_quality(3), "Fine");
    assert_eq!(decode_quality(5), "Superfine");
    assert_eq!(decode_quality(130), "Normal Movie");
    assert_eq!(decode_quality(131), "Movie (2)");
    assert_eq!(decode_quality(99), "Unknown (99)");
}

#[test]
fn test_decode_flash_mode() {
    assert_eq!(decode_flash_mode(0), "Off");
    assert_eq!(decode_flash_mode(1), "Auto");
    assert_eq!(decode_flash_mode(2), "On");
    assert_eq!(decode_flash_mode(3), "Red-eye Reduction");
    assert_eq!(decode_flash_mode(4), "Slow Sync");
    assert_eq!(decode_flash_mode(5), "Auto + Red-eye Reduction");
    assert_eq!(decode_flash_mode(6), "On + Red-eye Reduction");
    assert_eq!(decode_flash_mode(16), "External Flash");
    assert_eq!(decode_flash_mode(99), "Unknown (99)");
}

#[test]
fn test_decode_drive_mode() {
    assert_eq!(decode_drive_mode(0), "Single");
    assert_eq!(decode_drive_mode(1), "Continuous");
    assert_eq!(decode_drive_mode(2), "Movie");
    assert_eq!(decode_drive_mode(4), "Continuous, Speed Priority");
    assert_eq!(decode_drive_mode(5), "Continuous, Low");
    assert_eq!(decode_drive_mode(6), "Continuous, High");
    assert_eq!(decode_drive_mode(99), "Unknown (99)");
}

#[test]
fn test_decode_focus_mode() {
    assert_eq!(decode_focus_mode(0), "One-shot AF");
    assert_eq!(decode_focus_mode(1), "AI Servo AF");
    assert_eq!(decode_focus_mode(2), "AI Focus AF");
    assert_eq!(decode_focus_mode(3), "Manual Focus (3)");
    assert_eq!(decode_focus_mode(4), "Single");
    assert_eq!(decode_focus_mode(5), "Continuous");
    assert_eq!(decode_focus_mode(6), "Manual Focus (6)");
    assert_eq!(decode_focus_mode(16), "Pan Focus");
    assert_eq!(decode_focus_mode(99), "Unknown (99)");
}

#[test]
fn test_decode_metering_mode() {
    assert_eq!(decode_metering_mode(3), "Evaluative");
    assert_eq!(decode_metering_mode(4), "Partial");
    assert_eq!(decode_metering_mode(5), "Center-weighted Average");
    assert_eq!(decode_metering_mode(99), "Unknown (99)");
}

#[test]
fn test_decode_exposure_mode() {
    assert_eq!(decode_exposure_mode(0), "Easy");
    assert_eq!(decode_exposure_mode(1), "Program AE");
    assert_eq!(decode_exposure_mode(2), "Shutter Priority");
    assert_eq!(decode_exposure_mode(3), "Aperture Priority");
    assert_eq!(decode_exposure_mode(4), "Manual");
    assert_eq!(decode_exposure_mode(5), "Depth-of-field AE");
    assert_eq!(decode_exposure_mode(6), "M-Dep");
    assert_eq!(decode_exposure_mode(7), "Bulb");
    assert_eq!(decode_exposure_mode(99), "Unknown (99)");
}
```

### Step 2: Run test to verify it fails

Run: `cargo test test_decode_ --lib -- --nocapture`
Expected: FAIL - "cannot find function `decode_macro_mode`"

### Step 3: Implement decoder functions

Add after the CameraSettings constants (around line 50):

```rust
/// Decodes Canon macro mode value to human-readable string
fn decode_macro_mode(value: i16) -> String {
    match value {
        1 => "Macro".to_string(),
        2 => "Normal".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon quality setting to human-readable string
fn decode_quality(value: i16) -> String {
    match value {
        -1 => "n/a".to_string(),
        1 => "Economy".to_string(),
        2 => "Normal".to_string(),
        3 => "Fine".to_string(),
        4 => "RAW".to_string(),
        5 => "Superfine".to_string(),
        7 => "CRAW".to_string(),
        130 => "Normal Movie".to_string(),
        131 => "Movie (2)".to_string(),
        132 => "Movie (3)".to_string(),
        133 => "Movie (4)".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon flash mode to human-readable string
fn decode_flash_mode(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Auto".to_string(),
        2 => "On".to_string(),
        3 => "Red-eye Reduction".to_string(),
        4 => "Slow Sync".to_string(),
        5 => "Auto + Red-eye Reduction".to_string(),
        6 => "On + Red-eye Reduction".to_string(),
        16 => "External Flash".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon drive mode to human-readable string
fn decode_drive_mode(value: i16) -> String {
    match value {
        0 => "Single".to_string(),
        1 => "Continuous".to_string(),
        2 => "Movie".to_string(),
        4 => "Continuous, Speed Priority".to_string(),
        5 => "Continuous, Low".to_string(),
        6 => "Continuous, High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon focus mode to human-readable string
fn decode_focus_mode(value: i16) -> String {
    match value {
        0 => "One-shot AF".to_string(),
        1 => "AI Servo AF".to_string(),
        2 => "AI Focus AF".to_string(),
        3 => "Manual Focus (3)".to_string(),
        4 => "Single".to_string(),
        5 => "Continuous".to_string(),
        6 => "Manual Focus (6)".to_string(),
        16 => "Pan Focus".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon metering mode to human-readable string
fn decode_metering_mode(value: i16) -> String {
    match value {
        3 => "Evaluative".to_string(),
        4 => "Partial".to_string(),
        5 => "Center-weighted Average".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Canon exposure mode to human-readable string
fn decode_exposure_mode(value: i16) -> String {
    match value {
        0 => "Easy".to_string(),
        1 => "Program AE".to_string(),
        2 => "Shutter Priority".to_string(),
        3 => "Aperture Priority".to_string(),
        4 => "Manual".to_string(),
        5 => "Depth-of-field AE".to_string(),
        6 => "M-Dep".to_string(),
        7 => "Bulb".to_string(),
        _ => format!("Unknown ({})", value),
    }
}
```

### Step 4: Run test to verify it passes

Run: `cargo test test_decode_ --lib -- --nocapture`
Expected: PASS (7 tests)

### Step 5: Commit

```bash
git add src/parsers/tiff/makernotes/canon.rs
git commit -m "feat(canon): add CameraSettings value decoders

Add decoder functions to convert Canon numeric values to human-readable strings.
Covers macro mode, quality, flash mode, drive mode, focus mode, metering mode, and exposure mode.

Based on ExifTool Canon.pm lookup tables.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 4: Integrate CameraSettings Array Parsing

**Files:**
- Modify: `src/parsers/tiff/makernotes/canon.rs` (update parse_canon_makernote function)
- Test: Within `src/parsers/tiff/makernotes/canon.rs` (#[cfg(test)] module)

### Step 1: Write integration test for CameraSettings parsing

Add to the `#[cfg(test)]` module:

```rust
#[test]
fn test_parse_camera_settings_array() {
    // Create Canon MakerNote with CameraSettings array
    let mut data = Vec::new();

    // Canon signature
    data.extend_from_slice(b"Canon");

    // IFD: 1 entry (CameraSettings)
    data.extend_from_slice(&[0x01, 0x00]); // Entry count (LE)

    // IFD Entry for CameraSettings (tag 0x0001)
    data.extend_from_slice(&[0x01, 0x00]); // Tag: CameraSettings
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x15, 0x00, 0x00, 0x00]); // Count: 21 values
    data.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]); // Offset: 20 (after IFD)

    // Next IFD offset
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // CameraSettings array data at offset 20 (21 i16 values)
    let settings: Vec<i16> = vec![
        21,  // [0] Array length
        2,   // [1] Macro mode: Normal
        0,   // [2] Self-timer: Off
        3,   // [3] Quality: Fine
        2,   // [4] Flash mode: On
        0,   // [5] Drive mode: Single
        0,   // [6] (unused)
        0,   // [7] Focus mode: One-shot AF
        0,   // [8] (unused)
        0,   // [9] (unused)
        1,   // [10] Image size: Large
        0,   // [11] Easy mode: Off
        0,   // [12] (unused)
        0,   // [13] Contrast: Normal
        0,   // [14] Saturation: Normal
        0,   // [15] Sharpness: Normal
        80,  // [16] ISO: 80
        3,   // [17] Metering mode: Evaluative
        0,   // [18] Focus type
        0,   // [19] AF point
        1,   // [20] Exposure mode: Program AE
    ];

    for value in settings {
        data.extend_from_slice(&value.to_le_bytes());
    }

    let result = parse_canon_makernote(&data, ByteOrder::LittleEndian).unwrap();

    // Verify extracted values
    assert_eq!(result.get("Canon:MacroMode"), Some(&"Normal".to_string()));
    assert_eq!(result.get("Canon:Quality"), Some(&"Fine".to_string()));
    assert_eq!(result.get("Canon:FlashMode"), Some(&"On".to_string()));
    assert_eq!(result.get("Canon:DriveMode"), Some(&"Single".to_string()));
    assert_eq!(result.get("Canon:FocusMode"), Some(&"One-shot AF".to_string()));
    assert_eq!(result.get("Canon:MeteringMode"), Some(&"Evaluative".to_string()));
    assert_eq!(result.get("Canon:ExposureMode"), Some(&"Program AE".to_string()));
    assert_eq!(result.get("Canon:ISO"), Some(&"80".to_string()));
}
```

### Step 2: Run test to verify it fails

Run: `cargo test test_parse_camera_settings_array --lib -- --nocapture`
Expected: FAIL - CameraSettings tags not found (parser still skips array tags)

### Step 3: Update parse_canon_makernote to handle CameraSettings

Modify the `parse_canon_makernote` function around line 240, replace the Phase 1 tag matching with:

```rust
// Parse IFD entries and extract values
match parse_ifd_entries(ifd_data, byte_order) {
    Ok(entries) => {
        for entry in entries {
            match entry.tag {
                // Simple string tags (Phase 1)
                CANON_IMAGE_TYPE | CANON_FIRMWARE_VERSION |
                CANON_OWNER_NAME | CANON_SERIAL_NUMBER => {
                    if let Some(value) = extract_string_value(&entry, ifd_data) {
                        let tag_name = canon_tag_to_name(entry.tag);
                        tags.insert(tag_name, value);
                    }
                }

                // Simple integer tags (Phase 1)
                CANON_MODEL_ID | CANON_FILE_NUMBER => {
                    if let Some(value) = extract_integer_value(&entry) {
                        let tag_name = canon_tag_to_name(entry.tag);
                        tags.insert(tag_name, value);
                    }
                }

                // CameraSettings array (Phase 2)
                CANON_CAMERA_SETTINGS => {
                    if let Some(array) = extract_i16_array(&entry, ifd_data, byte_order) {
                        // Extract specific settings from array
                        if array.len() > CAMERA_SETTINGS_MACRO_MODE {
                            tags.insert(
                                "Canon:MacroMode".to_string(),
                                decode_macro_mode(array[CAMERA_SETTINGS_MACRO_MODE]),
                            );
                        }
                        if array.len() > CAMERA_SETTINGS_QUALITY {
                            tags.insert(
                                "Canon:Quality".to_string(),
                                decode_quality(array[CAMERA_SETTINGS_QUALITY]),
                            );
                        }
                        if array.len() > CAMERA_SETTINGS_FLASH_MODE {
                            tags.insert(
                                "Canon:FlashMode".to_string(),
                                decode_flash_mode(array[CAMERA_SETTINGS_FLASH_MODE]),
                            );
                        }
                        if array.len() > CAMERA_SETTINGS_DRIVE_MODE {
                            tags.insert(
                                "Canon:DriveMode".to_string(),
                                decode_drive_mode(array[CAMERA_SETTINGS_DRIVE_MODE]),
                            );
                        }
                        if array.len() > CAMERA_SETTINGS_FOCUS_MODE {
                            tags.insert(
                                "Canon:FocusMode".to_string(),
                                decode_focus_mode(array[CAMERA_SETTINGS_FOCUS_MODE]),
                            );
                        }
                        if array.len() > CAMERA_SETTINGS_ISO {
                            tags.insert(
                                "Canon:ISO".to_string(),
                                array[CAMERA_SETTINGS_ISO].to_string(),
                            );
                        }
                        if array.len() > CAMERA_SETTINGS_METERING_MODE {
                            tags.insert(
                                "Canon:MeteringMode".to_string(),
                                decode_metering_mode(array[CAMERA_SETTINGS_METERING_MODE]),
                            );
                        }
                        if array.len() > CAMERA_SETTINGS_EXPOSURE_MODE {
                            tags.insert(
                                "Canon:ExposureMode".to_string(),
                                decode_exposure_mode(array[CAMERA_SETTINGS_EXPOSURE_MODE]),
                            );
                        }
                    }
                }

                // Other array tags - skip for now (will add in subsequent tasks)
                _ => continue,
            }
        }
    }
    Err(_) => {}
}
```

### Step 4: Run test to verify it passes

Run: `cargo test test_parse_camera_settings_array --lib -- --nocapture`
Expected: PASS

### Step 5: Run all Canon tests

Run: `cargo test canon --lib -- --nocapture`
Expected: All Canon tests pass (13+ tests including Phase 1)

### Step 6: Commit

```bash
git add src/parsers/tiff/makernotes/canon.rs
git commit -m "feat(canon): parse CameraSettings array tag

Extract and decode CameraSettings array values:
- MacroMode, Quality, FlashMode, DriveMode
- FocusMode, ISO, MeteringMode, ExposureMode

Phase 2: Complex array parsing now active.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 5: Add ShotInfo Tag Support

**Files:**
- Modify: `src/parsers/tiff/makernotes/canon.rs` (add ShotInfo constants and parsing)
- Test: Within `src/parsers/tiff/makernotes/canon.rs` (#[cfg(test)] module)

### Step 1: Write test for ShotInfo array indices

Add to the `#[cfg(test)]` module:

```rust
#[test]
fn test_shot_info_indices() {
    assert_eq!(SHOT_INFO_AUTO_ISO, 1);
    assert_eq!(SHOT_INFO_BASE_ISO, 2);
    assert_eq!(SHOT_INFO_MEASURED_EV, 3);
    assert_eq!(SHOT_INFO_TARGET_APERTURE, 4);
    assert_eq!(SHOT_INFO_TARGET_SHUTTER_SPEED, 5);
    assert_eq!(SHOT_INFO_WHITE_BALANCE, 7);
    assert_eq!(SHOT_INFO_SLOW_SHUTTER, 8);
    assert_eq!(SHOT_INFO_SEQUENCE_NUMBER, 9);
    assert_eq!(SHOT_INFO_FLASH_GUIDE_NUMBER, 13);
    assert_eq!(SHOT_INFO_AF_POINTS_USED, 14);
    assert_eq!(SHOT_INFO_FLASH_EXPOSURE_COMP, 15);
    assert_eq!(SHOT_INFO_AUTO_EXPOSURE_BRACKETING, 16);
    assert_eq!(SHOT_INFO_SUBJECT_DISTANCE, 19);
}

#[test]
fn test_parse_shot_info_array() {
    let mut data = Vec::new();
    data.extend_from_slice(b"Canon");
    data.extend_from_slice(&[0x01, 0x00]); // 1 entry

    // ShotInfo tag (0x0004)
    data.extend_from_slice(&[0x04, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]); // Count: 20
    data.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]); // Offset: 20
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    // ShotInfo array (20 values)
    let shot_info: Vec<i16> = vec![
        20,   // [0] Array length
        100,  // [1] Auto ISO
        100,  // [2] Base ISO
        128,  // [3] Measured EV
        160,  // [4] Target aperture (f/5.6)
        96,   // [5] Target shutter speed (1/60)
        0,    // [6] (unused)
        0,    // [7] White balance: Auto
        0,    // [8] Slow shutter: Off
        0,    // [9] Sequence number
        0, 0, 0, 0, // [10-13]
        0,    // [14] AF points used
        0,    // [15] Flash exposure comp
        0,    // [16] Auto exposure bracketing
        0, 0, // [17-18]
        1000, // [19] Subject distance (mm)
    ];

    for value in shot_info {
        data.extend_from_slice(&value.to_le_bytes());
    }

    let result = parse_canon_makernote(&data, ByteOrder::LittleEndian).unwrap();

    assert_eq!(result.get("Canon:AutoISO"), Some(&"100".to_string()));
    assert_eq!(result.get("Canon:BaseISO"), Some(&"100".to_string()));
    assert_eq!(result.get("Canon:MeasuredEV"), Some(&"128".to_string()));
    assert_eq!(result.get("Canon:TargetAperture"), Some(&"160".to_string()));
    assert_eq!(result.get("Canon:TargetShutterSpeed"), Some(&"96".to_string()));
    assert_eq!(result.get("Canon:SubjectDistance"), Some(&"1000 mm".to_string()));
}
```

### Step 2: Run test to verify it fails

Run: `cargo test test_shot_info --lib -- --nocapture`
Expected: FAIL - Constants not defined, ShotInfo not parsed

### Step 3: Add ShotInfo constants

Add after CameraSettings constants:

```rust
// ShotInfo array (tag 0x0004) indices
const SHOT_INFO_AUTO_ISO: usize = 1;
const SHOT_INFO_BASE_ISO: usize = 2;
const SHOT_INFO_MEASURED_EV: usize = 3;
const SHOT_INFO_TARGET_APERTURE: usize = 4;
const SHOT_INFO_TARGET_SHUTTER_SPEED: usize = 5;
const SHOT_INFO_WHITE_BALANCE: usize = 7;
const SHOT_INFO_SLOW_SHUTTER: usize = 8;
const SHOT_INFO_SEQUENCE_NUMBER: usize = 9;
const SHOT_INFO_FLASH_GUIDE_NUMBER: usize = 13;
const SHOT_INFO_AF_POINTS_USED: usize = 14;
const SHOT_INFO_FLASH_EXPOSURE_COMP: usize = 15;
const SHOT_INFO_AUTO_EXPOSURE_BRACKETING: usize = 16;
const SHOT_INFO_SUBJECT_DISTANCE: usize = 19;
```

### Step 4: Add ShotInfo parsing to parse_canon_makernote

Add after the CameraSettings case in the match statement:

```rust
// ShotInfo array (Phase 2)
CANON_SHOT_INFO => {
    if let Some(array) = extract_i16_array(&entry, ifd_data, byte_order) {
        if array.len() > SHOT_INFO_AUTO_ISO {
            tags.insert(
                "Canon:AutoISO".to_string(),
                array[SHOT_INFO_AUTO_ISO].to_string(),
            );
        }
        if array.len() > SHOT_INFO_BASE_ISO {
            tags.insert(
                "Canon:BaseISO".to_string(),
                array[SHOT_INFO_BASE_ISO].to_string(),
            );
        }
        if array.len() > SHOT_INFO_MEASURED_EV {
            tags.insert(
                "Canon:MeasuredEV".to_string(),
                array[SHOT_INFO_MEASURED_EV].to_string(),
            );
        }
        if array.len() > SHOT_INFO_TARGET_APERTURE {
            tags.insert(
                "Canon:TargetAperture".to_string(),
                array[SHOT_INFO_TARGET_APERTURE].to_string(),
            );
        }
        if array.len() > SHOT_INFO_TARGET_SHUTTER_SPEED {
            tags.insert(
                "Canon:TargetShutterSpeed".to_string(),
                array[SHOT_INFO_TARGET_SHUTTER_SPEED].to_string(),
            );
        }
        if array.len() > SHOT_INFO_SUBJECT_DISTANCE {
            let distance = array[SHOT_INFO_SUBJECT_DISTANCE];
            tags.insert(
                "Canon:SubjectDistance".to_string(),
                format!("{} mm", distance),
            );
        }
    }
}
```

### Step 5: Run test to verify it passes

Run: `cargo test test_shot_info --lib -- --nocapture`
Expected: PASS (2 tests)

### Step 6: Commit

```bash
git add src/parsers/tiff/makernotes/canon.rs
git commit -m "feat(canon): add ShotInfo array parsing

Extract shooting information from Canon ShotInfo tag (0x0004):
- AutoISO, BaseISO, MeasuredEV
- TargetAperture, TargetShutterSpeed, SubjectDistance

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 6: Add FocalLength Array Support

**Files:**
- Modify: `src/parsers/tiff/makernotes/canon.rs`
- Test: Within `src/parsers/tiff/makernotes/canon.rs` (#[cfg(test)] module)

### Step 1: Write test for FocalLength parsing

Add to the `#[cfg(test)]` module:

```rust
#[test]
fn test_parse_focal_length_array() {
    let mut data = Vec::new();
    data.extend_from_slice(b"Canon");
    data.extend_from_slice(&[0x01, 0x00]); // 1 entry

    // FocalLength tag (0x0002)
    data.extend_from_slice(&[0x02, 0x00]); // Tag
    data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
    data.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]); // Offset: 20
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    // FocalLength array: [focal_type, focal_length, focal_plane_x_size, focal_plane_y_size]
    // focal_type: 2 (35mm equivalent available)
    // focal_length: 50mm (stored as 50)
    // focal_units: typically stored separately
    let focal_data: Vec<i16> = vec![2, 50, 0, 0];

    for value in focal_data {
        data.extend_from_slice(&value.to_le_bytes());
    }

    let result = parse_canon_makernote(&data, ByteOrder::LittleEndian).unwrap();

    assert_eq!(result.get("Canon:FocalType"), Some(&"2".to_string()));
    assert_eq!(result.get("Canon:FocalLength"), Some(&"50 mm".to_string()));
}
```

### Step 2: Run test to verify it fails

Run: `cargo test test_parse_focal_length_array --lib -- --nocapture`
Expected: FAIL - FocalLength not parsed

### Step 3: Add FocalLength array parsing

Add to the match statement in `parse_canon_makernote`:

```rust
// FocalLength array (Phase 2)
CANON_FOCAL_LENGTH => {
    if let Some(array) = extract_i16_array(&entry, ifd_data, byte_order) {
        // array[0] = focal type
        // array[1] = focal length
        if array.len() > 0 {
            tags.insert(
                "Canon:FocalType".to_string(),
                array[0].to_string(),
            );
        }
        if array.len() > 1 {
            tags.insert(
                "Canon:FocalLength".to_string(),
                format!("{} mm", array[1]),
            );
        }
    }
}
```

### Step 4: Run test to verify it passes

Run: `cargo test test_parse_focal_length_array --lib -- --nocapture`
Expected: PASS

### Step 5: Run all Canon tests

Run: `cargo test canon --lib -- --nocapture`
Expected: All tests pass (15+ tests)

### Step 6: Commit

```bash
git add src/parsers/tiff/makernotes/canon.rs
git commit -m "feat(canon): add FocalLength array parsing

Extract focal length information from tag 0x0002.
Reports focal type and focal length in mm.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 7: Update Integration Tests

**Files:**
- Modify: `tests/integration/exif_makernotes_tests.rs`
- Modify: `tests/integration/canon_real_image_test.rs`

### Step 1: Update integration test to verify Phase 2 tags

Modify `tests/integration/exif_makernotes_tests.rs`, update the `test_canon_makernote_extraction` test to include CameraSettings:

```rust
#[test]
fn test_canon_makernote_with_camera_settings() {
    let mut tiff_data = Vec::new();

    // === TIFF Header (8 bytes) ===
    tiff_data.extend_from_slice(&[0x49, 0x49]); // "II" (little-endian)
    tiff_data.extend_from_slice(&[0x2A, 0x00]); // Magic number 42
    tiff_data.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]); // Offset to IFD0

    // === IFD0 (2 + 12*1 + 4 = 18 bytes) ===
    tiff_data.extend_from_slice(&[0x01, 0x00]); // 1 entry

    // MakerNote tag (0x927C)
    tiff_data.extend_from_slice(&[0x7C, 0x92]); // Tag
    tiff_data.extend_from_slice(&[0x07, 0x00]); // Type: UNDEFINED
    let makernote_size = 200; // Approximate size
    tiff_data.extend_from_slice(&(makernote_size as u32).to_le_bytes()); // Count
    tiff_data.extend_from_slice(&[0x1A, 0x00, 0x00, 0x00]); // Offset to MakerNote

    tiff_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    // === Canon MakerNote at offset 0x1A (26) ===
    tiff_data.extend_from_slice(b"Canon"); // Signature

    // Canon IFD: 2 entries (ImageType + CameraSettings)
    tiff_data.extend_from_slice(&[0x02, 0x00]);

    // Entry 1: ImageType (0x0006)
    tiff_data.extend_from_slice(&[0x06, 0x00]); // Tag
    tiff_data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
    tiff_data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
    tiff_data.extend_from_slice(b"IMG:"); // Inline value

    // Entry 2: CameraSettings (0x0001)
    tiff_data.extend_from_slice(&[0x01, 0x00]); // Tag
    tiff_data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
    tiff_data.extend_from_slice(&[0x0A, 0x00, 0x00, 0x00]); // Count: 10
    tiff_data.extend_from_slice(&[0x2E, 0x00, 0x00, 0x00]); // Offset (relative to Canon IFD start)

    tiff_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

    // CameraSettings array at offset 0x2E (46 from Canon IFD start, 51 absolute)
    let settings: Vec<i16> = vec![
        10,  // [0] Length
        2,   // [1] Macro: Normal
        0,   // [2] Self-timer
        3,   // [3] Quality: Fine
        2,   // [4] Flash: On
        0,   // [5] Drive: Single
        0,   // [6]
        0,   // [7] Focus: One-shot
        0,   // [8]
        0,   // [9]
    ];

    for value in settings {
        tiff_data.extend_from_slice(&value.to_le_bytes());
    }

    // Write to temp file and test
    use std::fs;
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(&tiff_data).unwrap();

    let metadata = read_metadata(temp_file.path()).expect("Failed to read test TIFF");

    // Verify Phase 1 tags still work
    assert!(metadata.contains_key("Canon:ImageType"));

    // Verify Phase 2 tags are extracted
    assert_eq!(metadata.get("Canon:MacroMode"), Some(&"Normal".to_string()));
    assert_eq!(metadata.get("Canon:Quality"), Some(&"Fine".to_string()));
    assert_eq!(metadata.get("Canon:FlashMode"), Some(&"On".to_string()));
    assert_eq!(metadata.get("Canon:DriveMode"), Some(&"Single".to_string()));

    println!("✅ Phase 2: CameraSettings array successfully parsed!");
}
```

### Step 2: Run integration test

Run: `cargo test test_canon_makernote_with_camera_settings -- --nocapture`
Expected: PASS

### Step 3: Update real-world test documentation

Modify `tests/integration/canon_real_image_test.rs` to note Phase 2 tags in the expected output:

```rust
//! # Example Canon Tags (Phase 2 includes array tags)
//!
//! - `Canon:CanonModelID` - e.g., "0x80000001"
//! - `Canon:FirmwareVersion` - e.g., "Firmware Version 1.0.0"
//! - `Canon:OwnerName` - e.g., "John Doe"
//! - `Canon:SerialNumber` - e.g., "012345678901"
//! - `Canon:ImageType` - e.g., "IMG:EOS R5"
//! - `Canon:FileNumber` - e.g., "1234567"
//! - `Canon:MacroMode` - e.g., "Normal" (Phase 2)
//! - `Canon:Quality` - e.g., "Fine" (Phase 2)
//! - `Canon:FlashMode` - e.g., "Off" (Phase 2)
//! - `Canon:DriveMode` - e.g., "Single" (Phase 2)
//! - `Canon:FocusMode` - e.g., "One-shot AF" (Phase 2)
//! - `Canon:MeteringMode` - e.g., "Evaluative" (Phase 2)
//! - `Canon:ExposureMode` - e.g., "Manual" (Phase 2)
//! - `Canon:ISO` - e.g., "100" (Phase 2)
//! - `Canon:AutoISO` - e.g., "100" (Phase 2)
//! - `Canon:BaseISO` - e.g., "100" (Phase 2)
//! - `Canon:FocalLength` - e.g., "50 mm" (Phase 2)
```

### Step 4: Commit

```bash
git add tests/integration/exif_makernotes_tests.rs tests/integration/canon_real_image_test.rs
git commit -m "test(canon): add Phase 2 integration tests

Add test for CameraSettings array parsing in integration tests.
Update real-world test documentation with Phase 2 tags.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 8: Update Documentation

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `docs/IMPLEMENTATION_ROADMAP.md`

### Step 1: Update README.md with Phase 2 tags

Find the Canon MakerNotes section and update:

```markdown
### Canon MakerNotes

**Phase 1 (Simple Tags):**
- Canon:ImageType, Canon:FirmwareVersion, Canon:OwnerName, Canon:SerialNumber
- Canon:CanonModelID, Canon:FileNumber

**Phase 2 (Array Tags):**
- **CameraSettings:** MacroMode, Quality, FlashMode, DriveMode, FocusMode, ISO, MeteringMode, ExposureMode
- **ShotInfo:** AutoISO, BaseISO, MeasuredEV, TargetAperture, TargetShutterSpeed, SubjectDistance
- **FocalLength:** FocalType, FocalLength

Example:
```bash
oxidex -Canon:Quality -Canon:ExposureMode -Canon:ISO canon_photo.jpg
```
```

### Step 2: Update CHANGELOG.md

Add Phase 2 entry under `[Unreleased]`:

```markdown
### Added

#### Canon MakerNotes Phase 2 - Complex Array Tag Support

Extended Canon MakerNote parser to decode complex array tags with camera settings:

**CameraSettings Array (tag 0x0001):**
- `Canon:MacroMode` - Macro mode setting (Macro, Normal)
- `Canon:Quality` - Image quality (Economy, Normal, Fine, Superfine, RAW, CRAW)
- `Canon:FlashMode` - Flash mode (Off, Auto, On, Red-eye Reduction, etc.)
- `Canon:DriveMode` - Drive mode (Single, Continuous, Movie, etc.)
- `Canon:FocusMode` - Focus mode (One-shot AF, AI Servo AF, Manual, etc.)
- `Canon:ISO` - ISO speed setting
- `Canon:MeteringMode` - Metering mode (Evaluative, Partial, Center-weighted)
- `Canon:ExposureMode` - Exposure mode (Program, Av, Tv, Manual, Bulb, etc.)

**ShotInfo Array (tag 0x0004):**
- `Canon:AutoISO` - Auto ISO value
- `Canon:BaseISO` - Base ISO value
- `Canon:MeasuredEV` - Measured exposure value
- `Canon:TargetAperture` - Target aperture value
- `Canon:TargetShutterSpeed` - Target shutter speed
- `Canon:SubjectDistance` - Subject distance in mm

**FocalLength Array (tag 0x0002):**
- `Canon:FocalType` - Focal length type
- `Canon:FocalLength` - Focal length in mm

**Implementation details:**
- Added i16 array extraction with inline/offset handling
- Value decoders convert numeric codes to human-readable strings
- Based on ExifTool Canon.pm reference implementation
- All values gracefully handle missing/invalid data

**Files modified:**
- `src/parsers/tiff/makernotes/canon.rs` (+250 lines)
- `tests/integration/exif_makernotes_tests.rs` (updated)
- `tests/integration/canon_real_image_test.rs` (updated)
```

### Step 3: Update IMPLEMENTATION_ROADMAP.md

Mark Phase 2 complete:

```markdown
### Canon MakerNotes

**Status: Phase 2 Complete ✅**

- Phase 1: Simple tags ✅ (ImageType, FirmwareVersion, etc.)
- Phase 2: Array tags ✅ (CameraSettings, ShotInfo, FocalLength)
- Phase 3 (Future): Lens database, AFInfo, FileInfo, additional camera-specific arrays
```

### Step 4: Run documentation build

Run: `cargo doc --no-deps --all-features`
Expected: Success, no warnings

### Step 5: Commit

```bash
git add README.md CHANGELOG.md docs/IMPLEMENTATION_ROADMAP.md
git commit -m "docs: update documentation for Canon MakerNotes Phase 2

Document Phase 2 array tag support in README, CHANGELOG, and roadmap.
List all new tags with descriptions.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 9: Final Verification

**Files:**
- None (verification only)

### Step 1: Run full test suite

Run: `cargo test --all-features`
Expected: All tests pass (420+ tests including new Phase 2 tests)

### Step 2: Run linting

Run: `cargo clippy --all-features -- -D warnings`
Expected: No warnings

### Step 3: Run formatting check

Run: `cargo fmt --check`
Expected: All files properly formatted

### Step 4: Verify Canon-specific tests

Run: `cargo test canon --lib -- --nocapture`
Expected: 18+ Canon tests pass (Phase 1 + Phase 2)

### Step 5: Verify integration tests

Run: `cargo test test_canon --all -- --nocapture`
Expected: All Canon integration tests pass

### Step 6: Manual verification checklist

Verify:
- ✅ All 9 tasks completed
- ✅ Array extraction infrastructure added
- ✅ CameraSettings array parsing implemented
- ✅ ShotInfo array parsing implemented
- ✅ FocalLength array parsing implemented
- ✅ Integration tests updated
- ✅ Documentation updated
- ✅ All tests passing
- ✅ No clippy warnings
- ✅ Phase 2 complete

### Step 7: Create summary report

Document:
- 15+ new Phase 2 tags supported
- CameraSettings: 8 decoded values
- ShotInfo: 6 decoded values
- FocalLength: 2 decoded values
- ~250 lines of new code
- 18+ tests passing

---

## Success Criteria

**Phase 2 is complete when:**
1. ✅ All array extraction infrastructure works for i16 arrays
2. ✅ CameraSettings array tags are decoded and human-readable
3. ✅ ShotInfo array tags are extracted
4. ✅ FocalLength array tags are extracted
5. ✅ All tests pass (420+ total, 18+ Canon-specific)
6. ✅ No clippy warnings
7. ✅ Documentation updated (README, CHANGELOG, roadmap)
8. ✅ Integration tests verify Phase 2 tags work end-to-end

**Phase 3 Future Work:**
- Lens database (400+ Canon lens IDs)
- AFInfo array decoding
- FileInfo array decoding
- ColorInfo array decoding
- Additional camera-specific settings
- Support for newer Canon models (R3, R5, R6, etc.)

---

## Execution Handoff

Plan complete and saved to `docs/plans/2025-11-15-canon-makernotes-phase2.md`.

**Two execution options:**

**A. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration with quality gates

**B. Parallel Session (separate)** - Open new session with executing-plans skill, batch execution with review checkpoints

**Which approach?**
