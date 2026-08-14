//! vCard (VCF) contact format parser
//!
//! This parser extracts metadata from vCard files, which are text-based
//! contact information files following the vCard standard (RFC 6350).
//!
//! # Format Structure
//!
//! vCard files begin with "BEGIN:VCARD" and contain key-value pairs
//! for contact information such as name, email, telephone, etc.
//!
//! Tag names below match ExifTool's `Image::ExifTool::VCard::Main` table
//! (lib/Image/ExifTool/VCard.pm, ExifTool 13.59) so that oxidex output is
//! byte-for-byte comparable to `exiftool` for these fields.

#![allow(dead_code)]

use crate::core::{
    FileFormat, FileReader, FormatParser, Instance, MetadataMap, SHIM_DEFAULT_PRIORITY, TagValue,
};
use crate::error::{ExifToolError, Result};

/// VCF signature: "BEGIN:VCARD"
const VCF_SIGNATURE: &[u8] = b"BEGIN:VCARD";

/// VCF/vCard parser for extracting metadata from contact files
pub struct VCFParser;

/// Maps an uppercased vCard property name to the ExifTool tag name it is
/// documented under in `VCard::Main`. `None` values are handled specially
/// below (they need value transforms), everything else is a straight
/// name (and, for base values, value) mapping.
///
/// Ref: lib/Image/ExifTool/VCard.pm %Image::ExifTool::VCard::Main (v1.07)
fn main_table_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "VERSION" => "VCardVersion",
        "FN" => "FormattedName",
        "N" => "Name",
        "ORG" => "Organization",
        "TITLE" => "JobTitle",
        "EMAIL" => "Email",
        "TEL" => "Telephone",
        "ADR" => "Address",
        "URL" => "URL",
        "NOTE" => "Note",
        "UID" => "UID",
        "REV" => "Revision",
        "NICKNAME" => "Nickname",
        "GENDER" => "Gender",
        "ANNIVERSARY" => "Anniversary",
        "IMPP" => "IMPP",
        "LANG" => "Language",
        "LOGO" => "Logo",
        "PRODID" => "Software",
        "SOUND" => "Sound",
        // Time-group / geo tags handled separately below because they need
        // value conversion (%timeInfo / geo: prefix strip), not just renaming.
        "BDAY" | "TZ" | "GEO" | "PHOTO" => return None,
        _ => return None,
    })
}

/// Splits a vCard content line into its `group.name *(";" param)` prefix and
/// its value, at the first ":" that is not inside a double-quoted parameter
/// value.
///
/// RFC 6350 section 3.3 allows parameter values to be quoted strings
/// (`DQUOTE *QSAFE-CHAR DQUOTE`) which may themselves contain ":", ";", and
/// ",". A naive `str::split_once(':')` finds the first colon anywhere in the
/// line, so a line like:
///   `ADR;type=OTHER;GEO="geo:12.3457,78.910";LABEL=Test\nLabel:;;Other Rd....`
/// gets split inside the quoted GEO parameter value instead of at the real
/// property/value boundary, corrupting the extracted value. This walks the
/// line tracking quote state so the split lands on the correct colon.
fn split_property_line(line: &str) -> Option<(&str, &str)> {
    let mut in_quotes = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                return Some((&line[..idx], &line[idx + 1..]));
            }
            _ => {}
        }
    }
    None
}

/// Extracts a `LANGUAGE=xx` parameter from a property line's `raw_key`
/// (everything before the unquoted `:`), if present.
///
/// `ProcessVCard` reads `LANGUAGE` as `$param{Language}` and hands it to
/// `GetLangInfo`, which appends `-<lang>` to the tag *name*
/// (`GetVCardTag`'s `$langCode` argument, VCard.pm:207-224) -- so
/// `NOTE;LANGUAGE=fr:Oui!` becomes tag `Note-fr`, a name distinct from plain
/// `Note`, not a second occurrence of it. Before this existed, the value
/// bug this same commit fixes (dropped duplicate occurrences) hid the
/// consequence: `Oui!` silently landed as a third `Note` occurrence instead
/// of its own `Note-fr` tag, which would have surfaced as an extra,
/// oracle-mismatched occurrence the moment retention started working.
fn language_param(raw_key: &str) -> Option<&str> {
    raw_key.split(';').skip(1).find_map(|segment| {
        let (name, value) = segment.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("LANGUAGE")
            .then(|| value.trim())
            .filter(|value| !value.is_empty())
    })
}

/// Converts a vCard/iCalendar date-time value to ExifTool's EXIF-style
/// separators, matching the `%timeInfo` ValueConv in VCard.pm:
///   YYYYMMDDThhmmss(Z?) -> YYYY:MM:DD hh:mm:ss(Z?)
///   YYYYMMDD            -> YYYY:MM:DD
///   YYYY-MM-DD           -> YYYY:MM:DD
fn convert_vcard_time(val: &str) -> String {
    let bytes: Vec<char> = val.chars().collect();
    // YYYYMMDDThhmmssZ?
    if bytes.len() >= 15
        && bytes[..8].iter().all(|c| c.is_ascii_digit())
        && bytes[8] == 'T'
        && bytes[9..15].iter().all(|c| c.is_ascii_digit())
    {
        let date = &val[0..8];
        let time = &val[9..15];
        let rest = &val[15..];
        return format!(
            "{}:{}:{} {}:{}:{}{}",
            &date[0..4],
            &date[4..6],
            &date[6..8],
            &time[0..2],
            &time[2..4],
            &time[4..6],
            rest
        );
    }
    // YYYYMMDD
    if bytes.len() == 8 && bytes.iter().all(|c| c.is_ascii_digit()) {
        return format!("{}:{}:{}", &val[0..4], &val[4..6], &val[6..8]);
    }
    // YYYY-MM-DD
    if bytes.len() == 10
        && bytes[4] == '-'
        && bytes[7] == '-'
        && bytes[0..4].iter().all(|c| c.is_ascii_digit())
        && bytes[5..7].iter().all(|c| c.is_ascii_digit())
        && bytes[8..10].iter().all(|c| c.is_ascii_digit())
    {
        return format!("{}:{}:{}", &val[0..4], &val[5..7], &val[8..10]);
    }
    val.to_string()
}

impl VCFParser {
    /// Verifies VCF signature by checking for "BEGIN:VCARD" at the start of the file
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - File has valid VCF signature
    /// * `Ok(false)` - File does not have VCF signature
    /// * `Err(ExifToolError)` - I/O error reading file
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 11 {
            return Ok(false);
        }
        let header = reader.read(0, 11)?;
        Ok(header == VCF_SIGNATURE)
    }

    /// Parse vCard content to extract metadata, using ExifTool's VCard::Main
    /// tag names (see `main_table_name`) wherever we can derive the value with
    /// certainty.
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted vCard metadata
    /// * `Err(ExifToolError)` - Parse error or invalid UTF-8
    pub fn parse_vcard_content(reader: &dyn FileReader) -> Result<MetadataMap> {
        let size = reader.size() as usize;
        // Read first 8KB to avoid loading huge files entirely into memory
        let content = reader.read(0, size.min(8192))?;

        let text = std::str::from_utf8(content)
            .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8: {}", e)))?;

        let mut metadata = MetadataMap::new();
        let mut has_photo = false;
        let mut has_organization = false;
        let mut has_email = false;
        let mut has_phone = false;
        let mut has_address = false;
        let mut has_url = false;
        let mut vcard_count = 0;

        // Count vCARDs and collect feature flags
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed == "BEGIN:VCARD" {
                vcard_count += 1;
            }
        }

        // A VCF file can hold multiple `BEGIN:VCARD`...`END:VCARD` blocks
        // (this corpus sample has 2), and `VCard.pm`'s `ProcessVCard` reads
        // every one after the first as a new "document"
        // (`$$et{DOC_NUM} = ++$$et{DOC_COUNT}` right after the first
        // `END:VCARD`, VCard.pm:334ish). That matters for which value wins
        // the *default* (non-`-a`) view when two vCards define the same
        // property: `TagSink::record`'s Instance rule (see its own doc
        // comment) means an occurrence recorded under a non-default Instance
        // can never displace a winner recorded under a *different* instance,
        // so the first vCard's value stays the default winner even though a
        // later vCard's occurrence of the same tag is recorded after it --
        // this file's `FormattedName` default view is "Phil Harvey" (vCard
        // 1), never "VCard Test" (vCard 2), which plain last-write-wins would
        // get backwards. `doc_instance` mirrors that: `Instance(0)` (the
        // default) for the first vCard's properties, `Instance(1)` for the
        // second's, and so on.
        let mut doc_instance = Instance::default();
        let mut seen_begin = false;

        // Parse vCard line by line
        for line in text.lines() {
            if line.trim() == "BEGIN:VCARD" {
                if seen_begin {
                    doc_instance = Instance(doc_instance.0 + 1);
                }
                seen_begin = true;
            }
            if let Some((raw_key, raw_value)) = split_property_line(line) {
                // Strip any ";PARAM=..." group parameters from the key so
                // "TEL;TYPE=CELL" still matches on "TEL". We don't yet fold
                // the TYPE into the tag name the way ExifTool does (e.g.
                // "TelephoneCell") -- only the base tag is emitted.
                let key_base = raw_key.split(';').next().unwrap_or(raw_key).trim();
                let key = key_base.to_ascii_uppercase();
                let value = raw_value.trim();

                match key.as_str() {
                    "VERSION" => {
                        metadata.insert(
                            "VCardVersion".to_string(),
                            TagValue::String(value.to_string()),
                        );
                        // Add VCF:Version for Worker 28 compatibility
                        metadata.insert(
                            "VCF:Version".to_string(),
                            TagValue::new_string(value.to_string()),
                        );
                    }
                    "BDAY" => {
                        metadata.insert(
                            "Birthday".to_string(),
                            TagValue::String(convert_vcard_time(value)),
                        );
                    }
                    "TZ" => {
                        metadata
                            .insert("TimeZone".to_string(), TagValue::String(value.to_string()));
                    }
                    "GEO" => {
                        // VCard 4.0 prefixes with "geo:"; ValueConv strips it.
                        let stripped = value.strip_prefix("geo:").unwrap_or(value);
                        metadata.insert(
                            "Geolocation".to_string(),
                            TagValue::String(stripped.to_string()),
                        );
                    }
                    "PHOTO" => {
                        has_photo = true;
                    }
                    "ORG" => {
                        has_organization = true;
                        metadata.insert(
                            "Organization".to_string(),
                            TagValue::String(value.to_string()),
                        );
                    }
                    "ADR" => {
                        has_address = true;
                        metadata.insert("Address".to_string(), TagValue::String(value.to_string()));
                    }
                    "URL" => {
                        has_url = true;
                        metadata.insert("URL".to_string(), TagValue::String(value.to_string()));
                    }
                    "EMAIL" => {
                        has_email = true;
                        metadata.insert("Email".to_string(), TagValue::String(value.to_string()));
                    }
                    "TEL" => {
                        has_phone = true;
                        metadata
                            .insert("Telephone".to_string(), TagValue::String(value.to_string()));
                    }
                    _ => {
                        if let Some(name) = main_table_name(&key) {
                            // `VCard::Main`'s `GROUPS => { 2 => 'Document' }`
                            // leaves family-0/1 at the table default, `VCard`
                            // -- a bare (unprefixed) key instead gets family-1
                            // `""` from `TagOccurrence::from_insert_shim`,
                            // which prints as `-G1`'s empty `[]` bracket:
                            // invisible to `duplicate_loss_scan.py`'s `-a -G1
                            // -s` parser (its `LINE_RE` requires one-or-more
                            // chars inside the brackets). `insert_occurrence`
                            // with the file's own `doc_instance` reproduces
                            // both ExifTool's default-view winner (the first
                            // vCard, per this function's own `doc_instance`
                            // doc comment) and full `-a` retention of every
                            // vCard's occurrence -- unlike the plain
                            // `insert()` this replaced, which retains
                            // occurrences within *this* function's own
                            // `metadata` sink but then lost them anyway the
                            // moment `FormatParser::parse` merged this
                            // `MetadataMap` into the file's real one via
                            // `IntoIterator`, which only carries the winner
                            // projection across (see that impl's own doc
                            // comment) -- fixed below by switching that merge
                            // to `MetadataMap::merge`, which replays every
                            // occurrence instead.
                            // `LANGUAGE=xx` names a *different* tag
                            // (`Note-fr`, not a second `Note`) -- see
                            // `language_param`'s own doc comment.
                            let name = match language_param(raw_key) {
                                Some(lang) => format!("{name}-{lang}"),
                                None => name.to_string(),
                            };
                            // NOTE: the raw value is inserted as-is, not run
                            // through `DecodeVCardText`'s backslash-unescape
                            // (`VCard.pm:227-249`) -- ExifTool's own TEXT
                            // output (unlike `-b`) additionally replaces any
                            // embedded control character the unescape can
                            // produce (a decoded `\n`, in particular) with
                            // `.` before printing (`exiftool`'s own POD,
                            // "-b" section: "control characters ... are not
                            // replaced by '.' as they are in the default
                            // output" -- implying they *are* replaced
                            // outside `-b`). That substitution lives in the
                            // shared CLI text-output formatter this parser
                            // has no access to, and unescaping without it
                            // would plant a real newline into a `TagValue`
                            // that JSON/CSV output would then show verbatim
                            // -- a new, wrong mismatch in those modes to fix
                            // a `-s`-only one. Left as the pre-existing raw
                            // value pending that shared formatter change.
                            metadata.insert_occurrence(
                                format!("VCard:{name}"),
                                TagValue::String(value.to_string()),
                                SHIM_DEFAULT_PRIORITY,
                                "VCard",
                                doc_instance,
                            );
                        }
                    }
                }
            }
        }

        // Add Worker 28 tags for vCard properties
        metadata.insert(
            "VCF:Count".to_string(),
            TagValue::new_integer(vcard_count as i64),
        );

        metadata.insert(
            "VCF:HasPhoto".to_string(),
            TagValue::new_string(if has_photo { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasOrganization".to_string(),
            TagValue::new_string(if has_organization { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasEmail".to_string(),
            TagValue::new_string(if has_email { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasPhone".to_string(),
            TagValue::new_string(if has_phone { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasAddress".to_string(),
            TagValue::new_string(if has_address { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasURL".to_string(),
            TagValue::new_string(if has_url { "true" } else { "false" }),
        );

        Ok(metadata)
    }
}

impl FormatParser for VCFParser {
    /// Parses a VCF file and extracts metadata
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Successfully extracted metadata including FileType, FileSize, and vCard fields
    /// * `Err(ExifToolError)` - Invalid signature or parse error
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid VCF signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert(
            "FileType".to_string(),
            TagValue::String("vCard".to_string()),
        );

        // Parse vCard content and merge with basic metadata. `merge` (not a
        // per-key `for (key, value) in vcard_metadata { insert(...) }` loop)
        // matters here: `MetadataMap`'s `IntoIterator` deliberately only
        // yields the winner projection (see its own doc comment), so a
        // flatten-loop merge would discard every occurrence
        // `parse_vcard_content`'s `insert_occurrence` calls just retained --
        // e.g. `VCard:FormattedName`'s second-vCard occurrence -- the moment
        // it crossed into this function's own map. `merge` replays every
        // occurrence instead, the same fix `MetadataMap::merge`'s own doc
        // comment describes for the general case.
        let vcard_metadata = Self::parse_vcard_content(reader)?;
        metadata.merge(vcard_metadata);

        Ok(metadata)
    }

    /// Indicates whether this parser supports the given file format
    ///
    /// # Arguments
    ///
    /// * `format` - FileFormat to check
    ///
    /// # Returns
    ///
    /// * `true` if format is VCF
    /// * `false` otherwise
    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::VCF)
    }
}

/// Parses metadata from VCF files.
///
/// This is a convenience function that creates a VCFParser and invokes it.
///
/// # Arguments
///
/// * `reader` - FileReader implementation for accessing file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_vcf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = VCFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_vcard_time_handles_date_only() {
        assert_eq!(convert_vcard_time("19900101"), "1990:01:01");
        assert_eq!(convert_vcard_time("1990-01-01"), "1990:01:01");
    }

    #[test]
    fn convert_vcard_time_handles_datetime() {
        assert_eq!(
            convert_vcard_time("20200101T000000Z"),
            "2020:01:01 00:00:00Z"
        );
    }

    #[test]
    fn main_table_name_maps_known_tags() {
        assert_eq!(main_table_name("TITLE"), Some("JobTitle"));
        assert_eq!(main_table_name("PRODID"), Some("Software"));
        assert_eq!(main_table_name("UID"), Some("UID"));
        assert_eq!(main_table_name("BOGUS"), None);
    }

    #[test]
    fn split_property_line_ignores_colons_inside_quoted_params() {
        // RFC 6350 quoted parameter values may contain ":" (e.g. a "geo:"
        // URI), which must not be mistaken for the property/value delimiter.
        let line = r#"ADR;type=OTHER;GEO="geo:12.3457,78.910";LABEL=Test\nLabel:;;Other Rd.;City;ON;K0K0K0;Canada"#;
        let (key, value) = split_property_line(line).expect("line should split");
        assert_eq!(
            key,
            r#"ADR;type=OTHER;GEO="geo:12.3457,78.910";LABEL=Test\nLabel"#
        );
        assert_eq!(value, ";;Other Rd.;City;ON;K0K0K0;Canada");
    }

    #[test]
    fn split_property_line_handles_plain_lines() {
        assert_eq!(
            split_property_line("TEL;type=CELL:555-0000"),
            Some(("TEL;type=CELL", "555-0000"))
        );
        assert_eq!(split_property_line("no-colon-here"), None);
    }

    #[test]
    fn language_param_finds_language_and_ignores_other_params() {
        assert_eq!(language_param("NOTE;LANGUAGE=fr"), Some("fr"));
        assert_eq!(
            language_param("TEL;TYPE=CELL;LANGUAGE=en-us"),
            Some("en-us")
        );
        assert_eq!(language_param("NOTE"), None);
        assert_eq!(language_param("TEL;TYPE=CELL"), None);
    }

    /// A second `BEGIN:VCARD`...`END:VCARD` block redefining a tag the first
    /// block already set must not silently disappear (`-a` retention) and
    /// must not displace the first block's value as the default winner
    /// (`TagSink`'s Instance rule, matching `ProcessVCard`'s `DOC_NUM`
    /// increment) -- this pins both halves of the `VCard.pm` citations in
    /// `parse_vcard_content`'s own doc comment against a minimal fixture
    /// shaped like the corpus sample (`FormattedName` repeats across two
    /// vCards; `NOTE;LANGUAGE=fr` is a distinct `Note-fr` tag, not a third
    /// `Note` occurrence).
    #[test]
    fn a_repeated_tag_across_two_vcards_keeps_both_occurrences_first_wins_default() {
        use crate::test_support::TestReader;

        let text = "BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
FN:Phil Harvey\r\n\
NOTE:Hello\\, world!\r\n\
NOTE;LANGUAGE=fr:Bonjour\r\n\
END:VCARD\r\n\
BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
FN:VCard Test\r\n\
END:VCARD\r\n";
        let reader = TestReader::new(text.as_bytes().to_vec());
        let metadata = VCFParser.parse(&reader).expect("parse should succeed");

        let names: Vec<String> = metadata
            .occurrences_for("VCard:FormattedName")
            .into_iter()
            .map(|o| o.raw.as_string().unwrap_or_default().to_string())
            .collect();
        assert_eq!(names, vec!["Phil Harvey", "VCard Test"]);
        // First vCard's value stays the default-view winner.
        assert_eq!(
            metadata.get_string("VCard:FormattedName"),
            Some("Phil Harvey")
        );

        // `NOTE;LANGUAGE=fr` is `Note-fr`, not a second `Note` occurrence.
        let notes: Vec<String> = metadata
            .occurrences_for("VCard:Note")
            .into_iter()
            .map(|o| o.raw.as_string().unwrap_or_default().to_string())
            .collect();
        assert_eq!(notes, vec!["Hello\\, world!"]);
        assert_eq!(metadata.get_string("VCard:Note-fr"), Some("Bonjour"));
    }
}
