//! `Nikon::FlashInfo0100` .. `FlashInfo0300` (`Nikon::Main` 0x00a8).
//!
//! Six layouts share one field vocabulary and differ only in where the fields
//! sit, so the tables below are pure offset lists and the decoding lives in one
//! walker. The block is not encrypted.
//!
//! Two ExifTool details are reproduced deliberately rather than tidied up:
//!
//! * `Mask` is applied *and* right-shifted past its low bit, so a `Mask =>
//!   0xf0` field yields 0..15.
//! * Because of that, the `FlashGroupBControlMode >= 0x60` Conditions guarding
//!   `FlashGroupBOutput` in the 0102/0103/0106 tables can never hold, and
//!   ExifTool always reports `FlashGroupBCompensation` there instead. Matching
//!   ExifTool means matching that too.

use std::collections::HashMap;

use super::value_reader::ascii_value;

const FLASH_SOURCE: &[(u8, &str)] = &[(0, "None"), (1, "External"), (2, "Internal")];

/// `%flashControlMode`, shared by the master and the three group fields.
#[rustfmt::skip]
const FLASH_CONTROL_MODE: &[(u8, &str)] = &[
    (0, "Off"), (1, "iTTL-BL"), (2, "iTTL"), (3, "Auto Aperture"),
    (4, "Automatic"), (5, "GN (distance priority)"), (6, "Manual"),
    (7, "Repeating Flash"),
];

const OFF_ON: &[(u8, &str)] = &[(0, "Off"), (1, "On")];

/// `%flashGNDistance`.
#[rustfmt::skip]
const FLASH_GN_DISTANCE: &[(u8, &str)] = &[
    (0, "0"), (1, "0.1 m"), (2, "0.2 m"), (3, "0.3 m"), (4, "0.4 m"),
    (5, "0.5 m"), (6, "0.6 m"), (7, "0.7 m"), (8, "0.8 m"), (9, "0.9 m"),
    (10, "1.0 m"), (11, "1.1 m"), (12, "1.3 m"), (13, "1.4 m"), (14, "1.6 m"),
    (15, "1.8 m"), (16, "2.0 m"), (17, "2.2 m"), (18, "2.5 m"), (19, "2.8 m"),
    (20, "3.2 m"), (21, "3.6 m"), (22, "4.0 m"), (23, "4.5 m"), (24, "5.0 m"),
    (25, "5.6 m"), (26, "6.3 m"), (27, "7.1 m"), (28, "8.0 m"), (29, "9.0 m"),
    (30, "10.0 m"), (31, "11.0 m"), (32, "13.0 m"), (33, "14.0 m"),
    (34, "16.0 m"), (35, "18.0 m"), (36, "20.0 m"), (255, "n/a"),
];

/// `%flashColorFilter`.
#[rustfmt::skip]
const FLASH_COLOR_FILTER: &[(u8, &str)] = &[
    (0, "None"), (1, "FL-GL1 or SZ-2FL Fluorescent"), (2, "FL-GL2"),
    (9, "TN-A1 or SZ-2TN Incandescent"), (10, "TN-A2"), (65, "Red"),
    (66, "Blue"), (67, "Yellow"), (68, "Amber"), (128, "Incandescent"),
];

const FLASH_ILLUMINATION_PATTERN: &[(u8, &str)] =
    &[(0, "Standard"), (1, "Center-weighted"), (2, "Even")];

const EXTERNAL_FLASH_STATUS: &[(u8, &str)] = &[(0, "Flash Not Attached"), (1, "Flash Attached")];

const EXTERNAL_FLASH_READY_STATE: &[(u8, &str)] = &[(0, "n/a"), (1, "Ready"), (6, "Not Ready")];

const NO_YES: &[(u8, &str)] = &[(0, "No"), (1, "Yes")];

/// `%flashFirmware` -- keyed on the two bytes joined with a space.
#[rustfmt::skip]
const FLASH_FIRMWARE: &[(&str, &str)] = &[
    ("0 0", "n/a"),
    ("1 1", "1.01 (SB-800 or Metz 58 AF-1)"),
    ("1 3", "1.03 (SB-800)"),
    ("2 1", "2.01 (SB-800)"),
    ("2 4", "2.04 (SB-600)"),
    ("2 5", "2.05 (SB-600)"),
    ("3 1", "3.01 (SU-800 Remote Commander)"),
    ("4 1", "4.01 (SB-400)"),
    ("4 2", "4.02 (SB-400)"),
    ("4 4", "4.04 (SB-400)"),
    ("5 1", "5.01 (SB-900)"),
    ("5 2", "5.02 (SB-900)"),
    ("6 1", "6.01 (SB-700)"),
    ("7 1", "7.01 (SB-910)"),
    ("14 3", "14.03 (SB-5000)"),
];

/// `ExternalFlashFlags` BITMASK, 0100 through 0106.
const EXTERNAL_FLASH_FLAGS: &[(u8, &str)] = &[
    (0, "Fired"),
    (2, "Bounce Flash"),
    (4, "Wide Flash Adapter"),
    (5, "Dome Diffuser"),
];

/// The 0300 table renames two of the bits.
const EXTERNAL_FLASH_FLAGS_0300: &[(u8, &str)] = &[
    (0, "Flash Ready"),
    (2, "Bounce Flash"),
    (4, "Wide Flash Adapter"),
    (7, "Zoom Override"),
];

/// Byte offsets for one FlashInfo layout. `None` means the field is absent
/// from that version.
struct Layout {
    flash_source: Option<usize>,
    external_flash_firmware: Option<usize>,
    /// (offset, mask) -- 0107 splits this byte into two other tags instead.
    external_flash_flags: Option<usize>,
    /// 0300 renames two bits of the flags byte.
    flags_0300: bool,
    external_flash_zoom_override: Option<usize>,
    external_flash_status: Option<usize>,
    external_flash_ready_state: Option<usize>,
    /// Byte holding FlashCommanderMode (0x80) and FlashControlMode (0x7f).
    commander_and_control: Option<usize>,
    /// FlashOutput when the control mode is >= 6, FlashCompensation otherwise.
    output_or_compensation: Option<usize>,
    /// 0300 splits them: compensation here, output at its own offset.
    compensation_only: Option<usize>,
    output_only: Option<usize>,
    flash_focal_length: Option<usize>,
    repeating_flash_rate: Option<usize>,
    repeating_flash_count: Option<usize>,
    flash_gn_distance: Option<usize>,
    flash_color_filter: Option<usize>,
    flash_illumination_pattern: Option<usize>,
    /// (offset, mask) for groups A, B and C.
    group_control: [Option<(usize, u8)>; 3],
    /// Offsets of the per-group output/compensation bytes.
    group_value: [Option<usize>; 3],
    /// The 0300 group compensation ValueConv is `-($val-2)/6`, not `-$val/6`.
    group_compensation_bias: i32,
    /// `>= 0x60` guards that can never hold once the mask has been shifted.
    group_output_unreachable: [bool; 3],
    external_flash_compensation: Option<usize>,
    flash_exposure_comp3: Option<usize>,
    flash_exposure_comp4: Option<usize>,
}

const EMPTY: Layout = Layout {
    flash_source: None,
    external_flash_firmware: None,
    external_flash_flags: None,
    flags_0300: false,
    external_flash_zoom_override: None,
    external_flash_status: None,
    external_flash_ready_state: None,
    commander_and_control: None,
    output_or_compensation: None,
    compensation_only: None,
    output_only: None,
    flash_focal_length: None,
    repeating_flash_rate: None,
    repeating_flash_count: None,
    flash_gn_distance: None,
    flash_color_filter: None,
    flash_illumination_pattern: None,
    group_control: [None, None, None],
    group_value: [None, None, None],
    group_compensation_bias: 0,
    group_output_unreachable: [false, false, false],
    external_flash_compensation: None,
    flash_exposure_comp3: None,
    flash_exposure_comp4: None,
};

fn layout_for(version: &str) -> Option<Layout> {
    match version {
        // FlashInfo0100 (D2H, D2X, D50, D70, D80, D200)
        "0100" | "0101" => Some(Layout {
            flash_source: Some(4),
            external_flash_firmware: Some(6),
            external_flash_flags: Some(8),
            commander_and_control: Some(9),
            output_or_compensation: Some(10),
            flash_focal_length: Some(11),
            repeating_flash_rate: Some(12),
            repeating_flash_count: Some(13),
            flash_gn_distance: Some(14),
            group_control: [Some((15, 0x0f)), Some((16, 0x0f)), None],
            group_value: [Some(17), Some(18), None],
            ..EMPTY
        }),
        // FlashInfo0102 (D3 1.x, D40, D40X, D60, D300 1.00)
        "0102" => Some(Layout {
            flash_source: Some(4),
            external_flash_firmware: Some(6),
            external_flash_flags: Some(8),
            commander_and_control: Some(9),
            output_or_compensation: Some(10),
            flash_focal_length: Some(12),
            repeating_flash_rate: Some(13),
            repeating_flash_count: Some(14),
            flash_gn_distance: Some(15),
            group_control: [Some((16, 0x0f)), Some((17, 0xf0)), Some((17, 0x0f))],
            group_value: [Some(18), Some(19), Some(20)],
            group_output_unreachable: [false, true, false],
            ..EMPTY
        }),
        // FlashInfo0103 (D3 2.x, D3X, D3S, D4, D90, D300 1.10, D600, D700,
        // D800, D3000, D3100, D3200, D5000, D5100, D5200, D7000)
        // 0104 is the D7000 and 0105 the D800.
        "0103" | "0104" | "0105" => Some(Layout {
            flash_source: Some(4),
            external_flash_firmware: Some(6),
            external_flash_flags: Some(8),
            commander_and_control: Some(9),
            output_or_compensation: Some(10),
            flash_focal_length: Some(12),
            repeating_flash_rate: Some(13),
            repeating_flash_count: Some(14),
            flash_gn_distance: Some(15),
            flash_color_filter: Some(16),
            group_control: [Some((17, 0x0f)), Some((18, 0xf0)), Some((18, 0x0f))],
            group_value: [Some(19), Some(20), Some(21)],
            group_output_unreachable: [false, true, false],
            external_flash_compensation: Some(27),
            flash_exposure_comp3: Some(29),
            flash_exposure_comp4: Some(39),
            ..EMPTY
        }),
        // FlashInfo0106 (Df, D610, D3300, D5300, D7100, Coolpix A)
        "0106" => Some(Layout {
            flash_source: Some(4),
            external_flash_firmware: Some(6),
            external_flash_flags: Some(8),
            commander_and_control: Some(9),
            flash_focal_length: Some(12),
            repeating_flash_rate: Some(13),
            repeating_flash_count: Some(14),
            flash_gn_distance: Some(15),
            flash_color_filter: Some(16),
            group_control: [Some((17, 0x0f)), Some((18, 0xf0)), Some((18, 0x0f))],
            output_or_compensation: Some(39),
            group_value: [Some(40), Some(41), Some(42)],
            group_output_unreachable: [false, true, false],
            ..EMPTY
        }),
        // FlashInfo0107 (D4S, D750, D810, D5500, D7200) and 0108
        // (D5, D500, D850, D3400)
        "0107" | "0108" => Some(Layout {
            flash_source: Some(4),
            external_flash_firmware: Some(6),
            external_flash_zoom_override: Some(8),
            external_flash_status: Some(8),
            external_flash_ready_state: Some(9),
            compensation_only: Some(10),
            flash_focal_length: Some(12),
            repeating_flash_rate: Some(13),
            repeating_flash_count: Some(14),
            flash_gn_distance: Some(15),
            group_control: [Some((17, 0x0f)), Some((18, 0xf0)), Some((18, 0x0f))],
            group_value: [Some(40), Some(41), Some(42)],
            ..EMPTY
        }),
        // FlashInfo0300 (Z7II and later)
        "0300" | "0301" => Some(Layout {
            flash_source: Some(4),
            external_flash_firmware: Some(6),
            external_flash_flags: Some(8),
            flags_0300: true,
            commander_and_control: Some(9),
            compensation_only: Some(10),
            repeating_flash_rate: Some(13),
            repeating_flash_count: Some(14),
            flash_gn_distance: Some(15),
            flash_color_filter: Some(16),
            group_control: [Some((17, 0x0f)), Some((18, 0xf0)), Some((18, 0x0f))],
            output_only: Some(33),
            flash_illumination_pattern: Some(37),
            flash_focal_length: Some(38),
            group_value: [Some(40), Some(41), Some(42)],
            group_compensation_bias: 2,
            ..EMPTY
        }),
        _ => None,
    }
}

fn lookup(table: &[(u8, &str)], value: u8) -> String {
    match table.iter().find(|(k, _)| *k == value) {
        Some((_, name)) => (*name).to_string(),
        None => format!("Unknown ({})", value),
    }
}

/// ExifTool's BITMASK rendering: the set bit names joined with ", ", or
/// `(none)` when nothing is set.
fn print_bitmask(table: &[(u8, &str)], value: u8) -> String {
    let mut parts: Vec<String> = Vec::new();
    for bit in 0..8u8 {
        if value & (1 << bit) != 0 {
            match table.iter().find(|(k, _)| *k == bit) {
                Some((_, name)) => parts.push((*name).to_string()),
                None => parts.push(format!("Bit {}", bit)),
            }
        }
    }
    if parts.is_empty() {
        "(none)".to_string()
    } else {
        parts.join(", ")
    }
}

/// `Image::ExifTool::Exif::PrintFraction` -- the ±1/3 EV renderer the flash
/// compensations use.
fn print_fraction(value: f64) -> String {
    super::value_reader::print_fraction(value)
}

/// `$val>0.99 ? "Full" : sprintf("%.0f%%",$val*100)` over `2 ** (-$val/6)`.
fn print_flash_output(raw: u8) -> String {
    let value = 2f64.powf(-(raw as f64) / 6.0);
    if value > 0.99 {
        "Full".to_string()
    } else {
        format!("{:.0}%", value * 100.0)
    }
}

/// `$val ? sprintf("%+.1f",$val) : 0` over `-$val/6` (or `-($val-2)/6`).
fn print_group_compensation(raw: u8, bias: i32) -> String {
    let value = -((raw as i8 as i32 - bias) as f64) / 6.0;
    if value == 0.0 {
        "0".to_string()
    } else {
        format!("{:+.1}", value)
    }
}

/// Walk `Nikon::Main` 0x00a8.
pub fn parse_flash_info(data: &[u8], tags: &mut HashMap<String, String>) {
    if data.len() < 4 {
        return;
    }
    let version = ascii_value(&data[..4]);
    tags.insert("Nikon:FlashInfoVersion".to_string(), version.clone());
    let Some(l) = layout_for(&version) else {
        // FlashInfoUnknown reports the version and nothing else.
        return;
    };
    let byte = |at: Option<usize>| at.and_then(|at| data.get(at)).copied();
    // 0103 and later also treat 255 as "no value" for the repeating-flash trio.
    let drops_255 = !matches!(version.as_str(), "0100" | "0101" | "0102");

    if let Some(raw) = byte(l.flash_source) {
        tags.insert("Nikon:FlashSource".to_string(), lookup(FLASH_SOURCE, raw));
    }
    if let Some(at) = l.external_flash_firmware
        && let Some(pair) = data.get(at..at + 2)
    {
        let key = format!("{} {}", pair[0], pair[1]);
        let printed = FLASH_FIRMWARE
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_string())
            .unwrap_or_else(|| format!("Unknown ({})", key));
        tags.insert("Nikon:ExternalFlashFirmware".to_string(), printed);
    }
    if let Some(raw) = byte(l.external_flash_flags) {
        let table = if l.flags_0300 {
            EXTERNAL_FLASH_FLAGS_0300
        } else {
            EXTERNAL_FLASH_FLAGS
        };
        tags.insert(
            "Nikon:ExternalFlashFlags".to_string(),
            print_bitmask(table, raw),
        );
    }
    if let Some(raw) = byte(l.external_flash_zoom_override) {
        tags.insert(
            "Nikon:ExternalFlashZoomOverride".to_string(),
            lookup(NO_YES, (raw & 0x80) >> 7),
        );
    }
    if let Some(raw) = byte(l.external_flash_status) {
        tags.insert(
            "Nikon:ExternalFlashStatus".to_string(),
            lookup(EXTERNAL_FLASH_STATUS, raw & 0x01),
        );
    }
    if let Some(raw) = byte(l.external_flash_ready_state) {
        tags.insert(
            "Nikon:ExternalFlashReadyState".to_string(),
            lookup(EXTERNAL_FLASH_READY_STATE, raw & 0x07),
        );
    }

    let mut control_mode = 0u8;
    if let Some(raw) = byte(l.commander_and_control) {
        tags.insert(
            "Nikon:FlashCommanderMode".to_string(),
            lookup(OFF_ON, (raw & 0x80) >> 7),
        );
        control_mode = raw & 0x7f;
        tags.insert(
            "Nikon:FlashControlMode".to_string(),
            lookup(FLASH_CONTROL_MODE, control_mode),
        );
    }
    // One byte, two tags: ExifTool takes the first whose Condition holds.
    if let Some(raw) = byte(l.output_or_compensation) {
        if control_mode >= 0x06 {
            tags.insert("Nikon:FlashOutput".to_string(), print_flash_output(raw));
        } else {
            tags.insert(
                "Nikon:FlashCompensation".to_string(),
                print_fraction(-(raw as i8 as f64) / 6.0),
            );
        }
    }
    // 0300 keeps them apart, and gates the compensation on the control mode.
    if let Some(raw) = byte(l.compensation_only)
        && (l.output_only.is_none() || matches!(control_mode, 0x01 | 0x02 | 0x03 | 0x04 | 0x05))
    {
        tags.insert(
            "Nikon:FlashCompensation".to_string(),
            print_fraction(-(raw as i8 as f64) / 6.0),
        );
    }
    if let Some(raw) = byte(l.output_only)
        && control_mode >= 0x06
    {
        tags.insert("Nikon:FlashOutput".to_string(), print_flash_output(raw));
    }
    // RawConv `$val ? $val : undef` on 0100/0102, and
    // `($val and $val != 255) ? $val : undef` from 0103 on. A body with no
    // repeating flash writes zeros here, and ExifTool reports nothing rather
    // than "0 Hz" -- which matters because the D80's real RepeatingFlashRate
    // comes from the (encrypted) NikonCustom block instead.
    let live = |raw: u8| raw != 0 && !(raw == 255 && drops_255);
    if let Some(raw) = byte(l.flash_focal_length)
        && live(raw)
    {
        tags.insert("Nikon:FlashFocalLength".to_string(), format!("{} mm", raw));
    }
    if let Some(raw) = byte(l.repeating_flash_rate)
        && live(raw)
    {
        tags.insert(
            "Nikon:RepeatingFlashRate".to_string(),
            format!("{} Hz", raw),
        );
    }
    if let Some(raw) = byte(l.repeating_flash_count)
        && live(raw)
    {
        tags.insert("Nikon:RepeatingFlashCount".to_string(), raw.to_string());
    }
    if let Some(raw) = byte(l.flash_gn_distance) {
        tags.insert(
            "Nikon:FlashGNDistance".to_string(),
            lookup(FLASH_GN_DISTANCE, raw),
        );
    }
    if let Some(raw) = byte(l.flash_color_filter) {
        tags.insert(
            "Nikon:FlashColorFilter".to_string(),
            lookup(FLASH_COLOR_FILTER, raw),
        );
    }
    if let Some(raw) = byte(l.flash_illumination_pattern) {
        tags.insert(
            "Nikon:FlashIlluminationPattern".to_string(),
            lookup(FLASH_ILLUMINATION_PATTERN, raw),
        );
    }

    for (i, label) in ["A", "B", "C"].iter().enumerate() {
        let Some((at, mask)) = l.group_control[i] else {
            continue;
        };
        let Some(raw) = data.get(at).copied() else {
            continue;
        };
        // Mask, then shift past its low bit -- ExifTool's BitShift.
        let mode = (raw & mask) >> mask.trailing_zeros();
        tags.insert(
            format!("Nikon:FlashGroup{}ControlMode", label),
            lookup(FLASH_CONTROL_MODE, mode),
        );
        let Some(value_at) = l.group_value[i] else {
            continue;
        };
        let Some(value) = data.get(value_at).copied() else {
            continue;
        };
        if mode >= 0x06 && !l.group_output_unreachable[i] {
            tags.insert(
                format!("Nikon:FlashGroup{}Output", label),
                print_flash_output(value),
            );
        } else {
            tags.insert(
                format!("Nikon:FlashGroup{}Compensation", label),
                print_group_compensation(value, l.group_compensation_bias),
            );
        }
    }

    for (at, name) in [
        (l.external_flash_compensation, "ExternalFlashCompensation"),
        (l.flash_exposure_comp3, "FlashExposureComp3"),
        (l.flash_exposure_comp4, "FlashExposureComp4"),
    ] {
        if let Some(raw) = byte(at) {
            tags.insert(
                format!("Nikon:{}", name),
                print_fraction(-(raw as i8 as f64) / 6.0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmask_renders_like_exiftool() {
        assert_eq!(print_bitmask(EXTERNAL_FLASH_FLAGS, 0), "(none)");
        assert_eq!(print_bitmask(EXTERNAL_FLASH_FLAGS, 1), "Fired");
        assert_eq!(
            print_bitmask(EXTERNAL_FLASH_FLAGS, 0b0001_0101),
            "Fired, Bounce Flash, Wide Flash Adapter"
        );
    }

    #[test]
    fn flash_output_matches_the_perl_expression() {
        assert_eq!(print_flash_output(0), "Full");
        assert_eq!(print_flash_output(6), "50%");
        assert_eq!(print_flash_output(12), "25%");
    }

    #[test]
    fn unknown_version_reports_only_the_version() {
        let mut tags = HashMap::new();
        parse_flash_info(b"0999\x01\x02\x03\x04", &mut tags);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags["Nikon:FlashInfoVersion"], "0999");
    }

    #[test]
    fn group_control_mask_is_shifted() {
        // 0103: byte 18 holds B in the high nibble and C in the low one.
        let mut data = vec![0u8; 48];
        data[..4].copy_from_slice(b"0103");
        data[18] = 0x62; // B = 6 (Manual), C = 2 (iTTL)
        let mut tags = HashMap::new();
        parse_flash_info(&data, &mut tags);
        assert_eq!(tags["Nikon:FlashGroupBControlMode"], "Manual");
        assert_eq!(tags["Nikon:FlashGroupCControlMode"], "iTTL");
    }
}
