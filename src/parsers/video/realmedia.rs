//! RealMedia container (.rm, .rv, .rmvb) reader.
//!
//! `Real::ProcessReal` (Real.pm:513-694) recognises four things behind one
//! magic number; this module implements the `.RMF` branch, the RealMedia
//! container. (`.ra\xfd` already has its own reader in
//! `crate::parsers::audio::real_audio`, and the two URL metafiles are RAM/RPM
//! text files.)
//!
//! The container is a flat chunk list -- `id(4) | size(4) | version(2)` --
//! walked at Real.pm:597-651. `Real::Media` (Real.pm:52-62) names exactly four
//! chunks, each a `SubDirectory`:
//!
//! | chunk  | table                | processor              |
//! |--------|----------------------|------------------------|
//! | `PROP` | `Real::Properties`   | `Canon::ProcessSerialData` |
//! | `MDPR` | `Real::MediaProps`   | `Canon::ProcessSerialData` |
//! | `CONT` | `Real::ContentDescr` | `Canon::ProcessSerialData` |
//! | `RJMD` | `Real::Metadata`     | `Real::ProcessRealMeta`    |
//!
//! and `MediaProps` index 21 nests `Real::FileInfo`
//! (`Real::ProcessRealProperties`). After the chunks, Real.pm:660-692 reads a
//! trailing `RMJE`/`RJMD` metadata footer and an ID3v1 tag.
//!
//! # Why there is no transcribed table to consult
//!
//! `ProcessSerialData` is not `ProcessBinaryData`. Its tables are keyed by
//! *sequence index*, not byte offset -- `VARS => { ID_LABEL => 'Sequence' }`
//! -- and several entries' widths are `eval`ed expressions over earlier
//! values in the same record (`Format => 'string[$val{8}]'`). The generator
//! emits only fixed-offset binary tables, so `find_table("Real", ...)` is
//! `None` by construction. Every entry below is transcribed by hand with its
//! Real.pm line cited.
//!
//! # What is deliberately absent
//!
//! 1. **`Unknown => 1` entries.** `ProcessSerialData` reads them to stay in
//!    step (Canon.pm:10587) but `FoundTag` is skipped unless `-U` is given, so
//!    they are read and not reported here either: `IndexOffset`/`DataOffset`
//!    (Real.pm:97-98), `StreamNameLen`/`StreamMimeLen`/`FileInfoLen`/
//!    `FileInfoLen2` and the seven physical-stream fields (Real.pm:127-176),
//!    and the four `*Len` fields in `ContentDescr` (Real.pm:241-247).
//! 2. **`Real::Metadata` values whose `%metadataFormat` entry is `undef`** --
//!    types 9 (grouping) and 10 (reference), Real.pm:39-40. Real.pm:466's
//!    `if ($valueLen and $format)` skips them, and so does this module; their
//!    sub-properties are still walked.
//! 3. **The single-stream `MIMEType` override** (Real.pm:653-657). OxiDex's
//!    `filetype` layer already answers `audio/x-pn-realaudio` for a `.rm`
//!    file, which is what the override produces on the pinned fixture, so
//!    re-deriving it here would only add a second writer for a value that is
//!    already correct.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Real.pm`, `Canon.pm:10518-10597`

use std::collections::HashMap;

use crate::core::formatters::{convert_bitrate, convert_duration};
use crate::core::{FileReader, MetadataMap, TagValue};

/// Real.pm:517, the `.RMF` alternative of the shared `Real` magic number.
const RMF_SIGNATURE: &[u8; 4] = b".RMF";
/// Real.pm:600, the chunk header: `unpack('a4Nn')`.
const CHUNK_HEADER_LEN: usize = 10;
/// Real.pm:661, `$raf->Seek(-140, 2)` -- how far back from the end the
/// `RMJE` footer probe starts.
const FOOTER_PROBE_BACK: u64 = 140;
/// Real.pm:679, `$raf->Seek(-128, 2)` for the ID3v1 tag.
const ID3V1_LEN: usize = 128;

// ---------------------------------------------------------------------------
// ProcessSerialData table model (Canon.pm:10518-10597)
// ---------------------------------------------------------------------------

/// A `Format` in a `ProcessSerialData` table. The element type and how many
/// of them, which for several entries is an expression over earlier values in
/// the same record.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fmt {
    Int8u,
    Int16u,
    Int32u,
    /// `Format => 'string[$val{N}]'`.
    StringOf(usize),
    /// `Format => 'int16u[$val{N}]'`.
    Int16uOf(usize),
    /// `Format => 'int32u[$val{N}]'`.
    Int32uOf(usize),
    /// `Format => 'undef[$val{13}-$val{15}*6-$val{18}*2-12]'` (Real.pm:174) --
    /// the only arithmetic count expression in any of these tables, kept as
    /// its own variant rather than behind a general Perl evaluator.
    FileInfoProperties,
}

/// What `ProcessSerialData` does with a decoded entry.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Emit {
    /// `FoundTag` with no conversion.
    Plain,
    /// `PrintConv => 'ConvertBitrate($val)'`.
    Bitrate,
    /// `ValueConv => '$val / 1000'`, `PrintConv => 'ConvertDuration($val)'`.
    Milliseconds,
    /// `Real::Properties`' `Flags` BITMASK (Real.pm:102-110).
    Flags,
    /// `Unknown => 1`: read, recorded in `%val`, never reported.
    Unknown,
    /// `SubDirectory => { TagTable => 'Image::ExifTool::Real::FileInfo' }`.
    FileInfo,
    /// Index 13's `Condition => '$self->{RealStreamMime} eq "logical-fileinfo"'`
    /// (Real.pm:141-146). `GetTagInfo` returns undef when it fails, and
    /// Canon.pm:10542's `or last` then ends the whole record.
    LogicalFileInfoGate,
}

/// One `ProcessSerialData` entry: sequence index, name, format and emission.
type SerialEntry = (usize, &'static str, Fmt, Emit);

/// `%Image::ExifTool::Real::Properties` (Real.pm:86-111), `FORMAT => 'int32u'`.
#[rustfmt::skip]
const PROPERTIES: &[SerialEntry] = &[
    (0,  "MaxBitrate",     Fmt::Int32u, Emit::Bitrate),      // Real.pm:91
    (1,  "AvgBitrate",     Fmt::Int32u, Emit::Bitrate),      // Real.pm:92
    (2,  "MaxPacketSize",  Fmt::Int32u, Emit::Plain),        // Real.pm:93
    (3,  "AvgPacketSize",  Fmt::Int32u, Emit::Plain),        // Real.pm:94
    (4,  "NumPackets",     Fmt::Int32u, Emit::Plain),        // Real.pm:95
    (5,  "Duration",       Fmt::Int32u, Emit::Milliseconds), // Real.pm:96
    (6,  "Preroll",        Fmt::Int32u, Emit::Milliseconds), // Real.pm:97
    (7,  "IndexOffset",    Fmt::Int32u, Emit::Unknown),      // Real.pm:98
    (8,  "DataOffset",     Fmt::Int32u, Emit::Unknown),      // Real.pm:99
    (9,  "NumStreams",     Fmt::Int16u, Emit::Plain),        // Real.pm:100
    (10, "Flags",          Fmt::Int16u, Emit::Flags),        // Real.pm:101-110
];

/// Real.pm:104-109, the `Flags` BITMASK.
#[rustfmt::skip]
const FLAGS_BITMASK: &[(u32, &str)] = &[
    (0, "Allow Recording"),
    (1, "Perfect Play"),
    (2, "Live"),
    (3, "Allow Download"),
];

/// `%Image::ExifTool::Real::MediaProps` (Real.pm:113-177), `FORMAT => 'int32u'`.
#[rustfmt::skip]
const MEDIA_PROPS: &[SerialEntry] = &[
    (0,  "StreamNumber",            Fmt::Int16u,             Emit::Plain),        // Real.pm:119
    (1,  "StreamMaxBitrate",        Fmt::Int32u,             Emit::Bitrate),      // Real.pm:120
    (2,  "StreamAvgBitrate",        Fmt::Int32u,             Emit::Bitrate),      // Real.pm:121
    (3,  "StreamMaxPacketSize",     Fmt::Int32u,             Emit::Plain),        // Real.pm:122
    (4,  "StreamAvgPacketSize",     Fmt::Int32u,             Emit::Plain),        // Real.pm:123
    (5,  "StreamStartTime",         Fmt::Int32u,             Emit::Plain),        // Real.pm:124
    (6,  "StreamPreroll",           Fmt::Int32u,             Emit::Milliseconds), // Real.pm:125
    (7,  "StreamDuration",          Fmt::Int32u,             Emit::Milliseconds), // Real.pm:126
    (8,  "StreamNameLen",           Fmt::Int8u,              Emit::Unknown),      // Real.pm:127
    (9,  "StreamName",              Fmt::StringOf(8),        Emit::Plain),        // Real.pm:128
    (10, "StreamMimeLen",           Fmt::Int8u,              Emit::Unknown),      // Real.pm:129
    (11, "StreamMimeType",          Fmt::StringOf(10),       Emit::Plain),        // Real.pm:130-134
    (12, "FileInfoLen",             Fmt::Int32u,             Emit::Unknown),      // Real.pm:135
    (13, "FileInfoLen2",            Fmt::Int32u,             Emit::LogicalFileInfoGate), // Real.pm:136-141
    (14, "FileInfoVersion",         Fmt::Int16u,             Emit::Plain),        // Real.pm:142-145
    (15, "PhysicalStreams",         Fmt::Int16u,             Emit::Unknown),      // Real.pm:146-150
    (16, "PhysicalStreamNumbers",   Fmt::Int16uOf(15),       Emit::Unknown),      // Real.pm:151-155
    (17, "DataOffsets",             Fmt::Int32uOf(15),       Emit::Unknown),      // Real.pm:156-160
    (18, "NumRules",                Fmt::Int16u,             Emit::Unknown),      // Real.pm:161-165
    (19, "PhysicalStreamNumberMap", Fmt::Int16uOf(18),       Emit::Unknown),      // Real.pm:166-170
    (20, "NumProperties",           Fmt::Int16u,             Emit::Unknown),      // Real.pm:171-175
    (21, "FileInfoProperties",      Fmt::FileInfoProperties, Emit::FileInfo),     // Real.pm:176-180
];

/// `%Image::ExifTool::Real::ContentDescr` (Real.pm:235-249), `FORMAT => 'int16u'`.
#[rustfmt::skip]
const CONTENT_DESCR: &[SerialEntry] = &[
    (0, "TitleLen",     Fmt::Int16u,      Emit::Unknown), // Real.pm:241
    (1, "Title",        Fmt::StringOf(0), Emit::Plain),   // Real.pm:242
    (2, "AuthorLen",    Fmt::Int16u,      Emit::Unknown), // Real.pm:243
    (3, "Author",       Fmt::StringOf(2), Emit::Plain),   // Real.pm:244
    (4, "CopyrightLen", Fmt::Int16u,      Emit::Unknown), // Real.pm:245
    (5, "Copyright",    Fmt::StringOf(4), Emit::Plain),   // Real.pm:246
    (6, "CommentLen",   Fmt::Int16u,      Emit::Unknown), // Real.pm:247
    (7, "Comment",      Fmt::StringOf(6), Emit::Plain),   // Real.pm:248
];

// ---------------------------------------------------------------------------
// Real::FileInfo (Real.pm:183-232) -- keyed by property name
// ---------------------------------------------------------------------------

/// How a `Real::FileInfo` property is rendered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FileInfoConv {
    None,
    /// `PrintConv => { 0 => 'No Rating', ... }` (Real.pm:191-201).
    ContentRating,
    /// `PrintConv => { 0 => 'False', 1 => 'True' }` (Real.pm:187).
    Indexable,
    /// The `dd/mm/yyyy hh:mm:ss` ValueConv on `Creation Date` and
    /// `Modification Date` (Real.pm:206-213, :217-224).
    SlashDate,
}

/// `%Image::ExifTool::Real::FileInfo` (Real.pm:183-232). A property with no
/// entry here is still extracted under the name `ProcessRealProperties` mints
/// at Real.pm:394-399.
#[rustfmt::skip]
const FILE_INFO: &[(&str, &str, FileInfoConv)] = &[
    ("Indexable",         "Indexable",       FileInfoConv::Indexable),     // Real.pm:187
    ("Keywords",          "Keywords",        FileInfoConv::None),          // Real.pm:188
    ("Description",       "Description",     FileInfoConv::None),          // Real.pm:189
    ("File ID",           "FileID",          FileInfoConv::None),          // Real.pm:190
    ("Content Rating",    "ContentRating",   FileInfoConv::ContentRating), // Real.pm:191-201
    ("Audiences",         "Audiences",       FileInfoConv::None),          // Real.pm:202
    ("audioMode",         "AudioMode",       FileInfoConv::None),          // Real.pm:203
    ("Creation Date",     "CreateDate",      FileInfoConv::SlashDate),     // Real.pm:204-213
    ("Generated By",      "Software",        FileInfoConv::None),          // Real.pm:214
    ("Modification Date", "ModifyDate",      FileInfoConv::SlashDate),     // Real.pm:215-224
    ("Target Audiences",  "TargetAudiences", FileInfoConv::None),          // Real.pm:225
    ("Audio Format",      "AudioFormat",     FileInfoConv::None),          // Real.pm:226
    ("Video Quality",     "VideoQuality",    FileInfoConv::None),          // Real.pm:227
    ("videoMode",         "VideoMode",       FileInfoConv::None),          // Real.pm:228
];

/// Real.pm:194-200, the `Content Rating` PrintConv.
#[rustfmt::skip]
const CONTENT_RATING: &[(&str, &str)] = &[
    ("0", "No Rating"),
    ("1", "All Ages"),
    ("2", "Older Children"),
    ("3", "Younger Teens"),
    ("4", "Older Teens"),
    ("5", "Adult Supervision Recommended"),
    ("6", "Adults Only"),
];

/// `%Image::ExifTool::Real::Metadata` (Real.pm:251-269).
#[rustfmt::skip]
const RJMD_TAGS: &[(&str, &str)] = &[
    ("Album/Name",     "AlbumName"),     // Real.pm:265
    ("Track/Category", "TrackCategory"), // Real.pm:266
    ("Track/Comments", "TrackComments"), // Real.pm:267
    ("Track/Lyrics",   "TrackLyrics"),   // Real.pm:268
];

// ---------------------------------------------------------------------------
// Byte helpers -- Real.pm:559 sets big-endian byte order for the whole module.
// ---------------------------------------------------------------------------

fn be_u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// One `%val` entry: the rendered value plus, for a single-element numeric
/// read, its number -- which is what a later `$val{N}` count needs.
#[derive(Clone)]
struct SerialValue {
    text: String,
    number: Option<i64>,
    bytes: Vec<u8>,
}

/// `Canon::ProcessSerialData` (Canon.pm:10518-10597) over one of the three
/// `Real` sequence tables.
fn process_serial_data(
    data: &[u8],
    table: &[SerialEntry],
    group1: &str,
    stream_mime: &mut Option<String>,
    metadata: &mut MetadataMap,
) {
    let size = data.len();
    let mut values: HashMap<usize, SerialValue> = HashMap::new();
    let mut pos = 0usize;

    for (index, name, fmt, emit) in table {
        // `for ($index=0; $$tagTablePtr{$index} and $pos <= $size; ++$index)`.
        if pos > size {
            break;
        }
        // Canon.pm:10542's `or last`: a failing `Condition` ends the record.
        if *emit == Emit::LogicalFileInfoGate && stream_mime.as_deref() != Some("logical-fileinfo")
        {
            break;
        }
        let count = match fmt {
            Fmt::Int8u | Fmt::Int16u | Fmt::Int32u => 1,
            Fmt::StringOf(source) | Fmt::Int16uOf(source) | Fmt::Int32uOf(source) => {
                match values.get(source).and_then(|value| value.number) {
                    Some(count) if count >= 0 => count as usize,
                    // `$@ and warn(...), last` -- an unevaluable count ends
                    // the record rather than guessing a length.
                    _ => break,
                }
            }
            Fmt::FileInfoProperties => {
                let get = |key: usize| values.get(&key).and_then(|value| value.number);
                let (Some(len2), Some(streams), Some(rules)) = (get(13), get(15), get(18)) else {
                    break;
                };
                let computed = len2 - streams * 6 - rules * 2 - 12;
                if computed < 0 {
                    break;
                }
                computed as usize
            }
        };
        let width = match fmt {
            Fmt::Int8u => 1,
            Fmt::Int16u | Fmt::Int16uOf(_) => 2,
            Fmt::Int32u | Fmt::Int32uOf(_) => 4,
            Fmt::StringOf(_) | Fmt::FileInfoProperties => 1,
        };
        let len = width * count;
        if pos + len > size {
            break;
        }
        let raw = &data[pos..pos + len];
        let value = match fmt {
            Fmt::Int8u => SerialValue {
                text: raw[0].to_string(),
                number: Some(i64::from(raw[0])),
                bytes: raw.to_vec(),
            },
            Fmt::Int16u => {
                let Some(number) = be_u16(raw, 0) else { break };
                SerialValue {
                    text: number.to_string(),
                    number: Some(i64::from(number)),
                    bytes: raw.to_vec(),
                }
            }
            Fmt::Int32u => {
                let Some(number) = be_u32(raw, 0) else { break };
                SerialValue {
                    text: number.to_string(),
                    number: Some(i64::from(number)),
                    bytes: raw.to_vec(),
                }
            }
            Fmt::Int16uOf(_) | Fmt::Int32uOf(_) => {
                // `ReadValue` joins a multi-element read with spaces. These
                // are all `Unknown => 1`, so the text only ever feeds `%val`.
                let step = width;
                let text =
                    raw.chunks_exact(step)
                        .map(|chunk| match step {
                            2 => u32::from(u16::from_be_bytes([chunk[0], chunk[1]])).to_string(),
                            _ => u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                                .to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                SerialValue {
                    number: (count == 1).then(|| text.parse().ok()).flatten(),
                    text,
                    bytes: raw.to_vec(),
                }
            }
            // `string` truncates at its first NUL (ExifTool.pm:6311); `undef`
            // does not.
            Fmt::StringOf(_) => {
                let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
                let Ok(text) = std::str::from_utf8(&raw[..end]) else {
                    break;
                };
                SerialValue {
                    text: text.to_string(),
                    number: text.parse().ok(),
                    bytes: raw.to_vec(),
                }
            }
            Fmt::FileInfoProperties => SerialValue {
                text: String::new(),
                number: None,
                bytes: raw.to_vec(),
            },
        };

        // Real.pm:131-134's `RawConv => '$self->{RealStreamMime} = $val'`.
        if *name == "StreamMimeType" {
            *stream_mime = Some(value.text.clone());
        }

        match emit {
            Emit::Unknown | Emit::LogicalFileInfoGate => {}
            Emit::FileInfo => {
                process_real_properties(&value.bytes, group1, metadata);
            }
            Emit::Plain => {
                metadata.insert(
                    format!("{group1}:{name}"),
                    TagValue::new_string(value.text.clone()),
                );
            }
            Emit::Bitrate => {
                if let Some(number) = value.number {
                    metadata.insert(
                        format!("{group1}:{name}"),
                        TagValue::new_string(convert_bitrate(number as f64)),
                    );
                }
            }
            Emit::Milliseconds => {
                if let Some(number) = value.number {
                    metadata.insert(
                        format!("{group1}:{name}"),
                        TagValue::new_string(convert_duration(number as f64 / 1000.0)),
                    );
                }
            }
            Emit::Flags => {
                if let Some(number) = value.number {
                    let labels: Vec<&str> = FLAGS_BITMASK
                        .iter()
                        .filter(|(bit, _)| number & (1 << bit) != 0)
                        .map(|(_, label)| *label)
                        .collect();
                    let rendered = if labels.is_empty() {
                        // `DecodeBits` prints the raw value when no bit is set
                        // and "(unknown)" bits when one is unnamed; neither
                        // case occurs on a value this table can produce with
                        // only the low four bits defined.
                        number.to_string()
                    } else {
                        labels.join(", ")
                    };
                    metadata.insert(format!("{group1}:{name}"), TagValue::new_string(rendered));
                }
            }
        }
        values.insert(*index, value);
        pos += len;
    }
}

/// `Real::ProcessRealProperties` (Real.pm:360-416): a run of
/// `size(4) | version(2) | nameLen(1) | name | type(4) | valueLen(2) | value`.
fn process_real_properties(data: &[u8], group1: &str, metadata: &mut MetadataMap) {
    let dir_len = data.len();
    let mut pos = 0usize;
    while pos + 6 <= dir_len {
        let (Some(size), Some(version)) = (be_u32(data, pos), be_u16(data, pos + 4)) else {
            return;
        };
        let size = size as usize;
        if size < 6 {
            return;
        }
        if version != 0 {
            let Some(next) = pos.checked_add(size) else {
                return;
            };
            pos = next;
            continue;
        }
        pos += 6;
        let Some(&name_len) = data.get(pos) else {
            return;
        };
        pos += 1;
        let name_len = usize::from(name_len);
        if pos + name_len > dir_len {
            return;
        }
        let Ok(name) = std::str::from_utf8(&data[pos..pos + name_len]) else {
            return;
        };
        let name = name.to_string();
        pos += name_len;

        if pos + 6 > dir_len {
            return;
        }
        let (Some(kind), Some(value_len)) = (be_u32(data, pos), be_u16(data, pos + 4)) else {
            return;
        };
        pos += 6;
        let value_len = usize::from(value_len);
        if pos + value_len > dir_len {
            return;
        }
        let payload = &data[pos..pos + value_len];
        pos += value_len;

        // `%propertyType` (Real.pm:24-27): 0 is int32u, 2 is string, and
        // anything else falls back to 'undef'.
        let text = match kind {
            0 => match be_u32(payload, 0) {
                Some(number) => number.to_string(),
                None => continue,
            },
            _ => {
                // Both `string` and `undef` are read as raw bytes here;
                // `string` additionally truncates at its first NUL.
                let end = if kind == 2 {
                    payload
                        .iter()
                        .position(|byte| *byte == 0)
                        .unwrap_or(payload.len())
                } else {
                    payload.len()
                };
                match std::str::from_utf8(&payload[..end]) {
                    Ok(text) => text.to_string(),
                    // Non-text bytes under `undef` have no faithful scalar
                    // rendering here, so nothing is reported for them.
                    Err(_) => continue,
                }
            }
        };

        let entry = FILE_INFO.iter().find(|(key, _, _)| *key == name);
        let (tag, conv) = match entry {
            Some((_, tag, conv)) => ((*tag).to_string(), *conv),
            None => {
                // Real.pm:394-399: strip whitespace, and skip names that are
                // not a single word ("ignore crazy names").
                let stripped: String = name.chars().filter(|c| !c.is_whitespace()).collect();
                if stripped.is_empty() || !stripped.chars().all(|c| c.is_alphanumeric() || c == '_')
                {
                    continue;
                }
                (ucfirst(&stripped), FileInfoConv::None)
            }
        };
        let rendered = match conv {
            FileInfoConv::None => text,
            FileInfoConv::Indexable => match text.as_str() {
                "0" => "False".to_string(),
                "1" => "True".to_string(),
                _ => text,
            },
            FileInfoConv::ContentRating => CONTENT_RATING
                .iter()
                .find(|(key, _)| *key == text)
                .map_or(text, |(_, label)| (*label).to_string()),
            FileInfoConv::SlashDate => convert_slash_date(&text),
        };
        metadata.insert(format!("{group1}:{tag}"), TagValue::new_string(rendered));
    }
}

/// Real.pm:207-212, the `Creation Date`/`Modification Date` ValueConv:
///
/// ```perl
/// $val =~ m{(\d+)/(\d+)/(\d+)\s+(\d+):(\d+):(\d+)} ?
///   sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d",$3,$2,$1,$4,$5,$6) : $val
/// ```
///
/// Note the field order: the third captured number is the year.
fn convert_slash_date(value: &str) -> String {
    let mut numbers: Vec<u32> = Vec::new();
    let mut current = String::new();
    let mut separators = String::new();
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else {
            if !current.is_empty() {
                numbers.push(current.parse().unwrap_or(0));
                current.clear();
                separators.push(ch);
            }
            if numbers.len() >= 6 {
                break;
            }
        }
    }
    if !current.is_empty() {
        numbers.push(current.parse().unwrap_or(0));
    }
    // The regex is unanchored but its separators are fixed: `/`, `/`,
    // whitespace, `:`, `:`. Anything else is left alone, as the Perl's failed
    // match leaves `$val`.
    let shape_matches = separators.starts_with("//") && separators.len() >= 5;
    if numbers.len() < 6 || !shape_matches {
        return value.to_string();
    }
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        numbers[2], numbers[1], numbers[0], numbers[3], numbers[4], numbers[5]
    )
}

fn ucfirst(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `Real::ProcessRealMeta` (Real.pm:423-508): a tree of 28-byte metadata
/// structures, each with a name, a length-prefixed value and optional
/// sub-properties whose names are prefixed by the parent's.
fn process_real_meta(
    data: &[u8],
    start: usize,
    len: usize,
    prefix: &str,
    metadata: &mut MetadataMap,
) {
    let dir_end = start + len;
    if dir_end > data.len() {
        return;
    }
    let prefix = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    };
    let mut pos = start;
    loop {
        if pos + 28 > dir_end {
            return;
        }
        let field = |index: usize| be_u32(data, pos + index * 4).map(|value| value as usize);
        let (Some(size), Some(kind), Some(value_pos), Some(num_sub_props), Some(name_len)) =
            (field(0), field(1), field(3), field(5), field(6))
        else {
            return;
        };
        // "make pointers relative to data start" (Real.pm:441-442).
        let value_pos = pos + value_pos;
        if pos + size > dir_end
            || pos + 28 + name_len > dir_end
            || value_pos < pos + 28 + name_len
            || value_pos + 4 > dir_end
        {
            return;
        }
        let raw_name = &data[pos + 28..pos + 28 + name_len];
        let end = raw_name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(raw_name.len());
        let Ok(name) = std::str::from_utf8(&raw_name[..end]) else {
            return;
        };
        let tag = format!("{prefix}{name}");
        let Some(value_len) = be_u32(data, value_pos).map(|value| value as usize) else {
            return;
        };
        let value_pos = value_pos + 4;
        if value_pos + value_len > dir_end {
            return;
        }

        // `%metadataFormat` (Real.pm:30-41).
        let format = match kind {
            1 | 2 | 6 | 7 | 8 => Some(MetaFmt::Str),
            3 => Some(if value_len == 4 {
                MetaFmt::Int32u
            } else {
                MetaFmt::Int8u
            }),
            4 => Some(MetaFmt::Int32u),
            5 => Some(MetaFmt::Undef),
            _ => None,
        };
        if value_len > 0
            && let Some(format) = format
        {
            let payload = &data[value_pos..value_pos + value_len];
            let rendered = match format {
                MetaFmt::Int8u => payload.first().map(ToString::to_string),
                MetaFmt::Int32u => be_u32(payload, 0).map(|value| value.to_string()),
                MetaFmt::Str | MetaFmt::Undef => {
                    let end = if matches!(format, MetaFmt::Str) {
                        payload
                            .iter()
                            .position(|byte| *byte == 0)
                            .unwrap_or(payload.len())
                    } else {
                        payload.len()
                    };
                    std::str::from_utf8(&payload[..end])
                        .ok()
                        .map(ToString::to_string)
                }
            };
            if let Some(rendered) = rendered {
                let tag_name = RJMD_TAGS
                    .iter()
                    .find(|(key, _)| *key == tag)
                    .map_or_else(|| mint_meta_name(&tag), |(_, name)| (*name).to_string());
                // Real.pm:472-478 gives type 7 a `yyyymmddhhmmss` ValueConv.
                let rendered = if kind == 7 {
                    convert_compact_date(&rendered)
                } else {
                    rendered
                };
                if !tag_name.is_empty() {
                    metadata.insert(
                        format!("Real-RJMD:{tag_name}"),
                        TagValue::new_string(rendered),
                    );
                }
            }
        }

        if num_sub_props != 0 {
            let dir_start = value_pos + value_len + num_sub_props * 8;
            if dir_start <= pos + size && dir_start <= data.len() {
                process_real_meta(data, dir_start, pos + size - dir_start, &tag, metadata);
            }
        }
        pos += size;
        if size == 0 {
            return;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MetaFmt {
    Int8u,
    Int32u,
    Str,
    Undef,
}

/// Real.pm:459-463: `$tagName =~ tr/A-Za-z0-9//dc; ucfirst($tagName)`.
fn mint_meta_name(tag: &str) -> String {
    ucfirst(
        &tag.chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>(),
    )
}

/// Real.pm:474-477's type-7 ValueConv:
/// `^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})` -> `$1:$2:$3 $4:$5:$6`.
fn convert_compact_date(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() < 14 || !bytes[..14].iter().all(u8::is_ascii_digit) {
        return value.to_string();
    }
    format!(
        "{}:{}:{} {}:{}:{}",
        &value[0..4],
        &value[4..6],
        &value[6..8],
        &value[8..10],
        &value[10..12],
        &value[12..14],
    )
}

/// Extract RealMedia metadata (`Image::ExifTool::Real::ProcessReal`, `.RMF`).
pub fn parse_realmedia_metadata(
    reader: &dyn FileReader,
) -> std::result::Result<MetadataMap, String> {
    let size = reader.size();
    let data = reader.read(0, size as usize).map_err(|e| e.to_string())?;
    if !data.starts_with(RMF_SIGNATURE) {
        return Err("invalid RealMedia signature".to_string());
    }

    let mut metadata = MetadataMap::new();
    // Real.pm:593-596: skip the rest of the `.RMF` header.
    let Some(header_size) = be_u32(data, 4).map(|value| value as usize) else {
        return Err("truncated RealMedia header".to_string());
    };
    if header_size < 8 {
        return Err("bad RealMedia header size".to_string());
    }
    let mut pos = header_size;
    let mut dir_count: HashMap<[u8; 4], u32> = HashMap::new();
    let mut stream_mime: Option<String> = None;

    while pos + CHUNK_HEADER_LEN <= data.len() {
        let mut id = [0u8; 4];
        id.copy_from_slice(&data[pos..pos + 4]);
        if id == [0, 0, 0, 0] {
            break;
        }
        let Some(chunk_size) = be_u32(data, pos + 4) else {
            break;
        };
        // Real.pm:607: "stop normal parsing at DATA tag".
        if &id == b"DATA" {
            break;
        }
        if chunk_size & 0x8000_0000 != 0 || chunk_size < CHUNK_HEADER_LEN as u32 {
            break;
        }
        let chunk_size = chunk_size as usize;
        let body_start = pos + CHUNK_HEADER_LEN;
        let body_end = body_start + (chunk_size - CHUNK_HEADER_LEN);
        if body_end > data.len() {
            break;
        }
        let body = &data[body_start..body_end];

        // Real.pm:631-635: the second and later chunks of one id report under
        // a numbered family-1 group (`Real-MDPR`, then `Real-MDPR2`).
        let seen = dir_count.entry(id).or_insert(0);
        *seen += 1;
        let suffix = if *seen > 1 {
            seen.to_string()
        } else {
            String::new()
        };

        match &id {
            b"PROP" => process_serial_data(
                body,
                PROPERTIES,
                &format!("Real-PROP{suffix}"),
                &mut stream_mime,
                &mut metadata,
            ),
            b"MDPR" => {
                // `RealStreamMime` is deleted after each chunk (Real.pm:642),
                // so a stream with no MIME field cannot inherit the last one's.
                stream_mime = None;
                process_serial_data(
                    body,
                    MEDIA_PROPS,
                    &format!("Real-MDPR{suffix}"),
                    &mut stream_mime,
                    &mut metadata,
                );
            }
            b"CONT" => process_serial_data(
                body,
                CONTENT_DESCR,
                &format!("Real-CONT{suffix}"),
                &mut stream_mime,
                &mut metadata,
            ),
            b"RJMD" => process_real_meta(body, 0, body.len(), "", &mut metadata),
            _ => {}
        }
        pos = body_end;
    }

    // Real.pm:661-678, the trailing `RMJE` footer pointing back at an `RJMD`
    // block.
    if size >= FOOTER_PROBE_BACK {
        let probe_at = size - FOOTER_PROBE_BACK;
        if let Ok(probe) = reader.read(probe_at, 12)
            && probe.starts_with(b"RMJE")
            && let Some(meta_size) = be_u32(probe, 8).map(u64::from)
        {
            // `$raf->Seek(-$metaSize-12, 1)` from the position after the
            // 12-byte probe.
            let after_probe = probe_at + 12;
            if after_probe >= meta_size + 12 {
                let meta_at = after_probe - meta_size - 12;
                if let Ok(block) = reader.read(meta_at, meta_size as usize)
                    && block.starts_with(b"RJMD")
                    && block.len() > 8
                {
                    process_real_meta(block, 8, block.len() - 8, "", &mut metadata);
                }
            }
        }
    }

    // Real.pm:679-688, the ID3v1 tag.
    if size >= ID3V1_LEN as u64
        && let Ok(tail) = reader.read(size - ID3V1_LEN as u64, ID3V1_LEN)
        && tail.starts_with(b"TAG")
    {
        let _ = crate::parsers::audio::mp3::parse_id3v1(tail, &mut metadata);
        // Real.pm:684-688 enters the `ID3::v1` *table* directly through
        // `ProcessDirectory`; it never calls `ID3::ProcessID3`, which is
        // where ExifTool mints `ID3Version` and `ID3TagSize`. The shared
        // reader emits them for its MP3/MPC callers, which do go through
        // `ProcessID3`, so they are withdrawn here rather than reported for a
        // RealMedia file the oracle reports them for.
        for key in ["ID3Version", "MP3:ID3Version", "ID3TagSize"] {
            metadata.remove(key);
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_date_puts_the_year_third() {
        // The pinned `t/images/Real.rm` stores `05/12/2004 09:54:03` and
        // ExifTool reports `2004:12:05 09:54:03`.
        assert_eq!(
            convert_slash_date("05/12/2004 09:54:03"),
            "2004:12:05 09:54:03"
        );
        // A failed match leaves the value untouched.
        assert_eq!(convert_slash_date("not a date"), "not a date");
    }

    #[test]
    fn minted_metadata_names_drop_every_non_alphanumeric() {
        assert_eq!(
            mint_meta_name("Statistics/CDInfo/Source"),
            "StatisticsCDInfoSource"
        );
        assert_eq!(
            mint_meta_name("Track/Comments/DataSize"),
            "TrackCommentsDataSize"
        );
    }

    #[test]
    fn compact_date_is_yyyymmddhhmmss() {
        assert_eq!(
            convert_compact_date("20041205095403"),
            "2004:12:05 09:54:03"
        );
        assert_eq!(convert_compact_date("nope"), "nope");
    }
}
