//! `Image::ExifTool::NikonSettings` -- the user-settings directory hanging off
//! `Nikon::Main` tag 0x004e.
//!
//! Unlike ShotInfo this block is *not* encrypted: it is a 24-byte header
//! followed by a flat list of 8-byte records, each naming a tag id and holding
//! one `int32u`. `ProcessNikonSettings` in NikonSettings.pm is the reference.
//!
//! ```text
//! 0x00 undef[4]  '0100'
//! 0x04 int32u    1 (D bodies), 2 (Z bodies)
//! 0x08 undef[4]  '0100'
//! 0x0c int32u    layout id
//! 0x10 undef[4]  firmware version, e.g. '0110'
//! 0x14 int32u    entry count
//! 0x18 ...       entry[n]: int16u tag, byte 3 = format code (always 4),
//!                int32u value at +4
//! ```
//!
//! The tag table itself is generated (`settings_tables.rs`); this module holds
//! the walk, the Condition evaluation and the handful of ValueConv/PrintConv
//! expressions that are not plain lookup tables.

use std::collections::HashMap;

use super::settings_tables::SETTINGS_TAGS;
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Header size, and the offset of the first record.
const HEADER_LEN: usize = 0x18;

/// Bytes per record.
const ENTRY_LEN: usize = 8;

/// The only format code ExifTool has ever seen in this directory. Anything
/// else means the layout is not what we think it is, and ExifTool stops.
const FORMAT_INT32U: u8 = 4;

/// A `Condition` from `NikonSettings::Main`.
///
/// ExifTool picks the first variant whose Condition evaluates true, so the
/// order of [`SETTINGS_TAGS`] rows with the same id is significant.
#[derive(Clone, Copy)]
pub(super) enum Cond {
    Always,
    /// `$$self{Model} =~ /^NIKON D6\b/i`
    ModelD6,
    /// `$$self{Model} =~ /^NIKON Z (7|7_2)\b/i`
    ModelZ7,
    /// `$$self{Model} =~ /^NIKON Z (5|50|6|6_2|7|7_2|fc)\b/i`
    ModelZSeries,
    /// `$$self{Model} =~ /^NIKON Z [67]\b/` (no `/i`, and no `7_2`)
    ModelZ6Or7,
    /// `$$self{HDMIBitDepth} == 2`
    HdmiBitDepthIs2,
    /// `$$self{CmdDialsReverseRotExposureComp} and ... == 1`
    CmdDialsReverseRotExposureCompIs1,
    /// `$$self{CmdDialsChangeMainSubExposure} and ... == n`
    CmdDialsChangeMainSubExposureIs(u32),
    /// `$$self{BracketSet} < 4` -- true when unset, because Perl numifies
    /// undef to 0.
    BracketSetLt4,
    /// `$$self{BracketSet} and $$self{BracketSet} == n`
    BracketSetIs(u32),
    /// `$$self{BracketSet} < 4 and $$self{BracketProgram} ne n`
    BracketSetLt4AndProgramNe(u32),
    /// `$$self{BracketSet} == a and $$self{BracketProgram} ne b` -- note the
    /// bare `==` here, so an unset BracketSet compares as 0.
    BracketSetEqAndProgramNe(u32, u32),
    /// `$$self{PlaybackFlickUp} and ... == 1`
    PlaybackFlickUpIs1,
    /// `$$self{PlaybackFlickDown} and ... == 1`
    PlaybackFlickDownIs1,
}

/// The `RawConv` side effects that later Conditions read back.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Dm {
    None,
    HdmiBitDepth,
    HdmiOutputHdr,
    BracketSet,
    BracketProgram,
    PlaybackFlickUp,
    PlaybackFlickDown,
    CmdDialsReverseRotExposureComp,
    CmdDialsChangeMainSubExposure,
}

/// How a raw `int32u` becomes the string ExifTool prints.
#[derive(Clone, Copy)]
pub(super) enum Conv {
    /// A plain `PrintConv` hash. Values with no entry are reported the way
    /// ExifTool reports an unlisted code.
    Map(&'static [(u32, &'static str)]),
    /// `$val + n` (n is negative for the `$val - 6` forms).
    Offset(i64),
    /// `$$self{...} = n - $val`
    Negate(i64),
    /// ValueConv `n - $val`, PrintConv `"$val fps"`.
    Fps(i64),
    /// ValueConv `($val - 7) / 6`, PrintConv `$val ? sprintf("%+.2f",$val) : 0`.
    FineTune,
    /// No conversion at all.
    Raw,
}

/// One row of `NikonSettings::Main`.
pub(super) struct SettingsTag {
    pub id: u16,
    pub name: &'static str,
    pub cond: Cond,
    /// `Mask`, applied to the raw value before PrintConv. 0 means none.
    pub mask: u32,
    pub conv: Conv,
    pub dm: Dm,
}

/// The `$$self{...}` slots the Conditions above consult.
#[derive(Default)]
struct DataMembers {
    hdmi_bit_depth: Option<u32>,
    hdmi_output_hdr: Option<u32>,
    bracket_set: Option<u32>,
    bracket_program: Option<u32>,
    playback_flick_up: Option<u32>,
    playback_flick_down: Option<u32>,
    cmd_dials_reverse_rot_exposure_comp: Option<u32>,
    cmd_dials_change_main_sub_exposure: Option<u32>,
}

impl DataMembers {
    fn set(&mut self, dm: Dm, value: u32) {
        let slot = match dm {
            Dm::None => return,
            Dm::HdmiBitDepth => &mut self.hdmi_bit_depth,
            Dm::HdmiOutputHdr => &mut self.hdmi_output_hdr,
            Dm::BracketSet => &mut self.bracket_set,
            Dm::BracketProgram => &mut self.bracket_program,
            Dm::PlaybackFlickUp => &mut self.playback_flick_up,
            Dm::PlaybackFlickDown => &mut self.playback_flick_down,
            Dm::CmdDialsReverseRotExposureComp => &mut self.cmd_dials_reverse_rot_exposure_comp,
            Dm::CmdDialsChangeMainSubExposure => &mut self.cmd_dials_change_main_sub_exposure,
        };
        *slot = Some(value);
    }
}

/// Perl's `$x == n` where `$x` may be undef: undef numifies to 0.
fn num_eq(slot: Option<u32>, n: u32) -> bool {
    slot.unwrap_or(0) == n
}

/// Perl's `$x and $x == n`: undef is false, and so is 0.
fn truthy_eq(slot: Option<u32>, n: u32) -> bool {
    slot.is_some_and(|v| v != 0 && v == n)
}

/// Perl's `$x ne n`, a *string* comparison. undef stringifies to "", which is
/// never equal to a decimal literal, so an unset member makes this true.
fn str_ne(slot: Option<u32>, n: u32) -> bool {
    slot != Some(n)
}

/// `/^NIKON Z (...)\b/` -- match the alternation, then require that the next
/// character is not a word character (Perl's `\b`).
fn model_z_variant(model: &str, variants: &[&str], case_insensitive: bool) -> bool {
    let subject = if case_insensitive {
        model.to_ascii_uppercase()
    } else {
        model.to_string()
    };
    let Some(rest) = subject.strip_prefix("NIKON Z ") else {
        return false;
    };
    variants.iter().any(|v| {
        rest.strip_prefix(*v)
            .is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    })
}

fn cond_holds(cond: Cond, model: Option<&str>, dm: &DataMembers) -> bool {
    match cond {
        Cond::Always => true,
        Cond::ModelD6 => model.is_some_and(|m| {
            let m = m.to_ascii_uppercase();
            m.strip_prefix("NIKON D6")
                .is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
        }),
        Cond::ModelZ7 => model.is_some_and(|m| model_z_variant(m, &["7_2", "7"], true)),
        Cond::ModelZSeries => model
            .is_some_and(|m| model_z_variant(m, &["50", "5", "6_2", "6", "7_2", "7", "FC"], true)),
        Cond::ModelZ6Or7 => model.is_some_and(|m| model_z_variant(m, &["6", "7"], false)),
        Cond::HdmiBitDepthIs2 => num_eq(dm.hdmi_bit_depth, 2),
        Cond::CmdDialsReverseRotExposureCompIs1 => {
            truthy_eq(dm.cmd_dials_reverse_rot_exposure_comp, 1)
        }
        Cond::CmdDialsChangeMainSubExposureIs(n) => {
            truthy_eq(dm.cmd_dials_change_main_sub_exposure, n)
        }
        Cond::BracketSetLt4 => dm.bracket_set.unwrap_or(0) < 4,
        Cond::BracketSetIs(n) => truthy_eq(dm.bracket_set, n),
        Cond::BracketSetLt4AndProgramNe(n) => {
            dm.bracket_set.unwrap_or(0) < 4 && str_ne(dm.bracket_program, n)
        }
        Cond::BracketSetEqAndProgramNe(a, b) => {
            num_eq(dm.bracket_set, a) && str_ne(dm.bracket_program, b)
        }
        Cond::PlaybackFlickUpIs1 => truthy_eq(dm.playback_flick_up, 1),
        Cond::PlaybackFlickDownIs1 => truthy_eq(dm.playback_flick_down, 1),
    }
}

/// Render a Perl numeric scalar the way ExifTool prints one: integers bare,
/// fractions with the trailing zeros trimmed.
fn print_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    let mut s = format!("{:.6}", value);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

fn apply_conv(conv: Conv, value: u32) -> String {
    match conv {
        Conv::Map(pairs) => pairs
            .iter()
            .find(|(k, _)| *k == value)
            .map(|(_, v)| (*v).to_string())
            .unwrap_or_else(|| format!("Unknown ({})", value)),
        Conv::Offset(n) => (value as i64 + n).to_string(),
        Conv::Negate(n) => (n - value as i64).to_string(),
        Conv::Fps(n) => format!("{} fps", n - value as i64),
        Conv::FineTune => {
            let v = (value as f64 - 7.0) / 6.0;
            if v == 0.0 {
                "0".to_string()
            } else {
                format!("{:+.2}", v)
            }
        }
        Conv::Raw => print_number(value as f64),
    }
}

/// Walk `Nikon::Main` 0x004e and emit every named setting it carries.
///
/// `model` is the `IFD0:Model` string; several tags exist only for one body and
/// ExifTool selects between same-id variants with it.
pub fn parse_nikon_settings(
    data: &[u8],
    order: ByteOrder,
    model: Option<&str>,
    tags: &mut HashMap<String, String>,
) {
    if data.len() < HEADER_LEN {
        return;
    }
    let read_u16 = |at: usize| super::value_reader::read_u16(data, at, order);
    let read_u32 = |at: usize| super::value_reader::read_u32(data, at, order);

    let Some(declared) = read_u32(0x14) else {
        return;
    };
    // ExifTool warns and truncates when the directory claims more entries than
    // the block can hold.
    let available = (data.len() - HEADER_LEN) / ENTRY_LEN;
    let count = (declared as usize).min(available);

    let mut dm = DataMembers::default();
    for i in 0..count {
        let entry = HEADER_LEN + i * ENTRY_LEN;
        let (Some(id), Some(value)) = (read_u16(entry), read_u32(entry + 4)) else {
            break;
        };
        // A format code we have never seen means the record layout is not the
        // one decoded here; stop rather than emit guesses.
        if data.get(entry + 3) != Some(&FORMAT_INT32U) {
            break;
        }
        let Some(tag) = SETTINGS_TAGS
            .iter()
            .find(|t| t.id == id && cond_holds(t.cond, model, &dm))
        else {
            // Either an id with no table row, or one whose every variant is
            // conditioned off for this body. ExifTool reports neither.
            continue;
        };
        dm.set(tag.dm, value);
        let masked = if tag.mask != 0 {
            value & tag.mask
        } else {
            value
        };
        tags.insert(format!("Nikon:{}", tag.name), apply_conv(tag.conv, masked));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(entries: &[(u16, u32)]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"0100");
        v.extend_from_slice(&2u32.to_le_bytes());
        v.extend_from_slice(b"0100");
        v.extend_from_slice(&5u32.to_le_bytes());
        v.extend_from_slice(b"0100");
        v.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (id, value) in entries {
            v.extend_from_slice(&id.to_le_bytes());
            v.push(0);
            v.push(FORMAT_INT32U);
            v.extend_from_slice(&value.to_le_bytes());
        }
        v
    }

    #[test]
    fn reads_a_plain_printconv_tag() {
        // 0x00fc SilentPhotography, PrintConv 1 => On, 2 => Off.
        let data = header(&[(0x00fc, 2)]);
        let mut tags = HashMap::new();
        parse_nikon_settings(
            &data,
            ByteOrder::LittleEndian,
            Some("NIKON Z 7_2"),
            &mut tags,
        );
        assert_eq!(
            tags.get("Nikon:SilentPhotography").map(String::as_str),
            Some("Off")
        );
    }

    #[test]
    fn model_selects_between_same_id_variants() {
        // 0x0001 ISOAutoHiLimit has a D6 table and a Z7 table; 33 is a
        // different ISO on each.
        let data = header(&[(0x0001, 33)]);
        let mut z7 = HashMap::new();
        parse_nikon_settings(&data, ByteOrder::LittleEndian, Some("NIKON Z 7_2"), &mut z7);
        let mut d6 = HashMap::new();
        parse_nikon_settings(&data, ByteOrder::LittleEndian, Some("NIKON D6"), &mut d6);
        assert!(z7.contains_key("Nikon:ISOAutoHiLimit"));
        assert!(d6.contains_key("Nikon:ISOAutoHiLimit"));
        assert_ne!(z7["Nikon:ISOAutoHiLimit"], d6["Nikon:ISOAutoHiLimit"]);
    }

    #[test]
    fn an_unknown_model_drops_model_gated_tags() {
        let data = header(&[(0x0001, 33)]);
        let mut tags = HashMap::new();
        parse_nikon_settings(
            &data,
            ByteOrder::LittleEndian,
            Some("NIKON D850"),
            &mut tags,
        );
        assert!(!tags.contains_key("Nikon:ISOAutoHiLimit"));
    }

    #[test]
    fn a_bad_format_code_stops_the_walk() {
        let mut data = header(&[(0x00fc, 2), (0x0001, 33)]);
        data[HEADER_LEN + 3] = 9;
        let mut tags = HashMap::new();
        parse_nikon_settings(
            &data,
            ByteOrder::LittleEndian,
            Some("NIKON Z 7_2"),
            &mut tags,
        );
        assert!(tags.is_empty());
    }

    #[test]
    fn a_truncated_directory_stops_at_the_last_whole_entry() {
        let mut data = header(&[(0x00fc, 2), (0x0001, 33)]);
        data.truncate(HEADER_LEN + ENTRY_LEN + 3);
        let mut tags = HashMap::new();
        parse_nikon_settings(
            &data,
            ByteOrder::LittleEndian,
            Some("NIKON Z 7_2"),
            &mut tags,
        );
        assert_eq!(tags.len(), 1);
    }

    #[test]
    fn datamembers_gate_later_conditions() {
        // BracketSet=4 (White Balance) selects the WB variant of BracketProgram,
        // where 2 is 'A3F'; the default variant would call 2 unknown.
        let data = header(&[(0x0109, 4), (0x010a, 2)]);
        let mut tags = HashMap::new();
        parse_nikon_settings(
            &data,
            ByteOrder::LittleEndian,
            Some("NIKON Z 7_2"),
            &mut tags,
        );
        assert_eq!(
            tags.get("Nikon:BracketProgram").map(String::as_str),
            Some("A3F")
        );
    }

    #[test]
    fn model_word_boundary_is_respected() {
        // 'NIKON Z 70' must not satisfy /^NIKON Z (7|7_2)\b/.
        assert!(model_z_variant("NIKON Z 7_2", &["7_2", "7"], true));
        assert!(model_z_variant("NIKON Z 7", &["7_2", "7"], true));
        assert!(!model_z_variant("NIKON Z 70", &["7_2", "7"], true));
        assert!(!model_z_variant("NIKON Z 50", &["7_2", "7"], true));
    }
}
