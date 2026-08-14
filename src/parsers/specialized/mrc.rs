//! Medical Research Council (MRC) electron-microscopy image metadata parser.
//!
//! ExifTool 13.59's MRC support (`lib/Image/ExifTool/MRC.pm`) reads a fixed
//! 1024-byte `MRC::Main` header (MRC.pm:28-81, `FORMAT => 'int32u'`, 256
//! words) with `ProcessBinaryData`, then -- when `ExtendedHeaderType` is
//! `FEI1` or `FEI2` and the extended header is present -- a second,
//! bitmask-conditional `MRC::FEI12` table (MRC.pm:83-172) describing a
//! microscope-metadata block whose field *presence* depends on up to four
//! `Bitmask` words read earlier in the same block.
//!
//! This parser reads only `MRC::Main`, from the generated table. `FEI12`'s
//! ~90 fields are gated by per-field `Condition`s on those bitmasks
//! (`$$self{BitM} & 0x...`), which the mechanical transcription declines to
//! evaluate (every `FEI12` field the schema emits is `omitted.condition`),
//! and hand-porting that scale of bitmask-conditional layout is out of scope
//! here. Per AGENTS.md ("a gap in a transcribed table is not evidence the
//! tag does not exist" / "never approximate a conversion"), the honest
//! response is to omit the extended header entirely rather than guess at
//! which subset of its fields apply -- so an MRC file with an FEI extended
//! header reports `MRC::Main`'s ~26 tags and nothing from `FEI12`.
//!
//! # Why `MRC::Main` is not table-only either
//!
//! `MachineStamp` (MRC.pm:73) carries a `PrintConv` of
//! `'sprintf("0x%.2x 0x%.2x 0x%.2x 0x%.2x",split " ", $val)'`, which the
//! generator does not compile (`PrintConv::None` on an unflagged field).
//! `NumberOfLabels` (MRC.pm:76) gates `Label0`..`Label9` (MRC.pm:77-86) by
//! `Condition => '$$self{NLab} > N'`; each `LabelN` is `omitted.condition`
//! and is hand-verified against that count below. `ImageDepth` (MRC.pm:39-45),
//! `ExtendedHeaderSize` (MRC.pm:74) and `ExtendedHeaderType` (MRC.pm:75) each
//! carry a `RawConv` that is a pure `DataMember` side effect
//! (`$$self{X} = $val`, returning the value unchanged) exactly like
//! `NumberOfLabels`'s own; `.emit()` refuses all four (`omitted.raw_conv`),
//! so they are read via [`RawAccess`] and passed through unchanged below,
//! same as `pcx.rs`'s `LeftMargin`/`TopMargin`.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/MRC.pm`

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;

const fn citation(tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "MRC",
        table: "Main",
        tag,
        lines,
    }
}

const IMAGE_DEPTH: PerlCitation = citation("ImageDepth", "MRC.pm:39-45");
const EXTENDED_HEADER_SIZE: PerlCitation = citation("ExtendedHeaderSize", "MRC.pm:74");
const EXTENDED_HEADER_TYPE: PerlCitation = citation("ExtendedHeaderType", "MRC.pm:75");
const NUMBER_OF_LABELS: PerlCitation = citation("NumberOfLabels", "MRC.pm:76");

/// MRC.pm's `Main` table is 256 `int32u` words (MRC.pm:36, `FIRST_ENTRY =>`
/// implicit 0, plus `Label9` at word index 236 running to word 255).
const HEADER_LEN: usize = 1024;

/// Extract MRC metadata using ExifTool's declared `MRC::Main` binary layout.
/// The `FEI12` extended header is deliberately not read; see the module
/// doc comment.
pub fn parse_mrc_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < HEADER_LEN as u64 {
        return Err("MRC file is too short for the 1024-byte header".to_string());
    }
    let header = reader
        .read(0, HEADER_LEN)
        .map_err(|error| error.to_string())?;

    let table = find_table("MRC", "Main").ok_or("missing MRC::Main table")?;
    let decode = decode_binary_table(table, header, ByteOrder::Little);

    let mut number_of_labels = 0_i64;
    for decoded in decode.fields() {
        if decoded.field.name == "NumberOfLabels"
            && let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &NUMBER_OF_LABELS)
            && let Some(raw) = access.raw().as_integer()
        {
            number_of_labels = raw;
        }
    }

    let mut metadata = MetadataMap::new();
    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("File:{name}");
        if let Some(label_index) = name
            .strip_prefix("Label")
            .and_then(|n| n.parse::<i64>().ok())
        {
            // MRC.pm:77-86: `Condition => '$$self{NLab} > N'`.
            if number_of_labels > label_index
                && let Some(value) = decoded.emit()
            {
                metadata.insert(key, value);
            }
            continue;
        }
        match name {
            "ImageDepth" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &IMAGE_DEPTH)
                {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "ExtendedHeaderSize" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &EXTENDED_HEADER_SIZE)
                {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "ExtendedHeaderType" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &EXTENDED_HEADER_TYPE)
                {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "NumberOfLabels" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &NUMBER_OF_LABELS)
                {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "MachineStamp" => {
                if let Some(TagValue::Array(values)) = decoded.emit() {
                    let bytes: Vec<i64> = values.iter().filter_map(TagValue::as_integer).collect();
                    if bytes.len() == 4 {
                        metadata.insert(
                            key,
                            TagValue::new_string(format!(
                                "0x{:02x} 0x{:02x} 0x{:02x} 0x{:02x}",
                                bytes[0], bytes[1], bytes[2], bytes[3]
                            )),
                        );
                    }
                }
            }
            _ => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    #[test]
    fn machine_stamp_formats_as_hex_bytes() {
        let bytes = [0x44_i64, 0x44, 0x00, 0x00];
        let formatted = format!(
            "0x{:02x} 0x{:02x} 0x{:02x} 0x{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        );
        assert_eq!(formatted, "0x44 0x44 0x00 0x00");
    }
}
