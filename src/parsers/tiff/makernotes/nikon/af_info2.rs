//! `Nikon::AFInfo2V0100` .. `AFInfo2V0400` (`Nikon::Main` 0x00b7).
//!
//! Five layouts selected by the block's own version. Not encrypted.
//!
//! The AF *point* tags (`AFPointsUsed`, `AFPointsInFocus`, `AFPointsSelected`
//! and `PrimaryAFPoint`) are deliberately not decoded: each is a bitmap whose
//! meaning depends on a per-body point-name grid, and picking the wrong grid
//! would produce a confident, wrong list of focus points rather than none.

use std::collections::HashMap;

use super::value_reader::{ascii_value, read_u16};
use crate::parsers::tiff::ifd_parser::ByteOrder;

const AF_DETECTION_METHOD: &[(u8, &str)] =
    &[(0, "Phase Detect"), (1, "Contrast Detect"), (2, "Hybrid")];

const NO_YES: &[(u8, &str)] = &[(0, "No"), (1, "Yes")];

const FOCUS_RESULT: &[(u8, &str)] = &[(0, "Out of Focus"), (1, "Focus")];

const PHASE_DETECT_AF: &[(u8, &str)] =
    &[(4, "On (73-point)"), (5, "On (5)"), (6, "On (105-point)")];

/// AFAreaMode when AFDetectionMethod is 0 (phase detect), V0100/V0101/V0300.
#[rustfmt::skip]
const AF_AREA_MODE_PHASE: &[(u8, &str)] = &[
    (0, "Single Area"), (1, "Dynamic Area"), (2, "Dynamic Area (closest subject)"),
    (3, "Group Dynamic"), (4, "Dynamic Area (9 points)"),
    (5, "Dynamic Area (21 points)"), (6, "Dynamic Area (51 points)"),
    (7, "Dynamic Area (51 points, 3D-tracking)"), (8, "Auto-area"),
    (9, "Dynamic Area (3D-tracking)"), (10, "Single Area (wide)"),
    (11, "Dynamic Area (wide)"), (12, "Dynamic Area (wide, 3D-tracking)"),
    (13, "Group Area"), (14, "Dynamic Area (25 points)"),
    (15, "Dynamic Area (72 points)"), (16, "Group Area (HL)"),
    (17, "Group Area (VL)"), (18, "Dynamic Area (49 points)"), (128, "Single"),
    (129, "Auto (41 points)"), (130, "Subject Tracking (41 points)"),
    (131, "Face Priority (41 points)"), (192, "Pinpoint"), (193, "Single"),
    (194, "Dynamic"), (195, "Wide (S)"), (196, "Wide (L)"), (197, "Auto"),
    (199, "Auto"),
];

/// AFAreaMode otherwise, V0100/V0101/V0300.
#[rustfmt::skip]
const AF_AREA_MODE_OTHER: &[(u8, &str)] = &[
    (0, "Contrast-detect"), (1, "Contrast-detect (normal area)"),
    (2, "Contrast-detect (wide area)"), (3, "Contrast-detect (face priority)"),
    (4, "Contrast-detect (subject tracking)"), (128, "Single"),
    (129, "Auto (41 points)"), (130, "Subject Tracking (41 points)"),
    (131, "Face Priority (41 points)"), (192, "Pinpoint"), (193, "Single"),
    (194, "Dynamic"), (195, "Wide (S)"), (196, "Wide (L)"), (197, "Auto"),
    (198, "Auto (People)"), (199, "Auto (Animal)"), (200, "Normal-area AF"),
    (201, "Wide-area AF"), (202, "Face-priority AF"),
    (203, "Subject-tracking AF"), (204, "Dynamic Area (S)"),
    (205, "Dynamic Area (M)"), (206, "Dynamic Area (L)"), (207, "3D-tracking"),
    (208, "Wide-Area (C1/C2)"),
];

/// V0200 (the Nikon 1 series) has its own short table.
#[rustfmt::skip]
const AF_AREA_MODE_V0200: &[(u8, &str)] = &[
    (128, "Single"), (129, "Auto (41 points)"),
    (130, "Subject Tracking (41 points)"), (131, "Face Priority (41 points)"),
];

/// V0400 (Expeed 7) likewise.
#[rustfmt::skip]
const AF_AREA_MODE_V0400: &[(u8, &str)] = &[
    (192, "Pinpoint"), (193, "Single"), (195, "Wide (S)"), (196, "Wide (L)"),
    (197, "Auto"), (204, "Dynamic Area (S)"), (205, "Dynamic Area (M)"),
    (206, "Dynamic Area (L)"), (207, "3D-tracking"), (208, "Wide (C1/C2)"),
];

const FOCUS_POINT_SCHEMA_V0100: &[(u8, &str)] = &[
    (0, "Off"),
    (1, "51-point"),
    (2, "11-point"),
    (3, "39-point"),
];

const FOCUS_POINT_SCHEMA_V0101: &[(u8, &str)] = &[
    (0, "Off"),
    (1, "51-point"),
    (2, "11-point"),
    (7, "153-point"),
];

const FOCUS_POINT_SCHEMA_V0300: &[(u8, &str)] = &[
    (0, "Off"),
    (1, "51-point"),
    (8, "81-point"),
    (9, "105-point"),
];

fn lookup(table: &[(u8, &str)], value: u8) -> String {
    match table.iter().find(|(k, _)| *k == value) {
        Some((_, name)) => (*name).to_string(),
        None => format!("Unknown ({})", value),
    }
}

/// Walk `Nikon::Main` 0x00b7.
pub fn parse_af_info2(data: &[u8], order: ByteOrder, tags: &mut HashMap<String, String>) {
    if data.len() < 4 {
        return;
    }
    let version = ascii_value(&data[..4]);
    tags.insert("Nikon:AFInfo2Version".to_string(), version.clone());

    let byte = |at: usize| data.get(at).copied();
    // Every geometry field except the two coordinate ones carries
    // `RawConv => '$val ? $val : undef'`, so a zero is no value at all.
    let put_u16 = |at: usize, name: &str, tags: &mut HashMap<String, String>| {
        if let Some(value) = read_u16(data, at, order)
            && value != 0
        {
            tags.insert(format!("Nikon:{}", name), value.to_string());
        }
    };
    // The coordinates have a DataMember RawConv instead, and a zero survives it.
    let put_coord = |at: usize, name: &str, tags: &mut HashMap<String, String>| {
        if let Some(value) = read_u16(data, at, order) {
            tags.insert(format!("Nikon:{}", name), value.to_string());
        }
    };

    match version.as_str() {
        "0100" | "0101" => {
            let detection = byte(4).unwrap_or(0);
            tags.insert(
                "Nikon:AFDetectionMethod".to_string(),
                lookup(AF_DETECTION_METHOD, detection),
            );
            if let Some(raw) = byte(5) {
                let table = if detection == 0 {
                    AF_AREA_MODE_PHASE
                } else {
                    AF_AREA_MODE_OTHER
                };
                tags.insert("Nikon:AFAreaMode".to_string(), lookup(table, raw));
            }
            if let Some(raw) = byte(6) {
                let table = if version == "0100" {
                    FOCUS_POINT_SCHEMA_V0100
                } else {
                    FOCUS_POINT_SCHEMA_V0101
                };
                tags.insert("Nikon:FocusPointSchema".to_string(), lookup(table, raw));
            }
            // The contrast-detect geometry block moves between the two
            // versions; it is only present at all for AFDetectionMethod 1.
            if detection == 1 {
                let base = if version == "0100" { 16 } else { 70 };
                for (i, name) in [
                    "AFImageWidth",
                    "AFImageHeight",
                    "AFAreaXPosition",
                    "AFAreaYPosition",
                    "AFAreaWidth",
                    "AFAreaHeight",
                ]
                .iter()
                .enumerate()
                {
                    put_u16(base + i * 2, name, tags);
                }
                let focus_at = if version == "0100" { 28 } else { 82 };
                if let Some(raw) = byte(focus_at) {
                    tags.insert(
                        "Nikon:ContrastDetectAFInFocus".to_string(),
                        lookup(NO_YES, raw),
                    );
                }
            }
        }
        // Nikon 1 series.
        "0200" | "0201" => {
            if let Some(raw) = byte(5) {
                tags.insert(
                    "Nikon:AFAreaMode".to_string(),
                    lookup(AF_AREA_MODE_V0200, raw),
                );
            }
            if let Some(raw) = byte(6) {
                tags.insert(
                    "Nikon:PhaseDetectAF".to_string(),
                    lookup(PHASE_DETECT_AF, raw),
                );
            }
        }
        // Expeed 6: D6, D780, Z5, Z6, Z7, Z30, Z50, Z6_2, Z7_2, Zfc.
        "0300" | "0301" => {
            let detection = byte(4).unwrap_or(0);
            tags.insert(
                "Nikon:AFDetectionMethod".to_string(),
                lookup(AF_DETECTION_METHOD, detection),
            );
            if let Some(raw) = byte(5) {
                let table = if detection == 0 {
                    AF_AREA_MODE_PHASE
                } else {
                    AF_AREA_MODE_OTHER
                };
                tags.insert("Nikon:AFAreaMode".to_string(), lookup(table, raw));
            }
            if let Some(raw) = byte(6) {
                tags.insert(
                    "Nikon:FocusPointSchema".to_string(),
                    lookup(FOCUS_POINT_SCHEMA_V0300, raw),
                );
            }
            let coords = byte(7).unwrap_or(0);
            tags.insert(
                "Nikon:AFCoordinatesAvailable".to_string(),
                lookup(NO_YES, coords),
            );
            put_u16(42, "AFImageWidth", tags);
            put_u16(44, "AFImageHeight", tags);
            if coords == 1 {
                put_coord(46, "AFAreaXPosition", tags);
                put_coord(48, "AFAreaYPosition", tags);
            }
            put_u16(50, "AFAreaWidth", tags);
            put_u16(52, "AFAreaHeight", tags);
        }
        // Expeed 7: Z8, Z9 (0400), Z6III and Zf (0401), Z50II (0402).
        "0400" | "0401" | "0402" => {
            if let Some(raw) = byte(4) {
                tags.insert(
                    "Nikon:AFDetectionMethod".to_string(),
                    lookup(AF_DETECTION_METHOD, raw),
                );
            }
            if let Some(raw) = byte(5) {
                tags.insert(
                    "Nikon:AFAreaMode".to_string(),
                    lookup(AF_AREA_MODE_V0400, raw),
                );
            }
            let coords = byte(7).unwrap_or(0);
            tags.insert(
                "Nikon:AFCoordinatesAvailable".to_string(),
                lookup(NO_YES, coords),
            );
            put_u16(62, "AFImageWidth", tags);
            put_u16(64, "AFImageHeight", tags);
            if coords == 1 {
                put_coord(66, "AFAreaXPosition", tags);
                put_coord(68, "AFAreaYPosition", tags);
            }
            put_u16(70, "AFAreaWidth", tags);
            put_u16(72, "AFAreaHeight", tags);
            if let Some(raw) = byte(74) {
                tags.insert("Nikon:FocusResult".to_string(), lookup(FOCUS_RESULT, raw));
            }
        }
        // No table claims this version, so ExifTool reports nothing beyond it.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_mode_table_follows_the_detection_method() {
        let mut phase = vec![0u8; 32];
        phase[..4].copy_from_slice(b"0100");
        phase[4] = 0; // Phase Detect
        phase[5] = 1;
        let mut tags = HashMap::new();
        parse_af_info2(&phase, ByteOrder::BigEndian, &mut tags);
        assert_eq!(tags["Nikon:AFAreaMode"], "Dynamic Area");

        let mut contrast = phase.clone();
        contrast[4] = 1; // Contrast Detect
        let mut tags = HashMap::new();
        parse_af_info2(&contrast, ByteOrder::BigEndian, &mut tags);
        assert_eq!(tags["Nikon:AFAreaMode"], "Contrast-detect (normal area)");
    }

    #[test]
    fn coordinates_are_gated_on_the_availability_flag() {
        let mut data = vec![0u8; 80];
        data[..4].copy_from_slice(b"0300");
        data[7] = 0; // AFCoordinatesAvailable = No
        data[42..44].copy_from_slice(&8256u16.to_be_bytes()); // AFImageWidth
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, &mut tags);
        assert_eq!(tags["Nikon:AFCoordinatesAvailable"], "No");
        // The geometry outside the coordinate pair is reported regardless.
        assert_eq!(tags["Nikon:AFImageWidth"], "8256");
        // ...but a zero AFImageHeight is dropped by its RawConv, and the
        // coordinates are suppressed entirely by the availability flag.
        assert!(!tags.contains_key("Nikon:AFImageHeight"));
        assert!(!tags.contains_key("Nikon:AFAreaXPosition"));
    }

    #[test]
    fn an_unclaimed_version_reports_only_itself() {
        let mut tags = HashMap::new();
        parse_af_info2(b"0999\x01\x02\x03\x04", ByteOrder::BigEndian, &mut tags);
        assert_eq!(tags.len(), 1);
    }
}
