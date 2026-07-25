//! FITS (Flexible Image Transport System) parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

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

        let record_str = String::from_utf8_lossy(record);

        // Check for END keyword
        if record_str.starts_with("END ") || record_str.starts_with("END\0") {
            return Some(("END".to_string(), String::new(), None));
        }

        // Check for HISTORY or COMMENT records (no '=')
        if let Some(rest) = record_str.strip_prefix("HISTORY ") {
            let content = rest.trim().to_string();
            return Some(("HISTORY".to_string(), content, None));
        }
        if let Some(rest) = record_str.strip_prefix("COMMENT ") {
            let content = rest.trim().to_string();
            return Some(("COMMENT".to_string(), content, None));
        }

        // Find '=' separator
        let eq_pos = record_str.find('=')?;
        let keyword = record_str[..eq_pos].trim().to_string();

        // Parse value and optional comment
        let value_part = &record_str[eq_pos + 1..];

        // Find comment separator '/'
        let (value_str, comment) = if let Some(slash_pos) = value_part.find('/') {
            (
                value_part[..slash_pos].trim(),
                Some(value_part[slash_pos + 1..].trim().to_string()),
            )
        } else {
            (value_part.trim(), None)
        };

        // Remove quotes if present
        let value = if value_str.starts_with('\'') && value_str.ends_with('\'') {
            value_str[1..value_str.len() - 1].trim().to_string()
        } else {
            value_str.to_string()
        };

        Some((keyword, value, comment))
    }

    /// Converts BITPIX value to friendly pixel format name
    fn bitpix_to_format(bitpix: i32) -> String {
        match bitpix {
            8 => "8-bit unsigned integer".to_string(),
            16 => "16-bit signed integer".to_string(),
            32 => "32-bit signed integer".to_string(),
            -32 => "32-bit floating point".to_string(),
            -64 => "64-bit floating point".to_string(),
            _ => format!("Unknown ({})", bitpix),
        }
    }

    /// Parses FITS header and extracts all metadata
    fn parse_header(reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let mut offset = 0usize;
        let mut naxis_values: Vec<i64> = Vec::new();
        let mut history_entries: Vec<String> = Vec::new();
        let mut comment_entries: Vec<String> = Vec::new();

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
                    // Store comment if present
                    if keyword != "HISTORY"
                        && keyword != "COMMENT"
                        && keyword != "END"
                        && let Some(cmt) = comment
                        && !cmt.is_empty()
                    {
                        metadata.insert(format!("{}Comment", keyword), TagValue::String(cmt));
                    }

                    match keyword.as_str() {
                        "END" => {
                            // Process collected data
                            Self::finalize_metadata(&mut metadata, &naxis_values);
                            return Ok(metadata);
                        }
                        "HISTORY" => history_entries.push(value),
                        "COMMENT" => comment_entries.push(value),
                        "SIMPLE" => {
                            metadata.insert(keyword, TagValue::String(value));
                        }
                        "BITPIX" => {
                            if let Ok(bitpix) = value.parse::<i32>() {
                                metadata
                                    .insert("BITPIX".to_string(), TagValue::Integer(bitpix as i64));
                                metadata.insert(
                                    "PixelFormat".to_string(),
                                    TagValue::String(Self::bitpix_to_format(bitpix)),
                                );
                            }
                        }
                        "NAXIS" => {
                            if let Ok(naxis) = value.parse::<i64>() {
                                metadata.insert(keyword, TagValue::Integer(naxis));
                            }
                        }
                        k if k.starts_with("NAXIS") && k.len() > 5 => {
                            if let Ok(axis_val) = value.parse::<i64>() {
                                metadata.insert(keyword.to_string(), TagValue::Integer(axis_val));
                                naxis_values.push(axis_val);
                            }
                        }
                        "BSCALE" | "BZERO" | "EXPTIME" => {
                            if let Ok(float_val) = value.parse::<f64>() {
                                metadata.insert(keyword, TagValue::Float(float_val));
                            }
                        }
                        "TELESCOP" | "INSTRUME" | "OBJECT" | "OBSERVER" | "ORIGIN" | "AUTHOR"
                        | "DATE-OBS" | "FILTER" => {
                            if !value.is_empty() {
                                metadata.insert(keyword, TagValue::String(value));
                            }
                        }
                        _ => {
                            // Store other keywords as strings
                            if !value.is_empty() {
                                metadata.insert(keyword, TagValue::String(value));
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

        // Store history and comments if present
        if !history_entries.is_empty() {
            metadata.insert(
                "History".to_string(),
                TagValue::Array(history_entries.into_iter().map(TagValue::String).collect()),
            );
        }
        if !comment_entries.is_empty() {
            metadata.insert(
                "Comments".to_string(),
                TagValue::Array(comment_entries.into_iter().map(TagValue::String).collect()),
            );
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
        metadata.insert("FileType".to_string(), TagValue::String("FITS".to_string()));
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
