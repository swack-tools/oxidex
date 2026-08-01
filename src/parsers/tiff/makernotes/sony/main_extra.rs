//! `Sony::Main` scalars that [`super::main_table`] does not hand-implement.
//!
//! These are ordinary IFD entries, not `ProcessBinaryData` offsets, so they
//! need a different read: the value comes from the entry's own TIFF type and
//! count (or from a `Format` the tag declares, which `ProcessExif` honours as
//! a reinterpretation) rather than from an index into a block. Everything
//! after the read is the same, so the `RawConv`/`ValueConv`/`PrintConv`
//! machinery in [`super::binary_data`] is reused verbatim.
//!
//! The rows live in the generated [`super::main_extra_tables`]. This module is
//! consulted only when [`super::main_table::main_tag`] has no entry for the
//! id, so it can add tags but never change one that already reports.

use super::binary_data::{Ctx, Dm, Fmt, NumCmp, Pc, Raw, Scalar, Vc};
use super::binary_data::{apply_pc, apply_raw, apply_vc, re_matches};
use super::value::SonyValue;
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// A `Condition` on a `Sony::Main` entry.
///
/// Distinct from [`super::binary_data::Cond`] because two of its forms only
/// exist out here: a test on the entry's TIFF type, and ExifTool's habit of
/// writing an assignment where a condition belongs.
pub enum MCond {
    Always,
    /// `$$self{Model} =~ /RE/` (`true` for the negated `!~`)
    ModelRe(bool, &'static str),
    /// `$format eq "X"` -- the entry's own TIFF type, by ExifTool's name
    EntryFormat(&'static str),
    /// `$$self{X} <op> n`
    DmCmp(Dm, NumCmp, f64),
    /// `$$self{X}`
    DmTruthy(Dm),
    /// `not $$self{X}`
    DmFalsy(Dm),
    /// `$$self{X} eq/ne "s"` (`true` for `eq`)
    DmStrCmp(Dm, bool, &'static str),
    /// `$$self{X} = Get16u($valPt, 0)` -- an assignment whose result is also
    /// the test, so evaluating the Condition is what sets the member.
    StoreU16(Dm),
    All(&'static [MCond]),
    Any(&'static [MCond]),
}

pub struct MainExtraTag {
    pub id: u16,
    pub name: &'static str,
    pub cond: MCond,
    /// A `Format` the tag declares, which reinterprets the entry's bytes.
    pub fmt: Option<Fmt>,
    pub raw: Raw,
    pub vc: Vc,
    pub pc: Pc,
    pub print_hex: bool,
    /// `Priority => 0`
    pub low_priority: bool,
}

/// ExifTool's name for a TIFF field type, which is what `$format eq` compares.
fn format_name(field_type: u16) -> &'static str {
    match field_type {
        1 => "int8u",
        2 => "string",
        3 => "int16u",
        4 => "int32u",
        5 => "rational64u",
        6 => "int8s",
        7 => "undef",
        8 => "int16s",
        9 => "int32s",
        10 => "rational64s",
        11 => "float",
        12 => "double",
        _ => "",
    }
}

fn holds(cond: &MCond, ctx: &mut Ctx, value: &SonyValue<'_>) -> bool {
    match cond {
        MCond::Always => true,
        MCond::ModelRe(neg, re) => re_matches(re, &ctx.model) != *neg,
        MCond::EntryFormat(name) => format_name(value.field_type) == *name,
        MCond::DmCmp(dm, op, n) => op.holds(ctx.get(*dm).map_or(0.0, Scalar::num), *n),
        MCond::DmTruthy(dm) => ctx.get(*dm).is_some_and(Scalar::truthy),
        MCond::DmFalsy(dm) => !ctx.get(*dm).is_some_and(Scalar::truthy),
        MCond::DmStrCmp(dm, eq, s) => {
            let text = ctx.get(*dm).map(Scalar::text).unwrap_or_default();
            (text == *s) == *eq
        }
        MCond::StoreU16(dm) => {
            // `Get16u($valPt, 0)` reads the raw value, whatever its declared
            // type, in the MakerNote's byte order.
            let v = value.u16_at_raw(0).unwrap_or(0);
            ctx.set_member(*dm, Scalar::Num(v as f64));
            v != 0
        }
        MCond::All(list) => list.iter().all(|c| holds(c, ctx, value)),
        MCond::Any(list) => list.iter().any(|c| holds(c, ctx, value)),
    }
}

/// Reads an entry as a Perl scalar, the way `ProcessExif` does.
///
/// With no `Format` override the entry's own TIFF type and count decide; with
/// one, the bytes are reinterpreted and the count recomputed from their length,
/// which is what ExifTool's `$count = int($size / $formatSize)` amounts to.
fn read_entry(
    value: &SonyValue<'_>,
    fmt: Option<Fmt>,
    order: ByteOrder,
) -> Option<(Scalar, Vec<u8>)> {
    let raw = value.bytes().to_vec();
    if let Some(fmt) = fmt {
        let count = raw.len() / fmt.size();
        if count == 0 {
            return None;
        }
        let scalar = super::binary_data::read_values(&raw, 0, fmt, count as u32, raw.len(), order)?;
        return Some((scalar, raw));
    }
    // ASCII is a string; rationals are their quotient; everything else is the
    // integer components joined by spaces.
    let scalar = match value.field_type {
        2 => Scalar::Text(value.string().unwrap_or_default()),
        5 | 10 => {
            let parts: Vec<String> = (0..value.count as usize)
                .map_while(|i| value.rational(i))
                .map(super::binary_data::perl_num_to_string)
                .collect();
            if parts.is_empty() {
                return None;
            }
            if parts.len() == 1 {
                Scalar::Num(super::binary_data::perl_numify(&parts[0]))
            } else {
                Scalar::Text(parts.join(" "))
            }
        }
        _ => {
            let ints = value.ints();
            if ints.is_empty() {
                return None;
            }
            if ints.len() == 1 {
                Scalar::Num(ints[0] as f64)
            } else {
                Scalar::Text(
                    ints.iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            }
        }
    };
    Some((scalar, raw))
}

/// Renders one `Sony::Main` entry, or nothing when no variant applies.
///
/// Returns the printed value and whether the tag is `Priority => 0`.
pub fn render(
    id: u16,
    value: &SonyValue<'_>,
    order: ByteOrder,
    ctx: &mut Ctx,
) -> Option<(&'static str, String, bool)> {
    // ExifTool evaluates the variants in order and takes the first that holds;
    // a Condition may assign a data member on the way past, so every variant up
    // to the winner is evaluated, not just the winner.
    let mut chosen: Option<&'static MainExtraTag> = None;
    for tag in super::main_extra_tables::TAGS.iter().filter(|t| t.id == id) {
        if holds(&tag.cond, ctx, value) {
            chosen = Some(tag);
            break;
        }
    }
    let tag = chosen?;
    let (val, raw) = read_entry(value, tag.fmt, order)?;
    let val = apply_raw(tag.raw, val, ctx)?;
    let val = apply_vc(tag.vc, val, &raw)?;
    Some((
        tag.name,
        apply_pc(tag.pc, val, tag.print_hex),
        tag.low_priority,
    ))
}

/// Whether any variant of this id exists here.
pub fn has(id: u16) -> bool {
    super::main_extra_tables::TAGS.iter().any(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(field_type: u16, count: u32, bytes: Vec<u8>) -> SonyValue<'static> {
        SonyValue::new(field_type, count, bytes, ByteOrder::LittleEndian)
    }

    #[test]
    fn signed_parameters_print_exiftools_plus_sign() {
        // 0x2004 Contrast, int32s +2.
        let v = value(9, 1, 2i32.to_le_bytes().to_vec());
        let mut ctx = Ctx::new(Some("ILCE-7M3"), None);
        let (name, printed, _) = render(0x2004, &v, ByteOrder::LittleEndian, &mut ctx).unwrap();
        assert_eq!((name, printed.as_str()), ("Contrast", "+2"));

        let v = value(9, 1, (-3i32).to_le_bytes().to_vec());
        let (_, printed, _) = render(0x2004, &v, ByteOrder::LittleEndian, &mut ctx).unwrap();
        assert_eq!(printed, "-3");
    }

    #[test]
    fn exposure_mode_drops_exiftools_sentinel() {
        let mut ctx = Ctx::new(Some("SLT-A77"), None);
        let v = value(3, 1, 65535u16.to_le_bytes().to_vec());
        assert!(render(0xb041, &v, ByteOrder::LittleEndian, &mut ctx).is_none());
        let v = value(3, 1, 0u16.to_le_bytes().to_vec());
        let (_, printed, _) = render(0xb041, &v, ByteOrder::LittleEndian, &mut ctx).unwrap();
        assert_eq!(printed, "Program AE");
    }

    /// 0xb042's Condition is an assignment: evaluating it is what gives 0xb043
    /// and 0xb04e their `TagB042`, so a Condition that fails must still have
    /// stored the member.
    #[test]
    fn the_b042_condition_stores_even_when_it_fails() {
        let mut ctx = Ctx::new(Some("DSC-HX9V"), None);
        ctx.set_member(Dm::MetaVersion, Scalar::Text("DC7303320222000".into()));
        let v = value(3, 1, 7u16.to_le_bytes().to_vec());
        // MetaVersion is the excluded one, so 0xb042 reports nothing...
        assert!(render(0xb042, &v, ByteOrder::LittleEndian, &mut ctx).is_none());
        // ...but TagB042 is set all the same.
        assert_eq!(ctx.get(Dm::TagB042).map(Scalar::num), Some(7.0));
    }

    #[test]
    fn an_entry_format_condition_reads_the_tiff_type() {
        let mut ctx = Ctx::new(Some("DSC-P100"), None);
        // 0x1001 MultiBurstImageWidth is int16u-only.
        let as_int16u = value(3, 1, 640u16.to_le_bytes().to_vec());
        assert!(render(0x1001, &as_int16u, ByteOrder::LittleEndian, &mut ctx).is_some());
        let as_undef = value(7, 2, 640u16.to_le_bytes().to_vec());
        assert!(render(0x1001, &as_undef, ByteOrder::LittleEndian, &mut ctx).is_none());
    }
}
