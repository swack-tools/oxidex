//! TrueType Font (TTF) format parser
//!
//! Implements comprehensive metadata extraction from TrueType font files,
//! including name table records, timestamps, and font properties.
//!
//! TTF files use big-endian byte order for all multi-byte fields.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// TTF signature: 0x00 0x01 0x00 0x00 or "true"
const TTF_SIGNATURE_1: &[u8] = &[0x00, 0x01, 0x00, 0x00];
const TTF_SIGNATURE_2: &[u8] = b"true";

/// Platform IDs for name table records
const PLATFORM_UNICODE: u16 = 0;
const PLATFORM_MACINTOSH: u16 = 1;
const PLATFORM_WINDOWS: u16 = 3;

/// Platform-specific language IDs for localized name records.
///
/// The Macintosh values are the ones ExifTool lists in `%ttLang{Macintosh}`
/// (Font.pm) -- every value below is quoted from that table, not inferred
/// from the language's name or position. Two of them were originally wrong
/// (Spanish as 12, Italian as 4) and the recheck still passed, because
/// Font.ttf's Spanish and Italian records are 6 and 3: constant 12 matched
/// nothing, and 4 matched the *Dutch* record, so `FontSubfamily-it` would
/// have carried Dutch text. A sample only exercises the IDs it happens to
/// contain, so these must be read off the table rather than trusted to a
/// green recheck.
const LANGUAGE_DANISH_MACINTOSH: u16 = 7;
const LANGUAGE_GERMAN_MACINTOSH: u16 = 2;
const LANGUAGE_HEBREW_MACINTOSH: u16 = 10;
const LANGUAGE_SPANISH_MACINTOSH: u16 = 6;
const LANGUAGE_FINNISH_MACINTOSH: u16 = 13;
const LANGUAGE_FRENCH_MACINTOSH: u16 = 1;
const LANGUAGE_ITALIAN_MACINTOSH: u16 = 3;
const LANGUAGE_DUTCH_MACINTOSH: u16 = 4;
const LANGUAGE_NORWEGIAN_MACINTOSH: u16 = 9;
const LANGUAGE_JAPANESE_MACINTOSH: u16 = 11;
const LANGUAGE_KOREAN_MACINTOSH: u16 = 23;
const LANGUAGE_DANISH_WINDOWS: u16 = 0x0406;
const LANGUAGE_GERMAN_WINDOWS: u16 = 0x0407;
const LANGUAGE_HEBREW_WINDOWS: u16 = 0x040d;
const LANGUAGE_SPANISH_WINDOWS: u16 = 0x0c0a;
const LANGUAGE_FINNISH_WINDOWS: u16 = 0x040b;
const LANGUAGE_FRENCH_WINDOWS: u16 = 0x040c;
const LANGUAGE_ITALIAN_WINDOWS: u16 = 0x0410;
const LANGUAGE_ENGLISH_WINDOWS: u16 = 0x0409;
const LANGUAGE_JAPANESE_WINDOWS: u16 = 0x0411;
const LANGUAGE_KOREAN_WINDOWS: u16 = 0x0412;
const LANGUAGE_DUTCH_WINDOWS: u16 = 0x0413;
const LANGUAGE_NORWEGIAN_WINDOWS: u16 = 0x0414;

/// Name IDs for name table records
const NAME_COPYRIGHT: u16 = 0;
const NAME_FONT_FAMILY: u16 = 1;
const NAME_FONT_SUBFAMILY: u16 = 2;
const NAME_FULL_FONT_NAME: u16 = 4;
const NAME_VERSION: u16 = 5;
const NAME_POSTSCRIPT_NAME: u16 = 6;
const NAME_DESIGNER: u16 = 9;
const NAME_VENDOR_URL: u16 = 11;
const NAME_LICENSE: u16 = 13;

/// Table directory entry
#[derive(Debug, Clone)]
struct TableEntry {
    tag: [u8; 4],
    offset: u32,
    length: u32,
}

/// Name record from name table
#[derive(Debug)]
struct NameRecord {
    platform_id: u16,
    encoding_id: u16,
    language_id: u16,
    name_id: u16,
    length: u16,
    offset: u16,
}

/// TTF parser for extracting metadata from TrueType fonts
pub struct TTFParser;

impl TTFParser {
    /// Verifies TTF signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }

        let header = reader.read(0, 4)?;
        Ok(header == TTF_SIGNATURE_1 || header == TTF_SIGNATURE_2)
    }

    /// Reads number of tables (offset 4, 2 bytes)
    pub fn read_num_tables(reader: &dyn FileReader) -> Result<u16> {
        if reader.size() < 6 {
            return Ok(0);
        }

        let num_tables_bytes = reader.read(4, 2)?;
        let r = EndianReader::big_endian(num_tables_bytes);
        Ok(r.u16_at(0).unwrap_or(0))
    }

    /// Parses the table directory to find all tables
    fn parse_table_directory(reader: &dyn FileReader, num_tables: u16) -> Result<Vec<TableEntry>> {
        let mut tables = Vec::new();
        let table_dir_offset = 12u64; // After offset table

        for i in 0..num_tables {
            let entry_offset = table_dir_offset + (i as u64 * 16);
            if entry_offset + 16 > reader.size() {
                break;
            }

            let entry_data = reader.read(entry_offset, 16)?;
            let r = EndianReader::big_endian(entry_data);
            let tag = [entry_data[0], entry_data[1], entry_data[2], entry_data[3]];
            let offset = r.u32_at(8).unwrap_or(0);
            let length = r.u32_at(12).unwrap_or(0);

            tables.push(TableEntry {
                tag,
                offset,
                length,
            });
        }

        Ok(tables)
    }

    /// Finds a table by tag name
    fn find_table<'a>(tables: &'a [TableEntry], tag: &[u8; 4]) -> Option<&'a TableEntry> {
        tables.iter().find(|t| &t.tag == tag)
    }

    /// Parses name records from the name table
    fn parse_name_table(reader: &dyn FileReader, table: &TableEntry) -> Result<Vec<NameRecord>> {
        let offset = table.offset as u64;
        if offset + 6 > reader.size() {
            return Ok(Vec::new());
        }

        let header = reader.read(offset, 6)?;
        let r = EndianReader::big_endian(header);
        let count = r.u16_at(2).unwrap_or(0);

        let mut records = Vec::new();
        let records_start = offset + 6;

        for i in 0..count {
            let record_offset = records_start + (i as u64 * 12);
            if record_offset + 12 > reader.size() {
                break;
            }

            let record_data = reader.read(record_offset, 12)?;
            let rec_r = EndianReader::big_endian(record_data);
            records.push(NameRecord {
                platform_id: rec_r.u16_at(0).unwrap_or(0),
                encoding_id: rec_r.u16_at(2).unwrap_or(0),
                language_id: rec_r.u16_at(4).unwrap_or(0),
                name_id: rec_r.u16_at(6).unwrap_or(0),
                length: rec_r.u16_at(8).unwrap_or(0),
                offset: rec_r.u16_at(10).unwrap_or(0),
            });
        }

        Ok(records)
    }

    /// Decodes a string using the Macintosh Roman encoding.
    fn decode_mac_roman(data: &[u8]) -> String {
        const MAC_ROMAN_HIGH: [char; 128] = [
            'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è', 'ê',
            'ë', 'í', 'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü', '†', '°',
            '¢', '£', '§', '•', '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø', '∞', '±', '≤',
            '≥', '¥', 'µ', '∂', '∑', '∏', 'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø', '¿', '¡', '¬', '√',
            'ƒ', '≈', '∆', '«', '»', '…', '\u{a0}', 'À', 'Ã', 'Õ', 'Œ', 'œ', '–', '—', '“', '”',
            '‘', '’', '÷', '◊', 'ÿ', 'Ÿ', '⁄', '€', '‹', '›', 'ﬁ', 'ﬂ', '‡', '·', '‚', '„', '‰',
            'Â', 'Ê', 'Á', 'Ë', 'È', 'Í', 'Î', 'Ï', 'Ì', 'Ó', 'Ô', '\u{f8ff}', 'Ò', 'Ú', 'Û', 'Ù',
            'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚', '¸', '˝', '˛', 'ˇ',
        ];

        data.iter()
            .map(|&byte| {
                if byte < 0x80 {
                    char::from(byte)
                } else {
                    MAC_ROMAN_HIGH[(byte - 0x80) as usize]
                }
            })
            .collect()
    }

    /// Extracts a string from the name table
    fn extract_name_string(
        reader: &dyn FileReader,
        table: &TableEntry,
        record: &NameRecord,
        string_offset: u16,
    ) -> Result<Option<String>> {
        let str_start = table.offset as u64 + string_offset as u64 + record.offset as u64;
        let str_len = record.length as usize;

        if str_start + str_len as u64 > reader.size() || str_len == 0 {
            return Ok(None);
        }

        let str_data = reader.read(str_start, str_len)?;

        // Decode based on platform
        let decoded = match record.platform_id {
            PLATFORM_WINDOWS | PLATFORM_UNICODE => {
                // Windows and Unicode platform strings use UTF-16BE.
                if !str_len.is_multiple_of(2) {
                    return Ok(None);
                }
                let utf16_chars: Vec<u16> = str_data
                    .chunks_exact(2)
                    .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                    .collect();
                String::from_utf16(&utf16_chars).ok()
            }
            PLATFORM_MACINTOSH if record.encoding_id == 0 => {
                // Macintosh encoding 0 is Mac Roman, not UTF-8.
                Some(Self::decode_mac_roman(str_data))
            }
            PLATFORM_MACINTOSH => {
                // Preserve the previous behavior for unsupported Macintosh encodings.
                String::from_utf8(str_data.to_vec()).ok()
            }
            _ => String::from_utf8(str_data.to_vec()).ok(),
        };

        Ok(decoded)
    }

    /// Returns the ExifTool language suffix for supported localized name records.
    fn language_suffix(record: &NameRecord) -> Option<&'static str> {
        match (record.platform_id, record.language_id) {
            (PLATFORM_MACINTOSH, LANGUAGE_DANISH_MACINTOSH) => Some("da"),
            (PLATFORM_MACINTOSH, LANGUAGE_GERMAN_MACINTOSH) => Some("de"),
            (PLATFORM_MACINTOSH, LANGUAGE_HEBREW_MACINTOSH) => Some("he"),
            (PLATFORM_MACINTOSH, LANGUAGE_SPANISH_MACINTOSH) => Some("es"),
            (PLATFORM_MACINTOSH, LANGUAGE_FINNISH_MACINTOSH) => Some("fi"),
            (PLATFORM_MACINTOSH, LANGUAGE_FRENCH_MACINTOSH) => Some("fr"),
            (PLATFORM_MACINTOSH, LANGUAGE_ITALIAN_MACINTOSH) => Some("it"),
            (PLATFORM_MACINTOSH, LANGUAGE_DUTCH_MACINTOSH) => Some("nl-NL"),
            (PLATFORM_MACINTOSH, LANGUAGE_NORWEGIAN_MACINTOSH) => Some("no"),
            (PLATFORM_MACINTOSH, LANGUAGE_JAPANESE_MACINTOSH) => Some("ja"),
            (PLATFORM_MACINTOSH, LANGUAGE_KOREAN_MACINTOSH) => Some("ko"),
            (PLATFORM_WINDOWS | PLATFORM_UNICODE, lang_id) => match lang_id {
                LANGUAGE_DANISH_WINDOWS => Some("da"),
                LANGUAGE_GERMAN_WINDOWS => Some("de"),
                LANGUAGE_HEBREW_WINDOWS => Some("he"),
                LANGUAGE_SPANISH_WINDOWS => Some("es"),
                LANGUAGE_FINNISH_WINDOWS => Some("fi"),
                LANGUAGE_FRENCH_WINDOWS => Some("fr"),
                LANGUAGE_ITALIAN_WINDOWS => Some("it"),
                LANGUAGE_JAPANESE_WINDOWS => Some("ja"),
                LANGUAGE_KOREAN_WINDOWS => Some("ko"),
                LANGUAGE_DUTCH_WINDOWS => Some("nl-NL"),
                LANGUAGE_NORWEGIAN_WINDOWS => Some("no"),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether this record is the default English-language form of a name.
    fn is_default_language(record: &NameRecord) -> bool {
        match record.platform_id {
            PLATFORM_WINDOWS => record.language_id == LANGUAGE_ENGLISH_WINDOWS,
            PLATFORM_MACINTOSH | PLATFORM_UNICODE => record.language_id == 0,
            _ => false,
        }
    }

    /// Sort records by default language, then preferred platform.
    fn name_record_priority(record: &NameRecord) -> (u8, u8) {
        (
            u8::from(!Self::is_default_language(record)),
            match record.platform_id {
                PLATFORM_WINDOWS => 0,
                PLATFORM_UNICODE => 1,
                PLATFORM_MACINTOSH => 2,
                _ => 3,
            },
        )
    }

    /// Extracts metadata from name table
    fn extract_name_metadata(reader: &dyn FileReader, table: &TableEntry) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let offset = table.offset as u64;

        if offset + 6 > reader.size() {
            return Ok(metadata);
        }

        let header = reader.read(offset, 6)?;
        let r = EndianReader::big_endian(header);
        let string_offset = r.u16_at(4).unwrap_or(0);
        let records = Self::parse_name_table(reader, table)?;

        // Map of name IDs to metadata keys
        let name_mappings = [
            (NAME_COPYRIGHT, "Copyright"),
            (NAME_FONT_FAMILY, "FontFamily"),
            (NAME_FONT_SUBFAMILY, "FontSubfamily"),
            (NAME_FULL_FONT_NAME, "FontName"),
            (NAME_VERSION, "FontVersion"),
            (NAME_POSTSCRIPT_NAME, "PostScriptName"),
            (NAME_DESIGNER, "Designer"),
            (NAME_VENDOR_URL, "VendorURL"),
            (NAME_LICENSE, "License"),
        ];

        for (name_id, key) in &name_mappings {
            // Prefer Windows platform (3), then Unicode (0), then Mac (1)
            let record = records
                .iter()
                .filter(|r| r.name_id == *name_id)
                .min_by_key(|record| Self::name_record_priority(record));

            if let Some(rec) = record
                && let Ok(Some(value)) =
                    Self::extract_name_string(reader, table, rec, string_offset)
                && !value.is_empty()
            {
                let tag_value = TagValue::String(value);
                metadata.insert(key.to_string(), tag_value.clone());

                // ExifTool reports these name-table values in the shared
                // Font group as well.
                let font_key = match *name_id {
                    NAME_COPYRIGHT => Some("Font:Copyright"),
                    NAME_FONT_FAMILY => Some("Font:FontFamily"),
                    NAME_FULL_FONT_NAME => Some("Font:FontName"),
                    NAME_FONT_SUBFAMILY => Some("Font:FontSubfamily"),
                    _ => None,
                };
                if let Some(font_key) = font_key {
                    metadata.insert(font_key.to_string(), tag_value);
                }
            }
        }

        // ExifTool exposes localized name table records with a language suffix.
        for record in &records {
            let base_key = match record.name_id {
                NAME_FONT_FAMILY => Some("Font:FontFamily"),
                NAME_FULL_FONT_NAME => Some("Font:FontName"),
                NAME_FONT_SUBFAMILY => Some("Font:FontSubfamily"),
                _ => None,
            };
            let Some(base_key) = base_key else {
                continue;
            };
            let Some(language) = Self::language_suffix(record) else {
                continue;
            };
            match Self::extract_name_string(reader, table, record, string_offset) {
                Ok(Some(value)) if !value.is_empty() => {
                    metadata.insert(format!("{base_key}-{language}"), TagValue::String(value));
                }
                _ => {}
            }
        }

        // Preserve the localized Hebrew copyright record. The language ID
        // namespace depends on the name record's platform.
        for record in records.iter().filter(|record| {
            record.name_id == NAME_COPYRIGHT
                && ((record.platform_id == PLATFORM_MACINTOSH
                    && record.language_id == LANGUAGE_HEBREW_MACINTOSH)
                    || (record.platform_id == PLATFORM_WINDOWS
                        && record.language_id == LANGUAGE_HEBREW_WINDOWS))
        }) {
            if let Some(value) = Self::extract_name_string(reader, table, record, string_offset)?
                && !value.is_empty()
            {
                metadata.insert("Font:Copyright-he".to_string(), TagValue::String(value));
                break;
            }
        }

        Ok(metadata)
    }

    /// Converts Mac timestamp (seconds since 1904) to ISO 8601 string
    fn mac_timestamp_to_iso(timestamp: i64) -> Option<String> {
        // Mac epoch: January 1, 1904 00:00:00 UTC
        // Unix epoch: January 1, 1970 00:00:00 UTC
        // Difference: 2082844800 seconds
        const MAC_TO_UNIX_OFFSET: i64 = 2082844800;

        let unix_timestamp = timestamp.checked_sub(MAC_TO_UNIX_OFFSET)?;
        if unix_timestamp < 0 {
            return None;
        }

        // Basic ISO 8601 formatting
        const SECS_PER_DAY: i64 = 86400;
        const SECS_PER_HOUR: i64 = 3600;
        const SECS_PER_MINUTE: i64 = 60;

        let days = unix_timestamp / SECS_PER_DAY;
        let remaining = unix_timestamp % SECS_PER_DAY;
        let hours = remaining / SECS_PER_HOUR;
        let remaining = remaining % SECS_PER_HOUR;
        let minutes = remaining / SECS_PER_MINUTE;
        let seconds = remaining % SECS_PER_MINUTE;

        // Simplified date calculation (approximate)
        let year = 1970 + (days / 365);
        let day_of_year = days % 365;
        let month = (day_of_year / 30) + 1;
        let day = (day_of_year % 30) + 1;

        Some(format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            year, month, day, hours, minutes, seconds
        ))
    }

    /// Extracts metadata from head table
    fn extract_head_metadata(reader: &dyn FileReader, table: &TableEntry) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let offset = table.offset as u64;

        if offset + 54 > reader.size() {
            return Ok(metadata);
        }

        // Read units per em (offset 18)
        let units_data = reader.read(offset + 18, 2)?;
        let r = EndianReader::big_endian(units_data);
        let units_per_em = r.u16_at(0).unwrap_or(0);
        metadata.insert(
            "UnitsPerEm".to_string(),
            TagValue::String(units_per_em.to_string()),
        );

        // Read created timestamp (offset 20, 8 bytes)
        if offset + 28 <= reader.size() {
            let created_data = reader.read(offset + 20, 8)?;
            let created_r = EndianReader::big_endian(created_data);
            if let Some(created) = created_r.i64_at(0)
                && let Some(created_str) = Self::mac_timestamp_to_iso(created)
            {
                metadata.insert("FontCreated".to_string(), TagValue::String(created_str));
            }
        }

        // Read modified timestamp (offset 28, 8 bytes)
        if offset + 36 <= reader.size() {
            let modified_data = reader.read(offset + 28, 8)?;
            let modified_r = EndianReader::big_endian(modified_data);
            if let Some(modified) = modified_r.i64_at(0)
                && let Some(modified_str) = Self::mac_timestamp_to_iso(modified)
            {
                metadata.insert("FontModified".to_string(), TagValue::String(modified_str));
            }
        }

        Ok(metadata)
    }
}

impl FormatParser for TTFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid TTF signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("TTF".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        let num_tables = Self::read_num_tables(reader)?;
        metadata.insert(
            "NumTables".to_string(),
            TagValue::String(num_tables.to_string()),
        );

        // Parse table directory
        let tables = Self::parse_table_directory(reader, num_tables)?;

        // Extract metadata from name table
        if let Some(name_table) = Self::find_table(&tables, b"name") {
            let name_metadata = Self::extract_name_metadata(reader, name_table)?;
            for (key, value) in name_metadata {
                metadata.insert(key, value);
            }
        }

        // Extract metadata from head table
        if let Some(head_table) = Self::find_table(&tables, b"head") {
            let head_metadata = Self::extract_head_metadata(reader, head_table)?;
            for (key, value) in head_metadata {
                metadata.insert(key, value);
            }
        }

        // Add TTF-specific tag aliases for Worker 21 requirements
        add_ttf_tag_aliases(&mut metadata);

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::TTF)
    }
}

/// Parses metadata from TTF files.
///
/// This is a convenience wrapper around TTFParser that provides a functional API.
pub fn parse_ttf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = TTFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Adds TTF-specific tag aliases to metadata (Worker 21 requirements)
///
/// Maps generic font metadata to TTF-specific tags for ExifTool compatibility
/// Worker 21 requires: TTF:FontName, TTF:FamilyName, TTF:StyleName,
/// TTF:UnitsPerEm, TTF:XMin, TTF:YMin, TTF:XMax, TTF:YMax
fn add_ttf_tag_aliases(metadata: &mut MetadataMap) {
    // Create aliases with TTF prefix
    let mappings = [
        ("FontName", "TTF:FontName"),
        ("FontFamily", "TTF:FamilyName"),
        ("FontSubfamily", "TTF:StyleName"),
        ("UnitsPerEm", "TTF:UnitsPerEm"),
    ];

    let mut ttf_tags = Vec::new();
    for (source, ttf_tag) in &mappings {
        if let Some(value) = metadata.get(source) {
            ttf_tags.push((ttf_tag.to_string(), value.clone()));
        }
    }

    for (key, value) in ttf_tags {
        metadata.insert(key, value);
    }

    // Note: XMin, YMin, XMax, YMax would require parsing the glyf table
    // which is beyond the current scope. These would need to be extracted
    // from the glyf table bounding boxes.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// A Macintosh name record carrying `language_id`. Only the platform and
    /// language fields participate in language_suffix; the rest are filler.
    fn mac_record(language_id: u16) -> NameRecord {
        NameRecord {
            platform_id: PLATFORM_MACINTOSH,
            encoding_id: 0,
            language_id,
            name_id: 0,
            length: 0,
            offset: 0,
        }
    }

    /// Pins every Macintosh language ID to ExifTool's `%ttLang{Macintosh}`
    /// table (Font.pm). Transcribed from that table, not from memory.
    ///
    /// This exists because a green recheck does not prove these are right:
    /// a sample only exercises the IDs it actually contains, so a wrong
    /// constant is invisible unless the sample happens to hold a record
    /// with that ID. Font.ttf holds Dutch (4) but constant-set Italian to
    /// 4 as well, which silently routed Dutch text into FontSubfamily-it.
    #[test]
    fn macintosh_language_ids_match_exiftool_ttlang_table() {
        // Literal IDs on purpose. Writing LANGUAGE_SPANISH_MACINTOSH here
        // instead of 6 would make this a tautology -- it would feed the
        // constant in and assert the constant's own meaning back out, and
        // pass for any value it held. The literals ARE the table.
        for (id, expected) in [
            (1, "fr"),  // %ttLang{Macintosh}: 1 => 'fr'
            (2, "de"),  // 2 => 'de'
            (3, "it"),  // 3 => 'it'   (4 is nl-NL, not Italian)
            (6, "es"),  // 6 => 'es'   (12 is ar, not Spanish)
            (7, "da"),  // 7 => 'da'
            (10, "he"), // 10 => 'he'
            (13, "fi"), // 13 => 'fi'
            (4, "nl-NL"), // 4 => 'nl-NL' (now claimed; was incorrectly unclaimed)
            (9, "no"),    // 9 => 'no'
            (11, "ja"),   // 11 => 'ja'
            (23, "ko"),   // 23 => 'ko'
        ] {
            let record = mac_record(id);
            assert_eq!(
                TTFParser::language_suffix(&record),
                Some(expected),
                "Macintosh language ID {id} must map to {expected:?} per ExifTool %ttLang",
            );
        }
    }

    /// The IDs ExifTool assigns to languages this parser does NOT claim.
    /// Guards the specific failure mode above: mapping one of these to a
    /// language we do support would emit that record's text under the
    /// wrong suffix rather than simply leaving a gap open.
    #[test]
    fn unclaimed_macintosh_language_ids_stay_unmapped() {
        for id in [
            12, /* ar */
            5,  /* sv */
            8,  /* pt */
        ] {
            let record = mac_record(id);
            assert_eq!(
                TTFParser::language_suffix(&record),
                None,
                "Macintosh language ID {id} is not a language this parser claims; \
                 mapping it would mislabel that record's text",
            );
        }
    }

    #[test]
    fn test_ttf_signature_v1() {
        let data = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x10];
        let reader = TestReader::new(data);
        assert!(TTFParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_ttf_signature_true() {
        let mut data = b"true".to_vec();
        data.extend_from_slice(&[0x00, 0x10]);
        let reader = TestReader::new(data);
        assert!(TTFParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_parse_basic_metadata() {
        // Create minimal TTF with offset table and one table
        let mut data = vec![
            0x00, 0x01, 0x00, 0x00, // sfnt version
            0x00, 0x02, // numTables = 2
            0x00, 0x10, // searchRange
            0x00, 0x00, // entrySelector
            0x00, 0x00, // rangeShift
            // Table directory entry 1 (name table)
            b'n', b'a', b'm', b'e', // tag
            0x00, 0x00, 0x00, 0x00, // checksum
            0x00, 0x00, 0x00, 0x2C, // offset = 44
            0x00, 0x00, 0x00, 0x1A, // length = 26
            // Table directory entry 2 (head table)
            b'h', b'e', b'a', b'd', // tag
            0x00, 0x00, 0x00, 0x00, // checksum
            0x00, 0x00, 0x00, 0x46, // offset = 70 (44 + 26)
            0x00, 0x00, 0x00, 0x36, // length = 54
        ];

        // Name table data at offset 44
        data.extend_from_slice(&[
            0x00, 0x00, // format = 0
            0x00, 0x01, // count = 1
            0x00, 0x12, // stringOffset = 18
            // Name record
            0x00, 0x03, // platformID = 3 (Windows)
            0x00, 0x01, // encodingID = 1
            0x00, 0x09, // languageID = 9 (English)
            0x00, 0x01, // nameID = 1 (Font Family)
            0x00, 0x08, // length = 8
            0x00, 0x00, // offset = 0
            // String storage: "Test" in UTF-16BE
            0x00, b'T', 0x00, b'e', 0x00, b's', 0x00, b't',
        ]);

        // Head table data at offset 70
        // Structure: version(4), fontRevision(4), checksumAdjustment(4), magicNumber(4),
        //            flags(2), unitsPerEm(2), created(8), modified(8), bbox(8), macStyle(2),
        //            lowestRecPPEM(2), fontDirectionHint(2), indexToLocFormat(2), glyphDataFormat(2)
        data.extend_from_slice(&[
            0x00, 0x01, 0x00, 0x00, // offset 0: version = 1.0
            0x00, 0x00, 0x00, 0x00, // offset 4: fontRevision
            0x00, 0x00, 0x00, 0x00, // offset 8: checksumAdjustment
            0x5F, 0x0F, 0x3C, 0xF5, // offset 12: magicNumber
            0x00, 0x00, // offset 16: flags
            0x08, 0x00, // offset 18: unitsPerEm = 2048
            0x00, 0x00, 0x00, 0x00, // offset 20: created (high)
            0xD4, 0x36, 0x5E, 0x80, // offset 24: created (low)
            0x00, 0x00, 0x00, 0x00, // offset 28: modified (high)
            0xD4, 0x36, 0x5E, 0x80, // offset 32: modified (low)
            0x00, 0x00, // offset 36: xMin
            0x00, 0x00, // offset 38: yMin
            0x00, 0x00, // offset 40: xMax
            0x00, 0x00, // offset 42: yMax
            0x00, 0x00, // offset 44: macStyle
            0x00, 0x08, // offset 46: lowestRecPPEM
            0x00, 0x00, // offset 48: fontDirectionHint
            0x00, 0x00, // offset 50: indexToLocFormat
            0x00, 0x00, // offset 52: glyphDataFormat
        ]);

        let reader = TestReader::new(data);
        let parser = TTFParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("TTF".to_string()))
        );
        assert_eq!(
            metadata.get("NumTables"),
            Some(&TagValue::String("2".to_string()))
        );
        assert_eq!(
            metadata.get("UnitsPerEm"),
            Some(&TagValue::String("2048".to_string()))
        );
        assert_eq!(
            metadata.get("FontFamily"),
            Some(&TagValue::String("Test".to_string()))
        );
        assert_eq!(
            metadata.get("Font:FontFamily"),
            Some(&TagValue::String("Test".to_string()))
        );
        assert!(metadata.contains_key("FontCreated"));
        assert!(metadata.contains_key("FontModified"));
    }

    #[test]
    fn test_mac_roman_font_copyright() {
        let copyright = [
            0xa9, b' ', b'A', b'p', b'p', b'l', b'e', b' ', b'C', b'o', b'm', b'p', b'u', b't',
            b'e', b'r', b',', b' ', b'I', b'n', b'c', b'.', b' ', b'1', b'9', b'9', b'1', b'-',
            b'1', b'9', b'9', b'5',
        ];

        let mut data = vec![
            b't', b'r', b'u', b'e', // sfnt version
            0x00, 0x01, // numTables = 1
            0x00, 0x00, // searchRange
            0x00, 0x00, // entrySelector
            0x00, 0x00, // rangeShift
            b'n', b'a', b'm', b'e', // table tag
            0x00, 0x00, 0x00, 0x00, // checksum
            0x00, 0x00, 0x00, 0x1c, // table offset = 28
            0x00, 0x00, 0x00, 0x32, // table length = 50
            0x00, 0x00, // name table format
            0x00, 0x01, // record count
            0x00, 0x12, // string storage offset = 18
            0x00, 0x01, // platform ID = Macintosh
            0x00, 0x00, // encoding ID = Roman
            0x00, 0x00, // language ID
            0x00, 0x00, // name ID = Copyright
            0x00, 0x20, // string length = 32
            0x00, 0x00, // string offset
        ];
        data.extend_from_slice(&copyright);

        let reader = TestReader::new(data);
        let metadata = TTFParser.parse(&reader).unwrap();
        let expected = TagValue::String("© Apple Computer, Inc. 1991-1995".to_string());

        assert_eq!(metadata.get("Copyright"), Some(&expected));
        assert_eq!(metadata.get("Font:Copyright"), Some(&expected));
    }

    #[test]
    fn test_mac_timestamp_conversion() {
        // Test timestamp conversion
        // Mac epoch: 1904-01-01, Unix epoch: 1970-01-01
        // Difference: 2082844800 seconds
        let mac_timestamp = 2082844800i64; // Should be 1970-01-01 00:00:00
        let result = TTFParser::mac_timestamp_to_iso(mac_timestamp);
        assert!(result.is_some());
        let timestamp_str = result.unwrap();
        assert!(timestamp_str.starts_with("1970"));
    }
}
