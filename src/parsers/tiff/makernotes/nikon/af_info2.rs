//! `Nikon::AFInfo2V0100` .. `AFInfo2V0400` (`Nikon::Main` 0x00b7).
//!
//! Five layouts selected by the block's own version. Not encrypted.
//!
//! `AFPointsUsed` and `PrimaryAFPoint` are decoded via the point-name grids
//! transcribed into `af_points.rs` from ExifTool's own `afPoints*` tables.
//! `AFPointsInFocus` and `AFPointsSelected` remain deliberately undecoded:
//! they share the same per-body grid dependency but weren't in scope for
//! this pass (see docs/plans/specs/2026-08-01-nikon-af-points-design.md).

use std::collections::HashMap;

use super::af_points::{
    self, AF_POINTS_39, AF_POINTS_51, AF_POINTS_81, AF_POINTS_105, AF_POINTS_135, AF_POINTS_153,
};
use super::value_reader::{ascii_value, read_u16};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// 11-point bit-number -> name, for the `AFPointsUsed`/`PrimaryAFPoint`
/// pair at V0100/V0101 offset 8 / 0x44 (Nikon.pm:4258-4270, 4396-4408,
/// 4505-4522). This is *not* `af_points::AF_POINTS_11`: that table was
/// transcribed from the separate, legacy `%afPoints11` hash (Nikon.pm:1436,
/// used by the older `Nikon::AFInfo` table's `AFPoint`/`AFPointsInFocus`),
/// which orders bits 4-9 differently (`Mid-right` before `Upper-left`).
/// The two hashes are easy to conflate -- ExifTool itself keeps them as
/// separate literals rather than sharing one -- but mixing them up here
/// would silently swap point names for any bit above 3.
const AF_POINTS_11_OFFSET8: &[&str] = &[
    "Center",
    "Top",
    "Bottom",
    "Mid-left",
    "Upper-left",
    "Lower-left",
    "Far Left",
    "Mid-right",
    "Upper-right",
    "Lower-right",
    "Far Right",
];

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

/// `PrimaryAFPoint`'s direct-lookup shape: raw byte 0 -> "(none)"; raw byte
/// 1 (always the documented center bit-number in every one of these
/// tables) -> "{center name} (Center)"; otherwise a plain table lookup.
/// Ports the repeated `PrintConv => { 0 => '(none)', %afPointsNN, 1 =>
/// 'XX (Center)' }` pattern (e.g. Nikon.pm:4181-4193).
fn primary_af_point(raw: u8, table: &[(u8, &str)]) -> String {
    if raw == 0 {
        return "(none)".to_string();
    }
    match table.iter().find(|(n, _)| *n == raw) {
        Some((1, name)) => format!("{name} (Center)"),
        Some((_, name)) => (*name).to_string(),
        None => format!("Unknown ({raw})"),
    }
}

/// `PrimaryAFPoint`'s grid-computed shape (Nikon.pm ~4656, 4673): raw 0 ->
/// "(none)"; the documented center bit -> "{center_name} (Center)"; else
/// the name is computed the same way `print_af_points_grid` computes it
/// per-bit (ExifTool's `GetAFPointGrid`, non-inverse direction).
fn primary_af_point_grid(raw: u8, ncols: u16, center_bit: u32, center_name: &str) -> String {
    if raw == 0 {
        return "(none)".to_string();
    }
    let bit = raw as u32;
    if bit == center_bit {
        return format!("{center_name} (Center)");
    }
    let row = bit / (ncols as u32);
    let col = bit - (ncols as u32) * row + 1;
    match char::from_u32(65 + row) {
        Some(letter) => format!("{letter}{col}"),
        None => format!("Unknown ({raw})"),
    }
}

/// `AFPointsUsed`'s 11-point shape is a raw little-endian `int16u` BITMASK
/// -- read as such regardless of the MakerNote's own byte order, per
/// Nikon.pm's explicit "read as int16u in little-endian byte order"
/// comment (Nikon.pm:4258-4270) -- not the byte-array bitmap
/// `PrintAFPoints` walks.
fn af_points_used_bitmask11(raw: u16) -> String {
    if raw == 0 {
        return "(none)".to_string();
    }
    if raw == 0x7ff {
        return "All 11 Points".to_string();
    }
    let mut points: Vec<String> = Vec::new();
    for bit in 0..16u32 {
        if raw & (1 << bit) == 0 {
            continue;
        }
        match AF_POINTS_11_OFFSET8.get(bit as usize) {
            Some(name) => points.push((*name).to_string()),
            None => points.push(format!("[{bit}]")),
        }
    }
    points.join(", ")
}

/// `PrimaryAFPoint`'s 11-point shape: direct 1-based lookup into
/// `AF_POINTS_11_OFFSET8`, no "(Center)" suffix (Nikon.pm:4517-4528).
fn primary_af_point_11(raw: u8) -> String {
    if raw == 0 {
        return "(none)".to_string();
    }
    match AF_POINTS_11_OFFSET8.get((raw - 1) as usize) {
        Some(name) => (*name).to_string(),
        None => format!("Unknown ({raw})"),
    }
}

/// Ports ExifTool's `$$self{Model} =~ /^NIKON (...)\b/i` model-prefix
/// checks (Nikon.pm:4966,4975,4983) as a plain case-insensitive prefix
/// match against each literal in `prefixes` -- no regex dependency needed
/// for three fixed alternatives.
fn model_matches(model: &str, prefixes: &[&str]) -> bool {
    let model = model.to_ascii_uppercase();
    prefixes.iter().any(|p| {
        let prefix = p.to_ascii_uppercase();
        if !model.starts_with(&prefix) {
            return false;
        }
        // Mirror ExifTool's trailing `\b`: the prefix must consume the
        // whole string, or the next character must be a non-word boundary
        // (not alphanumeric/underscore), so "NIKON Z fc" doesn't match
        // prefix "NIKON Z f".
        match model[prefix.len()..].chars().next() {
            None => true,
            Some(c) => !(c.is_ascii_alphanumeric() || c == '_'),
        }
    })
}

/// Walk `Nikon::Main` 0x00b7.
pub fn parse_af_info2(
    data: &[u8],
    order: ByteOrder,
    model: Option<&str>,
    tags: &mut HashMap<String, String>,
) {
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
            // PrimaryAFPoint's Condition checks only FocusPointSchema (Nikon.pm:4181-
            // 4230, 4505-4551), never AFDetectionMethod -- `detection` (already bound
            // above for AFAreaMode) is unrelated here.
            let primary_at = if version == "0100" { 7 } else { 0x44 };
            if let Some(raw) = byte(primary_at) {
                let primary: Option<String> = match (version.as_str(), byte(6).unwrap_or(0)) {
                    (_, 1) => Some(primary_af_point(raw, AF_POINTS_51)),
                    (_, 2) => Some(primary_af_point_11(raw)),
                    ("0100", 3) => Some(primary_af_point(raw, AF_POINTS_39)),
                    ("0101", 7) => Some(primary_af_point(raw, AF_POINTS_153)),
                    (_, 0) => Some("(none)".to_string()),
                    _ => None,
                };
                if let Some(primary) = primary {
                    tags.insert("Nikon:PrimaryAFPoint".to_string(), primary);
                }
            }

            let schema = byte(6).unwrap_or(0);
            let used = match (version.as_str(), schema) {
                (_, 1) => data
                    .get(8..15)
                    .map(|bits| af_points::print_af_points_lookup(bits, AF_POINTS_51)),
                // Always little-endian per Nikon.pm's explicit comment,
                // independent of the MakerNote's own `order`.
                (_, 2) => read_u16(data, 8, ByteOrder::LittleEndian).map(af_points_used_bitmask11),
                ("0100", 3) => data
                    .get(8..13)
                    .map(|bits| af_points::print_af_points_lookup(bits, AF_POINTS_39)),
                ("0101", 7) => data
                    .get(8..28)
                    .map(|bits| af_points::print_af_points_lookup(bits, AF_POINTS_153)),
                (_, 0) => Some("(none)".to_string()),
                _ => None,
            };
            if let Some(used) = used {
                tags.insert("Nikon:AFPointsUsed".to_string(), used);
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
            let phase_detect = byte(6);
            if let Some(raw) = phase_detect {
                tags.insert(
                    "Nikon:PhaseDetectAF".to_string(),
                    lookup(PHASE_DETECT_AF, raw),
                );
            }
            match phase_detect {
                Some(4) => {
                    if let Some(raw) = byte(7) {
                        tags.insert(
                            "Nikon:PrimaryAFPoint".to_string(),
                            primary_af_point(raw, AF_POINTS_135),
                        );
                    }
                    if let Some(bits) = data.get(8..25) {
                        tags.insert(
                            "Nikon:AFPointsUsed".to_string(),
                            af_points::print_af_points_lookup(bits, AF_POINTS_135),
                        );
                    }
                }
                Some(5) => {
                    if let Some(raw) = byte(7) {
                        tags.insert(
                            "Nikon:PrimaryAFPoint".to_string(),
                            primary_af_point_grid(raw, 15, 82, "F8"),
                        );
                    }
                    if let Some(bits) = data.get(8..29) {
                        tags.insert(
                            "Nikon:AFPointsUsed".to_string(),
                            af_points::print_af_points_grid(bits, 15),
                        );
                    }
                }
                Some(6) => {
                    if let Some(raw) = byte(7) {
                        tags.insert(
                            "Nikon:PrimaryAFPoint".to_string(),
                            primary_af_point_grid(raw, 21, 115, "F11"),
                        );
                    }
                    if let Some(bits) = data.get(8..37) {
                        tags.insert(
                            "Nikon:AFPointsUsed".to_string(),
                            af_points::print_af_points_grid(bits, 21),
                        );
                    }
                }
                _ => {}
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
            if coords == 0 {
                let schema = byte(6).unwrap_or(0);
                let table_and_len: Option<(&[(u8, &str)], usize)> = match schema {
                    1 => Some((AF_POINTS_51, 7)),
                    8 => Some((AF_POINTS_81, 11)),
                    9 => Some((AF_POINTS_105, 14)),
                    _ => None,
                };
                if let Some((table, bitmap_len)) = table_and_len {
                    if let Some(raw) = byte(0x38) {
                        tags.insert(
                            "Nikon:PrimaryAFPoint".to_string(),
                            primary_af_point(raw, table),
                        );
                    }
                    if let Some(bits) = data.get(0x0a..0x0a + bitmap_len) {
                        tags.insert(
                            "Nikon:AFPointsUsed".to_string(),
                            af_points::print_af_points_lookup(bits, table),
                        );
                    }
                }
            }
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

            let area_mode_used = byte(5);
            if matches!(area_mode_used, Some(197) | Some(207)) {
                let m = model.unwrap_or("");
                let table_and_len: Option<(&[&str], usize)> =
                    if model_matches(m, &["NIKON Z 8", "NIKON Z 9"]) {
                        Some((af_points::AF_POINTS_405, 51))
                    } else if model_matches(m, &["NIKON Z6_3", "NIKON Z f", "NIKON Z5_2"]) {
                        Some((af_points::AF_POINTS_299, 38))
                    } else if model_matches(m, &["NIKON Z50_2"]) {
                        Some((af_points::AF_POINTS_231, 29))
                    } else {
                        None
                    };
                if let Some((table, len)) = table_and_len
                    && let Some(bits) = data.get(10..10 + len)
                {
                    tags.insert(
                        "Nikon:AFPointsUsed".to_string(),
                        af_points::print_af_points_array(bits, table),
                    );
                }
            }

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
        parse_af_info2(&phase, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFAreaMode"], "Dynamic Area");

        let mut contrast = phase.clone();
        contrast[4] = 1; // Contrast Detect
        let mut tags = HashMap::new();
        parse_af_info2(&contrast, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFAreaMode"], "Contrast-detect (normal area)");
    }

    #[test]
    fn coordinates_are_gated_on_the_availability_flag() {
        let mut data = vec![0u8; 80];
        data[..4].copy_from_slice(b"0300");
        data[7] = 0; // AFCoordinatesAvailable = No
        data[42..44].copy_from_slice(&8256u16.to_be_bytes()); // AFImageWidth
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
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
        parse_af_info2(
            b"0999\x01\x02\x03\x04",
            ByteOrder::BigEndian,
            None,
            &mut tags,
        );
        assert_eq!(tags.len(), 1);
    }

    #[test]
    fn v0100_51point_reports_center_from_real_sample_shape() {
        // NikonD3.jpg: AFPointsUsed=C6, PrimaryAFPoint=C6 (Center).
        // FocusPointSchema=1 (51-point), AFPointsUsed bit-number 1 = byte0 bit0.
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"0100");
        data[6] = 1; // FocusPointSchema = 51-point
        data[7] = 1; // PrimaryAFPoint raw = 1 (center)
        data[8] = 0x01; // AFPointsUsed bitmap byte 0, bit 0 -> bit-number 1
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "C6");
        assert_eq!(tags["Nikon:PrimaryAFPoint"], "C6 (Center)");
    }

    #[test]
    fn v0100_11point_uses_bitmask_not_lookup_table() {
        // NikonD90.jpg: AFPointsUsed=Top, PrimaryAFPoint=Top.
        // FocusPointSchema=2 (11-point). AFPointsUsed is little-endian int16u
        // BITMASK: bit 1 = "Top" (Nikon.pm:1446). PrimaryAFPoint raw=2 -> "Top"
        // (Nikon.pm:4204: 2 => 'Top', one-based).
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"0100");
        data[6] = 2; // FocusPointSchema = 11-point
        data[7] = 2; // PrimaryAFPoint raw = 2 -> Top
        data[8] = 0x02; // little-endian u16 = 2 -> bit 1 set -> "Top"
        data[9] = 0x00;
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "Top");
        assert_eq!(tags["Nikon:PrimaryAFPoint"], "Top");
    }

    #[test]
    fn v0100_11point_all_points_literal() {
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"0100");
        data[6] = 2;
        data[7] = 0;
        data[8] = 0xff; // 0x7ff little-endian
        data[9] = 0x07;
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "All 11 Points");
    }

    #[test]
    fn v0100_schema_zero_reports_none_for_both_tags() {
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"0100");
        data[6] = 0; // FocusPointSchema = Off
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "(none)");
        assert_eq!(tags["Nikon:PrimaryAFPoint"], "(none)");
    }

    #[test]
    fn v0101_153point_center_at_offset_0x44() {
        // NikonD850.jpg: AFPointsUsed=E9, PrimaryAFPoint=E9 (Center).
        let mut data = vec![0u8; 105];
        data[..4].copy_from_slice(b"0101");
        data[6] = 7; // FocusPointSchema = 153-point
        data[8] = 0x01; // AFPointsUsed bit-number 1 -> "E9"
        data[0x44] = 1; // PrimaryAFPoint raw = 1 -> center
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "E9");
        assert_eq!(tags["Nikon:PrimaryAFPoint"], "E9 (Center)");
    }

    #[test]
    fn v0200_135point_phase4_uses_lookup_table() {
        // Nikon1J1.jpg: AFPointsUsed=B11, PhaseDetectAF=4.
        // afPoints135 bit-number 13 = 'B9'... use a value traceable to the
        // table directly: bit-number 1 = 'E8' (Nikon.pm:1534).
        let mut data = vec![0u8; 30];
        data[..4].copy_from_slice(b"0200");
        data[6] = 4; // PhaseDetectAF = On (73-point)
        data[7] = 1; // PrimaryAFPoint raw = 1 -> center
        data[8] = 0x01; // AFPointsUsed bit-number 1 -> "E8"
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "E8");
        assert_eq!(tags["Nikon:PrimaryAFPoint"], "E8 (Center)");
    }

    #[test]
    fn v0200_135point_phase5_uses_computed_grid() {
        // PhaseDetectAF=5: grid-computed, ncols=15. Center is bit 82 -> "F8".
        let mut data = vec![0u8; 35];
        data[..4].copy_from_slice(b"0200");
        data[6] = 5;
        data[7] = 82; // PrimaryAFPoint raw = 82 -> literal "F8 (Center)" override
        data[8 + 10] = 1 << 2; // AFPointsUsed bit 82 (byte 10, offset 2) -> "F8"
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "F8");
        assert_eq!(tags["Nikon:PrimaryAFPoint"], "F8 (Center)");
    }

    #[test]
    fn v0300_81point_gated_on_coordinates_unavailable() {
        // NikonZ50.jpg: AFPointsUsed includes C5,C6,D5,E5,E6, PrimaryAFPoint=D5.
        // FocusPointSchema=8 (81-point). afPoints81 bit-number for 'E5' is 1
        // (Nikon.pm:1616) -- use that as the minimal traceable case.
        let mut data = vec![0u8; 60];
        data[..4].copy_from_slice(b"0300");
        data[6] = 8; // FocusPointSchema = 81-point
        data[7] = 0; // AFCoordinatesAvailable = No
        data[0x38] = 1; // PrimaryAFPoint raw = 1 -> center
        data[0x0a] = 0x01; // AFPointsUsed bit-number 1 -> "E5"
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "E5");
        assert_eq!(tags["Nikon:PrimaryAFPoint"], "E5 (Center)");
    }

    #[test]
    fn v0300_absent_when_coordinates_available() {
        let mut data = vec![0u8; 60];
        data[..4].copy_from_slice(b"0300");
        data[6] = 8;
        data[7] = 1; // AFCoordinatesAvailable = Yes -> neither tag exists
        data[0x38] = 1;
        data[0x0a] = 0x01;
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert!(!tags.contains_key("Nikon:AFPointsUsed"));
        assert!(!tags.contains_key("Nikon:PrimaryAFPoint"));
    }

    #[test]
    fn v0300_105point_d6() {
        // NikonD6.jpg-shaped: FocusPointSchema=9 (105-point), center bit-number
        // 1 -> "D8" (Nikon.pm:1500).
        let mut data = vec![0u8; 60];
        data[..4].copy_from_slice(b"0300");
        data[6] = 9;
        data[7] = 0;
        data[0x38] = 1;
        data[0x0a] = 0x01;
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "D8");
        assert_eq!(tags["Nikon:PrimaryAFPoint"], "D8 (Center)");
    }

    #[test]
    fn v0200_171point_phase6_uses_computed_grid_21_cols() {
        // Nikon1J4.jpg: AFPointsUsed=F11, PhaseDetectAF=6, ncols=21, center
        // bit=115 -> "F11" (115/21=5->'F', 115-21*5+1=11).
        let mut data = vec![0u8; 40];
        data[..4].copy_from_slice(b"0200");
        data[6] = 6;
        data[7] = 115; // literal "F11 (Center)" override
        data[8 + 14] = 1 << 3; // bit 115 = byte 14 (115/8=14), offset 3 (115%8=3)
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], "F11");
        assert_eq!(tags["Nikon:PrimaryAFPoint"], "F11 (Center)");
    }

    #[test]
    fn v0400_z8_z9_uses_405point_array() {
        let mut data = vec![0u8; 70];
        data[..4].copy_from_slice(b"0400");
        data[5] = 197; // AFAreaModeUsed = Auto
        data[7] = 0; // AFCoordinatesAvailable = No
        data[10] = 0x01; // AFPointsUsed bit index 0 -> AF_POINTS_405[0]
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON Z 9"), &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], af_points::AF_POINTS_405[0]);
    }

    #[test]
    fn v0400_z8_z9_absent_for_other_area_modes() {
        let mut data = vec![0u8; 70];
        data[..4].copy_from_slice(b"0400");
        data[5] = 193; // AFAreaModeUsed = Single, not Auto/3D-tracking
        data[10] = 0x01;
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON Z 9"), &mut tags);
        assert!(!tags.contains_key("Nikon:AFPointsUsed"));
    }

    #[test]
    fn v0401_zf_uses_299point_array() {
        let mut data = vec![0u8; 60];
        data[..4].copy_from_slice(b"0401");
        data[5] = 207; // 3D-tracking
        data[10] = 0x01;
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON Z f"), &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], af_points::AF_POINTS_299[0]);
    }

    #[test]
    fn v0402_z50ii_uses_231point_array() {
        let mut data = vec![0u8; 50];
        data[..4].copy_from_slice(b"0402");
        data[5] = 197;
        data[10] = 0x01;
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON Z50_2"), &mut tags);
        assert_eq!(tags["Nikon:AFPointsUsed"], af_points::AF_POINTS_231[0]);
    }

    #[test]
    fn v0400_unrecognized_model_reports_nothing() {
        let mut data = vec![0u8; 60];
        data[..4].copy_from_slice(b"0400");
        data[5] = 197;
        data[10] = 0x01;
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON D850"), &mut tags);
        assert!(!tags.contains_key("Nikon:AFPointsUsed"));
    }

    // -- Finding 1+2: af_points_used_bitmask11 separator + unmapped bits --

    #[test]
    fn bitmask11_joins_multiple_points_with_comma_space() {
        // Bits 1 and 2 -> AF_POINTS_11_OFFSET8[1]="Top", [2]="Bottom".
        assert_eq!(af_points_used_bitmask11(0x06), "Top, Bottom");
    }

    #[test]
    fn bitmask11_renders_unmapped_bit_as_bracketed_index() {
        // Bit 12 has no entry in AF_POINTS_11_OFFSET8 (only 11 entries, 0-10).
        assert_eq!(af_points_used_bitmask11(0x1000), "[12]");
    }

    // -- Finding 4: unclaimed FocusPointSchema yields no PrimaryAFPoint tag --

    #[test]
    fn v0100_unrecognized_schema_reports_no_primary_af_point() {
        let mut data = vec![0u8; 16];
        data[..4].copy_from_slice(b"0100");
        data[6] = 99; // FocusPointSchema not in {0,1,2,3} for V0100
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert!(!tags.contains_key("Nikon:PrimaryAFPoint"));
    }

    #[test]
    fn v0101_unrecognized_schema_reports_no_primary_af_point() {
        let mut data = vec![0u8; 105];
        data[..4].copy_from_slice(b"0101");
        data[6] = 99; // FocusPointSchema not in {0,1,2,7} for V0101
        let mut tags = HashMap::new();
        parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
        assert!(!tags.contains_key("Nikon:PrimaryAFPoint"));
    }

    // -- Finding 5: model_matches requires a word boundary after the prefix --

    #[test]
    fn model_matches_rejects_prefix_without_word_boundary() {
        assert!(!model_matches("NIKON Z fc", &["NIKON Z f"]));
        assert!(model_matches("NIKON Z f", &["NIKON Z f"]));
    }
}
