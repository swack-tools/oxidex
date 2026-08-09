//! Garmin FIT activity-file parser.

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

    fn byte(data: &[u8], offset: usize, context: &str) -> Result<u8> {
        data.get(offset)
            .copied()
            .ok_or_else(|| ExifToolError::parse_error(format!("truncated FIT {context}")))
    }

    fn parse_records(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
        let mut definitions: [Option<Definition>; 16] = std::array::from_fn(|_| None);
        let mut offset = 0usize;

        while offset < data.len() {
            let header = Self::byte(data, offset, "record header")?;
            offset += 1;
            let compressed = header & 0x80 != 0;
            let local = if compressed {
                usize::from((header >> 5) & 0x03)
            } else {
                usize::from(header & 0x0f)
            };

            if !compressed && header & 0x40 != 0 {
                let architecture = Self::byte(data, offset + 1, "definition")?;
                let big_endian = match architecture {
                    0 => false,
                    1 => true,
                    _ => return Err(ExifToolError::parse_error("invalid FIT architecture")),
                };
                let number = data
                    .get(offset + 2..offset + 4)
                    .ok_or_else(|| ExifToolError::parse_error("truncated FIT definition"))?;
                let global_number = if big_endian {
                    u16::from_be_bytes([number[0], number[1]])
                } else {
                    u16::from_le_bytes([number[0], number[1]])
                };
                let count = usize::from(Self::byte(data, offset + 4, "definition")?);
                offset = offset
                    .checked_add(5)
                    .ok_or_else(|| ExifToolError::parse_error("FIT offset overflow"))?;
                let definitions_len = count
                    .checked_mul(3)
                    .ok_or_else(|| ExifToolError::parse_error("FIT field count overflow"))?;
                let end = offset
                    .checked_add(definitions_len)
                    .ok_or_else(|| ExifToolError::parse_error("FIT offset overflow"))?;
                let bytes = data
                    .get(offset..end)
                    .ok_or_else(|| ExifToolError::parse_error("truncated FIT field definitions"))?;
                let fields = bytes
                    .chunks_exact(3)
                    .map(|field| Field {
                        number: field[0],
                        size: usize::from(field[1]),
                        base_type: field[2],
                    })
                    .collect();
                offset = end;

                let mut developer_size = 0usize;
                if header & 0x20 != 0 {
                    let count = usize::from(Self::byte(data, offset, "developer definition")?);
                    offset += 1;
                    let length = count.checked_mul(3).ok_or_else(|| {
                        ExifToolError::parse_error("FIT developer field count overflow")
                    })?;
                    let end = offset.checked_add(length).ok_or_else(|| {
                        ExifToolError::parse_error("FIT developer definition overflow")
                    })?;
                    let bytes = data.get(offset..end).ok_or_else(|| {
                        ExifToolError::parse_error("truncated FIT developer definition")
                    })?;
                    developer_size = bytes.chunks_exact(3).try_fold(0usize, |size, field| {
                        size.checked_add(usize::from(field[1]))
                    }).ok_or_else(|| ExifToolError::parse_error("FIT record size overflow"))?;
                    offset = end;
                }
                definitions[local] = Some(Definition {
                    global_number,
                    big_endian,
                    fields,
                    developer_size,
                });
                continue;
            }

            let definition = definitions[local].as_ref().ok_or_else(|| {
                ExifToolError::parse_error("FIT data record has no local definition")
            })?;
            let native_size = definition.fields.iter().try_fold(0usize, |size, field| {
                if compressed && field.number == 253 {
                    Ok(size)
                } else {
                    size.checked_add(field.size)
                        .ok_or_else(|| ExifToolError::parse_error("FIT record size overflow"))
                }
            })?;
            let record_size = native_size
                .checked_add(definition.developer_size)
                .ok_or_else(|| ExifToolError::parse_error("FIT record size overflow"))?;
            let end = offset
                .checked_add(record_size)
                .ok_or_else(|| ExifToolError::parse_error("FIT record offset overflow"))?;
            let record = data
                .get(offset..end)
                .ok_or_else(|| ExifToolError::parse_error("truncated FIT data record"))?;
            if definition.global_number == SESSION_MESSAGE {
                Self::extract_session(definition, record, compressed, metadata)?;
            }
            offset = end;
        }
        Ok(())
    }

    fn extract_session(
        definition: &Definition,
        record: &[u8],
        compressed: bool,
        metadata: &mut MetadataMap,
    ) -> Result<()> {
        let mut offset = 0usize;
        for field in &definition.fields {
            if compressed && field.number == 253 {
                continue;
            }
            let end = offset
                .checked_add(field.size)
                .ok_or_else(|| ExifToolError::parse_error("FIT session offset overflow"))?;
            let value = record
                .get(offset..end)
                .ok_or_else(|| ExifToolError::parse_error("truncated FIT session field"))?;
            match field.number {
                16 => Self::insert_unit(
                    metadata,
                    "AvgHeartRate",
                    value,
                    field,
                    definition.big_endian,
                    "bpm",
                )?,
                18 => Self::insert_unit(
                    metadata,
                    "AvgCadence",
                    value,
                    field,
                    definition.big_endian,
                    "rpm",
                )?,
                92 => {
                    let raw = Self::unsigned(value, field, definition.big_endian)?;
                    metadata.insert(
                        "Garmin:AvgFractionalCadence",
                        TagValue::String(format!("{} rpm", raw as f64 / 128.0)),
                    );
                }
                118 => Self::insert_array(metadata, "AvgLeftPowerPhase", value),
                119 => Self::insert_array(metadata, "AvgLeftPowerPhasePeak", value),
                122 => Self::insert_array(metadata, "AvgCadencePosition", value),
                _ => {}
            }
            offset = end;
        }
        Ok(())
    }

    fn unsigned(value: &[u8], field: &Field, big_endian: bool) -> Result<u64> {
        let result = match (field.base_type & 0x1f, value) {
            (0 | 2 | 10 | 13, [byte]) => u64::from(*byte),
            (3 | 4 | 11, [a, b]) => u64::from(if big_endian {
                u16::from_be_bytes([*a, *b])
            } else {
                u16::from_le_bytes([*a, *b])
            }),
            (5 | 6 | 12, [a, b, c, d]) => u64::from(if big_endian {
                u32::from_be_bytes([*a, *b, *c, *d])
            } else {
                u32::from_le_bytes([*a, *b, *c, *d])
            }),
            _ => return Err(ExifToolError::parse_error("invalid FIT unsigned field")),
        };
        Ok(result)
    }

    fn insert_unit(
        metadata: &mut MetadataMap,
        name: &str,
        value: &[u8],
        field: &Field,
        big_endian: bool,
        unit: &str,
    ) -> Result<()> {
        let value = Self::unsigned(value, field, big_endian)?;
        metadata.insert(
            format!("Garmin:{name}"),
            TagValue::String(format!("{value} {unit}")),
        );
        Ok(())
    }

    fn insert_array(metadata: &mut MetadataMap, name: &str, value: &[u8]) {
        let value = value
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        metadata.insert(format!("Garmin:{name}"), TagValue::String(value));
    }
}

impl FormatParser for FITParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("invalid FIT signature"));
        }
        let header_size = usize::from(reader.read(0, 1)?[0]);
        if header_size < 12 {
            return Err(ExifToolError::parse_error("invalid FIT header size"));
        }
        let header = reader.read(0, header_size)?;
        let size_bytes = header
            .get(4..8)
            .ok_or_else(|| ExifToolError::parse_error("truncated FIT header"))?;
        let data_size =
            u32::from_le_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]]) as usize;
        let end = header_size
            .checked_add(data_size)
            .ok_or_else(|| ExifToolError::parse_error("FIT data size overflow"))?;
        if end > reader.size() as usize {
            return Err(ExifToolError::parse_error("truncated FIT data section"));
        }
        let data = reader.read(header_size as u64, data_size)?;
        let mut metadata = MetadataMap::new();
        Self::parse_records(data, &mut metadata)?;
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        format == FileFormat::FIT
    }
}

pub fn parse_fit_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    FITParser.parse(reader).map_err(|error| error.to_string())
}
