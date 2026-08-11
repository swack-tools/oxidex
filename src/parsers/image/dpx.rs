//! Digital Picture Exchange (DPX) metadata parser.
//!
//! ExifTool 13.59's `Image::ExifTool::DPX::ProcessDPX` reads exactly the
//! 2,080-byte DPX header, accepts only `SDPX`/`XPDS`, then applies
//! `DPX::Main`.  This parser follows that bounded layout and uses the
//! generated table rather than duplicating field offsets or enum maps.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{decode_binary_table, find_table};
use crate::io::ByteOrder;

const DPX_HEADER_LEN: usize = 2080;

/// Parser for DPX files.
pub struct DPXParser;

impl DPXParser {
    fn byte_order(header: &[u8]) -> Option<ByteOrder> {
        match header.get(..4)? {
            b"SDPX" => Some(ByteOrder::Big),
            b"XPDS" => Some(ByteOrder::Little),
            _ => None,
        }
    }

    fn direct_field(name: &str) -> bool {
        matches!(
            name,
            "HeaderVersion"
                | "DPXFileSize"
                | "DittoKey"
                | "ImageFileName"
                | "Creator"
                | "Orientation"
                | "ImageElements"
                | "ImageWidth"
                | "ImageHeight"
                | "DataSign"
                | "ComponentsConfiguration"
                | "TransferCharacteristic"
                | "ColorimetricSpecification"
                | "BitDepth"
                | "ImageDescription"
        )
    }
}

impl FormatParser for DPXParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if reader.size() < DPX_HEADER_LEN as u64 {
            return Err(ExifToolError::parse_error(
                "DPX header is shorter than 2080 bytes",
            ));
        }
        let header = reader.read(0, DPX_HEADER_LEN)?;
        let byte_order = Self::byte_order(header)
            .ok_or_else(|| ExifToolError::parse_error("Invalid DPX signature"))?;
        let table = find_table("DPX", "Main")
            .ok_or_else(|| ExifToolError::parse_error("Missing generated DPX::Main table"))?;

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType", TagValue::String("DPX".to_string()));
        metadata.insert(
            "ByteOrder",
            TagValue::String(
                match byte_order {
                    ByteOrder::Big => "Big-endian",
                    ByteOrder::Little => "Little-endian",
                }
                .to_string(),
            ),
        );

        // Every field this parser reads is `Omitted::NONE` in the generated
        // `DPX::Main` table (verified against `src/exiftool_tables`), so
        // `emit` never refuses here; it either renders the `PrintConv` or
        // falls back to the raw value, matching the two print_conv shapes
        // (`None`, `IntEnum`) this whitelist actually contains.
        for decoded in decode_binary_table(table, header, byte_order).fields() {
            if !Self::direct_field(decoded.field.name) {
                continue;
            }
            let value = match decoded.field.print_conv {
                crate::exiftool_tables::PrintConv::None => match decoded.emit() {
                    Some(
                        value @ (TagValue::Integer(_) | TagValue::Float(_) | TagValue::String(_)),
                    ) => value,
                    _ => continue,
                },
                crate::exiftool_tables::PrintConv::IntEnum(_) => match decoded.emit() {
                    Some(value @ TagValue::String(_)) => value,
                    // A raw fallback here means the enum lookup missed;
                    // ExifTool's own DPX PrintConv hashes have no default, so
                    // the tag is skipped exactly as it was before.
                    _ => continue,
                },
                crate::exiftool_tables::PrintConv::StrEnum(_)
                | crate::exiftool_tables::PrintConv::Expr(_) => {
                    continue;
                }
            };
            metadata.insert(decoded.field.name, value);
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::DPX)
    }
}

/// Parses metadata from a DPX file.
pub fn parse_dpx_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    DPXParser.parse(reader).map_err(|error| error.to_string())
}
