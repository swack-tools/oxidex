//! Phase One tag registry
//!
//! Tag-ID -> display-name tables for `%Image::ExifTool::PhaseOne::Main` and
//! `%Image::ExifTool::PhaseOne::SensorCalibration`, verified line-by-line
//! against
//! `/opt/homebrew/Cellar/exiftool/13.55/libexec/lib/perl5/Image/ExifTool/PhaseOne.pm`
//! (byte-identical to the 13.59 checkout cached under
//! `/tmp/oxidex-exiftool-cache/exiftool`).
//!
//! The table this replaced invented eight entries wholesale -- three "lens"
//! tags that don't exist in `PhaseOne::Main` at all (0x0213/0x0214/0x0215),
//! and five real tag IDs relabeled with the name ExifTool assigns to a
//! *different* ID:
//!
//! | ID     | was registered as   | actually is (PhaseOne.pm)          |
//! |--------|----------------------|-------------------------------------|
//! | 0x0211 | `LensID`             | `SensorTemperature2` (float, :123)  |
//! | 0x0212 | `LensModel`          | `UnknownDate` (Unknown-flagged, :129) |
//! | 0x0401 | `ISO`                | `ApertureValue` (:223)              |
//! | 0x0402 | `ShutterSpeed`       | `ExposureCompensation` (:231)       |
//! | 0x0403 | `Aperture`           | `FocalLength` (:237)                |
//! | 0x0412 | `WhiteBalance`       | `LensModel` (string, :250)          |
//! | 0x0601 | `SensorTemperature`  | not a `PhaseOne::Main` tag; the real |
//! |        |                      | `SensorTemperature` is 0x0210        |
//!
//! Tags ExifTool marks `Unknown` (hidden from default, non-`-u`/`-U` output)
//! are intentionally absent from [`phaseone_tag_name`]: 0x0101, 0x0103,
//! 0x0104, 0x0212 (`UnknownDate`), 0x0213, 0x0215, 0x021a, 0x021e, 0x0220,
//! 0x0221, 0x0224, 0x0225, 0x0227, 0x0228, 0x0229, 0x022b, 0x0242, 0x0244,
//! 0x0245, 0x0258, 0x025a, 0x0300, 0x0304, 0x0404-0x0409, 0x0411, 0x0416,
//! 0x0417. The bare-comment IDs (`# 0x0101 - ...`) were never registered
//! entries in ExifTool's table at all.
//!
//! Value formatting (floats, strings, dates, APEX conversions, `PrintConv`
//! maps) lives in `phaseone.rs`'s own tag table, keyed off these names --
//! ExifTool's per-tag `Format`/`ValueConv`/`PrintConv` chains aren't
//! representable by a single-scalar decoder registry.

/// Look up a `PhaseOne::Main` tag's ExifTool display name by ID.
pub fn phaseone_tag_name(tag_id: u32) -> Option<&'static str> {
    match tag_id {
        0x0100 => Some("CameraOrientation"),
        0x0102 => Some("SerialNumber"),
        0x0105 => Some("ISO"),
        0x0106 => Some("ColorMatrix1"),
        0x0107 => Some("WB_RGBLevels"),
        0x0108 => Some("SensorWidth"),
        0x0109 => Some("SensorHeight"),
        0x010a => Some("SensorLeftMargin"),
        0x010b => Some("SensorTopMargin"),
        0x010c => Some("ImageWidth"),
        0x010d => Some("ImageHeight"),
        0x010e => Some("RawFormat"),
        0x010f => Some("RawData"),
        0x0110 => Some("SensorCalibration"), // SubDirectory; never emitted as a scalar value, see phaseone.rs
        0x0112 => Some("DateTimeOriginal"),
        0x0113 => Some("ImageNumber"),
        0x0203 => Some("Software"),
        0x0204 => Some("System"),
        0x0210 => Some("SensorTemperature"),
        0x0211 => Some("SensorTemperature2"),
        0x021c => Some("StripOffsets"),
        0x021d => Some("BlackLevel"),
        0x0222 => Some("SplitColumn"),
        0x0223 => Some("BlackLevelData"),
        0x0226 => Some("ColorMatrix2"),
        0x0262 => Some("SequenceID"),
        0x0263 => Some("SequenceKind"),
        0x0264 => Some("SequenceFrameNumber"),
        0x0265 => Some("SequenceFrameCount"),
        0x0267 => Some("AFAdjustment"),
        0x0301 => Some("FirmwareVersions"),
        0x0400 => Some("ShutterSpeedValue"),
        0x0401 => Some("ApertureValue"),
        0x0402 => Some("ExposureCompensation"),
        0x0403 => Some("FocalLength"),
        0x0410 => Some("CameraModel"),
        0x0412 => Some("LensModel"),
        0x0414 => Some("MaxApertureValue"),
        0x0415 => Some("MinApertureValue"),
        0x0455 => Some("Viewfinder"),
        _ => None,
    }
}

/// `%Image::ExifTool::PhaseOne::SensorCalibration` tag IDs that are not
/// `Unknown`-flagged and so appear in default (non `-u`) ExifTool output.
/// Every other ID in that table (0x0401, 0x0404-0x0406, 0x0408, 0x040b,
/// 0x040f, 0x0410, 0x0413, 0x0414, 0x0416, 0x0418, 0x041c, 0x041e) carries
/// `Flags => ['Unknown', ...]` and is intentionally absent here.
pub fn sensor_calibration_tag_name(tag_id: u32) -> Option<&'static str> {
    match tag_id {
        0x0400 => Some("SensorDefects"),
        0x0407 => Some("SerialNumber"),
        0x0419 => Some("LinearizationCoefficients1"),
        0x041a => Some("LinearizationCoefficients2"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_names_match_exiftool() {
        // Ground truth: PhaseOne.pm Main table, verified against
        // `exiftool -G1 -s /tmp/oxidex-exiftool-cache/combined-samples/PhaseOne.iiq`.
        assert_eq!(phaseone_tag_name(0x0211), Some("SensorTemperature2"));
        assert_eq!(phaseone_tag_name(0x0401), Some("ApertureValue"));
        assert_eq!(phaseone_tag_name(0x0402), Some("ExposureCompensation"));
        assert_eq!(phaseone_tag_name(0x0403), Some("FocalLength"));
        assert_eq!(phaseone_tag_name(0x0412), Some("LensModel"));
        assert_eq!(phaseone_tag_name(0x0601), None); // never a PhaseOne::Main tag
    }

    #[test]
    fn test_invented_tags_are_gone() {
        // 0x0213/0x0214/0x0215 are not tags in PhaseOne::Main at all.
        assert_eq!(phaseone_tag_name(0x0213), None);
        assert_eq!(phaseone_tag_name(0x0214), None);
        assert_eq!(phaseone_tag_name(0x0215), None);
        // 0x0212 is UnknownDate (Unknown-flagged; hidden by default).
        assert_eq!(phaseone_tag_name(0x0212), None);
    }

    #[test]
    fn test_sensor_calibration_names() {
        assert_eq!(sensor_calibration_tag_name(0x0400), Some("SensorDefects"));
        assert_eq!(
            sensor_calibration_tag_name(0x0419),
            Some("LinearizationCoefficients1")
        );
        // Unknown-flagged; hidden by default.
        assert_eq!(sensor_calibration_tag_name(0x0401), None);
    }
}
