//! TrueType Font (TTF) format parser
//!
//! Implements comprehensive metadata extraction from TrueType font files,
//! including name table records, timestamps, and font properties.
//!
//! TTF files use big-endian byte order for all multi-byte fields.

#![allow(dead_code)]

use super::{generated_languages, mac_charset};
use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use std::collections::HashMap;

/// TTF signature: 0x00 0x01 0x00 0x00 or "true"
const TTF_SIGNATURE_1: &[u8] = &[0x00, 0x01, 0x00, 0x00];
const TTF_SIGNATURE_2: &[u8] = b"true";

/// Platform IDs for name table records
const PLATFORM_UNICODE: u16 = 0;
const PLATFORM_MACINTOSH: u16 = 1;
const PLATFORM_ISO: u16 = 2;
const PLATFORM_WINDOWS: u16 = 3;
const PLATFORM_CUSTOM: u16 = 4;

/// The legacy best-record selector still treats Windows 0x0409 as English.
const LANGUAGE_ENGLISH_WINDOWS: u16 = 0x0409;

/// Macintosh encoding (script) IDs from ExifTool's `%ttCharset{Macintosh}`
/// (Font.pm). Only the two decoded inline are named here; the four CJK
/// scripts are dispatched by ID in `mac_charset::for_mac_encoding`, which
/// carries ExifTool's own tables.
const MAC_ENCODING_ROMAN: u16 = 0;
const MAC_ENCODING_HEBREW: u16 = 5;

/// Name IDs for name table records. Names match ExifTool's
/// `%Image::ExifTool::Font::Name` table (Font.pm), which is what determines
/// the emitted tag name.
const NAME_COPYRIGHT: u16 = 0;
const NAME_FONT_FAMILY: u16 = 1;
const NAME_FONT_SUBFAMILY: u16 = 2;
const NAME_FONT_SUBFAMILY_ID: u16 = 3;
const NAME_FULL_FONT_NAME: u16 = 4;
const NAME_VERSION: u16 = 5;
const NAME_POSTSCRIPT_NAME: u16 = 6;
const NAME_TRADEMARK: u16 = 7;
const NAME_MANUFACTURER: u16 = 8;
const NAME_DESIGNER: u16 = 9;
const NAME_DESCRIPTION: u16 = 10;
const NAME_VENDOR_URL: u16 = 11;
const NAME_DESIGNER_URL: u16 = 12;
const NAME_LICENSE: u16 = 13;
const NAME_LICENSE_INFO_URL: u16 = 14;
const NAME_PREFERRED_FAMILY: u16 = 16;
const NAME_PREFERRED_SUBFAMILY: u16 = 17;
const NAME_COMPATIBLE_FONT_NAME: u16 = 18;
const NAME_SAMPLE_TEXT: u16 = 19;
const NAME_POSTSCRIPT_NAME_20: u16 = 20;
const NAME_WWS_FAMILY_NAME: u16 = 21;
const NAME_WWS_SUBFAMILY_NAME: u16 = 22;

/// Table directory entry
#[derive(Debug, Clone)]
pub(crate) struct TableEntry {
    tag: [u8; 4],
    offset: u32,
    length: u32,
}

/// Whether the supported source charset has UCS2's BMP-only boundary.
#[derive(Clone, Copy)]
enum FontValueCharset {
    /// Font.pm's UCS2 is equivalent to this reader only for BMP text.
    Ucs2BmpOnly,
    /// A supported Macintosh source table or Unicode UTF16.
    FullyDecoded,
}

/// How a name record's language maps onto ExifTool's tag naming -- see
/// [`TTFParser::name_record_lang`].
enum NameLang<'a> {
    /// `$lang` is `'en'` or undefined: the tag name takes no suffix.
    Unsuffixed,
    /// A source language code or a valid format-1 tag: `Tag-<code>`.
    Suffixed(&'a str),
    /// The format-1 language tag is absent or the value decoder is outside
    /// this parser's supported platforms.
    Omitted,
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
    pub(crate) fn parse_table_directory(
        reader: &dyn FileReader,
        num_tables: u16,
    ) -> Result<Vec<TableEntry>> {
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
    pub(crate) fn find_table<'a>(
        tables: &'a [TableEntry],
        tag: &[u8; 4],
    ) -> Option<&'a TableEntry> {
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
    pub(crate) fn decode_mac_roman(data: &[u8]) -> String {
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

    /// Decodes Mac OS Hebrew (`%ttCharset{Macintosh}` encoding 5, "MacHebrew").
    ///
    /// Bytes below 0x80 are Unicode-identical; the 0x80..=0xFF half is
    /// transcribed verbatim from ExifTool 13.55's
    /// `Image/ExifTool/Charset/MacHebrew.pm`, which itself derives from
    /// unicode.org's APPLE/HEBREW.TXT. Entries are `&str` rather than `char`
    /// because three of them expand to multiple code points in that table
    /// (0x81 => [0x05f2,0x05b7], 0xc0 => [0xf86a,0x05dc,0x05b9],
    /// 0xde => [0x05b8,0xf87f]).
    ///
    /// This is not MacRoman with Hebrew letters appended: MacHebrew re-maps
    /// the whole 0xA0..0xBF band to ASCII punctuation and digits (0xA0 is
    /// SPACE, not U+00B0 DEGREE; 0xA6 is U+20AA NEW SHEQEL, not PILCROW),
    /// and swaps the paired delimiters at 0xFB..0xFF. Deriving it from the
    /// MacRoman table produces plausible-looking mojibake, so it is carried
    /// straight across from the .pm.
    fn decode_mac_hebrew(data: &[u8]) -> String {
        #[rustfmt::skip]
        const MAC_HEBREW_HIGH: [&str; 128] = [
            // 0x80
            "\u{00c4}", "\u{05f2}\u{05b7}", "\u{00c7}", "\u{00c9}",
            "\u{00d1}", "\u{00d6}", "\u{00dc}", "\u{00e1}",
            // 0x88
            "\u{00e0}", "\u{00e2}", "\u{00e4}", "\u{00e3}",
            "\u{00e5}", "\u{00e7}", "\u{00e9}", "\u{00e8}",
            // 0x90
            "\u{00ea}", "\u{00eb}", "\u{00ed}", "\u{00ec}",
            "\u{00ee}", "\u{00ef}", "\u{00f1}", "\u{00f3}",
            // 0x98
            "\u{00f2}", "\u{00f4}", "\u{00f6}", "\u{00f5}",
            "\u{00fa}", "\u{00f9}", "\u{00fb}", "\u{00fc}",
            // 0xA0
            "\u{0020}", "\u{0021}", "\u{0022}", "\u{0023}",
            "\u{0024}", "\u{0025}", "\u{20aa}", "\u{0027}",
            // 0xA8
            "\u{0029}", "\u{0028}", "\u{002a}", "\u{002b}",
            "\u{002c}", "\u{002d}", "\u{002e}", "\u{002f}",
            // 0xB0
            "\u{0030}", "\u{0031}", "\u{0032}", "\u{0033}",
            "\u{0034}", "\u{0035}", "\u{0036}", "\u{0037}",
            // 0xB8
            "\u{0038}", "\u{0039}", "\u{003a}", "\u{003b}",
            "\u{003c}", "\u{003d}", "\u{003e}", "\u{003f}",
            // 0xC0
            "\u{f86a}\u{05dc}\u{05b9}", "\u{201e}", "\u{f89b}", "\u{f89c}",
            "\u{f89d}", "\u{f89e}", "\u{05bc}", "\u{fb4b}",
            // 0xC8
            "\u{fb35}", "\u{2026}", "\u{00a0}", "\u{05b8}",
            "\u{05b7}", "\u{05b5}", "\u{05b6}", "\u{05b4}",
            // 0xD0
            "\u{2013}", "\u{2014}", "\u{201c}", "\u{201d}",
            "\u{2018}", "\u{2019}", "\u{fb2a}", "\u{fb2b}",
            // 0xD8
            "\u{05bf}", "\u{05b0}", "\u{05b2}", "\u{05b1}",
            "\u{05bb}", "\u{05b9}", "\u{05b8}\u{f87f}", "\u{05b3}",
            // 0xE0
            "\u{05d0}", "\u{05d1}", "\u{05d2}", "\u{05d3}",
            "\u{05d4}", "\u{05d5}", "\u{05d6}", "\u{05d7}",
            // 0xE8
            "\u{05d8}", "\u{05d9}", "\u{05da}", "\u{05db}",
            "\u{05dc}", "\u{05dd}", "\u{05de}", "\u{05df}",
            // 0xF0
            "\u{05e0}", "\u{05e1}", "\u{05e2}", "\u{05e3}",
            "\u{05e4}", "\u{05e5}", "\u{05e6}", "\u{05e7}",
            // 0xF8
            "\u{05e8}", "\u{05e9}", "\u{05ea}", "\u{007d}",
            "\u{005d}", "\u{007b}", "\u{005b}", "\u{007c}",
        ];

        let mut out = String::with_capacity(data.len());
        for &byte in data {
            if byte < 0x80 {
                out.push(char::from(byte));
            } else {
                out.push_str(MAC_HEBREW_HIGH[(byte - 0x80) as usize]);
            }
        }
        out
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
            PLATFORM_MACINTOSH if record.encoding_id == MAC_ENCODING_ROMAN => {
                // Macintosh encoding 0 is Mac Roman, not UTF-8.
                Some(Self::decode_mac_roman(str_data))
            }
            PLATFORM_MACINTOSH if record.encoding_id == MAC_ENCODING_HEBREW => {
                Some(Self::decode_mac_hebrew(str_data))
            }
            PLATFORM_MACINTOSH => {
                // The Macintosh CJK scripts are NOT the standard
                // Shift_JIS/EUC-KR/Big5/GBK codecs -- decoding them with
                // encoding_rs would emit text that differs from ExifTool on
                // hundreds of sequences per script (see `mac_charset` for the
                // measured divergence). `mac_charset` therefore carries
                // ExifTool's own Charset/Mac*.pm tables verbatim, and covers
                // MacJapanese, MacChineseTW, MacKorean and MacChineseCN.
                //
                // The remaining Macintosh scripts (MacArabic, MacThai,
                // MacDevanagari, ...) are still an open gap: ExifTool ships
                // tables for some of them, but none appear in the corpus, so
                // they stay on the previous best-effort path rather than
                // being carried untested. A wrong value is worse than an open
                // gap, so they are only emitted when the bytes happen to be
                // valid UTF-8.
                match mac_charset::for_mac_encoding(record.encoding_id) {
                    Some(charset) => Some(mac_charset::decode(str_data, charset)),
                    None => String::from_utf8(str_data.to_vec()).ok(),
                }
            }
            _ => String::from_utf8(str_data.to_vec()).ok(),
        };

        Ok(decoded)
    }

    /// Decode a Font.pm UCS2/UTF16 string after its BOM and NUL rules.
    /// Format-1 language tags need the source's loss-tolerant replacement;
    /// name values use `decode_font_name_bytes` so Perl's malformed raw
    /// bytes survive until the output mode chooses how to render them.
    fn decode_font_utf16(data: &[u8], lossy: bool) -> Option<String> {
        let (body, little_endian) = match data.get(..2) {
            Some([0xfe, 0xff]) => (&data[2..], false),
            Some([0xff, 0xfe]) => (&data[2..], true),
            _ => (data, false),
        };
        if !body.len().is_multiple_of(2) {
            return None;
        }
        let words: Vec<u16> = body
            .chunks_exact(2)
            .map(|pair| {
                if little_endian {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        let mut decoded = if lossy {
            String::from_utf16_lossy(&words)
        } else {
            String::from_utf16(&words).ok()?
        };
        if let Some(nul) = decoded.find('\0') {
            decoded.truncate(nul);
        }
        Some(decoded)
    }

    /// Charset.pm's fixed-width unpack followed by `pack('C0U*')`: UCS2
    /// leaves each surrogate as a code point, while UTF16 combines pairs.
    /// Perl's unpack ignores an incomplete final word. Recompose stops at
    /// the first NUL code point, after consuming any BOM.
    fn decode_font_name_bytes(data: &[u8], utf16: bool) -> Vec<u8> {
        let (body, little_endian) = match data.get(..2) {
            Some([0xfe, 0xff]) => (&data[2..], false),
            Some([0xff, 0xfe]) => (&data[2..], true),
            _ => (data, false),
        };
        let words: Vec<u16> = body
            .chunks_exact(2)
            .map(|pair| {
                if little_endian {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        let mut out = Vec::with_capacity(words.len() * 3);
        let mut index = 0;
        while index < words.len() {
            let word = words[index];
            if word == 0 {
                break;
            }
            let point = if utf16
                && (0xd800..=0xdbff).contains(&word)
                && words
                    .get(index + 1)
                    .is_some_and(|next| (0xdc00..=0xdfff).contains(next))
            {
                index += 1;
                0x10000 + ((u32::from(word) - 0xd800) << 10) + u32::from(words[index] - 0xdc00)
            } else {
                u32::from(word)
            };
            // `char::from_u32` excludes surrogates. Perl's pack does not.
            if let Some(ch) = char::from_u32(point) {
                let mut buffer = [0; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
            } else {
                out.extend_from_slice(&[
                    0xe0 | ((point >> 12) as u8),
                    0x80 | (((point >> 6) & 0x3f) as u8),
                    0x80 | ((point & 0x3f) as u8),
                ]);
            }
            index += 1;
        }
        out
    }

    /// Decode a `Font:` value with Font.pm's charset boundary. The older
    /// bare-name extractor above intentionally keeps its existing behavior.
    fn extract_font_name_string(
        reader: &dyn FileReader,
        table: &TableEntry,
        record: &NameRecord,
        string_offset: u16,
        charset: FontValueCharset,
    ) -> Result<Option<TagValue>> {
        let start = table.offset as u64 + u64::from(string_offset) + u64::from(record.offset);
        let data = reader.read(start, record.length as usize)?;
        if matches!(record.platform_id, PLATFORM_WINDOWS | PLATFORM_UNICODE) {
            return Ok(Some(TagValue::new_text_bytes(
                Self::decode_font_name_bytes(
                    data,
                    matches!(charset, FontValueCharset::FullyDecoded),
                ),
            )));
        }
        let mut decoded = match record.platform_id {
            PLATFORM_MACINTOSH => match record.encoding_id {
                MAC_ENCODING_ROMAN => Some(Self::decode_mac_roman(data)),
                MAC_ENCODING_HEBREW => Some(Self::decode_mac_hebrew(data)),
                encoding => mac_charset::for_mac_encoding(encoding)
                    .map(|charset| mac_charset::decode(data, charset)),
            },
            _ => None,
        };
        if let Some(value) = decoded.as_mut() {
            if record.platform_id == PLATFORM_MACINTOSH
                && record.encoding_id != 1
                && data.iter().all(|byte| *byte < 0x80)
            {
                // ExifTool::Decode skips conversion in this case. The final
                // tag value drops NULs but keeps later ASCII characters.
                value.retain(|ch| ch != '\0');
            } else if let Some(nul) = value.find('\0') {
                // Recompose truncates a converted value at its first NUL.
                value.truncate(nul);
            }
        }
        Ok(decoded.map(TagValue::String))
    }

    /// The exact source-defined suffix for this platform and language ID.
    fn language_suffix(record: &NameRecord) -> Option<&'static str> {
        generated_languages::font_language(record.platform_id, record.language_id)
    }

    /// Maps a name-table record's name ID to the key ExifTool reports it
    /// under in the `Font` group.
    ///
    /// Names are taken from `%Image::ExifTool::Font::Name` in Font.pm, which
    /// is the table `ProcessTTF` looks the nameID up in. Note that three of
    /// them do NOT match this parser's older un-prefixed keys: name ID 5 is
    /// `NameTableVersion` (not "FontVersion") and name ID 6 is
    /// `PostScriptFontName` (not "PostScriptName"). The legacy keys are left
    /// in place for the existing TTF: aliases; this is the ExifTool-facing
    /// spelling.
    fn font_group_key(name_id: u16) -> Option<&'static str> {
        // The full `%Image::ExifTool::Font::Name` table (Font.pm:246-282).
        // Every string is quoted from that table; 15 is absent there and so
        // stays absent here. IDs 6 and 20 both carry `PostScriptFontName` in
        // the Perl, so they share one key.
        match name_id {
            NAME_COPYRIGHT => Some("Font:Copyright"),
            NAME_FONT_FAMILY => Some("Font:FontFamily"),
            NAME_FONT_SUBFAMILY => Some("Font:FontSubfamily"),
            NAME_FONT_SUBFAMILY_ID => Some("Font:FontSubfamilyID"),
            NAME_FULL_FONT_NAME => Some("Font:FontName"),
            NAME_VERSION => Some("Font:NameTableVersion"),
            NAME_POSTSCRIPT_NAME | NAME_POSTSCRIPT_NAME_20 => Some("Font:PostScriptFontName"),
            NAME_TRADEMARK => Some("Font:Trademark"),
            NAME_MANUFACTURER => Some("Font:Manufacturer"),
            NAME_DESIGNER => Some("Font:Designer"),
            NAME_DESCRIPTION => Some("Font:Description"),
            NAME_VENDOR_URL => Some("Font:VendorURL"),
            NAME_DESIGNER_URL => Some("Font:DesignerURL"),
            NAME_LICENSE => Some("Font:License"),
            NAME_LICENSE_INFO_URL => Some("Font:LicenseInfoURL"),
            NAME_PREFERRED_FAMILY => Some("Font:PreferredFamily"),
            NAME_PREFERRED_SUBFAMILY => Some("Font:PreferredSubfamily"),
            NAME_COMPATIBLE_FONT_NAME => Some("Font:CompatibleFontName"),
            NAME_SAMPLE_TEXT => Some("Font:SampleText"),
            NAME_WWS_FAMILY_NAME => Some("Font:WWSFamilyName"),
            NAME_WWS_SUBFAMILY_NAME => Some("Font:WWSSubfamilyName"),
            _ => None,
        }
    }

    /// Match Font.pm's `%ttLang{$sys}{$langID} || %langTag{$langID}`.
    /// Unknown source IDs stay unsuffixed. A missing format-1 tag is omitted
    /// rather than guessed; every source-defined platform uses this naming
    /// path, while unsupported text decoding is refused separately.
    fn name_record_lang_with_fallback<'a>(
        record: &NameRecord,
        format_one_language: Option<&'a str>,
    ) -> NameLang<'a> {
        if !matches!(
            record.platform_id,
            PLATFORM_MACINTOSH
                | PLATFORM_WINDOWS
                | PLATFORM_UNICODE
                | PLATFORM_ISO
                | PLATFORM_CUSTOM
        ) {
            return NameLang::Omitted;
        }
        let source = Self::language_suffix(record)
            .or(format_one_language.filter(|language| !language.is_empty()));
        if record.language_id >= 0x8000 && source.is_none() && format_one_language.is_none() {
            return NameLang::Omitted;
        }
        match source {
            Some("en") | None => NameLang::Unsuffixed,
            Some(suffix) => NameLang::Suffixed(suffix),
        }
    }

    fn name_record_lang(record: &NameRecord) -> NameLang<'static> {
        Self::name_record_lang_with_fallback(record, None)
    }

    /// Which Font.pm name charset this reader can reproduce for a `Font:`
    /// value. `%ttLang` is independent of `%ttCharset`: adding a language
    /// name must not turn an unsupported script into a guessed text value.
    fn font_value_charset(record: &NameRecord) -> Option<FontValueCharset> {
        match (record.platform_id, record.encoding_id) {
            (PLATFORM_MACINTOSH, MAC_ENCODING_ROMAN | MAC_ENCODING_HEBREW) => {
                Some(FontValueCharset::FullyDecoded)
            }
            (PLATFORM_MACINTOSH, encoding) if mac_charset::for_mac_encoding(encoding).is_some() => {
                Some(FontValueCharset::FullyDecoded)
            }
            (PLATFORM_WINDOWS, 1) | (PLATFORM_UNICODE, 0..=3) => {
                Some(FontValueCharset::Ucs2BmpOnly)
            }
            (PLATFORM_UNICODE, 4) => Some(FontValueCharset::FullyDecoded),
            _ => None,
        }
    }

    /// Font.pm's `Decode` leaves raw ASCII alone when the source charset is
    /// absent or does not require remapping low bytes. The excluded Windows
    /// charsets are Symbol, UCS2, ShiftJIS and UCS4; ISO UCS2 is fixed-width,
    /// and MacJapanese remaps ASCII.
    fn font_ascii_passthrough(record: &NameRecord) -> bool {
        match (record.platform_id, record.encoding_id) {
            (PLATFORM_MACINTOSH, 1) => false,
            (PLATFORM_MACINTOSH, _) => true,
            (PLATFORM_WINDOWS, 0 | 1 | 2 | 10) => false,
            (PLATFORM_WINDOWS, _) => true,
            (PLATFORM_UNICODE, 0..=4) | (PLATFORM_ISO, 1) => false,
            (PLATFORM_UNICODE | PLATFORM_ISO | PLATFORM_CUSTOM, _) => true,
            _ => false,
        }
    }

    /// Parse format-1 language tags with Font.pm's bounds, decode and filter.
    fn format_one_language_tags(
        reader: &dyn FileReader,
        table: &TableEntry,
    ) -> Result<HashMap<u16, String>> {
        let mut tags = HashMap::new();
        let table_start = table.offset as u64;
        let size = table.length as u64;
        if size < 6 || table_start + size > reader.size() {
            return Ok(tags);
        }
        let header = reader.read(table_start, 6)?;
        let r = EndianReader::big_endian(header);
        if r.u16_at(0) != Some(1) {
            return Ok(tags);
        }
        let entries = r.u16_at(2).unwrap_or(0) as u64;
        let str_start = r.u16_at(4).unwrap_or(0) as u64;
        let rec_end = 6 + entries * 12;
        if rec_end > size || str_start < rec_end || str_start > size || rec_end + 2 > size {
            return Ok(tags);
        }
        let lang_count = reader.read(table_start + rec_end, 2)?;
        let count = u16::from_be_bytes([lang_count[0], lang_count[1]]) as u64;
        if count == 0 || rec_end + 2 + count * 4 >= size {
            return Ok(tags);
        }
        for index in 0..count {
            if index > 0x7fff {
                break;
            }
            let record = reader.read(table_start + rec_end + 2 + index * 4, 4)?;
            let len = u16::from_be_bytes([record[0], record[1]]) as u64;
            let offset = u16::from_be_bytes([record[2], record[3]]) as u64;
            if len == 0 || len % 2 != 0 || len > 40 || str_start + offset + len > size {
                break;
            }
            let data = reader.read(table_start + str_start + offset, len as usize)?;
            // Font.pm's UCS2 decoder consumes a BOM, truncates at NUL, and
            // replaces invalid surrogates without stopping later records.
            // The ASCII filter removes replacement characters.
            let Some(decoded) = Self::decode_font_utf16(data, true) else {
                break;
            };
            let filtered: String = decoded
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
                .collect();
            tags.insert(0x8000 + index as u16, filtered);
        }
        Ok(tags)
    }

    /// The name-table walk exactly as `ProcessTableEntry` performs it
    /// (Font.pm:452-538): one tag per record, in record order, later records
    /// replacing earlier ones with the same name -- `FoundTag` keeps the new
    /// value whenever `$priority >= $oldPriority` (ExifTool.pm:9564-9586:
    /// the old tag is moved aside to `"$tag (1)"` and the bare name takes
    /// the new value), and name records all carry the same default
    /// priority.
    ///
    /// This is the path shared with DFONT: RSRC.pm:38-41 routes a `sfnt`
    /// resource's name table through the same `Font::Name` table, so the
    /// resource-fork parser calls this against the embedded sfnt block.
    pub(crate) fn extract_exiftool_name_tags(
        reader: &dyn FileReader,
        table: &TableEntry,
    ) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let offset = table.offset as u64;
        let table_size = u64::from(table.length).min(reader.size().saturating_sub(offset));
        if table_size < 8 {
            return Ok(metadata);
        }

        let header = reader.read(offset, 6)?;
        let r = EndianReader::big_endian(header);
        let records_end = 6 + u64::from(r.u16_at(2).unwrap_or(0)) * 12;
        let string_offset = r.u16_at(4).unwrap_or(0);
        let string_start = u64::from(string_offset);
        if records_end > table_size || string_start < records_end || string_start > table_size {
            return Ok(metadata);
        }
        let records = Self::parse_name_table(reader, table)?;
        let format_one_tags = Self::format_one_language_tags(reader, table)?;

        for record in &records {
            // Font.pm checks the string against the name table's own size
            // before it decodes or replaces any tag from this record.
            if string_start + u64::from(record.offset) + u64::from(record.length) > table_size {
                continue;
            }
            let Some(base_key) = Self::font_group_key(record.name_id) else {
                continue;
            };
            let key = match Self::name_record_lang_with_fallback(
                record,
                format_one_tags.get(&record.language_id).map(String::as_str),
            ) {
                NameLang::Unsuffixed => base_key.to_string(),
                NameLang::Suffixed(suffix) => format!("{base_key}-{suffix}"),
                NameLang::Omitted => continue,
            };
            // A zero-length in-bounds record is still a present value in
            // Font.pm. It needs no character decoding, even when the
            // platform's nonempty charset is unsupported here.
            if record.length == 0 {
                metadata.insert(key, TagValue::String(String::new()));
                continue;
            }
            let Some(charset) = Self::font_value_charset(record) else {
                if Self::font_ascii_passthrough(record) {
                    let start = offset + string_start + u64::from(record.offset);
                    let data = reader.read(start, record.length as usize)?;
                    if data.is_ascii() {
                        // Decode leaves source-skipped conversion bytes raw.
                        // The final tag drops NULs but retains later ASCII.
                        let value = data
                            .iter()
                            .copied()
                            .filter(|byte| *byte != 0)
                            .map(char::from)
                            .collect();
                        metadata.insert(key, TagValue::String(value));
                        continue;
                    }
                }
                // Native name records replace earlier duplicates in order.
                // An unreadable later value must not leave the earlier value
                // looking like the final ExifTool value.
                metadata.remove(&key);
                continue;
            };
            let Some(value) =
                Self::extract_font_name_string(reader, table, record, string_offset, charset)?
            else {
                metadata.remove(&key);
                continue;
            };
            metadata.insert(key, value);
        }

        Ok(metadata)
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
            (NAME_FONT_SUBFAMILY_ID, "FontSubfamilyID"),
            (NAME_FULL_FONT_NAME, "FontName"),
            (NAME_VERSION, "FontVersion"),
            (NAME_POSTSCRIPT_NAME, "PostScriptName"),
            (NAME_MANUFACTURER, "Manufacturer"),
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
                metadata.insert(key.to_string(), TagValue::String(value));
            }
        }

        // The ExifTool-facing `Font:` keys -- default-language and localized
        // alike -- come from the faithful per-record walk, which reproduces
        // `ProcessTableEntry`'s naming (suffix from `%ttLang`) and ordering
        // (later records replace earlier ones). The legacy bare keys above
        // keep their historical best-record selection; only the `Font:`
        // group is claimed to match ExifTool.
        for (key, value) in Self::extract_exiftool_name_tags(reader, table)? {
            metadata.insert(key, value);
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
        // Every row here is read from the file (`metadata_map::file_rows`):
        // a caller's later `insert`/`get_mut` is what counts as assigned.
        crate::core::metadata_map::file_rows(|| -> Result<MetadataMap> {
            if !Self::verify_signature(reader)? {
                return Err(ExifToolError::parse_error("Invalid TTF signature"));
            }

            let mut metadata = MetadataMap::new();

            metadata.insert("FileType".to_string(), TagValue::String("TTF".to_string()));

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
        })
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::TTF)
    }
}

/// Parses metadata from TTF files.
///
/// This is a convenience wrapper around TTFParser that provides a functional API.
pub fn parse_ttf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    // Every row here is read from the file (`metadata_map::file_rows`):
    // a caller's later `insert`/`get_mut` is what counts as assigned.
    crate::core::metadata_map::file_rows(|| -> std::result::Result<MetadataMap, String> {
        let parser = TTFParser;
        parser.parse(reader).map_err(|e| e.to_string())
    })
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
        // Literal IDs and expected strings are independent of the generated
        // Rust lookup; the complete fixture is checked against fresh Perl.
        for (id, expected) in [
            (1, "fr"),  // %ttLang{Macintosh}: 1 => 'fr'
            (2, "de"),  // 2 => 'de'
            (3, "it"),  // 3 => 'it'   (4 is nl-NL, not Italian)
            (6, "es"),  // 6 => 'es'   (12 is ar, not Spanish)
            (7, "da"),  // 7 => 'da'
            (10, "he"), // 10 => 'he'
            (13, "fi"), // 13 => 'fi'
            // Added 2026-07-27. Same rule: the number on the left is read off
            // %ttLang{Macintosh}, not inferred. Two of today's backlog
            // patches got these wrong in exactly the way this test exists to
            // catch -- one set Norwegian to 12, which the table gives as
            // 'ar', and Font.ttf has no Arabic record to expose it.
            (4, "nl-NL"),  // 4 => 'nl-NL'  (NOT plain 'nl')
            (5, "sv"),     // 5 => 'sv'
            (8, "pt"),     // 8 => 'pt'
            (9, "no"),     // 9 => 'no'     (12 is 'ar', not Norwegian)
            (11, "ja"),    // 11 => 'ja'
            (19, "zh-TW"), // 19 => 'zh-TW'
            (23, "ko"),    // 23 => 'ko'
            (33, "zh-CN"), // 33 => 'zh-CN'
        ] {
            let record = mac_record(id);
            assert_eq!(
                TTFParser::language_suffix(&record),
                Some(expected),
                "Macintosh language ID {id} must map to {expected:?} per ExifTool %ttLang",
            );
        }
    }

    /// A Windows name record carrying `language_id`.
    fn windows_record(language_id: u16) -> NameRecord {
        NameRecord {
            platform_id: PLATFORM_WINDOWS,
            encoding_id: 1,
            language_id,
            name_id: 0,
            length: 0,
            offset: 0,
        }
    }

    /// Pins every Windows LCID to ExifTool's `%ttLang{Windows}` table
    /// (Font.pm), which is a DIFFERENT table from `%ttLang{Macintosh}` with
    /// DIFFERENT spellings for the same languages.
    ///
    /// `ProcessTTF` does `$lang = $ttLang{$sys}{$langID}` and then
    /// `GetLangInfo($tagInfo, $lang)`, so the suffix is the table's string
    /// verbatim -- there is no normalisation step that would strip a region
    /// subtag. Four of these were previously aliased onto the Macintosh
    /// suffix, so a German Windows record was emitted as `FontSubfamily-de`
    /// where ExifTool reports `FontSubfamily-de-DE`. Font.ttf cannot expose
    /// that: every one of its name records is platform 1 (Macintosh).
    #[test]
    fn windows_language_ids_match_exiftool_ttlang_table() {
        // Literal LCIDs and literal ExifTool strings on purpose: naming the
        // constants here would assert each constant's own value back at
        // itself and pass for anything they held.
        for (id, expected) in [
            (0x0409, "en-US"), // 0x0409 => 'en-US'   (NOT bare 'en')
            (0x0406, "da"),    // %ttLang{Windows}: 0x0406 => 'da'
            (0x0407, "de-DE"), // 0x0407 => 'de-DE'   (NOT plain 'de')
            (0x040b, "fi"),    // 0x040b => 'fi'
            (0x040c, "fr-FR"), // 0x040c => 'fr-FR'   (NOT plain 'fr')
            (0x040d, "he"),    // 0x040d => 'he'
            (0x0410, "it-IT"), // 0x0410 => 'it-IT'   (NOT plain 'it')
            (0x0411, "ja"),    // 0x0411 => 'ja'
            (0x0412, "ko"),    // 0x0412 => 'ko'
            (0x0413, "nl-NL"), // 0x0413 => 'nl-NL'
            (0x0c0a, "es-ES"), // 0x0c0a => 'es-ES'   (NOT plain 'es')
            (0x0404, "zh-TW"), // 0x0404 => 'zh-TW'
            (0x0804, "zh-CN"), // 0x0804 => 'zh-CN'
        ] {
            let record = windows_record(id);
            assert_eq!(
                TTFParser::language_suffix(&record),
                Some(expected),
                "Windows LCID {id:#06x} must map to {expected:?} per ExifTool %ttLang{{Windows}}",
            );
        }
    }

    /// `name_record_lang` uses Font.pm's platform-specific source mapping.
    /// Missing IDs are unsuffixed; unavailable format-1 records are refused.
    #[test]
    fn name_record_lang_unsuffixes_omits_and_suffixes_per_ttlang() {
        let rec = |platform_id: u16, language_id: u16| NameRecord {
            platform_id,
            encoding_id: 0,
            language_id,
            name_id: 0,
            length: 0,
            offset: 0,
        };
        // Macintosh 0 is %ttLang's 'en': unsuffixed.
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_MACINTOSH, 0)),
            NameLang::Unsuffixed
        ));
        // A claimed pair suffixes with the %ttLang string verbatim.
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_MACINTOSH, 2)),
            NameLang::Suffixed("de")
        ));
        // Every source-defined ID uses its exact platform-specific suffix.
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_MACINTOSH, 12)),
            NameLang::Suffixed("ar")
        ));
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_WINDOWS, 0x0414)),
            NameLang::Suffixed("no-NO")
        ));
        // IDs ABSENT from %ttLang leave `$lang` undef -- unsuffixed. The
        // key sets are dumped from the pinned Perl: Windows has no 0x0009
        // ("English, neutral") and no 0x0429, Macintosh stops at 0x5e
        // before resuming at 0x80 and skips 0x8f.
        for record in [
            rec(PLATFORM_WINDOWS, 0x0009),
            rec(PLATFORM_WINDOWS, 0x0429),
            rec(PLATFORM_MACINTOSH, 0x5f),
            rec(PLATFORM_MACINTOSH, 0x8f),
        ] {
            assert!(
                matches!(TTFParser::name_record_lang(&record), NameLang::Unsuffixed),
                "platform {} language {:#06x} is absent from %ttLang and must be unsuffixed",
                record.platform_id,
                record.language_id,
            );
        }
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_WINDOWS, 0x0408)),
            NameLang::Suffixed("el")
        ));
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_MACINTOSH, 0x5e)),
            NameLang::Suffixed("eo")
        ));
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_MACINTOSH, 0x90)),
            NameLang::Suffixed("gd")
        ));
        // %ttLang{Unicode} is empty (Font.pm), so $lang is undef and the
        // tag is unsuffixed -- for ordinary language IDs.
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_UNICODE, 0)),
            NameLang::Unsuffixed
        ));
        // IDs >= 0x8000 without a valid format-1 language tag are omitted
        // rather than guessed unsuffixed.
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_UNICODE, 0x8000)),
            NameLang::Omitted
        ));
        // ExifTool treats a format-1 tag filtered to an empty string like
        // an absent language; a literal 'en' is likewise unsuffixed.
        assert!(matches!(
            TTFParser::name_record_lang_with_fallback(&rec(PLATFORM_UNICODE, 0x8000), Some("")),
            NameLang::Unsuffixed
        ));
        assert!(matches!(
            TTFParser::name_record_lang_with_fallback(&rec(PLATFORM_UNICODE, 0x8000), Some("en")),
            NameLang::Unsuffixed
        ));
        // %ttLang{ISO} and %ttLang{Custom} are empty too, so ordinary
        // language IDs use the unsuffixed source name.
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_ISO, 0)),
            NameLang::Unsuffixed
        ));
        assert!(matches!(
            TTFParser::name_record_lang(&rec(PLATFORM_CUSTOM, 0)),
            NameLang::Unsuffixed
        ));
    }

    /// A later name record with the same tag name replaces an earlier one:
    /// `FoundTag` moves the old tag aside and gives the bare name the new
    /// value whenever `$priority >= $oldPriority` (ExifTool.pm:9564-9586),
    /// which equal-priority name records always satisfy.
    #[test]
    fn later_name_record_replaces_earlier_in_exiftool_walk() {
        // A bare name table: format 0, two Macintosh/Roman/English records
        // both carrying nameID 1 (FontFamily), strings "One" then "Two".
        let mut data = vec![
            0x00, 0x00, // format
            0x00, 0x02, // count = 2
            0x00, 0x1e, // stringOffset = 30
        ];
        data.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 1, 0, 3, 0, 0]); // "One"
        data.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 1, 0, 3, 0, 3]); // "Two"
        data.extend_from_slice(b"OneTwo");
        let reader = TestReader::new(data);
        let table = TableEntry {
            tag: *b"name",
            offset: 0,
            length: 36,
        };
        let tags = TTFParser::extract_exiftool_name_tags(&reader, &table).unwrap();
        assert_eq!(
            tags.get("Font:FontFamily"),
            Some(&TagValue::String("Two".to_string()))
        );
    }

    #[test]
    fn format_one_language_tag_names_font_family() {
        // Format 1 has one Unicode record with language ID 0x8000 and one
        // UTF-16BE language tag, nb-NO. The pinned native reader emits
        // Font:FontFamily-nb-NO for this shape.
        let language = "nb-NO"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let family = "Recovery Format One"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_be_bytes()); // format
        data.extend_from_slice(&1u16.to_be_bytes()); // one name record
        data.extend_from_slice(&24u16.to_be_bytes()); // string storage
        for field in [
            0u16,
            3,
            0x8000,
            1,
            family.len() as u16,
            language.len() as u16,
        ] {
            data.extend_from_slice(&field.to_be_bytes());
        }
        data.extend_from_slice(&1u16.to_be_bytes()); // one language tag
        data.extend_from_slice(&(language.len() as u16).to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes());
        data.extend_from_slice(&language);
        data.extend_from_slice(&family);
        let table = TableEntry {
            tag: *b"name",
            offset: 0,
            length: data.len() as u32,
        };
        let tags = TTFParser::extract_exiftool_name_tags(&TestReader::new(data), &table).unwrap();
        assert_eq!(
            tags.get("Font:FontFamily-nb-NO"),
            Some(&TagValue::String("Recovery Format One".to_string()))
        );
        assert!(!tags.contains_key("Font:FontFamily"));
    }

    fn format_one_font_family_with_language(language: &[u8]) -> MetadataMap {
        let family = [0, b'F'];
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&24u16.to_be_bytes());
        for field in [
            PLATFORM_UNICODE,
            3,
            0x8000,
            NAME_FONT_FAMILY,
            family.len() as u16,
            language.len() as u16,
        ] {
            data.extend_from_slice(&field.to_be_bytes());
        }
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&(language.len() as u16).to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes());
        data.extend_from_slice(language);
        data.extend_from_slice(&family);
        let table = TableEntry {
            tag: *b"name",
            offset: 0,
            length: data.len() as u32,
        };
        TTFParser::extract_exiftool_name_tags(&TestReader::new(data), &table).unwrap()
    }

    #[test]
    fn format_one_language_bom_matches_pinned_source() {
        // Pinned native matrix: format1-decode-native-matrix.json.
        for language in [
            &[0, b'n', 0, b'b'][..],
            &[0xfe, 0xff, 0, b'n', 0, b'b'][..],
            &[0xff, 0xfe, b'n', 0, b'b', 0][..],
        ] {
            let tags = format_one_font_family_with_language(language);
            assert_eq!(
                tags.get("Font:FontFamily-nb"),
                Some(&TagValue::String("F".to_string())),
                "language bytes {language:02x?}"
            );
            assert!(!tags.contains_key("Font:FontFamily"));
        }
    }

    #[test]
    fn format_one_language_nul_filter_and_loss_match_pinned_source() {
        for (language, expected_key) in [
            (
                &[0, b'e', 0, b'n', 0, 0, 0, b'-', 0, b'U', 0, b'S'][..],
                "Font:FontFamily",
            ),
            (
                &[0, b'n', 0, b'b', 0, 0, 0, b'-', 0, b'N', 0, b'O'][..],
                "Font:FontFamily-nb",
            ),
            (&[0, 0, 0, b'n', 0, b'b'][..], "Font:FontFamily"),
            (&[0, b'A', 0xd8, 0, 0, b'B'][..], "Font:FontFamily-AB"),
            (&[0, b'n', 0, b'!', 0, b'b'][..], "Font:FontFamily-nb"),
            (&[0xff, 0xfe][..], "Font:FontFamily"),
        ] {
            let tags = format_one_font_family_with_language(language);
            assert_eq!(
                tags.get(expected_key),
                Some(&TagValue::String("F".to_string())),
                "language bytes {language:02x?}"
            );
            assert_eq!(
                tags.keys()
                    .filter(|key| key.starts_with("Font:FontFamily"))
                    .count(),
                1,
                "language bytes {language:02x?}"
            );
        }
    }

    #[test]
    fn malformed_format_one_language_does_not_hide_later_valid_tag() {
        // Pinned ExifTool decodes A, a lone surrogate, B to a string whose
        // language-name filter is AB, then still reads the next nb-NO tag.
        let malformed = [0, b'A', 0xd8, 0, 0, b'B'];
        let later = "nb-NO"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let family = "Later Valid"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_be_bytes()); // format
        data.extend_from_slice(&1u16.to_be_bytes()); // one name record
        data.extend_from_slice(&28u16.to_be_bytes()); // string storage
        for field in [
            0u16,
            3,
            0x8001,
            1,
            family.len() as u16,
            (malformed.len() + later.len()) as u16,
        ] {
            data.extend_from_slice(&field.to_be_bytes());
        }
        data.extend_from_slice(&2u16.to_be_bytes()); // two language tags
        for (len, offset) in [(malformed.len(), 0), (later.len(), malformed.len())] {
            data.extend_from_slice(&(len as u16).to_be_bytes());
            data.extend_from_slice(&(offset as u16).to_be_bytes());
        }
        data.extend_from_slice(&malformed);
        data.extend_from_slice(&later);
        data.extend_from_slice(&family);
        let table = TableEntry {
            tag: *b"name",
            offset: 0,
            length: data.len() as u32,
        };
        let reader = TestReader::new(data);
        let languages = TTFParser::format_one_language_tags(&reader, &table).unwrap();
        assert_eq!(languages.get(&0x8000).map(String::as_str), Some("AB"));
        assert_eq!(languages.get(&0x8001).map(String::as_str), Some("nb-NO"));
        let tags = TTFParser::extract_exiftool_name_tags(&reader, &table).unwrap();
        assert_eq!(
            tags.get("Font:FontFamily-nb-NO"),
            Some(&TagValue::String("Later Valid".to_string()))
        );
    }

    /// The Windows LCIDs retain Font.pm's region subtags.
    #[test]
    fn windows_region_tagged_language_ids_keep_source_suffixes() {
        for (id, expected) in [
            (0x0414, "no-NO"),
            (0x0416, "pt-BR"),
            (0x041d, "sv-SE"),
            (0x0816, "pt-PT"),
        ] {
            let record = windows_record(id);
            assert_eq!(
                TTFParser::language_suffix(&record),
                Some(expected),
                "Windows LCID {id:#06x} must retain Font.pm's region subtag",
            );
        }
    }

    /// Macintosh and Windows suffixes for the same language must not be
    /// assumed equal. This is the property the old paired match arms
    /// violated.
    #[test]
    fn macintosh_and_windows_suffixes_differ_where_exiftool_differs() {
        // Macintosh 2 => 'de' but Windows 0x0407 => 'de-DE'.
        assert_eq!(TTFParser::language_suffix(&mac_record(2)), Some("de"));
        assert_eq!(
            TTFParser::language_suffix(&windows_record(0x0407)),
            Some("de-DE")
        );
        // Macintosh 3 => 'it' but Windows 0x0410 => 'it-IT'.
        assert_eq!(TTFParser::language_suffix(&mac_record(3)), Some("it"));
        assert_eq!(
            TTFParser::language_suffix(&windows_record(0x0410)),
            Some("it-IT")
        );
    }

    /// Source-defined Macintosh IDs are no longer dropped by a manual subset.
    #[test]
    fn source_defined_macintosh_language_ids_are_mapped() {
        for (id, expected) in [(12, "ar"), (14, "el"), (32, "ru")] {
            let record = mac_record(id);
            assert_eq!(
                TTFParser::language_suffix(&record),
                Some(expected),
                "Macintosh language ID {id} must keep Font.pm's suffix",
            );
        }
    }

    /// Pins Mac OS Hebrew decoding to ExifTool's Charset/MacHebrew.pm.
    ///
    /// The bytes below are the FontFamily record actually stored in
    /// /tmp/oxidex-exiftool-cache/combined-samples/Font.ttf
    /// (Plat=1/Macintosh, Enc=5/MacHebrew, Lang=10/he), and the expected
    /// string is what `exiftool -Font:FontFamily-he` prints for that file.
    #[test]
    fn mac_hebrew_decodes_to_exiftool_value() {
        // 0xF8 0xF2 0xF0 0xF0 0xE4 -> U+05E8 U+05E2 U+05E0 U+05E0 U+05D4
        let raanana = TTFParser::decode_mac_hebrew(&[0xf8, 0xf2, 0xf0, 0xf0, 0xe4]);
        assert_eq!(raanana, "\u{5e8}\u{5e2}\u{5e0}\u{5e0}\u{5d4}");

        // The bytes that separate MacHebrew from MacRoman. Decoding this
        // record with the MacRoman table -- which one backlog patch did by
        // copying MacRoman and appending the Hebrew block -- yields
        // "°¶}|" instead. Literals are from MacHebrew.pm.
        assert_eq!(TTFParser::decode_mac_hebrew(&[0xa0]), " "); // 0xa0 => 0x20
        assert_eq!(TTFParser::decode_mac_hebrew(&[0xa6]), "\u{20aa}"); // 0xa6 => 0x20aa
        assert_eq!(TTFParser::decode_mac_hebrew(&[0xfb]), "}"); // 0xfb => 0x7d
        assert_eq!(TTFParser::decode_mac_hebrew(&[0xff]), "|"); // 0xff => 0x7c

        // The three multi-code-point entries in the table.
        assert_eq!(TTFParser::decode_mac_hebrew(&[0x81]), "\u{5f2}\u{5b7}");
        assert_eq!(
            TTFParser::decode_mac_hebrew(&[0xc0]),
            "\u{f86a}\u{5dc}\u{5b9}"
        );
        assert_eq!(TTFParser::decode_mac_hebrew(&[0xde]), "\u{5b8}\u{f87f}");

        // ASCII is pass-through: this is why FontSubfamily-he ("Regular")
        // already matched before MacHebrew was wired.
        assert_eq!(TTFParser::decode_mac_hebrew(b"Regular"), "Regular");
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
    fn unsupported_macintosh_charset_does_not_publish_wrong_font_value() {
        // Pinned Font.pm decodes c2 a0 with MacArabic as "آ "; interpreting
        // it as UTF-8 would publish a non-breaking space under the new ar tag.
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_be_bytes()); // format
        data.extend_from_slice(&1u16.to_be_bytes()); // one record
        data.extend_from_slice(&18u16.to_be_bytes()); // string storage
        for field in [PLATFORM_MACINTOSH, 4, 12, NAME_FONT_FAMILY, 2, 0] {
            data.extend_from_slice(&field.to_be_bytes());
        }
        data.extend_from_slice(&[0xc2, 0xa0]);
        let table = TableEntry {
            tag: *b"name",
            offset: 0,
            length: data.len() as u32,
        };
        let reader = TestReader::new(data);
        let tags = TTFParser::extract_exiftool_name_tags(&reader, &table).unwrap();
        assert!(!tags.contains_key("Font:FontFamily-ar"));
    }

    #[test]
    fn unsupported_macintosh_charset_preserves_ascii_font_value() {
        // Pinned Font.pm 13.59 returns A and AB for these Mac encodings,
        // including a supported-but-unimplemented table (4), an absent
        // table (9), an uninterpreted charset (32), and an unknown ID (33).
        // Decode leaves bytes needing no remapping alone; a later record
        // replaces the earlier same-key value.
        for encoding in [4, 9, 21, 32, 33] {
            for (last, expected) in [(&b"A"[..], "A"), (&b"A\0B"[..], "AB")] {
                let tags = font_two_record_decode_case(
                    PLATFORM_MACINTOSH,
                    MAC_ENCODING_ROMAN,
                    encoding,
                    12,
                    b"Earlier",
                    last,
                );
                assert_eq!(
                    tags.get("Font:FontFamily-ar"),
                    Some(&TagValue::String(expected.to_string())),
                    "Mac encoding {encoding} final record {last:?}"
                );
            }
        }
    }

    #[test]
    fn windows_and_unicode_unknown_charsets_preserve_raw_ascii_font_values() {
        // Pinned ascii-eligibility-native-matrix.json reports A and AB for
        // these charset IDs. record-order-native-matrix.json also reports B
        // when a Windows encoding-99 record follows a UCS2 A record.
        for (platform, initial_encoding, encoding, language, key) in [
            (PLATFORM_WINDOWS, 1, 3, 0x0409, "Font:FontFamily-en-US"),
            (PLATFORM_WINDOWS, 1, 4, 0x0409, "Font:FontFamily-en-US"),
            (PLATFORM_WINDOWS, 1, 5, 0x0409, "Font:FontFamily-en-US"),
            (PLATFORM_WINDOWS, 1, 6, 0x0409, "Font:FontFamily-en-US"),
            (PLATFORM_WINDOWS, 1, 99, 0x0409, "Font:FontFamily-en-US"),
            (PLATFORM_UNICODE, 4, 5, 0, "Font:FontFamily"),
        ] {
            for (last, expected) in [(&b"A"[..], "A"), (&b"A\0B"[..], "AB"), (&b"B"[..], "B")] {
                let tags = font_two_record_decode_case(
                    platform,
                    initial_encoding,
                    encoding,
                    language,
                    &[0, b'E'],
                    last,
                );
                assert_eq!(
                    tags.get(key),
                    Some(&TagValue::String(expected.to_string())),
                    "platform {platform} encoding {encoding} final record {last:?}"
                );
            }
        }
    }

    #[test]
    fn iso_and_custom_ascii_name_records_publish_unsuffixed_font_values() {
        // Font.pm maps ISO and Custom platforms to empty ttLang tables;
        // pinned ascii-eligibility-native-matrix.json returns A and AB for
        // ISO UTF8/Latin and unknown Custom raw ASCII name records.
        for (platform, encoding) in [(2, 0), (2, 2), (4, 0)] {
            for (last, expected) in [(&b"A"[..], "A"), (&b"A\0B"[..], "AB")] {
                let tags =
                    font_two_record_decode_case(platform, encoding, encoding, 0, b"Earlier", last);
                assert_eq!(
                    tags.get("Font:FontFamily"),
                    Some(&TagValue::String(expected.to_string())),
                    "platform {platform} encoding {encoding} final record {last:?}"
                );
            }
        }
    }

    #[test]
    fn font_group_respects_platform_charset_and_ucs2_boundary() {
        // These bytes were checked against pinned Font.pm 13.59: ShiftJIS
        // 82 a0 is あ, while interpreting it as UTF-16BE produces 芠.
        // UCS2 does not combine D83D DE00 into the emoji that UTF16 does.
        let smile = &[0xd8, 0x3d, 0xde, 0x00];
        let omega = &[0x03, 0xa9];
        let cases: &[(u16, u16, u16, &[u8], &str, Option<&str>)] = &[
            (
                PLATFORM_WINDOWS,
                2,
                0x0414,
                &[0x82, 0xa0],
                "Font:FontFamily-no-NO",
                None,
            ),
            (
                PLATFORM_WINDOWS,
                0,
                0x0414,
                omega,
                "Font:FontFamily-no-NO",
                None,
            ),
            (
                PLATFORM_WINDOWS,
                1,
                0x0414,
                smile,
                "Font:FontFamily-no-NO",
                None,
            ),
            (
                PLATFORM_WINDOWS,
                1,
                0x0414,
                omega,
                "Font:FontFamily-no-NO",
                Some("Ω"),
            ),
            (PLATFORM_UNICODE, 0, 0, smile, "Font:FontFamily", None),
            (PLATFORM_UNICODE, 0, 0, omega, "Font:FontFamily", Some("Ω")),
            (PLATFORM_UNICODE, 4, 0, smile, "Font:FontFamily", Some("😀")),
            (PLATFORM_UNICODE, 5, 0, omega, "Font:FontFamily", None),
        ];
        for &(platform, encoding, language, bytes, key, expected) in cases {
            let mut data = Vec::new();
            data.extend_from_slice(&0u16.to_be_bytes()); // format
            data.extend_from_slice(&1u16.to_be_bytes()); // one record
            data.extend_from_slice(&18u16.to_be_bytes()); // string storage
            for field in [
                platform,
                encoding,
                language,
                NAME_FONT_FAMILY,
                bytes.len() as u16,
                0,
            ] {
                data.extend_from_slice(&field.to_be_bytes());
            }
            data.extend_from_slice(bytes);
            let table = TableEntry {
                tag: *b"name",
                offset: 0,
                length: data.len() as u32,
            };
            let reader = TestReader::new(data);
            let tags = TTFParser::extract_exiftool_name_tags(&reader, &table).unwrap();
            let expected_value = if bytes == smile
                && matches!(
                    (platform, encoding),
                    (PLATFORM_WINDOWS, 1) | (PLATFORM_UNICODE, 0)
                ) {
                Some(TagValue::TextBytes(vec![
                    0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80,
                ]))
            } else {
                expected.map(|value| TagValue::String(value.to_string()))
            };
            assert_eq!(
                tags.get(key),
                expected_value.as_ref(),
                "platform {platform}, encoding {encoding}, language {language}"
            );
        }
    }

    #[test]
    fn malformed_ucs2_name_records_are_retained() {
        // Pinned 13.59 `Font.pm`/`Charset.pm` yields raw ED-prefixed UTF-8
        // for each surrogate; the JSON writer later applies FixUTF8.
        for (bytes, expected_present) in [
            (&[0xd8, 0x00, 0x00, 0x41][..], true),
            (&[0xdc, 0x00, 0x00, 0x41][..], true),
            (&[0xd8, 0x3d, 0xde, 0x00][..], true),
            (&[0x00, 0x41, 0x00][..], true),
        ] {
            let tags =
                font_two_record_decode_case(PLATFORM_WINDOWS, 1, 1, 0x0409, &[0x00, 0x42], bytes);
            assert_eq!(
                tags.contains_key("Font:FontFamily-en-US"),
                expected_present,
                "bytes {bytes:02x?}"
            );
        }
    }

    #[test]
    fn font_fixed_width_name_bytes_and_json_follow_pinned_charset_boundary() {
        use crate::cli::output_formatter::tag_value_to_json;

        // Raw expected bytes from pinned Perl 5.38.2 + ExifTool 13.59
        // `-b -FontFamily-en-US` (Unicode 4 uses `-FontFamily`); JSON values
        // from `-j -G1` on the same single-record name-table files.
        let cases: &[(u16, u16, &[u8], &[u8], &str)] = &[
            (
                PLATFORM_WINDOWS,
                1,
                &[0xd8, 0x00, 0x00, 0x41],
                &[0xed, 0xa0, 0x80, b'A'],
                "???A",
            ),
            (
                PLATFORM_WINDOWS,
                1,
                &[0xdc, 0x00, 0x00, 0x41],
                &[0xed, 0xb0, 0x80, b'A'],
                "???A",
            ),
            (
                PLATFORM_WINDOWS,
                1,
                &[0xd8, 0x3d, 0xde, 0x00],
                &[0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80],
                "??????",
            ),
            (
                PLATFORM_UNICODE,
                4,
                &[0xd8, 0x3d, 0xde, 0x00],
                "😀".as_bytes(),
                "😀",
            ),
            (PLATFORM_WINDOWS, 1, &[0xff, 0xfe, 0x41, 0x00], b"A", "A"),
            (PLATFORM_WINDOWS, 1, &[0xfe, 0xff, 0x00, 0x41], b"A", "A"),
            (PLATFORM_WINDOWS, 1, &[0, 0, 0xd8, 0], b"", ""),
            (
                PLATFORM_WINDOWS,
                1,
                &[0xff, 0xff, 0, 0x41],
                &[0xef, 0xbf, 0xbf, b'A'],
                "???A",
            ),
            (PLATFORM_WINDOWS, 1, &[0, 0x41, 0], b"A", "A"),
        ];
        for &(platform, encoding, source, raw, json) in cases {
            let language = if platform == PLATFORM_WINDOWS {
                0x0409
            } else {
                0
            };
            let tags = font_two_record_decode_case(
                platform,
                encoding,
                encoding,
                language,
                &[0, b'B'],
                source,
            );
            let key = if platform == PLATFORM_WINDOWS {
                "Font:FontFamily-en-US"
            } else {
                "Font:FontFamily"
            };
            let value = tags
                .get(key)
                .unwrap_or_else(|| panic!("missing {key}: {source:02x?}"));
            assert_eq!(value.as_text_bytes(), Some(raw), "source {source:02x?}");
            assert_eq!(
                tag_value_to_json(Some(key), value),
                serde_json::json!(json),
                "source {source:02x?}"
            );
        }
    }

    #[test]
    fn unsupported_duplicate_clears_prior_font_value_in_name_order() {
        // Native Font.pm's final no-NO value is "あ" when ShiftJIS is last,
        // and "A" when the UCS2 record is last. This reader cannot decode
        // ShiftJIS, so it must not leave an earlier "A" as the final value.
        for unsupported_last in [true, false] {
            let records: [(u16, &[u8]); 2] = if unsupported_last {
                [(1, &[0, b'A']), (2, &[0x82, 0xa0])]
            } else {
                [(2, &[0x82, 0xa0]), (1, &[0, b'A'])]
            };
            let mut data = Vec::new();
            data.extend_from_slice(&0u16.to_be_bytes()); // format
            data.extend_from_slice(&2u16.to_be_bytes()); // two records
            data.extend_from_slice(&30u16.to_be_bytes()); // string storage
            let mut strings = Vec::new();
            for &(encoding, bytes) in &records {
                for field in [
                    PLATFORM_WINDOWS,
                    encoding,
                    0x0414,
                    NAME_FONT_FAMILY,
                    bytes.len() as u16,
                    strings.len() as u16,
                ] {
                    data.extend_from_slice(&field.to_be_bytes());
                }
                strings.extend_from_slice(bytes);
            }
            data.extend_from_slice(&strings);
            let table = TableEntry {
                tag: *b"name",
                offset: 0,
                length: data.len() as u32,
            };
            let reader = TestReader::new(data);
            let tags = TTFParser::extract_exiftool_name_tags(&reader, &table).unwrap();
            let expected = (!unsupported_last).then(|| TagValue::String("A".to_string()));
            assert_eq!(tags.get("Font:FontFamily-no-NO"), expected.as_ref());
        }
    }

    #[test]
    fn font_name_record_bounds_precede_duplicate_refusal() {
        // Native Font.pm skips out-of-table records before HandleTag, but an
        // in-bounds empty or malformed value replaces the earlier primary.
        let tags = |encoding: u16,
                    length: u16,
                    offset: u16,
                    payload: &[u8],
                    declared_size: Option<u32>,
                    string_start: u16| {
            let mut data = Vec::new();
            data.extend_from_slice(&0u16.to_be_bytes()); // format
            data.extend_from_slice(&2u16.to_be_bytes()); // two records
            data.extend_from_slice(&string_start.to_be_bytes());
            for (enc, len, off) in [(1u16, 2u16, 0u16), (encoding, length, offset)] {
                for field in [PLATFORM_WINDOWS, enc, 0x0409, NAME_FONT_FAMILY, len, off] {
                    data.extend_from_slice(&field.to_be_bytes());
                }
            }
            data.extend_from_slice(&[0, b'A']);
            data.extend_from_slice(payload);
            let table = TableEntry {
                tag: *b"name",
                offset: 0,
                length: declared_size.unwrap_or(data.len() as u32),
            };
            TTFParser::extract_exiftool_name_tags(&TestReader::new(data), &table).unwrap()
        };
        let key = "Font:FontFamily-en-US";
        let earlier_value = TagValue::String("A".to_string());
        let earlier = Some(&earlier_value);
        let out_of_bounds = tags(2, 2, 100, &[], None, 30);
        assert_eq!(out_of_bounds.get(key), earlier);
        let past_declared_table = tags(2, 2, 2, &[0x82, 0xa0], Some(32), 30);
        assert_eq!(past_declared_table.get(key), earlier);
        for encoding in [1, 2, 99] {
            let empty = tags(encoding, 0, 2, &[], None, 30);
            assert_eq!(
                empty.get(key),
                Some(&TagValue::String(String::new())),
                "empty encoding {encoding}"
            );
        }
        // Perl's `unpack('n*')` drops the odd byte; an isolated surrogate
        // remains a three-byte non-Unicode text value. Both displace `A`.
        assert_eq!(
            tags(1, 1, 2, &[0], None, 30).get(key),
            Some(&TagValue::new_string(""))
        );
        assert_eq!(
            tags(1, 2, 2, &[0xd8, 0], None, 30).get(key),
            Some(&TagValue::new_text_bytes(vec![0xed, 0xa0, 0x80]))
        );
        for (declared_size, string_start) in [(20, 30), (34, 18), (34, 40)] {
            let invalid_header = tags(1, 2, 2, &[0, b'B'], Some(declared_size), string_start);
            assert!(!invalid_header.contains_key(key));
        }
    }

    #[test]
    fn zero_length_name_records_replace_across_source_charsets() {
        // Pinned Font.pm handles an in-bounds empty record as a present empty
        // value even when its encoding has no text decoder. The ten native
        // fixtures are recorded in record-order-empty-platform-matrix.json.
        for (platform, first_encoding, last_encoding, language, initial) in [
            (PLATFORM_WINDOWS, 1, 1, 0x0409, &[0, b'A'][..]),
            (PLATFORM_WINDOWS, 1, 2, 0x0409, &[0, b'A'][..]),
            (PLATFORM_WINDOWS, 1, 99, 0x0409, &[0, b'A'][..]),
            (PLATFORM_UNICODE, 4, 0, 0, &[0, b'A'][..]),
            (PLATFORM_UNICODE, 4, 4, 0, &[0, b'A'][..]),
            (PLATFORM_UNICODE, 4, 99, 0, &[0, b'A'][..]),
            (PLATFORM_MACINTOSH, 0, 0, 0, &[b'A'][..]),
            (PLATFORM_MACINTOSH, 0, 1, 0, &[b'A'][..]),
            (PLATFORM_MACINTOSH, 0, 4, 0, &[b'A'][..]),
            (PLATFORM_MACINTOSH, 0, 99, 0, &[b'A'][..]),
        ] {
            let mut data = Vec::new();
            data.extend_from_slice(&0u16.to_be_bytes());
            data.extend_from_slice(&2u16.to_be_bytes());
            data.extend_from_slice(&30u16.to_be_bytes());
            for (encoding, length, offset) in [
                (first_encoding, initial.len() as u16, 0),
                (last_encoding, 0, initial.len() as u16),
            ] {
                for field in [
                    platform,
                    encoding,
                    language,
                    NAME_FONT_FAMILY,
                    length,
                    offset,
                ] {
                    data.extend_from_slice(&field.to_be_bytes());
                }
            }
            data.extend_from_slice(initial);
            let table = TableEntry {
                tag: *b"name",
                offset: 0,
                length: data.len() as u32,
            };
            let tags = TTFParser::extract_exiftool_name_tags(&TestReader::new(data), &table)
                .expect("name table parses");
            let key = if platform == PLATFORM_WINDOWS {
                "Font:FontFamily-en-US"
            } else {
                "Font:FontFamily"
            };
            assert_eq!(
                tags.get(key),
                Some(&TagValue::String(String::new())),
                "platform {platform}, last encoding {last_encoding}"
            );
        }
    }

    fn font_two_record_decode_case(
        platform: u16,
        initial_encoding: u16,
        second_encoding: u16,
        language: u16,
        initial: &[u8],
        second: &[u8],
    ) -> MetadataMap {
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_be_bytes());
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(&30u16.to_be_bytes());
        for (encoding, length, offset) in [
            (initial_encoding, initial.len() as u16, 0),
            (second_encoding, second.len() as u16, initial.len() as u16),
        ] {
            for field in [
                platform,
                encoding,
                language,
                NAME_FONT_FAMILY,
                length,
                offset,
            ] {
                data.extend_from_slice(&field.to_be_bytes());
            }
        }
        data.extend_from_slice(initial);
        data.extend_from_slice(second);
        let table = TableEntry {
            tag: *b"name",
            offset: 0,
            length: data.len() as u32,
        };
        TTFParser::extract_exiftool_name_tags(&TestReader::new(data), &table).unwrap()
    }

    #[test]
    fn font_unicode_bom_and_decoded_nul_match_source() {
        // Pinned native values: decode-boundary-native-matrix.json and
        // nul-boundary-native-matrix.json.
        for (platform, initial_encoding, encoding, language, bytes, expected) in [
            (PLATFORM_WINDOWS, 1, 1, 0x0414, &[0, b'B'][..], "B"),
            (
                PLATFORM_WINDOWS,
                1,
                1,
                0x0414,
                &[0xfe, 0xff, 0, b'B'][..],
                "B",
            ),
            (
                PLATFORM_WINDOWS,
                1,
                1,
                0x0414,
                &[0xff, 0xfe, b'B', 0][..],
                "B",
            ),
            (PLATFORM_WINDOWS, 1, 1, 0x0414, &[0xff, 0xfe][..], ""),
            (PLATFORM_UNICODE, 4, 0, 0, &[0xff, 0xfe, b'B', 0][..], "B"),
            (PLATFORM_UNICODE, 4, 4, 0, &[0xfe, 0xff, 0, b'B'][..], "B"),
            (PLATFORM_UNICODE, 4, 4, 0, &[0xff, 0xfe, b'B', 0][..], "B"),
            (PLATFORM_UNICODE, 4, 4, 0, &[0xff, 0xfe][..], ""),
            (PLATFORM_WINDOWS, 1, 1, 0x0414, &[0, 0, 0, b'B'][..], ""),
            (
                PLATFORM_WINDOWS,
                1,
                1,
                0x0414,
                &[0, b'A', 0, 0, 0, b'B'][..],
                "A",
            ),
            (PLATFORM_UNICODE, 4, 0, 0, &[0, 0, 0, b'B'][..], ""),
            (
                PLATFORM_UNICODE,
                4,
                4,
                0,
                &[0, b'A', 0, 0, 0, b'B'][..],
                "A",
            ),
        ] {
            let initial = &[0, b'A'];
            let tags = font_two_record_decode_case(
                platform,
                initial_encoding,
                encoding,
                language,
                initial,
                bytes,
            );
            let key = if platform == PLATFORM_WINDOWS {
                "Font:FontFamily-no-NO"
            } else {
                "Font:FontFamily"
            };
            assert_eq!(
                tags.get(key),
                Some(&TagValue::String(expected.to_string())),
                "platform {platform}, encoding {encoding}, bytes {bytes:02x?}"
            );
        }
    }

    #[test]
    fn font_macintosh_decoded_empty_and_nul_match_source() {
        // MacJapanese converts even ASCII; the other supported Macintosh
        // charsets skip conversion for ASCII-only strings. Source conversion
        // truncates at NUL, while the fast path removes NUL and keeps text.
        for (encoding, language, bytes, expected) in [
            (0, 0, &[0, b'A'][..], "A"),
            (0, 0, &[0, 0x80][..], ""),
            (0, 0, &[b'A', 0, b'B'][..], "AB"),
            (5, 0, &[0, b'A'][..], "A"),
            (5, 0, &[0, 0x80][..], ""),
            (1, 12, &[0, b'A'][..], ""),
            (1, 12, &[0, 0x80][..], ""),
            (2, 0, &[0, b'A'][..], "A"),
            (2, 0, &[0, 0x80][..], ""),
            (3, 0, &[0, b'A'][..], "A"),
            (3, 0, &[0, 0x80][..], ""),
            (25, 0, &[0, b'A'][..], "A"),
            (25, 0, &[0, 0x80][..], ""),
        ] {
            let tags =
                font_two_record_decode_case(PLATFORM_MACINTOSH, 0, encoding, language, b"A", bytes);
            let key = if language == 12 {
                "Font:FontFamily-ar"
            } else {
                "Font:FontFamily"
            };
            assert_eq!(
                tags.get(key),
                Some(&TagValue::String(expected.to_string())),
                "Macintosh encoding {encoding}, bytes {bytes:02x?}"
            );
        }
    }

    /// End-to-end check that a Macintosh CJK record reaches the right tag
    /// with the right text: the encoding ID has to pick the charset, the
    /// language ID has to pick the suffix, and the two are different numbers.
    ///
    /// The bytes and the expected strings are the four `FontSubfamily`
    /// records in the corpus's `Font.ttf`, verified against
    /// `exiftool -s -FontSubfamily-ja Font.ttf` (and -ko, -zh-CN, -zh-TW) on
    /// ExifTool 13.55. `mac_charset`'s own tests cover the decoder; this
    /// covers the wiring around it.
    #[test]
    fn macintosh_cjk_subfamilies_match_exiftool() {
        // (encoding ID, language ID, raw bytes, tag suffix, expected text)
        let records: [(u16, u16, &[u8], &str, &str); 4] = [
            (
                1,
                11,
                &[0x83, 0x8c, 0x83, 0x4d, 0x83, 0x85, 0x83, 0x89, 0x81, 0x5b],
                "ja",
                "レギュラー",
            ),
            (
                2,
                19,
                &[0xbc, 0xd0, 0xb7, 0xc7, 0xc5, 0xe9],
                "zh-TW",
                "標準體",
            ),
            (3, 23, &[0xc0, 0xcf, 0xb9, 0xdd], "ko", "일반"),
            (25, 33, &[0xb3, 0xa3, 0xb9, 0xe6], "zh-CN", "常规"),
        ];

        const RECORD_LEN: u16 = 12;
        let count = records.len() as u16;
        let storage_offset = 6 + RECORD_LEN * count;

        let mut header = vec![
            b't', b'r', b'u', b'e', // sfnt version
            0x00, 0x01, // numTables = 1
            0x00, 0x00, // searchRange
            0x00, 0x00, // entrySelector
            0x00, 0x00, // rangeShift
            b'n', b'a', b'm', b'e', // table tag
            0x00, 0x00, 0x00, 0x00, // checksum
            0x00, 0x00, 0x00, 0x1c, // table offset = 28
            0x00, 0x00, 0x00, 0x00, // table length filled after strings
        ];
        header.extend_from_slice(&0u16.to_be_bytes()); // name table format
        header.extend_from_slice(&count.to_be_bytes());
        header.extend_from_slice(&storage_offset.to_be_bytes());

        let mut strings = Vec::new();
        for &(encoding_id, language_id, bytes, _, _) in &records {
            header.extend_from_slice(&PLATFORM_MACINTOSH.to_be_bytes());
            header.extend_from_slice(&encoding_id.to_be_bytes());
            header.extend_from_slice(&language_id.to_be_bytes());
            header.extend_from_slice(&NAME_FONT_SUBFAMILY.to_be_bytes());
            header.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            header.extend_from_slice(&(strings.len() as u16).to_be_bytes());
            strings.extend_from_slice(bytes);
        }
        header.extend_from_slice(&strings);
        let table_length = (header.len() - 28) as u32;
        header[24..28].copy_from_slice(&table_length.to_be_bytes());

        let metadata = TTFParser.parse(&TestReader::new(header)).unwrap();
        for (_, _, _, suffix, expected) in records {
            assert_eq!(
                metadata.get(&format!("Font:FontSubfamily-{suffix}")),
                Some(&TagValue::String(expected.to_string())),
                "FontSubfamily-{suffix}"
            );
        }
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
