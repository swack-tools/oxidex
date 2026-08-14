//! PostScript Type 1 font (.pfb, .pfa) metadata parser.
//!
//! ExifTool 13.59 reaches these through `Font::ProcessFont`
//! (Font.pm:828-885), whose PFA/PFB arm is:
//!
//! ```text
//! } elsif ($buff =~ /^(.{6})?%!(PS-(AdobeFont-|Bitstream )|FontType1-)/s) {  # PFA, PFB
//!     $raf->Seek(6,0) and $et->SetFileType('PFB') if $1;
//!     require Image::ExifTool::PostScript;
//!     $rtnVal = Image::ExifTool::PostScript::ProcessPS($et, $dirInfo);
//! ```
//!
//! (Font.pm:840-843.) The optional six-byte prefix is a PFB segment header;
//! when it is there the file is PFB and PostScript parsing starts past it,
//! and when it is not the file is PFA and parsing starts at zero.
//!
//! `ProcessPS` (PostScript.pm:415-768) is a general PostScript reader, but
//! the font arm of it is narrow and self-contained: PostScript.pm:452-457
//! selects `Font::PSInfo` as a second tag table and turns on comment
//! accumulation, and PostScript.pm:693-717 is the only branch those two
//! reach. This parser implements that arm -- DSC comments into
//! `PostScript::Main`, the leading `%` comment block into `File:Comment`, and
//! the initial `/FontInfo` dictionary into `Font::PSInfo` -- and nothing
//! else.
//!
//! # Deliberately absent
//!
//! `ProcessPS`'s other branches do not apply to a Type 1 font and are not
//! implemented: the DOS `\xc5\xd0\xd3\xc6` binary header and its TIFF preview
//! (PostScript.pm:437-447, 490-514), `%%BeginPhotoshop` / `%%BeginICCProfile`
//! / `%%Begin_xml_packet` embedded blocks (PostScript.pm:583-599),
//! `%%BeginDocument` nesting (PostScript.pm:601-625) and `%AI12_CompressedData`
//! (PostScript.pm:645-660). A PFB reaching any of them would be a PostScript
//! program with an embedded document, which `Font::PSInfo` says nothing about
//! -- and guessing at one is what this project rates worse than omitting it.
//!
//! Two `Font::PSInfo` tags the fixture does not carry are still declared
//! below, because omitting a *declared* tag would under-report a real one:
//! `Copyright` (Font.pm:327) and `FontType` (Font.pm:331).
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Font.pm`,
//!   `lib/Image/ExifTool/PostScript.pm`

use crate::core::{FileReader, MetadataMap, TagValue};

/// Font.pm:836, `$raf->Read($buff, 24)` -- the dispatch peek.
const DISPATCH_PEEK: usize = 24;
/// Font.pm:840-841: the `(.{6})?` prefix, i.e. a PFB segment header.
const PFB_SEGMENT_HEADER_LEN: usize = 6;

/// PostScript.pm:451, `$data =~ /^%!(PS-(AdobeFont-|Bitstream )|FontType1-)/`.
fn is_font_program(data: &[u8]) -> bool {
    data.starts_with(b"%!PS-AdobeFont-")
        || data.starts_with(b"%!PS-Bitstream ")
        || data.starts_with(b"%!FontType1-")
}

/// `%Image::ExifTool::PostScript::Main`'s DSC comment tags
/// (PostScript.pm:31-56). Only the plain-comment entries are listed: the
/// `SubDirectory` and `AI*` entries belong to branches this parser does not
/// implement, and `TIFFPreview`/`EmbeddedFileName` are not real comment IDs.
const PS_MAIN: &[(&str, &str)] = &[
    ("Author", "Author"),           // PostScript.pm:32
    ("BoundingBox", "BoundingBox"), // PostScript.pm:33
    ("Copyright", "Copyright"),     // PostScript.pm:34
    ("CreationDate", "CreateDate"), // PostScript.pm:35-42
    ("Creator", "Creator"),         // PostScript.pm:43
    ("ImageData", "ImageData"),     // PostScript.pm:44
    ("For", "For"),                 // PostScript.pm:45
    ("Keywords", "Keywords"),       // PostScript.pm:46
    ("ModDate", "ModifyDate"),      // PostScript.pm:47-54
    ("Pages", "Pages"),             // PostScript.pm:55
    ("Routing", "Routing"),         // PostScript.pm:56
    ("Subject", "Subject"),         // PostScript.pm:57
    ("Title", "Title"),             // PostScript.pm:58
    ("Version", "Version"),         // PostScript.pm:59
];

/// `%Image::ExifTool::Font::PSInfo` (Font.pm:317-333). Where no `Name` is
/// declared, the reported name is `AddTagToTable`'s `ucfirst $tagID`
/// (ExifTool.pm:9254-9257) -- which is why `version` reports as `Version` and
/// `isFixedPitch` as `IsFixedPitch`.
const PS_INFO: &[(&str, &str)] = &[
    ("FullName", "FullName"),                     // Font.pm:320
    ("FamilyName", "FontFamily"),                 // Font.pm:321
    ("Weight", "Weight"),                         // Font.pm:322
    ("ItalicAngle", "ItalicAngle"),               // Font.pm:323
    ("isFixedPitch", "IsFixedPitch"),             // Font.pm:324
    ("UnderlinePosition", "UnderlinePosition"),   // Font.pm:325
    ("UnderlineThickness", "UnderlineThickness"), // Font.pm:326
    ("Copyright", "Copyright"),                   // Font.pm:327
    ("Notice", "Notice"),                         // Font.pm:328
    ("version", "Version"),                       // Font.pm:329
    ("FontName", "FontName"),                     // Font.pm:330
    ("FontType", "FontType"),                     // Font.pm:331
    ("FSType", "FSType"),                         // Font.pm:332
];

fn lookup<'a>(table: &'a [(&str, &'a str)], id: &str) -> Option<&'a str> {
    table
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, name)| *name)
}

/// Whether `header` is a PostScript Type 1 font program, with or without the
/// PFB segment header -- i.e. Font.pm:840's whole regex. Public so
/// `detection` can gate on the same test the parser does.
#[must_use]
pub fn is_type1_font_program(header: &[u8]) -> bool {
    font_program_start(header).is_some()
}

/// Where the PostScript program starts: `0` for a PFA, or
/// [`PFB_SEGMENT_HEADER_LEN`] for a PFB (Font.pm:841, `$raf->Seek(6,0)`).
/// `None` when this is neither.
fn font_program_start(header: &[u8]) -> Option<usize> {
    if is_font_program(header) {
        return Some(0);
    }
    if header.len() > PFB_SEGMENT_HEADER_LEN && is_font_program(&header[PFB_SEGMENT_HEADER_LEN..]) {
        return Some(PFB_SEGMENT_HEADER_LEN);
    }
    None
}

/// Extract PostScript Type 1 font metadata.
pub fn parse_pfb_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let peek_len = DISPATCH_PEEK.min(reader.size() as usize);
    let peek = reader
        .read(0, peek_len)
        .map_err(|error| error.to_string())?;
    let start = font_program_start(&peek).ok_or("not a PostScript Type 1 font")?;

    let size = usize::try_from(reader.size()).map_err(|_| "font file is too large")?;
    let data = reader.read(0, size).map_err(|error| error.to_string())?;
    let mut metadata = MetadataMap::new();
    parse_font_program(&data[start..], &mut metadata);
    Ok(metadata)
}

/// PostScript.pm:524-768's line loop, restricted to the font arm.
fn parse_font_program(data: &[u8], metadata: &mut MetadataMap) {
    let lines = split_lines(data);
    // PostScript.pm:456, `$comment = 1` -- a Perl truth value used as the
    // accumulator's seed, which then gets stringified into the value. The
    // leading `1` in ExifTool's own `Comment` output for a PFB is that seed,
    // not data from the file, so reproducing the tag means reproducing it.
    let mut comment: Option<String> = Some("1".to_string());
    // PostScript.pm:717, `undef $fontTable` at `currentdict end`: only the
    // *initial* FontInfo dictionary is read.
    let mut font_table_live = true;
    // PostScript.pm:483 consumes the first line to decide EPS/AI/PS before
    // the loop starts, so the `%!PS-AdobeFont-...` line never reaches it.
    let mut index = 1;

    while index < lines.len() {
        let line = lines[index];
        index += 1;

        // PostScript.pm:637, the DSC comment branch. Checked before the font
        // branch because PostScript.pm reaches it in an earlier `elsif`.
        if let Some((id, value)) = dsc_comment(line)
            && let Some(name) = lookup(PS_MAIN, id)
        {
            // PostScript.pm:640, `next unless $data =~ /^%(%|AI\d+_)/ or
            // $tag eq 'ImageData'`.
            if line.starts_with(b"%%") || id == "ImageData" {
                let decoded = decode_comment(value, &lines, &mut index);
                metadata.insert(format!("PostScript:{name}"), TagValue::new_string(decoded));
            }
            continue;
        }

        if !font_table_live {
            continue;
        }

        // PostScript.pm:694-703, the leading comment block.
        if comment.is_some() {
            if let Some(text) = leading_comment(line) {
                let accumulated = comment.as_mut().expect("checked above");
                if !accumulated.is_empty() {
                    accumulated.push('\n');
                }
                accumulated.push_str(&text);
                continue;
            }
            if !line.starts_with(b"%") {
                // PostScript.pm:702, `$et->FoundTag('Comment', $comment) if
                // length $comment` -- an ungrouped Extra tag, i.e. `File:`.
                if let Some(accumulated) = comment.take()
                    && !accumulated.is_empty()
                {
                    metadata.insert("File:Comment", TagValue::new_string(accumulated));
                }
            }
        }

        // PostScript.pm:706-714, the FontInfo dictionary.
        if let Some((id, rest)) = font_dict_entry(line) {
            if let Some(name) = lookup(PS_INFO, &id) {
                metadata.insert(
                    format!("Font:{name}"),
                    TagValue::new_string(font_dict_value(rest)),
                );
            }
        } else if line.starts_with(b"currentdict end") {
            font_table_live = false;
        }
    }
}

/// PostScript.pm:495-523's newline handling, which reduces to: a line ends at
/// the first `\r` or `\n`, `\r\n` counts as one terminator, and the
/// terminator stays on the line (PostScript.pm:695 and :315 both test for
/// it).
fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut pos = 0;
    while pos < data.len() {
        if data[pos] == b'\n' {
            pos += 1;
        } else if data[pos] == b'\r' {
            pos += 1;
            if data.get(pos) == Some(&b'\n') {
                pos += 1;
            }
        } else {
            pos += 1;
            continue;
        }
        lines.push(&data[start..pos]);
        start = pos;
    }
    if start < data.len() {
        lines.push(&data[start..]);
    }
    lines
}

/// PostScript.pm:637, `$data =~ /^%%?(\w+): ?(.*)/s`.
fn dsc_comment(line: &[u8]) -> Option<(&str, &[u8])> {
    let after_percent = line.strip_prefix(b"%")?;
    let body = after_percent.strip_prefix(b"%").unwrap_or(after_percent);
    let id_len = body
        .iter()
        .position(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))?;
    if id_len == 0 || body.get(id_len) != Some(&b':') {
        return None;
    }
    let id = std::str::from_utf8(&body[..id_len]).ok()?;
    // ` ?` -- at most one space is consumed.
    let mut value = &body[id_len + 1..];
    if value.first() == Some(&b' ') {
        value = &value[1..];
    }
    Some((id, value))
}

/// PostScript.pm:695, `$data =~ /^%\s+(.*?)[\x0d\x0a]/`.
///
/// The trailing character class is part of the match, so a final line with no
/// newline does not produce a comment at all -- which is why the terminator is
/// kept by [`split_lines`].
fn leading_comment(line: &[u8]) -> Option<String> {
    let rest = line.strip_prefix(b"%")?;
    let text_start = rest
        .iter()
        .position(|byte| !byte.is_ascii_whitespace() || *byte == b'\r' || *byte == b'\n')?;
    if text_start == 0 {
        return None;
    }
    let text = &rest[text_start..];
    // `(.*?)` is non-greedy and `.` does not match a newline, so the capture
    // runs to the first `\r` or `\n`.
    let end = text
        .iter()
        .position(|byte| *byte == b'\r' || *byte == b'\n')?;
    Some(String::from_utf8_lossy(&text[..end]).into_owned())
}

/// PostScript.pm:706, `$data =~ m{^\s*/(\w+)\s*(.*)}`.
fn font_dict_entry(line: &[u8]) -> Option<(String, &[u8])> {
    let trimmed = line
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|start| &line[start..])?;
    let rest = trimmed.strip_prefix(b"/")?;
    let id_len = rest
        .iter()
        .position(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
        .unwrap_or(rest.len());
    if id_len == 0 {
        return None;
    }
    let id = std::str::from_utf8(&rest[..id_len]).ok()?.to_string();
    let after = &rest[id_len..];
    let value_start = after
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(after.len());
    Some((id, &after[value_start..]))
}

/// PostScript.pm:708-712:
///
/// ```text
/// if ($val =~ /^\((.*)\)/) {
///     $val = UnescapePostScript($1);
/// } elsif ($val =~ m{/?(\S+)}) {
///     $val = $1;
/// }
/// ```
fn font_dict_value(rest: &[u8]) -> String {
    if rest.first() == Some(&b'(') {
        // `(.*)\)` is greedy: the capture runs to the *last* `)` on the line.
        if let Some(end) = rest.iter().rposition(|byte| *byte == b')') {
            return unescape_postscript(&rest[1..end]);
        }
    }
    // `m{/?(\S+)}` is unanchored, but `rest` has already had its leading
    // whitespace stripped, so it matches at position 0.
    let body = rest.strip_prefix(b"/").unwrap_or(rest);
    let end = body
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(body.len());
    String::from_utf8_lossy(&body[..end]).into_owned()
}

/// `UnescapePostScript` (PostScript.pm:380-409).
///
/// `\` + an octal digit takes up to two more octal digits and becomes that
/// byte; `\` + a newline (or CR/LF) is a line continuation and vanishes;
/// `\n`, `\r`, `\t`, `\b`, `\f` become their control characters; anything
/// else `\x` becomes `x`.
fn unescape_postscript(value: &[u8]) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        if value[i] != b'\\' || i + 1 >= value.len() {
            out.push(value[i]);
            i += 1;
            continue;
        }
        let c = value[i + 1];
        i += 2;
        match c {
            b'0'..=b'7' => {
                let mut octal = u32::from(c - b'0');
                let mut digits = 0;
                while digits < 2
                    && let Some(&next) = value.get(i)
                    && (b'0'..=b'7').contains(&next)
                {
                    octal = octal * 8 + u32::from(next - b'0');
                    i += 1;
                    digits += 1;
                }
                // `chr(oct($c) & 0xff)`.
                out.push((octal & 0xff) as u8);
            }
            b'\r' => {
                if value.get(i) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'\n' => {}
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            other => out.push(other),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `DecodeComment` (PostScript.pm:312-374), minus the `$dataPt` accumulation
/// its EPS-rewriting caller uses.
///
/// Consumes `%%+` continuation lines from `lines`, advancing `index` past
/// them, then unwraps a fully-bracketed value. ExifTool splits a bracketed
/// value into a *list* when it contains several balanced `(...)` groups
/// (PostScript.pm:337-354); the joined rendering below is ExifTool's own
/// `-s` list separator.
fn decode_comment(value: &[u8], lines: &[&[u8]], index: &mut usize) -> String {
    let mut val = strip_eol(value).to_vec();
    // PostScript.pm:317-323.
    while let Some(next) = lines.get(*index) {
        if !next.starts_with(b"%%+") {
            break;
        }
        val.extend_from_slice(strip_eol(&next[3..]));
        *index += 1;
    }

    // PostScript.pm:326, `if ($val =~ s/^\((.*)\)$/$1/)` -- anchored at both
    // ends, so a value that merely starts with `(` is left alone.
    if !(val.first() == Some(&b'(') && val.last() == Some(&b')') && val.len() >= 2) {
        return String::from_utf8_lossy(&val).into_owned();
    }
    let inner = &val[1..val.len() - 1];

    // PostScript.pm:328-355's nesting split.
    let mut parts: Vec<Vec<u8>> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let mut nesting = 1usize;
    let mut i = 0;
    while i < inner.len() {
        let byte = inner[i];
        if byte != b'(' && byte != b')' {
            current.push(byte);
            i += 1;
            continue;
        }
        // PostScript.pm:332-337: an odd run of preceding backslashes escapes
        // the bracket.
        let mut backslashes = 0;
        let mut back = i;
        while back > 0 && inner[back - 1] == b'\\' {
            back -= 1;
            backslashes += 1;
        }
        if backslashes % 2 == 1 {
            current.push(byte);
            i += 1;
            continue;
        }
        if byte == b'(' {
            nesting += 1;
            current.push(byte);
            i += 1;
            continue;
        }
        nesting -= 1;
        if nesting > 0 {
            current.push(byte);
            i += 1;
            continue;
        }
        parts.push(std::mem::take(&mut current));
        i += 1;
        // PostScript.pm:352, `++$nesting if $val =~ s/\s*\(//` -- a following
        // bracketed group starts a new list item.
        let mut skip = i;
        while matches!(inner.get(skip), Some(byte) if byte.is_ascii_whitespace()) {
            skip += 1;
        }
        if inner.get(skip) == Some(&b'(') {
            nesting += 1;
            i = skip + 1;
        }
    }
    parts.push(current);

    parts
        .iter()
        // PostScript.pm:357-372: the same escape decoding as
        // `UnescapePostScript` minus its line-continuation cases.
        .map(|part| unescape_bracketed(part))
        .collect::<Vec<_>>()
        .join(", ")
}

/// PostScript.pm:351-372's escape decoding, which differs from
/// [`unescape_postscript`] in exactly one way: `\` + newline is *not* a line
/// continuation here, so it decodes through the `tr/nrtbf/` fallback like any
/// other escaped character.
fn unescape_bracketed(value: &[u8]) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        if value[i] != b'\\' || i + 1 >= value.len() {
            out.push(value[i]);
            i += 1;
            continue;
        }
        let c = value[i + 1];
        i += 2;
        match c {
            b'0'..=b'7' => {
                let mut octal = u32::from(c - b'0');
                let mut digits = 0;
                while digits < 2
                    && let Some(&next) = value.get(i)
                    && (b'0'..=b'7').contains(&next)
                {
                    octal = octal * 8 + u32::from(next - b'0');
                    i += 1;
                    digits += 1;
                }
                out.push((octal & 0xff) as u8);
            }
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            other => out.push(other),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// PostScript.pm:315, `$val =~ s/\x0d*\x0a*$//`.
fn strip_eol(value: &[u8]) -> &[u8] {
    let mut end = value.len();
    while end > 0 && value[end - 1] == b'\n' {
        end -= 1;
    }
    while end > 0 && value[end - 1] == b'\r' {
        end -= 1;
    }
    &value[..end]
}
