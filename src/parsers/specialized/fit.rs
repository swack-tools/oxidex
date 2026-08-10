//! Garmin FIT activity-file parser.
//!
//! Modeled on ExifTool 13.59 Garmin.pm ProcessFIT. Divergences are always
//! omissions, never approximations: fields whose exact ExifTool rendering
//! cannot be reproduced (string/byte/float base types, oversized values fed
//! through Perl numeric stringification) are skipped, not guessed.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

const SESSION_MESSAGE: u16 = 18;

#[derive(Clone, Copy)]
struct Field {
    number: u8,
    size: usize,
    base_type: u8,
}

struct Definition {
    global_number: u16,
    big_endian: bool,
    fields: Vec<Field>,
    developer_size: usize,
}

pub struct FITParser;

impl FITParser {
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        Ok(reader.read(8, 4)? == b".FIT")
    }

    /// Walk the record stream. A mid-stream malformation (missing local
    /// definition, truncated record, ...) ends the walk but keeps everything
    /// already extracted, matching ExifTool's warn-and-return-1 tolerance.
    fn parse_records(data: &[u8], metadata: &mut MetadataMap) {
        let mut definitions: [Option<Definition>; 16] = std::array::from_fn(|_| None);
        let mut offset = 0usize;
        // ExifTool without ExtractEmbedded processes only the FIRST data
        // message of each global message number (the %done gate).
        let mut session_done = false;

        while offset < data.len() {
            let header = data[offset];
            offset += 1;
            let compressed = header & 0x80 != 0;
            let local = if compressed {
                usize::from((header >> 5) & 0x03)
            } else {
                usize::from(header & 0x0f)
            };

            if !compressed && header & 0x40 != 0 {
                // Definition message: reserved, architecture, global number,
                // field count, then 3 bytes per field.
                let Some(fixed) = data.get(offset..offset + 5) else {
                    break;
                };
                // ExifTool: SetByteOrder(Get8u(..) ? 'MM' : 'II') -- any
                // non-zero architecture byte selects big-endian.
                let big_endian = fixed[1] != 0;
                let global_number = if big_endian {
                    u16::from_be_bytes([fixed[2], fixed[3]])
                } else {
                    u16::from_le_bytes([fixed[2], fixed[3]])
                };
                let count = usize::from(fixed[4]);
                offset += 5;
                let Some(bytes) = data.get(offset..offset + count * 3) else {
                    break;
                };
                // Fields with base types ExifTool does not know still count
                // toward the record size (only extraction skips them), so
                // keep every declared field here.
                let fields = bytes
                    .chunks_exact(3)
                    .map(|field| Field {
                        number: field[0],
                        size: usize::from(field[1]),
                        base_type: field[2],
                    })
                    .collect();
                offset += count * 3;

                let mut developer_size = 0usize;
                if header & 0x20 != 0 {
                    let Some(&dev_count) = data.get(offset) else {
                        break;
                    };
                    offset += 1;
                    let length = usize::from(dev_count) * 3;
                    let Some(bytes) = data.get(offset..offset + length) else {
                        break;
                    };
                    developer_size = bytes
                        .chunks_exact(3)
                        .map(|field| usize::from(field[1]))
                        .sum();
                    offset += length;
                }
                definitions[local] = Some(Definition {
                    global_number,
                    big_endian,
                    fields,
                    developer_size,
                });
                continue;
            }

            // Data message (normal or compressed-timestamp header). ExifTool
            // reads the full defined size in both cases -- field 253 is NOT
            // elided from compressed-header records.
            let Some(definition) = definitions[local].as_ref() else {
                break; // "Missing definition for local message"
            };
            let record_size: usize = definition
                .fields
                .iter()
                .map(|field| field.size)
                .sum::<usize>()
                + definition.developer_size;
            let Some(record) = data.get(offset..offset + record_size) else {
                break; // "Truncated data message"
            };
            if definition.global_number == SESSION_MESSAGE && !session_done {
                session_done = true;
                Self::extract_session(definition, record, metadata);
            }
            offset += record_size;
        }
    }

    fn extract_session(definition: &Definition, record: &[u8], metadata: &mut MetadataMap) {
        let mut offset = 0usize;
        for field in &definition.fields {
            let Some(value) = record.get(offset..offset + field.size) else {
                return;
            };
            offset += field.size;
            if !matches!(field.number, 16 | 18 | 92 | 116..=119 | 122) {
                continue;
            }
            let Some(text) = Self::read_value(value, field.base_type, definition.big_endian) else {
                continue; // invalid sentinel, unknown type, or bad count
            };
            // Garmin.pm %Image::ExifTool::Garmin::Session (13.59):
            //   16  => AvgHeartRate            PrintConv '"$val bpm"'
            //   18  => AvgCadence              PrintConv '"$val rpm"'
            //   92  => AvgFractionalCadence    ValueConv '$val / 128', PrintConv '"$val rpm"'
            //   116 => AvgLeftPowerPhase       117 => AvgLeftPowerPhasePeak
            //   118 => AvgRightPowerPhase      119 => AvgRightPowerPhasePeak
            //   122 => AvgCadencePosition
            match field.number {
                16 => {
                    metadata.insert(
                        "Garmin:AvgHeartRate",
                        TagValue::String(format!("{text} bpm")),
                    );
                }
                18 => {
                    metadata.insert("Garmin:AvgCadence", TagValue::String(format!("{text} rpm")));
                }
                92 => {
                    // ValueConv '$val / 128': Perl numifies the value string,
                    // i.e. uses its leading number even for arrays. Restrict
                    // to magnitudes below 2^32 so the quotient needs at most
                    // 15 significant digits and Rust's float formatting is
                    // guaranteed to match Perl's %.15g; larger values are
                    // omitted rather than approximated.
                    let first = text.split(' ').next().unwrap_or("");
                    if let Ok(raw) = first.parse::<i64>() {
                        if raw.unsigned_abs() < 1 << 32 {
                            metadata.insert(
                                "Garmin:AvgFractionalCadence",
                                TagValue::String(format!("{} rpm", raw as f64 / 128.0)),
                            );
                        }
                    }
                }
                116 => {
                    metadata.insert("Garmin:AvgLeftPowerPhase", TagValue::String(text));
                }
                117 => {
                    metadata.insert("Garmin:AvgLeftPowerPhasePeak", TagValue::String(text));
                }
                118 => {
                    metadata.insert("Garmin:AvgRightPowerPhase", TagValue::String(text));
                }
                119 => {
                    metadata.insert("Garmin:AvgRightPowerPhasePeak", TagValue::String(text));
                }
                122 => {
                    metadata.insert("Garmin:AvgCadencePosition", TagValue::String(text));
                }
                _ => {}
            }
        }
    }

    /// Decode one field the way ExifTool's ReadValue + invalid-sentinel check
    /// does (Garmin.pm %baseType), returning the space-joined value string.
    ///
    /// Returns None when ExifTool would skip the field (base type not in
    /// %baseType, non-integral count, single value equal to the type's
    /// invalid sentinel -- multi-element arrays of sentinels are kept, per
    /// the string comparison `lc $val eq $baseType{$type}[2]`) and for the
    /// string/byte/float base types, whose Perl rendering we cannot
    /// reproduce exactly and therefore omit rather than approximate.
    fn read_value(value: &[u8], base_type: u8, big_endian: bool) -> Option<String> {
        let (width, signed, sentinel): (usize, bool, &str) = match base_type {
            0x00 | 0x02 => (1, false, "255"),           // enum, uint8
            0x01 => (1, true, "127"),                   // sint8
            0x83 => (2, true, "32767"),                 // sint16
            0x84 => (2, false, "65535"),                // uint16
            0x85 => (4, true, "2147483647"),            // sint32
            0x86 => (4, false, "4294967295"),           // uint32
            0x0a => (1, false, "0"),                    // uint8z
            0x8b => (2, false, "0"),                    // uint16z
            0x8c => (4, false, "0"),                    // uint32z
            0x8e => (8, true, "9223372036854775807"),   // sint64
            0x8f => (8, false, "18446744073709551615"), // uint64
            0x90 => (8, false, "0"),                    // uint64z
            // 0x07 string, 0x0d byte, 0x88 float32, 0x89 float64: omitted
            // (cannot guarantee ExifTool's exact formatting); anything else
            // is not in %baseType and ExifTool never extracts it.
            _ => return None,
        };
        if value.is_empty() || value.len() % width != 0 {
            return None; // ExifTool: "Bad count" warning, field skipped
        }
        let mut parts = Vec::with_capacity(value.len() / width);
        for chunk in value.chunks_exact(width) {
            let mut raw = 0u64;
            if big_endian {
                for &byte in chunk {
                    raw = raw << 8 | u64::from(byte);
                }
            } else {
                for &byte in chunk.iter().rev() {
                    raw = raw << 8 | u64::from(byte);
                }
            }
            if signed {
                let shift = 64 - width * 8;
                parts.push((((raw << shift) as i64) >> shift).to_string());
            } else {
                parts.push(raw.to_string());
            }
        }
        let text = parts.join(" ");
        if text == sentinel {
            return None; // invalid value, suppressed by ExifTool
        }
        Some(text)
    }
}

impl FormatParser for FITParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("invalid FIT signature"));
        }
        // ExifTool reads 12 header bytes (so data never starts before offset
        // 12), takes the data length from bytes 4..8, and stops at
        // header_size + data_size without pre-validating it against the file
        // size -- a truncated file still yields the tags read so far.
        let header = reader.read(0, 12)?;
        let header_size = usize::from(header[0]);
        let data_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let start = header_size.max(12);
        let limit = header_size
            .saturating_add(data_size)
            .min(reader.size() as usize);
        let mut metadata = MetadataMap::new();
        if limit > start {
            let data = reader.read(start as u64, limit - start)?;
            Self::parse_records(data, &mut metadata);
        }
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        format == FileFormat::FIT
    }
}

pub fn parse_fit_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    FITParser.parse(reader).map_err(|error| error.to_string())
}
