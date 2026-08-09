//! FITS (Flexible Image Transport System) parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::{ByteOrder, EndianReader};

mod tables;

use tables::FITS_TAG_NAMES;

const FITS_SIGNATURE: &[u8] = b"SIMPLE";
const FITS_RECORD_SIZE: usize = 80;
const FITS_BLOCK_SIZE: usize = 2880;

/// Parser for FITS (Flexible Image Transport System) files
///
/// Extracts metadata from FITS astronomical data files used for scientific imaging.
pub struct FITSParser;

impl FITSParser {
    /// Verifies the FITS file signature ("SIMPLE")
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 6 {
            return Ok(false);
        }
        let header = reader.read(0, 6)?;
        Ok(header == FITS_SIGNATURE)
    }

    /// Parses a FITS header record (80-character fixed-width)
    /// Returns (keyword, value, comment) tuple
    fn parse_record(record: &[u8]) -> Option<(String, String, Option<String>)> {
        if record.len() != FITS_RECORD_SIZE {
            return None;
        }

        let keyword = String::from_utf8_lossy(&record[..8]).trim_end().to_string();

        // Check for END keyword
        if keyword == "END" {
            return Some(("END".to_string(), String::new(), None));
        }

        // Check for HISTORY or COMMENT records (no '=')
        if keyword == "HISTORY" || keyword == "COMMENT" {
            let content = String::from_utf8_lossy(&record[8..]).trim().to_string();
            return Some((keyword, content, None));
        }

        // Like ExifTool's ProcessFITS, accept a value only when the equals sign
        // occupies the standard columns. A slash begins a comment only outside
        // quotes: dates and identifiers routinely contain literal slashes.
        if &record[8..10] != b"= " {
            return None;
        }
        let value_part = String::from_utf8_lossy(&record[10..]);
        let value_part = value_part.as_ref();
        if let Some(quoted) = value_part.strip_prefix('\'') {
            let mut value = String::new();
            let mut chars = quoted.chars().peekable();
            let mut closed = false;
            while let Some(ch) = chars.next() {
                if ch == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        value.push('\'');
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    value.push(ch);
                }
            }
            if closed {
                // FITS pads quoted strings on the right. Leading spaces are
                // data (DATASUM in ExifTool's fixture relies on this).
                return Some((keyword, value.trim_end().to_string(), None));
            }
        }

        let (value, comment) = if let Some(slash_pos) = value_part.find('/') {
            (
                value_part[..slash_pos].trim(),
                Some(value_part[slash_pos + 1..].trim().to_string()),
            )
        } else {
            (value_part.trim(), None)
        };
        if value.is_empty() {
            None
        } else {
            // FITS permits Fortran D exponents; ExifTool renders both D and E
            // using an `e` before passing the value on.
            let normalized_number = value.replace(['D', 'E'], "e");
            let value = if normalized_number.parse::<f64>().is_ok() {
                normalized_number
            } else {
                value.to_string()
            };
            Some((keyword, value, comment))
        }
    }

    /// Resolve FITS keywords the same way ExifTool does.
    ///
    /// Standard names are generated from `Image::ExifTool::FITS::Main`. Any
    /// other valid keyword is lowercased, title-cased, and has underscores
    /// removed while capitalizing the following character.
    fn tag_name(keyword: &str) -> String {
        if let Some((_, name)) = FITS_TAG_NAMES
            .iter()
            .find(|(candidate, _)| *candidate == keyword)
        {
            return (*name).to_string();
        }

        let mut name = String::with_capacity(keyword.len());
        let mut capitalize = true;
        for ch in keyword.chars() {
            if ch == '_' {
                capitalize = true;
            } else if capitalize {
                name.push(ch.to_ascii_uppercase());
                capitalize = false;
            } else {
                name.push(ch.to_ascii_lowercase());
            }
        }
        name
    }

    fn tag_value(value: String) -> TagValue {
        if let Ok(integer) = value.parse::<i64>() {
            TagValue::Integer(integer)
        } else if let Ok(float) = value.parse::<f64>() {
            TagValue::Float(float)
        } else {
            TagValue::String(value)
        }
    }

    /// Parses FITS header and extracts all metadata
    fn parse_header(reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let mut offset = 0usize;
        let mut naxis_values: Vec<i64> = Vec::new();

        // Read header blocks until END keyword
        loop {
            // Read one FITS block (2880 bytes)
            let block_size = FITS_BLOCK_SIZE.min(reader.size() as usize - offset);
            if block_size < FITS_RECORD_SIZE {
                break;
            }

            let block = reader.read(offset as u64, block_size)?;

            // Process 80-byte records
            for chunk in block.chunks(FITS_RECORD_SIZE) {
                if chunk.len() != FITS_RECORD_SIZE {
                    break;
                }

                if let Some((keyword, value, comment)) = Self::parse_record(chunk) {
                    // Comments after a card value describe that card; ExifTool
                    // does not emit them as separate `KeywordComment` tags.
                    let _ = comment;

                    match keyword.as_str() {
                        "END" => {
                            // Process collected data
                            Self::finalize_metadata(&mut metadata, &naxis_values);
                            return Ok(metadata);
                        }
                        // ProcessFITS consumes SIMPLE while validating the
                        // signature, so it is not reported as metadata.
                        "SIMPLE" => {}
                        "HISTORY" | "COMMENT" => {
                            // MetadataMap represents one value per name, which
                            // matches ExifTool without `-a`: the last wins.
                            metadata.insert(Self::tag_name(&keyword), TagValue::String(value));
                        }
                        k if k.starts_with("NAXIS") && k.len() > 5 => {
                            if let Ok(axis_val) = value.parse::<i64>() {
                                metadata
                                    .insert(Self::tag_name(&keyword), TagValue::Integer(axis_val));
                                naxis_values.push(axis_val);
                            }
                        }
                        _ => {
                            if !value.is_empty() {
                                metadata.insert(Self::tag_name(&keyword), Self::tag_value(value));
                            }
                        }
                    }
                }
            }

            offset += FITS_BLOCK_SIZE;
            if offset >= reader.size() as usize {
                break;
            }
        }

        Self::finalize_metadata(&mut metadata, &naxis_values);
        Ok(metadata)
    }

    /// Finalizes metadata by calculating dimensions and other derived values
    fn finalize_metadata(metadata: &mut MetadataMap, naxis_values: &[i64]) {
        // Calculate image dimensions
        if naxis_values.len() >= 2 {
            let width = naxis_values[0];
            let height = naxis_values[1];

            metadata.insert("ImageWidth".to_string(), TagValue::Integer(width));
            metadata.insert("ImageHeight".to_string(), TagValue::Integer(height));

            if naxis_values.len() >= 3 {
                let depth = naxis_values[2];
                metadata.insert("ImageDepth".to_string(), TagValue::Integer(depth));
            }
        }
    }
}

impl FormatParser for FITSParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid FITS signature"));
        }

        let mut metadata = Self::parse_header(reader)?;

        // Add basic file info
        metadata.insert("File:FileType", TagValue::String("FITS".to_string()));
        metadata.insert(
            "File:FileTypeExtension",
            TagValue::String("fits".to_string()),
        );
        metadata.insert("File:MIMEType", TagValue::String("image/fits".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::FITS)
    }
}

/// Parses metadata from FITS files.
///
/// This is a convenience wrapper around FITSParser that provides a functional API.
pub fn parse_fits_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = FITSParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

const DICOM_MAGIC_OFFSET: usize = 128;
const DICOM_DATA_OFFSET: usize = 132;

#[derive(Clone, Copy)]
struct DicomEncoding {
    order: ByteOrder,
    explicit_vr: bool,
}

impl DicomEncoding {
    const EXPLICIT_LE: Self = Self {
        order: ByteOrder::Little,
        explicit_vr: true,
    };
}

struct DicomElement<'a> {
    group: u16,
    element: u16,
    value: &'a [u8],
    next_offset: usize,
}

fn dicom_long_vr(vr: [u8; 2]) -> bool {
    matches!(
        &vr,
        b"OB" | b"OD" | b"OF" | b"OL" | b"OV" | b"OW" | b"SQ" | b"UC" | b"UN" | b"UR" | b"UT"
    )
}

fn parse_dicom_element<'a>(
    data: &'a [u8],
    offset: usize,
    encoding: DicomEncoding,
) -> Result<DicomElement<'a>> {
    let reader = EndianReader::new(data, encoding.order);
    let group = reader
        .u16_at(offset)
        .ok_or_else(|| ExifToolError::parse_error_at("truncated DICOM tag group", offset))?;
    let element = reader
        .u16_at(offset + 2)
        .ok_or_else(|| ExifToolError::parse_error_at("truncated DICOM tag element", offset))?;

    let (header_len, value_len) = if encoding.explicit_vr {
        let vr_bytes = data.get(offset + 4..offset + 6).ok_or_else(|| {
            ExifToolError::parse_error_at("truncated DICOM value representation", offset)
        })?;
        let vr = [vr_bytes[0], vr_bytes[1]];
        if dicom_long_vr(vr) {
            (
                12usize,
                reader.u32_at(offset + 8).ok_or_else(|| {
                    ExifToolError::parse_error_at("truncated DICOM value length", offset)
                })?,
            )
        } else {
            (
                8usize,
                u32::from(reader.u16_at(offset + 6).ok_or_else(|| {
                    ExifToolError::parse_error_at("truncated DICOM value length", offset)
                })?),
            )
        }
    } else {
        (
            8usize,
            reader.u32_at(offset + 4).ok_or_else(|| {
                ExifToolError::parse_error_at("truncated DICOM value length", offset)
            })?,
        )
    };

    if value_len == u32::MAX {
        return Err(ExifToolError::parse_error_at(
            "undefined-length DICOM element is not supported",
            offset,
        ));
    }
    let value_len = usize::try_from(value_len)
        .map_err(|_| ExifToolError::parse_error_at("DICOM value is too large", offset))?;
    let value_start = offset
        .checked_add(header_len)
        .ok_or_else(|| ExifToolError::parse_error_at("DICOM offset overflow", offset))?;
    let next_offset = value_start
        .checked_add(value_len)
        .ok_or_else(|| ExifToolError::parse_error_at("DICOM value length overflow", offset))?;
    let value = data.get(value_start..next_offset).ok_or_else(|| {
        ExifToolError::parse_error_at("DICOM value extends beyond file", offset)
    })?;

    Ok(DicomElement {
        group,
        element,
        value,
        next_offset,
    })
}

fn dicom_text(value: &[u8]) -> String {
    String::from_utf8_lossy(value)
        .trim_end_matches(['\0', ' '])
        .to_string()
}

fn dicom_date(value: &[u8]) -> String {
    let text = dicom_text(value);
    if text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_digit()) {
        format!("{}:{}:{}", &text[..4], &text[4..6], &text[6..])
    } else {
        text
    }
}

fn dicom_time(value: &[u8]) -> String {
    let text = dicom_text(value);
    let main_len = text.find('.').unwrap_or(text.len()).min(6);
    let main = &text[..main_len];
    if main.len() < 2 || !main.bytes().all(|byte| byte.is_ascii_digit()) {
        return text;
    }

    let mut result = main[..2].to_string();
    if main.len() >= 4 {
        result.push(':');
        result.push_str(&main[2..4]);
    }
    if main.len() >= 6 {
        result.push(':');
        result.push_str(&main[4..6]);
    }
    result.push_str(&text[main_len..]);
    result
}

fn dicom_us(value: &[u8], order: ByteOrder) -> Result<String> {
    if value.len() % 2 != 0 {
        return Err(ExifToolError::parse_error(
            "odd byte count for DICOM unsigned-short value",
        ));
    }
    let reader = EndianReader::new(value, order);
    let mut values = Vec::with_capacity(value.len() / 2);
    for offset in (0..value.len()).step_by(2) {
        let number = reader.u16_at(offset).ok_or_else(|| {
            ExifToolError::parse_error_at("truncated DICOM unsigned-short value", offset)
        })?;
        values.push(number.to_string());
    }
    Ok(values.join(" "))
}

fn dicom_tag_name(group: u16, element: u16) -> Option<String> {
    let name = match (group, element) {
        (0x0008, 0x0050) => "AccessionNumber",
        (0x0008, 0x0022) => "AcquisitionDate",
        (0x0008, 0x0032) => "AcquisitionTime",
        (0x0010, 0x21B0) => "AdditionalPatientHistory",
        (0x0018, 0x1310) => "AcquisitionMatrix",
        (0x0020, 0x0012) => "AcquisitionNumber",
        _ => return None,
    };

    let table = oxidex_tags::specialty::get_tag_table("DICOM::Main")?;
    let tag = table.tags.iter().find(|tag| tag.name == name)?;
    let group = table.name.split("::").next()?;
    Some(format!("{group}:{}", tag.name))
}

fn dicom_value(element: &DicomElement<'_>, encoding: DicomEncoding) -> Result<TagValue> {
    let value = match (element.group, element.element) {
        (0x0008, 0x0022) => dicom_date(element.value),
        (0x0008, 0x0032) => dicom_time(element.value),
        (0x0018, 0x1310) => dicom_us(element.value, encoding.order)?,
        _ => dicom_text(element.value),
    };
    Ok(TagValue::String(value))
}

/// Parses DICOM Part 10 metadata using this existing specialty parser module.
pub fn parse_dicom_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let size = usize::try_from(reader.size())
        .map_err(|_| ExifToolError::parse_error("DICOM file is too large"))?;
    let data = reader.read(0, size)?;

    if data.get(DICOM_MAGIC_OFFSET..DICOM_DATA_OFFSET) != Some(b"DICM") {
        return Err(ExifToolError::parse_error("invalid DICOM signature"));
    }

    let mut metadata = MetadataMap::new();
    let mut offset = DICOM_DATA_OFFSET;
    let mut data_encoding = DicomEncoding::EXPLICIT_LE;
    let mut file_meta = true;

    while offset < data.len() {
        if data.len() - offset < 8 {
            break;
        }
        if file_meta {
            let little = EndianReader::little_endian(data);
            let group = little.u16_at(offset).ok_or_else(|| {
                ExifToolError::parse_error_at("truncated DICOM group", offset)
            })?;
            if group != 0x0002 {
                file_meta = false;
            }
        }

        let encoding = if file_meta {
            DicomEncoding::EXPLICIT_LE
        } else {
            data_encoding
        };
        let element = parse_dicom_element(data, offset, encoding)?;

        if element.group == 0x0002 && element.element == 0x0010 {
            data_encoding = match dicom_text(element.value).as_str() {
                "1.2.840.10008.1.2" => DicomEncoding {
                    order: ByteOrder::Little,
                    explicit_vr: false,
                },
                "1.2.840.10008.1.2.2" => DicomEncoding {
                    order: ByteOrder::Big,
                    explicit_vr: true,
                },
                _ => DicomEncoding::EXPLICIT_LE,
            };
        }

        if let Some(name) = dicom_tag_name(element.group, element.element) {
            metadata.insert(name, dicom_value(&element, encoding)?);
        }
        if element.next_offset <= offset {
            return Err(ExifToolError::parse_error_at(
                "DICOM element did not advance",
                offset,
            ));
        }
        offset = element.next_offset;
    }

    metadata.insert("File:FileType", TagValue::new_string("DICOM"));
    metadata.insert("File:FileTypeExtension", TagValue::new_string("dcm"));
    metadata.insert(
        "File:MIMEType",
        TagValue::new_string("application/dicom"),
    );
    Ok(metadata)
}

#[cfg(test)]
mod dicom_tests {
    use super::*;

    #[test]
    fn parses_requested_tags_from_real_dicom_sample() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = format!(
            "{}/DICOM.dcm",
            crate::test_support::PINNED_CORPUS_ROOT
        );
        let data = std::fs::read(path).expect("pinned DICOM sample should be readable");
        let metadata = parse_dicom_metadata(&crate::test_support::TestReader::new(data))
            .expect("pinned DICOM sample should parse");

        assert_eq!(
            metadata.get("DICOM:AccessionNumber"),
            Some(&TagValue::String(String::new()))
        );
        assert_eq!(
            metadata.get("DICOM:AcquisitionDate"),
            Some(&TagValue::String("2001:03:16".to_string()))
        );
        assert_eq!(
            metadata.get("DICOM:AcquisitionMatrix"),
            Some(&TagValue::String("0 256 256 0".to_string()))
        );
        assert_eq!(
            metadata.get("DICOM:AcquisitionNumber"),
            Some(&TagValue::String("31763".to_string()))
        );
        assert_eq!(
            metadata.get("DICOM:AcquisitionTime"),
            Some(&TagValue::String("14:34:15".to_string()))
        );
        assert_eq!(
            metadata.get("DICOM:AdditionalPatientHistory"),
            Some(&TagValue::String(String::new()))
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn card(text: &str) -> [u8; FITS_RECORD_SIZE] {
        assert!(text.len() <= FITS_RECORD_SIZE);
        let mut card = [b' '; FITS_RECORD_SIZE];
        card[..text.len()].copy_from_slice(text.as_bytes());
        card
    }

    fn fits(cards: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for text in cards {
            bytes.extend_from_slice(&card(text));
        }
        bytes.resize(FITS_BLOCK_SIZE, b' ');
        bytes
    }

    #[test]
    fn canonical_names_cover_the_value_confirmed_fits_renames() {
        let expected = [
            ("BITPIX", "Bitpix"),
            ("ORIGIN", "Origin"),
            ("CREATOR", "Creator"),
            ("TIME-OBS", "ObservationTime"),
            ("TIME-END", "ObservationTimeEnd"),
            ("TIMESYS", "Timesys"),
            ("MJDREFI", "Mjdrefi"),
            ("MJDREFF", "Mjdreff"),
            ("TIMEZERO", "Timezero"),
            ("TIMEUNIT", "Timeunit"),
            ("TIMEREF", "Timeref"),
            ("TASSIGN", "Tassign"),
            ("TIERRELA", "Tierrela"),
            ("TIERABSO", "Tierabso"),
            ("OBJECT", "Object"),
            ("RA_OBJ", "RaObj"),
            ("DEC_OBJ", "DecObj"),
            ("EQUINOX", "Equinox"),
            ("RADECSYS", "Radecsys"),
            ("OBSERVER", "Observer"),
            ("OBS_ID", "ObsId"),
            ("CHECKSUM", "Checksum"),
        ];
        for (keyword, name) in expected {
            assert_eq!(FITSParser::tag_name(keyword), name);
        }
    }

    #[test]
    fn quoted_slashes_and_escaped_quotes_are_values_not_comments() {
        assert_eq!(
            FITSParser::parse_record(&card(
                "TIMVERSN= 'XFF/95-004'         / XFF design document"
            )),
            Some(("TIMVERSN".into(), "XFF/95-004".into(), None))
        );
        assert_eq!(
            FITSParser::parse_record(&card("OBJECT  = 'O''Brien / field'   / observer target")),
            Some(("OBJECT".into(), "O'Brien / field".into(), None))
        );
        assert_eq!(
            FITSParser::parse_record(&card("DATASUM = '         0'         / data unit checksum")),
            Some(("DATASUM".into(), "         0".into(), None))
        );
    }

    #[test]
    fn unquoted_card_comments_are_separated_from_values() {
        assert_eq!(
            FITSParser::parse_record(&card(
                "MJDREFF =   6.965740740000D-04 / fractional reference"
            )),
            Some((
                "MJDREFF".into(),
                "6.965740740000e-04".into(),
                Some("fractional reference".into()),
            ))
        );
    }

    #[test]
    fn parser_uses_exiftool_names_and_does_not_emit_card_comments() {
        let reader = TestReader::new(fits(&[
            "SIMPLE  =                    T / conforms to FITS",
            "BITPIX  =                    8 / bits per pixel",
            "NAXIS   =                    0 / axes",
            "DATE    = '28/01/97'           / creation date",
            "TIME-OBS= '11:56:26'           / start time",
            "TIMVERSN= 'XFF/95-004'         / design document",
            "DATASUM = '         0'         / data checksum",
            "END",
        ]));

        let metadata = FITSParser.parse(&reader).unwrap();
        assert_eq!(metadata.get_integer("Bitpix"), Some(8));
        assert_eq!(metadata.get_integer("Naxis"), Some(0));
        assert_eq!(metadata.get_string("CreateDate"), Some("28/01/97"));
        assert_eq!(metadata.get_string("ObservationTime"), Some("11:56:26"));
        assert_eq!(metadata.get_string("Timversn"), Some("XFF/95-004"));
        assert_eq!(metadata.get_string("Datasum"), Some("         0"));
        assert!(!metadata.keys().any(|name| name.ends_with("Comment")));
    }
}
