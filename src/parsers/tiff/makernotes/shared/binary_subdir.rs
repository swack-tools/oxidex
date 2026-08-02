//! ExifTool's `ProcessBinaryData`, for MakerNote sub-directories whose layout is
//! a plain fixed record.
//!
//! A vendor MakerNote tag whose ExifTool entry carries
//! `SubDirectory => { TagTable => '...' }` is not a value: it is a nested record
//! that ExifTool descends into and reports several tags out of. A reader that
//! stops at the pointer produces no row at all for those tags -- not a wrong
//! value, not a missing-tag report, nothing -- which is why they can sit unmeasured.
//!
//! This module is the interpreter; the per-vendor tables next to it are generated
//! from ExifTool's own in-memory hashes by
//! `tools/exiftool-tables/codegen_subdirs.py`.
//!
//! It reproduces the parts of `ProcessBinaryData` (Image/ExifTool.pm) these tables
//! use, and nothing else:
//!
//! * the table's `FORMAT` (defaulting to `int8u` when absent, as ExifTool does
//!   with `$$tagTablePtr{FORMAT} || 'int8u'`) fixes the *unit* of a tag key:
//!   the byte offset of a field is `index * sizeof(FORMAT)`. A field's own
//!   `Format` changes how the bytes there are read, never where they start --
//!   so `int16u[4]` at key 5 of an `int16u` table lives at byte 10, not byte 5.
//! * tags are visited in ascending key order, which is what makes a `RawConv`
//!   data member set by a low key visible to a gate on a higher one.
//! * `RawConv => '$$self{X} = $val'` records a data member;
//!   `RawConv => '$$self{X} < n ? undef : $val'` suppresses the tag when fewer
//!   than `n` were reported. Without the gate a record padded with zeros
//!   reports `Face2Position = 0 0 0 0` for a face that was never detected.
//! * a value that would run past the end of the record has its element count
//!   shortened to what fits and is dropped entirely if not even one element
//!   does (ExifTool's `ReadValue`), so a truncated record degrades instead of
//!   reporting garbage.
//!
//! * a field's `Condition`, and the arrayref of `Condition`-bearing alternatives
//!   ExifTool writes when one offset means different things on different bodies.
//!   ExifTool takes the *first* alternative whose condition holds and reports
//!   nothing for that key when none does; a reader that ignores the condition
//!   prints one body's meaning under another body's file. `%Pentax::BatteryInfo`
//!   byte 2 alone is `BodyBatteryADNoLoad` on a K10D, an uncalibrated
//!   `BodyBatteryADNoLoad` on a \*istD, half of `BodyBatteryVoltage1` on a K-5,
//!   and `BodyBatteryState` on a K-3 III (Pentax.pm:4846-4935).
//!
//! `FIRST_ENTRY` is carried on the table but deliberately does not shift any
//! index: in ExifTool it only bounds the synthetic tag range `-U` walks, and the
//! keys of declared tags are absolute either way.

use std::collections::HashMap;

use super::tag_priority;
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// How the bytes at a field's offset are read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Fmt {
    U8,
    I8,
    U16,
    I16,
    /// `int16uRev`: an int16u stored in the opposite byte order to the record.
    U16Rev,
    U32,
    I32,
    F32,
    F64,
    /// `string[n]`: n bytes, truncated at the first NUL.
    Str(u32),
    /// `undef[n]`: n raw bytes, rendered as space-separated decimal.
    Undef(u32),
}

impl Fmt {
    /// Size of one element in bytes.
    pub(crate) const fn size(self) -> usize {
        match self {
            Fmt::U8 | Fmt::I8 => 1,
            Fmt::U16 | Fmt::I16 | Fmt::U16Rev => 2,
            Fmt::U32 | Fmt::I32 | Fmt::F32 => 4,
            Fmt::F64 => 8,
            Fmt::Str(n) | Fmt::Undef(n) => n as usize,
        }
    }
}

/// One branch of an ExifTool model regex, expanded from the pattern text by
/// `tools/exiftool-tables/codegen_subdirs.py`.
///
/// The alternations `%Pentax` writes are finite and literal once the groups and
/// character classes are multiplied out (`/(645D|645Z|K-(1|01|...)|KP)\b/`), so
/// the generator does the expansion and refuses any pattern it cannot expand.
/// Carrying a regex engine here would mean the model test lived somewhere other
/// than ExifTool's own text.
pub(crate) struct ModelPat {
    /// The literal this branch matches, searched for anywhere in the model.
    pub(crate) text: &'static str,
    /// The pattern ended in `\b`, so the character after the match must be a
    /// non-word character or the end of the string. Without this, `/K-5\b/`
    /// would also fire on a "K-5 II" -- which is a different body with a
    /// different `BatteryInfo` layout.
    pub(crate) word_end: bool,
}

/// An ExifTool `Condition` on a field, over `$$self{Model}`.
#[derive(Clone, Copy)]
pub(crate) enum Cond {
    /// No `Condition`: the field always applies.
    Always,
    /// `$$self{Model} =~ /.../`, optionally with the `and $$self{Model} !~ /.../`
    /// second clause `%Pentax::BatteryInfo` uses to exclude the K-3 Mark III
    /// from a pattern that would otherwise catch it (Pentax.pm:4864, :4919).
    ///
    /// An empty `any_of` means the condition is a bare negation.
    Model {
        any_of: &'static [ModelPat],
        none_of: &'static [ModelPat],
    },
    /// `$$self{Model} eq "..."` -- an equality, not a search.
    ModelEq(&'static str),
}

impl Cond {
    /// Whether this condition holds for a file whose `Model` is `model`.
    ///
    /// A condition that reads `$$self{Model}` cannot hold when the model is
    /// unknown: ExifTool's `$$self{Model}` is then undef and neither `=~` nor
    /// `eq` matches. Reporting the field anyway would be guessing at the body.
    pub(crate) fn holds(self, model: Option<&str>) -> bool {
        match self {
            Cond::Always => true,
            Cond::ModelEq(want) => model == Some(want),
            Cond::Model { any_of, none_of } => {
                let Some(model) = model else { return false };
                (any_of.is_empty() || any_of.iter().any(|p| pat_matches(p, model)))
                    && !none_of.iter().any(|p| pat_matches(p, model))
            }
        }
    }
}

/// Perl's `\w`: the character class `\b` is defined against.
const fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `$model =~ /<pat>/` for one expanded branch.
fn pat_matches(pat: &ModelPat, model: &str) -> bool {
    let (hay, needle) = (model.as_bytes(), pat.text.as_bytes());
    if needle.is_empty() {
        return true;
    }
    hay.windows(needle.len()).enumerate().any(|(at, window)| {
        window == needle
            && (!pat.word_end
                || hay
                    .get(at + needle.len())
                    .is_none_or(|&next| !is_word_byte(next)))
    })
}

/// The `PrintConv` ExifTool applies to a field.
#[derive(Clone, Copy)]
pub(crate) enum PrintConv {
    /// No `PrintConv`: the raw number.
    None,
    /// A literal lookup hash. An unlisted value prints as ExifTool's `Unknown (n)`.
    Map(&'static [(i64, &'static str)]),
    /// A lookup hash plus ExifTool's `OTHER` fallback, run for a value the hash
    /// does not list. `OTHER` is an anonymous Perl closure, so the vendor module
    /// carries a hand-written translation of the one body and the generator
    /// binds it by that body's deparsed text -- an upstream edit stops the
    /// generator rather than leaving a stale conversion behind a real tag name.
    MapOr(&'static [(i64, &'static str)], fn(i64) -> String),
    /// A `PrintConv` that is a Perl expression rather than a hash -- a
    /// `sprintf` or a string interpolation. Like `ValueConv`, it is a
    /// computation, so the vendor module carries a hand-written port and the
    /// generator binds it by ExifTool's own expression text.
    ///
    /// It runs on the value *after* `ValueConv`, which is why it takes an `f64`:
    /// `%Pentax::BatteryInfo` divides a raw 686 by 100 and then prints
    /// `sprintf("%.2f V", $val)` (Pentax.pm:4866-4868).
    Expr(fn(f64) -> String),
}

/// The `ValueConv` ExifTool applies before the `PrintConv`.
///
/// A `ValueConv` is a computation, not a lookup, so it cannot be carried as
/// data: the vendor module holds a hand-written port and the generator binds it
/// by ExifTool's own text for the conversion.
#[derive(Clone, Copy)]
pub(crate) enum ValueConv {
    None,
    /// A scalar expression, applied to each element in turn.
    Each(fn(f64) -> f64),
    /// A conversion over the whole element list -- the only shape that can
    /// express one that treats each slot of an array differently.
    List(fn(&[f64]) -> Vec<f64>),
}

/// One field of a `ProcessBinaryData` table.
pub(crate) struct Field {
    /// ExifTool's own tag key verbatim, e.g. `"545.1"`.
    ///
    /// This is the identity of a *tag*, which `index` is not: `545`, `545.1` and
    /// `545.2` are three masked tags at one offset, while two entries both keyed
    /// `2` are two `Condition`-guarded readings of the same tag and at most one
    /// of them may fire. Adjacent fields sharing a key are those alternatives,
    /// in ExifTool's own order.
    pub(crate) key: &'static str,
    /// ExifTool's own tag key: an index in units of the table's `FORMAT`.
    pub(crate) index: i64,
    /// ExifTool's `Condition` on this alternative.
    pub(crate) cond: Cond,
    pub(crate) name: &'static str,
    /// Overrides the table's `FORMAT` for reading, not for locating, this field.
    pub(crate) format: Option<Fmt>,
    /// Element repetitions, from ExifTool's `fmt[N]` array syntax.
    pub(crate) count: usize,
    /// `RawConv => '$$self{X} = $val'`: the data member this field records.
    pub(crate) set_member: Option<&'static str>,
    /// `RawConv => '$$self{X} < n ? undef : $val'`: suppress below a count.
    pub(crate) gate: Option<(&'static str, i64)>,
    /// ExifTool's `Mask`. Applied as `($val & mask) >> BitShift`, where
    /// `BitShift` is the mask's trailing-zero count -- ExifTool derives it in
    /// `ExifTool.pm:5893-5898` and applies it at `ExifTool.pm:10056`. Masking
    /// without the shift would leave the field's value multiplied by its own
    /// low bit and quietly disagree on every packed field.
    pub(crate) mask: Option<i64>,
    /// Runs after `Mask` and before `print_conv`, as in ExifTool.
    pub(crate) value_conv: ValueConv,
    pub(crate) print_conv: PrintConv,
    /// ExifTool's `Priority => 0` (or `Avoid => 1`, which implies it at
    /// `ExifTool.pm:9472`): this field never displaces a value already reported
    /// under the same name. A sub-directory copy of a tag the vendor's `Main`
    /// table also carries is normally marked this way, so that the `Main` copy
    /// is the one that prints. See
    /// [`shared::tag_priority`](super::tag_priority).
    pub(crate) low_priority: bool,
}

/// A `ProcessBinaryData` table.
pub(crate) struct BinaryTable {
    pub(crate) name: &'static str,
    /// ExifTool's `FORMAT`, defaulted to `int8u` by the generator when absent.
    pub(crate) default_format: Fmt,
    /// ExifTool's `FIRST_ENTRY`. Recorded for provenance; see the module note.
    #[allow(dead_code)]
    pub(crate) first_entry: i64,
    pub(crate) fields: &'static [Field],
}

/// One decoded element: either a number or an already-rendered string.
enum Elem {
    Num(i64),
    Real(f64),
    Text(String),
}

fn read_u16(bytes: &[u8], order: ByteOrder) -> u16 {
    match order {
        ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
    }
}

/// ExifTool's `int16uRev` is an int16u stored the other way round from the rest
/// of the record.
const fn flipped(order: ByteOrder) -> ByteOrder {
    match order {
        ByteOrder::LittleEndian => ByteOrder::BigEndian,
        ByteOrder::BigEndian => ByteOrder::LittleEndian,
    }
}

fn read_u32(bytes: &[u8], order: ByteOrder) -> u32 {
    let b = [bytes[0], bytes[1], bytes[2], bytes[3]];
    match order {
        ByteOrder::LittleEndian => u32::from_le_bytes(b),
        ByteOrder::BigEndian => u32::from_be_bytes(b),
    }
}

/// Reads one element of `fmt` at `at`, or `None` if it does not fit.
fn read_elem(record: &[u8], at: usize, fmt: Fmt, order: ByteOrder) -> Option<Elem> {
    let need = fmt.size();
    let bytes = record.get(at..at.checked_add(need)?)?;
    Some(match fmt {
        Fmt::U8 => Elem::Num(i64::from(bytes[0])),
        Fmt::I8 => Elem::Num(i64::from(bytes[0] as i8)),
        Fmt::U16 => Elem::Num(i64::from(read_u16(bytes, order))),
        Fmt::I16 => Elem::Num(i64::from(read_u16(bytes, order) as i16)),
        Fmt::U16Rev => Elem::Num(i64::from(read_u16(bytes, flipped(order)))),
        Fmt::U32 => Elem::Num(i64::from(read_u32(bytes, order))),
        Fmt::I32 => Elem::Num(i64::from(read_u32(bytes, order) as i32)),
        Fmt::F32 => Elem::Real(f64::from(f32::from_bits(read_u32(bytes, order)))),
        Fmt::F64 => {
            let hi = u64::from(read_u32(&bytes[0..4], order));
            let lo = u64::from(read_u32(&bytes[4..8], order));
            let bits = match order {
                ByteOrder::LittleEndian => hi | (lo << 32),
                ByteOrder::BigEndian => (hi << 32) | lo,
            };
            Elem::Real(f64::from_bits(bits))
        }
        Fmt::Str(_) => {
            // ExifTool truncates a `string[n]` at the first NUL and does nothing
            // else: `$vals[0] =~ s/\0.*//s if $format eq 'string'` (ExifTool.pm
            // ReadValue, 13.55:6288). It does not trim trailing whitespace, and
            // adding a trim here would disagree with it on padded fields.
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            Elem::Text(String::from_utf8_lossy(&bytes[..end]).into_owned())
        }
        Fmt::Undef(_) => Elem::Text(
            bytes
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        ),
    })
}

fn render(elem: &Elem, conv: PrintConv) -> String {
    match elem {
        Elem::Text(s) => s.clone(),
        Elem::Real(v) => match conv {
            PrintConv::Expr(f) => f(*v),
            // ExifTool prints a float with %.6g-like trimming; the integral case
            // is by far the common one in these tables.
            _ => {
                if v.fract() == 0.0 && v.abs() < 1e15 {
                    format!("{}", *v as i64)
                } else {
                    format!("{v}")
                }
            }
        },
        Elem::Num(v) => match conv {
            PrintConv::None => v.to_string(),
            PrintConv::Expr(f) => f(*v as f64),
            PrintConv::Map(table) => table.iter().find(|(key, _)| key == v).map_or_else(
                || format!("Unknown ({v})"),
                |(_, label)| (*label).to_string(),
            ),
            PrintConv::MapOr(table, other) => table
                .iter()
                .find(|(key, _)| key == v)
                .map_or_else(|| other(*v), |(_, label)| (*label).to_string()),
        },
    }
}

/// Decodes one `ProcessBinaryData` record into `<prefix>:<Name>` tags.
///
/// `record` is the whole sub-directory as ExifTool sees it -- the raw bytes of the
/// MakerNote tag that carries the `SubDirectory`.
pub(crate) fn decode_binary_subdir(
    table: &BinaryTable,
    record: &[u8],
    order: ByteOrder,
    prefix: &str,
    tags: &mut HashMap<String, String>,
) {
    // A record whose gates only read members it sets itself needs no history.
    let mut members = Members::new();
    decode_binary_subdir_with(table, record, order, prefix, None, &mut members, tags);
}

/// ExifTool's `$$self{...}` slots, threaded across the sub-directories of one
/// file.
///
/// A `RawConv` data member is set on the ExifTool object, not on the record, so
/// a later directory can read one an earlier directory wrote. `%Pentax` relies
/// on exactly that: `FaceInfo` (tag 0x0060) sets `FacesDetected`, and every
/// field of `FacePos` and `FaceSize` (0x0227, 0x0228) is gated on it. Give each
/// of those its own empty map and they report nothing at all.
pub(crate) type Members = HashMap<&'static str, i64>;

/// As [`decode_binary_subdir`], but over data members shared with the other
/// directories of the same file.
pub(crate) fn decode_binary_subdir_with(
    table: &BinaryTable,
    record: &[u8],
    order: ByteOrder,
    prefix: &str,
    model: Option<&str>,
    members: &mut Members,
    tags: &mut HashMap<String, String>,
) {
    let unit = table.default_format.size();

    // Generated tables are already sorted by index; ExifTool visits them in
    // ascending order so a DataMember is set before any gate that reads it.
    //
    // Fields sharing an ExifTool key are the `Condition`-guarded alternatives of
    // one tag. ExifTool takes the first whose condition holds and reports
    // nothing when none does, so the group is consumed as a unit rather than
    // field by field -- otherwise a body matching the second alternative would
    // also be offered the first one's reading of the same bytes.
    let mut rest = table.fields;
    while let Some((first, tail)) = rest.split_first() {
        let group_len = 1 + tail.iter().take_while(|f| f.key == first.key).count();
        let (group, tail) = rest.split_at(group_len);
        rest = tail;
        let Some(field) = group.iter().find(|f| f.cond.holds(model)) else {
            continue;
        };
        if let Some((member, minimum)) = field.gate {
            // A gate on a member no record set is not satisfied: ExifTool's
            // `$$self{X}` is then undef, and `undef < n` is true in numeric
            // context, so the tag is suppressed.
            if members.get(member).copied().unwrap_or(0) < minimum {
                continue;
            }
        }

        let fmt = field.format.unwrap_or(table.default_format);
        let Some(start) = usize::try_from(field.index)
            .ok()
            .and_then(|i| i.checked_mul(unit))
        else {
            continue;
        };

        let mut parts = Vec::with_capacity(field.count);
        let mut numbers: Vec<i64> = Vec::with_capacity(field.count);
        let mut first_num: Option<i64> = None;
        for element in 0..field.count {
            let Some(at) = element
                .checked_mul(fmt.size())
                .and_then(|d| start.checked_add(d))
            else {
                break;
            };
            let Some(elem) = read_elem(record, at, fmt, order) else {
                // ExifTool's ReadValue shortens the count to what fits rather
                // than dropping a partially present array.
                break;
            };
            if let Elem::Num(v) = elem {
                let v = field.mask.map_or(v, |m| (v & m) >> m.trailing_zeros());
                if first_num.is_none() {
                    first_num = Some(v);
                }
                numbers.push(v);
                parts.push(render(&Elem::Num(v), field.print_conv));
            } else {
                parts.push(render(&elem, field.print_conv));
            }
        }
        if parts.is_empty() {
            continue;
        }

        // ExifTool's order is RawConv, then ValueConv, then PrintConv, and a
        // `PrintConv` that is an expression sees the converted value: Pentax's
        // `BodyBatteryVoltage1` divides the raw 686 by 100 and only then runs
        // `sprintf("%.2f V", $val)`. The generator still refuses a `ValueConv`
        // paired with a *hash* `PrintConv`, which would mean looking a computed
        // number up in a table of raw ones.
        match field.value_conv {
            ValueConv::None => {}
            ValueConv::Each(f) => {
                parts = numbers
                    .iter()
                    .map(|&v| render(&Elem::Real(f(v as f64)), field.print_conv))
                    .collect();
            }
            ValueConv::List(f) => {
                let input: Vec<f64> = numbers.iter().map(|&v| v as f64).collect();
                parts = f(&input)
                    .into_iter()
                    .map(|v| render(&Elem::Real(v), field.print_conv))
                    .collect();
            }
        }
        if parts.is_empty() {
            continue;
        }

        if let (Some(member), Some(value)) = (field.set_member, first_num) {
            members.insert(member, value);
        }
        let key = format!("{prefix}:{}", field.name);
        let value = parts.join(" ");
        if field.low_priority {
            // ExifTool's `Priority => 0`: a sub-directory copy of a tag the
            // `Main` table also reports must not overwrite it.
            tag_priority::insert_low_priority(tags, key, value);
        } else {
            tags.insert(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T_CONV: &[(i64, &str)] = &[(0, "Off"), (1, "On")];

    static T_TABLE: BinaryTable = BinaryTable {
        name: "Test",
        default_format: Fmt::U16,
        first_entry: 0,
        fields: &[
            Field {
                key: "0",
                index: 0,
                cond: Cond::Always,
                name: "Count",
                format: Some(Fmt::U16),
                count: 1,
                set_member: Some("Count"),
                gate: None,
                mask: None,
                value_conv: ValueConv::None,
                print_conv: PrintConv::None,
                low_priority: false,
            },
            Field {
                key: "1",
                index: 1,
                cond: Cond::Always,
                name: "First",
                format: Some(Fmt::U16),
                count: 4,
                set_member: None,
                gate: Some(("Count", 1)),
                mask: None,
                value_conv: ValueConv::None,
                print_conv: PrintConv::None,
                low_priority: false,
            },
            Field {
                key: "5",
                index: 5,
                cond: Cond::Always,
                name: "Second",
                format: Some(Fmt::U16),
                count: 4,
                set_member: None,
                gate: Some(("Count", 2)),
                mask: None,
                value_conv: ValueConv::None,
                print_conv: PrintConv::None,
                low_priority: false,
            },
        ],
    };

    fn decode(bytes: &[u8]) -> HashMap<String, String> {
        let mut tags = HashMap::new();
        decode_binary_subdir(&T_TABLE, bytes, ByteOrder::LittleEndian, "X", &mut tags);
        tags
    }

    /// The exact 42 record bytes `exiftool -v3` prints for
    /// `combined-samples/Panasonic/PanasonicDC-S1.jpg` tag 0x004e.
    const DC_S1_FACEDET: [u8; 42] = [
        0x02, 0x00, 0x2e, 0x00, 0x51, 0x00, 0x1b, 0x00, 0x1b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn index_is_scaled_by_the_table_format() {
        // Key 1 of an int16u table is byte 2, so Face1Position must read the
        // 0x2e word -- reading byte 1 would report a neighbour's high half.
        let tags = decode(&DC_S1_FACEDET);
        assert_eq!(tags.get("X:Count").map(String::as_str), Some("2"));
        assert_eq!(tags.get("X:First").map(String::as_str), Some("46 81 27 27"));
    }

    #[test]
    fn gate_suppresses_below_the_recorded_count() {
        // Count = 2, so Second is reported (as the zeros really in the record)
        // and a third would not be.
        let tags = decode(&DC_S1_FACEDET);
        assert_eq!(tags.get("X:Second").map(String::as_str), Some("0 0 0 0"));

        let mut zero = DC_S1_FACEDET;
        zero[0] = 0;
        let tags = decode(&zero);
        assert_eq!(tags.get("X:Count").map(String::as_str), Some("0"));
        assert!(!tags.contains_key("X:First"));
        assert!(!tags.contains_key("X:Second"));
    }

    #[test]
    fn short_record_shortens_the_count_then_drops() {
        // 6 bytes: Count plus two of First's four elements.
        let tags = decode(&DC_S1_FACEDET[..6]);
        assert_eq!(tags.get("X:First").map(String::as_str), Some("46 81"));
        assert!(!tags.contains_key("X:Second"));

        // 2 bytes: only Count fits.
        let tags = decode(&DC_S1_FACEDET[..2]);
        assert_eq!(tags.get("X:Count").map(String::as_str), Some("2"));
        assert!(!tags.contains_key("X:First"));
    }

    #[test]
    fn empty_record_yields_nothing() {
        assert!(decode(&[]).is_empty());
    }

    #[test]
    fn string_is_truncated_at_nul() {
        static S_TABLE: BinaryTable = BinaryTable {
            name: "S",
            default_format: Fmt::U8,
            first_entry: 0,
            fields: &[Field {
                key: "0",
                index: 0,
                cond: Cond::Always,
                name: "Name",
                format: Some(Fmt::Str(8)),
                count: 1,
                set_member: None,
                gate: None,
                mask: None,
                value_conv: ValueConv::None,
                print_conv: PrintConv::None,
                low_priority: false,
            }],
        };
        let mut tags = HashMap::new();
        decode_binary_subdir(
            &S_TABLE,
            b"Bob\0\xff\xff\xff\xff",
            ByteOrder::LittleEndian,
            "X",
            &mut tags,
        );
        assert_eq!(tags.get("X:Name").map(String::as_str), Some("Bob"));
    }

    #[test]
    fn print_conv_maps_and_falls_back_to_unknown() {
        static P_TABLE: BinaryTable = BinaryTable {
            name: "P",
            default_format: Fmt::U8,
            first_entry: 0,
            fields: &[
                Field {
                    key: "0",
                    index: 0,
                    cond: Cond::Always,
                    name: "Known",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: None,
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::Map(T_CONV),
                    low_priority: false,
                },
                Field {
                    key: "1",
                    index: 1,
                    cond: Cond::Always,
                    name: "Other",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: None,
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::Map(T_CONV),
                    low_priority: false,
                },
            ],
        };
        let mut tags = HashMap::new();
        decode_binary_subdir(&P_TABLE, &[1, 7], ByteOrder::LittleEndian, "X", &mut tags);
        assert_eq!(tags.get("X:Known").map(String::as_str), Some("On"));
        assert_eq!(tags.get("X:Other").map(String::as_str), Some("Unknown (7)"));
    }

    /// ExifTool derives `BitShift` from `Mask` and applies both
    /// (`ExifTool.pm:5893-5898` and `:10056`), so a masked field reports the
    /// bits' own value, not the value shifted up by the mask's position.
    #[test]
    fn mask_shifts_down_by_its_trailing_zeros() {
        static M_TABLE: BinaryTable = BinaryTable {
            name: "M",
            default_format: Fmt::U16,
            first_entry: 0,
            fields: &[
                Field {
                    key: "0",
                    index: 0,
                    cond: Cond::Always,
                    name: "Low",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: Some(0x000f),
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::None,
                    low_priority: false,
                },
                Field {
                    // ExifTool's own key for a second tag at one offset: `0.1`,
                    // not a repeat of `0`. Two entries keyed `0` would be
                    // `Condition` alternatives, of which only one may fire.
                    key: "0.1",
                    index: 0,
                    cond: Cond::Always,
                    name: "High",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: Some(0xf000),
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::None,
                    low_priority: false,
                },
            ],
        };
        let mut tags = HashMap::new();
        // 0xa005 little-endian
        decode_binary_subdir(
            &M_TABLE,
            &[0x05, 0xa0],
            ByteOrder::LittleEndian,
            "X",
            &mut tags,
        );
        assert_eq!(tags.get("X:Low").map(String::as_str), Some("5"));
        assert_eq!(tags.get("X:High").map(String::as_str), Some("10"));
    }

    #[test]
    fn big_endian_record_reads_the_same_numbers() {
        let mut swapped = DC_S1_FACEDET;
        for pair in swapped.chunks_exact_mut(2) {
            pair.swap(0, 1);
        }
        let mut tags = HashMap::new();
        decode_binary_subdir(&T_TABLE, &swapped, ByteOrder::BigEndian, "X", &mut tags);
        assert_eq!(tags.get("X:First").map(String::as_str), Some("46 81 27 27"));
    }

    /// Perl's `\b` after an alternation, which is what separates a `K-5` body
    /// from a `K-50` -- two different `%Pentax::BatteryInfo` layouts.
    #[test]
    fn word_boundary_is_the_difference_between_k5_and_k50() {
        static K5: Cond = Cond::Model {
            any_of: &[ModelPat {
                text: "K-5",
                word_end: true,
            }],
            none_of: &[],
        };
        assert!(K5.holds(Some("PENTAX K-5")));
        assert!(K5.holds(Some("PENTAX K-5 II s")));
        assert!(!K5.holds(Some("PENTAX K-50")));
        assert!(!K5.holds(Some("PENTAX K-500")));
        assert!(!K5.holds(None));

        // Without the boundary the same literal is a plain substring search.
        static ANY: Cond = Cond::Model {
            any_of: &[ModelPat {
                text: "K-5",
                word_end: false,
            }],
            none_of: &[],
        };
        assert!(ANY.holds(Some("PENTAX K-500")));
    }

    /// `A and $$self{Model} !~ /B/`: `none_of` vetoes a model `any_of` accepts.
    #[test]
    fn negated_clause_vetoes_a_matching_alternation() {
        static C: Cond = Cond::Model {
            any_of: &[ModelPat {
                text: "K-3",
                word_end: true,
            }],
            none_of: &[ModelPat {
                text: "III",
                word_end: false,
            }],
        };
        assert!(C.holds(Some("PENTAX K-3")));
        assert!(!C.holds(Some("PENTAX K-3 Mark III")));
        assert!(Cond::ModelEq("PENTAX K-3 II").holds(Some("PENTAX K-3 II")));
        assert!(!Cond::ModelEq("PENTAX K-3 II").holds(Some("PENTAX K-3 II s")));
    }

    /// Three alternatives of one key: the first whose condition holds wins, the
    /// rest are not offered, and a model matching none reports nothing for that
    /// key at all.
    #[test]
    fn variants_take_the_first_match_in_exiftools_order() {
        static V_TABLE: BinaryTable = BinaryTable {
            name: "V",
            default_format: Fmt::U8,
            first_entry: 0,
            fields: &[
                Field {
                    key: "0",
                    index: 0,
                    cond: Cond::Model {
                        any_of: &[ModelPat {
                            text: "Alpha",
                            word_end: false,
                        }],
                        none_of: &[],
                    },
                    name: "First",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: None,
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::None,
                    low_priority: false,
                },
                Field {
                    key: "0",
                    index: 0,
                    cond: Cond::Model {
                        any_of: &[
                            ModelPat {
                                text: "Alpha",
                                word_end: false,
                            },
                            ModelPat {
                                text: "Beta",
                                word_end: false,
                            },
                        ],
                        none_of: &[],
                    },
                    name: "Second",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: None,
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::None,
                    low_priority: false,
                },
                // A different key at the same offset: a masked neighbour, not
                // an alternative, so it is decoded independently.
                Field {
                    key: "0.1",
                    index: 0,
                    cond: Cond::Always,
                    name: "Neighbour",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: Some(0xf0),
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::None,
                    low_priority: false,
                },
            ],
        };
        let decode = |model| {
            let mut tags = HashMap::new();
            let mut members = Members::new();
            decode_binary_subdir_with(
                &V_TABLE,
                &[0x42],
                ByteOrder::BigEndian,
                "X",
                model,
                &mut members,
                &mut tags,
            );
            tags
        };

        let alpha = decode(Some("Alpha"));
        assert_eq!(alpha.get("X:First").map(String::as_str), Some("66"));
        assert!(!alpha.contains_key("X:Second"));

        let beta = decode(Some("Beta"));
        assert_eq!(beta.get("X:Second").map(String::as_str), Some("66"));
        assert!(!beta.contains_key("X:First"));

        // No alternative applies: the key produces nothing rather than the
        // wrong body's reading. The neighbouring key still does.
        let gamma = decode(Some("Gamma"));
        assert!(!gamma.contains_key("X:First"));
        assert!(!gamma.contains_key("X:Second"));
        assert_eq!(gamma.get("X:Neighbour").map(String::as_str), Some("4"));
    }

    /// ExifTool runs `ValueConv` and then hands the result to `PrintConv`, so
    /// an expression `PrintConv` must see the converted number: a raw 686 of
    /// centivolts prints "6.86 V", not "686.00 V".
    #[test]
    fn expression_print_conv_runs_after_the_value_conv() {
        fn div_100(v: f64) -> f64 {
            v / 100.0
        }
        fn volts(v: f64) -> String {
            format!("{v:.2} V")
        }
        static E_TABLE: BinaryTable = BinaryTable {
            name: "E",
            default_format: Fmt::U8,
            first_entry: 0,
            fields: &[Field {
                key: "0",
                index: 0,
                cond: Cond::Always,
                name: "Volts",
                format: Some(Fmt::U16),
                count: 1,
                set_member: None,
                gate: None,
                mask: None,
                value_conv: ValueConv::Each(div_100),
                print_conv: PrintConv::Expr(volts),
                low_priority: false,
            }],
        };
        let mut tags = HashMap::new();
        decode_binary_subdir(
            &E_TABLE,
            &[0x02, 0xae],
            ByteOrder::BigEndian,
            "X",
            &mut tags,
        );
        assert_eq!(tags.get("X:Volts").map(String::as_str), Some("6.86 V"));
    }
}
