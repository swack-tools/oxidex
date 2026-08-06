//! Digital Picture Exchange (DPX) metadata parser.
//!
//! ExifTool 13.59's `Image::ExifTool::DPX::ProcessDPX` reads exactly the
//! 2,080-byte DPX header, accepts only `SDPX`/`XPDS`, then applies
//! `DPX::Main`.  This parser follows that bounded layout and uses the
//! generated table rather than duplicating field offsets or enum maps.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{DecodedValue, decode_binary_table, find_table};
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
        metadata.insert("MIMEType", TagValue::String("image/x-dpx".to_string()));
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

        for decoded in decode_binary_table(table, header, byte_order) {
            if !Self::direct_field(decoded.field.name) {
                continue;
            }
            let value = match decoded.field.print_conv {
                crate::exiftool_tables::PrintConv::None => match decoded.raw {
                    DecodedValue::Integer(value) => TagValue::Integer(value),
                    DecodedValue::Float(value) => TagValue::Float(value),
                    DecodedValue::String(value) => TagValue::String(value),
                    _ => continue,
                },
                crate::exiftool_tables::PrintConv::IntEnum(_) => {
                    let Some(value) = decoded.apply_print_conv_to_raw() else {
                        continue;
                    };
                    TagValue::String(value)
                }
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
