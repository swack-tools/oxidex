//! Runtime for source-derived `WriteExif` new-directory mandatory defaults.
//!
//! This has no public writer route.  All tag IDs, values, directory names and
//! JFIF substitutions are generated operands.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MandatoryValue {
    Integer(i64),
    Text(&'static str),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MandatoryDefault {
    pub tag_id: u16,
    pub value: MandatoryValue,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MandatoryDirectory {
    pub directory: &'static str,
    pub defaults: &'static [MandatoryDefault],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MandatoryCleanup {
    pub all_mandatory: bool,
    pub no_next_ifd: bool,
    pub entry_count_shrinks_or_new: bool,
    pub omit_empty_ifd1: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JfifAssignment {
    pub tag_id: u16,
    pub property: &'static str,
    pub adjustment: i64,
}
/// The source-selected direct `WriteValue` packing format for one IFD0
/// default.  The generator refuses every other native format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DefaultEncoding {
    pub tag_id: u16,
    pub format_name: &'static str,
    pub tiff_type: u16,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SurvivorEncoding {
    pub format_name: &'static str,
    pub tiff_type: u16,
    pub width: u8,
    pub operation: &'static str,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MandatoryRecipe {
    pub writer_source_file: &'static str,
    pub writer_source_sha256: &'static str,
    pub core_source_sha256: &'static str,
    pub exif_source_sha256: &'static str,
    pub write_value_source_sha256: &'static str,
    pub perl_version: &'static str,
    pub no_mandatory_guard: bool,
    pub directories: &'static [MandatoryDirectory],
    pub jfif_directory: &'static str,
    pub jfif_probe: &'static str,
    pub jfif_assignments: &'static [JfifAssignment],
    pub encodings: &'static [DefaultEncoding],
    pub survivor_encodings: &'static [SurvivorEncoding],
    pub cleanup: MandatoryCleanup,
}

/// Refuse cleanup unless the captured WriteExif body authenticated each
/// predicate used by the public IFD1 carrier path.
pub(crate) fn require_ifd1_mandatory_cleanup(recipe: &MandatoryRecipe) -> Result<(), String> {
    if recipe.cleanup
        != (MandatoryCleanup {
            all_mandatory: true,
            no_next_ifd: true,
            entry_count_shrinks_or_new: true,
            omit_empty_ifd1: true,
        })
    {
        return Err(refusal("mandatory IFD1 cleanup source is unsupported"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TiffByteOrder {
    Little,
    Big,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EncodedMandatoryDefault {
    pub tag_id: u16,
    /// TIFF type selected from the captured row's Writable operand.
    pub tiff_type: u16,
    pub count: u32,
    pub bytes: Vec<u8>,
}

/// Reproduce WriteExif's mandatory comparison for a surviving physical entry.
/// WriteExif uses that entry's selected format and count, so this deliberately
/// does not reuse the fixed new-directory encoding. A native packing failure
/// means "not mandatory", so the caller retains IFD1 and still completes the
/// selected deletion.
pub(crate) fn matches_existing_mandatory_value(
    recipe: &MandatoryRecipe,
    default: MandatoryDefault,
    field_type: u16,
    count: u32,
    bytes: &[u8],
    byte_order: TiffByteOrder,
) -> Result<bool, String> {
    let scalar = match default.value {
        MandatoryValue::Integer(value) => value.to_string(),
        MandatoryValue::Text(value) => value.to_owned(),
    };
    let integer = match default.value {
        MandatoryValue::Integer(value) => Some(value),
        MandatoryValue::Text(_) => None,
    };
    let endian = |value: u32| match byte_order {
        TiffByteOrder::Little => value.to_le_bytes().to_vec(),
        TiffByteOrder::Big => value.to_be_bytes().to_vec(),
    };
    let signed = |value: i32| match byte_order {
        TiffByteOrder::Little => value.to_le_bytes().to_vec(),
        TiffByteOrder::Big => value.to_be_bytes().to_vec(),
    };
    if matches!(field_type, 1 | 3 | 4 | 6 | 8 | 9 | 11 | 12 | 5 | 10) && count != 1 {
        return Ok(false);
    }
    let supported = recipe.survivor_encodings.iter().any(|encoding| {
        encoding.tiff_type == field_type && encoding.operation == "write_value_scalar"
    });
    if !supported {
        return Ok(false);
    }
    let expected = match field_type {
        // DoPackStd/Set*s mirror Perl pack and retain the low bits rather
        // than rejecting an out-of-range integral scalar.
        1 => match integer {
            Some(value) => vec![value as u8],
            None => return Ok(false),
        },
        3 => match integer {
            Some(value) => match byte_order {
                TiffByteOrder::Little => (value as u16).to_le_bytes().to_vec(),
                TiffByteOrder::Big => (value as u16).to_be_bytes().to_vec(),
            },
            None => return Ok(false),
        },
        4 => match integer {
            Some(value) => endian(value as u32),
            None => return Ok(false),
        },
        6 => match integer {
            Some(value) => vec![(value as i8) as u8],
            None => return Ok(false),
        },
        8 => match integer {
            Some(value) => match byte_order {
                TiffByteOrder::Little => (value as i16).to_le_bytes().to_vec(),
                TiffByteOrder::Big => (value as i16).to_be_bytes().to_vec(),
            },
            None => return Ok(false),
        },
        9 => match integer {
            Some(value) => signed(value as i32),
            None => return Ok(false),
        },
        11 => match integer {
            Some(value) => match byte_order {
                TiffByteOrder::Little => (value as f32).to_le_bytes().to_vec(),
                TiffByteOrder::Big => (value as f32).to_be_bytes().to_vec(),
            },
            None => return Ok(false),
        },
        12 => match integer {
            Some(value) => match byte_order {
                TiffByteOrder::Little => (value as f64).to_le_bytes().to_vec(),
                TiffByteOrder::Big => (value as f64).to_be_bytes().to_vec(),
            },
            None => return Ok(false),
        },
        5 => {
            let Some(value) = integer.and_then(|value| u32::try_from(value).ok()) else {
                return Ok(false);
            };
            match byte_order {
                TiffByteOrder::Little => [value.to_le_bytes(), 1u32.to_le_bytes()].concat(),
                TiffByteOrder::Big => [value.to_be_bytes(), 1u32.to_be_bytes()].concat(),
            }
        }
        10 => {
            let Some(value) = integer.and_then(|value| i32::try_from(value).ok()) else {
                return Ok(false);
            };
            match byte_order {
                TiffByteOrder::Little => [value.to_le_bytes(), 1i32.to_le_bytes()].concat(),
                TiffByteOrder::Big => [value.to_be_bytes(), 1i32.to_be_bytes()].concat(),
            }
        }
        // Writer.pl:5415-5437. A zero count means infer the payload length;
        // string truncation always reserves its final NUL byte.
        2 => {
            let mut out = scalar.into_bytes();
            out.push(0);
            if count > 0 {
                if out.len() > count as usize {
                    out.truncate(count as usize - 1);
                    out.push(0);
                } else {
                    out.resize(count as usize, 0);
                }
            }
            out
        }
        7 => {
            let mut out = scalar.into_bytes();
            if count > 0 {
                out.resize(count as usize, 0);
                out.truncate(count as usize);
            }
            out
        }
        _ => return Ok(false),
    };
    Ok(bytes == expected)
}

/// Construct the TIFF payload for a newly-created, IFD0-only EXIF block.
///
/// The caller supplies only native branch inputs. Tag ids, types, defaults,
/// and JFIF substitutions all come from the generated recipe. This is an
/// internal carrier for a future JPEG APP1 insertion and does not write files.
pub(crate) fn minimal_ifd0_tiff(
    recipe: &MandatoryRecipe,
    byte_order: TiffByteOrder,
    no_mandatory: bool,
    num_entries: u32,
    jfif: Option<JfifValues>,
) -> Result<Vec<u8>, String> {
    let defaults = defaults_for_new_directory(recipe, "IFD0", no_mandatory, num_entries, jfif)?;
    let entries = encode_mandatory_defaults(recipe, &defaults, byte_order)?;
    serialize_ifd0_defaults(entries, byte_order)
}

/// Pure TIFF framing; an empty set creates a tagless classic IFD0 carrier.
pub(crate) fn serialize_ifd0_defaults(
    mut entries: Vec<EncodedMandatoryDefault>,
    byte_order: TiffByteOrder,
) -> Result<Vec<u8>, String> {
    entries.sort_by_key(|entry| entry.tag_id);
    if entries
        .windows(2)
        .any(|pair| pair[0].tag_id == pair[1].tag_id)
    {
        return Err(refusal(
            "generated mandatory defaults contain duplicate IFD0 ids",
        ));
    }
    let count =
        u16::try_from(entries.len()).map_err(|_| refusal("IFD0 entry count exceeds TIFF limit"))?;
    let ifd_size = 2usize
        .checked_add(
            entries
                .len()
                .checked_mul(12)
                .ok_or_else(|| refusal("IFD0 size overflow"))?,
        )
        .and_then(|size| size.checked_add(4))
        .ok_or_else(|| refusal("IFD0 size overflow"))?;
    let mut result = Vec::with_capacity(
        8 + ifd_size + entries.iter().map(|entry| entry.bytes.len()).sum::<usize>(),
    );
    match byte_order {
        TiffByteOrder::Little => result.extend_from_slice(b"II\x2a\0\x08\0\0\0"),
        TiffByteOrder::Big => result.extend_from_slice(b"MM\0\x2a\0\0\0\x08"),
    }
    push_u16(&mut result, count, byte_order);
    let data_start = 8usize
        .checked_add(ifd_size)
        .ok_or_else(|| refusal("TIFF offset overflow"))?;
    let mut external = Vec::new();
    for entry in entries {
        push_u16(&mut result, entry.tag_id, byte_order);
        push_u16(&mut result, entry.tiff_type, byte_order);
        push_u32(&mut result, entry.count, byte_order);
        if entry.bytes.len() <= 4 {
            result.extend_from_slice(&entry.bytes);
            result.resize(result.len() + (4 - entry.bytes.len()), 0);
        } else {
            let offset = data_start
                .checked_add(external.len())
                .ok_or_else(|| refusal("TIFF offset overflow"))?;
            push_u32(
                &mut result,
                u32::try_from(offset).map_err(|_| refusal("TIFF offset exceeds u32"))?,
                byte_order,
            );
            external.extend_from_slice(&entry.bytes);
            if external.len() & 1 != 0 {
                external.push(0);
            }
        }
    }
    push_u32(&mut result, 0, byte_order); // no next IFD
    result.extend_from_slice(&external);
    Ok(result)
}

fn push_u16(out: &mut Vec<u8>, value: u16, order: TiffByteOrder) {
    match order {
        TiffByteOrder::Little => out.extend_from_slice(&value.to_le_bytes()),
        TiffByteOrder::Big => out.extend_from_slice(&value.to_be_bytes()),
    }
}
fn push_u32(out: &mut Vec<u8>, value: u32, order: TiffByteOrder) {
    match order {
        TiffByteOrder::Little => out.extend_from_slice(&value.to_le_bytes()),
        TiffByteOrder::Big => out.extend_from_slice(&value.to_be_bytes()),
    }
}

/// Execute the admitted direct `WriteValue` packing path for generated numeric
/// mandatory operands in the selected directory. It intentionally has no public
/// writer entry point.
pub(crate) fn encode_mandatory_defaults(
    recipe: &MandatoryRecipe,
    defaults: &[MandatoryDefault],
    byte_order: TiffByteOrder,
) -> Result<Vec<EncodedMandatoryDefault>, String> {
    let mut encoded = Vec::with_capacity(defaults.len());
    for default in defaults {
        let format = recipe
            .encodings
            .iter()
            .find(|item| item.tag_id == default.tag_id)
            .ok_or_else(|| refusal("mandatory default has no generated WriteValue operand"))?;
        let value = match default.value {
            MandatoryValue::Integer(value) => value,
            MandatoryValue::Text(_) => {
                return Err(refusal(
                    "text mandatory default is outside numeric encoder scope",
                ));
            }
        };
        let (tiff_type, count, bytes) = match format.format_name {
            "int16u" => {
                let value = u16::try_from(value)
                    .map_err(|_| refusal("int16u mandatory operand is outside native range"))?;
                let bytes = match byte_order {
                    TiffByteOrder::Little => value.to_le_bytes(),
                    TiffByteOrder::Big => value.to_be_bytes(),
                };
                (format.tiff_type, 1, bytes.to_vec())
            }
            "rational64u" => {
                let value = u32::try_from(value).map_err(|_| {
                    refusal("rational64u mandatory operand is outside native range")
                })?;
                let mut bytes = Vec::with_capacity(8);
                match byte_order {
                    TiffByteOrder::Little => {
                        bytes.extend(value.to_le_bytes());
                        bytes.extend(1u32.to_le_bytes());
                    }
                    TiffByteOrder::Big => {
                        bytes.extend(value.to_be_bytes());
                        bytes.extend(1u32.to_be_bytes());
                    }
                }
                (format.tiff_type, 1, bytes)
            }
            _ => return Err(refusal("mandatory WriteValue format is unsupported")),
        };
        encoded.push(EncodedMandatoryDefault {
            tag_id: default.tag_id,
            tiff_type,
            count,
            bytes,
        });
    }
    Ok(encoded)
}
/// The `WriteValue` operands of a mandatory row the generated recipe carries
/// no numeric encoding for: its `Format || Writable` packing format, the IFD
/// format code (`Writable`) and the `Count` the row declares (`None`: the
/// packed length). WriteExif packs a mandatory default raw, with no
/// conversion (`$newVal = $$mandatory{$newID}`, then `WriteValue` with
/// `$newFormName` and the row's count, WriteExif.pl 13.59:1197-1216), so
/// these three operands decide the entry completely. Transcribed from the
/// pinned rows; the oracle read-back test `tests/exif_mandatory_creation.rs`
/// pins each against the entry pinned ExifTool 13.59 itself creates.
struct CreationOperand {
    directory: &'static str,
    tag_id: u16,
    write_format: &'static str,
    tiff_type: u16,
    count: Option<u32>,
}

const CREATION_OPERANDS: &[CreationOperand] = &[
    // Exif.pm 13.59:2237-2240 ExifVersion: Writable 'undef'.
    CreationOperand {
        directory: "ExifIFD",
        tag_id: 0x9000,
        write_format: "undef",
        tiff_type: 7,
        count: None,
    },
    // Exif.pm 13.59:2296-2302 ComponentsConfiguration: Format 'int8u',
    // Writable 'undef', Count 4.
    CreationOperand {
        directory: "ExifIFD",
        tag_id: 0x9101,
        write_format: "int8u",
        tiff_type: 7,
        count: Some(4),
    },
    // Exif.pm 13.59:2684-2692 ColorSpace: Writable 'int16u'.
    CreationOperand {
        directory: "ExifIFD",
        tag_id: 0xa001,
        write_format: "int16u",
        tiff_type: 3,
        count: Some(1),
    },
    // GPS.pm 13.59:57-61 GPSVersionID: Writable 'int8u', Count 4.
    CreationOperand {
        directory: "GPS",
        tag_id: 0x0000,
        write_format: "int8u",
        tiff_type: 1,
        count: Some(4),
    },
    // Exif.pm 13.59:429-435 InteropVersion: Writable 'undef'.
    CreationOperand {
        directory: "InteropIFD",
        tag_id: 0x0002,
        write_format: "undef",
        tiff_type: 7,
        count: None,
    },
];

/// The entries WriteExif adds to `directory` when it creates it (`unless
/// ($numEntries)`, WriteExif.pl 13.59:714-719): the generated recipe's
/// defaults for that directory -- with its JFIF substitutions, from
/// `properties` -- packed in `byte_order`. A numeric default the recipe
/// carries an encoding for takes that encoding; any other takes its row's
/// [`CreationOperand`]. A default with neither is refused, never guessed.
pub(crate) fn encode_creation_defaults(
    recipe: &MandatoryRecipe,
    directory: &str,
    byte_order: TiffByteOrder,
    properties: &std::collections::BTreeMap<String, i64>,
) -> Result<Vec<EncodedMandatoryDefault>, String> {
    let defaults = defaults_with_properties(recipe, directory, false, 0, properties)?;
    let mut encoded = Vec::with_capacity(defaults.len());
    for default in defaults {
        if recipe
            .encodings
            .iter()
            .any(|item| item.tag_id == default.tag_id)
        {
            encoded.extend(encode_mandatory_defaults(recipe, &[default], byte_order)?);
            continue;
        }
        let operand = CREATION_OPERANDS
            .iter()
            .find(|item| item.directory == directory && item.tag_id == default.tag_id)
            .ok_or_else(|| refusal("mandatory default has no creation operand"))?;
        let (width, bytes) = match (operand.write_format, default.value) {
            ("undef", MandatoryValue::Text(text)) => (1, text.as_bytes().to_vec()),
            ("int8u", MandatoryValue::Text(text)) => (
                1,
                text.split(' ')
                    .map(str::parse::<u8>)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| refusal("int8u mandatory operand is outside native range"))?,
            ),
            ("int16u", MandatoryValue::Integer(value)) => {
                let value = u16::try_from(value)
                    .map_err(|_| refusal("int16u mandatory operand is outside native range"))?;
                let bytes = match byte_order {
                    TiffByteOrder::Little => value.to_le_bytes(),
                    TiffByteOrder::Big => value.to_be_bytes(),
                };
                (2, bytes.to_vec())
            }
            _ => {
                return Err(refusal(
                    "mandatory creation operand does not fit its default",
                ));
            }
        };
        let count = u32::try_from(bytes.len() / width)
            .map_err(|_| refusal("mandatory default count exceeds u32"))?;
        if operand.count.is_some_and(|declared| declared != count) {
            return Err(refusal(
                "mandatory default does not fill its declared count",
            ));
        }
        encoded.push(EncodedMandatoryDefault {
            tag_id: default.tag_id,
            tiff_type: operand.tiff_type,
            count,
            bytes,
        });
    }
    Ok(encoded)
}

/// Compatibility wrapper for the admitted IFD0 fresh-carrier caller.
pub(crate) fn encode_ifd0_defaults(
    recipe: &MandatoryRecipe,
    defaults: &[MandatoryDefault],
    byte_order: TiffByteOrder,
) -> Result<Vec<EncodedMandatoryDefault>, String> {
    encode_mandatory_defaults(recipe, defaults, byte_order)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JfifValues {
    pub x: Option<i64>,
    pub y: Option<i64>,
    pub resolution_unit: Option<i64>,
}
fn refusal(reason: &str) -> String {
    format!("generated mandatory defaults refused: {reason}")
}

#[cfg(test)]
mod tests {
    use super::*;
    const SURVIVORS: &[SurvivorEncoding] = &[
        SurvivorEncoding {
            format_name: "int8u",
            tiff_type: 1,
            width: 1,
            operation: "write_value_scalar",
        },
        SurvivorEncoding {
            format_name: "int8s",
            tiff_type: 6,
            width: 1,
            operation: "write_value_scalar",
        },
        SurvivorEncoding {
            format_name: "int16s",
            tiff_type: 8,
            width: 2,
            operation: "write_value_scalar",
        },
        SurvivorEncoding {
            format_name: "int32s",
            tiff_type: 9,
            width: 4,
            operation: "write_value_scalar",
        },
        SurvivorEncoding {
            format_name: "int16u",
            tiff_type: 3,
            width: 2,
            operation: "write_value_scalar",
        },
        SurvivorEncoding {
            format_name: "int32u",
            tiff_type: 4,
            width: 4,
            operation: "write_value_scalar",
        },
        SurvivorEncoding {
            format_name: "rational64u",
            tiff_type: 5,
            width: 8,
            operation: "write_value_scalar",
        },
    ];
    fn test_recipe() -> MandatoryRecipe {
        MandatoryRecipe {
            writer_source_file: "",
            writer_source_sha256: "",
            core_source_sha256: "",
            exif_source_sha256: "",
            write_value_source_sha256: "",
            perl_version: "",
            no_mandatory_guard: false,
            directories: &[],
            jfif_directory: "",
            jfif_probe: "",
            jfif_assignments: &[],
            encodings: &[],
            survivor_encodings: SURVIVORS,
            cleanup: MandatoryCleanup {
                all_mandatory: true,
                no_next_ifd: true,
                entry_count_shrinks_or_new: true,
                omit_empty_ifd1: true,
            },
        }
    }

    #[test]
    fn surviving_mandatory_integer_uses_the_physical_long_format() {
        let default = MandatoryDefault {
            tag_id: 77,
            value: MandatoryValue::Integer(6),
        };
        assert!(
            matches_existing_mandatory_value(
                &test_recipe(),
                default,
                4,
                1,
                &6u32.to_le_bytes(),
                TiffByteOrder::Little,
            )
            .unwrap()
        );
        assert!(
            matches_existing_mandatory_value(
                &test_recipe(),
                default,
                4,
                1,
                &6u32.to_be_bytes(),
                TiffByteOrder::Big,
            )
            .unwrap()
        );
        assert!(
            !matches_existing_mandatory_value(
                &test_recipe(),
                default,
                4,
                1,
                &7u32.to_be_bytes(),
                TiffByteOrder::Big,
            )
            .unwrap()
        );
        // WriteValue splits the scalar input and cannot pack it twice, so it
        // is not mandatory and the containing IFD remains.
        assert!(
            matches_existing_mandatory_value(
                &test_recipe(),
                default,
                4,
                2,
                &[0; 8],
                TiffByteOrder::Little,
            )
            .is_ok_and(|matches| !matches)
        );
        // Pinned WriteValue helper capability artifact permits only the
        // capture-validated int16u/int32u/rational64u physical forms.
        for (field_type, bytes) in [
            (3, 6u16.to_le_bytes().to_vec()),
            (4, 6u32.to_le_bytes().to_vec()),
            (5, [6u32.to_le_bytes(), 1u32.to_le_bytes()].concat()),
        ] {
            assert!(
                matches_existing_mandatory_value(
                    &test_recipe(),
                    default,
                    field_type,
                    1,
                    &bytes,
                    TiffByteOrder::Little,
                )
                .unwrap(),
                "type {field_type} count one"
            );
        }
        // Positive multi-value numeric input is native undef, so it is a
        // non-match and cannot turn a successful deletion into an error.
        assert!(
            !matches_existing_mandatory_value(
                &test_recipe(),
                default,
                4,
                2,
                &[0; 8],
                TiffByteOrder::Little,
            )
            .unwrap()
        );
        // An unlisted physical form is not silently interpreted by an old
        // conversion implementation.
        assert!(
            !matches_existing_mandatory_value(
                &test_recipe(),
                default,
                11,
                1,
                &6f32.to_le_bytes(),
                TiffByteOrder::Little,
            )
            .unwrap()
        );
    }
}
/// Apply the captured new-directory branch. A caller must provide the already
/// observed JFIF fields; this runtime does not discover tags or defaults.
pub(crate) fn defaults_for_new_directory(
    recipe: &MandatoryRecipe,
    directory: &str,
    no_mandatory: bool,
    num_entries: u32,
    jfif: Option<JfifValues>,
) -> Result<Vec<MandatoryDefault>, String> {
    let mut properties = std::collections::BTreeMap::new();
    if let Some(jfif) = jfif {
        for (key, value) in [
            ("JFIFXResolution", jfif.x),
            ("JFIFYResolution", jfif.y),
            ("JFIFResolutionUnit", jfif.resolution_unit),
        ] {
            if let Some(value) = value {
                properties.insert(key.to_owned(), value);
            }
        }
    }
    defaults_with_properties(recipe, directory, no_mandatory, num_entries, &properties)
}

/// Execute generated property assignments against raw native self-state.
/// Property names, the presence gate, target IDs and arithmetic are operands.
pub(crate) fn defaults_with_properties(
    recipe: &MandatoryRecipe,
    directory: &str,
    no_mandatory: bool,
    num_entries: u32,
    properties: &std::collections::BTreeMap<String, i64>,
) -> Result<Vec<MandatoryDefault>, String> {
    if recipe.writer_source_file != "Image/ExifTool/WriteExif.pl"
        || recipe.writer_source_sha256.len() != 64
    {
        return Err(refusal("writer provenance is unresolved"));
    }
    if num_entries != 0 || (recipe.no_mandatory_guard && no_mandatory) {
        return Ok(Vec::new());
    }
    let source = recipe
        .directories
        .iter()
        .find(|item| item.directory == directory)
        .ok_or_else(|| refusal("directory has no generated mandatory defaults"))?;
    let mut values = source.defaults.to_vec();
    if directory == recipe.jfif_directory && properties.contains_key(recipe.jfif_probe) {
        for assignment in recipe.jfif_assignments {
            let raw = properties
                .get(assignment.property)
                .ok_or_else(|| refusal("generated mandatory source property is undefined"))?;
            let value = raw
                .checked_add(assignment.adjustment)
                .ok_or_else(|| refusal("generated mandatory source adjustment overflows"))?;
            if let Some(target) = values
                .iter_mut()
                .find(|item| item.tag_id == assignment.tag_id)
            {
                target.value = MandatoryValue::Integer(value);
            } else {
                values.push(MandatoryDefault {
                    tag_id: assignment.tag_id,
                    value: MandatoryValue::Integer(value),
                });
            }
        }
    }
    Ok(values)
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    use crate::writers::generated_mandatory_defaults::MANDATORY_DEFAULTS;
    use std::collections::BTreeMap;

    fn fields(directory: &str, order: TiffByteOrder) -> Vec<(u16, u16, u32, Vec<u8>)> {
        let mut out: Vec<_> =
            encode_creation_defaults(&MANDATORY_DEFAULTS, directory, order, &BTreeMap::new())
                .unwrap()
                .into_iter()
                .map(|e| (e.tag_id, e.tiff_type, e.count, e.bytes))
                .collect();
        out.sort();
        out
    }

    /// The entries pinned ExifTool 13.59 writes into a directory it creates
    /// (WriteExif.pl 13.59:25-50), as its own output holds them.
    #[test]
    fn created_directory_entries_are_writeexifs() {
        for (order, ffff, one) in [
            (TiffByteOrder::Little, vec![0xff, 0xff], vec![1, 0]),
            (TiffByteOrder::Big, vec![0xff, 0xff], vec![0, 1]),
        ] {
            assert_eq!(
                fields("ExifIFD", order),
                vec![
                    (0x9000, 7, 4, b"0232".to_vec()),
                    (0x9101, 7, 4, vec![1, 2, 3, 0]),
                    (0xa001, 3, 1, ffff),
                ]
            );
            assert_eq!(fields("GPS", order), vec![(0x0000, 1, 4, vec![2, 3, 0, 0])]);
            assert_eq!(
                fields("InteropIFD", order),
                vec![(0x0002, 7, 4, b"0100".to_vec())]
            );
            assert_eq!(fields("IFD0", order), vec![(0x0213, 3, 1, one)]);
        }
    }

    /// JFIF resolution replaces a created IFD0's defaults as WriteExif's
    /// `$$et{JFIFYResolution}` branch does (WriteExif.pl 13.59:705-711).
    #[test]
    fn a_created_ifd0_takes_the_jfif_resolution() {
        let properties = BTreeMap::from([
            ("JFIFXResolution".to_owned(), 300),
            ("JFIFYResolution".to_owned(), 300),
            ("JFIFResolutionUnit".to_owned(), 1),
        ]);
        let mut got: Vec<_> =
            encode_creation_defaults(&MANDATORY_DEFAULTS, "IFD0", TiffByteOrder::Big, &properties)
                .unwrap()
                .into_iter()
                .map(|e| (e.tag_id, e.tiff_type, e.bytes))
                .collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                (0x011a, 5, vec![0, 0, 1, 0x2c, 0, 0, 0, 1]),
                (0x011b, 5, vec![0, 0, 1, 0x2c, 0, 0, 0, 1]),
                (0x0128, 3, vec![0, 2]),
                (0x0213, 3, vec![0, 1]),
            ]
        );
    }

    /// A default with neither a recipe encoding nor a creation operand is
    /// refused rather than packed by guess.
    #[test]
    fn a_default_without_an_operand_is_refused() {
        const UNKNOWN: &[MandatoryDefault] = &[MandatoryDefault {
            tag_id: 0x1234,
            value: MandatoryValue::Text("1"),
        }];
        const DIRECTORIES: &[MandatoryDirectory] = &[MandatoryDirectory {
            directory: "ExifIFD",
            defaults: UNKNOWN,
        }];
        let recipe = MandatoryRecipe {
            directories: DIRECTORIES,
            ..MANDATORY_DEFAULTS
        };
        assert!(
            encode_creation_defaults(&recipe, "ExifIFD", TiffByteOrder::Big, &BTreeMap::new())
                .is_err()
        );
    }
}
