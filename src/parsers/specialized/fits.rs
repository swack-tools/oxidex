//! FITS (Flexible Image Transport System) parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

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
