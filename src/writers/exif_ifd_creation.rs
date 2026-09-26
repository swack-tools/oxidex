//! The entries `WriteExif` gives an ExifIFD it creates in a TIFF-structured
//! file (pinned ExifTool 13.59 on t/images ExifTool.tif, which has no
//! ExifIFD: `-ExifIFD:ModifyDate=<d>` leaves `[ExifIFD] ExifVersion 0232`,
//! `ComponentsConfiguration 1 2 3 0` and `ColorSpace 65535` beside the new
//! ModifyDate).
//!
//! `WriteExif` adds a directory's `%mandatory` entries when it creates the
//! directory (`unless ($numEntries)`, WriteExif.pl 13.59:714-719), packing
//! each raw with its row's format (`$newVal = $$mandatory{$newID}`, then
//! `WriteValue`, :1197-1216). The values come from the generated recipe
//! (`generated_mandatory_defaults`); the format of each from its transcribed
//! `Exif::Main` row (`exiftool_tables::ifd_tables::IFD_EXIF_MAIN`: `Format`
//! or `Writable`, `Count`) and the IFD type number from the generated
//! `%formatNumber` registry. Nothing is copied by hand, and a row this
//! packer cannot model is refused rather than guessed.
//!
//! The roll-up (#957, from #955) carries the same packing for every
//! directory as `mandatory_defaults_runtime::encode_creation_defaults`; this
//! is its ExifIFD case for the one TIFF caller here, and gives way to it on
//! merge.

use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::writers::exif_surgical::IfdKind;
use crate::writers::generated_mandatory_defaults::MANDATORY_DEFAULTS;
use crate::writers::mandatory_defaults_runtime::MandatoryValue;
use crate::writers::tiff_surgical::entry_edits::{EntryMutation, ScopedEntryEdit};

fn refused(reason: &str) -> ExifToolError {
    ExifToolError::unsupported_format(format!(
        "Cannot create the ExifIFD this write needs: its mandatory entries cannot be \
         packed ({reason}); nothing was written"
    ))
}

/// The IFD type number `%formatNumber` gives `name`.
fn format_number(name: &str) -> Option<u16> {
    let registry =
        crate::writers::generated_tiff_scalar_final_rules::TIFF_SCALAR_FINAL_FORMAT_REGISTRY?;
    if registry.source_sha256 != MANDATORY_DEFAULTS.exif_source_sha256 {
        return None;
    }
    registry
        .facts
        .iter()
        .map(|fact| (fact.name, fact.number))
        .chain(
            registry
                .aliases
                .iter()
                .map(|alias| (alias.name, alias.number)),
        )
        .find(|(fact, _)| *fact == name)
        .map(|(_, number)| number)
}

/// The mandatory entries of a new ExifIFD, packed in `byte_order`, less the
/// ones whose tag ID is in `set` (the caller's own values win, as
/// `defined $set{$_} or $set{$_} = ...` keeps them, :716-718).
pub(crate) fn created_exif_ifd_entries(
    byte_order: ByteOrder,
    set: &[u16],
) -> Result<Vec<ScopedEntryEdit>> {
    use crate::exiftool_tables::Fmt;
    use crate::exiftool_tables::ifd_tables::IFD_EXIF_MAIN;
    let defaults = MANDATORY_DEFAULTS
        .directories
        .iter()
        .find(|directory| directory.directory == "ExifIFD")
        .ok_or_else(|| refused("the recipe has no ExifIFD defaults"))?
        .defaults;
    let mut edits = Vec::new();
    for default in defaults
        .iter()
        .filter(|default| !set.contains(&default.tag_id))
    {
        if IFD_EXIF_MAIN.variant_group(default.tag_id).is_some() {
            return Err(refused("a mandatory row is a conditional variant"));
        }
        let row = IFD_EXIF_MAIN
            .tag(default.tag_id)
            .ok_or_else(|| refused("a mandatory row is not transcribed"))?;
        if row.condition.is_some() || row.omitted.condition || row.subdir.is_some() {
            return Err(refused("a mandatory row is conditional or a sub-directory"));
        }
        let writable = row
            .writable
            .ok_or_else(|| refused("a mandatory row has no Writable"))?;
        let field_type =
            format_number(writable).ok_or_else(|| refused("no %formatNumber for its Writable"))?;
        let packing = match row.format {
            None => writable,
            Some(Fmt::Int8u) => "int8u",
            Some(Fmt::Int16u) => "int16u",
            Some(_) => return Err(refused("a mandatory row Format is outside the packer")),
        };
        let bytes = match (packing, default.value) {
            ("undef", MandatoryValue::Text(text)) => text.as_bytes().to_vec(),
            ("int8u", MandatoryValue::Text(text)) => text
                .split(' ')
                .map(str::parse::<u8>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| refused("an int8u operand is out of range"))?,
            ("int16u", MandatoryValue::Integer(value)) => {
                let value = u16::try_from(value)
                    .map_err(|_| refused("an int16u operand is out of range"))?;
                match byte_order {
                    ByteOrder::LittleEndian => value.to_le_bytes().to_vec(),
                    ByteOrder::BigEndian => value.to_be_bytes().to_vec(),
                }
            }
            _ => return Err(refused("an operand does not fit its row")),
        };
        let width = if packing == "int16u" { 2 } else { 1 };
        let count =
            u32::try_from(bytes.len() / width).map_err(|_| refused("a count exceeds u32"))?;
        if row.count.is_some_and(|declared| declared != count) {
            return Err(refused("a default does not fill its declared count"));
        }
        edits.push(ScopedEntryEdit {
            ifd: IfdKind::ExifIfd,
            tag_id: default.tag_id,
            mutation: EntryMutation::Set {
                field_type,
                count,
                bytes,
            },
        });
    }
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(byte_order: ByteOrder, set: &[u16]) -> Vec<(u16, u16, u32, Vec<u8>)> {
        created_exif_ifd_entries(byte_order, set)
            .unwrap()
            .into_iter()
            .map(|edit| match edit.mutation {
                EntryMutation::Set {
                    field_type,
                    count,
                    bytes,
                } => (edit.tag_id, field_type, count, bytes),
                EntryMutation::Delete => panic!("a creation entry is a set"),
            })
            .collect()
    }

    /// What pinned ExifTool 13.59 writes into the ExifIFD it creates in
    /// t/images ExifTool.tif (`-v3`: ExifVersion undef[4] "0232",
    /// ComponentsConfiguration undef[4] 01 02 03 00, ColorSpace int16u 0xffff).
    #[test]
    fn a_created_exif_ifd_gets_writeexifs_mandatory_entries() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            assert_eq!(
                fields(order, &[]),
                vec![
                    (0x9000, 7, 4, b"0232".to_vec()),
                    (0x9101, 7, 4, vec![1, 2, 3, 0]),
                    (0xa001, 3, 1, vec![0xff, 0xff]),
                ]
            );
        }
        // A value the caller sets itself is not replaced.
        assert_eq!(
            fields(ByteOrder::BigEndian, &[0xa001])
                .iter()
                .map(|field| field.0)
                .collect::<Vec<_>>(),
            [0x9000, 0x9101]
        );
    }
}
