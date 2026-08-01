//! InfiRay IJPEG APPn records (`Image::ExifTool::InfiRay`).
//!
//! Cameras built on InfiRay's IJPEG SDK (the P2 Pro and friends) spread eight
//! records across JPEG APP2 through APP9. Seven of them are
//! `ProcessBinaryData` tables; the eighth, APP3, is a single opaque blob.
//!
//! None of the seven carries an identifier of its own. ExifTool reads them
//! only after an APP2 segment matching `/^....IJPEG\0/s` has set
//! `$$self{HasIJPEG}`, and then only when the segment clears a per-marker
//! minimum length. Both the flag and the minimums are what keep an unrelated
//! APP4 or APP7 from being decoded as InfiRay data.
//!
//! The field tables live in [`super::infiray_tables`] and are generated from
//! ExifTool's own in-memory hashes by `scripts/gen_infiray_tables.pl`; this
//! module is only the reader. It implements the slice of
//! `ExifTool::ProcessBinaryData` + `ExifTool::ReadValue` those tables reach:
//! little-endian scalars (`SetByteOrder('II')`), NUL-truncated fixed-width
//! strings, and the four `sprintf` print conversions InfiRay.pm declares.

use super::infiray_tables::{Conv, Field, Fmt};
use super::perl_number;
use crate::core::{MetadataMap, TagValue};

/// Byte width of one scalar, i.e. ExifTool's `%formatSize`.
const fn format_size(format: Fmt) -> usize {
    match format {
        Fmt::Int8u | Fmt::Int8s | Fmt::Str => 1,
        Fmt::Int16u | Fmt::Int16s => 2,
        Fmt::Int32u | Fmt::Int32s | Fmt::Float => 4,
        Fmt::Int64u => 8,
    }
}

/// A value as `ReadValue` returns it, before any `PrintConv`.
enum Raw {
    /// One or more integers. Widened to `i128` so an `int64u` near the top of
    /// its range is carried exactly rather than wrapping into a negative.
    Ints(Vec<i128>),
    /// One or more floats.
    Floats(Vec<f64>),
    /// A `string` value, already truncated at its first NUL.
    Text(String),
}

/// Reads one field, mirroring `ExifTool::ReadValue`.
///
/// `more` is the number of bytes available from `entry` to the end of the
/// record. ReadValue shortens an over-long count to fit, and returns undef
/// (here `None`) when not even one scalar fits.
fn read_value(data: &[u8], entry: usize, format: Fmt, count: usize, more: usize) -> Option<Raw> {
    let len = format_size(format);
    let count = if len.checked_mul(count)? > more {
        more / len
    } else {
        count
    };
    if count < 1 {
        return None;
    }
    let bytes = data.get(entry..entry.checked_add(count.checked_mul(len)?)?)?;

    // `string` has no entry in %readValueProc, so ReadValue takes the whole
    // span as one scalar and truncates it at the first NUL.
    if format == Fmt::Str {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        return Some(Raw::Text(
            String::from_utf8_lossy(&bytes[..end]).into_owned(),
        ));
    }

    let mut ints = Vec::new();
    let mut floats = Vec::new();
    for chunk in bytes.chunks_exact(len) {
        // Every InfiRay table is read little-endian (`SetByteOrder('II')`).
        match format {
            Fmt::Int8u => ints.push(i128::from(chunk[0])),
            Fmt::Int8s => ints.push(i128::from(chunk[0] as i8)),
            Fmt::Int16u => ints.push(i128::from(u16::from_le_bytes([chunk[0], chunk[1]]))),
            Fmt::Int16s => ints.push(i128::from(i16::from_le_bytes([chunk[0], chunk[1]]))),
            Fmt::Int32u => ints.push(i128::from(u32::from_le_bytes(
                chunk.try_into().expect("chunks_exact(4)"),
            ))),
            Fmt::Int32s => ints.push(i128::from(i32::from_le_bytes(
                chunk.try_into().expect("chunks_exact(4)"),
            ))),
            Fmt::Int64u => ints.push(i128::from(u64::from_le_bytes(
                chunk.try_into().expect("chunks_exact(8)"),
            ))),
            Fmt::Float => floats.push(f64::from(f32::from_le_bytes(
                chunk.try_into().expect("chunks_exact(4)"),
            ))),
            // Handled above; a Str never reaches this loop.
            Fmt::Str => unreachable!("string is read as one span, not per-scalar"),
        }
    }

    if floats.is_empty() {
        Some(Raw::Ints(ints))
    } else {
        Some(Raw::Floats(floats))
    }
}

/// Renders a value with no `PrintConv`, as ExifTool interpolates it.
///
/// A lone number stays a number; several join with a single space, which is
/// what `ReadValue` does in scalar context. Floats go through
/// [`perl_number`]: a 32-bit float widened to a double keeps its binary
/// error, so ExifTool prints `1.10000002384186`, not `1.1`.
fn render_raw(raw: &Raw) -> TagValue {
    match raw {
        Raw::Text(s) => TagValue::String(s.clone()),
        Raw::Ints(v) => match v.as_slice() {
            [single] => match i64::try_from(*single) {
                Ok(n) => TagValue::Integer(n),
                // An int64u above i64::MAX still has an exact decimal form.
                Err(_) => TagValue::String(single.to_string()),
            },
            many => TagValue::String(
                many.iter()
                    .map(i128::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        },
        Raw::Floats(v) => match v.as_slice() {
            [single] => TagValue::String(perl_number(*single)),
            many => TagValue::String(
                many.iter()
                    .copied()
                    .map(perl_number)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        },
    }
}

/// Applies one of InfiRay.pm's four `PrintConv` expressions (InfiRay.pm:21-24).
///
/// All four are `sprintf` on a single numeric `$val`. The generated tables
/// only ever attach them to a `float` field with count 1 -- asserted by
/// `every_print_conv_sits_on_a_single_float` below -- so a value that is not
/// one number cannot reach here; if one somehow did, it falls back to the raw
/// rendering rather than being printed under a unit it does not have.
fn print_conv(conv: Conv, raw: &Raw) -> TagValue {
    if conv == Conv::Raw {
        return render_raw(raw);
    }
    let val = match raw {
        Raw::Floats(v) => match v.as_slice() {
            [single] => *single,
            _ => return render_raw(raw),
        },
        Raw::Ints(v) => match v.as_slice() {
            [single] => *single as f64,
            _ => return render_raw(raw),
        },
        Raw::Text(_) => return render_raw(raw),
    };
    TagValue::String(match conv {
        // 'sprintf("%.2f", $val)'
        Conv::Float2 => format!("{:.2}", val),
        // 'sprintf("%.1f %%", $val * 100)'
        Conv::Percent => format!("{:.1} %", val * 100.0),
        // 'sprintf("%.2f m", $val)'
        Conv::Meters => format!("{:.2} m", val),
        // 'sprintf("%.2f C", $val)'
        Conv::Celsius => format!("{:.2} C", val),
        // Returned above.
        Conv::Raw => unreachable!("raw conversion is handled before this match"),
    })
}

/// Reads an InfiRay binary-data record into `<group>:<Name>` keys.
///
/// Mirrors `ExifTool::ProcessBinaryData` for the subset these tables use. The
/// tables declare no `FORMAT`, so the default `int8u` increment applies and
/// each key is a plain byte offset; entries are visited in ascending offset
/// order, and the walk stops at the first field that starts at or past the end
/// of the record (`last if $more <= 0`).
///
/// # Arguments
///
/// * `group` - family-0 group name for the emitted keys, e.g. `"APP4"`
/// * `data` - the APPn payload, which is the record itself (no identifier)
/// * `fields` - one of the generated tables in [`super::infiray_tables`]
pub(crate) fn read_record(group: &str, data: &[u8], fields: &[Field]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    for field in fields {
        // `my $more = $size - $entry; last if $more <= 0;`
        let Some(more) = data.len().checked_sub(field.offset).filter(|m| *m > 0) else {
            break;
        };
        // `next unless defined $val`
        let Some(raw) = read_value(data, field.offset, field.format, field.count, more) else {
            continue;
        };
        metadata.insert(
            format!("{}:{}", group, field.name),
            print_conv(field.conv, &raw),
        );
    }
    metadata
}

/// ExifTool's placeholder for a tag it declares `Binary => 1`.
///
/// `JPEG::Main`'s APP3 `ImagingData` (JPEG.pm:119) is such a tag, so ExifTool
/// prints the byte count rather than the bytes.
pub(crate) fn binary_data_placeholder(len: usize) -> TagValue {
    TagValue::String(format!(
        "(Binary data {} bytes, use -b option to extract)",
        len
    ))
}

#[cfg(test)]
mod tests {
    use super::super::infiray_tables::{
        FACTORY, GENERATED_FIELD_COUNT, ISOTHERMAL, MIX_MODE, OP_MODE, PICTURE, SENSOR, VERSION,
    };
    use super::*;

    /// Every generated table, so the invariants below cannot miss a new one.
    const ALL_TABLES: [&[Field]; 7] = [
        VERSION, FACTORY, PICTURE, MIX_MODE, OP_MODE, ISOTHERMAL, SENSOR,
    ];

    #[test]
    fn all_generated_tables_are_wired() {
        let total: usize = ALL_TABLES.iter().map(|t| t.len()).sum();
        assert_eq!(total, GENERATED_FIELD_COUNT);
    }

    /// [`print_conv`] reads a single number out of the value, so a `PrintConv`
    /// on anything else would silently fall back to the raw rendering. Every
    /// one InfiRay.pm declares is on a scalar `float`; this pins that down so
    /// a regenerated table cannot quietly break the assumption.
    #[test]
    fn every_print_conv_sits_on_a_single_float() {
        for table in ALL_TABLES {
            for f in table {
                if f.conv != Conv::Raw {
                    assert_eq!(f.format, Fmt::Float, "{} is not a float", f.name);
                    assert_eq!(f.count, 1, "{} is not a scalar", f.name);
                }
            }
        }
    }

    /// `read_record` walks fields in ascending offset order and stops at the
    /// first one past the end of the record, which is only correct if the
    /// generated tables are sorted.
    #[test]
    fn generated_tables_are_in_ascending_offset_order() {
        for table in ALL_TABLES {
            for pair in table.windows(2) {
                assert!(
                    pair[0].offset < pair[1].offset,
                    "{} (0x{:x}) must precede {} (0x{:x})",
                    pair[0].name,
                    pair[0].offset,
                    pair[1].name,
                    pair[1].offset,
                );
            }
        }
    }

    #[test]
    fn string_is_truncated_at_its_first_nul() {
        // ReadValue: `$vals[0] =~ s/\0.*//s if $format eq 'string'`. The bytes
        // after the NUL are dropped, not appended.
        let data = b"infisense\0XY\0junk";
        let m = read_record(
            "APP9",
            data,
            &[Field {
                offset: 0,
                name: "IRSensorManufacturer",
                format: Fmt::Str,
                count: 12,
                conv: Conv::Raw,
            }],
        );
        assert_eq!(m.get_string("APP9:IRSensorManufacturer"), Some("infisense"));
    }

    #[test]
    fn a_field_past_the_end_of_the_record_stops_the_walk() {
        // ProcessBinaryData: `last if $more <= 0`.
        let fields = [
            Field {
                offset: 0,
                name: "First",
                format: Fmt::Int8u,
                count: 1,
                conv: Conv::Raw,
            },
            Field {
                offset: 8,
                name: "Second",
                format: Fmt::Int8u,
                count: 1,
                conv: Conv::Raw,
            },
        ];
        let m = read_record("APP7", &[1u8, 2, 3, 4], &fields);
        assert_eq!(m.get_integer("APP7:First"), Some(1));
        assert!(m.get("APP7:Second").is_none());
    }

    #[test]
    fn a_truncated_multi_element_field_shortens_rather_than_vanishing() {
        // ReadValue: `$count = int($size / $len)` when the declared count
        // overruns the record. Two of the four declared int8u fit.
        let m = read_record(
            "APP2",
            &[0u8, 2],
            &[Field {
                offset: 0,
                name: "IJPEGVersion",
                format: Fmt::Int8u,
                count: 4,
                conv: Conv::Raw,
            }],
        );
        assert_eq!(m.get_string("APP2:IJPEGVersion"), Some("0 2"));
    }

    #[test]
    fn a_field_with_room_for_no_whole_scalar_is_skipped() {
        // ReadValue: `$count < 1 and return undef`, and ProcessBinaryData
        // does `next unless defined $val` -- so later fields still get a turn.
        let fields = [
            Field {
                offset: 0,
                name: "Wide",
                format: Fmt::Int64u,
                count: 1,
                conv: Conv::Raw,
            },
            Field {
                offset: 1,
                name: "Narrow",
                format: Fmt::Int8u,
                count: 1,
                conv: Conv::Raw,
            },
        ];
        let m = read_record("APP4", &[0x11u8, 0x22, 0x33], &fields);
        assert!(m.get("APP4:Wide").is_none());
        assert_eq!(m.get_integer("APP4:Narrow"), Some(0x22));
    }

    #[test]
    fn signed_and_unsigned_bytes_are_not_confused() {
        // FactDefEmissivity is int8s: ExifTool prints -128 for 0x80, and an
        // int8u reading of the same byte would print 128.
        let fields = [
            Field {
                offset: 0,
                name: "Signed",
                format: Fmt::Int8s,
                count: 1,
                conv: Conv::Raw,
            },
            Field {
                offset: 1,
                name: "Unsigned",
                format: Fmt::Int8u,
                count: 1,
                conv: Conv::Raw,
            },
        ];
        let m = read_record("APP4", &[0x80u8, 0x80], &fields);
        assert_eq!(m.get_integer("APP4:Signed"), Some(-128));
        assert_eq!(m.get_integer("APP4:Unsigned"), Some(128));
    }

    #[test]
    fn int64u_above_i64_max_keeps_its_exact_decimal() {
        let m = read_record(
            "APP2",
            &u64::MAX.to_le_bytes(),
            &[Field {
                offset: 0,
                name: "IRDataSize",
                format: Fmt::Int64u,
                count: 1,
                conv: Conv::Raw,
            }],
        );
        assert_eq!(
            m.get_string("APP2:IRDataSize"),
            Some(u64::MAX.to_string().as_str())
        );
    }

    #[test]
    fn print_conversions_match_their_sprintf() {
        let cases: [(Conv, f32, &str); 4] = [
            (Conv::Float2, 0.99, "0.99"),
            (Conv::Percent, 0.5, "50.0 %"),
            (Conv::Meters, 0.25, "0.25 m"),
            (Conv::Celsius, 25.0, "25.00 C"),
        ];
        for (conv, input, expected) in cases {
            let m = read_record(
                "APP5",
                &input.to_le_bytes(),
                &[Field {
                    offset: 0,
                    name: "T",
                    format: Fmt::Float,
                    count: 1,
                    conv,
                }],
            );
            assert_eq!(m.get_string("APP5:T"), Some(expected), "{:?}", conv);
        }
    }

    #[test]
    fn binary_data_placeholder_matches_exiftool() {
        // `exiftool -a -G1 -s combined-samples/InfiRay.jpg` reports the
        // 20-byte APP3 record as this exact string.
        assert_eq!(
            binary_data_placeholder(20),
            TagValue::String("(Binary data 20 bytes, use -b option to extract)".to_string())
        );
    }
}
