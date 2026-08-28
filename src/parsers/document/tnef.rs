//! Transport Neutral Encapsulation Format (TNEF / winmail.dat) reader.
//!
//! `TNEF::ProcessTNEF` (TNEF.pm:396-440) walks a flat run of attributes --
//! `level(1) | tag(4) | length(4) | payload | checksum(2)` -- against
//! `TNEF::Main` (TNEF.pm:60-127). Two of those attributes are `SubDirectory`
//! edges into `TNEF::MsgProps` (TNEF.pm:129-190) and `TNEF::AttachInfo`
//! (TNEF.pm:193-222), both processed by `ProcessProps` (TNEF.pm:277-395) over
//! self-describing MAPI property records.
//!
//! # Why there is no transcribed table to consult
//!
//! None of the three TNEF tables is a `ProcessBinaryData` layout: `Main` is
//! keyed by a 24-bit attribute id read from the stream, and the other two are
//! `PROCESS_PROC` tables keyed by MAPI property id. `src/exiftool_tables`
//! emits only `ProcessBinaryData` tables, so `find_table("TNEF", ...)` is
//! `None` by construction. Every entry below is transcribed by hand from the
//! pinned TNEF.pm with its line cited.
//!
//! # What is deliberately absent
//!
//! 1. **Non-ASCII `PT_STRING8` values under a code page this module does not
//!    model.** TNEF.pm:71-75's `CodePage` `RawConv` sets `$$self{Charset}` from
//!    `$charsetName{"cp$val"}`, and TNEF.pm:372-373 decodes every 8-bit string
//!    through it. Windows-1252 is implemented exactly (it is Latin-1 plus the
//!    32-codepoint 0x80-0x9F block); a string carrying a byte above 0x7f under
//!    any other code page is omitted rather than rendered through the wrong
//!    table, because a mis-decoded name under a real tag name is exactly the
//!    silent-wrong-value case AGENTS.md rules out. Pure-ASCII strings are
//!    unaffected: every supported charset agrees on 0x00-0x7f.
//! 2. **`PT_CLSID` (0x48) values.** `%propType` maps it to `GUID`
//!    (TNEF.pm:41), `%fmtSize` gives it 16 bytes (TNEF.pm:51), and the record
//!    is walked past correctly here -- but `ReadValue` has no `GUID` format,
//!    so ExifTool's own `$val = ReadValue(...)` returns undef and the
//!    `elsif ($fmt eq 'GUID')` branch at TNEF.pm:365-366 is unreachable. There
//!    is no value to match, so none is emitted.
//! 3. **`PT_ERROR` (0x0a), currency (0x06) and OLE-date (0x07) records** are
//!    decoded and converted per TNEF.pm:349-362, but no tag in the three
//!    tables is declared with those types on the pinned fixture, so the paths
//!    exist only to keep the walk in step.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/TNEF.pm`

use crate::core::file_metadata::{format_unix_time_local, round_to_second};
use crate::core::{FileReader, MetadataMap, TagValue};

/// TNEF.pm:403, `$buff =~ /^\x78\x9f\x3e\x22..\x01\x06\x90\x08\0/s`.
const TNEF_KEY: &[u8; 4] = b"\x78\x9f\x3e\x22";
/// TNEF.pm:114-117, `MessageProps` -- a `SubDirectory` into `TNEF::MsgProps`.
const MESSAGE_PROPS: u32 = 0x069003;
/// TNEF.pm:124-127, `AttachInfo` -- a `SubDirectory` into `TNEF::AttachInfo`.
const ATTACH_INFO: u32 = 0x069005;

/// The value shape a `TNEF::Main` attribute or MAPI property decodes to.
enum Value {
    Int(i64),
    Text(String),
    /// A scalar ref in ExifTool's terms: ordinary output prints a byte count.
    Binary(usize),
}

impl Value {
    fn render(&self) -> String {
        match self {
            Value::Int(value) => value.to_string(),
            Value::Text(text) => text.clone(),
            Value::Binary(len) => {
                format!("(Binary data {len} bytes, use -b option to extract)")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TNEF::Main (TNEF.pm:60-127)
// ---------------------------------------------------------------------------

/// How a `TNEF::Main` attribute's payload is read and rendered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MainConv {
    /// No `Format`: `$val = $buff`, the payload verbatim (TNEF.pm:426-428).
    /// ExifTool does not strip the trailing NUL these attributes carry, and
    /// `-b` confirms it: `MessageClass` comes back as 24 bytes for a 23-byte
    /// string.
    Raw,
    /// `%dateInfo`'s `Format => 'date'` (TNEF.pm:55-59), read by
    /// TNEF.pm:422-425 as `unpack('v6')` into `%.4d:%.2d:%.2d %.2d:%.2d:%.2d`.
    Date,
    /// `Format => 'int32u'` plus the `%Microsoft::codePage` PrintConv
    /// (TNEF.pm:70-76).
    CodePage,
    /// `Format => 'int8u'` with `ValueConv => 'my @a = reverse split " ", $val;
    /// "@a"'` and `PrintConv => '$val =~ tr/ /./'` (TNEF.pm:77-81).
    TnefVersion,
    /// `Format => 'int16u'` with the Low/Normal/High PrintConv (TNEF.pm:93-101).
    Priority,
    /// `Binary => 1` (TNEF.pm:118, :120).
    Binary,
}

/// `%Image::ExifTool::TNEF::Main` (TNEF.pm:60-127).
#[rustfmt::skip]
const MAIN_TAGS: &[(u32, &str, MainConv)] = &[
    (0x069007, "CodePage",                 MainConv::CodePage),    // TNEF.pm:70-76
    (0x089006, "TNEFVersion",              MainConv::TnefVersion), // TNEF.pm:77-81
    (0x078008, "MessageClass",             MainConv::Raw),         // TNEF.pm:82
    (0x008000, "From",                     MainConv::Raw),         // TNEF.pm:83
    (0x018004, "Subject",                  MainConv::Raw),         // TNEF.pm:84
    (0x038005, "SentDate",                 MainConv::Date),        // TNEF.pm:85
    (0x038006, "ReceivedDate",             MainConv::Date),        // TNEF.pm:86
    (0x068007, "MessageStatus",            MainConv::Raw),         // TNEF.pm:87
    (0x018009, "MessageID",                MainConv::Raw),         // TNEF.pm:88
    (0x02800C, "MessageBody",              MainConv::Raw),         // TNEF.pm:89
    (0x04800D, "Priority",                 MainConv::Priority),    // TNEF.pm:90-98
    (0x038020, "MessageModifyDate",        MainConv::Date),        // TNEF.pm:99
    (0x069004, "RecipientTable",           MainConv::Raw),         // TNEF.pm:118
    (0x070600, "OriginalMessageClass",     MainConv::Raw),         // TNEF.pm:119
    (0x060000, "Owner",                    MainConv::Raw),         // TNEF.pm:120
    (0x060001, "SentFor",                  MainConv::Raw),         // TNEF.pm:121
    (0x060002, "Delegate",                 MainConv::Raw),         // TNEF.pm:122
    (0x030006, "StartDate",                MainConv::Date),        // TNEF.pm:123
    (0x030007, "EndDate",                  MainConv::Date),        // TNEF.pm:124
    (0x050008, "OwnerAppointmentID",       MainConv::Raw),         // TNEF.pm:125
    (0x040009, "ResponseRequested",        MainConv::Raw),         // TNEF.pm:126
    (0x06800F, "AttachData",               MainConv::Binary),      // TNEF.pm:127
    (0x018010, "AttachTitle",              MainConv::Raw),         // TNEF.pm:128
    (0x068011, "AttachMetaFile",           MainConv::Binary),      // TNEF.pm:129
    (0x038012, "AttachCreateDate",         MainConv::Date),        // TNEF.pm:130
    (0x038013, "AttachModifyDate",         MainConv::Date),        // TNEF.pm:131
    (0x069001, "AttachTransportFilename",  MainConv::Raw),         // TNEF.pm:132
    (0x069002, "AttachRenderingData",      MainConv::Binary),      // TNEF.pm:133
];

/// TNEF.pm:93-98's `Priority` PrintConv.
const PRIORITY: &[(i64, &str)] = &[(0, "Low"), (1, "Normal"), (2, "High")];

// ---------------------------------------------------------------------------
// TNEF::MsgProps and TNEF::AttachInfo (TNEF.pm:129-222)
// ---------------------------------------------------------------------------

/// How a MAPI property in one of the two `ProcessProps` tables is rendered
/// once `ProcessProps` has decoded it by MAPI type.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PropConv {
    /// No conversion declared: the decoded value stands.
    None,
    /// `RawConv => '$$val'` / `'ref $val ? $$val : $val'` -- dereference a
    /// binary value into an ordinary scalar (TNEF.pm:145, :147).
    Deref,
    /// `Binary => 1` (TNEF.pm:159, :164).
    Binary,
    /// `RawConv => '$$val'` then
    /// `ValueConv => '...DecompressRTF($self,$val); \$dat'` (TNEF.pm:160-163):
    /// the compressed blob is decompressed and the *result* is binary again,
    /// so ordinary output reports the decompressed length.
    Rtf,
    /// `PrintConv` on `AttachMethod` (TNEF.pm:201-212).
    AttachMethod,
}

/// A MAPI property id, its tag name and its conversion.
type PropTag = (&'static str, &'static str, PropConv);

/// `%Image::ExifTool::TNEF::MsgProps` (TNEF.pm:129-190).
///
/// The id is a string because named properties are keyed by
/// `<guid>_<hex id>` or `<guid>_<name>` (TNEF.pm:294-322), not by a number.
#[rustfmt::skip]
const MSG_PROPS: &[PropTag] = &[
    ("0002", "AlternateRecipientAllowed",        PropConv::None),   // TNEF.pm:134
    ("0039", "ClientSubmitTime",                 PropConv::None),   // TNEF.pm:135
    ("0040", "ReceivedByName",                   PropConv::None),   // TNEF.pm:136
    ("0044", "ReceivedRepresentingName",         PropConv::None),   // TNEF.pm:137
    ("004d", "OriginalAuthorName",               PropConv::None),   // TNEF.pm:138
    ("0055", "OriginalDeliveryTime",             PropConv::None),   // TNEF.pm:139
    ("0070", "Subject",                          PropConv::None),   // TNEF.pm:140
    ("0075", "ReceivedByAddressType",            PropConv::None),   // TNEF.pm:141
    ("0076", "ReceivedByEmailAddress",           PropConv::None),   // TNEF.pm:142
    ("0077", "ReceivedRepresentingAddressType",  PropConv::None),   // TNEF.pm:143
    ("0078", "ReceivedRepresentingEmailAddress", PropConv::None),   // TNEF.pm:144
    ("007f", "CorrelationKey",                   PropConv::Deref),  // TNEF.pm:145
    ("0c1a", "SenderName",                       PropConv::None),   // TNEF.pm:146
    ("0c1d", "SenderSearchKey",                  PropConv::Deref),  // TNEF.pm:147
    ("0e06", "MessageDeliveryTime",              PropConv::None),   // TNEF.pm:148
    ("0e1d", "NormalizedSubject",                PropConv::None),   // TNEF.pm:149
    ("0e28", "PrimarySendAccount",               PropConv::None),   // TNEF.pm:150
    ("0e29", "NextSendAccount",                  PropConv::None),   // TNEF.pm:151
    ("0f02", "DeliveryOrRenewTime",              PropConv::None),   // TNEF.pm:152
    ("1000", "MessageBodyText",                  PropConv::Binary), // TNEF.pm:153
    ("1007", "SyncBodyCount",                    PropConv::None),   // TNEF.pm:154
    ("1008", "SyncBodyData",                     PropConv::None),   // TNEF.pm:155
    ("1009", "MessageBodyRTF",                   PropConv::Rtf),    // TNEF.pm:156-161
    ("1013", "MessageBodyHTML",                  PropConv::Binary), // TNEF.pm:162
    ("1035", "InternetMessageID",                PropConv::None),   // TNEF.pm:163
    ("10f4", "Hidden",                           PropConv::None),   // TNEF.pm:164
    ("10f6", "ReadOnly",                         PropConv::None),   // TNEF.pm:165
    ("3007", "CreateDate",                       PropConv::None),   // TNEF.pm:166
    ("3008", "ModifyDate",                       PropConv::None),   // TNEF.pm:167
    ("3fde", "InternetCodePage",                 PropConv::None),   // TNEF.pm:168
    ("3ff1", "LocalUserID",                      PropConv::None),   // TNEF.pm:169
    ("3ff8", "CreatorName",                      PropConv::None),   // TNEF.pm:170
    ("3ffa", "LastModifierName",                 PropConv::None),   // TNEF.pm:171
    ("3ffd", "MessageCodePage",                  PropConv::None),   // TNEF.pm:172
    ("4076", "SpamConfidenceLevel",              PropConv::None),   // TNEF.pm:173
    // Named properties (TNEF.pm:175-189). The id is the namespace GUID with
    // `-0000-0000-C000-000000000046` removed, an underscore, then the string
    // or 8-hex-digit numeric id.
    ("00020329_Author",     "Author",                 PropConv::None), // TNEF.pm:175-184
    ("00020329_LastAuthor", "LastAuthor",             PropConv::None), // TNEF.pm:185
    ("00062004_0000801A",   "HomeAddress",            PropConv::None), // TNEF.pm:186
    ("00062004_000080DA",   "HomeAddressCountryCode", PropConv::None), // TNEF.pm:187
    ("00062008_00008554",   "AppVersion",             PropConv::None), // TNEF.pm:188
];

/// `%Image::ExifTool::TNEF::AttachInfo` (TNEF.pm:193-222).
///
/// `MappingSignature` (TNEF.pm:196), `ExceptionStartTime` (TNEF.pm:216-220)
/// and `ExceptionEndTime` (TNEF.pm:221) carry `Unknown => 1`, so ordinary
/// output never shows them; they are absent here for the same reason.
#[rustfmt::skip]
const ATTACH_PROPS: &[PropTag] = &[
    ("0e20", "AttachSize",           PropConv::None),         // TNEF.pm:194
    ("0e21", "AttachNum",            PropConv::None),         // TNEF.pm:195
    ("3001", "AttachFileName",       PropConv::None),         // TNEF.pm:197
    ("3703", "AttachFileExtension",  PropConv::None),         // TNEF.pm:198
    ("3701", "AttachBinary",         PropConv::None),         // TNEF.pm:199
    ("3705", "AttachMethod",         PropConv::AttachMethod), // TNEF.pm:200-212
    ("3707", "AttachLongFileName",   PropConv::None),         // TNEF.pm:213
    ("3708", "AttachPathName",       PropConv::None),         // TNEF.pm:214
    ("370d", "AttachLongPathName",   PropConv::None),         // TNEF.pm:215
    ("370e", "AttachMIMEType",       PropConv::None),         // TNEF.pm:216
];

/// TNEF.pm:202-211's `AttachMethod` PrintConv.
#[rustfmt::skip]
const ATTACH_METHOD: &[(i64, &str)] = &[
    (0, "Attachment Created"),
    (1, "AttachData"),
    (2, "AttachLongPathName (recipients with access)"),
    (4, "AttachLongPathName"),
    (5, "Embedded Message"),
    (6, "AttachBinary (object)"),
    (7, "AttachLongPathName (using AttachmentProviderType)"),
];

// ---------------------------------------------------------------------------
// Byte helpers
// ---------------------------------------------------------------------------

fn le_u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn le_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn le_u64(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        data.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// TNEF.pm:389, `$pos += ($size + 3) & 0xfffffffc`.
fn padded(size: usize) -> Option<usize> {
    size.checked_add(3).map(|value| value & !3)
}

// ---------------------------------------------------------------------------
// Charset
// ---------------------------------------------------------------------------

/// Windows-1252's 0x80-0x9F block; every other byte is its own Unicode
/// codepoint (Latin-1). Five slots are unassigned in the standard and stay
/// undecodable here rather than being mapped to a lookalike.
#[rustfmt::skip]
const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021,
    0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017D, 0,
    0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
];

/// `$et->Decode($val, $$et{Charset})` for a `PT_STRING8` payload
/// (TNEF.pm:372-373).
///
/// `None` means "this module cannot decode these bytes under this code page";
/// see the module header's omission #1.
fn decode_8bit(bytes: &[u8], code_page: Option<u32>) -> Option<String> {
    if bytes.iter().all(|byte| byte.is_ascii()) {
        // Every code page ExifTool can name agrees on 0x00-0x7f, and with no
        // CodePage attribute `$$et{Charset}` is unset and TNEF.pm:373 leaves
        // the bytes alone -- both paths give the same answer here.
        return Some(bytes.iter().map(|&byte| byte as char).collect());
    }
    if code_page != Some(1252) {
        return None;
    }
    bytes
        .iter()
        .map(|&byte| {
            if byte < 0x80 {
                Some(byte as char)
            } else if byte < 0xa0 {
                char::from_u32(u32::from(CP1252_HIGH[(byte - 0x80) as usize]))
                    .filter(|_| CP1252_HIGH[(byte - 0x80) as usize] != 0)
            } else {
                char::from_u32(u32::from(byte))
            }
        })
        .collect()
}

/// `$et->Decode($val, 'UTF16')` (TNEF.pm:369-370). ExifTool's UTF-16 -> UTF-8
/// recomposition truncates at the first NUL (Charset.pm:326), which subsumes
/// the `s/\0+$//` that follows it.
fn decode_utf16le(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16(&units).ok()
}

// ---------------------------------------------------------------------------
// RTF decompression (TNEF.pm:226-275)
// ---------------------------------------------------------------------------

/// TNEF.pm:239-243, the LZFu seed dictionary.
const RTF_DICT: &[u8] = concat!(
    r"{\rtf1\ansi\mac\deff0\deftab720{\fonttbl;}",
    r"{\f0\fnil \froman \fswiss \fmodern ",
    r"\fscript \fdecor MS Sans SerifSymbolArialTimes",
    r" New RomanCourier{\colortbl\red0\green0\blue0",
    "\r\n",
    r"\par \pard\plain\f0\fs20\b\i\u\tab\tx",
)
.as_bytes();

/// `Image::ExifTool::TNEF::DecompressRTF` (TNEF.pm:230-275). Returns `None`
/// where the Perl returns `''` after a warning: too short, or an unrecognised
/// compression word.
fn decompress_rtf(compressed: &[u8]) -> Option<Vec<u8>> {
    if compressed.len() <= 16 {
        return None;
    }
    let comp = le_u32(compressed, 8)?;
    // TNEF.pm:235-236: `MELA` means the payload is stored uncompressed.
    if comp == 0x414c_454d {
        return Some(compressed[16..].to_vec());
    }
    if comp != 0x7546_5a4c {
        return None;
    }
    let mut dict = [0u8; 4096];
    dict[..RTF_DICT.len()].copy_from_slice(RTF_DICT);
    let mut cpos = 16usize;
    let clen = compressed.len();
    let mut dpos = RTF_DICT.len();
    let mut out = Vec::new();
    while cpos < clen {
        let control = compressed[cpos];
        cpos += 1;
        for bit in 0..8u32 {
            if cpos >= clen {
                break;
            }
            if control & (1 << bit) != 0 {
                if cpos + 2 > clen {
                    return Some(out);
                }
                let reference = u16::from_be_bytes([compressed[cpos], compressed[cpos + 1]]);
                cpos += 2;
                let mut off = usize::from(reference >> 4);
                let len = usize::from(reference & 0x0f) + 2;
                // TNEF.pm:258: `return $rtnVal if $off == $dpos % 4096 or
                // $off % 4096 >= length($dict)` -- the dictionary is a fixed
                // 4096-byte ring here, so the second test can never fire.
                if off == dpos % 4096 {
                    return Some(out);
                }
                for _ in 0..len {
                    let ch = dict[off % 4096];
                    off += 1;
                    dict[dpos % 4096] = ch;
                    dpos += 1;
                    out.push(ch);
                }
            } else {
                let ch = compressed[cpos];
                cpos += 1;
                dict[dpos % 4096] = ch;
                dpos += 1;
                out.push(ch);
            }
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// ProcessProps (TNEF.pm:277-395)
// ---------------------------------------------------------------------------

/// `Image::ExifTool::ASF::GetGUID` (ASF.pm:525-533).
fn get_guid(bytes: &[u8]) -> Option<String> {
    if bytes.len() != 16 {
        return None;
    }
    let d1 = le_u32(bytes, 0)?;
    let d2 = le_u16(bytes, 4)?;
    let d3 = le_u16(bytes, 6)?;
    let tail: String = bytes[8..16].iter().map(|b| format!("{b:02X}")).collect();
    Some(format!(
        "{d1:08X}-{d2:04X}-{d3:04X}-{}-{}",
        &tail[0..4],
        &tail[4..]
    ))
}

/// `%propType` (TNEF.pm:27-43) plus `%fmtSize` (TNEF.pm:46-52): the fixed
/// width of a single element, or `None` for the length-prefixed types.
fn fixed_width(kind: u16) -> Option<usize> {
    match kind {
        0x0001 => Some(0),                   // null
        0x0002 | 0x000b => Some(2),          // int16s, boolean
        0x0003 | 0x0004 | 0x000a => Some(4), // int32s, float, error
        0x0005 | 0x0006 | 0x0007 | 0x0014 | 0x0040 => Some(8),
        0x0048 => Some(16), // GUID
        _ => None,
    }
}

/// TNEF.pm:349-362's per-type conversions of a decoded fixed-width record.
fn convert_fixed(kind: u16, bytes: &[u8]) -> Option<Value> {
    match kind {
        // int16s
        0x0002 => Some(Value::Int(i64::from(i16::from_le_bytes(
            bytes.get(0..2)?.try_into().ok()?,
        )))),
        // boolean -> 'True' / 'False' (TNEF.pm:359-360)
        0x000b => {
            let raw = i16::from_le_bytes(bytes.get(0..2)?.try_into().ok()?);
            Some(Value::Text(
                if raw != 0 { "True" } else { "False" }.to_string(),
            ))
        }
        // int32s and PT_ERROR
        0x0003 | 0x000a => Some(Value::Int(i64::from(i32::from_le_bytes(
            bytes.get(0..4)?.try_into().ok()?,
        )))),
        // int64s
        0x0014 => Some(Value::Int(i64::from_le_bytes(
            bytes.get(0..8)?.try_into().ok()?,
        ))),
        // SYSTIME: 100-ns intervals since 1601, reported in LOCAL time --
        // `ConvertUnixTime($_/1e7-11644473600, 1)` (TNEF.pm:361-362).
        0x0040 => {
            let raw = le_u64(bytes, 0)?;
            let seconds = raw as f64 / 1e7 - 11_644_473_600.0;
            Some(Value::Text(format_unix_time_local(round_to_second(
                seconds,
            ))))
        }
        // Currency (TNEF.pm:352-353) divides an int64s by 10000 and lets
        // Perl stringify the result, whose exact rendering depends on Perl's
        // own float formatting. No tag in the three tables is declared with
        // this type, so it is walked past rather than approximated.
        // OLE date: days since Dec 30 1899, in UTC (TNEF.pm:354-358).
        0x0007 => {
            let days = f64::from_le_bytes(bytes.get(0..8)?.try_into().ok()?);
            if days == 0.0 {
                return Some(Value::Text("0000:00:00 00:00:00".to_string()));
            }
            let seconds = (days - 25569.0) * 24.0 * 3600.0;
            Some(Value::Text(
                chrono::DateTime::from_timestamp(round_to_second(seconds), 0).map_or_else(
                    || "0000:00:00 00:00:00".to_string(),
                    |dt| crate::core::value_formatter::format_exif_datetime(&dt),
                ),
            ))
        }
        // float / double: no tag in the three tables declares one, and
        // ExifTool would print Perl's own default stringification. Not
        // approximated; see the module header.
        _ => None,
    }
}

/// `ProcessProps` (TNEF.pm:277-395).
fn process_props(
    data: &[u8],
    table: &[PropTag],
    code_page: Option<u32>,
    metadata: &mut MetadataMap,
) {
    let Some(entries) = le_u32(data, 0) else {
        return;
    };
    let dir_len = data.len();
    let mut pos = 4usize;

    for _ in 0..entries {
        if pos + 4 > dir_len {
            return;
        }
        let (Some(raw_type), Some(numeric_tag)) = (le_u16(data, pos), le_u16(data, pos + 2)) else {
            return;
        };
        pos += 4;

        // TNEF.pm:293-322: a named property replaces the numeric id.
        let mut tag = format!("{numeric_tag:04x}");
        if numeric_tag & 0x8000 != 0 {
            if pos + 24 > dir_len {
                return;
            }
            let Some(uid) = get_guid(&data[pos..pos + 16]) else {
                return;
            };
            let uid = uid
                .strip_suffix("-0000-0000-C000-000000000046")
                .unwrap_or(&uid)
                .to_string();
            let (Some(id_kind), Some(num)) = (le_u32(data, pos + 16), le_u32(data, pos + 20))
            else {
                return;
            };
            pos += 24;
            match id_kind {
                0 => tag = format!("{uid}_{num:08x}"),
                1 => {
                    let num = num as usize;
                    if pos + num > dir_len || num < 2 {
                        return;
                    }
                    let Some(name) = decode_utf16le(&data[pos..pos + num - 2]) else {
                        return;
                    };
                    tag = format!("{uid}_{name}");
                    let Some(next) = padded(num).and_then(|skip| pos.checked_add(skip)) else {
                        return;
                    };
                    pos = next;
                }
                _ => return,
            }
        }

        let multi = raw_type & 0x1000 != 0;
        let kind = raw_type & 0x0fff;
        let mut count = if multi {
            let Some(count) = le_u32(data, pos) else {
                return;
            };
            pos += 4;
            count
        } else {
            1
        };
        // `$fmt = $propType{$type} or last` (TNEF.pm:327).
        if !matches!(
            kind,
            0x0001
                | 0x0002
                | 0x0003
                | 0x0004
                | 0x0005
                | 0x0006
                | 0x0007
                | 0x000a
                | 0x000b
                | 0x000d
                | 0x0014
                | 0x001e
                | 0x001f
                | 0x0040
                | 0x0048
                | 0x0102
        ) {
            return;
        }

        while count > 0 {
            if let Some(width) = fixed_width(kind) {
                // `unless ($size)` is false for float/double/GUID, and for the
                // integer formats the width comes from `$fmt =~ /(\d+)/`,
                // which multiplies by the element count.
                let total = match kind {
                    0x0001 => 0,
                    0x0004 | 0x0005 | 0x0048 => width,
                    _ => width * count as usize,
                };
                let Some(end) = pos.checked_add(total) else {
                    return;
                };
                if end > dir_len {
                    return;
                }
                if let Some(value) = convert_fixed(kind, &data[pos..end]) {
                    emit(table, &tag, value, metadata);
                } else if kind == 0x0001 {
                    emit(table, &tag, Value::Text(String::new()), metadata);
                }
                let Some(next) = padded(total).and_then(|skip| pos.checked_add(skip)) else {
                    return;
                };
                pos = next;
                // `$count = 1` after ReadValue: every element was read at once.
                break;
            }

            // The length-prefixed types: string, Unicode, object, blob.
            // TNEF.pm:333-337's "skip 1 count for special case stupidity".
            if !multi {
                let Some(next) = pos.checked_add(4) else {
                    return;
                };
                pos = next;
            }
            if pos + 4 > dir_len {
                return;
            }
            let Some(size) = le_u32(data, pos).map(|size| size as usize) else {
                return;
            };
            pos += 4;
            let Some(end) = pos.checked_add(size) else {
                return;
            };
            if end > dir_len {
                return;
            }
            let payload = &data[pos..end];
            let value = match kind {
                // `$val =~ s/\0+$//` then Decode through $$et{Charset}.
                0x001e => decode_8bit(payload, code_page)
                    .map(|text| Value::Text(text.trim_end_matches('\0').to_string())),
                0x001f => decode_utf16le(payload).map(Value::Text),
                // `$val = \$copy` -- a binary reference (TNEF.pm:376-378).
                // A zero-length blob keeps its empty-scalar form instead.
                _ if payload.is_empty() => Some(Value::Text(String::new())),
                _ => Some(Value::Binary(payload.len())),
            };
            if let Some(value) = value {
                emit_binary_aware(table, &tag, value, payload, metadata);
            }
            let Some(next) = padded(size).and_then(|skip| pos.checked_add(skip)) else {
                return;
            };
            pos = next;
            count -= 1;
        }
    }
}

/// `HandleTag` against one of the two `ProcessProps` tables: a property with
/// no entry there is not reported at all.
fn emit(table: &[PropTag], tag: &str, value: Value, metadata: &mut MetadataMap) {
    let Some((_, name, conv)) = table.iter().find(|(id, _, _)| id.eq_ignore_ascii_case(tag)) else {
        return;
    };
    let rendered = match conv {
        PropConv::AttachMethod => match &value {
            Value::Int(raw) => ATTACH_METHOD
                .iter()
                .find(|(key, _)| key == raw)
                .map_or_else(|| value.render(), |(_, label)| (*label).to_string()),
            _ => value.render(),
        },
        _ => value.render(),
    };
    metadata.insert(format!("File:{name}"), TagValue::new_string(rendered));
}

/// [`emit`] for the length-prefixed types, where the declared conversion also
/// decides whether the raw bytes are dereferenced or transformed.
fn emit_binary_aware(
    table: &[PropTag],
    tag: &str,
    value: Value,
    payload: &[u8],
    metadata: &mut MetadataMap,
) {
    let Some((_, name, conv)) = table.iter().find(|(id, _, _)| id.eq_ignore_ascii_case(tag)) else {
        return;
    };
    let rendered = match conv {
        // `RawConv => '$$val'`: the blob is dereferenced into a plain scalar,
        // which ordinary output prints as text.
        PropConv::Deref => match String::from_utf8(payload.to_vec()) {
            Ok(text) => text.trim_end_matches('\0').to_string(),
            // Not text, so there is no faithful scalar rendering to print.
            Err(_) => return,
        },
        PropConv::Rtf => match decompress_rtf(payload) {
            Some(decompressed) => Value::Binary(decompressed.len()).render(),
            // `DecompressRTF` returns '' after a warning; ExifTool then
            // reports an empty binary reference.
            None => Value::Binary(0).render(),
        },
        PropConv::Binary => Value::Binary(payload.len()).render(),
        PropConv::None | PropConv::AttachMethod => value.render(),
    };
    metadata.insert(format!("File:{name}"), TagValue::new_string(rendered));
}

// ---------------------------------------------------------------------------
// ProcessTNEF (TNEF.pm:396-440)
// ---------------------------------------------------------------------------

/// The `TNEF::Main` attribute payload, rendered per its declared `Format`.
fn main_value(conv: MainConv, payload: &[u8]) -> Option<Value> {
    match conv {
        // `$val = $buff` verbatim -- including any trailing NUL, which
        // `exiftool -b -MessageClass` confirms is kept.
        MainConv::Raw => String::from_utf8(payload.to_vec()).ok().map(Value::Text),
        MainConv::Binary => Some(Value::Binary(payload.len())),
        MainConv::Date => {
            if payload.len() < 12 {
                return None;
            }
            let field = |index: usize| le_u16(payload, index * 2).unwrap_or(0);
            Some(Value::Text(format!(
                "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
                field(0),
                field(1),
                field(2),
                field(3),
                field(4),
                field(5)
            )))
        }
        MainConv::CodePage => {
            let raw = le_u32(payload, 0)?;
            Some(Value::Text(
                crate::parsers::archive::ole_properties::code_page_name(i64::from(raw)),
            ))
        }
        // `int8u` over the whole payload, reversed, then `tr/ /./`.
        MainConv::TnefVersion => Some(Value::Text(
            payload
                .iter()
                .rev()
                .map(|byte| byte.to_string())
                .collect::<Vec<_>>()
                .join("."),
        )),
        MainConv::Priority => {
            let raw = i64::from(le_u16(payload, 0)?);
            Some(Value::Text(
                PRIORITY
                    .iter()
                    .find(|(key, _)| *key == raw)
                    .map_or_else(|| raw.to_string(), |(_, label)| (*label).to_string()),
            ))
        }
    }
}

pub fn parse_tnef_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let data = reader
        .read(0, reader.size() as usize)
        .map_err(|err| err.to_string())?;
    if data.len() < 0x15 || !data.starts_with(TNEF_KEY) {
        return Err("invalid TNEF header".to_owned());
    }

    let mut metadata = MetadataMap::new();
    // `$$et{Charset}`, set by `CodePage`'s RawConv as the attributes are
    // walked, so it only affects strings decoded after it.
    let mut code_page: Option<u32> = None;
    // TNEF.pm:401-402: the walk starts after the 4-byte key and the 2-byte
    // legacy key. Each attribute is level(1), tag(4), length(4), payload,
    // checksum(2).
    let mut pos = 6usize;
    while let (Some(tag), Some(size)) = (le_u32(data, pos + 1), le_u32(data, pos + 5)) {
        let payload_start = pos + 9;
        let Some(end) = payload_start.checked_add(size as usize) else {
            break;
        };
        let Some(payload) = data.get(payload_start..end) else {
            break;
        };

        match tag {
            MESSAGE_PROPS => process_props(payload, MSG_PROPS, code_page, &mut metadata),
            ATTACH_INFO => process_props(payload, ATTACH_PROPS, code_page, &mut metadata),
            _ => {}
        }
        if let Some((_, name, conv)) = MAIN_TAGS.iter().find(|(id, _, _)| *id == tag) {
            if *conv == MainConv::CodePage {
                code_page = le_u32(payload, 0);
            }
            if let Some(value) = main_value(*conv, payload) {
                metadata.insert(format!("File:{name}"), TagValue::new_string(value.render()));
            }
        }

        let Some(next) = end.checked_add(2) else {
            break;
        };
        pos = next;
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_matches_asf_getguid() {
        // The namespace GUID carried by the pinned `t/images/TNEF.tnef`'s
        // AppVersion property: `00062008-0000-0000-C000-000000000046`.
        let bytes = [
            0x08, 0x20, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x46,
        ];
        assert_eq!(
            get_guid(&bytes).as_deref(),
            Some("00062008-0000-0000-C000-000000000046")
        );
    }

    #[test]
    fn tnef_version_reverses_then_dots() {
        // `t/images/TNEF.tnef` stores 00 00 01 00 and ExifTool reports 0.1.0.0.
        let value =
            main_value(MainConv::TnefVersion, &[0x00, 0x00, 0x01, 0x00]).expect("version renders");
        assert_eq!(value.render(), "0.1.0.0");
    }

    #[test]
    fn main_date_is_six_little_endian_words() {
        // `SentDate` on the pinned fixture: 2004:02:17 11:25:35.
        let payload = [
            0xd4, 0x07, 0x02, 0x00, 0x11, 0x00, 0x0b, 0x00, 0x19, 0x00, 0x23, 0x00, 0x02, 0x00,
        ];
        let value = main_value(MainConv::Date, &payload).expect("date renders");
        assert_eq!(value.render(), "2004:02:17 11:25:35");
    }

    #[test]
    fn cp1252_decodes_its_own_high_block() {
        // 0x93/0x94 are the curly double quotes; 0x81 is unassigned and must
        // not be invented.
        assert_eq!(
            decode_8bit(&[0x93, 0x41, 0x94], Some(1252)).as_deref(),
            Some("\u{201C}A\u{201D}")
        );
        assert_eq!(decode_8bit(&[0x81], Some(1252)), None);
        // A different code page with a high byte is omitted, not guessed.
        assert_eq!(decode_8bit(&[0xe9], Some(932)), None);
        // ASCII is code-page independent.
        assert_eq!(
            decode_8bit(b"Test21uw2", None).as_deref(),
            Some("Test21uw2")
        );
    }
}
