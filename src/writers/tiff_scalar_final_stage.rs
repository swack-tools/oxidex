//! Closed final scalar stage for ordinary `Exif::Main` TIFF entries.
//!
//! This is deliberately a resolver, not a public writer.  Its recipe is
//! emitted only by `final_scalar_stage.py` from the final-loaded native table
//! sidecar and it returns a resolved IFD edit for the existing surgical TIFF
//! primitive to consume.  It never looks up a tag name or chooses a TIFF type
//! itself.

use crate::error::{ExifToolError, Result};
use crate::writers::generated_scalar::{Scalar, ScalarWriteRecipe, serialize_scalar};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TiffByteOrder {
    LittleEndian,
    BigEndian,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScalarWriteOperation {
    Create,
    Update,
}

/// A TIFF format fact captured from ExifTool's final-loaded
/// `@formatName`, `@formatSize`, and `%formatNumber` registries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeTiffFormatFact {
    pub name: &'static str,
    pub number: u16,
    pub size: u32,
}

/// `%formatNumber` can deliberately contain aliases (`binary` and `undef`
/// share wire type 7).  Keep those source facts instead of treating the alias
/// as an ambiguous canonical registry entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeTiffFormatAliasFact {
    pub name: &'static str,
    pub number: u16,
}

/// No Rust format table belongs here.  The generated caller supplies all of
/// these facts after the native registry capture has validated its aliases and
/// sparse slots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeTiffFormatRegistry {
    pub source_file: &'static str,
    pub source_sha256: &'static str,
    /// Exactly one fact per defined `@formatName` slot.
    pub facts: &'static [NativeTiffFormatFact],
    /// Every final-loaded `%formatNumber` key, including aliases.
    pub aliases: &'static [NativeTiffFormatAliasFact],
}

impl NativeTiffFormatRegistry {
    pub(crate) fn resolve(&self, name: &str) -> Result<NativeTiffFormatFact> {
        if self.source_file != "Image/ExifTool/Exif.pm" || self.source_sha256.len() != 64 {
            return Err(refused(
                "native TIFF format registry provenance is unresolved",
            ));
        }
        let fact = self
            .facts
            .iter()
            .copied()
            .find(|fact| fact.name == name)
            .ok_or_else(|| refused("selected format is absent from native TIFF registry"))?;
        if fact.number == 0 || fact.size == 0 {
            return Err(refused("selected native TIFF format has zero type or size"));
        }
        Ok(fact)
    }
}

/// Source-derived operands for the restricted ordinary-EXIF scalar branch.
/// `conversion_format` and `wire_format` are names from the captured source;
/// their numeric TIFF representation is resolved only through the registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TiffScalarFinalStageRecipe {
    pub module: &'static str,
    pub table: &'static str,
    pub full_name: &'static str,
    pub raw_tag_id: u16,
    pub tag_name: &'static str,
    pub table_group0: &'static str,
    pub physical_write_group: &'static str,
    pub conversion_format: &'static str,
    pub wire_format: &'static str,
    /// Kept optional in the generated artifact so an unresolved helper cannot
    /// be papered over by a Rust fallback.
    pub write_value: Option<&'static ScalarWriteRecipe>,
    /// Translation of the source's final `$newCount = length(...) / size`
    /// statement.  This is not inferred from a registry size.
    pub count_rule: NativeCountRule,
    pub source_control_sha256: &'static str,
    pub write_proc_source_sha256: &'static str,
    pub registry_source_sha256: &'static str,
    pub writer_source_sha256: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeCountRule {
    /// The later native `int(($newSize + $fsize - 1) / $fsize)` assignment
    /// replaces the earlier provisional count for rows without FixedSize.
    CeilDivision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResolvedTiffScalarEdit {
    /// Native does not create an entry for an undefined requested value.
    NoEdit,
    /// Native deletion is only meaningful once the row already exists.
    Delete { raw_tag_id: u16 },
    /// Native keeps an existing entry after the `NoOverwrite` fall-through.
    NoOverwrite { native_warning: String },
    /// The caller still owns placement and offsets; this contains the exact
    /// final type/count and an already byte-order encoded IFD entry.
    Write {
        raw_tag_id: u16,
        wire_format: u16,
        count: u32,
        entry: [u8; 12],
        /// Bytes for the value buffer when the IFD field is an offset.
        out_of_line_value: Option<Vec<u8>>,
    },
}

fn refused(reason: &str) -> ExifToolError {
    ExifToolError::unsupported_format(format!(
        "generated TIFF scalar final stage refused: {reason}"
    ))
}

fn put_u16(order: TiffByteOrder, value: u16, dst: &mut [u8]) {
    let bytes = match order {
        TiffByteOrder::LittleEndian => value.to_le_bytes(),
        TiffByteOrder::BigEndian => value.to_be_bytes(),
    };
    dst.copy_from_slice(&bytes);
}

fn put_u32(order: TiffByteOrder, value: u32, dst: &mut [u8]) {
    let bytes = match order {
        TiffByteOrder::LittleEndian => value.to_le_bytes(),
        TiffByteOrder::BigEndian => value.to_be_bytes(),
    };
    dst.copy_from_slice(&bytes);
}

fn final_bytes(value: Scalar) -> Result<Vec<u8>> {
    match value {
        Scalar::Undefined => Ok(Vec::new()),
        Scalar::Bytes(bytes) => Ok(bytes),
        // The declared boundary is after Sanitize/ConvInv. Default Sanitize
        // has already turned Unicode into UTF-8 *bytes* here. Treating an
        // upgraded Perl scalar as a byte string would be an untested Charset
        // rule, so leave it visible instead of encoding it in Rust.
        Scalar::Utf8(_) => Err(refused("final stage requires post-Sanitize bytes")),
    }
}

/// Resolve the source-selected scalar format before invoking generated
/// `WriteValue`, then apply the native default CharsetEXIF-disabled byte/count
/// rules.  Explicit CharsetEXIF, format overrides, callbacks, MakerNotes,
/// EntryBased directories and physical mutation remain compiler refusals.
pub(crate) fn resolve_tiff_scalar_final_stage(
    recipe: &TiffScalarFinalStageRecipe,
    registry: &NativeTiffFormatRegistry,
    scalar: Scalar,
    operation: ScalarWriteOperation,
    byte_order: TiffByteOrder,
) -> Result<ResolvedTiffScalarEdit> {
    if recipe.table_group0 != "EXIF" || recipe.conversion_format != recipe.wire_format {
        return Err(refused(
            "recipe is outside default ordinary EXIF scalar format selection",
        ));
    }
    if recipe.source_control_sha256.len() != 64 {
        return Err(refused("final-stage source control sequence is unresolved"));
    }
    if matches!(scalar, Scalar::Undefined) {
        return Ok(match operation {
            ScalarWriteOperation::Create => ResolvedTiffScalarEdit::NoEdit,
            ScalarWriteOperation::Update => ResolvedTiffScalarEdit::Delete {
                raw_tag_id: recipe.raw_tag_id,
            },
        });
    }

    let format = registry.resolve(recipe.conversion_format)?;
    // This call intentionally receives the scalar boundary value.  Feeding it
    // pre-serialized bytes would move the source's terminator/count behavior.
    let write_value = recipe
        .write_value
        .ok_or_else(|| refused("source-derived WriteValue helper is unresolved"))?;
    let serialized = serialize_scalar(write_value, scalar, recipe.conversion_format, None)?;
    let bytes = final_bytes(serialized.value)?;
    if bytes.is_empty() {
        return Ok(ResolvedTiffScalarEdit::NoOverwrite {
            native_warning: format!(
                "Can't write zero length {} in {}",
                recipe.tag_name, recipe.physical_write_group
            ),
        });
    }
    let byte_len =
        u64::try_from(bytes.len()).map_err(|_| refused("serialized byte length overflows u64"))?;
    let count = match recipe.count_rule {
        NativeCountRule::CeilDivision => {
            byte_len
                .checked_add(u64::from(format.size - 1))
                .ok_or_else(|| refused("native TIFF count addition overflow"))?
                / u64::from(format.size)
        }
    };
    let count = u32::try_from(count).map_err(|_| refused("native TIFF count exceeds u32"))?;

    let mut entry = [0u8; 12];
    put_u16(byte_order, recipe.raw_tag_id, &mut entry[0..2]);
    put_u16(byte_order, format.number, &mut entry[2..4]);
    put_u32(byte_order, count, &mut entry[4..8]);
    let out_of_line_value = if bytes.len() <= 4 {
        entry[8..8 + bytes.len()].copy_from_slice(&bytes);
        None
    } else {
        // This is semantic encoded data, without EXIF even-byte storage
        // padding. `apply_entry_edits` owns its own placement/alignment and
        // requires exactly `count * ExifType::width()` bytes for Set.
        Some(bytes)
    };
    Ok(ResolvedTiffScalarEdit::Write {
        raw_tag_id: recipe.raw_tag_id,
        wire_format: format.number,
        count,
        entry,
        out_of_line_value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writers::generated_scalar::Comparison;

    const WRITE: ScalarWriteRecipe = ScalarWriteRecipe {
        formats: ["string", "undef"],
        terminated_format: "string",
        count_positive_operator: Comparison::Gt,
        count_positive_bound: 0,
        diff_negative_operator: Comparison::Lt,
        terminator: 0,
    };
    const FACTS: &[NativeTiffFormatFact] = &[
        NativeTiffFormatFact {
            name: "string",
            number: 2,
            size: 1,
        },
        NativeTiffFormatFact {
            name: "undef",
            number: 7,
            size: 1,
        },
    ];
    const REGISTRY: NativeTiffFormatRegistry = NativeTiffFormatRegistry {
        source_file: "Image/ExifTool/Exif.pm",
        source_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        facts: FACTS,
        aliases: &[],
    };
    const RECIPE: TiffScalarFinalStageRecipe = TiffScalarFinalStageRecipe {
        module: "Exif",
        table: "Main",
        full_name: "Image::ExifTool::Exif::Main",
        raw_tag_id: 0x013c,
        tag_name: "HostComputer",
        table_group0: "EXIF",
        physical_write_group: "IFD0",
        conversion_format: "string",
        wire_format: "string",
        write_value: Some(&WRITE),
        count_rule: NativeCountRule::CeilDivision,
        source_control_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        write_proc_source_sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        registry_source_sha256: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        writer_source_sha256: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
    };

    #[test]
    fn unicode_bytes_nul_and_empty_follow_writevalue_before_final_count() {
        let unicode = resolve_tiff_scalar_final_stage(
            &RECIPE,
            &REGISTRY,
            Scalar::Bytes("é".as_bytes().to_vec()),
            ScalarWriteOperation::Create,
            TiffByteOrder::LittleEndian,
        )
        .unwrap();
        match unicode {
            ResolvedTiffScalarEdit::Write { count, entry, .. } => {
                assert_eq!(count, 3);
                assert_eq!(&entry[8..], &[0xc3, 0xa9, 0, 0]);
            }
            _ => panic!("expected write"),
        }
        let nul = resolve_tiff_scalar_final_stage(
            &RECIPE,
            &REGISTRY,
            Scalar::Bytes(b"a\0b".to_vec()),
            ScalarWriteOperation::Create,
            TiffByteOrder::BigEndian,
        )
        .unwrap();
        match nul {
            ResolvedTiffScalarEdit::Write { count, entry, .. } => {
                assert_eq!(count, 4);
                assert_eq!(&entry[8..], b"a\0b\0");
            }
            _ => panic!("expected write"),
        }
        let empty = resolve_tiff_scalar_final_stage(
            &RECIPE,
            &REGISTRY,
            Scalar::Bytes(vec![]),
            ScalarWriteOperation::Create,
            TiffByteOrder::LittleEndian,
        )
        .unwrap();
        match empty {
            ResolvedTiffScalarEdit::Write { count, entry, .. } => {
                assert_eq!(count, 1);
                assert_eq!(&entry[8..], &[0, 0, 0, 0]);
            }
            _ => panic!("expected write"),
        }
    }

    #[test]
    fn undefined_deletes_only_existing_rows() {
        assert_eq!(
            resolve_tiff_scalar_final_stage(
                &RECIPE,
                &REGISTRY,
                Scalar::Undefined,
                ScalarWriteOperation::Create,
                TiffByteOrder::LittleEndian
            )
            .unwrap(),
            ResolvedTiffScalarEdit::NoEdit
        );
        assert_eq!(
            resolve_tiff_scalar_final_stage(
                &RECIPE,
                &REGISTRY,
                Scalar::Undefined,
                ScalarWriteOperation::Update,
                TiffByteOrder::LittleEndian
            )
            .unwrap(),
            ResolvedTiffScalarEdit::Delete { raw_tag_id: 0x013c }
        );
    }

    #[test]
    fn zero_serialized_length_is_native_nooverwrite_not_deletion() {
        let undef_recipe = TiffScalarFinalStageRecipe {
            conversion_format: "undef",
            wire_format: "undef",
            ..RECIPE
        };
        assert_eq!(
            resolve_tiff_scalar_final_stage(
                &undef_recipe,
                &REGISTRY,
                Scalar::Bytes(vec![]),
                ScalarWriteOperation::Update,
                TiffByteOrder::LittleEndian,
            )
            .unwrap(),
            ResolvedTiffScalarEdit::NoOverwrite {
                native_warning: "Can't write zero length HostComputer in IFD0".to_owned(),
            }
        );
    }

    #[test]
    fn numeric_type_is_registry_not_tag_data() {
        let changed = NativeTiffFormatRegistry {
            facts: &[NativeTiffFormatFact {
                name: "string",
                number: 129,
                size: 1,
            }],
            ..REGISTRY
        };
        match resolve_tiff_scalar_final_stage(
            &RECIPE,
            &changed,
            Scalar::Bytes(b"x".to_vec()),
            ScalarWriteOperation::Create,
            TiffByteOrder::LittleEndian,
        )
        .unwrap()
        {
            ResolvedTiffScalarEdit::Write {
                wire_format, entry, ..
            } => {
                assert_eq!(wire_format, 129);
                assert_eq!(&entry[2..4], &129u16.to_le_bytes());
            }
            _ => panic!("expected write"),
        }
    }

    #[test]
    fn count_uses_later_native_non_fixed_size_ceil_division() {
        let changed = NativeTiffFormatRegistry {
            facts: &[NativeTiffFormatFact {
                name: "string",
                number: 2,
                size: 2,
            }],
            ..REGISTRY
        };
        match resolve_tiff_scalar_final_stage(
            &RECIPE,
            &changed,
            Scalar::Bytes("é".as_bytes().to_vec()),
            ScalarWriteOperation::Create,
            TiffByteOrder::LittleEndian,
        )
        .unwrap()
        {
            ResolvedTiffScalarEdit::Write { count, .. } => assert_eq!(count, 2),
            _ => panic!("expected write"),
        }
    }

    #[test]
    fn out_of_line_value_excludes_storage_alignment_padding() {
        match resolve_tiff_scalar_final_stage(
            &RECIPE,
            &REGISTRY,
            Scalar::Bytes(b"abcd".to_vec()),
            ScalarWriteOperation::Create,
            TiffByteOrder::LittleEndian,
        )
        .unwrap()
        {
            ResolvedTiffScalarEdit::Write {
                count,
                out_of_line_value,
                ..
            } => {
                assert_eq!(count, 5);
                assert_eq!(out_of_line_value, Some(b"abcd\0".to_vec()));
            }
            _ => panic!("expected out-of-line write"),
        }
    }
}
