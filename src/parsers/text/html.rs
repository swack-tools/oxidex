//! HTML / XHTML `<head>` meta information parser.
//!
//! A transcription of ExifTool's `HTML.pm` (`ProcessHTML`, ExifTool 13.55).
//! Everything ExifTool reports for an HTML file comes out of the document
//! head: `META` elements, the `TITLE` element, and the MS-Office `XML` island
//! that Word and Excel write inside a `<!--[if gte mso 9]>` conditional
//! comment.
//!
//! # Four family-1 groups from one file
//!
//! A meta name carries its own namespace (`dc:creator`, `ncc:totalTime`,
//! `o:Author`), and ExifTool routes each namespace to a different tag table
//! with a different family-1 group. So a single file yields `HTML:*`,
//! `HTML-dc:*`, `HTML-ncc:*`, `HTML-prod:*`, `HTML-vw96:*`, `HTML-office:*`
//! and `HTTP-equiv:*` tags side by side. The tables below are transcribed
//! from `HTML.pm`'s `%Image::ExifTool::HTML::{Main,dc,ncc,prod,vw96,equiv,Office}`.
//!
//! # Character set
//!
//! Values are decoded using the charset declared by
//! `<meta http-equiv="Content-Type" content="...charset=X">`, and only that:
//! ExifTool maps exactly four charset names and ignores every other one,
//! leaving those values as raw bytes. Because the charset is discovered while
//! walking the head, tags that appear *before* the content-type element are
//! not decoded -- HTML.pm says so explicitly, and this reproduces it rather
//! than "fixing" it.

#![allow(dead_code)]

use std::collections::HashMap;

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::Result;

/// How many bytes ExifTool validates the header against, and the window it
/// determines the input record separator from (`ProcessHTML`).
const HEADER_PROBE: usize = 256;

// ---------------------------------------------------------------------------
// Tag tables (transcribed from HTML.pm)
// ---------------------------------------------------------------------------

/// The conversion a tag carries in `HTML.pm`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conv {
    /// No `ValueConv`/`PrintConv`, or one that is the identity with default
    /// options (`$self->ConvertDateTime($val)` returns `$val` unchanged
    /// unless `-d` was given, which OxiDex does not implement here).
    None,
    /// `ValueConv => 'Image::ExifTool::XMP::ConvertXMPDate($val)'`.
    XmpDate,
    /// `PrintConv => 'ConvertTimeSpan($val, 60)'`.
    TimeSpanMinutes,
    /// `RawConv => \&SetHTMLCharset` -- records the document charset and
    /// returns the value untouched.
    SetCharset,
}

struct TagDef {
    /// ExifTool's tag ID, i.e. the table key. Meta names are looked up
    /// lowercased; the Office table is keyed by the XML element name as
    /// written, so its keys are capitalised.
    key: &'static str,
    /// ExifTool's tag name.
    name: &'static str,
    /// Whether repeated occurrences accumulate into a list.
    list: bool,
    conv: Conv,
}

const fn tag(key: &'static str, name: &'static str) -> TagDef {
    TagDef {
        key,
        name,
        list: false,
        conv: Conv::None,
    }
}

const fn list_tag(key: &'static str, name: &'static str) -> TagDef {
    TagDef {
        key,
        name,
        list: true,
        conv: Conv::None,
    }
}

const fn conv_tag(key: &'static str, name: &'static str, conv: Conv) -> TagDef {
    TagDef {
        key,
        name,
        list: false,
        conv,
    }
}

/// `%Image::ExifTool::HTML::Main` -- the leaf tags only. The `dc`, `ncc`,
/// `prod`, `vw96`, `http-equiv` and `o` entries are `SubDirectory` pointers
/// and are resolved by [`sub_table`].
static MAIN: &[TagDef] = &[
    tag("abstract", "Abstract"),
    tag("author", "Author"),
    tag("classification", "Classification"),
    tag("content-language", "ContentLanguage"),
    tag("copyright", "Copyright"),
    tag("description", "Description"),
    tag("distribution", "Distribution"),
    tag("doc-class", "DocClass"),
    tag("doc-rights", "DocRights"),
    tag("doc-type", "DocType"),
    tag("formatter", "Formatter"),
    tag("generator", "Generator"),
    tag("generatorversion", "GeneratorVersion"),
    tag("googlebot", "GoogleBot"),
    list_tag("keywords", "Keywords"),
    tag("mssmarttagspreventparsing", "NoMSSmartTags"),
    tag("originator", "Originator"),
    tag("owner", "Owner"),
    tag("progid", "ProgID"),
    tag("rating", "Rating"),
    tag("refresh", "Refresh"),
    tag("resource-type", "ResourceType"),
    tag("revisit-after", "RevisitAfter"),
    list_tag("robots", "Robots"),
    tag("title", "Title"),
];

/// `%Image::ExifTool::HTML::dc` -- Dublin Core.
static DC: &[TagDef] = &[
    list_tag("contributor", "Contributor"),
    tag("coverage", "Coverage"),
    list_tag("creator", "Creator"),
    list_tag("date", "Date"),
    tag("description", "Description"),
    tag("format", "Format"),
    tag("identifier", "Identifier"),
    list_tag("language", "Language"),
    list_tag("publisher", "Publisher"),
    list_tag("relation", "Relation"),
    tag("rights", "Rights"),
    tag("source", "Source"),
    list_tag("subject", "Subject"),
    tag("title", "Title"),
    list_tag("type", "Type"),
];

/// `%Image::ExifTool::HTML::ncc` -- Daisy 2.02 navigation control centre.
static NCC: &[TagDef] = &[
    tag("charset", "CharacterSet"),
    tag("depth", "Depth"),
    tag("files", "Files"),
    tag("footnotes", "Footnotes"),
    tag("generator", "Generator"),
    tag("kbytesize", "KByteSize"),
    tag("maxpagenormal", "MaxPageNormal"),
    tag("multimediatype", "MultimediaType"),
    tag("narrator", "Narrator"),
    tag("pagefront", "PageFront"),
    tag("pagenormal", "PageNormal"),
    tag("pagespecial", "PageSpecial"),
    tag("prodnotes", "ProdNotes"),
    tag("produceddate", "ProducedDate"),
    tag("producer", "Producer"),
    tag("revision", "Revision"),
    tag("revisiondate", "RevisionDate"),
    tag("setinfo", "SetInfo"),
    tag("sidebars", "Sidebars"),
    tag("sourcedate", "SourceDate"),
    tag("sourceedition", "SourceEdition"),
    tag("sourcepublisher", "SourcePublisher"),
    tag("sourcerights", "SourceRights"),
    tag("sourcetitle", "SourceTitle"),
    tag("tocitems", "TOCItems"),
    tag("totaltime", "Duration"),
];

/// `%Image::ExifTool::HTML::prod`.
static PROD: &[TagDef] = &[
    tag("recengineer", "RecEngineer"),
    tag("reclocation", "RecLocation"),
];

/// `%Image::ExifTool::HTML::vw96`.
static VW96: &[TagDef] = &[tag("objecttype", "ObjectType")];

/// `%Image::ExifTool::HTML::equiv` -- `HTTP-equiv` meta elements.
static EQUIV: &[TagDef] = &[
    tag("cache-control", "CacheControl"),
    tag("content-disposition", "ContentDisposition"),
    tag("content-language", "ContentLanguage"),
    tag("content-script-type", "ContentScriptType"),
    tag("content-style-type", "ContentStyleType"),
    conv_tag("content-type", "ContentType", Conv::SetCharset),
    tag("default-style", "DefaultStyle"),
    tag("expires", "Expires"),
    tag("ext-cache", "ExtCache"),
    tag("imagetoolbar", "ImageToolbar"),
    tag("lotus", "Lotus"),
    tag("page-enter", "PageEnter"),
    tag("page-exit", "PageExit"),
    tag("pics-label", "PicsLabel"),
    tag("pragma", "Pragma"),
    tag("refresh", "Refresh"),
    tag("reply-to", "ReplyTo"),
    tag("set-cookie", "SetCookie"),
    tag("site-enter", "SiteEnter"),
    tag("site-exit", "SiteExit"),
    tag("vary", "Vary"),
    tag("window-target", "WindowTarget"),
];

/// `%Image::ExifTool::HTML::Office` -- the MS-Office XML island. Keyed by the
/// element name as written, so these keys are matched case-sensitively.
static OFFICE: &[TagDef] = &[
    tag("Author", "Author"),
    tag("Category", "Category"),
    tag("Characters", "Characters"),
    tag("CharactersWithSpaces", "CharactersWithSpaces"),
    tag("Company", "Company"),
    conv_tag("Created", "CreateDate", Conv::XmpDate),
    tag("Description", "Description"),
    tag("Keywords", "Keywords"),
    tag("LastAuthor", "LastAuthor"),
    conv_tag("LastPrinted", "LastPrinted", Conv::XmpDate),
    conv_tag("LastSaved", "ModifyDate", Conv::XmpDate),
    tag("Lines", "Lines"),
    tag("Manager", "Manager"),
    tag("Pages", "Pages"),
    tag("Paragraphs", "Paragraphs"),
    tag("Revision", "RevisionNumber"),
    tag("Subject", "Subject"),
    tag("Template", "Template"),
    conv_tag("TotalTime", "TotalEditTime", Conv::TimeSpanMinutes),
    tag("Version", "RevisionNumber"),
    tag("Words", "Words"),
];

/// The `SubDirectory` entries of the Main table: namespace to
/// (family-1 group, tag table).
fn sub_table(group: &str) -> Option<(&'static str, &'static [TagDef])> {
    match group {
        "dc" => Some(("HTML-dc", DC)),
        "ncc" => Some(("HTML-ncc", NCC)),
        "prod" => Some(("HTML-prod", PROD)),
        "vw96" => Some(("HTML-vw96", VW96)),
        "http-equiv" => Some(("HTTP-equiv", EQUIV)),
        "o" => Some(("HTML-office", OFFICE)),
        _ => None,
    }
}

fn lookup<'a>(table: &'a [TagDef], key: &str) -> Option<&'a TagDef> {
    table.iter().find(|t| t.key == key)
}

// ---------------------------------------------------------------------------
// Character entities (transcribed from HTML.pm's %entityNum)
// ---------------------------------------------------------------------------

/// HTML 4 character entity references, sorted by name for binary search.
/// Transcribed verbatim from `%entityNum` in `HTML.pm` (253 entries).
#[rustfmt::skip]
static ENTITY_NUM: &[(&str, u32)] = &[
    ("AElig", 198), ("Aacute", 193), ("Acirc", 194), ("Agrave", 192),
    ("Alpha", 913), ("Aring", 197), ("Atilde", 195), ("Auml", 196),
    ("Beta", 914), ("Ccedil", 199), ("Chi", 935), ("Dagger", 8225),
    ("Delta", 916), ("ETH", 208), ("Eacute", 201), ("Ecirc", 202),
    ("Egrave", 200), ("Epsilon", 917), ("Eta", 919), ("Euml", 203),
    ("Gamma", 915), ("Iacute", 205), ("Icirc", 206), ("Igrave", 204),
    ("Iota", 921), ("Iuml", 207), ("Kappa", 922), ("Lambda", 923),
    ("Mu", 924), ("Ntilde", 209), ("Nu", 925), ("OElig", 338),
    ("Oacute", 211), ("Ocirc", 212), ("Ograve", 210), ("Omega", 937),
    ("Omicron", 927), ("Oslash", 216), ("Otilde", 213), ("Ouml", 214),
    ("Phi", 934), ("Pi", 928), ("Prime", 8243), ("Psi", 936),
    ("Rho", 929), ("Scaron", 352), ("Sigma", 931), ("THORN", 222),
    ("Tau", 932), ("Theta", 920), ("Uacute", 218), ("Ucirc", 219),
    ("Ugrave", 217), ("Upsilon", 933), ("Uuml", 220), ("Xi", 926),
    ("Yacute", 221), ("Yuml", 376), ("Zeta", 918), ("aacute", 225),
    ("acirc", 226), ("acute", 180), ("aelig", 230), ("agrave", 224),
    ("alefsym", 8501), ("alpha", 945), ("amp", 38), ("and", 8743),
    ("ang", 8736), ("apos", 39), ("aring", 229), ("asymp", 8776),
    ("atilde", 227), ("auml", 228), ("bdquo", 8222), ("beta", 946),
    ("brvbar", 166), ("bull", 8226), ("cap", 8745), ("ccedil", 231),
    ("cedil", 184), ("cent", 162), ("chi", 967), ("circ", 710),
    ("clubs", 9827), ("cong", 8773), ("copy", 169), ("crarr", 8629),
    ("cup", 8746), ("curren", 164), ("dArr", 8659), ("dagger", 8224),
    ("darr", 8595), ("deg", 176), ("delta", 948), ("diams", 9830),
    ("divide", 247), ("eacute", 233), ("ecirc", 234), ("egrave", 232),
    ("empty", 8709), ("emsp", 8195), ("ensp", 8194), ("epsilon", 949),
    ("equiv", 8801), ("eta", 951), ("eth", 240), ("euml", 235),
    ("euro", 8364), ("exist", 8707), ("fnof", 402), ("forall", 8704),
    ("frac12", 189), ("frac14", 188), ("frac34", 190), ("frasl", 8260),
    ("gamma", 947), ("ge", 8805), ("gt", 62), ("hArr", 8660),
    ("harr", 8596), ("hearts", 9829), ("hellip", 8230), ("iacute", 237),
    ("icirc", 238), ("iexcl", 161), ("igrave", 236), ("image", 8465),
    ("infin", 8734), ("int", 8747), ("iota", 953), ("iquest", 191),
    ("isin", 8712), ("iuml", 239), ("kappa", 954), ("lArr", 8656),
    ("lambda", 955), ("lang", 9001), ("laquo", 171), ("larr", 8592),
    ("lceil", 8968), ("ldquo", 8220), ("le", 8804), ("lfloor", 8970),
    ("lowast", 8727), ("loz", 9674), ("lrm", 8206), ("lsaquo", 8249),
    ("lsquo", 8216), ("lt", 60), ("macr", 175), ("mdash", 8212),
    ("micro", 181), ("middot", 183), ("minus", 8722), ("mu", 956),
    ("nabla", 8711), ("nbsp", 160), ("ndash", 8211), ("ne", 8800),
    ("ni", 8715), ("not", 172), ("notin", 8713), ("nsub", 8836),
    ("ntilde", 241), ("nu", 957), ("oacute", 243), ("ocirc", 244),
    ("oelig", 339), ("ograve", 242), ("oline", 8254), ("omega", 969),
    ("omicron", 959), ("oplus", 8853), ("or", 8744), ("ordf", 170),
    ("ordm", 186), ("oslash", 248), ("otilde", 245), ("otimes", 8855),
    ("ouml", 246), ("para", 182), ("part", 8706), ("permil", 8240),
    ("perp", 8869), ("phi", 966), ("pi", 960), ("piv", 982),
    ("plusmn", 177), ("pound", 163), ("prime", 8242), ("prod", 8719),
    ("prop", 8733), ("psi", 968), ("quot", 34), ("rArr", 8658),
    ("radic", 8730), ("rang", 9002), ("raquo", 187), ("rarr", 8594),
    ("rceil", 8969), ("rdquo", 8221), ("real", 8476), ("reg", 174),
    ("rfloor", 8971), ("rho", 961), ("rlm", 8207), ("rsaquo", 8250),
    ("rsquo", 8217), ("sbquo", 8218), ("scaron", 353), ("sdot", 8901),
    ("sect", 167), ("shy", 173), ("sigma", 963), ("sigmaf", 962),
    ("sim", 8764), ("spades", 9824), ("sub", 8834), ("sube", 8838),
    ("sum", 8721), ("sup", 8835), ("sup1", 185), ("sup2", 178),
    ("sup3", 179), ("supe", 8839), ("szlig", 223), ("tau", 964),
    ("there4", 8756), ("theta", 952), ("thetasym", 977), ("thinsp", 8201),
    ("thorn", 254), ("tilde", 732), ("times", 215), ("trade", 8482),
    ("uArr", 8657), ("uacute", 250), ("uarr", 8593), ("ucirc", 251),
    ("ugrave", 249), ("uml", 168), ("upsih", 978), ("upsilon", 965),
    ("uuml", 252), ("weierp", 8472), ("xi", 958), ("yacute", 253),
    ("yen", 165), ("yuml", 255), ("zeta", 950), ("zwj", 8205),
    ("zwnj", 8204),
];

/// `%Image::ExifTool::XMP::charNum` -- the entity set `UnescapeXML` uses when
/// no lookup table is supplied, which is the case on the XML-island path.
static XML_CHAR_NUM: &[(&str, u32)] = &[
    ("amp", 38),
    ("apos", 39),
    ("gt", 62),
    ("lt", 60),
    ("quot", 34),
];

fn entity_value(table: &[(&str, u32)], name: &str) -> Option<u32> {
    table
        .binary_search_by_key(&name, |(n, _)| n)
        .ok()
        .map(|i| table[i].1)
}

/// `Image::ExifTool::XMP::UnescapeXML`, with the entity table supplied by the
/// caller. Unknown entities are left as written, exactly as `UnescapeChar`
/// does when the name is not in the table and is not a numeric reference.
fn unescape(text: &str, table: &[(&str, u32)]) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'&' {
                i += 1;
            }
            out.push_str(&text[start..i]);
            continue;
        }
        // /&(#?\w+);/
        let mut j = i + 1;
        if j < bytes.len() && bytes[j] == b'#' {
            j += 1;
        }
        let name_start = j;
        while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
            j += 1;
        }
        if j == name_start || j >= bytes.len() || bytes[j] != b';' {
            out.push('&');
            i += 1;
            continue;
        }
        let reference = &text[i + 1..j];
        let code = entity_value(table, reference).or_else(|| {
            let digits = reference.strip_prefix('#')?;
            if let Some(hex) = digits
                .strip_prefix('x')
                .or_else(|| digits.strip_prefix('X'))
            {
                u32::from_str_radix(hex, 16).ok()
            } else {
                digits.parse::<u32>().ok()
            }
        });
        match code.and_then(char::from_u32) {
            Some(character) => {
                out.push(character);
                i = j + 1;
            }
            None => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Character sets
// ---------------------------------------------------------------------------

/// The charsets `%htmlCharset` in HTML.pm maps to. Any other declared charset
/// leaves `HTMLCharset` unset, and ExifTool then does not recode at all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Charset {
    /// ExifTool's `Latin`, i.e. cp1252 (`Charset/Latin.pm`).
    Latin,
    Utf8,
    /// `Charset/MacRoman.pm`.
    MacRoman,
}

/// `%Image::ExifTool::Charset::Latin` -- cp1252. Only 0x80..=0x9F differ from
/// Unicode; the table omits 1-byte characters equal to their code point.
#[rustfmt::skip]
const LATIN_HIGH: [u32; 32] = [
    0x20ac, 0x0081, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021,
    0x02c6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008d, 0x017d, 0x008f,
    0x0090, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
    0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0x009d, 0x017e, 0x0178,
];

fn decode_latin(data: &[u8]) -> String {
    data.iter()
        .map(|&byte| {
            let code = if (0x80..0xA0).contains(&byte) {
                LATIN_HIGH[(byte - 0x80) as usize]
            } else {
                u32::from(byte)
            };
            char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER)
        })
        .collect()
}

/// `$et->Decode($val, $$et{HTMLCharset})`.
///
/// With no charset declared -- or one ExifTool does not map -- the bytes are
/// passed through untouched, which is what ExifTool does.
fn decode(data: &[u8], charset: Option<Charset>) -> String {
    match charset {
        Some(Charset::Latin) => decode_latin(data),
        Some(Charset::MacRoman) => crate::parsers::font::ttf::TTFParser::decode_mac_roman(data),
        Some(Charset::Utf8) | None => match std::str::from_utf8(data) {
            Ok(text) => text.to_string(),
            Err(_) => String::from_utf8_lossy(data).into_owned(),
        },
    }
}

/// `SetHTMLCharset` -- `$$et{HTMLCharset} = $htmlCharset{lc $1} if $val =~
/// /charset=['"]?([-\w]+)/`.
fn charset_from_content_type(value: &str) -> Option<Option<Charset>> {
    let bytes = value.as_bytes();
    let position = value.find("charset=")?;
    let mut i = position + "charset=".len();
    if i < bytes.len() && (bytes[i] == b'\'' || bytes[i] == b'"') {
        i += 1;
    }
    let start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-')) {
        i += 1;
    }
    if i == start {
        return None;
    }
    let name = value[start..i].to_ascii_lowercase();
    Some(match name.as_str() {
        "macintosh" => Some(Charset::MacRoman),
        "iso-8859-1" | "windows-1252" => Some(Charset::Latin),
        "utf-8" => Some(Charset::Utf8),
        // An unmapped charset assigns undef, clearing any earlier setting.
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

/// `Image::ExifTool::IsFloat`.
fn is_float(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
        i += 1;
    }
    // (?=\d|\.\d)
    let has_lookahead = match bytes.get(i) {
        Some(b) if b.is_ascii_digit() => true,
        Some(b'.') => bytes.get(i + 1).is_some_and(u8::is_ascii_digit),
        _ => false,
    };
    if !has_lookahead {
        return false;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if matches!(bytes.get(i), Some(b'E') | Some(b'e')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+') | Some(b'-')) {
            i += 1;
        }
        let digits = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits {
            return false;
        }
    }
    i == bytes.len()
}

/// `Image::ExifTool::ConvertTimeSpan($val, 60)`.
///
/// Returns `None` when the seconds branch would need Perl's `%.15g` number
/// stringification for a non-integral value. Printing an approximation of
/// ExifTool's string under ExifTool's tag name would be undetectable
/// downstream, so the tag is dropped instead.
fn convert_time_span_minutes(value: &str) -> Option<String> {
    if !is_float(value) {
        return Some(value.to_string());
    }
    let Ok(raw) = value.parse::<f64>() else {
        return Some(value.to_string());
    };
    if raw == 0.0 {
        return Some(value.to_string());
    }
    let seconds = raw * 60.0;
    if seconds < 60.0 {
        if seconds.fract() != 0.0 {
            return None;
        }
        Some(format!("{} seconds", seconds as i64))
    } else if seconds < 3600.0 {
        // ($mult and $mult >= 60) selects '%d', and the plural is dropped for
        // exactly one minute.
        let plural = if seconds == 60.0 { "" } else { "s" };
        Some(format!("{} minute{}", (seconds / 60.0) as i64, plural))
    } else if seconds < 24.0 * 3600.0 {
        Some(format!("{:.1} hours", seconds / 3600.0))
    } else {
        Some(format!("{:.1} days", seconds / (24.0 * 3600.0)))
    }
}

/// `Image::ExifTool::XMP::ConvertXMPDate($val)`.
fn convert_xmp_date(value: &str) -> String {
    // /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})(:\d{2})?\s*(\S*)$/
    if let Some(converted) = xmp_date_full(value) {
        return converted;
    }
    // elsif ($val =~ /^(\d{4})(-\d{2}){0,2}/) { $val =~ tr/-/:/ }
    if xmp_date_prefix(value) {
        return value.replace('-', ":");
    }
    value.to_string()
}

fn xmp_date_full(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range.clone())
            .is_some_and(|s| s.len() == range.len() && s.iter().all(u8::is_ascii_digit))
    };
    if !(digits(0..4) && bytes.get(4) == Some(&b'-') && digits(5..7) && bytes.get(7) == Some(&b'-'))
    {
        return None;
    }
    if !digits(8..10) || !matches!(bytes.get(10), Some(b'T') | Some(b' ')) {
        return None;
    }
    if !digits(11..13) || bytes.get(13) != Some(&b':') || !digits(14..16) {
        return None;
    }
    let mut i = 16;
    let mut secs = "";
    if bytes.get(i) == Some(&b':') && digits(i + 1..i + 3) {
        secs = &value[i..i + 3];
        i += 3;
    }
    // \s* then (\S*) to end of string: the remainder must hold no interior
    // whitespace once the leading run is consumed.
    let rest = value[i..].trim_start_matches(is_perl_space_char);
    if rest.contains(is_perl_space_char) {
        return None;
    }
    Some(format!(
        "{}:{}:{} {}{}{}",
        &value[0..4],
        &value[5..7],
        &value[8..10],
        &value[11..16],
        secs,
        rest
    ))
}

fn xmp_date_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 4 || !bytes[..4].iter().all(u8::is_ascii_digit) {
        return false;
    }
    true
}

fn is_perl_space_char(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{0b}' | '\u{0c}')
}

fn is_perl_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// `[\w:.-]`, the character class HTML.pm uses for element and meta names.
fn is_name_char(byte: u8) -> bool {
    is_word(byte) || matches!(byte, b':' | b'.' | b'-')
}

// ---------------------------------------------------------------------------
// Header validation and the head section
// ---------------------------------------------------------------------------

/// `ProcessHTML`'s validation gate:
///
/// ```text
///     $buff =~ /^(\xef\xbb\xbf)?\s*<(!DOCTYPE\s+HTML|HTML|\?xml)/i or return 0;
///     $buff =~ /<(!DOCTYPE\s+)?HTML/i or return 0 if $2 eq '?xml';
/// ```
///
/// An XML declaration alone is not enough -- that is what keeps SVG, XMP and
/// plists out.
#[must_use]
pub fn looks_like_html(data: &[u8]) -> bool {
    let head = &data[..data.len().min(HEADER_PROBE)];
    let mut i = 0;
    if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        i = 3;
    }
    while i < head.len() && is_perl_space(head[i]) {
        i += 1;
    }
    if head.get(i) != Some(&b'<') {
        return false;
    }
    i += 1;
    let rest = &head[i..];
    if starts_with_doctype_html(rest) || starts_with_ignore_case(rest, b"html") {
        return true;
    }
    // Only the literal lowercase '?xml' takes the second branch: Perl compares
    // the captured text with `eq`, so `<?XML` skips the extra requirement.
    if rest.starts_with(b"?xml") {
        return contains_html_element(head);
    }
    false
}

fn starts_with_ignore_case(data: &[u8], needle: &[u8]) -> bool {
    data.len() >= needle.len() && data[..needle.len()].eq_ignore_ascii_case(needle)
}

/// `!DOCTYPE\s+HTML`
fn starts_with_doctype_html(data: &[u8]) -> bool {
    if !starts_with_ignore_case(data, b"!DOCTYPE") {
        return false;
    }
    let mut i = "!DOCTYPE".len();
    let start = i;
    while i < data.len() && is_perl_space(data[i]) {
        i += 1;
    }
    i > start && starts_with_ignore_case(&data[i..], b"html")
}

/// `/<(!DOCTYPE\s+)?HTML/i` anywhere in the probe.
fn contains_html_element(head: &[u8]) -> bool {
    head.iter().enumerate().any(|(i, &byte)| {
        byte == b'<' && {
            let rest = &head[i + 1..];
            starts_with_ignore_case(rest, b"html") || starts_with_doctype_html(rest)
        }
    })
}

/// `Image::ExifTool::PostScript::GetInputRecordSeparator`.
fn record_separator(head: &[u8]) -> Option<&'static [u8]> {
    let probe = &head[..head.len().min(HEADER_PROBE)];
    // Perl's pos() after a successful //g match is one past the character.
    let lf = probe
        .iter()
        .position(|&b| b == 0x0A)
        .map_or(999i64, |p| i64::try_from(p + 1).unwrap_or(i64::MAX));
    let cr = probe
        .iter()
        .position(|&b| b == 0x0D)
        .map_or(999i64, |p| i64::try_from(p + 1).unwrap_or(i64::MAX));
    match lf - cr {
        1 => Some(b"\x0d\x0a"),
        -1 => Some(b"\x0a\x0d"),
        d if d > 0 => Some(b"\x0d"),
        d if d < 0 => Some(b"\x0a"),
        _ => None,
    }
}

/// The document head, as `ProcessHTML` assembles it: everything after the
/// first `<head\b`, plus whole records up to and including the one holding
/// `</head>`.
fn head_section<'a>(data: &'a [u8], separator: &[u8]) -> Option<&'a [u8]> {
    let mut start = None;
    let mut position = 0;
    while position < data.len() {
        let end = find(data, separator, position).map_or(data.len(), |i| i + separator.len());
        let line = &data[position..end];
        match start {
            None => {
                // /<head\b/i -- \b after "head" means the next byte is not a
                // word character.
                if let Some(offset) = find_head_tag(line) {
                    start = Some(position + offset);
                }
            }
            Some(begin) => {
                if find_ignore_case(line, b"</head>").is_some() {
                    return Some(&data[begin..end]);
                }
            }
        }
        position = end;
    }
    start.map(|begin| &data[begin..])
}

/// Offset just past a `<head` whose next byte is not a word character.
fn find_head_tag(line: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(index) = find_ignore_case(&line[from..], b"<head") {
        let at = from + index;
        let after = at + b"<head".len();
        if line.get(after).is_none_or(|&b| !is_word(b)) {
            return Some(after);
        }
        from = at + 1;
    }
    None
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

fn find_ignore_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

// ---------------------------------------------------------------------------
// Attribute matching
// ---------------------------------------------------------------------------

/// `/\b<name>\s*=\s*['"]?([\w:.-]+)/si`
fn attr_name_value<'a>(attrs: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let mut from = 0;
    while let Some(index) = find_ignore_case(&attrs[from..], name) {
        let at = from + index;
        from = at + 1;
        // \b: the byte before the first character of `name` must not be a word
        // character when that first character is one.
        if is_word(name[0]) && at > 0 && is_word(attrs[at - 1]) {
            continue;
        }
        let mut i = at + name.len();
        while i < attrs.len() && is_perl_space(attrs[i]) {
            i += 1;
        }
        if attrs.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while i < attrs.len() && is_perl_space(attrs[i]) {
            i += 1;
        }
        if matches!(attrs.get(i), Some(b'\'') | Some(b'"')) {
            i += 1;
        }
        let start = i;
        while i < attrs.len() && is_name_char(attrs[i]) {
            i += 1;
        }
        if i > start {
            return Some(&attrs[start..i]);
        }
    }
    None
}

/// The META `content` attribute:
///
/// ```text
///     $attrs =~ /\bcontent\s*=\s*(['"])(.*?)\1/si or
///     $attrs =~ /\bcontent\s*=\s*(['"]?)([\w:.-]+)/si
/// ```
fn attr_content<'a>(attrs: &'a [u8]) -> Option<&'a [u8]> {
    let mut from = 0;
    while let Some(index) = find_ignore_case(&attrs[from..], b"content") {
        let at = from + index;
        from = at + 1;
        if at > 0 && is_word(attrs[at - 1]) {
            continue;
        }
        let mut i = at + b"content".len();
        while i < attrs.len() && is_perl_space(attrs[i]) {
            i += 1;
        }
        if attrs.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while i < attrs.len() && is_perl_space(attrs[i]) {
            i += 1;
        }
        if let Some(&quote) = attrs.get(i)
            && (quote == b'\'' || quote == b'"')
            && let Some(close) = attrs[i + 1..].iter().position(|&b| b == quote)
        {
            return Some(&attrs[i + 1..i + 1 + close]);
        }
        // Fall back to the unquoted form.
        let mut j = i;
        if matches!(attrs.get(j), Some(b'\'') | Some(b'"')) {
            j += 1;
        }
        let start = j;
        while j < attrs.len() && is_name_char(attrs[j]) {
            j += 1;
        }
        if j > start {
            return Some(&attrs[start..j]);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Name derivation
// ---------------------------------------------------------------------------

/// `$name =~ s/\W+(\w)/\u$1/sg` -- the META path's name for an unknown tag.
fn meta_tag_name(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < bytes.len() {
        if is_word(bytes[i]) {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        let run = i;
        while i < bytes.len() && !is_word(bytes[i]) {
            i += 1;
        }
        match bytes.get(i) {
            Some(&next) => {
                out.push(next.to_ascii_uppercase() as char);
                i += 1;
            }
            // A trailing non-word run has no following \w, so it is not
            // matched and stays as written.
            None => out.push_str(&raw[run..]),
        }
    }
    out
}

/// The XML island's name for an unknown tag:
///
/// ```text
///     my $name = ucfirst $tag;
///     $name =~ s/_x([0-9a-f]{4})_/chr(hex($1))/gie;
///     $name =~ s/\s(.)/\U$1/g;
///     $name =~ tr/-_a-zA-Z0-9//dc;
/// ```
fn xml_tag_name(raw: &str) -> String {
    let mut name = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    if let Some(first) = characters.next() {
        name.extend(first.to_uppercase());
        name.push_str(characters.as_str());
    }

    // _xHHHH_ -> chr(0xHHHH)
    let bytes = name.clone();
    let mut expanded = String::with_capacity(bytes.len());
    let raw_bytes = bytes.as_bytes();
    let mut i = 0;
    while i < raw_bytes.len() {
        if raw_bytes[i] == b'_'
            && i + 7 <= raw_bytes.len()
            && (raw_bytes[i + 1] == b'x' || raw_bytes[i + 1] == b'X')
            && raw_bytes[i + 2..i + 6].iter().all(u8::is_ascii_hexdigit)
            && raw_bytes[i + 6] == b'_'
            && let Ok(code) = u32::from_str_radix(&bytes[i + 2..i + 6], 16)
            && let Some(character) = char::from_u32(code)
        {
            expanded.push(character);
            i += 7;
            continue;
        }
        let character = bytes[i..].chars().next().unwrap_or('\u{fffd}');
        expanded.push(character);
        i += character.len_utf8();
    }

    // s/\s(.)/\U$1/g -- '.' does not match a newline.
    let mut collapsed = String::with_capacity(expanded.len());
    let mut chars = expanded.chars().peekable();
    while let Some(character) = chars.next() {
        if is_perl_space_char(character) && character != '\n' {
            match chars.peek().copied() {
                Some(next) if next != '\n' => {
                    chars.next();
                    collapsed.extend(next.to_uppercase());
                }
                _ => collapsed.push(character),
            }
        } else {
            collapsed.push(character);
        }
    }

    // tr/-_a-zA-Z0-9//dc
    collapsed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Collected {
    values: HashMap<String, Vec<String>>,
}

impl Collected {
    fn add(&mut self, key: String, value: String, list: bool) {
        let entry = self.values.entry(key).or_default();
        if !list {
            entry.clear();
        }
        entry.push(value);
    }

    fn into_metadata(self) -> MetadataMap {
        let mut metadata = MetadataMap::new();
        for (key, mut values) in self.values {
            let value = if values.len() == 1 {
                TagValue::String(values.pop().unwrap_or_default())
            } else {
                TagValue::Array(values.into_iter().map(TagValue::String).collect())
            };
            metadata.insert(key, value);
        }
        metadata
    }
}

struct Walker {
    charset: Option<Charset>,
    separator: Vec<u8>,
    out: Collected,
}

impl Walker {
    /// `$et->HandleTag($table, $tag, $val)` for a resolved tag.
    fn handle(&mut self, group: &str, definition: &TagDef, value: &str) {
        let converted = match definition.conv {
            Conv::None => Some(value.to_string()),
            Conv::XmpDate => Some(convert_xmp_date(value)),
            Conv::TimeSpanMinutes => convert_time_span_minutes(value),
            Conv::SetCharset => {
                if let Some(charset) = charset_from_content_type(value) {
                    self.charset = charset;
                }
                Some(value.to_string())
            }
        };
        if let Some(converted) = converted {
            self.out.add(
                format!("{}:{}", group, definition.name),
                converted,
                definition.list,
            );
        }
    }

    fn handle_unknown(&mut self, group: &str, name: &str, value: &str) {
        if name.is_empty() {
            return;
        }
        self.out
            .add(format!("{}:{}", group, name), value.to_string(), false);
    }

    /// The non-XML value pipeline: recode, collapse record separators, then
    /// unescape HTML character references.
    fn cook(&self, raw: &[u8]) -> String {
        let decoded = decode(raw, self.charset);
        let collapsed = collapse_separators(&decoded, &self.separator);
        unescape(&collapsed, ENTITY_NUM)
    }

    fn walk(&mut self, doc: &[u8]) {
        let mut position = 0;
        while let Some(element) = next_element(doc, position) {
            let Element {
                name_range,
                attrs_range,
                end,
            } = element;
            let tag_name = &doc[name_range.clone()];
            let attrs = &doc[attrs_range];
            let lower = tag_name.to_ascii_lowercase();

            let mut value: &[u8] = b"";
            position = end;
            if attrs.last() == Some(&b'/') {
                // Self-contained XHTML element.
            } else {
                let close = {
                    let mut c = Vec::with_capacity(tag_name.len() + 3);
                    c.extend_from_slice(b"</");
                    c.extend_from_slice(tag_name);
                    c.push(b'>');
                    c
                };
                match find(doc, &close, end) {
                    Some(at) => {
                        value = &doc[end..at];
                        position = at + close.len();
                    }
                    None => {
                        if lower != b"meta" {
                            continue;
                        }
                    }
                }
            }

            if lower == b"meta" {
                self.handle_meta(attrs, value);
            } else if lower == b"xml" {
                self.handle_xml_island(value);
            } else if lower == b"title" {
                let cooked = self.cook(value);
                match lookup(MAIN, "title") {
                    Some(definition) => self.handle("HTML", definition, &cooked),
                    None => self.handle_unknown("HTML", "Title", &cooked),
                }
            }
        }
    }

    fn handle_meta(&mut self, attrs: &[u8], element_value: &[u8]) {
        let raw_name = match attr_name_value(attrs, b"name") {
            Some(name) => String::from_utf8_lossy(name).into_owned(),
            None => match attr_name_value(attrs, b"http-equiv") {
                Some(name) => format!("HTTP-equiv.{}", String::from_utf8_lossy(name)),
                None => return,
            },
        };
        if raw_name.is_empty() {
            return;
        }
        let lower = raw_name.to_ascii_lowercase();

        let raw_value: &[u8] = match attr_content(attrs) {
            Some(content) => content,
            None => {
                if element_value.is_empty() {
                    return;
                }
                element_value
            }
        };
        let cooked = self.cook(raw_value);

        // /^([\w-]+)[:.]([\w-]+)/ -- isolate the namespace.
        let (group, key) = match split_namespace(&lower) {
            Some((namespace, rest)) => match sub_table(namespace) {
                Some((group, table)) => {
                    if let Some(definition) = lookup(table, rest) {
                        self.handle(group, definition, &cooked);
                        return;
                    }
                    (group.to_string(), rest.to_string())
                }
                None => (
                    format!("HTML-{}", namespace),
                    format!("{}.{}", namespace, rest),
                ),
            },
            None => ("HTML".to_string(), lower.clone()),
        };
        if group == "HTML"
            && let Some(definition) = lookup(MAIN, &key)
        {
            self.handle("HTML", definition, &cooked);
            return;
        }
        self.handle_unknown(&group, &meta_tag_name(&raw_name), &cooked);
    }

    /// The MS-Office XML island:
    /// `/<([\w-]+):([\w-]+)(\s.*?)?>([^<]*?)<\/\1:\2>/g`
    fn handle_xml_island(&mut self, xml: &[u8]) {
        let mut position = 0;
        while let Some(node) = next_xml_node(xml, position) {
            position = node.end;
            let namespace = String::from_utf8_lossy(&xml[node.namespace.clone()]).into_owned();
            let key = String::from_utf8_lossy(&xml[node.key.clone()]).into_owned();
            let Some((group, table)) = sub_table(&namespace) else {
                continue;
            };
            let decoded = decode(&xml[node.value.clone()], self.charset);
            let value = unescape(&decoded, XML_CHAR_NUM);
            match lookup(table, &key) {
                Some(definition) => self.handle(group, definition, &value),
                None => self.handle_unknown(group, &xml_tag_name(&key), &value),
            }
        }
    }
}

/// `$val =~ s{\s*$/\s*}{ }sg`
fn collapse_separators(value: &str, separator: &[u8]) -> String {
    let Ok(separator) = std::str::from_utf8(separator) else {
        return value.to_string();
    };
    if !value.contains(separator) {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find(separator) {
        let head = rest[..at].trim_end_matches(is_perl_space_char);
        out.push_str(head);
        out.push(' ');
        rest = rest[at + separator.len()..].trim_start_matches(is_perl_space_char);
    }
    out.push_str(rest);
    out
}

/// `/^([\w-]+)[:.]([\w-]+)/`
fn split_namespace(tag: &str) -> Option<(&str, &str)> {
    let bytes = tag.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (is_word(bytes[i]) || bytes[i] == b'-') {
        i += 1;
    }
    if i == 0 || !matches!(bytes.get(i), Some(b':') | Some(b'.')) {
        return None;
    }
    let separator = i;
    i += 1;
    let start = i;
    while i < bytes.len() && (is_word(bytes[i]) || bytes[i] == b'-') {
        i += 1;
    }
    if i == start {
        return None;
    }
    Some((&tag[..separator], &tag[start..i]))
}

struct Element {
    name_range: std::ops::Range<usize>,
    attrs_range: std::ops::Range<usize>,
    /// Just past the closing `>`.
    end: usize,
}

/// `m{<([\w:.-]+)(.*?)>}sg`
fn next_element(doc: &[u8], from: usize) -> Option<Element> {
    let mut i = from;
    while i < doc.len() {
        if doc[i] != b'<' {
            i += 1;
            continue;
        }
        let name_start = i + 1;
        let mut name_end = name_start;
        while name_end < doc.len() && is_name_char(doc[name_end]) {
            name_end += 1;
        }
        if name_end == name_start {
            i += 1;
            continue;
        }
        let Some(offset) = doc[name_end..].iter().position(|&b| b == b'>') else {
            return None;
        };
        let close = name_end + offset;
        return Some(Element {
            name_range: name_start..name_end,
            attrs_range: name_end..close,
            end: close + 1,
        });
    }
    None
}

struct XmlNode {
    namespace: std::ops::Range<usize>,
    key: std::ops::Range<usize>,
    value: std::ops::Range<usize>,
    end: usize,
}

/// `/<([\w-]+):([\w-]+)(\s.*?)?>([^<]*?)<\/\1:\2>/g`
fn next_xml_node(xml: &[u8], from: usize) -> Option<XmlNode> {
    let mut i = from;
    'outer: while i < xml.len() {
        if xml[i] != b'<' {
            i += 1;
            continue;
        }
        let ns_start = i + 1;
        let mut ns_end = ns_start;
        while ns_end < xml.len() && (is_word(xml[ns_end]) || xml[ns_end] == b'-') {
            ns_end += 1;
        }
        if ns_end == ns_start || xml.get(ns_end) != Some(&b':') {
            i += 1;
            continue;
        }
        let key_start = ns_end + 1;
        let mut key_end = key_start;
        while key_end < xml.len() && (is_word(xml[key_end]) || xml[key_end] == b'-') {
            key_end += 1;
        }
        if key_end == key_start {
            i += 1;
            continue;
        }

        // (\s.*?)? -- the group must start with whitespace, and '.' does not
        // match a newline, so the '>' has to arrive before the next one.
        let content_start = match xml.get(key_end) {
            Some(b'>') => key_end + 1,
            Some(&byte) if is_perl_space(byte) => {
                let mut j = key_end + 1;
                loop {
                    match xml.get(j) {
                        Some(b'>') => break j + 1,
                        Some(b'\n') | None => {
                            i += 1;
                            continue 'outer;
                        }
                        Some(_) => j += 1,
                    }
                }
            }
            _ => {
                i += 1;
                continue;
            }
        };

        // ([^<]*?)</ns:key>
        let mut close = Vec::with_capacity(key_end - ns_start + 4);
        close.extend_from_slice(b"</");
        close.extend_from_slice(&xml[ns_start..key_end]);
        close.push(b'>');
        let Some(at) = find(xml, b"<", content_start) else {
            i += 1;
            continue;
        };
        if !xml[at..].starts_with(&close) {
            i += 1;
            continue;
        }
        return Some(XmlNode {
            namespace: ns_start..ns_end,
            key: key_start..key_end,
            value: content_start..at,
            end: at + close.len(),
        });
    }
    None
}

// ---------------------------------------------------------------------------
// Parser entry points
// ---------------------------------------------------------------------------

/// Reads the `<head>` of an HTML or XHTML document.
pub struct HTMLParser;

impl HTMLParser {
    /// Extract HTML meta information from a whole-file buffer.
    #[must_use]
    pub fn parse_bytes(data: &[u8]) -> MetadataMap {
        if !looks_like_html(data) {
            return MetadataMap::new();
        }
        // ExifTool warns 'Invalid HTML data' and extracts nothing when it
        // cannot determine the record separator.
        let Some(separator) = record_separator(data) else {
            return MetadataMap::new();
        };
        let Some(doc) = head_section(data, separator) else {
            return MetadataMap::new();
        };
        let mut walker = Walker {
            charset: None,
            separator: separator.to_vec(),
            out: Collected::default(),
        };
        walker.walk(doc);
        walker.out.into_metadata()
    }
}

impl FormatParser for HTMLParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let size = reader.size() as usize;
        let data = reader.read(0, size)?;
        Ok(Self::parse_bytes(data))
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::HTML)
    }
}

/// Parses metadata from HTML/XHTML files.
pub fn parse_html_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    HTMLParser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(metadata: &MetadataMap, key: &str) -> Option<String> {
        match metadata.get(key)? {
            TagValue::String(s) => Some(s.clone()),
            other => Some(format!("{:?}", other)),
        }
    }

    #[test]
    fn header_gate_rejects_xml_without_an_html_element() {
        // The XML declaration alone must not claim the file -- this is what
        // keeps SVG, XMP sidecars and XML plists off the HTML parser.
        assert!(!looks_like_html(
            b"<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        ));
        assert!(!looks_like_html(b"<?xpacket begin=\"\" id=\"W5M0Mp\"?>"));
        assert!(!looks_like_html(
            b"<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict/></plist>"
        ));
        assert!(looks_like_html(
            b"<?xml version=\"1.0\"?>\n<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0\">"
        ));
        assert!(looks_like_html(b"<html><head></head></html>"));
        assert!(looks_like_html(b"\xef\xbb\xbf  <!DOCTYPE  HTML>"));
        assert!(!looks_like_html(b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"));
    }

    #[test]
    fn record_separator_matches_getinputrecordseparator() {
        assert_eq!(record_separator(b"a\nb"), Some(&b"\x0a"[..]));
        assert_eq!(record_separator(b"a\r\nb"), Some(&b"\x0d\x0a"[..]));
        assert_eq!(record_separator(b"a\rb"), Some(&b"\x0d"[..]));
        assert_eq!(record_separator(b"a\n\rb"), Some(&b"\x0a\x0d"[..]));
        assert_eq!(record_separator(b"no line breaks"), None);
    }

    /// `ConvertTimeSpan($val, 60)` -- the plural is dropped at exactly one
    /// minute, and the minutes branch prints `%d`, not `%.1f`.
    #[test]
    fn time_span_matches_exiftool() {
        assert_eq!(
            convert_time_span_minutes("1").as_deref(),
            Some("1 minute"),
            "o:TotalTime 1 is ExifTool's '1 minute'"
        );
        assert_eq!(convert_time_span_minutes("2").as_deref(), Some("2 minutes"));
        assert_eq!(
            convert_time_span_minutes("59").as_deref(),
            Some("59 minutes")
        );
        assert_eq!(
            convert_time_span_minutes("60").as_deref(),
            Some("1.0 hours")
        );
        assert_eq!(
            convert_time_span_minutes("90").as_deref(),
            Some("1.5 hours")
        );
        assert_eq!(
            convert_time_span_minutes("1440").as_deref(),
            Some("1.0 days")
        );
        // Not a number: ExifTool returns the value untouched.
        assert_eq!(convert_time_span_minutes("abc").as_deref(), Some("abc"));
        assert_eq!(convert_time_span_minutes("0").as_deref(), Some("0"));
    }

    #[test]
    fn xmp_date_conversion_matches_exiftool() {
        assert_eq!(
            convert_xmp_date("2010-06-28T23:52:00Z"),
            "2010:06:28 23:52:00Z"
        );
        assert_eq!(convert_xmp_date("2010-06-28T23:52Z"), "2010:06:28 23:52Z");
        assert_eq!(convert_xmp_date("2010-06-28"), "2010:06:28");
        // Not a date at all: unchanged.
        assert_eq!(convert_xmp_date("Normal.dotm"), "Normal.dotm");
    }

    #[test]
    fn entity_table_is_sorted_for_binary_search() {
        assert_eq!(ENTITY_NUM.len(), 253);
        assert!(ENTITY_NUM.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(XML_CHAR_NUM.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn unescapes_named_and_numeric_references() {
        assert_eq!(unescape("Greek: &alpha; &beta;", ENTITY_NUM), "Greek: α β");
        assert_eq!(unescape("a&#13;b", XML_CHAR_NUM), "a\rb");
        assert_eq!(unescape("&#x41;&#66;", ENTITY_NUM), "AB");
        // An unknown entity is left exactly as written.
        assert_eq!(
            unescape("&notarealentity;", XML_CHAR_NUM),
            "&notarealentity;"
        );
        // The XML table has only five names, so &alpha; is not one of them.
        assert_eq!(unescape("&alpha;", XML_CHAR_NUM), "&alpha;");
    }

    #[test]
    fn latin_decoding_is_cp1252() {
        // ExifTool's 'Latin' is cp1252, not bare ISO 8859-1: 0x80 is the euro
        // sign, which ISO 8859-1 leaves undefined.
        assert_eq!(decode_latin(&[b'a', 0xE9]), "aé");
        assert_eq!(decode_latin(&[0x80]), "\u{20ac}");
        assert_eq!(decode_latin(&[0x92]), "\u{2019}");
    }

    #[test]
    fn derives_office_names_the_way_exiftool_does() {
        assert_eq!(xml_tag_name("Checked_x0020_by"), "CheckedBy");
        assert_eq!(xml_tag_name("test1"), "Test1");
        assert_eq!(xml_tag_name("my tag"), "MyTag");
    }

    #[test]
    fn parses_the_four_namespaces_of_a_daisy_office_document() {
        // Byte-level fixture: the charset is declared iso-8859-1 and the
        // Category holds a raw 0xE9.
        let mut doc: Vec<u8> = Vec::new();
        doc.extend_from_slice(
            b"<?xml version=\"1.0\" encoding=\"iso-8859-1\"?>\n\
              <!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\">\n\
              <html><head>\n\
              <title>ExifTool HTML Test</title>\n\
              <meta http-equiv=\"Content-type\" content='text/html; charset=\"iso-8859-1\"' />\n\
              <meta name=\"dc:creator\" content=\"Phil Harvey\" />\n\
              <meta name=\"dc:creator\" content=\"Another Creator\" />\n\
              <meta name=\"dc:date\" content=\"2007-30-01\" scheme=\"yyyy-mm-dd\" />\n\
              <meta name=\"dc:subject\" content=\"Greek: &alpha; &beta; &gamma;\" />\n\
              <meta name=\"ncc:totalTime\" content=\"91:27:21\" scheme=\"hh:mm:ss\" />\n\
              <meta name=\"prod:recEngineer\" content=\"P Harvey\" />\n\
              <!--[if gte mso 9]><xml>\n\
              <o:DocumentProperties>\n\
              <o:Category>a cat",
        );
        doc.push(0xE9);
        doc.extend_from_slice(
            b"gory</o:Category>\n\
              <o:Description>a comments&#13;a new line</o:Description>\n\
              <o:TotalTime>1</o:TotalTime>\n\
              <o:Created>2010-06-28T23:52:00Z</o:Created>\n\
              <o:Revision>2</o:Revision>\n\
              <o:Version>12.0</o:Version>\n\
              </o:DocumentProperties>\n\
              <o:CustomDocumentProperties>\n\
              <o:Checked_x0020_by dt:dt=\"string\">Phil</o:Checked_x0020_by>\n\
              </o:CustomDocumentProperties>\n\
              </xml><![endif]-->\n\
              </head></html>\n",
        );

        let metadata = HTMLParser::parse_bytes(&doc);

        // One file, five family-1 groups.
        assert_eq!(
            value(&metadata, "HTML:Title").as_deref(),
            Some("ExifTool HTML Test")
        );
        assert_eq!(
            value(&metadata, "HTTP-equiv:ContentType").as_deref(),
            Some("text/html; charset=\"iso-8859-1\"")
        );
        assert_eq!(
            value(&metadata, "HTML-ncc:Duration").as_deref(),
            Some("91:27:21")
        );
        assert_eq!(
            value(&metadata, "HTML-prod:RecEngineer").as_deref(),
            Some("P Harvey")
        );

        // dc:creator is a Seq, so two META elements collapse into one list.
        assert_eq!(
            metadata.get("HTML-dc:Creator"),
            Some(&TagValue::Array(vec![
                TagValue::String("Phil Harvey".into()),
                TagValue::String("Another Creator".into()),
            ]))
        );
        // dc:date carries only ConvertDateTime, which is the identity here --
        // the malformed date is reported exactly as written.
        assert_eq!(
            value(&metadata, "HTML-dc:Date").as_deref(),
            Some("2007-30-01")
        );
        assert_eq!(
            value(&metadata, "HTML-dc:Subject").as_deref(),
            Some("Greek: α β γ")
        );

        // The Office island: charset-decoded, XML-unescaped, converted.
        assert_eq!(
            value(&metadata, "HTML-office:Category").as_deref(),
            Some("a catégory")
        );
        assert_eq!(
            value(&metadata, "HTML-office:Description").as_deref(),
            Some("a comments\ra new line")
        );
        assert_eq!(
            value(&metadata, "HTML-office:TotalEditTime").as_deref(),
            Some("1 minute")
        );
        assert_eq!(
            value(&metadata, "HTML-office:CreateDate").as_deref(),
            Some("2010:06:28 23:52:00Z")
        );
        // o:Revision and o:Version are both RevisionNumber and neither is a
        // list, so the later element wins.
        assert_eq!(
            value(&metadata, "HTML-office:RevisionNumber").as_deref(),
            Some("12.0")
        );
        // A custom property not in the table gets its name derived.
        assert_eq!(
            value(&metadata, "HTML-office:CheckedBy").as_deref(),
            Some("Phil")
        );
    }

    #[test]
    fn a_document_without_a_head_yields_nothing() {
        let metadata = HTMLParser::parse_bytes(b"<html>\n<body>no head here</body>\n</html>\n");
        assert_eq!(metadata.len(), 0);
    }
}
