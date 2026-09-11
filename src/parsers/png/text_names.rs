//! Tag naming for PNG textual-data chunks (tEXt / zTXt / iTXt), transcribed
//! from ExifTool 13.59.
//!
//! ExifTool names a text chunk's tag from its keyword through
//! `FoundPNG` (`PNG.pm:908-927`, `:1115-1124`):
//!
//! 1. the keyword (suffixed `-<lang>` for an iTXt with a language tag, the
//!    tag first normalised by `StandardLangCase`, `PNG.pm:796-802`) is looked
//!    up in `%Image::ExifTool::PNG::TextualData` (`PNG.pm:592-763`), then
//!    its `ucfirst` form (`:919-921`, "some software forgets to capitalize
//!    first letter");
//! 2. with a language tag and no hit, the bare keyword is looked up the same
//!    way and, when found, wrapped by `PNG::GetLangInfo` (`:890-899`, which
//!    refuses SubDirectory entries) into a `<Name>-<lang>` copy through the
//!    core `GetLangInfo` (`Writer.pl:4106-4126`);
//! 3. otherwise the keyword becomes a new tag: whitespace runs are removed
//!    and the character after each is upper-cased (`:1117`), `Raw profile
//!    type ` keywords are flagged Binary (`:1122`), and `AddTagToTable`
//!    (`ExifTool.pm:9228-9270`) strips everything but `-_a-zA-Z0-9`,
//!    `ucfirst`s, and prefixes `Tag` when the result is shorter than two
//!    characters or does not start with a letter (`:9256-9265`). The new
//!    entry is registered under the raw keyword (`:1124`), which is why the
//!    *first* sighting of an unknown keyword with a language tag prints the
//!    bare name while every later one prints the `-<lang>` form.
//!
//! The dynamic registrations (steps 2 and 3) make naming order-dependent
//! within one ExifTool process; `conformance.py` runs the oracle one process
//! per file, so [`TextTagNamer`] carries that state per parsed file.

use crate::core::TagValue;
use crate::parsers::png::chunk_parser::{PngTextRecord, TextPayload};
use crate::parsers::text::html::decode_latin;
use std::collections::{HashMap, HashSet};

/// The value conversion a `%TextualData` entry applies before printing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextConv {
    /// Plain string.
    None,
    /// `RawConv => \&ConvertPNGDate` (`PNG.pm:626`, sub at `:833-857`).
    PngDate,
    /// `ValueConv => Image::ExifTool::XMP::ConvertXMPDate($val)`
    /// (`PNG.pm:664`, `:674`; sub at `XMP.pm:3382-3393`).
    XmpDate,
}

/// What ExifTool does with the payload of a resolved keyword.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryKind {
    /// A text tag (possibly with a conversion).
    Text,
    /// `XML:com.adobe.xmp`: `SubDirectory => XMP::Main` (`PNG.pm:680-688`).
    Xmp,
    /// `Raw profile type <x>`: `SubDirectory` through `ProcessProfile`
    /// (`PNG.pm:689-762`, sub at `:1155-1282`).
    Profile,
}

/// One row of `%Image::ExifTool::PNG::TextualData` (`PNG.pm:592-763`).
struct Known {
    keyword: &'static str,
    name: &'static str,
    conv: TextConv,
    kind: EntryKind,
}

const fn text(keyword: &'static str, name: &'static str) -> Known {
    Known {
        keyword,
        name,
        conv: TextConv::None,
        kind: EntryKind::Text,
    }
}

const fn profile(keyword: &'static str, name: &'static str) -> Known {
    Known {
        keyword,
        name,
        conv: TextConv::None,
        kind: EntryKind::Profile,
    }
}

/// `%Image::ExifTool::PNG::TextualData`, `PNG.pm:618-762`. Entries without
/// an explicit `Name` take `ucfirst` of the key (`ExifTool.pm:9256-9257`),
/// which is the key itself for every such row here except `parameters`.
const TEXTUAL_DATA: &[Known] = &[
    text("Title", "Title"),
    text("Author", "Author"),
    text("Description", "Description"),
    text("Copyright", "Copyright"),
    Known {
        keyword: "Creation Time",
        name: "CreationTime",
        conv: TextConv::PngDate,
        kind: EntryKind::Text,
    },
    text("Software", "Software"),
    text("Disclaimer", "Disclaimer"),
    // "change name to differentiate from ExifTool Warning" (PNG.pm:640-641)
    text("Warning", "PNGWarning"),
    text("Source", "Source"),
    text("Comment", "Comment"),
    text("Collection", "Collection"),
    text("Artist", "Artist"),
    text("Document", "Document"),
    text("Label", "Label"),
    text("Make", "Make"),
    text("Model", "Model"),
    text("parameters", "Parameters"),
    text("aesthetic_score", "AestheticScore"),
    Known {
        keyword: "create-date",
        name: "CreateDate",
        conv: TextConv::XmpDate,
        kind: EntryKind::Text,
    },
    Known {
        keyword: "modify-date",
        name: "ModDate",
        conv: TextConv::XmpDate,
        kind: EntryKind::Text,
    },
    text("TimeStamp", "TimeStamp"),
    text("URL", "URL"),
    Known {
        keyword: "XML:com.adobe.xmp",
        name: "XMP",
        conv: TextConv::None,
        kind: EntryKind::Xmp,
    },
    profile("Raw profile type APP1", "APP1_Profile"),
    profile("Raw profile type exif", "EXIF_Profile"),
    profile("Raw profile type icc", "ICC_Profile"),
    profile("Raw profile type icm", "ICC_Profile"),
    profile("Raw profile type iptc", "IPTC_Profile"),
    profile("Raw profile type xmp", "XMP_Profile"),
    profile("Raw profile type 8bim", "Photoshop_Profile"),
];

/// `%Image::ExifTool::specialTags` (`ExifTool.pm:1230-1236`): table keys
/// that hold table metadata, not tags. `GetTagInfoList` finds no tag under
/// them (`ExifTool.pm:9130-9132`, warning "conflicts with internal ExifTool
/// variable") and `AddTagToTable` never registers one (`:9269`), so a text
/// keyword equal to one of these is an unknown tag on every sighting and
/// never gains a language copy.
const SPECIAL_TAGS: [&str; 28] = [
    "TABLE_NAME",
    "SHORT_NAME",
    "PROCESS_PROC",
    "WRITE_PROC",
    "CHECK_PROC",
    "GROUPS",
    "FORMAT",
    "FIRST_ENTRY",
    "TAG_PREFIX",
    "PRINT_CONV",
    "WRITABLE",
    "TABLE_DESC",
    "NOTES",
    "IS_OFFSET",
    "IS_SUBDIR",
    "EXTRACT_UNKNOWN",
    "NAMESPACE",
    "PREFERRED",
    "SRC_TABLE",
    "PRIORITY",
    "AVOID",
    "WRITE_GROUP",
    "LANG_INFO",
    "VARS",
    "DATAMEMBER",
    "SET_GROUP1",
    "PERMANENT",
    "INIT_TABLE",
];

fn is_special_tag(id: &str) -> bool {
    SPECIAL_TAGS.contains(&id)
}

/// A resolved table entry: a static `%TextualData` row or a run-time
/// registration.
#[derive(Clone, Debug)]
struct Entry {
    /// The table key (`$$tagInfo{TagID}`), needed to build the
    /// `<TagID>-<lang>` key of a language copy (`Writer.pl:4111`).
    tag_id: String,
    name: String,
    conv: TextConv,
    /// `Binary => 1` (`PNG.pm:1122`): the value prints as a binary
    /// placeholder.
    binary: bool,
    kind: EntryKind,
}

/// Where `FoundPNG` files a text chunk once its keyword is resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TextTagRoute {
    /// Emit `PNG:<name>`; `binary` means the value is shown as
    /// `(Binary data N bytes, use -b option to extract)`.
    Tag {
        name: String,
        conv: TextConv,
        binary: bool,
    },
    /// `XML:com.adobe.xmp` with no language tag: the payload is parsed as an
    /// XMP packet and no `PNG:` row is produced.
    Xmp,
    /// A known `Raw profile type` keyword: ExifTool hex-decodes the payload
    /// and hands it to the EXIF/XMP/ICC/IPTC/Photoshop parsers, producing no
    /// `PNG:` text row. Carries the table `Name` for diagnostics.
    Profile(&'static str),
}

/// Per-file naming state (see the module docs for why it is stateful).
#[derive(Debug, Default)]
pub(crate) struct TextTagNamer {
    /// Run-time `AddTagToTable` registrations: unknown keywords under their
    /// raw keyword (`PNG.pm:1124`) and language copies under
    /// `<TagID>-<lang>` (`Writer.pl:4111-4123`). `AddTagToTable` never
    /// overrides an existing key (`ExifTool.pm:9268`), and the static rows
    /// are consulted first, so a dynamic entry can never shadow one.
    dynamic: HashMap<String, Entry>,
    /// Static rows whose `RawConv` `FoundPNG` has deleted. For a value that
    /// is still compressed (unknown method, or an inflate that failed) and a
    /// tag with no `ValueConv`, `FoundPNG` sets `$$tagInfo{RawConv} =
    /// '\$val'` and afterwards `delete`s it (`PNG.pm:1140-1147`) -- from the
    /// shared table hash, so the tag's own static `RawConv` (only `Creation
    /// Time`'s `ConvertPNGDate`, `PNG.pm:635`) is gone for every later chunk,
    /// and language copies made afterwards copy the stripped hash
    /// (`Writer.pl:4114`). Dynamic entries are stripped in place.
    stripped: HashSet<String>,
}

impl TextTagNamer {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn lookup(&self, id: &str) -> Option<Entry> {
        if is_special_tag(id) {
            return None;
        }
        if let Some(k) = TEXTUAL_DATA.iter().find(|k| k.keyword == id) {
            let conv = if self.stripped.contains(k.keyword) {
                TextConv::None
            } else {
                k.conv
            };
            return Some(Entry {
                tag_id: k.keyword.to_string(),
                name: k.name.to_string(),
                conv,
                binary: false,
                kind: k.kind,
            });
        }
        self.dynamic.get(id).cloned()
    }

    /// `delete $$tagInfo{RawConv} if $delRawConv` (`PNG.pm:1147`) for the
    /// entry registered under `tag_id`. Only `ConvertPNGDate` is a `RawConv`
    /// here; an `XmpDate` entry has a `ValueConv`, so `:1142` never set
    /// `$delRawConv` for it.
    fn strip_raw_conv(&mut self, tag_id: &str) {
        if TEXTUAL_DATA
            .iter()
            .any(|k| k.keyword == tag_id && k.conv == TextConv::PngDate)
        {
            self.stripped.insert(tag_id.to_string());
        } else if let Some(entry) = self.dynamic.get_mut(tag_id)
            && entry.conv == TextConv::PngDate
        {
            entry.conv = TextConv::None;
        }
    }

    /// `Image::ExifTool::PNG::GetLangInfo` (`PNG.pm:890-899`) followed by the
    /// core `GetLangInfo` (`Writer.pl:4106-4126`).
    fn lang_info(&mut self, base: &Entry, lang: &str) -> Option<Entry> {
        // "RFC 3066 specifies '-' as a separator" (PNG.pm:893)
        let lang = lang.replace('_', "-");
        // "no alternate languages for XMP or raw profile directories" (:895)
        if base.kind != EntryKind::Text {
            return None;
        }
        // "language code must normalized for use in tag ID" (:897-898)
        let lang = standard_lang_case(&lang);
        let tag_id = format!("{}-{}", base.tag_id, lang);
        if let Some(existing) = self.dynamic.get(&tag_id) {
            return Some(existing.clone());
        }
        let entry = Entry {
            tag_id: tag_id.clone(),
            // `Name => "$Name-$langCode"` (Writer.pl:4115), then cleaned by
            // the `AddTagToTable` call that registers it (:4121,
            // ExifTool.pm:9256-9266 writes the cleaned name back into the
            // same hash GetLangInfo returns).
            name: add_tag_to_table_name(&format!("{}-{}", base.name, lang)),
            conv: base.conv,
            binary: base.binary,
            kind: base.kind,
        };
        self.dynamic.insert(tag_id, entry.clone());
        Some(entry)
    }

    /// `FoundPNG`'s name resolution (`PNG.pm:908-927`, `:1115-1124`) for a
    /// chunk keyword and, for iTXt, its language tag.
    pub(crate) fn resolve(&mut self, keyword: &str, lang: Option<&str>) -> TextTagRoute {
        self.resolve_entry(keyword, lang).0
    }

    /// [`Self::resolve`], plus the table key (`TagID`) of the entry the
    /// chunk resolved to, which `FoundPNG`'s `RawConv` deletion acts on.
    fn resolve_entry(&mut self, keyword: &str, lang: Option<&str>) -> (TextTagRoute, String) {
        // `if ($lang)` (:914): Perl truthiness, so '' and '0' mean no tag.
        let lang = lang.filter(|l| !l.is_empty() && *l != "0");
        // "case of language code must be normalized since they are case
        // insensitive" (:915-916)
        let std_lang = lang.map(standard_lang_case);
        let id = match &std_lang {
            Some(l) => format!("{keyword}-{l}"),
            None => keyword.to_string(),
        };
        let mut info = self.lookup(&id).or_else(|| self.lookup(&ucfirst(&id)));
        if info.is_none()
            && let Some(l) = &std_lang
        {
            // "create alternate language tag if necessary" (:922-927)
            let base = self
                .lookup(keyword)
                .or_else(|| self.lookup(&ucfirst(keyword)));
            if let Some(base) = base {
                info = self.lang_info(&base, l);
            }
        }
        if let Some(entry) = info {
            let route = match entry.kind {
                EntryKind::Text => TextTagRoute::Tag {
                    name: entry.name,
                    conv: entry.conv,
                    binary: entry.binary,
                },
                EntryKind::Xmp => TextTagRoute::Xmp,
                EntryKind::Profile => TextTagRoute::Profile(
                    TEXTUAL_DATA
                        .iter()
                        .find(|k| k.keyword == entry.tag_id)
                        .map(|k| k.name)
                        .unwrap_or("Profile"),
                ),
            };
            return (route, entry.tag_id);
        }
        // Unknown keyword: PNG.pm:1115-1124.
        let name = add_tag_to_table_name(&collapse_whitespace(keyword));
        // "make unknown profiles binary data type" (:1121-1122)
        let binary = keyword.starts_with("Raw profile type ");
        // AddTagToTable($tagTablePtr, $tag, $tagInfo) (:1124) registers the
        // raw keyword; an existing key is never overridden, and a special
        // table key is never registered (ExifTool.pm:9268-9269).
        if !is_special_tag(keyword) {
            self.dynamic.entry(keyword.to_string()).or_insert(Entry {
                tag_id: keyword.to_string(),
                name: name.clone(),
                conv: TextConv::None,
                binary,
                kind: EntryKind::Text,
            });
        }
        let route = TextTagRoute::Tag {
            name,
            conv: TextConv::None,
            binary,
        };
        (route, keyword.to_string())
    }
}

/// The keyword ExifTool would write for a `PNG:<name>` tag the caller
/// authored (no chunk to inherit it from): the `%TextualData` key whose
/// `Name` matches, else the name itself (`WritePNG.pl:182-246` writes the
/// tag ID, which for a user-defined tag is its name). Only text rows are
/// invertible; `XMP` and the `*_Profile` names are directories, not text.
pub(crate) fn keyword_for_name(name: &str) -> Option<&'static str> {
    TEXTUAL_DATA
        .iter()
        .find(|k| k.kind == EntryKind::Text && k.name == name)
        .map(|k| k.keyword)
}

/// A caller-authored `PNG:<name>` key the writer may turn into a new text
/// chunk: a `%TextualData` text row, optionally with a `-<lang>` suffix
/// (`PNG:Comment-fr`). These are the only TextualData tags ExifTool itself
/// can write without a user-defined tag (the table NOTES, `PNG.pm:599-603`).
/// Returns the chunk keyword and the language tag.
pub(crate) fn writable_text_name(name: &str) -> Option<(&'static str, Option<&str>)> {
    if let Some(keyword) = keyword_for_name(name) {
        return Some((keyword, None));
    }
    let (base, lang) = name.split_once('-')?;
    if lang.is_empty() {
        return None;
    }
    keyword_for_name(base).map(|keyword| (keyword, Some(lang)))
}

/// What one textual-data chunk contributes to the metadata map.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TextRow {
    /// `PNG:<name>` with its printed value. `binary` marks a
    /// `(Binary data N bytes, ...)` placeholder, which is not the chunk text.
    Tag {
        name: String,
        value: TagValue,
        binary: bool,
    },
    /// An XMP packet (`XML:com.adobe.xmp`, no language tag) to hand to the
    /// XMP parser undecoded (`FoundPNG` skips `Decode` for SubDirectory tags,
    /// `PNG.pm:964`).
    Xmp(Vec<u8>),
    /// Nothing oxidex can emit exactly: a known `Raw profile type` payload
    /// (ExifTool decodes it into EXIF/XMP/ICC/IPTC/Photoshop rows through
    /// `ProcessProfile`, `PNG.pm:1155-1282`, which oxidex does not
    /// implement), a failed inflate, or a date too long to convert.
    Omit,
}

/// `(Binary data N bytes, use -b option to extract)`.
fn binary_placeholder(len: usize) -> TagValue {
    TagValue::new_string(format!(
        "(Binary data {len} bytes, use -b option to extract)"
    ))
}

impl TextTagNamer {
    /// Names and decodes one tEXt / zTXt / iTXt record the way `FoundPNG`
    /// does (`PNG.pm:908-1149`). Must be called for every such chunk, in
    /// file order, whether or not its row is emitted: naming registers
    /// tags that later chunks resolve against.
    pub(crate) fn row(&mut self, rec: &PngTextRecord) -> TextRow {
        // The keyword and language are byte strings in Perl; `decode_latin`
        // maps each byte to one char and leaves ASCII alone, and every
        // naming step either compares ASCII or deletes non-ASCII.
        let keyword = decode_latin(&rec.keyword);
        let lang = rec.lang.as_deref().map(decode_latin);
        let (route, tag_id) = self.resolve_entry(&keyword, lang.as_deref());
        if matches!(
            rec.payload,
            TextPayload::UnknownMethod(_) | TextPayload::InflateError
        ) && let TextTagRoute::Tag {
            conv: TextConv::PngDate,
            ..
        } = route
        {
            // `$compressed` is still true here, so FoundPNG overrides and
            // then deletes the entry's RawConv (PNG.pm:1140-1147). This
            // chunk's own value is binary (or omitted) either way; what
            // changes is every later chunk that resolves to the same entry.
            self.strip_raw_conv(&tag_id);
        }
        match (route, &rec.payload) {
            (TextTagRoute::Tag { name, conv, binary }, TextPayload::Bytes(bytes)) => {
                // `$val = $et->Decode($val, $enc)` (:963-966): 'Latin'
                // (cp1252) for tEXt/zTXt, 'UTF8' for iTXt.
                let text = if rec.is_latin() {
                    decode_latin(bytes)
                } else {
                    String::from_utf8_lossy(bytes).into_owned()
                };
                if binary {
                    // Length of `$val` after `Decode` (:965): tEXt/zTXt went
                    // Latin -> UTF-8, so count the UTF-8 string; iTXt went
                    // UTF8 -> UTF8, which `Decode` returns untouched
                    // (`$from ne $to` is false, ExifTool.pm:6349), so
                    // count the raw bytes -- invalid UTF-8 included, not the
                    // U+FFFD-widened lossy string.
                    let len = if rec.is_latin() {
                        text.len()
                    } else {
                        bytes.len()
                    };
                    return TextRow::Tag {
                        name,
                        value: binary_placeholder(len),
                        binary: true,
                    };
                }
                let value = match conv {
                    TextConv::None => text,
                    TextConv::PngDate => match convert_png_date(&text) {
                        Some(v) => v,
                        None => return TextRow::Omit,
                    },
                    TextConv::XmpDate => convert_xmp_date(&text),
                };
                TextRow::Tag {
                    name,
                    value: TagValue::new_string(value),
                    binary: false,
                }
            }
            // Unknown compression method: the value stays the compressed
            // bytes and `RawConv => '\$val'` makes it binary -- but only
            // when the tag has no ValueConv (:1140-1143). CreateDate and
            // ModDate do, and would run ConvertXMPDate over the compressed
            // bytes; that string is not reproducible, so omit.
            (TextTagRoute::Tag { name, conv, .. }, TextPayload::UnknownMethod(bytes))
                if conv != TextConv::XmpDate =>
            {
                TextRow::Tag {
                    name,
                    value: binary_placeholder(bytes.len()),
                    binary: true,
                }
            }
            (TextTagRoute::Xmp, TextPayload::Bytes(bytes)) => TextRow::Xmp(bytes.clone()),
            _ => TextRow::Omit,
        }
    }
}

/// Encodes a string as ExifTool's `Latin` (cp1252, `Charset/Latin.pm`), the
/// inverse of [`decode_latin`]; `None` when a character has no cp1252 byte.
pub(crate) fn encode_latin(s: &str) -> Option<Vec<u8>> {
    s.chars()
        .map(|c| {
            if (c as u32) < 0x80 {
                return Some(c as u8);
            }
            (0x80..=0xFFu8).find(|&b| decode_latin(&[b]).starts_with(c))
        })
        .collect()
}

/// Perl `ucfirst` on a byte string: only an ASCII first character changes.
fn ucfirst(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let mut out = String::with_capacity(s.len());
            out.push(c.to_ascii_uppercase());
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

/// `\s` as Perl matches it on a non-UTF-8 string: the six ASCII whitespace
/// characters (vertical tab included since Perl 5.18).
fn is_perl_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C' | '\x0B')
}

/// `($name = $tag) =~ s/\s+(.)/\u$1/g` (`PNG.pm:1117`): every whitespace run
/// is deleted and the character after it upper-cased. A trailing run has no
/// following character and is left for `AddTagToTable` to delete.
fn collapse_whitespace(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    let mut upcase_next = false;
    for c in tag.chars() {
        if is_perl_space(c) {
            upcase_next = true;
            continue;
        }
        if upcase_next {
            out.push(c.to_ascii_uppercase());
            upcase_next = false;
        } else {
            out.push(c);
        }
    }
    if upcase_next {
        // The run was trailing: `(.)` found nothing, so the run stays.
        // Re-append one space so the `tr` step below deletes it, keeping the
        // two functions independently faithful.
        out.push(' ');
    }
    out
}

/// `AddTagToTable`'s name cleaning (`ExifTool.pm:9254-9265`):
/// `tr/-_a-zA-Z0-9//dc`, `ucfirst`, then a `Tag` prefix when shorter than
/// two characters or not starting with a letter. `%TextualData` declares no
/// `TAG_PREFIX`, so the prefix branch at `:9258-9264` never runs here.
fn add_tag_to_table_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let kept = ucfirst(&kept);
    if kept.len() < 2 || !kept.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        format!("Tag{kept}")
    } else {
        kept
    }
}

/// `Image::ExifTool::PNG::StandardLangCase` (`PNG.pm:796-802`):
/// `/^([a-z]{2,3}|[xi])(-[a-z]{2})\b(.*)/i` -> `lc($1) . uc($2) . lc($3)`,
/// otherwise `lc` of the whole code.
pub(crate) fn standard_lang_case(lang: &str) -> String {
    let b = lang.as_bytes();
    let alpha = |i: usize| b.get(i).is_some_and(u8::is_ascii_alphabetic);
    // Alternation order as the regex engine tries it: `[a-z]{2,3}` greedy
    // (3, then 2), then `[xi]`.
    let mut primary_lens = Vec::with_capacity(3);
    if alpha(0) && alpha(1) && alpha(2) {
        primary_lens.push(3);
    }
    if alpha(0) && alpha(1) {
        primary_lens.push(2);
    }
    if b.first()
        .is_some_and(|c| matches!(c, b'x' | b'X' | b'i' | b'I'))
    {
        primary_lens.push(1);
    }
    for n in primary_lens {
        if b.get(n) == Some(&b'-') && alpha(n + 1) && alpha(n + 2) {
            // `\b` on a byte string: end of string or a non-word character.
            let boundary = match b.get(n + 3) {
                None => true,
                Some(c) => !(c.is_ascii_alphanumeric() || *c == b'_'),
            };
            if boundary {
                // `(.*)` is $3: Perl's `.` does not match "\n" (no /s on the
                // regex at PNG.pm:800), so the tail stops at the first newline
                // and anything after it is dropped from the returned name.
                let tail = lang[n + 3..].split('\n').next().unwrap_or_default();
                return format!(
                    "{}{}{}",
                    lang[..n].to_ascii_lowercase(),
                    lang[n..n + 3].to_ascii_uppercase(),
                    tail.to_ascii_lowercase()
                );
            }
        }
    }
    lang.to_ascii_lowercase()
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `%tzConv` (`PNG.pm:811-831`).
fn tz_conv(tz: &str) -> Option<&'static str> {
    Some(match tz.to_ascii_uppercase().as_str() {
        "UT" | "GMT" | "UTC" | "Z" => "+00:00",
        "EST" | "CDT" | "E" => "-05:00",
        "EDT" | "D" => "-04:00",
        "CST" | "MDT" | "F" => "-06:00",
        "MST" | "PDT" | "G" => "-07:00",
        "PST" | "H" => "-08:00",
        "A" => "-01:00",
        "B" => "-02:00",
        "C" => "-03:00",
        "I" => "-09:00",
        "K" => "-10:00",
        "L" => "-11:00",
        "M" => "-12:00",
        "N" => "+01:00",
        "O" => "+02:00",
        "P" => "+03:00",
        "Q" => "+04:00",
        "R" => "+05:00",
        "S" => "+06:00",
        "T" => "+07:00",
        "U" => "+08:00",
        "V" => "+09:00",
        "W" => "+10:00",
        "X" => "+11:00",
        "Y" => "+12:00",
        _ => return None,
    })
}

/// `ConvertPNGDate` (`PNG.pm:833-857`): an RFC-1123 date such as
/// `Mon, 1 Jan 2018 12:10:22 EST` becomes `2018:01:01 12:10:22-05:00`.
///
/// The Perl is `while ($val =~ /.../i) { ...; return ... }` where every
/// path through the body either returns the converted string or `last`s on
/// an unrecognised zone, so the value is returned unchanged when the regex
/// does not match *or* the first match has an unknown zone (the `Non
/// standard PNG date/time format` warning only fires under
/// `-validate`/`StrictDate`).
///
/// `None` means the value cannot be reproduced exactly (a numeric field too
/// long for an `i64`, which Perl would numify to a float) and the tag should
/// be omitted rather than approximated.
pub(crate) fn convert_png_date(val: &str) -> Option<String> {
    let Some(c) = png_date_captures(val) else {
        return Some(val.to_string());
    };
    let num = |s: &str| s.parse::<i64>().ok();
    let (mut yr, day, hr) = (num(c.yr)?, num(c.day)?, num(c.hr)?);
    // `$yr += $yr > 70 ? 1900 : 2000 if $yr < 100;` (:839)
    if yr < 100 {
        yr += if yr > 70 { 1900 } else { 2000 };
    }
    // `if (not $tz)`: Perl truthiness, so '' and '0' are both "no zone".
    let tz = if c.tz.is_empty() || c.tz == "0" {
        String::new()
    } else if let Some(t) = tz_conv(c.tz) {
        t.to_string()
    } else if let Some(t) = numeric_zone(c.tz) {
        t
    } else {
        // `last; # (non-standard date)` (:848)
        return Some(val.to_string());
    };
    let sec = if c.sec.is_empty() { ":00" } else { c.sec };
    // sprintf("%.4d:%.2d:%.2d %.2d:%.2d%s%s", ...) (:850)
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{}{}{}",
        yr,
        c.mon_idx + 1,
        day,
        hr,
        c.min,
        sec,
        tz
    ))
}

/// The capture groups of the `ConvertPNGDate` regex.
struct PngDateCaptures<'a> {
    day: &'a str,
    mon_idx: usize,
    yr: &'a str,
    hr: &'a str,
    min: &'a str,
    /// `(:\d{2})?`, empty when absent.
    sec: &'a str,
    tz: &'a str,
}

/// Leftmost match of
/// `/(\d+)\s*(Jan|...|Dec)\s*(\d+)\s+(\d+):(\d{2})(:\d{2})?\s*(\S*)/i`
/// (`PNG.pm:837`). Every quantifier here is followed by a token that the
/// characters it could give back can never satisfy (a digit run is followed
/// by a non-digit, a whitespace run by a non-space), so backtracking never
/// changes a capture and one greedy pass per start position is exact.
fn png_date_captures(val: &str) -> Option<PngDateCaptures<'_>> {
    let b = val.as_bytes();
    let digits = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let spaces = |mut i: usize| {
        while i < b.len() && is_perl_space(char::from(b[i])) {
            i += 1;
        }
        i
    };
    (0..b.len()).find_map(|start| {
        let day_end = digits(start);
        if day_end == start {
            return None;
        }
        let i = spaces(day_end);
        let mon_idx = (0..12).find(|&m| {
            b.get(i..i + 3)
                .is_some_and(|s| s.eq_ignore_ascii_case(MONTHS[m].as_bytes()))
        })?;
        let yr_s = spaces(i + 3);
        let yr_e = digits(yr_s);
        if yr_e == yr_s {
            return None;
        }
        let hr_s = spaces(yr_e);
        if hr_s == yr_e {
            return None; // `\s+` needs at least one
        }
        let hr_e = digits(hr_s);
        if hr_e == hr_s || b.get(hr_e) != Some(&b':') {
            return None;
        }
        let mn_s = hr_e + 1;
        if !b
            .get(mn_s..mn_s + 2)
            .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
        {
            return None;
        }
        let mn_e = mn_s + 2;
        let sec_e = match b.get(mn_e..mn_e + 3) {
            Some([b':', x, y]) if x.is_ascii_digit() && y.is_ascii_digit() => mn_e + 3,
            _ => mn_e,
        };
        let tz_s = spaces(sec_e);
        let mut tz_e = tz_s;
        while tz_e < b.len() && !is_perl_space(char::from(b[tz_e])) {
            tz_e += 1;
        }
        Some(PngDateCaptures {
            day: &val[start..day_end],
            mon_idx,
            yr: &val[yr_s..yr_e],
            hr: &val[hr_s..hr_e],
            min: &val[mn_s..mn_e],
            sec: &val[mn_e..sec_e],
            tz: &val[tz_s..tz_e],
        })
    })
}

/// `$tz =~ /^([-+]\d+):?(\d{2})/` then `$1 . ':' . $2` (`PNG.pm:845-846`).
/// `\d+` is greedy and gives digits back until `:?(\d{2})` can match, so
/// `+0530` splits as `+05` / `30`.
fn numeric_zone(tz: &str) -> Option<String> {
    let b = tz.as_bytes();
    if !matches!(b.first(), Some(b'+' | b'-')) {
        return None;
    }
    let mut run_end = 1;
    while run_end < b.len() && b[run_end].is_ascii_digit() {
        run_end += 1;
    }
    // `\d+` takes 1..=run digits, longest first; `:?` is greedy.
    (2..=run_end).rev().find_map(|end| {
        let two = |j: usize| {
            b.get(j..j + 2)
                .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
                .then(|| format!("{}:{}", &tz[..end], &tz[j..j + 2]))
        };
        if b.get(end) == Some(&b':') {
            two(end + 1).or_else(|| two(end))
        } else {
            two(end)
        }
    })
}

/// `Image::ExifTool::XMP::ConvertXMPDate($val)` with `$unsure` unset
/// (`XMP.pm:3383-3394`): `YYYY-MM-DD[T ]HH:MM[:SS][ ]TZ` becomes the EXIF
/// form; otherwise a value starting with four digits has every dash turned
/// into a colon; anything else is returned unchanged.
pub(crate) fn convert_xmp_date(val: &str) -> String {
    if let Some(out) = xmp_date_full(val) {
        return out;
    }
    // `elsif (not $unsure and $val =~ /^(\d{4})(-\d{2}){0,2}/) { tr/-/:/ }`
    let b = val.as_bytes();
    if b.len() >= 4 && b[..4].iter().all(u8::is_ascii_digit) {
        return val.replace('-', ":");
    }
    val.to_string()
}

/// `/^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})(:\d{2})?\s*(\S*)$/` then
/// `"$1:$2:$3 $4$s$6"`. Perl's `$` also matches before a final newline,
/// which the result then drops.
fn xmp_date_full(val: &str) -> Option<String> {
    let b = val.as_bytes();
    let d = |i: usize, n: usize| {
        b.get(i..i + n)
            .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
    };
    if !(d(0, 4) && b.get(4) == Some(&b'-') && d(5, 2) && b.get(7) == Some(&b'-') && d(8, 2)) {
        return None;
    }
    if !matches!(b.get(10), Some(b'T' | b' ')) {
        return None;
    }
    if !(d(11, 2) && b.get(13) == Some(&b':') && d(14, 2)) {
        return None;
    }
    // `(:\d{2})?` greedy; giving it back cannot help `\s*(\S*)$`, whose
    // `\S*` would then have to cover the same ':SS' and whatever follows.
    let sec_e = if b.get(16) == Some(&b':') && d(17, 2) {
        19
    } else {
        16
    };
    let tz_s = (sec_e..b.len())
        .find(|&i| !is_perl_space(char::from(b[i])))
        .unwrap_or(b.len());
    let tz_e = (tz_s..b.len())
        .find(|&i| is_perl_space(char::from(b[i])))
        .unwrap_or(b.len());
    // `$`: end of string, or just before a string-final newline.
    let at_end = tz_e == b.len() || (tz_e + 1 == b.len() && b[tz_e] == b'\n');
    if !at_end {
        return None;
    }
    Some(format!(
        "{}:{}:{} {}{}{}",
        &val[0..4],
        &val[5..7],
        &val[8..10],
        &val[11..16],
        &val[16..sec_e],
        &val[tz_s..tz_e]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(namer: &mut TextTagNamer, kw: &str, lang: Option<&str>) -> String {
        match namer.resolve(kw, lang) {
            TextTagRoute::Tag { name, .. } => name,
            other => panic!("{kw:?} resolved to {other:?}"),
        }
    }

    #[test]
    fn known_keywords_take_the_table_name() {
        let mut n = TextTagNamer::new();
        assert_eq!(name(&mut n, "Author", None), "Author");
        assert_eq!(name(&mut n, "Creation Time", None), "CreationTime");
        assert_eq!(name(&mut n, "Warning", None), "PNGWarning");
        assert_eq!(name(&mut n, "modify-date", None), "ModDate");
        assert_eq!(name(&mut n, "aesthetic_score", None), "AestheticScore");
        assert_eq!(name(&mut n, "parameters", None), "Parameters");
        // ucfirst fallback (PNG.pm:921): t/images/PNG.png's lowercase keyword
        assert_eq!(name(&mut n, "comment", None), "Comment");
        // ... but nothing beyond ucfirst (pngsyn/02_known_case.png)
        assert_eq!(name(&mut n, "MAKE", None), "MAKE");
        assert_eq!(name(&mut n, "CREATE-DATE", None), "CREATE-DATE");
        assert_eq!(name(&mut n, "creation time", None), "CreationTime");
        assert_eq!(
            n.resolve("creation time", None),
            TextTagRoute::Tag {
                name: "CreationTime".into(),
                conv: TextConv::None,
                binary: false
            },
            "'Creation time' != 'Creation Time', so no RawConv"
        );
        assert_eq!(
            n.resolve("Creation Time", None),
            TextTagRoute::Tag {
                name: "CreationTime".into(),
                conv: TextConv::PngDate,
                binary: false
            }
        );
    }

    #[test]
    fn unknown_keywords_follow_the_addtagtotable_rule() {
        // Every row of pngsyn/01_names.png as printed by ExifTool 13.59.
        let cases = [
            ("exif:Make", "ExifMake"),
            ("date:create", "Datecreate"),
            ("exif:ExifOffset", "ExifExifOffset"),
            ("foo bar", "FooBar"),
            ("foo  BAR", "FooBAR"),
            ("a\tb", "AB"),
            (" lead", "Lead"),
            ("trail ", "Trail"),
            ("123abc", "Tag123abc"),
            ("-dash", "Tag-dash"),
            ("_under", "Tag_under"),
            ("my-tag", "My-tag"),
            ("x.y/z", "Xyz"),
            ("a", "TagA"),
            ("ab", "Ab"),
            ("", "Tag"),
            ("#", "Tag"),
            ("a b", "AB"),
        ];
        for (kw, expected) in cases {
            let mut n = TextTagNamer::new();
            assert_eq!(name(&mut n, kw, None), expected, "keyword {kw:?}");
        }
    }

    #[test]
    fn unknown_raw_profile_is_binary() {
        let mut n = TextTagNamer::new();
        assert_eq!(
            n.resolve("Raw profile type foo", None),
            TextTagRoute::Tag {
                name: "RawProfileTypeFoo".into(),
                conv: TextConv::None,
                binary: true
            }
        );
        assert_eq!(
            n.resolve("Raw profile type exif", None),
            TextTagRoute::Profile("EXIF_Profile")
        );
        assert_eq!(n.resolve("XML:com.adobe.xmp", None), TextTagRoute::Xmp);
        assert_eq!(n.resolve("XML:com.adobe.xmp", Some("")), TextTagRoute::Xmp);
    }

    #[test]
    fn itxt_language_suffixes_a_known_keyword() {
        // pngsyn/03_lang.png
        let mut n = TextTagNamer::new();
        assert_eq!(name(&mut n, "Comment", Some("fr")), "Comment-fr");
        assert_eq!(name(&mut n, "Comment", Some("EN-us")), "Comment-en-US");
        assert_eq!(
            name(&mut n, "Comment", Some("x-default")),
            "Comment-x-default"
        );
        assert_eq!(name(&mut n, "Comment", Some("")), "Comment");
        assert_eq!(name(&mut n, "Title", Some("de_DE")), "Title-de-DE");
        assert_eq!(name(&mut n, "comment", Some("fr")), "Comment-fr");
        assert_eq!(
            n.resolve("Creation Time", Some("fr")),
            TextTagRoute::Tag {
                name: "CreationTime-fr".into(),
                conv: TextConv::PngDate,
                binary: false
            },
            "the language copy keeps the base conversions (Writer.pl:4114)"
        );
        // A SubDirectory entry has no language copies (PNG.pm:894-895):
        // the unknown path names it from the raw keyword.
        assert_eq!(
            name(&mut n, "XML:com.adobe.xmp", Some("fr")),
            "XMLcomadobexmp"
        );
        assert_eq!(
            n.resolve("XML:com.adobe.xmp", None),
            TextTagRoute::Xmp,
            "AddTagToTable never overrides the static row (ExifTool.pm:9268)"
        );
    }

    #[test]
    fn itxt_language_on_an_unknown_keyword_is_order_dependent() {
        // pngsyn/04_unknown_lang.png: zzz fr, zzz de, zzz fr, zzz nolang
        let mut n = TextTagNamer::new();
        assert_eq!(name(&mut n, "zzz", Some("fr")), "Zzz");
        assert_eq!(name(&mut n, "zzz", Some("de")), "Zzz-de");
        assert_eq!(name(&mut n, "zzz", Some("fr")), "Zzz-fr");
        assert_eq!(name(&mut n, "zzz", None), "Zzz");
        // pngsyn/04b: yyy plain, yyy fr, "foo bar" fr
        let mut n = TextTagNamer::new();
        assert_eq!(name(&mut n, "yyy", None), "Yyy");
        assert_eq!(name(&mut n, "yyy", Some("fr")), "Yyy-fr");
        assert_eq!(name(&mut n, "foo bar", Some("fr")), "FooBar");
        // The registration key is the raw keyword: 'Zzz' after 'zzz' is a
        // different key, but 'zzz' after 'Zzz' resolves through ucfirst.
        let mut n = TextTagNamer::new();
        assert_eq!(name(&mut n, "zzz", None), "Zzz");
        assert_eq!(name(&mut n, "Zzz", Some("fr")), "Zzz");
        let mut n = TextTagNamer::new();
        assert_eq!(name(&mut n, "Zzz", None), "Zzz");
        assert_eq!(name(&mut n, "zzz", Some("fr")), "Zzz-fr");
    }

    fn record(chunk_type: &[u8; 4], data: &[u8]) -> PngTextRecord {
        crate::parsers::png::chunk_parser::parse_text_record(chunk_type, data)
            .expect("well-formed chunk")
    }

    fn itxt(keyword: &[u8], lang: &[u8], value: &[u8], compressed: bool) -> Vec<u8> {
        let mut d = keyword.to_vec();
        d.push(0);
        d.extend_from_slice(if compressed { b"\x01\x00" } else { b"\x00\x00" });
        d.extend_from_slice(lang);
        d.extend_from_slice(b"\0\0");
        if compressed {
            use std::io::Write;
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(value).unwrap();
            d.extend_from_slice(&e.finish().unwrap());
        } else {
            d.extend_from_slice(value);
        }
        d
    }

    fn tag(name: &str, value: &str) -> TextRow {
        TextRow::Tag {
            name: name.into(),
            value: TagValue::new_string(value),
            binary: false,
        }
    }

    #[test]
    fn rows_match_exiftool_for_pngimpl_n1() {
        // Every expectation is ExifTool 13.59 `-G1 -s -a` on pngimpl/n1.png,
        // whose chunks these are, in order.
        let mut n = TextTagNamer::new();
        assert_eq!(n.row(&record(b"tEXt", b"\0empty")), tag("Tag", "empty"));
        // `Comment-en.us` is cleaned by AddTagToTable (ExifTool.pm:9256)
        assert_eq!(
            n.row(&record(b"iTXt", &itxt(b"Comment", b"en.us", b"dot", false))),
            tag("Comment-enus", "dot")
        );
        // `tr/_/-/` before StandardLangCase (PNG.pm:893)
        assert_eq!(
            n.row(&record(
                b"iTXt",
                &itxt(b"Comment", b"zh_tw", b"uscore", false)
            )),
            tag("Comment-zh-TW", "uscore")
        );
        // '0' is false in Perl: no language suffix
        assert_eq!(
            n.row(&record(b"iTXt", &itxt(b"Comment", b"0", b"zero", false))),
            tag("Comment", "zero")
        );
        // a compressed iTXt is inflated (PNG.pm:931-941)
        assert_eq!(
            n.row(&record(b"iTXt", &itxt(b"Title", b"fr", b"cz", true))),
            tag("Title-fr", "cz")
        );
        // an unknown 'Raw profile type' is Binary (PNG.pm:1122); 16 bytes
        assert_eq!(
            n.row(&record(
                b"tEXt",
                b"Raw profile type foo\0\nfoo\n  3\n616263\n"
            )),
            TextRow::Tag {
                name: "RawProfileTypeFoo".into(),
                value: TagValue::new_string("(Binary data 16 bytes, use -b option to extract)"),
                binary: true,
            }
        );
        // unknown compression method 5: the compressed bytes, as binary
        assert_eq!(
            n.row(&record(b"zTXt", b"Comment\0\x05garbage")),
            TextRow::Tag {
                name: "Comment".into(),
                value: TagValue::new_string("(Binary data 7 bytes, use -b option to extract)"),
                binary: true,
            }
        );
        // tEXt is Latin (cp1252): 0xE9 is e-acute, 0x80 the euro sign; the
        // keyword's 0xE9 is deleted by `tr` after `s/\s+(.)/\u$1/`
        assert_eq!(
            n.row(&record(b"tEXt", b"caf\xe9 x\0caf\xe9 \x80")),
            tag("CafX", "caf\u{e9} \u{20ac}")
        );
        assert_eq!(
            n.row(&record(
                b"iTXt",
                &itxt(b"Comment", b"DE-at-1996", b"deu", false)
            )),
            tag("Comment-de-AT-1996", "deu")
        );
        // iTXt is UTF-8, not Latin
        assert_eq!(
            n.row(&record(
                b"iTXt",
                &itxt(b"Author", b"", "utf8 \u{e9}".as_bytes(), false)
            )),
            tag("Author", "utf8 \u{e9}")
        );
    }

    #[test]
    fn rows_ztxt_inflation_and_routes() {
        let mut n = TextTagNamer::new();
        let mut z = b"Comment\0\0".to_vec();
        {
            use std::io::Write;
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(b"zcomment").unwrap();
            z.extend_from_slice(&e.finish().unwrap());
        }
        // pngsyn/05_compressed.png: zTXt Comment -> `Comment : zcomment`
        assert_eq!(n.row(&record(b"zTXt", &z)), tag("Comment", "zcomment"));
        // a truncated stream never reaches Z_STREAM_END: omitted, since
        // ExifTool's byte count depends on zlib's partial consumption
        let truncated = &z[..z.len() - 3];
        assert_eq!(n.row(&record(b"zTXt", truncated)), TextRow::Omit);
        // 'Creation Time' gets ConvertPNGDate; create-date ConvertXMPDate
        assert_eq!(
            n.row(&record(
                b"tEXt",
                b"Creation Time\0Mon, 1 Jan 2018 12:10:22 EST"
            )),
            tag("CreationTime", "2018:01:01 12:10:22-05:00")
        );
        assert_eq!(
            n.row(&record(b"tEXt", b"create-date\02020-01-02T03:04:05Z")),
            tag("CreateDate", "2020:01:02 03:04:05Z")
        );
        // XMP is parsed, known raw profiles are not text rows
        assert_eq!(
            n.row(&record(b"tEXt", b"XML:com.adobe.xmp\0<x/>")),
            TextRow::Xmp(b"<x/>".to_vec())
        );
        assert_eq!(
            n.row(&record(b"zTXt", b"Raw profile type exif\0\x05zz")),
            TextRow::Omit
        );
        // ... but with a language tag XMP has no alternate language
        // (PNG.pm:894-895) and becomes a text tag named from the keyword
        assert_eq!(
            n.row(&record(
                b"iTXt",
                &itxt(b"XML:com.adobe.xmp", b"fr", b"<x/>", false)
            )),
            tag("XMLcomadobexmp", "<x/>")
        );
    }

    /// Hex of a PNG file's tEXt/zTXt/iTXt chunks, in order, as
    /// `(chunk_type, data)`.
    fn text_chunks(png_hex: &str) -> Vec<([u8; 4], Vec<u8>)> {
        let d: Vec<u8> = (0..png_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&png_hex[i..i + 2], 16).unwrap())
            .collect();
        let mut out = Vec::new();
        let mut i = 8;
        while i + 8 <= d.len() {
            let n = u32::from_be_bytes(d[i..i + 4].try_into().unwrap()) as usize;
            let t: [u8; 4] = d[i + 4..i + 8].try_into().unwrap();
            if matches!(&t, b"tEXt" | b"zTXt" | b"iTXt") {
                out.push((t, d[i + 8..i + 8 + n].to_vec()));
            }
            i += 12 + n;
        }
        out
    }

    fn rows_of(png_hex: &str) -> Vec<TextRow> {
        let mut n = TextTagNamer::new();
        text_chunks(png_hex)
            .iter()
            .map(|(t, d)| n.row(&record(t, d)))
            .collect()
    }

    fn binary(name: &str, len: usize) -> TextRow {
        TextRow::Tag {
            name: name.into(),
            value: binary_placeholder(len),
            binary: true,
        }
    }

    #[test]
    fn a_still_compressed_creation_time_deletes_its_rawconv_for_later_chunks() {
        // FoundPNG sets and then deletes $$tagInfo{RawConv} on the shared
        // 'Creation Time' hash (PNG.pm:1140-1147). Every expectation is
        // ExifTool 13.59 `-a -G1 -s -PNG:all` on the file whose hex is given.
        const IHDR: &str = "89504e470d0a1a0a0000000d49484452000000010000000108000000003a7e9b55";
        const TAIL: &str = "0000000a49444154789c636000000002000148afa4710000000049454e44ae426082";
        let date = "Mon, 1 Jan 2018 12:10:22 EST";
        // e10_rawconv_del.png: zTXt method 1, then tEXt: the tEXt value is
        // printed raw, not converted.
        let e10 = format!(
            "{IHDR}000000127a5458744372656174696f6e2054696d650001010203d56235c9\
             0000002a744558744372656174696f6e2054696d65004d6f6e2c2031204a616e2032\
             3031382031323a31303a323220455354fefb7633{TAIL}"
        );
        assert_eq!(
            rows_of(&e10),
            vec![binary("CreationTime", 3), tag("CreationTime", date)]
        );
        // e11_inflate_fail.png: a method-0 stream that never reaches
        // Z_STREAM_END strips it too (the failed row itself is omitted).
        let e11 = format!(
            "{IHDR}000000137a5458744372656174696f6e2054696d650000789c0102308da75e\
             0000002a744558744372656174696f6e2054696d65004d6f6e2c2031204a616e2032\
             3031382031323a31303a323220455354fefb7633{TAIL}"
        );
        assert_eq!(
            rows_of(&e11),
            vec![TextRow::Omit, tag("CreationTime", date)]
        );
        // g05_ct_lang_after_del.png: a language copy made after the
        // deletion copies the stripped hash (Writer.pl:4114).
        let g05 = format!(
            "{IHDR}000000107a5458744372656174696f6e2054696d65000205501dde74\
             00000030695458744372656174696f6e2054696d65000000656e00004d6f6e2c2031\
             204a616e20323031382031323a31303a323220455354942d83b9{TAIL}"
        );
        assert_eq!(
            rows_of(&g05),
            vec![binary("CreationTime", 1), tag("CreationTime-en", date)]
        );
        // Without a compressed chunk first, the conversion still applies,
        // and a language copy made *before* a deletion keeps its own RawConv.
        let mut n = TextTagNamer::new();
        let text = |kw: &[u8], v: &[u8]| {
            let mut d = kw.to_vec();
            d.push(0);
            d.extend_from_slice(v);
            d
        };
        assert_eq!(
            n.row(&record(
                b"iTXt",
                &itxt(b"Creation Time", b"en", date.as_bytes(), false)
            )),
            tag("CreationTime-en", "2018:01:01 12:10:22-05:00")
        );
        assert_eq!(
            n.row(&record(b"zTXt", b"Creation Time\0\x01zz")),
            binary("CreationTime", 2)
        );
        assert_eq!(
            n.row(&record(b"tEXt", &text(b"Creation Time", date.as_bytes()))),
            tag("CreationTime", date)
        );
        assert_eq!(
            n.row(&record(
                b"iTXt",
                &itxt(b"Creation Time", b"en", date.as_bytes(), false)
            )),
            tag("CreationTime-en", "2018:01:01 12:10:22-05:00")
        );
        // create-date has a ValueConv, so :1142 never deletes anything and
        // the conversion survives a still-compressed chunk.
        assert_eq!(
            n.row(&record(b"zTXt", b"create-date\0\x01zz")),
            TextRow::Omit
        );
        assert_eq!(
            n.row(&record(
                b"tEXt",
                &text(b"create-date", b"2020-01-02T03:04:05Z")
            )),
            tag("CreateDate", "2020:01:02 03:04:05Z")
        );
    }

    #[test]
    fn itxt_binary_placeholder_counts_raw_bytes() {
        // f03_badutf8.png: ExifTool 13.59 prints RawProfileTypeQ as 3 bytes
        // for the invalid UTF-8 value ff fe fd (Decode UTF8->UTF8 is a
        // no-op, ExifTool.pm:6349), not 9 (three U+FFFD).
        let mut n = TextTagNamer::new();
        assert_eq!(
            n.row(&record(
                b"iTXt",
                &itxt(b"Raw profile type q", b"", b"\xff\xfe\xfd", false)
            )),
            binary("RawProfileTypeQ", 3)
        );
        // tEXt still counts the cp1252-decoded UTF-8 string: 0xE9 -> 2 bytes
        assert_eq!(
            n.row(&record(b"tEXt", b"Raw profile type r\0\xe9")),
            binary("RawProfileTypeR", 2)
        );
    }

    #[test]
    fn special_table_keys_are_never_found_or_registered() {
        // e12_special.png: ExifTool 13.59 `-a` prints NOTES, GROUPS (g1) and
        // GROUPS (g2) with no language suffix, warning "Tag GROUPS conflicts
        // with internal ExifTool variable" (ExifTool.pm:9130-9132, 9269).
        let mut n = TextTagNamer::new();
        assert_eq!(n.row(&record(b"tEXt", b"NOTES\0n")), tag("NOTES", "n"));
        assert_eq!(
            n.row(&record(b"iTXt", &itxt(b"GROUPS", b"en", b"g1", false))),
            tag("GROUPS", "g1")
        );
        assert_eq!(
            n.row(&record(b"iTXt", &itxt(b"GROUPS", b"fr", b"g2", false))),
            tag("GROUPS", "g2")
        );
        // ucfirst of a special key is special too; a lower-case spelling is
        // an ordinary unknown keyword and registers.
        assert_eq!(name(&mut n, "nOTES", Some("de")), "NOTES");
        assert_eq!(name(&mut n, "notes", None), "Notes");
        assert_eq!(name(&mut n, "notes", Some("de")), "Notes-de");
    }

    #[test]
    fn handler_rejections_yield_no_record() {
        use crate::parsers::png::chunk_parser::parse_text_record;
        assert!(parse_text_record(b"tEXt", b"no separator").is_none());
        assert!(parse_text_record(b"zTXt", b"kw\0").is_none());
        // `length($dat) >= 4` (PNG.pm:1343)
        assert!(parse_text_record(b"iTXt", b"kw\0\0\0\0").is_none());
        // `$val` undefined: no translated-keyword separator
        assert!(parse_text_record(b"iTXt", b"kw\0\0\0a\0b").is_none());
        let r = parse_text_record(b"iTXt", b"kw\0\0\0\0\0").unwrap();
        assert_eq!(r.payload, TextPayload::Bytes(Vec::new()));
    }

    #[test]
    fn writable_names_are_the_textual_data_text_rows() {
        assert_eq!(writable_text_name("Comment"), Some(("Comment", None)));
        assert_eq!(
            writable_text_name("Comment-fr"),
            Some(("Comment", Some("fr")))
        );
        assert_eq!(
            writable_text_name("CreationTime-en-US"),
            Some(("Creation Time", Some("en-US")))
        );
        assert_eq!(writable_text_name("ExifMake"), None);
        assert_eq!(writable_text_name("XMP"), None);
        assert_eq!(writable_text_name("ImageWidth"), None);
        assert_eq!(writable_text_name("Comment-"), None);
    }

    #[test]
    fn latin_encoding_inverts_decoding() {
        assert_eq!(
            encode_latin("caf\u{e9} \u{20ac}"),
            Some(b"caf\xe9 \x80".to_vec())
        );
        assert_eq!(encode_latin("\u{4f60}"), None);
        let all: Vec<u8> = (0..=255u8).collect();
        assert_eq!(encode_latin(&decode_latin(&all)), Some(all));
    }

    #[test]
    fn standard_lang_case_matches_the_perl_regex() {
        assert_eq!(standard_lang_case("EN-us"), "en-US");
        assert_eq!(standard_lang_case("en"), "en");
        assert_eq!(standard_lang_case("x-default"), "x-default");
        assert_eq!(standard_lang_case("X-DEFAULT"), "x-default");
        assert_eq!(standard_lang_case("eng-US"), "eng-US");
        assert_eq!(standard_lang_case("en-USA"), "en-usa");
        assert_eq!(standard_lang_case("zh-Hans"), "zh-hans");
        assert_eq!(standard_lang_case("en-us-x"), "en-US-x");
        assert_eq!(standard_lang_case("de_DE"), "de_de");
        assert_eq!(standard_lang_case("i-klingon"), "i-klingon");
        // PNG.pm:800: `.` stops at "\n", so the tail after a newline is dropped
        // (pinned oracle: iTXt lang "en-us\nXY" -> Title-en-US).
        assert_eq!(standard_lang_case("en-us\nXY"), "en-US");
        assert_eq!(standard_lang_case("fr-ca\nq"), "fr-CA");
        // No match: lc($lang) keeps the whole string, newline included.
        assert_eq!(standard_lang_case("EN\nX"), "en\nx");
    }

    #[test]
    fn keyword_inverse_of_known_names() {
        assert_eq!(keyword_for_name("CreationTime"), Some("Creation Time"));
        assert_eq!(keyword_for_name("PNGWarning"), Some("Warning"));
        assert_eq!(keyword_for_name("ModDate"), Some("modify-date"));
        assert_eq!(keyword_for_name("Author"), Some("Author"));
        assert_eq!(keyword_for_name("XMP"), None);
        assert_eq!(keyword_for_name("EXIF_Profile"), None);
        assert_eq!(keyword_for_name("ExifMake"), None);
    }

    #[test]
    fn png_date_conversion() {
        // Every row is ExifTool 13.59 output for a tEXt 'Creation Time'
        // chunk carrying the input (pngimpl/d1.png, `-G1 -s -a`).
        let cases = [
            ("Mon, 1 Jan 2018 12:10:22 EST", "2018:01:01 12:10:22-05:00"),
            ("1 Jan 2020 10:00 Z", "2020:01:01 10:00:00+00:00"),
            ("1 Jan 2020 10:00:00", "2020:01:01 10:00:00"),
            ("5 feb 99 03:04:05 +0530", "1999:02:05 03:04:05+05:30"),
            ("5 Feb 19 03:04:05 -05:00", "2019:02:05 03:04:05-05:00"),
            // unknown zone => `last` => unchanged
            ("1 Jan 2020 10:00:00 XYZ", "1 Jan 2020 10:00:00 XYZ"),
            // '0' is false in Perl: no zone
            ("1 Jan 2020 10:00:00 0", "2020:01:01 10:00:00"),
            ("x 12Jan70 1:2345 cdt", "x 12Jan70 1:2345 cdt"),
            // the first match decides; a later valid date is never tried
            (
                "1 Jan 2020 10:00:00 XYZ 2 Feb 2021 11:00 GMT",
                "1 Jan 2020 10:00:00 XYZ 2 Feb 2021 11:00 GMT",
            ),
            ("31 dec 2020  9:05:07GMT", "2020:12:31 09:05:07+00:00"),
            ("1 Jan 2020 10:00 +5:30x", "2020:01:01 10:00:00+5:30"),
            ("1 Jan 2020 10:00 -123", "2020:01:01 10:00:00-1:23"),
            ("hello", "hello"),
            ("007 Mar 0005 1:00 j", "007 Mar 0005 1:00 j"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                convert_png_date(input).as_deref(),
                Some(expected),
                "{input:?}"
            );
        }
        assert_eq!(
            convert_png_date("1 Jan 99999999999999999999 10:00"),
            None,
            "a year Perl would numify to a float is omitted, not guessed"
        );
    }

    #[test]
    fn xmp_date_conversion() {
        // pngimpl/d2.png (tEXt 'create-date'), ExifTool 13.59
        let cases = [
            ("2020-01-02T03:04:05Z", "2020:01:02 03:04:05Z"),
            ("2020-01-02 03:04", "2020:01:02 03:04"),
            ("2020-01", "2020:01"),
            ("2020-01-02T03:04:5Z", "2020:01:02 03:04:5Z"),
            // Perl `$` matches before a final newline
            ("2020-01-02T03:04:05Z\n", "2020:01:02 03:04:05Z"),
            ("2020-01-02T03:04:05 Z x", "2020:01:02T03:04:05 Z x"),
            ("20201-01", "20201:01"),
            ("ts", "ts"),
            ("2025-10-30T11:57:59+00:00", "2025:10:30 11:57:59+00:00"),
        ];
        for (input, expected) in cases {
            assert_eq!(convert_xmp_date(input), expected, "{input:?}");
        }
    }
}
