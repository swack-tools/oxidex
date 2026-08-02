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
//! `FIRST_ENTRY` is carried on the table but deliberately does not shift any
//! index: in ExifTool it only bounds the synthetic tag range `-U` walks, and the
//! keys of declared tags are absolute either way.

use std::collections::HashMap;

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
    /// ExifTool's own tag key: an index in units of the table's `FORMAT`.
    pub(crate) index: i64,
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
        Elem::Real(v) => {
            // ExifTool prints a float with %.6g-like trimming; the integral case
            // is by far the common one in these tables.
            if v.fract() == 0.0 && v.abs() < 1e15 {
                format!("{}", *v as i64)
            } else {
                format!("{v}")
            }
        }
        Elem::Num(v) => match conv {
            PrintConv::None => v.to_string(),
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
    decode_binary_subdir_with(table, record, order, prefix, &mut members, tags);
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
    members: &mut Members,
    tags: &mut HashMap<String, String>,
) {
    let unit = table.default_format.size();

    // Generated tables are already sorted by index; ExifTool visits them in
    // ascending order so a DataMember is set before any gate that reads it.
    for field in table.fields {
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

        // ExifTool's order is RawConv, then ValueConv, then PrintConv. Nothing
        // transcribed here carries both a ValueConv and a PrintConv -- the
        // generator refuses that pairing -- so a converted value renders as the
        // number it converted to.
        match field.value_conv {
            ValueConv::None => {}
            ValueConv::Each(f) => {
                parts = numbers
                    .iter()
                    .map(|&v| render(&Elem::Real(f(v as f64)), PrintConv::None))
                    .collect();
            }
            ValueConv::List(f) => {
                let input: Vec<f64> = numbers.iter().map(|&v| v as f64).collect();
                parts = f(&input)
                    .into_iter()
                    .map(|v| render(&Elem::Real(v), PrintConv::None))
                    .collect();
            }
        }
        if parts.is_empty() {
            continue;
        }

        if let (Some(member), Some(value)) = (field.set_member, first_num) {
            members.insert(member, value);
        }
        tags.insert(format!("{prefix}:{}", field.name), parts.join(" "));
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
                index: 0,
                name: "Count",
                format: Some(Fmt::U16),
                count: 1,
                set_member: Some("Count"),
                gate: None,
                mask: None,
                value_conv: ValueConv::None,
                print_conv: PrintConv::None,
            },
            Field {
                index: 1,
                name: "First",
                format: Some(Fmt::U16),
                count: 4,
                set_member: None,
                gate: Some(("Count", 1)),
                mask: None,
                value_conv: ValueConv::None,
                print_conv: PrintConv::None,
            },
            Field {
                index: 5,
                name: "Second",
                format: Some(Fmt::U16),
                count: 4,
                set_member: None,
                gate: Some(("Count", 2)),
                mask: None,
                value_conv: ValueConv::None,
                print_conv: PrintConv::None,
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
                index: 0,
                name: "Name",
                format: Some(Fmt::Str(8)),
                count: 1,
                set_member: None,
                gate: None,
                mask: None,
                value_conv: ValueConv::None,
                print_conv: PrintConv::None,
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
                    index: 0,
                    name: "Known",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: None,
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::Map(T_CONV),
                },
                Field {
                    index: 1,
                    name: "Other",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: None,
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::Map(T_CONV),
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
                    index: 0,
                    name: "Low",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: Some(0x000f),
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::None,
                },
                Field {
                    index: 0,
                    name: "High",
                    format: None,
                    count: 1,
                    set_member: None,
                    gate: None,
                    mask: Some(0xf000),
                    value_conv: ValueConv::None,
                    print_conv: PrintConv::None,
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
}
