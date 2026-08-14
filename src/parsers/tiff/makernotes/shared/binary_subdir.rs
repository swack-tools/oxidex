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
use crate::exiftool_tables::engine::{self, Cursor, Step};
use crate::exiftool_tables::{Fmt as TableFmt, runtime::DecodedValue};
use crate::io::ByteOrder as IoByteOrder;
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
    /// A hash `PrintConv` carrying a `BITMASK` sub-hash, e.g.
    /// `{ 0 => 'Auto', BITMASK => { 0 => 'Select', 1 => 'Fixed Center' } }`
    /// (`%Pentax::CameraSettings`'s `AFPointMode`, Pentax.pm:3454-3465).
    ///
    /// ExifTool's dispatch (`ExifTool.pm:3614-3618`) tries an exact match
    /// against `direct` first; only when that misses does it decode the value
    /// bit by bit against `bits` (`DecodeBits`, `ExifTool.pm:6385`). This is
    /// not a variant of `Map`/`MapOr`: those never fall through to a bitwise
    /// reading, and a hash with `BITMASK` never falls through to
    /// `Unknown (n)`.
    Bitmask(
        &'static [(i64, &'static str)],
        &'static [(i64, &'static str)],
    ),
}

/// ExifTool's `DecodeBits` (`ExifTool.pm:6385`), for a `PrintConv` hash's
/// `BITMASK` sub-hash: each set bit of `val` becomes its label, joined by
/// `", "`; a set bit `BITMASK` does not name prints as `[n]`; no bits set
/// prints `(none)`.
fn decode_bits(val: i64, bits: &[(i64, &str)]) -> String {
    let mut out = Vec::new();
    for bit in 0..32i64 {
        if val & (1 << bit) == 0 {
            continue;
        }
        match bits.iter().find(|(b, _)| *b == bit) {
            Some((_, label)) => out.push((*label).to_string()),
            None => out.push(format!("[{bit}]")),
        }
    }
    if out.is_empty() {
        "(none)".to_string()
    } else {
        out.join(", ")
    }
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

/// This module's `Fmt` in the generated-schema vocabulary the shared
/// `ReadValue` speaks. Every variant has an exact counterpart there -- this
/// enum is a subset of `exiftool_tables::Fmt`, missing only the rational
/// formats these sub-directory tables never declare -- so this is a renaming,
/// not a conversion, and it cannot lose a format.
const fn shared_fmt(fmt: Fmt) -> TableFmt {
    match fmt {
        Fmt::U8 => TableFmt::Int8u,
        Fmt::I8 => TableFmt::Int8s,
        Fmt::U16 => TableFmt::Int16u,
        Fmt::I16 => TableFmt::Int16s,
        // ExifTool's `int16uRev` is an int16u stored the other way round
        // from the rest of the record; the shared reader flips it the same
        // way (`exiftool_tables::runtime::decode_value_of`).
        Fmt::U16Rev => TableFmt::Int16uRev,
        Fmt::U32 => TableFmt::Int32u,
        Fmt::I32 => TableFmt::Int32s,
        Fmt::F32 => TableFmt::Float,
        Fmt::F64 => TableFmt::Double,
        Fmt::Str(n) => TableFmt::Str(n),
        Fmt::Undef(n) => TableFmt::Undef(n),
    }
}

/// Reads one element of `fmt` at `at` through the single `ReadValue` port
/// (ExifTool.pm:6286) Step 28 folded the three engines onto.
///
/// `more` is ExifTool's own `$more` -- the bytes of record left at `at` --
/// which is what lets the shared reader apply ExifTool's count shortening
/// (ExifTool.pm:6301-6303) rather than this module's older all-or-nothing
/// bounds check. For the scalar formats the two agree exactly; for a
/// `string[n]`/`undef[n]` running into the end of a truncated record,
/// ExifTool reports the bytes that fit and this now does too.
///
/// `string` is truncated at the first NUL and nothing else
/// (`$vals[0] =~ s/\0.*//s if $format eq 'string'`, ExifTool.pm:6311): no
/// trailing-whitespace trim, which would disagree with ExifTool on padded
/// fields. `undef` is rendered here as space-separated decimals, which is
/// this table set's own convention and stays this module's business, not the
/// shared reader's.
fn read_elem(record: &[u8], at: usize, fmt: Fmt, order: ByteOrder) -> Option<Elem> {
    let order = match order {
        ByteOrder::LittleEndian => IoByteOrder::Little,
        ByteOrder::BigEndian => IoByteOrder::Big,
    };
    let more = i64::try_from(record.len().saturating_sub(at)).ok()?;
    match engine::read_value(record, at, shared_fmt(fmt), 1, more, order)? {
        DecodedValue::Integer(n) => Some(Elem::Num(n)),
        DecodedValue::Float(v) => Some(Elem::Real(v)),
        DecodedValue::String(s) => Some(Elem::Text(s)),
        DecodedValue::Undefined(bytes) => Some(Elem::Text(
            bytes
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        )),
        // No field in these tables declares a rational or an array format, so
        // the shared reader cannot produce one from a `shared_fmt` output.
        DecodedValue::UnsignedRational(..)
        | DecodedValue::SignedRational(..)
        | DecodedValue::Array(_) => None,
    }
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
            PrintConv::Bitmask(direct, bits) => direct
                .iter()
                .find(|(key, _)| key == v)
                .map_or_else(|| decode_bits(*v, bits), |(_, label)| (*label).to_string()),
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
    // Step 28: one port of ExifTool.pm:9957-9964's offset arithmetic. This
    // module had no `varSize` and no negative indices, and skipped an
    // out-of-range field where ExifTool ends the walk (`last if $more <= 0`,
    // ExifTool.pm:9964) -- equivalent for an ascending fixed-width table,
    // which is all these are, but "equivalent by argument" is exactly the
    // kind of claim three separate copies stopped being able to make.
    let unit = table.default_format.size();
    let cursor = Cursor::new(record.len() as i64, unit as i64);

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
        let (start, _more) = match cursor.step(field.index) {
            Step::At { entry, more } => match usize::try_from(entry) {
                Ok(entry) => (entry, more),
                Err(_) => continue,
            },
            Step::Skip => continue,
            Step::Stop => break,
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

    /// `%Pentax::CameraSettings`'s `AFPointMode` (Pentax.pm:3454-3465): value 0
    /// is the direct-hit override `"Auto"`, not "(none)" from `DecodeBits`.
    #[test]
    fn bitmask_direct_hit_wins_over_decoding() {
        const DIRECT: &[(i64, &str)] = &[(0, "Auto")];
        const BITS: &[(i64, &str)] = &[(0, "Select"), (1, "Fixed Center")];
        assert_eq!(decode_bits_via_render(0, DIRECT, BITS), "Auto");
    }

    /// Two bits set decode to both labels, joined by `", "`.
    #[test]
    fn bitmask_joins_multiple_set_bits() {
        const DIRECT: &[(i64, &str)] = &[(0, "Single-frame")];
        const BITS: &[(i64, &str)] = &[(0, "Continuous"), (1, "Continuous (Lo)")];
        assert_eq!(
            decode_bits_via_render(3, DIRECT, BITS),
            "Continuous, Continuous (Lo)"
        );
    }

    /// A set bit `BITMASK` doesn't name (Pentax.pm:3462: "have seen bit 2 set
    /// in pre-production images") still gets reported, as `[n]`.
    #[test]
    fn bitmask_unlisted_bit_renders_bracketed() {
        const DIRECT: &[(i64, &str)] = &[(0, "Auto")];
        const BITS: &[(i64, &str)] = &[(0, "Select"), (1, "Fixed Center")];
        assert_eq!(decode_bits_via_render(4, DIRECT, BITS), "[2]");
    }

    /// A value with no bits set at all -- not reachable from a real `%Pentax`
    /// table today (0 is always the direct hit there), but `DecodeBits` itself
    /// returns `(none)` here rather than an empty string.
    #[test]
    fn bitmask_zero_bits_is_none() {
        assert_eq!(decode_bits(0, &[(1, "x")]), "(none)");
    }

    fn decode_bits_via_render(
        val: i64,
        direct: &'static [(i64, &str)],
        bits: &'static [(i64, &str)],
    ) -> String {
        render(&Elem::Num(val), PrintConv::Bitmask(direct, bits))
    }
}
