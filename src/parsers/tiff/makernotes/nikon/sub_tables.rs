//! The unencrypted `ProcessBinaryData` tables hanging off `Nikon::Main`.
//!
//! Everything here is a flat record read at fixed offsets from a block that
//! Nikon writes in the clear. The encrypted tables (ShotInfo, the 02xx+
//! ColorBalance and LensData layouts, and the NikonCustom settings that live
//! inside ShotInfo) are deliberately absent: without running Nikon's
//! serial-number key schedule those bytes decode to noise, and a plausible
//! wrong value is worse than a missing one.
//!
//! Each function names the ExifTool table it mirrors so the offsets can be
//! checked against `Image/ExifTool/Nikon.pm`.

use std::collections::HashMap;

use super::value_reader::{ascii_value, format_string, read_u16, read_u32};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// `%retouchValues` -- shared by all ten `RetouchHistory` elements.
#[rustfmt::skip]
const RETOUCH_VALUES: &[(u16, &str)] = &[
    (0, "None"), (3, "B & W"), (4, "Sepia"), (5, "Trim"), (6, "Small Picture"),
    (7, "D-Lighting"), (8, "Red Eye"), (9, "Cyanotype"), (10, "Sky Light"),
    (11, "Warm Tone"), (12, "Color Custom"), (13, "Image Overlay"),
    (14, "Red Intensifier"), (15, "Green Intensifier"), (16, "Blue Intensifier"),
    (17, "Cross Screen"), (18, "Quick Retouch"), (19, "NEF Processing"),
    (23, "Distortion Control"), (25, "Fisheye"), (26, "Straighten"),
    (29, "Perspective Control"), (30, "Color Outline"), (31, "Soft Filter"),
    (32, "Resize"), (33, "Miniature Effect"), (34, "Skin Softening"),
    (35, "Selected Frame"), (37, "Color Sketch"), (38, "Selective Color"),
    (39, "Glamour"), (40, "Drawing"), (44, "Pop"), (45, "Toy Camera Effect 1"),
    (46, "Toy Camera Effect 2"), (47, "Cross Process (red)"),
    (48, "Cross Process (blue)"), (49, "Cross Process (green)"),
    (50, "Cross Process (yellow)"), (51, "Super Vivid"),
    (52, "High-contrast Monochrome"), (53, "High Key"), (54, "Low Key"),
];

/// `%Image::ExifTool::Nikon::PictureControl` 0x37 (and 0x3f/0x47 in V2/V3).
#[rustfmt::skip]
const FILTER_EFFECT: &[(u8, &str)] = &[
    (0x80, "Off"), (0x81, "Yellow"), (0x82, "Orange"), (0x83, "Red"),
    (0x84, "Green"), (0xff, "n/a"),
];

/// `%Image::ExifTool::Nikon::PictureControl` 0x38 (and 0x40/0x48 in V2/V3).
#[rustfmt::skip]
const TONING_EFFECT: &[(u8, &str)] = &[
    (0x80, "B&W"), (0x81, "Sepia"), (0x82, "Cyanotype"), (0x83, "Red"),
    (0x84, "Yellow"), (0x85, "Green"), (0x86, "Blue-green"), (0x87, "Blue"),
    (0x88, "Purple-blue"), (0x89, "Red-purple"), (0xff, "n/a"),
];

const PICTURE_CONTROL_ADJUST: &[(u8, &str)] = &[
    (0, "Default Settings"),
    (1, "Quick Adjust"),
    (2, "Full Control"),
];

/// Keep the value already recorded for `key` unless it is empty, matching the
/// way ExifTool resolves two tags that share a name.
pub fn prefer_existing(tags: &mut HashMap<String, String>, key: &str, value: String) {
    match tags.get(key) {
        Some(existing) if !existing.is_empty() => {}
        _ => {
            tags.insert(key.to_string(), value);
        }
    }
}

/// ExifTool prints an unlisted PrintConv code as `Unknown (n)`, or as
/// `Unknown (0xnn)` when the tag carries `PrintHex`.
fn lookup_u8(table: &[(u8, &str)], value: u8, print_hex: bool) -> String {
    match table.iter().find(|(k, _)| *k == value) {
        Some((_, name)) => (*name).to_string(),
        None if print_hex => format!("Unknown (0x{:x})", value),
        None => format!("Unknown ({})", value),
    }
}

fn lookup_u16(table: &[(u16, &str)], value: u16) -> String {
    match table.iter().find(|(k, _)| *k == value) {
        Some((_, name)) => (*name).to_string(),
        None => format!("Unknown ({})", value),
    }
}

/// The `$fmt` argument callers pass to `PrintPC`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PcFormat {
    /// The default `%+d`.
    Signed,
    /// `"%d"` -- only PictureControl V1's Sharpness, which prints `3`, not `+3`.
    Plain,
    /// `"%.2f"` with a divisor of 4, used throughout V2 and V3.
    Quarters,
}

/// `Image::ExifTool::Nikon::PrintPC` -- the shared PictureControl scale
/// printer. `norm` is the label for 0 (`Normal` when the caller passes undef).
fn print_pc(value: i32, norm: &str, fmt: PcFormat) -> String {
    match value {
        0 => norm.to_string(),
        0x7f => "n/a".to_string(),
        -128 => "Auto".to_string(),
        -127 => "User".to_string(),
        _ => match fmt {
            PcFormat::Signed => format!("{:+}", value),
            PcFormat::Plain => value.to_string(),
            PcFormat::Quarters => format!("{:.2}", value as f64 / 4.0),
        },
    }
}

/// The `$val - 0x80` ValueConv every PictureControl scale carries.
fn pc_value(byte: u8) -> i32 {
    byte as i32 - 0x80
}

/// `Nikon::PictureControl` / `PictureControl2` / `PictureControl3`
/// (`Nikon::Main` 0x0023, and 0x00bd on the P6000).
///
/// The three layouts differ only in where the fields sit and in whether the
/// scales print as `%+d` or `%.2f` over four steps, so one walker with an
/// offset table covers all three.
pub fn parse_picture_control(data: &[u8], tags: &mut HashMap<String, String>) {
    if data.len() < 4 {
        return;
    }
    let version = ascii_value(&data[..4]);
    tags.insert("Nikon:PictureControlVersion".to_string(), version.clone());
    // Sharpness, Saturation, HueAdjustment, Contrast, Brightness and
    // ToningEffect also exist as plain tags in `Nikon::Main`. When both are
    // present ExifTool keeps the first one it extracted -- unless that one is
    // empty, in which case the later value is promoted. (CoolpixP330 reports
    // the main table's `Sharpness: Normal` over PictureControl's `n/a`, while
    // CoolpixP510 reports PictureControl's `ToningEffect: n/a` over the main
    // table's empty string.)
    fn put(tags: &mut HashMap<String, String>, name: &str, value: String) {
        prefer_existing(tags, &format!("Nikon:{}", name), value);
    }

    // `scales` is (offset, tag name, zero label) in table order. V1 prints
    // %+d and V2/V3 print %.2f over a divisor of 4, with V1's Sharpness the
    // one field that asks for a bare %d.
    let (name_at, base_at, adjust_at, quick_at, scales, filter_at, quarters): (
        usize,
        usize,
        usize,
        usize,
        &[(usize, &str, &str)],
        usize,
        bool,
    ) = if version.starts_with("01") {
        (
            4,
            24,
            48,
            49,
            &[
                (50, "Sharpness", "No Sharpening"),
                (51, "Contrast", "Normal"),
                (52, "Brightness", "Normal"),
                (53, "Saturation", "Normal"),
                (54, "HueAdjustment", "None"),
            ],
            55,
            false,
        )
    } else if version.starts_with("02") {
        (
            4,
            24,
            48,
            49,
            &[
                (51, "Sharpness", "None"),
                (53, "Clarity", "None"),
                (55, "Contrast", "None"),
                (57, "Brightness", "Normal"),
                (59, "Saturation", "None"),
                (61, "Hue", "None"),
            ],
            63,
            true,
        )
    } else if version.starts_with("03") {
        (
            8,
            28,
            54,
            55,
            &[
                (57, "Sharpness", "None"),
                (59, "MidRangeSharpness", "None"),
                (61, "Clarity", "None"),
                (63, "Contrast", "None"),
                (65, "Brightness", "Normal"),
                (67, "Saturation", "None"),
                (69, "Hue", "None"),
            ],
            71,
            true,
        )
    } else {
        // PictureControlUnknown: ExifTool reports only the version.
        return;
    };

    // string[20], run through the table's FormatString PrintConv.
    for (at, name) in [
        (name_at, "PictureControlName"),
        (base_at, "PictureControlBase"),
    ] {
        if let Some(bytes) = data.get(at..at + 20) {
            put(tags, name, format_string(&ascii_value(bytes)));
        }
    }
    if let Some(&raw) = data.get(adjust_at) {
        put(
            tags,
            "PictureControlAdjust",
            lookup_u8(PICTURE_CONTROL_ADJUST, raw, false),
        );
    }
    // PictureControlQuickAdjust always prints %+d, even in the V2/V3 tables
    // where the other scales switch to %.2f.
    if let Some(&raw) = data.get(quick_at) {
        put(
            tags,
            "PictureControlQuickAdjust",
            print_pc(pc_value(raw), "Normal", PcFormat::Signed),
        );
    }
    for (at, name, norm) in scales {
        if let Some(&raw) = data.get(*at) {
            // V1's Sharpness is the sole `"%d"` caller in the three tables.
            let fmt = if quarters {
                PcFormat::Quarters
            } else if *name == "Sharpness" {
                PcFormat::Plain
            } else {
                PcFormat::Signed
            };
            put(tags, name, print_pc(pc_value(raw), norm, fmt));
        }
    }
    if let Some(&raw) = data.get(filter_at) {
        put(tags, "FilterEffect", lookup_u8(FILTER_EFFECT, raw, true));
    }
    if let Some(&raw) = data.get(filter_at + 1) {
        put(tags, "ToningEffect", lookup_u8(TONING_EFFECT, raw, true));
    }
    // V1's ToningSaturation is the one field that does not go through PrintPC:
    // its PrintConv is `$val==0x7f ? "n/a" : $val`, so a zero prints as `0`
    // rather than as a `Normal` label. V2 and V3 use the ordinary scale.
    if let Some(&raw) = data.get(filter_at + 2) {
        let value = pc_value(raw);
        let printed = if quarters {
            print_pc(value, "None", PcFormat::Quarters)
        } else if value == 0x7f {
            "n/a".to_string()
        } else {
            value.to_string()
        };
        put(tags, "ToningSaturation", printed);
    }
}

/// `Nikon::FileInfo` (`Nikon::Main` 0x00b8), FORMAT int16u.
///
/// The block's byte order is not the MakerNote's: ExifTool picks whichever of
/// the two yields a DirectoryNumber in 100..=999 and a FileNumber <= 9999, and
/// only falls back to a model list when both or neither do.
pub fn parse_file_info(
    data: &[u8],
    order: ByteOrder,
    model: Option<&str>,
    tags: &mut HashMap<String, String>,
) {
    if data.len() < 10 {
        return;
    }
    let plausible = |order: ByteOrder| -> bool {
        let (Some(dir), Some(file)) = (read_u16(data, 6, order), read_u16(data, 8, order)) else {
            return false;
        };
        (100..=999).contains(&dir) && file <= 9999
    };
    let little = plausible(ByteOrder::LittleEndian);
    let big = plausible(ByteOrder::BigEndian);
    let chosen = if little != big {
        if little {
            ByteOrder::LittleEndian
        } else {
            ByteOrder::BigEndian
        }
    } else if model.is_some_and(|m| {
        matches!(
            m,
            "NIKON D4S"
                | "NIKON D750"
                | "NIKON D810"
                | "NIKON D3300"
                | "NIKON D5200"
                | "NIKON D5300"
                | "NIKON D5500"
                | "NIKON D7100"
        )
    }) {
        ByteOrder::LittleEndian
    } else {
        // The MakerNote's own order is irrelevant here; ExifTool's fallback
        // variant pins BigEndian.
        let _ = order;
        ByteOrder::BigEndian
    };

    tags.insert("Nikon:FileInfoVersion".to_string(), ascii_value(&data[..4]));
    if let Some(value) = read_u16(data, 4, chosen) {
        tags.insert("Nikon:MemoryCardNumber".to_string(), value.to_string());
    }
    if let Some(value) = read_u16(data, 6, chosen) {
        tags.insert("Nikon:DirectoryNumber".to_string(), format!("{:03}", value));
    }
    if let Some(value) = read_u16(data, 8, chosen) {
        tags.insert("Nikon:FileNumber".to_string(), format!("{:04}", value));
    }
}

/// `Nikon::MakerNotes0x56` (Main tag 0x0056), the Z-series burst record.
///
/// The packed word at offset four is only meaningful when it is non-zero;
/// ExifTool uses that same value as the `BurstFlag` condition for the five
/// burst fields.  The final word is independently reported as pixel shift.
pub fn parse_maker_notes_0x56(data: &[u8], order: ByteOrder, tags: &mut HashMap<String, String>) {
    if data.len() < 4 {
        return;
    }
    let firmware = ascii_value(&data[..4]);
    if firmware.len() == 4 && firmware.as_bytes().iter().all(u8::is_ascii_digit) {
        tags.insert(
            "Nikon:FirmwareVersion56".to_string(),
            format!("{}.{}", &firmware[..2], &firmware[2..]),
        );
    }

    let burst = read_u32(data, 4, order).unwrap_or(0);
    if burst != 0 {
        tags.insert(
            "Nikon:BurstStartSlotNumber".to_string(),
            (((burst & 0x2000_0000) >> 29) + 1).to_string(),
        );
        tags.insert(
            "Nikon:BurstStartFolderNumber".to_string(),
            ((burst & 0x1ff8_0000) >> 19).to_string(),
        );
        tags.insert(
            "Nikon:BurstStartImageNumber".to_string(),
            ((burst & 0x0007_ffe0) >> 5).to_string(),
        );
        if let Some(kind) = match burst & 0x1f {
            0 => Some("JPG"),
            2 => Some("NEF"),
            3 => Some("TIF"),
            4 => Some("NDF"),
            5 => Some("MOV"),
            6 => Some("NEV"),
            7 => Some("MP4"),
            _ => None,
        } {
            tags.insert("Nikon:BurstStartImageType".to_string(), kind.to_string());
        }
        if let Some(number) = read_u32(data, 8, order) {
            tags.insert("Nikon:BurstShotNumber".to_string(), number.to_string());
        }
    }
    if let Some(active) = read_u32(data, 12, order) {
        if let Some(printed) = match active {
            0 => Some("No"),
            1 => Some("Yes"),
            _ => None,
        } {
            tags.insert("Nikon:PixelShiftActive".to_string(), printed.to_string());
        }
    }
}

/// `Nikon::AFTune` (`Nikon::Main` 0x00b9).
pub fn parse_af_tune(data: &[u8], tags: &mut HashMap<String, String>) {
    const AF_FINE_TUNE: &[(u8, &str)] =
        &[(0, "Off"), (1, "On (1)"), (2, "On (2)"), (3, "On (Zoom)")];
    if let Some(&raw) = data.first() {
        tags.insert(
            "Nikon:AFFineTune".to_string(),
            lookup_u8(AF_FINE_TUNE, raw, false),
        );
    }
    if let Some(&raw) = data.get(1) {
        let printed = if raw == 255 {
            "n/a".to_string()
        } else {
            raw.to_string()
        };
        tags.insert("Nikon:AFFineTuneIndex".to_string(), printed);
    }
    // int8s, PrintConv '$val > 0 ? "+$val" : $val'
    for (at, name) in [(2usize, "AFFineTuneAdj"), (3, "AFFineTuneAdjTele")] {
        if let Some(&raw) = data.get(at) {
            let value = raw as i8;
            let printed = if value > 0 {
                format!("+{}", value)
            } else {
                value.to_string()
            };
            tags.insert(format!("Nikon:{}", name), printed);
        }
    }
}

/// `Nikon::RetouchInfo` (`Nikon::Main` 0x00bb), FORMAT int8s.
pub fn parse_retouch_info(data: &[u8], tags: &mut HashMap<String, String>) {
    if data.len() < 4 {
        return;
    }
    let version = ascii_value(&data[..4]);
    tags.insert("Nikon:RetouchInfoVersion".to_string(), version.clone());
    // Condition => '$$self{RetouchInfoVersion} ge "0200"' -- a string compare.
    if version.as_str() < "0200" {
        return;
    }
    if let Some(&raw) = data.get(5) {
        let printed = match raw as i8 {
            -1 => "Off".to_string(),
            1 => "On".to_string(),
            other => format!("Unknown ({})", other),
        };
        tags.insert("Nikon:RetouchNEFProcessing".to_string(), printed);
    }
}

/// `Nikon::MultiExposure` and `MultiExposure2` (`Nikon::Main` 0x00b0),
/// FORMAT int32u. Version 0100/0101 is the first table, 0102/0103 the second;
/// they share the first two fields and diverge at index 3.
pub fn parse_multi_exposure(data: &[u8], order: ByteOrder, tags: &mut HashMap<String, String>) {
    if data.len() < 4 {
        return;
    }
    let version = ascii_value(&data[..4]);
    tags.insert("Nikon:MultiExposureVersion".to_string(), version.clone());
    let v2 = matches!(version.as_str(), "0102" | "0103");
    if !v2 && !matches!(version.as_str(), "0100" | "0101") {
        return;
    }
    // 0101 pins LittleEndian in ExifTool's SubDirectory; the others follow the
    // MakerNote.
    let order = if version == "0101" {
        ByteOrder::LittleEndian
    } else {
        order
    };
    if let Some(raw) = read_u32(data, 4, order) {
        let mode = match (raw, v2) {
            (0, _) => "Off".to_string(),
            (1, _) => "Multiple Exposure".to_string(),
            (2, false) => "Image Overlay".to_string(),
            (3, _) => "HDR".to_string(),
            (other, _) => format!("Unknown ({})", other),
        };
        tags.insert("Nikon:MultiExposureMode".to_string(), mode);
    }
    if let Some(raw) = read_u32(data, 8, order) {
        tags.insert("Nikon:MultiExposureShots".to_string(), raw.to_string());
    }
    if let Some(raw) = read_u32(data, 12, order) {
        if v2 {
            let printed = match raw {
                0 => "Add".to_string(),
                1 => "Average".to_string(),
                2 => "Light".to_string(),
                3 => "Dark".to_string(),
                other => format!("Unknown ({})", other),
            };
            tags.insert("Nikon:MultiExposureOverlayMode".to_string(), printed);
        } else {
            let printed = match raw {
                0 => "Off".to_string(),
                1 => "On".to_string(),
                other => format!("Unknown ({})", other),
            };
            tags.insert("Nikon:MultiExposureAutoGain".to_string(), printed);
        }
    }
}

/// `Nikon::HDRInfo` / `HDRInfo2` (`Nikon::Main` 0x0035). ExifTool picks the
/// second table when the value is exactly six bytes.
pub fn parse_hdr_info(data: &[u8], tags: &mut HashMap<String, String>) {
    if data.len() < 4 {
        return;
    }
    tags.insert("Nikon:HDRInfoVersion".to_string(), ascii_value(&data[..4]));
    if data.len() == 6 {
        const HDR: &[(u8, &str)] = &[(0, "Off"), (1, "On (normal)")];
        const LEVEL: &[(u8, &str)] = &[
            (0, "n/a"),
            (1, "Normal"),
            (2, "Low"),
            (3, "High"),
            (4, "High+"),
            (5, "Auto"),
        ];
        if let Some(&raw) = data.get(4) {
            tags.insert("Nikon:HDR".to_string(), lookup_u8(HDR, raw, false));
        }
        if let Some(&raw) = data.get(5) {
            tags.insert("Nikon:HDRLevel".to_string(), lookup_u8(LEVEL, raw, false));
        }
        return;
    }
    const HDR: &[(u8, &str)] = &[(0, "Off"), (1, "On (normal)"), (48, "Auto")];
    const LEVEL: &[(u8, &str)] = &[
        (0, "Auto"),
        (1, "1 EV"),
        (2, "2 EV"),
        (3, "3 EV"),
        (255, "n/a"),
    ];
    const SMOOTHING: &[(u8, &str)] = &[
        (0, "Off"),
        (1, "Normal"),
        (2, "Low"),
        (3, "High"),
        (48, "Auto"),
        (255, "n/a"),
    ];
    if let Some(&raw) = data.get(4) {
        tags.insert("Nikon:HDR".to_string(), lookup_u8(HDR, raw, false));
    }
    if let Some(&raw) = data.get(5) {
        tags.insert("Nikon:HDRLevel".to_string(), lookup_u8(LEVEL, raw, false));
    }
    if let Some(&raw) = data.get(6) {
        tags.insert(
            "Nikon:HDRSmoothing".to_string(),
            lookup_u8(SMOOTHING, raw, false),
        );
    }
    if let Some(&raw) = data.get(7) {
        tags.insert("Nikon:HDRLevel2".to_string(), lookup_u8(LEVEL, raw, false));
    }
}

/// `Nikon::LocationInfo` (`Nikon::Main` 0x0039).
///
/// `Location` itself is only emitted for the ASCII encodings: the UTF16 form
/// needs ExifTool's `Decode`, and guessing at it would produce mojibake rather
/// than a missing tag.
pub fn parse_location_info(data: &[u8], tags: &mut HashMap<String, String>) {
    if data.len() < 5 {
        return;
    }
    tags.insert(
        "Nikon:LocationInfoVersion".to_string(),
        ascii_value(&data[..4]),
    );
    let encoding = data[4];
    let printed = match encoding {
        0 => "n/a".to_string(),
        1 => "UTF8".to_string(),
        2 => "UTF16".to_string(),
        other => format!("Unknown ({})", other),
    };
    tags.insert("Nikon:TextEncoding".to_string(), printed);
    // undef[3], ValueConv truncates at the first NUL.
    if let Some(bytes) = data.get(5..8) {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        tags.insert(
            "Nikon:CountryCode".to_string(),
            String::from_utf8_lossy(&bytes[..end]).to_string(),
        );
    }
    if let Some(&raw) = data.get(8) {
        tags.insert("Nikon:POILevel".to_string(), raw.to_string());
    }
    if encoding <= 1
        && let Some(bytes) = data.get(9..)
    {
        let bytes = &bytes[..bytes.len().min(70)];
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        tags.insert(
            "Nikon:Location".to_string(),
            String::from_utf8_lossy(&bytes[..end]).to_string(),
        );
    }
}

/// `Nikon::BarometerInfo` (`Nikon::Main` 0x00c3).
pub fn parse_barometer_info(data: &[u8], order: ByteOrder, tags: &mut HashMap<String, String>) {
    if data.len() < 4 {
        return;
    }
    tags.insert(
        "Nikon:BarometerInfoVersion".to_string(),
        ascii_value(&data[..4]),
    );
    if let Some(raw) = read_u32(data, 6, order) {
        tags.insert("Nikon:Altitude".to_string(), format!("{} m", raw as i32));
    }
}

/// `Nikon::DistortInfo` (`Nikon::Main` 0x002b). `DistortionVersion` is
/// `Unknown => 1`, so ExifTool reports only the one field.
pub fn parse_distort_info(data: &[u8], tags: &mut HashMap<String, String>) {
    const CONTROL: &[(u8, &str)] = &[(0, "Off"), (1, "On"), (2, "On (underwater)")];
    if let Some(&raw) = data.get(4) {
        tags.insert(
            "Nikon:AutoDistortionControl".to_string(),
            lookup_u8(CONTROL, raw, false),
        );
    }
}

/// `Nikon::FaceDetect` (`Nikon::Main` 0x0021), FORMAT int16u.
///
/// `FIRST_ENTRY => 0` and the index is in int16u units, so entry `n` sits at
/// byte `2n`. Each FaceNPosition is suppressed unless FacesDetected reaches it.
pub fn parse_face_detect(data: &[u8], order: ByteOrder, tags: &mut HashMap<String, String>) {
    let u16_at = |index: usize| read_u16(data, index * 2, order);
    if let (Some(w), Some(h)) = (u16_at(1), u16_at(2)) {
        tags.insert(
            "Nikon:FaceDetectFrameSize".to_string(),
            format!("{} {}", w, h),
        );
    }
    let Some(faces) = u16_at(3) else {
        return;
    };
    tags.insert("Nikon:FacesDetected".to_string(), faces.to_string());
    for n in 1..=12u16 {
        if faces < n {
            break;
        }
        let base = 4 + (n as usize - 1) * 4;
        let coords: Vec<String> = (0..4)
            .filter_map(|i| u16_at(base + i))
            .map(|v| v.to_string())
            .collect();
        if coords.len() == 4 {
            tags.insert(format!("Nikon:Face{}Position", n), coords.join(" "));
        }
    }
}

/// `%infoZSeries` in Nikon.pm -- the Condition several tables use to pick a
/// Z-body variant of a PrintConv.
///
/// `/^NIKON Z (30|5|50|6|6_2|7|7_2|8|f|fc|9)\b/i` or `/^NIKON Z(5_2|50_2|6_3)\b/i`
pub fn is_z_series(model: Option<&str>) -> bool {
    let Some(model) = model else {
        return false;
    };
    let upper = model.to_ascii_uppercase();
    let word_end = |tail: &str| !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_');
    if let Some(rest) = upper.strip_prefix("NIKON Z ") {
        for v in ["30", "5", "50", "6", "6_2", "7", "7_2", "8", "F", "FC", "9"] {
            if let Some(tail) = rest.strip_prefix(v)
                && word_end(tail)
            {
                return true;
            }
        }
    }
    if let Some(rest) = upper.strip_prefix("NIKON Z") {
        for v in ["5_2", "50_2", "6_3"] {
            if let Some(tail) = rest.strip_prefix(v)
                && word_end(tail)
            {
                return true;
            }
        }
    }
    false
}

/// `Nikon::Main` 0x0097 `ColorBalance*`.
///
/// Three of the layouts are stored in the clear and the rest are encrypted
/// with a SerialNumber/ShutterCount key. For the encrypted ones ExifTool falls
/// through to a table that reports only the plaintext version string, and so
/// does this -- inventing WB levels from ciphertext would be worse than
/// leaving them out.
pub fn parse_color_balance(data: &[u8], order: ByteOrder, tags: &mut HashMap<String, String>) {
    let version = ascii_value(&data[..4]);
    let levels = |at: usize, name: &str, tags: &mut HashMap<String, String>| {
        let values: Vec<String> = (0..4)
            .filter_map(|i| read_u16(data, at + i * 2, order))
            .map(|v| v.to_string())
            .collect();
        if values.len() == 4 {
            tags.insert(format!("Nikon:{}", name), values.join(" "));
        }
    };
    match version.as_str() {
        // ColorBalance1, Start => $valuePtr + 72 (D100 and Coolpix).
        "0100" => levels(72, "WB_RBGGLevels", tags),
        // ColorBalance2, Start => $valuePtr + 10 (D2H).
        "0102" => levels(10, "WB_RGGBLevels", tags),
        // ColorBalance3, Start => $valuePtr + 20 (D70/D70s).
        "0103" => levels(20, "WB_RGBGLevels", tags),
        _ => {
            // Everything else goes through ProcessNikonEncrypted. ExifTool
            // reports ColorBalanceVersion only for the versions with no
            // decryptable table behind them.
            if !color_balance_has_table(&version) {
                tags.insert("Nikon:ColorBalanceVersion".to_string(), version);
            }
        }
    }
}

/// The `$$valPt` conditions on `Nikon::Main` 0x0097, in ExifTool's order.
/// True means a real ColorBalance table claims this version, so no bare
/// `ColorBalanceVersion` is emitted for it.
fn color_balance_has_table(version: &str) -> bool {
    if version.len() < 4 || !version.is_char_boundary(2) {
        return false;
    }
    match version {
        "0205" | "0209" | "0212" | "0214" | "0211" | "0213" => return true,
        "0215" | "0216" | "0217" => return true,
        "0219" | "0221" | "0222" | "0223" | "0224" => return true,
        _ => {}
    }
    // '/^02(\d{2})/ and $1 < 11'
    if let Some(rest) = version.strip_prefix("02")
        && let Ok(n) = rest[..2].parse::<u32>()
    {
        return n < 11;
    }
    false
}

/// `Nikon::Main` 0x009e `RetouchHistory`: int16u[10], the trailing `None`
/// entries trimmed off by ValueConv before each remaining element is looked up
/// in `%retouchValues`.
pub fn print_retouch_history(values: &[u16]) -> Option<String> {
    let mut end = values.len();
    while end > 1 && values[end - 1] == 0 {
        end -= 1;
    }
    let kept = values.get(..end)?;
    if kept.is_empty() {
        return None;
    }
    Some(
        kept.iter()
            .map(|v| lookup_u16(RETOUCH_VALUES, *v))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

/// `Nikon::Main` 0x00b6 `PowerUpTime`: an int16u year followed by five bytes.
pub fn print_power_up_time(bytes: &[u8], order: ByteOrder) -> Option<String> {
    if bytes.len() < 7 {
        return None;
    }
    let year = read_u16(bytes, 0, order)?;
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        year, bytes[2], bytes[3], bytes[4], bytes[5], bytes[6]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_pc_matches_exiftool_special_cases() {
        assert_eq!(print_pc(0, "None", PcFormat::Signed), "None");
        assert_eq!(print_pc(0x7f, "None", PcFormat::Signed), "n/a");
        assert_eq!(print_pc(-128, "None", PcFormat::Signed), "Auto");
        assert_eq!(print_pc(-127, "None", PcFormat::Signed), "User");
        assert_eq!(print_pc(3, "None", PcFormat::Signed), "+3");
        // PictureControl V1 Sharpness asks for "%d": NikonD3400 prints 3.
        assert_eq!(print_pc(3, "No Sharpening", PcFormat::Plain), "3");
        assert_eq!(print_pc(2, "None", PcFormat::Quarters), "0.50");
        assert_eq!(print_pc(-6, "None", PcFormat::Quarters), "-1.50");
    }

    #[test]
    fn retouch_history_trims_trailing_none() {
        assert_eq!(
            print_retouch_history(&[7, 18, 0, 0, 0, 0, 0, 0, 0, 0]).as_deref(),
            Some("D-Lighting; Quick Retouch")
        );
        assert_eq!(
            print_retouch_history(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).as_deref(),
            Some("None")
        );
    }

    #[test]
    fn file_info_picks_the_order_that_makes_sense() {
        // DirectoryNumber 100, FileNumber 27 written big-endian.
        let mut data = vec![0u8; 10];
        data[..4].copy_from_slice(b"0100");
        data[6..8].copy_from_slice(&100u16.to_be_bytes());
        data[8..10].copy_from_slice(&27u16.to_be_bytes());
        let mut tags = HashMap::new();
        parse_file_info(&data, ByteOrder::LittleEndian, None, &mut tags);
        assert_eq!(tags["Nikon:DirectoryNumber"], "100");
        assert_eq!(tags["Nikon:FileNumber"], "0027");
    }

    #[test]
    fn maker_notes_0x56_decodes_z_series_burst_record() {
        // Exact 0x0056 payload from the pinned NikonZf.jpg fixture.
        let data = [
            b'0', b'1', b'0', b'0', 0xa0, 0xe7, 0x30, 0x03, 1, 0, 0, 0, 1, 0, 0, 0,
        ];
        let mut tags = HashMap::new();
        parse_maker_notes_0x56(&data, ByteOrder::LittleEndian, &mut tags);
        assert_eq!(tags["Nikon:FirmwareVersion56"], "01.00");
        assert_eq!(tags["Nikon:BurstStartSlotNumber"], "1");
        assert_eq!(tags["Nikon:BurstStartFolderNumber"], "102");
        assert_eq!(tags["Nikon:BurstStartImageNumber"], "1853");
        assert_eq!(tags["Nikon:BurstStartImageType"], "JPG");
        assert_eq!(tags["Nikon:BurstShotNumber"], "1");
        assert_eq!(tags["Nikon:PixelShiftActive"], "Yes");
    }

    #[test]
    fn hdr_info_switches_table_on_length() {
        let mut six = b"0200".to_vec();
        six.extend_from_slice(&[1, 4]);
        let mut tags = HashMap::new();
        parse_hdr_info(&six, &mut tags);
        assert_eq!(tags["Nikon:HDRLevel"], "High+");
        assert!(!tags.contains_key("Nikon:HDRSmoothing"));

        let mut eight = b"0100".to_vec();
        eight.extend_from_slice(&[1, 0, 0, 255]);
        let mut tags = HashMap::new();
        parse_hdr_info(&eight, &mut tags);
        assert_eq!(tags["Nikon:HDRLevel"], "Auto");
        assert_eq!(tags["Nikon:HDRLevel2"], "n/a");
    }

    #[test]
    fn face_positions_stop_at_the_detected_count() {
        let mut data = vec![0u8; 64];
        // entry 3 = FacesDetected = 1
        data[6..8].copy_from_slice(&1u16.to_be_bytes());
        for i in 0..4 {
            data[8 + i * 2..10 + i * 2].copy_from_slice(&((i as u16 + 1) * 10).to_be_bytes());
        }
        let mut tags = HashMap::new();
        parse_face_detect(&data, ByteOrder::BigEndian, &mut tags);
        assert_eq!(tags["Nikon:FacesDetected"], "1");
        assert_eq!(tags["Nikon:Face1Position"], "10 20 30 40");
        assert!(!tags.contains_key("Nikon:Face2Position"));
    }
}
