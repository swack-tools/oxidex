//! Pentax main-IFD tags the hand-written match in [`super::pentax`] never
//! registered.
//!
//! These are ordinary `Pentax::Main` entries -- ported straight from
//! ExifTool's table -- so they walk with the shared table-IFD engine instead
//! of growing the existing 2000-line match arm. The caller only keeps keys the
//! main parser did not already produce, so this can add tags but never change
//! one the main parser owns.

use super::shared::table_ifd::{OlyVal, TagDef, ftype, walk_directory};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

/// `Pentax.pm` `%convertMeteringSegments`: `255` is `n/a`, `0` stays `0`, and
/// anything else converts to LV as `$_ / 8 - 6` with one decimal.
fn print_metering_segments(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    Some(
        v.iter()
            .map(|&n| match n {
                255 => "n/a".to_string(),
                0 => "0".to_string(),
                _ => format!("{:.1}", n as f64 / 8.0 - 6.0),
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// `ColorMatrixA`/`ColorMatrixB`: `$_/8192` then `sprintf("%.5f")`.
fn print_color_matrix_scaled(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    Some(
        v.iter()
            .map(|&n| format!("{:.5}", n as f64 / 8192.0))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

static PENTAX_IMAGE_SIZE: &[(&str, &str)] = &[
    ("0", "640x480"),
    ("0 0", "2304x1728"),
    ("1", "Full"),
    ("2", "1024x768"),
    ("3", "1280x960"),
    ("4", "1600x1200"),
    ("4 0", "1600x1200"),
    ("5", "2048x1536"),
    ("5 0", "2048x1536"),
    ("8", "2560x1920 or 2304x1728"),
    ("8 0", "2560x1920"),
    ("9", "3072x2304"),
    ("10", "3264x2448"),
    ("19", "320x240"),
    ("20", "2288x1712"),
    ("21", "2592x1944"),
    ("22", "2304x1728 or 2592x1944"),
    ("23", "3056x2296"),
    ("25", "2816x2212 or 2816x2112"),
    ("27", "3648x2736"),
    ("29", "4000x3000"),
    ("30", "4288x3216"),
    ("31", "4608x3456"),
    ("129", "1920x1080"),
    ("135", "4608x2592"),
    ("257", "3216x3216"),
    ("32 2", "960x640"),
    ("33 2", "1152x768"),
    ("34 2", "1536x1024"),
    ("35 1", "2400x1600"),
    ("36 0", "3008x2008 or 3040x2024"),
    ("37 0", "3008x2000"),
];

static IMAGE_EDITING: &[(&str, &str)] = &[
    ("0 0", "None"),
    ("0 0 0 0", "None"),
    ("0 0 0 4", "Digital Filter"),
    ("1 0 0 0", "Resized"),
    ("2 0 0 0", "Cropped"),
    ("4 0 0 0", "Digital Filter 4"),
    ("6 0 0 0", "Digital Filter 6"),
    ("8 0 0 0", "Red-eye Correction"),
    ("16 0 0 0", "Frame Synthesis?"),
];

static BLEACH_BYPASS_TONING: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Green"),
    (2, "Yellow"),
    (3, "Orange"),
    (4, "Red"),
    (5, "Magenta"),
    (6, "Purple"),
    (7, "Blue"),
    (8, "Cyan"),
    (65535, "n/a"),
];

static SUPPLEMENT: &[TagDef] = &[
    TagDef::list_lookup(0x0009, "PentaxImageSize", PENTAX_IMAGE_SIZE),
    TagDef::list_lookup(0x0032, "ImageEditing", IMAGE_EDITING),
    TagDef::raw(0x007A, "ISOAutoMinSpeed"),
    TagDef::lookup(0x007F, "BleachBypassToning", BLEACH_BYPASS_TONING),
    TagDef::raw(0x0082, "BlurControl"),
    TagDef::raw(0x0200, "BlackPoint"),
    TagDef::raw(0x0201, "WhitePoint"),
    TagDef::func(0x0203, "ColorMatrixA", print_color_matrix_scaled),
    TagDef::func(0x0204, "ColorMatrixB", print_color_matrix_scaled),
    TagDef::typed_func(
        0x0209,
        "AEMeteringSegments",
        ftype::TIFF_BYTE,
        print_metering_segments,
    ),
    TagDef::typed_func(
        0x020A,
        "FlashMeteringSegments",
        ftype::TIFF_BYTE,
        print_metering_segments,
    ),
    TagDef::typed_func(
        0x020B,
        "SlaveFlashMeteringSegments",
        ftype::TIFF_BYTE,
        print_metering_segments,
    ),
    TagDef::raw(0x0211, "WB_RGGBLevelsFluorescentD"),
    TagDef::raw(0x0212, "WB_RGGBLevelsFluorescentN"),
    TagDef::raw(0x0213, "WB_RGGBLevelsFluorescentW"),
    TagDef::typed(0x021C, "ColorMatrixA2", ftype::TIFF_SSHORT),
    TagDef::typed(0x021D, "ColorMatrixB2", ftype::TIFF_SSHORT),
    TagDef::raw(0x0231, "ContrastDetectAFArea"),
];

/// Add the supplemental tags, leaving anything the main parser already emitted
/// untouched.
pub fn add_supplemental_tags(
    data: &[u8],
    ifd_start: usize,
    value_base: i64,
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let mut extra: HashMap<String, String> = HashMap::new();
    walk_directory(
        data,
        ifd_start,
        Some(value_base),
        byte_order,
        "Pentax",
        SUPPLEMENT,
        &mut extra,
    );
    for (k, v) in extra {
        tags.entry(k).or_insert(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metering_segments_convert_to_lv() {
        let v = OlyVal::Int(vec![255, 0, 48, 100]);
        assert_eq!(print_metering_segments(&v).unwrap(), "n/a 0 0.0 6.5");
    }

    #[test]
    fn color_matrix_scales_by_8192() {
        let v = OlyVal::Int(vec![8192, 4096, 0]);
        assert_eq!(
            print_color_matrix_scaled(&v).unwrap(),
            "1.00000 0.50000 0.00000"
        );
    }
}
