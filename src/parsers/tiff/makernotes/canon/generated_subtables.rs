//! `%Canon` records the hand walk in `canon.rs` reaches but decodes through
//! the GENERATED layouts rather than a hand transcription.
//!
//! Forward-ported from `origin/main` (#703, #708, #709), re-expressed on the
//! tip's generated path:
//!
//! * `%Canon::FaceDetect2` (Main 0x0025), `%Canon::ColorBalance` (0x00a9) and
//!   the two `%Canon::ModifiedInfo` (0x00b1) fields the hand
//!   `binary_tables` transcription leaves out go through
//!   [`process_binary_data`] over the generated `BinaryTable`, so the byte
//!   layout, the `Condition`s (`ModifiedSharpness`'s `/\b(1D|5D)/`, the
//!   `/EOS D60\b/` `_variants` pair at ColorBalance key 29) and the compiled
//!   `ValueConv` (`ModifiedDigitalGain`'s `$val / 10`) all come from the
//!   generator. main decoded these by hand.
//! * `%Canon::SerialInfo` (the `/EOS 5D/` alternative of Main 0x0096) reads
//!   the generated layout through [`RawAccess`]: its `RawConv`
//!   (`$val =~ /^\w{6}/ ? $val : undef`) is withheld by the generator and
//!   reproduced here, with the citation.
//!
//! Main 0x0094 `AFPointsInFocus1D` and the six `%longBin` Main ids are scalar
//! rows the generated `Canon::Main` withholds (`omitted.print_conv` /
//! `omitted.value_conv`), so they are `main_engine::CANON_MAIN_RESIDUAL_IDS`
//! hand arms; their decoders live here too.

use std::collections::HashMap;

use crate::exiftool_tables::{
    Acknowledged, Ctx, DecodedValue, Dir, MemberValue, PerlCitation, RawAccess,
    decode_binary_table, find_table, process_binary_data,
};
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::parsers::tiff::makernotes::shared::engine_value::engine_value_text;

/// Walks the generated `%Canon::<table>` through the binary engine and
/// inserts every `Canon`-group row it reports, or only `only`'s names.
///
/// `model` is `$$self{Model}` for the table's `Condition`s; an empty model
/// leaves the member unset, as on the Model-less entry points.
pub(super) fn insert_generated_rows(
    table: &str,
    record: &[u8],
    byte_order: ByteOrder,
    model: &str,
    only: Option<&[&str]>,
    tags: &mut HashMap<String, String>,
) {
    let Some(table) = find_table("Canon", table) else {
        return;
    };
    let mut members = HashMap::new();
    if !model.is_empty() {
        members.insert("Model", MemberValue::Str(model.to_string()));
    }
    let mut ctx = Ctx::new(&mut members);
    let mut rows = Vec::new();
    process_binary_data(
        table,
        Dir::whole(record, byte_order.to_io_byte_order()),
        &mut ctx,
        &mut rows,
    );
    for row in rows {
        if row.module != "Canon" || row.group1 != "Canon" {
            continue;
        }
        if only.is_some_and(|names| !names.contains(&row.name)) {
            continue;
        }
        if let Some(text) = engine_value_text(&row.value) {
            tags.insert(format!("Canon:{}", row.name), text);
        }
    }
}

const SERIAL_INFO_RAW_CONV: PerlCitation = PerlCitation {
    module: "Canon",
    table: "SerialInfo",
    tag: "InternalSerialNumber2, InternalSerialNumber",
    lines: "7146-7162",
};

/// `%Canon::SerialInfo` (Canon.pm:7146-7162), the `/EOS 5D/` alternative of
/// Main 0x0096. Both keys carry `RawConv => '$val =~ /^\w{6}/ ? $val :
/// undef'`, which the generator withholds; the value is the `string` read
/// (truncated at the first NUL), and `\w` on a byte string is `[A-Za-z0-9_]`.
pub(super) fn insert_serial_info(
    record: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let Some(table) = find_table("Canon", "SerialInfo") else {
        return;
    };
    let decode = decode_binary_table(table, record, byte_order.to_io_byte_order());
    for decoded in decode.fields() {
        let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &SERIAL_INFO_RAW_CONV)
        else {
            continue;
        };
        let DecodedValue::String(value) = access.raw() else {
            continue;
        };
        let word_prefix = value.len() >= 6
            && value.as_bytes()[..6]
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
        if word_prefix {
            tags.insert(format!("Canon:{}", access.field().name), value.clone());
        }
    }
}

/// ExifTool's Binary placeholder text.
fn binary_placeholder(size: usize) -> String {
    format!("(Binary data {size} bytes, use -b option to extract)")
}

/// Canon.pm's `%longBin` (`ValueConv => 'length($val) > 64 ? \$val : $val'`)
/// on an `int16u` array: ExifTool joins the numbers with spaces first, so the
/// length it tests -- and the one the placeholder reports -- is that of the
/// decoded text, not of the stored bytes (a 626-byte tone curve reports
/// 1,679).
pub(super) fn long_bin_u16(bytes: &[u8], byte_order: ByteOrder) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let reader = EndianReader::new(bytes, byte_order.to_io_byte_order());
    let rendered = (0..bytes.len() / 2)
        .map(|index| reader.u16_at(index * 2).map(|value| value.to_string()))
        .collect::<Option<Vec<_>>>()?
        .join(" ");
    Some(if rendered.len() > 64 {
        binary_placeholder(rendered.len())
    } else {
        rendered
    })
}

/// `@focusPts` of `Image::ExifTool::Canon::PrintAFPoints1D` (Canon.pm:10618):
/// the focusing-point code of each of the 56 grid positions.
const AF_POINT_1D_CODES: [u8; 56] = [
    0, 0, 0x04, 0x06, 0x08, 0x0a, 0x0c, 0x0e, 0x10, 0, 0, 0x21, 0x23, 0x25, 0x27, 0x29, 0x2b, 0x2d,
    0x2f, 0x31, 0x33, 0x40, 0x42, 0x44, 0x46, 0x48, 0x4a, 0x4c, 0x4d, 0x50, 0x52, 0x54, 0x61, 0x63,
    0x65, 0x67, 0x69, 0x6b, 0x6d, 0x6f, 0x71, 0x73, 0, 0, 0x84, 0x86, 0x88, 0x8a, 0x8c, 0x8e, 0x90,
    0, 0, 0, 0, 0,
];
/// `@rows` of the same sub (Canon.pm:10627), one row letter per position.
const AF_POINT_1D_ROWS: &[u8; 56] = b"  AAAAAAA  BBBBBBBBBBCCCCCCCCCCCDDDDDDDDDD  EEEEEEE     ";

/// Main 0x0094 `AFPointsInFocus1D`: `Canon::PrintAFPoints1D` (Canon.pm:
/// 10611-10640) transcribed statement for statement, including its naming of
/// the padding positions (`" 1"`..) and its last-match-wins `$focusing`.
pub(super) fn af_points_in_focus_1d(val: &[u8]) -> String {
    if val.len() != 8 {
        return "Unknown".to_string();
    }
    let focus = val[0];
    // `unpack('b*', ...)`: least significant bit of each byte first.
    let mut bits = val[1..]
        .iter()
        .flat_map(|byte| (0..8).map(move |bit| byte & (1 << bit) != 0));
    let mut focusing = None;
    let mut points = Vec::new();
    let mut last_row = None;
    let mut col = 0;
    for (&code, &row) in AF_POINT_1D_CODES.iter().zip(AF_POINT_1D_ROWS) {
        col = if last_row == Some(row) { col + 1 } else { 1 };
        last_row = Some(row);
        let name = format!("{}{col}", row as char);
        if focus == code {
            focusing = Some(name.clone());
        }
        if bits.next() == Some(true) {
            points.push(name);
        }
    }
    let focusing = focusing.unwrap_or_else(|| {
        if focus == 0xff {
            "Auto".to_string()
        } else {
            format!("Unknown (0x{focus:02x})")
        }
    });
    format!("{focusing} ({})", points.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags_of(
        table: &str,
        record: &[u8],
        order: ByteOrder,
        model: &str,
        only: Option<&[&str]>,
    ) -> HashMap<String, String> {
        let mut tags = HashMap::new();
        insert_generated_rows(table, record, order, model, only, &mut tags);
        tags
    }

    /// `%Canon::FaceDetect2` is `int8u` with `FIRST_ENTRY => 0`: byte 1 is
    /// FaceWidth, byte 2 FacesDetected.
    #[test]
    fn face_detect2_reads_bytes_one_and_two() {
        let tags = tags_of(
            "FaceDetect2",
            &[0x07, 35, 1, 0, 0, 0],
            ByteOrder::LittleEndian,
            "Canon PowerShot A560",
            None,
        );
        assert_eq!(tags.get("Canon:FaceWidth").map(String::as_str), Some("35"));
        assert_eq!(
            tags.get("Canon:FacesDetected").map(String::as_str),
            Some("1")
        );
    }

    fn color_balance_record() -> Vec<u8> {
        (0i16..41).flat_map(|word| word.to_le_bytes()).collect()
    }

    /// Key 29 is `WB_RGGBLevelsCustom` unless `$$self{Model} =~ /EOS D60\b/`,
    /// where it is `BlackLevels` (Canon.pm:7282-7291).
    #[test]
    fn color_balance_key_29_follows_the_d60_condition() {
        let record = color_balance_record();
        let d10 = tags_of(
            "ColorBalance",
            &record,
            ByteOrder::LittleEndian,
            "Canon EOS 10D",
            None,
        );
        assert_eq!(
            d10.get("Canon:WB_RGGBLevelsAuto").map(String::as_str),
            Some("1 2 3 4")
        );
        assert_eq!(
            d10.get("Canon:WB_RGGBLevelsCustom").map(String::as_str),
            Some("29 30 31 32")
        );
        assert!(!d10.contains_key("Canon:BlackLevels"));
        assert_eq!(
            d10.get("Canon:WB_RGGBBlackLevels").map(String::as_str),
            Some("37 38 39 40")
        );

        let d60 = tags_of(
            "ColorBalance",
            &record,
            ByteOrder::LittleEndian,
            "Canon EOS D60",
            None,
        );
        assert_eq!(
            d60.get("Canon:BlackLevels").map(String::as_str),
            Some("29 30 31 32")
        );
        assert!(!d60.contains_key("Canon:WB_RGGBLevelsCustom"));
    }

    /// `ModifiedSharpness` is `/\b(1D|5D)/` only -- a leading boundary with
    /// no trailing one, so `EOS-1Ds` qualifies and `EOS 15D`-style digits do
    /// not; `ModifiedDigitalGain` is `$val / 10`.
    #[test]
    fn modified_info_condition_and_value_conv() {
        let mut words = vec![0i16; 12];
        words[0] = 24;
        words[2] = 3;
        words[11] = 15;
        let record: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let only: &[&str] = &["ModifiedSharpness", "ModifiedDigitalGain"];
        for (model, sharpness) in [
            ("Canon EOS-1D", true),
            ("Canon EOS-1Ds", true),
            ("Canon EOS 5D Mark II", true),
            ("Canon EOS 15D", false),
            ("Canon EOS 40D", false),
        ] {
            let tags = tags_of(
                "ModifiedInfo",
                &record,
                ByteOrder::LittleEndian,
                model,
                Some(only),
            );
            assert_eq!(
                tags.get("Canon:ModifiedSharpness").map(String::as_str),
                sharpness.then_some("3"),
                "{model}"
            );
            assert_eq!(
                tags.get("Canon:ModifiedDigitalGain").map(String::as_str),
                Some("1.5"),
                "{model}"
            );
            assert_eq!(tags.len(), usize::from(sharpness) + 1, "{model}: only");
        }
    }

    #[test]
    fn serial_info_applies_the_word_prefix_raw_conv() {
        let mut tags = HashMap::new();
        insert_serial_info(b"AD0010003\0\0\0", ByteOrder::LittleEndian, &mut tags);
        assert_eq!(
            tags.get("Canon:InternalSerialNumber2").map(String::as_str),
            Some("AD0010003")
        );
        assert!(!tags.contains_key("Canon:InternalSerialNumber"));

        let mut tags = HashMap::new();
        insert_serial_info(b"AD-010003\0\0\0", ByteOrder::LittleEndian, &mut tags);
        assert!(tags.is_empty());
    }

    #[test]
    fn long_bin_measures_the_decoded_text() {
        let short: Vec<u8> = [0u16; 15].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(
            long_bin_u16(&short, ByteOrder::LittleEndian).as_deref(),
            Some("0 0 0 0 0 0 0 0 0 0 0 0 0 0 0")
        );
        // 33 zeros join to 65 characters: one past ExifTool's 64.
        let long: Vec<u8> = [0u16; 33].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(
            long_bin_u16(&long, ByteOrder::LittleEndian).as_deref(),
            Some("(Binary data 65 bytes, use -b option to extract)")
        );
        assert_eq!(long_bin_u16(&[0], ByteOrder::LittleEndian), None);
    }

    /// Perl names every position, padding included (`" 1"`, `" 2"`, ...), and
    /// a focus code of 0 matches every padding slot, the last of which wins.
    #[test]
    fn af_points_1d_matches_the_perl_sub() {
        assert_eq!(
            af_points_in_focus_1d(&[0xff, 0b0000_0100, 0, 0, 0, 0, 0, 0]),
            "Auto (A1)"
        );
        assert_eq!(
            af_points_in_focus_1d(&[0x04, 0b0000_0001, 0, 0, 0, 0, 0, 0]),
            "A1 ( 1)"
        );
        assert_eq!(af_points_in_focus_1d(&[0; 8]), " 5 ()");
        assert_eq!(
            af_points_in_focus_1d(&[0x05, 0, 0, 0, 0, 0, 0, 0]),
            "Unknown (0x05) ()"
        );
        assert_eq!(af_points_in_focus_1d(&[0; 7]), "Unknown");
    }
}
