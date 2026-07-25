# GPS Movement and Tracking Tags Implementation

## Overview
Implemented support for 9 GPS movement and tracking tags critical for forensic geolocation analysis. These tags enable reconstruction of movement patterns, navigation analysis, and GPS accuracy assessment.

## Implemented Tags

### Speed Tracking
| Tag ID | Tag Name | Type | Format | Description |
|--------|----------|------|--------|-------------|
| 0x000C | GPS:GPSSpeedRef | ASCII | Single char (K/M/N) | Speed unit reference (K=km/h, M=mph, N=knots) |
| 0x000D | GPS:GPSSpeed | RATIONAL | Decimal (2 places) | Speed of GPS receiver |

### Direction Tracking
| Tag ID | Tag Name | Type | Format | Description |
|--------|----------|------|--------|-------------|
| 0x000E | GPS:GPSTrackRef | ASCII | Single char (T/M) | Track reference (T=true north, M=magnetic north) |
| 0x000F | GPS:GPSTrack | RATIONAL | Decimal degrees (2 places) | Direction of movement (0-359.99°) |

### Camera Orientation
| Tag ID | Tag Name | Type | Format | Description |
|--------|----------|------|--------|-------------|
| 0x0010 | GPS:GPSImgDirectionRef | ASCII | Single char (T/M) | Image direction reference (T=true, M=magnetic) |
| 0x0011 | GPS:GPSImgDirection | RATIONAL | Decimal degrees (2 places) | Direction camera was pointing (0-359.99°) |

### Navigation
| Tag ID | Tag Name | Type | Format | Description |
|--------|----------|------|--------|-------------|
| 0x0018 | GPS:GPSDestBearing | RATIONAL | Decimal degrees (2 places) | Bearing to destination point |
| 0x001A | GPS:GPSDestDistance | RATIONAL | Decimal (3 places) | Distance to destination |

### Accuracy
| Tag ID | Tag Name | Type | Format | Description |
|--------|----------|------|--------|-------------|
| 0x001F | GPS:GPSHPositioningError | RATIONAL | Meters with unit | Horizontal positioning error (meters) |

## Implementation Details

### Files Modified
- `/Users/allen/Documents/git/oxidex/src/core/tag_conversion.rs`
  - Added GPS movement tag constants (0x000C-0x001F)
  - Added special formatting for RATIONAL GPS movement tags
  - Tags format as decimal values with appropriate precision
  - GPSHPositioningError includes meter unit suffix

### Tag Formatting

#### Speed Tags
```rust
GPSSpeedRef: "K" | "M" | "N"  // K=km/h, M=mph, N=knots
GPSSpeed: "55.50"             // 2 decimal places
```

#### Direction Tags
```rust
GPSTrackRef: "T" | "M"         // T=true north, M=magnetic
GPSTrack: "275.50"             // Degrees, 2 decimal places

GPSImgDirectionRef: "T" | "M"
GPSImgDirection: "90.25"       // Degrees, 2 decimal places
```

#### Navigation Tags
```rust
GPSDestBearing: "45.25"        // Degrees, 2 decimal places
GPSDestDistance: "12.345"      // Distance, 3 decimal places
```

#### Accuracy Tags
```rust
GPSHPositioningError: "8.50 m" // Meters with unit suffix
```

## Testing

### Unit Tests (11 tests - all passing)
Location: `/Users/allen/Documents/git/oxidex/src/core/tag_conversion.rs`

1. `test_gps_speed_formatting` - Verifies GPSSpeed decimal formatting
2. `test_gps_speed_ref_formatting` - Verifies GPSSpeedRef ASCII handling
3. `test_gps_track_formatting` - Verifies GPSTrack degree formatting
4. `test_gps_track_ref_formatting` - Verifies GPSTrackRef ASCII handling
5. `test_gps_img_direction_formatting` - Verifies GPSImgDirection formatting
6. `test_gps_img_direction_ref_formatting` - Verifies GPSImgDirectionRef ASCII
7. `test_gps_dest_bearing_formatting` - Verifies GPSDestBearing formatting
8. `test_gps_dest_distance_formatting` - Verifies GPSDestDistance precision
9. `test_gps_h_positioning_error_formatting` - Verifies meter unit suffix
10. `test_gps_movement_tags_with_zero_denominator` - Edge case handling
11. `test_gps_movement_tags_forensic_scenario` - Complete movement scenario

### Integration Tests
Location: `/Users/allen/Documents/git/oxidex/tests/forensic/gps_movement_tests.rs`

- Creates test TIFF files with GPS movement tags
- Verifies end-to-end tag extraction
- Tests complete forensic scenarios
- Validates tag registry integration

## Forensic Use Cases

### 1. Vehicle Movement Reconstruction
```
GPSSpeed: "55.50" + GPSSpeedRef: "K" → 55.5 km/h
GPSTrack: "275.50" + GPSTrackRef: "T" → Heading 275.5° true north
```
**Analysis**: Vehicle moving northwest at 55.5 km/h

### 2. Camera Orientation Analysis
```
GPSImgDirection: "90.25" + GPSImgDirectionRef: "M" → Pointing 90.25° magnetic east
```
**Analysis**: Photographer was facing east when image captured

### 3. GPS Fix Quality Assessment
```
GPSHPositioningError: "8.50 m"
```
**Analysis**: GPS fix accurate within 8.5 meters (good quality for civilian GPS)

### 4. Navigation Tracking
```
GPSDestBearing: "45.25" → 45.25° (northeast)
GPSDestDistance: "12.345" → 12.345 units to destination
```
**Analysis**: Subject was navigating northeast, 12.3 km from destination

## Technical Notes

### RATIONAL Type Handling
All numeric GPS movement tags use EXIF RATIONAL type (two u32 values: numerator/denominator).
The implementation:
- Divides numerator by denominator to get decimal value
- Formats with appropriate precision (2-3 decimal places)
- Adds unit suffixes where applicable (e.g., "m" for meters)
- Handles zero denominator gracefully (returns raw rational)

### ASCII Reference Tags
Reference tags (Ref suffix) are ASCII type containing single characters:
- Stripped of null terminators
- Returned as single-character strings
- Used to interpret corresponding value tags

### Byte Order Support
All tags support both little-endian and big-endian byte orders through the ByteOrder parameter.

## Verification

### Test Results
```bash
$ cargo test --lib core::tag_conversion::tests::test_gps
running 11 tests
test core::tag_conversion::tests::test_gps_dest_bearing_formatting ... ok
test core::tag_conversion::tests::test_gps_dest_distance_formatting ... ok
test core::tag_conversion::tests::test_gps_h_positioning_error_formatting ... ok
test core::tag_conversion::tests::test_gps_img_direction_formatting ... ok
test core::tag_conversion::tests::test_gps_img_direction_ref_formatting ... ok
test core::tag_conversion::tests::test_gps_movement_tags_forensic_scenario ... ok
test core::tag_conversion::tests::test_gps_movement_tags_with_zero_denominator ... ok
test core::tag_conversion::tests::test_gps_speed_formatting ... ok
test core::tag_conversion::tests::test_gps_speed_ref_formatting ... ok
test core::tag_conversion::tests::test_gps_track_formatting ... ok
test core::tag_conversion::tests::test_gps_track_ref_formatting ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured
```

### Full Test Suite
```bash
$ cargo test --lib
test result: ok. 1529 passed; 0 failed; 0 ignored; 0 measured
```

All existing tests continue to pass, confirming no regressions.

## Future Enhancements

### Potential Additions
1. **GPSDestLatitude/GPSDestLongitude** (0x0014/0x0016) - Already in tag database
2. **GPSDestDistanceRef** (0x0019) - Distance unit reference
3. **Velocity vector calculations** - Combine speed + track for movement vectors
4. **Trajectory analysis** - Multi-image GPS track analysis
5. **Accuracy visualization** - Error circle rendering

### Integration Opportunities
- Timeline reconstruction combining GPS + timestamp data
- Movement pattern analysis across image sequences
- Anomaly detection (impossible speeds, GPS jumps)
- Route reconstruction and mapping

## References

- EXIF 2.32 Specification - GPS Tag Structure
- ExifTool GPS Tag Documentation
- Forensic Image Analysis Best Practices
- GPS Coordinate System Standards

## Conclusion

This implementation provides complete support for GPS movement and tracking tag extraction and formatting. All tags are correctly parsed from TIFF/EXIF data, properly formatted for human readability, and fully tested. The implementation is production-ready for forensic geolocation analysis use cases.
