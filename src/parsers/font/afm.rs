//! Adobe Font Metrics parser (AFM, ACFM, AMFM).
//!
//! ExifTool reads these through `Font::ProcessAFM` (Font.pm:589-628): the
//! first line names the file type, then each `Keyword value` line is looked
//! up in `%Image::ExifTool::Font::AFM` (Font.pm:361-384). Everything here is
//! transcribed from those two spots; the AFM table is not a binary table, so
//! it has no `src/exiftool_tables` transcription to lean on.
//!
//! Behaviors reproduced exactly:
//!
//! * `Comment` lines accumulate (joined with `\n`) and flush into the
//!   generic `Comment` tag -- an Extra-table tag, family-0 group `File` --
//!   when a non-`Comment` line arrives (Font.pm:604-607, 618-621). A
//!   comment block still pending when reading stops is never flushed,
//!   because the flush only runs at the top of a later iteration.
//! * A `Comment Creation Date: ...` line is re-tagged as `Creation Date`
//!   (Font.pm:611-613), which the AFM table names `CreateDate`
//!   (Font.pm:365). The value is reported verbatim -- the table entry has
//!   no conversion.
//! * A value wrapped in parentheses is unwrapped (Font.pm:614,
//!   `$val =~ s/^\((.*)\)$/$1/`) -- `Notice` values arrive this way.
//! * An unknown keyword starting with `Start` (other than `StartDirection`)
//!   ends parsing (Font.pm:625-626), so `StartCharMetrics` and everything
//!   after it is never read. Known-but-unlisted keywords like `ItalicAngle`
//!   or `FontBBox` are simply skipped: they are real AFM keywords that
//!   `%Font::AFM` does not carry, and ExifTool emits nothing for them.
//! * A line only parses when it still carries its `\r`/`\n` terminator
//!   (Font.pm:609, `/^(\w+)\s+(.*?)[\x0d\x0a]/`), so a final line with no
//!   newline is ignored, as it is by ExifTool.

use crate::core::{FileReader, MetadataMap, TagValue};

/// `%Image::ExifTool::Font::AFM` (Font.pm:361-384). Left side is the AFM
/// keyword, right side the reported name. Where the Perl declares no `Name`,
/// the reported name is the keyword itself (already `ucfirst`).
/// `Creation Date` is synthesized from a `Comment` line before lookup.
const AFM_TAGS: &[(&str, &str)] = &[
    ("Creation Date", "CreateDate"),      // Font.pm:365
    ("FontName", "FontName"),             // Font.pm:366
    ("FullName", "FullName"),             // Font.pm:367
    ("FamilyName", "FontFamily"),         // Font.pm:368
    ("Weight", "Weight"),                 // Font.pm:369
    ("Version", "Version"),               // Font.pm:370
    ("Notice", "Notice"),                 // Font.pm:371
    ("EncodingScheme", "EncodingScheme"), // Font.pm:372
    ("MappingScheme", "MappingScheme"),   // Font.pm:373
    ("EscChar", "EscChar"),               // Font.pm:374
    ("CharacterSet", "CharacterSet"),     // Font.pm:375
    ("Characters", "Characters"),         // Font.pm:376
    ("IsBaseFont", "IsBaseFont"),         // Font.pm:377
    // VVector is commented out in the Perl (Font.pm:378) and stays out here.
    ("IsFixedV", "IsFixedV"),   // Font.pm:379
    ("CapHeight", "CapHeight"), // Font.pm:380
    ("XHeight", "XHeight"),     // Font.pm:381
    ("Ascender", "Ascender"),   // Font.pm:382
    ("Descender", "Descender"), // Font.pm:383
];

/// Whether `header` opens like an AFM-family file:
/// `^Start(Comp|Master)?FontMetrics\s+\d+` (Font.pm:596). Public so
/// detection can gate on the same test the parser does.
#[must_use]
pub fn is_afm_file(header: &[u8]) -> bool {
    afm_file_type(header).is_some()
}

/// The file type the first line declares (Font.pm:596-598):
/// `StartFontMetrics` is AFM, `StartCompFontMetrics` ACFM,
/// `StartMasterFontMetrics` AMFM.
fn afm_file_type(header: &[u8]) -> Option<&'static str> {
    let (file_type, rest) = if let Some(r) = header.strip_prefix(b"StartCompFontMetrics") {
        ("ACFM", r)
    } else if let Some(r) = header.strip_prefix(b"StartMasterFontMetrics") {
        ("AMFM", r)
    } else if let Some(r) = header.strip_prefix(b"StartFontMetrics") {
        ("AFM", r)
    } else {
        return None;
    };
    // `\s+\d+`: at least one whitespace, then at least one digit.
    let ws = rest.iter().take_while(|b| b.is_ascii_whitespace()).count();
    if ws == 0 || !rest.get(ws).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    Some(file_type)
}

/// Splits into lines the way `ReadLine` with
/// `PostScript::GetInputRecordSeparator` does (Font.pm:592-594): the
/// separator is fixed for the whole file from its first occurrence --
/// `\r\n`, `\r` or `\n` -- and stays attached to the line it ends.
fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    let sep: &[u8] = match data.iter().position(|&b| b == b'\r' || b == b'\n') {
        Some(i) if data[i] == b'\r' && data.get(i + 1) == Some(&b'\n') => b"\r\n",
        Some(i) if data[i] == b'\r' => b"\r",
        Some(_) => b"\n",
        None => return vec![data],
    };
    let mut lines = Vec::new();
    let mut start = 0;
    let mut pos = 0;
    while pos + sep.len() <= data.len() {
        if &data[pos..pos + sep.len()] == sep {
            pos += sep.len();
            lines.push(&data[start..pos]);
            start = pos;
        } else {
            pos += 1;
        }
    }
    if start < data.len() {
        lines.push(&data[start..]);
    }
    lines
}

/// `/^(\w+)\s+(.*?)[\x0d\x0a]/` (Font.pm:609). Returns the keyword and the
/// value. The line's terminator must still be present; with one terminator
/// sequence at the end of the line, Perl's greedy `\s+` / lazy `(.*?)`
/// resolve to: value = text between the whitespace run after the keyword
/// and the first `\r`/`\n` (empty when only whitespace follows the keyword).
fn parse_line(line: &[u8]) -> Option<(&str, &str)> {
    let word_end = line
        .iter()
        .position(|&b| !(b.is_ascii_alphanumeric() || b == b'_'))?;
    if word_end == 0 {
        return None;
    }
    let term = line[word_end..]
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')?
        + word_end;
    let rest = &line[word_end..term];
    if rest.is_empty() {
        // `\s+` needs at least one whitespace char; the terminator itself
        // can satisfy it only when another terminator follows (`\r\n`).
        if line[term] == b'\r' && line.get(term + 1) == Some(&b'\n') {
            return Some((std::str::from_utf8(&line[..word_end]).ok()?, ""));
        }
        return None;
    }
    if !rest[0].is_ascii_whitespace() {
        return None;
    }
    let ws_end = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    let tag = std::str::from_utf8(&line[..word_end]).ok()?;
    let val = std::str::from_utf8(&rest[ws_end..]).ok()?;
    Some((tag, val))
}

/// Extract metadata from an Adobe Font Metrics file.
pub fn parse_afm_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let size = usize::try_from(reader.size()).map_err(|_| "AFM file too large")?;
    let data = reader.read(0, size).map_err(|e| e.to_string())?;
    let lines = split_lines(data);

    let file_type = lines
        .first()
        .and_then(|line| afm_file_type(line))
        .ok_or("not an Adobe Font Metrics file")?;

    let mut metadata = MetadataMap::new();
    metadata.insert("FileType", TagValue::new_string(file_type));

    let mut comment: Option<String> = None;

    for line in lines.iter().skip(1) {
        // Font.pm:604-607: flush the accumulated comment when a line does
        // not match /^Comment\s/. (`\s` includes the line terminator, so a
        // bare `Comment\n` line does NOT flush.)
        let is_comment_line = line
            .strip_prefix(b"Comment")
            .is_some_and(|r| r.first().is_some_and(|b| b.is_ascii_whitespace()));
        if !is_comment_line && let Some(text) = comment.take() {
            // FoundTag('Comment', ...) -- the Extra table's Comment,
            // family-0 group File (Font.pm:606).
            metadata.insert("File:Comment", TagValue::String(text));
        }

        let Some((mut tag, mut val)) = parse_line(line) else {
            continue;
        };

        // Font.pm:611-613: a Comment carrying `Creation Date: ...` becomes
        // the `Creation Date` tag. `/^(Creation Date):\s+(.*)/` -- greedy
        // `\s+`, so all leading whitespace after the colon is consumed, and
        // internal spacing in the date survives verbatim.
        if tag == "Comment"
            && let Some(after) = val.strip_prefix("Creation Date:")
            && after.starts_with(|c: char| c.is_ascii_whitespace())
        {
            tag = "Creation Date";
            val = after.trim_start_matches(|c: char| c.is_ascii_whitespace());
        }

        // Font.pm:614: unwrap a parenthesized value.
        if val.len() >= 2 && val.starts_with('(') && val.ends_with(')') {
            val = &val[1..val.len() - 1];
        }

        if tag == "Comment" {
            // Font.pm:616-620: concatenate consecutive comments.
            comment = Some(match comment.take() {
                Some(prev) => format!("{prev}\n{val}"),
                None => val.to_string(),
            });
            continue;
        }

        if let Some((_, name)) = AFM_TAGS.iter().find(|(kw, _)| *kw == tag) {
            metadata.insert(format!("Font:{name}"), TagValue::new_string(val));
        } else if tag.starts_with("Start") && tag != "StartDirection" {
            // Font.pm:625-626: any unknown subsection ends the scan.
            break;
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// The corpus `Font.afm` in miniature. Every expected value is quoted
    /// from `exiftool -G1 -s Font.afm` (13.59):
    /// `[Font] CreateDate : Sat Sep  4 16:12:41 2004`,
    /// `[File] Comment : Generated by pfaedit`, the parenthesized Notice
    /// unwrapped, and nothing at all for ItalicAngle/FontBBox/CharMetrics.
    #[test]
    fn corpus_shaped_afm_decodes_to_oracle_values() {
        let text = "StartFontMetrics 2.0\n\
                    Comment Generated by pfaedit\n\
                    Comment Creation Date: Sat Sep  4 16:12:41 2004\n\
                    FontName NimbusSanL-ReguCondItal\n\
                    FullName Nimbus Sans L Condensed Regular Italic\n\
                    FamilyName Nimbus Sans L Condensed\n\
                    Weight Regular\n\
                    Notice (Copyright (URW)++,Copyright 1999 by (URW)++ Design & Development)\n\
                    ItalicAngle -9.9\n\
                    IsFixedPitch false\n\
                    UnderlinePosition -100\n\
                    UnderlineThickness 50\n\
                    Version 1.06\n\
                    EncodingScheme AdobeStandardEncoding\n\
                    FontBBox -139 -376 1021 1130\n\
                    CapHeight 718\n\
                    XHeight 523\n\
                    Ascender 718\n\
                    Descender -207\n\
                    StartCharMetrics 1\n\
                    C 32 ; WX 228 ; N space ; B 0 0 0 0 ;\n\
                    EndCharMetrics\n";
        let metadata = parse_afm_metadata(&TestReader::from_slice(text.as_bytes())).unwrap();

        let get = |k: &str| {
            metadata
                .get(k)
                .and_then(|v| v.as_string())
                .map(str::to_string)
        };
        assert_eq!(get("FileType").as_deref(), Some("AFM"));
        assert_eq!(get("File:Comment").as_deref(), Some("Generated by pfaedit"));
        // The double space inside the date survives verbatim: the AFM
        // CreateDate has no conversion (Font.pm:365).
        assert_eq!(
            get("Font:CreateDate").as_deref(),
            Some("Sat Sep  4 16:12:41 2004")
        );
        assert_eq!(
            get("Font:FontName").as_deref(),
            Some("NimbusSanL-ReguCondItal")
        );
        assert_eq!(
            get("Font:FullName").as_deref(),
            Some("Nimbus Sans L Condensed Regular Italic")
        );
        assert_eq!(
            get("Font:FontFamily").as_deref(),
            Some("Nimbus Sans L Condensed")
        );
        assert_eq!(get("Font:Weight").as_deref(), Some("Regular"));
        // Parentheses unwrapped (Font.pm:614); inner parens kept.
        assert_eq!(
            get("Font:Notice").as_deref(),
            Some("Copyright (URW)++,Copyright 1999 by (URW)++ Design & Development")
        );
        assert_eq!(get("Font:Version").as_deref(), Some("1.06"));
        assert_eq!(
            get("Font:EncodingScheme").as_deref(),
            Some("AdobeStandardEncoding")
        );
        assert_eq!(get("Font:CapHeight").as_deref(), Some("718"));
        assert_eq!(get("Font:XHeight").as_deref(), Some("523"));
        assert_eq!(get("Font:Ascender").as_deref(), Some("718"));
        assert_eq!(get("Font:Descender").as_deref(), Some("-207"));

        // Real AFM keywords `%Font::AFM` does not carry: nothing emitted.
        for absent in [
            "ItalicAngle",
            "IsFixedPitch",
            "FontBBox",
            "Font:ItalicAngle",
        ] {
            assert!(metadata.get(absent).is_none(), "{absent} must be absent");
        }
    }

    /// `StartCharMetrics` ends the scan (Font.pm:625-626): a known keyword
    /// after it is never read.
    #[test]
    fn subsection_ends_parsing() {
        let text = "StartFontMetrics 4.1\nStartCharMetrics 2\nWeight Bold\n";
        let metadata = parse_afm_metadata(&TestReader::from_slice(text.as_bytes())).unwrap();
        assert!(metadata.get("Font:Weight").is_none());
    }

    /// Consecutive comments concatenate with `\n` (Font.pm:619), and the
    /// block flushes on the first non-Comment line.
    #[test]
    fn consecutive_comments_concatenate() {
        let text = "StartFontMetrics 2.0\nComment one\nComment two\nWeight Bold\n";
        let metadata = parse_afm_metadata(&TestReader::from_slice(text.as_bytes())).unwrap();
        assert_eq!(
            metadata.get("File:Comment"),
            Some(&TagValue::String("one\ntwo".to_string()))
        );
    }

    /// A comment block still pending at EOF is never flushed: the flush
    /// runs only at the top of a later line's iteration (Font.pm:603-607).
    #[test]
    fn trailing_comment_block_is_dropped() {
        let text = "StartFontMetrics 2.0\nWeight Bold\nComment tail\n";
        let metadata = parse_afm_metadata(&TestReader::from_slice(text.as_bytes())).unwrap();
        assert!(metadata.get("File:Comment").is_none());
        assert_eq!(
            metadata.get("Font:Weight"),
            Some(&TagValue::String("Bold".to_string()))
        );
    }

    /// The first line names the sibling formats too (Font.pm:596-598).
    #[test]
    fn sibling_first_lines_name_their_types() {
        for (first, ftyp) in [
            ("StartFontMetrics 2.0", "AFM"),
            ("StartCompFontMetrics 4.1", "ACFM"),
            ("StartMasterFontMetrics 4.1", "AMFM"),
        ] {
            let text = format!("{first}\n");
            let metadata = parse_afm_metadata(&TestReader::from_slice(text.as_bytes())).unwrap();
            assert_eq!(
                metadata.get("FileType"),
                Some(&TagValue::String(ftyp.to_string())),
                "{first}"
            );
        }
        assert!(parse_afm_metadata(&TestReader::from_slice(b"StartFontMetricsX 2\n")).is_err());
        assert!(parse_afm_metadata(&TestReader::from_slice(b"StartFontMetrics\n")).is_err());
    }

    /// Mac (`\r`) line endings: GetInputRecordSeparator fixes the separator
    /// from the first occurrence (PostScript.pm, via Font.pm:592-594).
    #[test]
    fn carriage_return_line_endings_parse() {
        let text = "StartFontMetrics 2.0\rWeight Bold\r";
        let metadata = parse_afm_metadata(&TestReader::from_slice(text.as_bytes())).unwrap();
        assert_eq!(
            metadata.get("Font:Weight"),
            Some(&TagValue::String("Bold".to_string()))
        );
    }

    /// A final line with no terminator never matches Font.pm:609's
    /// `[\x0d\x0a]` and is ignored.
    #[test]
    fn unterminated_final_line_is_ignored() {
        let text = "StartFontMetrics 2.0\nWeight Bold";
        let metadata = parse_afm_metadata(&TestReader::from_slice(text.as_bytes())).unwrap();
        assert!(metadata.get("Font:Weight").is_none());
    }
}
