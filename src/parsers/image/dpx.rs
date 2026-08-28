//! Digital Picture Exchange (DPX) metadata parser.
//!
//! ExifTool 13.59's `Image::ExifTool::DPX::ProcessDPX` reads exactly the
//! 2,080-byte DPX header, accepts only `SDPX`/`XPDS`, then applies
//! `DPX::Main`.  This parser follows that bounded layout and uses the
//! generated table rather than duplicating field offsets or enum maps.
//!
//! `DPX::Main`'s `GROUPS` are `{ 0 => 'File', 1 => 'File', 2 => 'Image' }`,
//! so every tag is emitted under the `File` group, matching
//! `exiftool -G1 -s`.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
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
}

/// Mirrors `DPX.pm` CreateDate's `ValueConv`:
/// `$val =~ s/(\d{4}:\d{2}:\d{2}):/$1 /; $val` -- replace the colon after
/// the leading `YYYY:MM:DD` with a space, leaving anything else untouched.
/// (Perl `s///` without `/g` rewrites only the first match; with a
/// `\d{4}:\d{2}:\d{2}` anchor pattern that is the date prefix or nothing.)
fn dpx_create_date(value: &str) -> String {
    let bytes = value.as_bytes();
    for start in 0..bytes.len().saturating_sub(10) {
        let window = &bytes[start..start + 11];
        let is_date = window.iter().enumerate().all(|(i, &b)| match i {
            4 | 7 | 10 => b == b':',
            _ => b.is_ascii_digit(),
        });
        if is_date {
            let mut out = value.to_string();
            out.replace_range(start + 10..start + 11, " ");
            return out;
        }
    }
    value.to_string()
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

        // CreateDate is the one `DPX::Main` field whose `ValueConv` the
        // generated schema refuses to model (`omitted.value_conv`);
        // `dpx_create_date` above is the hand-verified equivalent and this
        // citation is RawAccess's required acknowledgment. Its PrintConv is
        // `$self->ConvertDateTime($val)`, identity under default options
        // (the generated `SelfConvertDateTimeVal7455B8` expr).
        const CREATE_DATE_CITATION: PerlCitation = PerlCitation {
            module: "DPX",
            table: "Main",
            tag: "CreateDate",
            lines: "ValueConv 's/(\\d{4}:\\d{2}:\\d{2}):/$1 /', DPX.pm",
        };

        // Every clean field (`Omitted::NONE`) renders through the generated
        // table's own emit path -- enum PrintConvs, the `sprintf("%.8x")`
        // EncryptionKey expr, and plain values alike. The four fields whose
        // `RawConv` the schema refuses (`Image2Description`..
        // `Image8Description`, `AspectRatio`, `ShutterAngle`, `FrameRate`)
        // stay omitted here; see the module tests for the citations.
        for decoded in decode_binary_table(table, header, byte_order).fields() {
            if decoded.field.name == "CreateDate" {
                let value =
                    RawAccess::new(decoded, Acknowledged::VALUE_CONV, &CREATE_DATE_CITATION)
                        .and_then(|access| match access.raw() {
                            DecodedValue::String(value) => Some(value.clone()),
                            _ => None,
                        });
                if let Some(value) = value {
                    metadata.insert(
                        "File:CreateDate".to_string(),
                        TagValue::new_string(dpx_create_date(&value)),
                    );
                }
                continue;
            }
            if let Some(value) = decoded.emit() {
                metadata.insert(format!("File:{}", decoded.field.name), value);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// A minimal big-endian DPX header carrying the fields the
    /// `t/images/DPX.dpx` sample exercises, with the pinned 13.59 oracle's
    /// own output as the expected values (`exiftool -G1 -s DPX.dpx`).
    fn dpx_sample_header() -> Vec<u8> {
        let mut h = vec![0u8; DPX_HEADER_LEN];
        h[0..4].copy_from_slice(b"SDPX");
        h[8..12].copy_from_slice(b"V1.0");
        h[16..20].copy_from_slice(&12812288u32.to_be_bytes());
        h[20..24].copy_from_slice(&1u32.to_be_bytes()); // DittoKey: New
        h[36..50].copy_from_slice(b"Image filename");
        h[136..155].copy_from_slice(b"2010:08:03:08:38:16");
        h[160..167].copy_from_slice(b"Creator");
        h[260..272].copy_from_slice(b"Project name");
        h[460..474].copy_from_slice(b"Copyright info");
        h[660..664].copy_from_slice(&0xffff_ffffu32.to_be_bytes()); // EncryptionKey
        h[768..770].copy_from_slice(&0u16.to_be_bytes()); // Orientation
        h[770..772].copy_from_slice(&1u16.to_be_bytes()); // ImageElements
        h[772..776].copy_from_slice(&2048u32.to_be_bytes());
        h[776..780].copy_from_slice(&1556u32.to_be_bytes());
        h[780..784].copy_from_slice(&0u32.to_be_bytes()); // DataSign
        h[800] = 50; // ComponentsConfiguration: R, G, B
        h[801] = 1; // TransferCharacteristic: Printing density
        h[802] = 1; // ColorimetricSpecification: Printing density
        h[803] = 10; // BitDepth
        h[820..823].copy_from_slice(b"CPD");
        h[1556..1574].copy_from_slice(b"SPIRIT-4K DATACINE");
        h[1588..1593].copy_from_slice(b"01018");
        h[1724..1728].copy_from_slice(&0f32.to_be_bytes()); // OriginalFrameRate
        h[1920..1924].copy_from_slice(&4117u32.to_be_bytes()); // TimeCode
        h[2048..2059].copy_from_slice(b"Thomson BTS");
        h
    }

    #[test]
    fn test_dpx_sample_fields_match_oracle() {
        let reader = TestReader::new(dpx_sample_header());
        let metadata = DPXParser.parse(&reader).unwrap();
        // ByteOrder now comes from the table field's own StrEnum PrintConv.
        assert_eq!(metadata.get_string("File:ByteOrder"), Some("Big-endian"));
        assert_eq!(metadata.get_string("File:HeaderVersion"), Some("V1.0"));
        assert_eq!(
            metadata.get("File:DPXFileSize"),
            Some(&TagValue::Integer(12812288))
        );
        assert_eq!(metadata.get_string("File:DittoKey"), Some("New"));
        // `ValueConv` hand-implementation: the raw header holds
        // `2010:08:03:08:38:16`; ExifTool reports `2010:08:03 08:38:16`.
        assert_eq!(
            metadata.get_string("File:CreateDate"),
            Some("2010:08:03 08:38:16")
        );
        assert_eq!(metadata.get_string("File:Project"), Some("Project name"));
        assert_eq!(
            metadata.get_string("File:Copyright"),
            Some("Copyright info")
        );
        // sprintf("%.8x",$val) via the generated expr.
        assert_eq!(metadata.get_string("File:EncryptionKey"), Some("ffffffff"));
        assert_eq!(
            metadata.get_string("File:InputDeviceName"),
            Some("SPIRIT-4K DATACINE")
        );
        assert_eq!(
            metadata.get_string("File:InputDeviceSerialNumber"),
            Some("01018")
        );
        // Empty strings are still values: ExifTool reports them.
        assert_eq!(metadata.get_string("File:SourceFileName"), Some(""));
        assert_eq!(metadata.get_string("File:SourceCreateDate"), Some(""));
        assert_eq!(metadata.get_string("File:FrameID"), Some(""));
        assert_eq!(metadata.get_string("File:SlateInformation"), Some(""));
        assert_eq!(metadata.get_string("File:UserID"), Some("Thomson BTS"));
        assert_eq!(
            metadata.get("File:TimeCode"),
            Some(&TagValue::Integer(4117))
        );
        assert_eq!(
            metadata.get("File:OriginalFrameRate"),
            Some(&TagValue::Float(0.0))
        );
        // The RawConv-carrying fields stay omitted rather than approximated:
        // Image2..8Description (`RawConv '$val=~/[^\xff]/ ? $val : undef'`),
        // AspectRatio (`RawConv` + Rationalize PrintConv), ShutterAngle and
        // FrameRate (`RawConv '($val =~ /\d/ and $val !~ /nan/i) ? $val :
        // undef'`), all DPX.pm. On this header every one of them is undef in
        // ExifTool too (all-zero/all-0xff regions).
        assert_eq!(metadata.get("File:AspectRatio"), None);
        assert_eq!(metadata.get("File:ShutterAngle"), None);
        assert_eq!(metadata.get("File:FrameRate"), None);
        assert_eq!(metadata.get("File:Image2Description"), None);
    }

    #[test]
    fn test_dpx_create_date_valueconv() {
        assert_eq!(
            dpx_create_date("2010:08:03:08:38:16"),
            "2010:08:03 08:38:16"
        );
        // No date prefix: the substitution does not fire.
        assert_eq!(dpx_create_date(""), "");
        assert_eq!(dpx_create_date("not a date"), "not a date");
        // Only the first match is rewritten, like Perl s/// without /g.
        assert_eq!(
            dpx_create_date("2010:08:03:2011:09:04:x"),
            "2010:08:03 2011:09:04:x"
        );
    }

    #[test]
    fn test_dpx_rejects_short_and_bad_signature() {
        let reader = TestReader::new(vec![0u8; 100]);
        assert!(DPXParser.parse(&reader).is_err());
        let mut bad = dpx_sample_header();
        bad[0..4].copy_from_slice(b"NOPE");
        let reader = TestReader::new(bad);
        assert!(DPXParser.parse(&reader).is_err());
    }
}
