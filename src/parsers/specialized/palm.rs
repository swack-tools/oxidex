//! Palm database (.pdb, .prc) and Mobipocket / Kindle (.mobi, .azw, .azw3)
//! metadata parser.
//!
//! ExifTool 13.59 routes these through `Image::ExifTool::Palm::ProcessPDB`
//! (Palm.pm:285-360): an 86-byte Palm database header whose bytes 60..68 must
//! be one of 28 known type/creator pairs (Palm.pm:23-52, 294-295), then --
//! only for Mobipocket -- a 274-byte MOBI header at the first record, the
//! `BookName` string it points at, and a variable-length `EXTH` extended
//! header of `(tag, length, value)` records.
//!
//! # What comes from the generated tables and what does not
//!
//! `Palm::Main` (Palm.pm:67-100) and `Palm::MOBI` (Palm.pm:104-165) are both
//! `ProcessBinaryData` tables with `FORMAT => 'int32u'`, so their byte layout
//! is read from `exiftool_tables` rather than restated here. Three things in
//! them are not:
//!
//! * The three `Palm::Main` dates carry `%dateTimeInfo` (Palm.pm:54-64), whose
//!   `RawConv` re-bases a Mac-epoch timestamp onto the Unix epoch and whose
//!   `ValueConv` is `ConvertUnixTime($val, 1)`. The generator flags both as
//!   [`Omitted`], so they are hand-implemented behind a [`RawAccess`]
//!   citation.
//! * `Palm::MOBI`'s `UncompressedTextLength` declares `PrintConv =>
//!   \&Image::ExifTool::ConvertFileSize` (Palm.pm:123). A Perl code ref is not
//!   something the transcription can reproduce, so the generated field carries
//!   `PrintConv::None` and the table fails Gate A with `conv_dropped: 1`.
//!   Calling `.emit()` on it would report the raw byte count where ExifTool
//!   prints `172 kB` -- a plausible wrong value under a real tag name -- so
//!   that one field is converted by hand instead.
//! * `BookName` (index 21) is an offset in the record, which Palm.pm:326-330
//!   overwrites with the string it points at. The table's integer is
//!   discarded for the same reason.
//!
//! `Palm::EXTH` (Palm.pm:168-244) is not a `ProcessBinaryData` table at all --
//! it declares `PROCESS_PROC => \&ProcessEXTH` -- so it is absent from the
//! generated registry and its ID-to-name map is transcribed below, each entry
//! against its Perl line.
//!
//! # Deliberately absent
//!
//! Palm.pm comments out six EXTH IDs -- 121 `KF8BoundaryOffset`
//! (Palm.pm:204), 201 `CoverOffset` and 202 `ThumbOffset` (Palm.pm:208-209),
//! 203 `HasFakeCover` (Palm.pm:210) and 300 `FontSignature` (Palm.pm:227).
//! ExifTool does not extract them, so neither does this; `Palm.mobi` carries
//! four of the five, which is why that matters.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Palm.pm`

use crate::core::value_formatter::format_file_size;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;
use crate::io::timestamp::unix_time_to_local_exif_datetime;
use crate::parsers::text::html::decode_latin;

/// Palm.pm:293, `$raf->Read($buff, 86) == 86`.
const PDB_HEADER_LEN: usize = 86;
/// Palm.pm:294, `$palmTypes{substr($buff, 60, 8)}`.
const TYPE_CREATOR_OFFSET: usize = 60;
/// Palm.pm:310, `$raf->Read($buff, 274) == 274`.
const MOBI_HEADER_LEN: usize = 274;

const fn citation(table: &'static str, tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "Palm",
        table,
        tag,
        lines,
    }
}

/// `%dateTimeInfo` (Palm.pm:54-64), shared by all three `Palm::Main` dates.
const CREATE_DATE: PerlCitation = citation("Main", "CreateDate", "Palm.pm:54-64,79-83");
const MODIFY_DATE: PerlCitation = citation("Main", "ModifyDate", "Palm.pm:54-64,84-88");
const LAST_BACKUP_DATE: PerlCitation = citation("Main", "LastBackupDate", "Palm.pm:54-64,89-93");
/// Palm.pm:154, `RawConv => '$$self{CodePage} = $val'` -- a side effect that
/// leaves the value itself untouched.
const CODE_PAGE: PerlCitation = citation("MOBI", "CodePage", "Palm.pm:152-161");

/// Extract Palm database / Mobipocket metadata.
pub fn parse_palm_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < PDB_HEADER_LEN as u64 {
        return Err("Palm file is too short for the 86-byte header".to_string());
    }
    let header = reader
        .read(0, PDB_HEADER_LEN)
        .map_err(|error| error.to_string())?;

    // Palm.pm:294-295, `return 0 unless $type`.
    let type_creator = &header[TYPE_CREATOR_OFFSET..TYPE_CREATOR_OFFSET + 8];
    let is_mobipocket = palm_type(type_creator)
        .ok_or("unrecognised Palm type/creator pair")?
        .eq("Mobipocket");

    let mut metadata = MetadataMap::new();
    read_palm_header(&header, &mut metadata)?;

    // Palm.pm:305, `return 1 unless $type eq 'Mobipocket' and Get16u(\$buff, 76)`
    // -- the second test is the record count, i.e. whether there is a first
    // record to seek to at all.
    let record_count = u16::from_be_bytes([header[76], header[77]]);
    if !is_mobipocket || record_count == 0 {
        return Ok(metadata);
    }

    // Palm.pm:309-313. A truncated MOBI header is an ExifTool *warning*, not
    // a failure: the Palm header tags already extracted still stand.
    let offset = u64::from(u32::from_be_bytes([
        header[78], header[79], header[80], header[81],
    ]));
    let Ok(mobi) = reader.read(offset, MOBI_HEADER_LEN) else {
        return Ok(metadata);
    };
    if mobi.len() < MOBI_HEADER_LEN {
        return Ok(metadata);
    }
    // Palm.pm:314-317, `substr($buff, 16, 4) eq 'MOBI'`.
    if &mobi[16..20] != b"MOBI" {
        return Ok(metadata);
    }

    let code_page = read_mobi_header(&mobi, &mut metadata)?;
    // Palm.pm:322-323: an unrecognised code page falls back to UTF-8.
    let encoding = Encoding::for_code_page(code_page);

    read_book_name(reader, offset, &mobi, encoding, &mut metadata);
    read_exth(reader, offset, &mobi, encoding, &mut metadata);

    Ok(metadata)
}

/// Palm.pm:302-303: `Palm::Main` over the 86-byte database header, big-endian
/// (Palm.pm:300, `SetByteOrder('MM')`).
fn read_palm_header(header: &[u8], metadata: &mut MetadataMap) -> std::result::Result<(), String> {
    let table = find_table("Palm", "Main").ok_or("missing Palm::Main table")?;
    for decoded in decode_binary_table(table, header, ByteOrder::Big).fields() {
        let name = decoded.field.name;
        let key = format!("Palm:{name}");
        let date_citation = match name {
            "CreateDate" => Some(&CREATE_DATE),
            "ModifyDate" => Some(&MODIFY_DATE),
            "LastBackupDate" => Some(&LAST_BACKUP_DATE),
            _ => None,
        };
        match date_citation {
            Some(cited) => {
                if let Some(access) = RawAccess::new(
                    decoded,
                    Acknowledged::VALUE_CONV | Acknowledged::RAW_CONV,
                    cited,
                ) && let Some(raw) = access.raw().as_integer()
                {
                    metadata.insert(key, TagValue::new_string(palm_date(raw)));
                }
            }
            None => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }
    Ok(())
}

/// Palm.pm:318-319: `Palm::MOBI` over the 274-byte MOBI header. Returns the
/// `CodePage` value, which Palm.pm:154's `RawConv` stashes for the text
/// decoding that follows.
fn read_mobi_header(
    mobi: &[u8],
    metadata: &mut MetadataMap,
) -> std::result::Result<Option<i64>, String> {
    let table = find_table("Palm", "MOBI").ok_or("missing Palm::MOBI table")?;
    let mut code_page = None;
    for decoded in decode_binary_table(table, mobi, ByteOrder::Big).fields() {
        let name = decoded.field.name;
        let key = format!("MOBI:{name}");
        match name {
            // Palm.pm:163: the table value is an offset, replaced with the
            // string it points at by `read_book_name`.
            "BookName" => {}
            "UncompressedTextLength" => {
                // Palm.pm:122-124's `PrintConv => \&ConvertFileSize`, which
                // the generator could not transcribe (`conv_dropped`).
                if let Some(TagValue::Integer(bytes)) = decoded.emit()
                    && let Ok(bytes) = u64::try_from(bytes)
                {
                    metadata.insert(key, TagValue::new_string(format_file_size(bytes)));
                }
            }
            "CodePage" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &CODE_PAGE) {
                    code_page = access.raw().as_integer();
                    // The `RawConv` only records the value; the tag itself
                    // still renders through the table's own `PrintConv`.
                    if let Some(raw) = code_page {
                        metadata.insert(key, code_page_value(raw));
                    }
                }
            }
            _ => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }
    Ok(code_page)
}

/// Palm.pm:155-160's `PrintConv`, applied by hand because the value reaches
/// here through [`RawAccess`] rather than `.emit()`.
fn code_page_value(raw: i64) -> TagValue {
    match raw {
        1252 => TagValue::new_string("Windows Latin 1 (Western European)"),
        65001 => TagValue::new_string("Unicode (UTF-8)"),
        other => TagValue::Integer(other),
    }
}

/// Palm.pm:325-330: the `BookName` string lives at `offset + Get32u($buff,84)`
/// and runs for `Get32u($buff,88)` bytes.
fn read_book_name(
    reader: &dyn FileReader,
    offset: u64,
    mobi: &[u8],
    encoding: Encoding,
    metadata: &mut MetadataMap,
) {
    let off = u64::from(u32::from_be_bytes([mobi[84], mobi[85], mobi[86], mobi[87]]));
    let len = u32::from_be_bytes([mobi[88], mobi[89], mobi[90], mobi[91]]) as usize;
    // Palm.pm:329: a failed read stores the literal `<err>`, which ExifTool
    // then reports as the tag value.
    let value = match reader.read(offset + off, len) {
        Ok(bytes) if bytes.len() == len => encoding.decode(&bytes),
        _ => "<err>".to_string(),
    };
    metadata.insert("MOBI:BookName", TagValue::new_string(value));
}

/// Palm.pm:334-357 plus `ProcessEXTH` (Palm.pm:250-279).
fn read_exth(
    reader: &dyn FileReader,
    offset: u64,
    mobi: &[u8],
    encoding: Encoding,
    metadata: &mut MetadataMap,
) {
    // Palm.pm:335-336, `return 1 unless $flag & 0x40`.
    let flag = u32::from_be_bytes([mobi[128], mobi[129], mobi[130], mobi[131]]);
    if flag & 0x40 == 0 {
        return;
    }
    // Palm.pm:338: MOBI header length, including the 16-byte PalmDOC header.
    let header_len = u64::from(u32::from_be_bytes([mobi[20], mobi[21], mobi[22], mobi[23]])) + 16;
    // Palm.pm:340-345: `EXTH` magic plus a total size that must exceed its
    // own 12-byte header.
    let Ok(exth_header) = reader.read(offset + header_len, 12) else {
        return;
    };
    if exth_header.len() < 12 || &exth_header[0..4] != b"EXTH" {
        return;
    }
    let size = u32::from_be_bytes([
        exth_header[4],
        exth_header[5],
        exth_header[6],
        exth_header[7],
    ]);
    if size <= 12 {
        return;
    }
    let body_len = (size - 12) as usize;
    // Palm.pm:349: a short read is a warning, and no entries are processed.
    let Ok(body) = reader.read(offset + header_len + 12, body_len) else {
        return;
    };
    if body.len() < body_len {
        return;
    }

    // Palm.pm:262-277's entry walk.
    let mut pos = 0usize;
    while pos + 8 <= body.len() {
        let tag = u32::from_be_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]);
        let len = u32::from_be_bytes([body[pos + 4], body[pos + 5], body[pos + 6], body[pos + 7]])
            as usize;
        // Palm.pm:266, `last if $len < 8 or $pos + $len > $dirLen`.
        if len < 8 || pos + len > body.len() {
            break;
        }
        let value = &body[pos + 8..pos + len];
        if let Some(entry) = exth_tag(tag)
            && let Some(tag_value) = entry.decode(value, encoding)
        {
            let key = format!("MOBI:{}", entry.name);
            match (entry.format, metadata.get(&key)) {
                // A `List => 1` tag accumulates; ExifTool's `-s` output joins
                // the items with `, ` (ExifTool.pm's `$$self{OPTIONS}{ListSep}`,
                // default `', '`).
                (ExthFormat::List, Some(TagValue::String(existing))) => {
                    let combined = match &tag_value {
                        TagValue::String(next) => format!("{existing}, {next}"),
                        _ => existing.clone(),
                    };
                    metadata.insert(key, TagValue::new_string(combined));
                }
                _ => {
                    metadata.insert(key, tag_value);
                }
            }
        }
        pos += len;
    }
}

/// The `Palm::EXTH` value formats. The table's `FORMAT => 'string'`
/// (Palm.pm:170) is the default; the handful of entries that override it
/// declare `Format => 'int32u'` or `Format => 'int8u'`.
#[derive(Clone, Copy)]
enum ExthFormat {
    Str,
    Int32u,
    Int8u,
    /// A `string` field carrying `List => 1` (Palm.pm:181).
    List,
    /// A `string` field whose `ValueConv` is
    /// `Image::ExifTool::XMP::ConvertXMPDate($val, 1)` (Palm.pm:185-188).
    XmpDate,
}

struct ExthTag {
    name: &'static str,
    format: ExthFormat,
    /// The `PrintConv` hash, where one is declared.
    print_conv: &'static [(i64, &'static str)],
}

impl ExthTag {
    fn decode(&self, value: &[u8], encoding: Encoding) -> Option<TagValue> {
        match self.format {
            ExthFormat::Str | ExthFormat::List => {
                // ExifTool.pm:6310, `$val =~ s/\0.*//s` for `string`, then
                // Palm.pm:275's `$et->Decode($val, $enc)`.
                Some(TagValue::new_string(
                    encoding.decode(truncate_at_null(value)),
                ))
            }
            ExthFormat::XmpDate => Some(TagValue::new_string(convert_xmp_date(
                &encoding.decode(truncate_at_null(value)),
            ))),
            ExthFormat::Int32u => {
                let bytes: [u8; 4] = value.get(..4)?.try_into().ok()?;
                Some(self.render(i64::from(u32::from_be_bytes(bytes))))
            }
            ExthFormat::Int8u => Some(self.render(i64::from(*value.first()?))),
        }
    }

    fn render(&self, raw: i64) -> TagValue {
        match self.print_conv.iter().find(|(key, _)| *key == raw) {
            Some((_, text)) => TagValue::new_string(*text),
            None => TagValue::Integer(raw),
        }
    }
}

const fn text(name: &'static str) -> ExthTag {
    ExthTag {
        name,
        format: ExthFormat::Str,
        print_conv: &[],
    }
}

const fn int32u(name: &'static str) -> ExthTag {
    ExthTag {
        name,
        format: ExthFormat::Int32u,
        print_conv: &[],
    }
}

/// `%Image::ExifTool::Palm::EXTH` (Palm.pm:168-244). Not in the generated
/// registry: the table declares `PROCESS_PROC => \&ProcessEXTH`, so the
/// transcription pipeline -- which only walks `ProcessBinaryData` tables --
/// never saw it.
fn exth_tag(tag: u32) -> Option<ExthTag> {
    Some(match tag {
        1 => text("DRMServerID"),      // Palm.pm:173
        2 => text("DRMCommerceID"),    // Palm.pm:174
        3 => text("DRM_E-BookBaseID"), // Palm.pm:175
        100 => text("Author"),         // Palm.pm:176
        101 => text("Publisher"),      // Palm.pm:177
        102 => text("Imprint"),        // Palm.pm:178
        103 => text("Description"),    // Palm.pm:179
        104 => text("ISBN"),           // Palm.pm:180
        105 => ExthTag {
            name: "Subject",
            // Palm.pm:181, `List => 1`: repeated entries accumulate into one
            // value rather than the last one winning.
            format: ExthFormat::List,
            print_conv: &[],
        },
        106 => ExthTag {
            name: "PublishDate",
            // Palm.pm:185-189.
            format: ExthFormat::XmpDate,
            print_conv: &[],
        },
        107 => text("Review"),              // Palm.pm:191
        108 => text("Contributor"),         // Palm.pm:192
        109 => text("Rights"),              // Palm.pm:193
        110 => text("SubjectCode"),         // Palm.pm:194
        111 => text("BookType"),            // Palm.pm:195
        112 => text("Source"),              // Palm.pm:196
        113 => text("ASIN"),                // Palm.pm:197
        114 => text("BookVersion"),         // Palm.pm:198
        115 => int32u("SampleFlag"),        // Palm.pm:199
        116 => int32u("StartReading"),      // Palm.pm:200
        117 => text("Adult"),               // Palm.pm:201
        118 => text("RetailPrice"),         // Palm.pm:202
        119 => text("RetailPriceCurrency"), // Palm.pm:203
        125 => int32u("ResourceCount"),     // Palm.pm:205
        129 => text("KF8CoverURI"),         // Palm.pm:206
        200 => text("DictionaryShortName"), // Palm.pm:207
        204 => ExthTag {
            name: "CreatorSoftware",
            format: ExthFormat::Int32u,
            // Palm.pm:214-220.
            print_conv: &[
                (1, "Mobigen"),
                (2, "Mobipocket"),
                (200, "Kindlegen (Windows)"),
                (201, "Kindlegen (Linux)"),
                (202, "Kindlegen (Mac)"),
            ],
        },
        205 => int32u("CreatorMajorVersion"), // Palm.pm:222
        206 => int32u("CreatorMinorVersion"), // Palm.pm:223
        207 => int32u("CreatorBuildNumber"),  // Palm.pm:224
        208 => text("Watermark"),             // Palm.pm:225
        209 => text("Tamper-proofKeys"),      // Palm.pm:226
        401 => ExthTag {
            name: "ClippingLimit",
            format: ExthFormat::Int8u,
            print_conv: &[],
        }, // Palm.pm:228
        402 => text("PublisherLimit"),        // Palm.pm:229
        404 => ExthTag {
            name: "TextToSpeech",
            format: ExthFormat::Int8u,
            // Palm.pm:233.
            print_conv: &[(0, "Enabled"), (1, "Disabled")],
        },
        405 => ExthTag {
            name: "RentalFlag",
            format: ExthFormat::Int8u,
            print_conv: &[],
        }, // Palm.pm:235
        406 => text("RentalExpirationDate"), // Palm.pm:236
        501 => int32u("CDEType"),            // Palm.pm:237
        502 => text("LastUpdateTime"),       // Palm.pm:238
        503 => text("UpdatedTitle"),         // Palm.pm:239
        504 => text("ASIN2"),                // Palm.pm:240
        524 => text("Language"),             // Palm.pm:241
        525 => text("Alignment"),            // Palm.pm:242
        535 => text("CreatorBuildNumber2"),  // Palm.pm:243
        _ => return None,
    })
}

/// ExifTool.pm:6310's `string` rule, `$val =~ s/\0.*//s`.
fn truncate_at_null(value: &[u8]) -> &[u8] {
    let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
    &value[..end]
}

/// `Image::ExifTool::XMP::ConvertXMPDate($val, 1)` (XMP.pm:3383-3394).
///
/// With `$unsure` set, only the first branch can fire: a full
/// `YYYY-MM-DD` + `T`-or-space + `HH:MM` (+ optional `:SS`) + optional
/// trailing zone is rewritten into EXIF's colon-separated form. Anything else
/// is returned verbatim -- including the `elsif` branch's dash-to-colon
/// rewrite, which `$unsure` disables. The `PrintConv`,
/// `$self->ConvertDateTime($val)`, is the identity with no `-dateFormat` in
/// play, so this is the whole chain.
fn convert_xmp_date(value: &str) -> String {
    let bytes = value.as_bytes();
    // ^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})
    if bytes.len() < 16 {
        return value.to_string();
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if !(digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && matches!(bytes[10], b'T' | b' ')
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16))
    {
        return value.to_string();
    }
    let mut rest = &value[16..];
    // (:\d{2})?
    let mut seconds = "";
    if rest.len() >= 3 && rest.as_bytes()[0] == b':' && digits(17..19) {
        seconds = &rest[..3];
        rest = &rest[3..];
    }
    // \s*(\S*)$ -- the trailing group must run to end of string, so any
    // interior whitespace means the pattern did not match at all.
    let zone = rest.trim_start();
    if zone.chars().any(char::is_whitespace) {
        return value.to_string();
    }
    format!(
        "{}:{}:{} {}{seconds}{zone}",
        &value[0..4],
        &value[5..7],
        &value[8..10],
        &value[11..16]
    )
}

/// The text encodings Palm.pm:322-323 can select.
///
/// `$Image::ExifTool::charsetName{"cp$CodePage"}` (ExifTool.pm:1077-1090) maps
/// only a handful of code pages, and anything it does not map falls back to
/// UTF-8. Rather than guess at the ones this crate has no table for, an
/// unhandled *mapped* code page suppresses the tag entirely -- decoding cp1251
/// bytes as UTF-8 would put a plausible wrong string under a real tag name.
#[derive(Clone, Copy, PartialEq)]
enum Encoding {
    /// `cp65001 => 'UTF8'` (ExifTool.pm:1080), and Palm.pm:323's fallback for
    /// any code page `%charsetName` does not name.
    Utf8,
    /// `cp1252 => 'Latin'` (ExifTool.pm:1081).
    Latin,
    /// A code page `%charsetName` names but this crate has no table for.
    Unsupported,
}

impl Encoding {
    fn for_code_page(code_page: Option<i64>) -> Self {
        match code_page {
            Some(1252) => Self::Latin,
            Some(65001) | None => Self::Utf8,
            // ExifTool.pm:1082-1090's remaining `cpNNNN` aliases.
            Some(
                1250 | 1251 | 1253 | 1254 | 1255 | 1256 | 1257 | 1258 | 874 | 932 | 936 | 949 | 950,
            ) => Self::Unsupported,
            // Not in `%charsetName` at all, so Palm.pm:323 uses UTF-8.
            Some(_) => Self::Utf8,
        }
    }

    fn decode(self, bytes: &[u8]) -> String {
        match self {
            Self::Latin => decode_latin(bytes),
            // `$et->Decode($val, 'UTF8')` with the default UTF-8 Charset is a
            // no-op on well-formed input.
            Self::Utf8 | Self::Unsupported => String::from_utf8_lossy(bytes).into_owned(),
        }
    }
}

/// `%dateTimeInfo`'s `RawConv` + `ValueConv` (Palm.pm:57-63):
///
/// ```text
/// my $offset = (66 * 365 + 17) * 24 * 3600;
/// return $val - $offset if $val >= $offset;
/// return $val;
/// ...
/// ValueConv => 'ConvertUnixTime($val, 1)'
/// ```
///
/// The re-base exists because the Palm epoch is nominally 1904-01-01 but not
/// all writers use it (Palm.pm:55-56), so a value large enough to be a Mac
/// timestamp is treated as one and anything smaller is left as a Unix time.
fn palm_date(raw: i64) -> String {
    const MAC_EPOCH_OFFSET: i64 = (66 * 365 + 17) * 24 * 3600;
    let seconds = if raw >= MAC_EPOCH_OFFSET {
        raw - MAC_EPOCH_OFFSET
    } else {
        raw
    };
    unix_time_to_local_exif_datetime(seconds)
}

/// `%palmTypes` (Palm.pm:23-52). Only the membership test matters here --
/// the *displayed* `PalmFileType` comes from the generated table's own
/// `PrintConv` -- but Palm.pm:294-295 rejects the file outright when the pair
/// is unknown, and Palm.pm:299 branches on `Mobipocket`.
fn palm_type(type_creator: &[u8]) -> Option<&'static str> {
    let key = std::str::from_utf8(type_creator).ok()?;
    Some(match key {
        ".pdfADBE" => "Adobe Reader",
        "TEXtREAd" => "PalmDOC",
        "BVokBDIC" => "BDicty",
        "DB99DBOS" => "DB (Database program)",
        "PNRdPPrs" | "DataPPrs" => "eReader",
        "vIMGView" => "FireViewer (ImageViewer)",
        "PmDBPmDB" => "HanDBase",
        "InfoINDB" => "InfoView",
        "ToGoToGo" => "iSilo",
        "SDocSilX" => "iSilo 3",
        "JbDbJBas" => "JFile",
        "JfDbJFil" => "JFile Pro",
        "DATALSdb" => "LIST",
        "Mdb1Mdb1" => "MobileDB",
        "BOOKMOBI" => "Mobipocket",
        "DataPlkr" => "Plucker",
        "DataSprd" => "QuickSheet",
        "SM01SMem" => "SuperMemo",
        "TEXtTlDc" => "TealDoc",
        "InfoTlIf" => "TealInfo",
        "DataTlMl" => "TealMeal",
        "DataTlPt" => "TealPaint",
        "dataTDBP" => "ThinkDB",
        "TdatTide" => "Tides",
        "ToRaTRPW" => "TomeRaider",
        "zTXTGPlm" => "Weasel",
        "BDOCWrdS" => "WordSmith",
        _ => return None,
    })
}
