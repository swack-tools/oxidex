//! Nikon `LensData` (MakerNote tag 0x0098) parser.
//!
//! `LensData` is a flat binary block whose layout is selected by the four ASCII
//! version bytes at its start:
//!
//! | Version  | Cameras                    | ExifTool table   | Encrypted |
//! |----------|----------------------------|------------------|-----------|
//! | `0100`   | D100, D1X                  | `LensData00`     | no        |
//! | `0101`   | D70, D70s                  | `LensData01`     | no        |
//! | `020x`+  | D200 and later              | `LensData0204`+  | yes       |
//!
//! From version `0201` onward everything after the version string is encrypted
//! with a key derived from `SerialNumber` and `ShutterCount`. This module
//! decodes the two unencrypted layouts and, for the encrypted ones, emits only
//! `LensDataVersion` -- which is genuinely plaintext, because ExifTool's
//! `DecryptStart` is 4. Reading the ciphertext as if it were a `LensData01`
//! block would produce plausible-looking but fabricated apertures and focal
//! lengths, which is strictly worse than reporting nothing.
//!
//! Offsets and conversions below are transcribed from
//! `Image::ExifTool::Nikon::LensData00` / `LensData01`.

use std::collections::HashMap;

use super::value_reader::{ascii_value, nikon_aperture, nikon_focal_length};

/// Byte offsets within an unencrypted `LensData` block.
struct LensDataLayout {
    exit_pupil_position: Option<usize>,
    af_aperture: Option<usize>,
    focus_position: Option<usize>,
    focus_distance: Option<usize>,
    focal_length: Option<usize>,
    lens_id_number: usize,
    lens_fstops: usize,
    min_focal_length: usize,
    max_focal_length: usize,
    max_aperture_at_min_focal: usize,
    max_aperture_at_max_focal: usize,
    mcu_version: usize,
    effective_max_aperture: Option<usize>,
}

/// `Image::ExifTool::Nikon::LensData00` (version 0100).
const LAYOUT_00: LensDataLayout = LensDataLayout {
    exit_pupil_position: None,
    af_aperture: None,
    focus_position: None,
    focus_distance: None,
    focal_length: None,
    lens_id_number: 0x06,
    lens_fstops: 0x07,
    min_focal_length: 0x08,
    max_focal_length: 0x09,
    max_aperture_at_min_focal: 0x0a,
    max_aperture_at_max_focal: 0x0b,
    mcu_version: 0x0c,
    effective_max_aperture: None,
};

/// `Image::ExifTool::Nikon::LensData01` (version 0101).
const LAYOUT_01: LensDataLayout = LensDataLayout {
    exit_pupil_position: Some(0x04),
    af_aperture: Some(0x05),
    focus_position: Some(0x08),
    focus_distance: Some(0x09),
    focal_length: Some(0x0a),
    lens_id_number: 0x0b,
    lens_fstops: 0x0c,
    min_focal_length: 0x0d,
    max_focal_length: 0x0e,
    max_aperture_at_min_focal: 0x0f,
    max_aperture_at_max_focal: 0x10,
    mcu_version: 0x11,
    effective_max_aperture: Some(0x12),
};

/// Parse a Nikon `LensData` block into `Nikon:`-prefixed tags.
///
/// `data` is the raw value of MakerNote tag 0x0098, starting at the version
/// string. Tags whose offsets fall outside the block are skipped rather than
/// defaulted.
pub fn parse_lens_data(data: &[u8], tags: &mut HashMap<String, String>) {
    parse_lens_data_with_values(data, tags, &mut HashMap::new());
}

/// Parse plaintext LensData while retaining ValueConv forms separately from
/// their rounded PrintConv strings.
pub fn parse_lens_data_with_values(
    data: &[u8],
    tags: &mut HashMap<String, String>,
    value_forms: &mut HashMap<String, String>,
) {
    if data.len() < 4 {
        return;
    }
    let version = ascii_value(&data[..4]);
    if version.is_empty() {
        return;
    }
    tags.insert("Nikon:LensDataVersion".to_string(), version.clone());

    let layout = match version.as_str() {
        "0100" => &LAYOUT_00,
        "0101" => &LAYOUT_01,
        // 020x and later are encrypted; only the version string is plaintext.
        _ => return,
    };

    let at = |offset: usize| data.get(offset).copied();

    if let Some(raw) = layout.exit_pupil_position.and_then(at) {
        // ValueConv: $val ? 2048 / $val : $val
        let value = if raw == 0 { 0.0 } else { 2048.0 / raw as f64 };
        tags.insert(
            "Nikon:ExitPupilPosition".to_string(),
            format!("{:.1} mm", value),
        );
    }
    if let Some(raw) = layout.af_aperture.and_then(at) {
        tags.insert(
            "Nikon:AFAperture".to_string(),
            format!("{:.1}", nikon_aperture(raw)),
        );
    }
    if let Some(raw) = layout.focus_position.and_then(at) {
        // Upper nibble = far focus range, lower nibble = near focus range.
        tags.insert("Nikon:FocusPosition".to_string(), format!("0x{:02x}", raw));
    }
    if let Some(raw) = layout.focus_distance.and_then(at) {
        // ValueConv: 0.01 * 10**($val/40), in metres.
        let metres = 0.01 * 10.0_f64.powf(raw as f64 / 40.0);
        let printed = if metres == 0.0 {
            "inf".to_string()
        } else {
            format!("{:.2} m", metres)
        };
        tags.insert("Nikon:FocusDistance".to_string(), printed);
        value_forms.insert("Nikon:FocusDistance".to_string(), metres.to_string());
    }
    if let Some(raw) = layout.focal_length.and_then(at) {
        tags.insert(
            "Nikon:FocalLength".to_string(),
            format!("{:.1} mm", nikon_focal_length(raw)),
        );
    }
    if let Some(raw) = at(layout.lens_id_number) {
        tags.insert("Nikon:LensIDNumber".to_string(), raw.to_string());
    }
    if let Some(raw) = at(layout.lens_fstops) {
        tags.insert(
            "Nikon:LensFStops".to_string(),
            format!("{:.2}", raw as f64 / 12.0),
        );
    }
    if let Some(raw) = at(layout.min_focal_length) {
        tags.insert(
            "Nikon:MinFocalLength".to_string(),
            format!("{:.1} mm", nikon_focal_length(raw)),
        );
    }
    if let Some(raw) = at(layout.max_focal_length) {
        tags.insert(
            "Nikon:MaxFocalLength".to_string(),
            format!("{:.1} mm", nikon_focal_length(raw)),
        );
    }
    if let Some(raw) = at(layout.max_aperture_at_min_focal) {
        tags.insert(
            "Nikon:MaxApertureAtMinFocal".to_string(),
            format!("{:.1}", nikon_aperture(raw)),
        );
    }
    if let Some(raw) = at(layout.max_aperture_at_max_focal) {
        tags.insert(
            "Nikon:MaxApertureAtMaxFocal".to_string(),
            format!("{:.1}", nikon_aperture(raw)),
        );
    }
    if let Some(raw) = at(layout.mcu_version) {
        tags.insert("Nikon:MCUVersion".to_string(), raw.to_string());
    }
    if let Some(raw) = layout.effective_max_aperture.and_then(at) {
        tags.insert(
            "Nikon:EffectiveMaxAperture".to_string(),
            format!("{:.1}", nikon_aperture(raw)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The LensData0101 block from `Nikon.nef` (D70, AF-S DX 18-70mm), read
    /// straight out of MakerNote tag 0x0098. Every expectation below is the
    /// output of `exiftool -s` on that file.
    const D70_LENS_DATA: &[u8] = &[
        0x30, 0x31, 0x30, 0x31, 0x14, 0x2c, 0x07, 0x00, 0xf1, 0x5f, 0x2d, 0x7f, 0x40, 0x2d, 0x5c,
        0x2c, 0x34, 0x84, 0x2c, 0xef, 0x40, 0x20, 0x59, 0x05, 0x00, 0x00, 0x15, 0x05, 0x00, 0x02,
        0x02,
    ];

    fn parse(data: &[u8]) -> HashMap<String, String> {
        let mut tags = HashMap::new();
        parse_lens_data(data, &mut tags);
        tags
    }

    #[test]
    fn parses_version_0101_exactly_as_exiftool_does() {
        let tags = parse(D70_LENS_DATA);
        let expect = [
            ("Nikon:LensDataVersion", "0101"),
            ("Nikon:ExitPupilPosition", "102.4 mm"),
            ("Nikon:AFAperture", "3.6"),
            ("Nikon:FocusPosition", "0xf1"),
            ("Nikon:FocusDistance", "2.37 m"),
            ("Nikon:FocalLength", "18.3 mm"),
            ("Nikon:LensIDNumber", "127"),
            ("Nikon:LensFStops", "5.33"),
            ("Nikon:MinFocalLength", "18.3 mm"),
            ("Nikon:MaxFocalLength", "71.3 mm"),
            ("Nikon:MaxApertureAtMinFocal", "3.6"),
            ("Nikon:MaxApertureAtMaxFocal", "4.5"),
            ("Nikon:MCUVersion", "132"),
            ("Nikon:EffectiveMaxAperture", "3.6"),
        ];
        for (key, value) in expect {
            assert_eq!(tags.get(key).map(String::as_str), Some(value), "{}", key);
        }
    }

    #[test]
    fn version_0100_uses_the_shorter_layout() {
        let mut data = vec![0u8; 13];
        data[..4].copy_from_slice(b"0100");
        data[0x06] = 127; // LensIDNumber
        data[0x07] = 64; // LensFStops -> 64/12
        data[0x08] = 45; // MinFocalLength
        data[0x09] = 92; // MaxFocalLength
        data[0x0a] = 44; // MaxApertureAtMinFocal
        data[0x0b] = 52; // MaxApertureAtMaxFocal
        data[0x0c] = 132; // MCUVersion
        let tags = parse(&data);
        assert_eq!(tags.get("Nikon:LensDataVersion").unwrap(), "0100");
        assert_eq!(tags.get("Nikon:LensIDNumber").unwrap(), "127");
        assert_eq!(tags.get("Nikon:LensFStops").unwrap(), "5.33");
        assert_eq!(tags.get("Nikon:MinFocalLength").unwrap(), "18.3 mm");
        assert_eq!(tags.get("Nikon:MaxFocalLength").unwrap(), "71.3 mm");
        assert_eq!(tags.get("Nikon:MCUVersion").unwrap(), "132");
        // Version 0100 has no ExitPupilPosition/AFAperture/FocusDistance.
        assert!(!tags.contains_key("Nikon:ExitPupilPosition"));
        assert!(!tags.contains_key("Nikon:AFAperture"));
        assert!(!tags.contains_key("Nikon:FocusDistance"));
        assert!(!tags.contains_key("Nikon:EffectiveMaxAperture"));
    }

    #[test]
    fn encrypted_versions_report_only_the_plaintext_version_string() {
        let mut data = vec![0xAAu8; 32];
        data[..4].copy_from_slice(b"0204");
        let tags = parse(&data);
        assert_eq!(tags.get("Nikon:LensDataVersion").unwrap(), "0204");
        assert_eq!(
            tags.len(),
            1,
            "encrypted LensData must not be decoded as plaintext: {:?}",
            tags
        );
    }

    #[test]
    fn truncated_blocks_emit_only_what_is_present() {
        let tags = parse(b"0101\x14\x2c");
        assert_eq!(tags.get("Nikon:LensDataVersion").unwrap(), "0101");
        assert_eq!(tags.get("Nikon:ExitPupilPosition").unwrap(), "102.4 mm");
        assert_eq!(tags.get("Nikon:AFAperture").unwrap(), "3.6");
        assert!(!tags.contains_key("Nikon:MCUVersion"));
    }

    #[test]
    fn empty_and_short_input_is_ignored() {
        assert!(parse(&[]).is_empty());
        assert!(parse(b"01").is_empty());
    }
}
