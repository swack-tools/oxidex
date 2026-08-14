//! Apple binary property lists (`bplist00`), for MakerNote tags whose value is
//! a serialised plist rather than a number.
//!
//! Several Apple MakerNote tags do not hold a value at all: they hold a whole
//! `bplist00` blob that ExifTool unpacks. `%Apple::Main` reaches it two ways
//! (Apple.pm):
//!
//! * `0x0003 RunTime` is a `SubDirectory` over `%Apple::RunTime`, whose
//!   `PROCESS_PROC` is `Image::ExifTool::PLIST::ProcessBinaryPLIST`
//!   (Apple.pm:40-43, :324-325). The blob's top object is a dictionary and each
//!   of its keys selects a tag in that table -- `timescale` is `RunTimeScale`,
//!   `epoch` is `RunTimeEpoch`, `value` is `RunTimeValue` and `flags` is
//!   `RunTimeFlags` (Apple.pm:332-336). The pointer itself is never a value.
//! * `0x0040 SemanticStyle`, `0x0041 SemanticStyleRenderingVer` and
//!   `0x0042 SemanticStylePreset` carry `ValueConv => \&ConvertPLIST`
//!   (Apple.pm:276, :280, :284). `ConvertPLIST` (Apple.pm:367-380) decodes the
//!   blob and, when the result is a dictionary and the `Struct` option is off,
//!   flattens it with `XMP::SerializeStruct` -- which is why ExifTool prints
//!   `SemanticStyle` as `{_0=1,_1=0.5,_2=0,_3=2}`.
//!
//! This module is a transcription of the object grammar in
//! `Image::ExifTool::PLIST::ExtractObject` and the trailer parsing in
//! `ProcessBinaryPLIST` (PLIST.pm:260-395 and :398-450). The parts that matter
//! for matching ExifTool byte for byte:
//!
//! * `SetByteOrder('MM')` -- a binary plist is big-endian regardless of the
//!   byte order of the file that carries it (PLIST.pm:406).
//! * integers are read *unsigned*. `%readProc` maps every integer size to
//!   `Get8u`/`Get16u`/`Get24u`/`Get32u`/`Get64u` (PLIST.pm:30-38), so an
//!   8-byte integer with the high bit set prints as a large positive number,
//!   not a negative one.
//! * only the sizes in `%readProc` decode. An integer marker with a size nibble
//!   above 3 selects a 16-byte read that has no entry there, and `ExtractObject`
//!   returns undef rather than guessing (PLIST.pm:272-274). Reals are the same
//!   with the `+ 0x100` keys, so only 4- and 8-byte reals decode.
//! * `$topObj >= $numObj` is rejected outright (PLIST.pm:424), as is any
//!   object reference at or past the end of the offset table (PLIST.pm:320).

use std::collections::BTreeMap;

use crate::core::formatters::numeric_precision::perl_number;

/// A decoded plist object.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PlistValue {
    /// Marker `0x00`. ExifTool yields the literal string `<null>`.
    Null,
    /// Markers `0x08` / `0x09`, which ExifTool yields as `True` / `False`.
    Bool(bool),
    /// Marker `0x0f`. ExifTool yields the literal string `<fill>`.
    Fill,
    /// An integer, read unsigned as ExifTool's `%readProc` does.
    Int(u64),
    Real(f64),
    /// A date, carrying the raw seconds-since-2001 the plist stores.
    ///
    /// ExifTool converts it with `ConvertUnixTime($val + 11323 * 24 * 3600, 1)`
    /// (PLIST.pm:279), and that second argument is `$toLocal`, so the printed
    /// string carries the *local* time zone of the machine that ran the
    /// extraction. [`Self::scalar`] therefore still returns `None` here -- a
    /// caller that wants the date has to opt into that time-zone dependency
    /// explicitly, which `parsers::specialized::macos` does because
    /// MacOS.pm's `._` sidecar tags are exactly the case where ExifTool
    /// prints one.
    Date(f64),
    Data(Vec<u8>),
    Str(String),
    /// A UID, rendered as ExifTool's `%readProc` integer or, failing that, its
    /// hex (PLIST.pm:281-292).
    Uid(String),
    Array(Vec<PlistValue>),
    /// Key/value pairs in the order the dictionary stores them.
    Dict(Vec<(String, PlistValue)>),
}

impl PlistValue {
    /// The scalar rendering ExifTool gives an object when it becomes a tag
    /// value. `None` for the aggregate and date cases, which never reach a
    /// tag directly.
    pub(crate) fn scalar(&self) -> Option<String> {
        match self {
            PlistValue::Null => Some("<null>".to_string()),
            PlistValue::Bool(true) => Some("True".to_string()),
            PlistValue::Bool(false) => Some("False".to_string()),
            PlistValue::Fill => Some("<fill>".to_string()),
            PlistValue::Int(n) => Some(n.to_string()),
            PlistValue::Real(f) => Some(perl_number(*f)),
            PlistValue::Str(s) => Some(s.clone()),
            PlistValue::Uid(s) => Some(s.clone()),
            PlistValue::Data(_)
            | PlistValue::Date(_)
            | PlistValue::Array(_)
            | PlistValue::Dict(_) => None,
        }
    }
}

/// `%readProc` (PLIST.pm:30-38): the integer sizes that have a reader. Any
/// other size makes `ExtractObject` return undef.
const fn int_size_supported(size: usize) -> bool {
    matches!(size, 1 | 2 | 3 | 4 | 8)
}

/// A cursor over the blob, standing in for the `File::RandomAccess` handle
/// `ProcessBinaryPLIST` builds over the data (PLIST.pm:407-411).
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    fn seek(&mut self, pos: usize) -> Option<()> {
        // ExifTool's `$raf->Seek` past the end still succeeds; the following
        // read is what fails. Refusing here is equivalent and simpler.
        if pos > self.data.len() {
            return None;
        }
        self.pos = pos;
        Some(())
    }
}

/// Read a big-endian unsigned integer of `size` bytes (`%readProc`).
fn read_uint(bytes: &[u8]) -> u64 {
    let mut v: u64 = 0;
    for &b in bytes {
        v = (v << 8) | u64::from(b);
    }
    v
}

struct Ctx<'a> {
    reader: Reader<'a>,
    /// Offset of every object, indexed by object number.
    table: Vec<usize>,
    ref_size: usize,
}

/// Recursion guard. ExifTool bounds the dictionary depth through the length of
/// the accumulated tag path (`length $parent > 1000`, PLIST.pm:327-330); a
/// plain depth counter bounds the same recursion without carrying the path.
const MAX_DEPTH: usize = 64;

/// `ExtractObject` (PLIST.pm:260-390).
fn extract(ctx: &mut Ctx<'_>, depth: usize) -> Option<PlistValue> {
    if depth > MAX_DEPTH {
        return None;
    }
    let marker = *ctx.reader.take(1)?.first()?;
    let ty = marker >> 4;
    let mut size = usize::from(marker & 0x0f);

    match ty {
        // null / bool / fill (PLIST.pm:269-270)
        0 => match size {
            0x00 => Some(PlistValue::Null),
            0x08 => Some(PlistValue::Bool(true)),
            0x09 => Some(PlistValue::Bool(false)),
            0x0f => Some(PlistValue::Fill),
            _ => None,
        },
        // int (PLIST.pm:271-274)
        1 => {
            let n = 1usize << size;
            if !int_size_supported(n) {
                return None;
            }
            Some(PlistValue::Int(read_uint(ctx.reader.take(n)?)))
        }
        // real: only the `0x104`/`0x108` entries of %readProc exist
        2 => {
            let n = 1usize << size;
            let b = ctx.reader.take(n)?;
            match n {
                4 => Some(PlistValue::Real(f64::from(f32::from_be_bytes([
                    b[0], b[1], b[2], b[3],
                ])))),
                8 => Some(PlistValue::Real(f64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]))),
                _ => None,
            }
        }
        // date: same readers as a real, then ExifTool's local-time conversion
        3 => {
            let n = 1usize << size;
            let b = ctx.reader.take(n)?;
            match n {
                4 => Some(PlistValue::Date(f64::from(f32::from_be_bytes([
                    b[0], b[1], b[2], b[3],
                ])))),
                8 => Some(PlistValue::Date(f64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]))),
                _ => None,
            }
        }
        // UID (PLIST.pm:281-292)
        8 => {
            size += 1;
            let b = ctx.reader.take(size)?;
            if int_size_supported(size) {
                Some(PlistValue::Uid(read_uint(b).to_string()))
            } else {
                // ExifTool renders a 16-byte UID as an ASF GUID and anything
                // else as `"0x" . unpack 'H*'`. A GUID has a byte order of its
                // own that no Apple MakerNote in the corpus exercises, so only
                // the hex form is reproduced and the GUID case is declined.
                if size == 16 {
                    return None;
                }
                let mut s = String::with_capacity(2 + size * 2);
                s.push_str("0x");
                for byte in b {
                    s.push_str(&format!("{byte:02x}"));
                }
                Some(PlistValue::Uid(s))
            }
        }
        4 | 5 | 6 | 10 | 12 | 13 => {
            // `0x0f` means the count lives in a following integer object
            // (PLIST.pm:294-298).
            if size == 0x0f {
                let Some(PlistValue::Int(n)) = extract(ctx, depth + 1) else {
                    return None;
                };
                size = usize::try_from(n).ok()?;
            }
            match ty {
                // data
                4 => Some(PlistValue::Data(ctx.reader.take(size)?.to_vec())),
                // ASCII string
                5 => {
                    let b = ctx.reader.take(size)?;
                    Some(PlistValue::Str(
                        b.iter().map(|&c| char::from(c)).collect::<String>(),
                    ))
                }
                // UCS-2BE string
                6 => {
                    let b = ctx.reader.take(size.checked_mul(2)?)?;
                    let units: Vec<u16> = b
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    Some(PlistValue::Str(String::from_utf16_lossy(&units)))
                }
                // array (10), set (12) and dict (13) store a list of references
                _ => extract_collection(ctx, ty, size, depth),
            }
        }
        _ => None,
    }
}

/// The array/set/dict branch of `ExtractObject` (PLIST.pm:310-380).
fn extract_collection(ctx: &mut Ctx<'_>, ty: u8, size: usize, depth: usize) -> Option<PlistValue> {
    let num = if ty == 13 { size.checked_mul(2)? } else { size };
    let len = num.checked_mul(ctx.ref_size)?;
    let buf = ctx.reader.take(len)?.to_vec();
    let mut refs = Vec::with_capacity(num);
    for i in 0..num {
        let r = read_uint(&buf[i * ctx.ref_size..(i + 1) * ctx.ref_size]);
        let r = usize::try_from(r).ok()?;
        // `return 0 if $ref >= @$table` (PLIST.pm:320)
        if r >= ctx.table.len() {
            return None;
        }
        refs.push(r);
    }
    if ty == 13 {
        let mut entries = Vec::with_capacity(size);
        for i in 0..size {
            ctx.reader.seek(ctx.table[refs[i]])?;
            let key = extract(ctx, depth + 1);
            // "silently ignore bad dict entries" (PLIST.pm:337)
            let Some(key) = key.and_then(|k| k.scalar()).filter(|k| !k.is_empty()) else {
                continue;
            };
            ctx.reader.seek(ctx.table[refs[i + size]])?;
            let Some(obj) = extract(ctx, depth + 1) else {
                continue;
            };
            entries.push((key, obj));
        }
        Some(PlistValue::Dict(entries))
    } else {
        let mut items = Vec::with_capacity(refs.len());
        for r in refs {
            ctx.reader.seek(ctx.table[r])?;
            let Some(v) = extract(ctx, depth + 1) else {
                continue;
            };
            // "next unless defined $val and ref $val ne 'HASH'" (PLIST.pm:378)
            if matches!(v, PlistValue::Dict(_)) {
                continue;
            }
            items.push(v);
        }
        Some(PlistValue::Array(items))
    }
}

/// `ProcessBinaryPLIST` (PLIST.pm:398-450): parse the trailer, load the offset
/// table and extract the top object.
///
/// Returns `None` for anything ExifTool would have returned 0 for, so a blob we
/// cannot read produces no tag rather than a wrong one.
pub(crate) fn parse(data: &[u8]) -> Option<PlistValue> {
    // `$raf->Seek(-32,2) and $raf->Read($buff,32)==32 or return 0`
    if data.len() < 32 {
        return None;
    }
    let trailer = &data[data.len() - 32..];
    let int_size = usize::from(trailer[6]);
    let ref_size = usize::from(trailer[7]);
    let num_obj = read_uint(&trailer[8..16]);
    let top_obj = read_uint(&trailer[16..24]);
    let table_off = usize::try_from(read_uint(&trailer[24..32])).ok()?;

    // `return 0 if $topObj >= $numObj` (PLIST.pm:424)
    if top_obj >= num_obj {
        return None;
    }
    // `my $intProc = $readProc{$intSize} or return 0` (PLIST.pm:425-426)
    if !int_size_supported(int_size) || !int_size_supported(ref_size) {
        return None;
    }
    let num_obj = usize::try_from(num_obj).ok()?;
    let top_obj = usize::try_from(top_obj).ok()?;

    let table_size = num_obj.checked_mul(int_size)?;
    let table_end = table_off.checked_add(table_size)?;
    if table_end > data.len() {
        return None;
    }
    let mut table = Vec::with_capacity(num_obj);
    for i in 0..num_obj {
        let off = table_off + i * int_size;
        table.push(usize::try_from(read_uint(&data[off..off + int_size])).ok()?);
    }

    let start = *table.get(top_obj)?;
    let mut ctx = Ctx {
        reader: Reader { data, pos: 0 },
        table,
        ref_size,
    };
    ctx.reader.seek(start)?;
    extract(&mut ctx, 0)
}

/// The key rewriting `ExtractObject` applies when a dictionary is decoded
/// *without* a tag table -- the `ConvertPLIST` case (PLIST.pm:355-361).
///
/// A key that is not made purely of `[-_a-zA-Z0-9]` becomes `Tag<i>` for its
/// position in the dictionary, and one that does not start with a letter or an
/// underscore is prefixed with one. That second rule is why ExifTool prints
/// Apple's `SemanticStyle` keys `0`..`3` as `_0`..`_3`.
fn struct_field_name(key: &str, index: usize) -> String {
    let valid = !key.is_empty()
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
    if !valid {
        return format!("Tag{index}");
    }
    let first = key.as_bytes()[0];
    if first.is_ascii_alphabetic() || first == b'_' {
        key.to_string()
    } else {
        format!("_{key}")
    }
}

/// `XMP::SerializeStruct` (XMPStruct.pl:34-69) with the default
/// (non-JSON) `StructFormat`.
///
/// `ket` is the closing bracket of the enclosing container, which joins `,` and
/// `|` in the set of characters a scalar has to escape.
fn serialize_value(v: &PlistValue, ket: Option<char>) -> String {
    match v {
        PlistValue::Dict(entries) => {
            // `Image::ExifTool::OrderedKeys` returns `sort keys %$hash` unless
            // the hash carries an explicit ordering, and a hash built by
            // `ExtractObject` never does -- which is why ExifTool prints
            // Apple's `SemanticStyle` as `_0,_1,_2,_3` even though the
            // dictionary stores the keys in the order `3,1,2,0`.
            let mut map: BTreeMap<String, &PlistValue> = BTreeMap::new();
            for (i, (k, val)) in entries.iter().enumerate() {
                // "$$val{$key} = $obj if defined $obj" -- a later duplicate
                // wins, as it would in a Perl hash.
                map.insert(struct_field_name(k, i), val);
            }
            let body: Vec<String> = map
                .iter()
                .map(|(k, val)| format!("{k}={}", serialize_value(val, Some('}'))))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        PlistValue::Array(items) => {
            let body: Vec<String> = items
                .iter()
                .map(|item| serialize_value(item, Some(']')))
                .collect();
            format!("[{}]", body.join(","))
        }
        PlistValue::Date(_) | PlistValue::Data(_) => String::new(),
        other => match other.scalar() {
            Some(s) => escape_scalar(&s, ket),
            // `$rtnVal = ''` for an undefined item (XMPStruct.pl:66)
            None => String::new(),
        },
    }
}

/// The scalar escape of `SerializeStruct` (XMPStruct.pl:57-62): `,` and `|`
/// always, the enclosing closing bracket when there is one, and a leading
/// space, `[` or `{`.
fn escape_scalar(s: &str, ket: Option<char>) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == ',' || c == '|' || Some(c) == ket {
            out.push('|');
        }
        out.push(c);
    }
    let leading_escape = out
        .chars()
        .next()
        .is_some_and(|c| c.is_whitespace() || c == '[' || c == '{');
    if leading_escape {
        out.insert(0, '|');
    }
    out
}

/// `ConvertPLIST` (Apple.pm:367-380): decode the blob, and flatten a dictionary
/// result the way ExifTool does when the `Struct` option is off.
///
/// `None` means ExifTool would not have produced a printable value, so the tag
/// is omitted rather than given an approximation.
pub(crate) fn convert_plist(data: &[u8]) -> Option<String> {
    match parse(data)? {
        v @ (PlistValue::Dict(_) | PlistValue::Array(_)) => Some(serialize_value(&v, None)),
        other => other.scalar(),
    }
}

/// Decode a blob whose top object is a dictionary, for the `SubDirectory` case
/// where each key selects a tag in an ExifTool table.
///
/// Returns the dictionary entries in storage order. A blob whose top object is
/// not a dictionary yields nothing, matching `ExtractObject`, which only calls
/// `HandleTag` from its dictionary branch.
pub(crate) fn parse_dict(data: &[u8]) -> Vec<(String, PlistValue)> {
    match parse(data) {
        Some(PlistValue::Dict(entries)) => entries,
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `RunTime` blob of `Apple_iPhone13Pro.jpg`, tag 0x0003, exactly as
    /// `exiftool -v3` dumps it. ExifTool reports `RunTimeFlags = 1`,
    /// `RunTimeValue = 235706184764708`, `RunTimeScale = 1000000000` and
    /// `RunTimeEpoch = 0` from it.
    const RUNTIME_BLOB: &[u8] = &[
        0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0xd4, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
        0x07, 0x08, 0x55, 0x66, 0x6c, 0x61, 0x67, 0x73, 0x55, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x59,
        0x74, 0x69, 0x6d, 0x65, 0x73, 0x63, 0x61, 0x6c, 0x65, 0x55, 0x65, 0x70, 0x6f, 0x63, 0x68,
        0x10, 0x01, 0x13, 0x00, 0x00, 0xd6, 0x5f, 0x9f, 0x6a, 0x0d, 0x24, 0x12, 0x3b, 0x9a, 0xca,
        0x00, 0x10, 0x00, 0x08, 0x11, 0x17, 0x1d, 0x27, 0x2d, 0x2f, 0x38, 0x3d, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3f,
    ];

    /// The `SemanticStyle` blob of the same file, tag 0x0040. ExifTool prints
    /// `{_0=1,_1=0.5,_2=0,_3=2}`.
    const SEMANTIC_STYLE_BLOB: &[u8] = &[
        0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0xd4, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
        0x07, 0x08, 0x51, 0x33, 0x51, 0x31, 0x51, 0x32, 0x51, 0x30, 0x10, 0x02, 0x22, 0x3f, 0x00,
        0x00, 0x00, 0x22, 0x00, 0x00, 0x00, 0x00, 0x10, 0x01, 0x08, 0x11, 0x13, 0x15, 0x17, 0x19,
        0x1b, 0x20, 0x25, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x27,
    ];

    /// The `SemanticStyleRenderingVer` blob of the same file, tag 0x0041: a
    /// whole plist whose only object is the boolean ExifTool prints as `True`.
    const BOOL_BLOB: &[u8] = &[
        0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0x08, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09,
    ];

    #[test]
    fn runtime_dict_matches_exiftool() {
        let entries = parse_dict(RUNTIME_BLOB);
        let printed: Vec<(String, String)> = entries
            .iter()
            .map(|(k, v)| (k.clone(), v.scalar().unwrap()))
            .collect();
        assert_eq!(
            printed,
            vec![
                ("flags".to_string(), "1".to_string()),
                ("value".to_string(), "235706184764708".to_string()),
                ("timescale".to_string(), "1000000000".to_string()),
                ("epoch".to_string(), "0".to_string()),
            ]
        );
    }

    #[test]
    fn semantic_style_serializes_in_sorted_key_order() {
        // The dictionary stores the keys as 3, 1, 2, 0; ExifTool's OrderedKeys
        // falls back to a plain sort, so the printed order is _0.._3.
        assert_eq!(
            convert_plist(SEMANTIC_STYLE_BLOB).as_deref(),
            Some("{_0=1,_1=0.5,_2=0,_3=2}")
        );
    }

    #[test]
    fn lone_boolean_prints_as_true() {
        assert_eq!(convert_plist(BOOL_BLOB).as_deref(), Some("True"));
    }

    #[test]
    fn rejects_a_truncated_or_bogus_trailer() {
        assert!(parse(b"bplist00").is_none());
        // topObj >= numObj
        let mut bad = RUNTIME_BLOB.to_vec();
        let n = bad.len();
        bad[n - 16..n - 8].copy_from_slice(&9u64.to_be_bytes());
        assert!(parse(&bad).is_none());
        // an unreadable integer size
        let mut bad = RUNTIME_BLOB.to_vec();
        let n = bad.len();
        bad[n - 32 + 6] = 5;
        assert!(parse(&bad).is_none());
    }

    #[test]
    fn escapes_the_characters_serializestruct_does() {
        assert_eq!(escape_scalar("a,b", Some('}')), "a|,b");
        assert_eq!(escape_scalar("a|b", Some('}')), "a||b");
        assert_eq!(escape_scalar("a}b", Some('}')), "a|}b");
        assert_eq!(escape_scalar("a}b", None), "a}b");
        // The leading-bracket rule adds exactly one `|`; `{` is not in the
        // character class the first substitution escapes.
        assert_eq!(escape_scalar("{x", Some('}')), "|{x");
        assert_eq!(escape_scalar(" x", Some(']')), "| x");
    }

    #[test]
    fn generates_struct_field_names_the_way_exiftool_does() {
        assert_eq!(struct_field_name("0", 3), "_0");
        assert_eq!(struct_field_name("flags", 0), "flags");
        assert_eq!(struct_field_name("_x", 0), "_x");
        assert_eq!(struct_field_name("a b", 2), "Tag2");
        assert_eq!(struct_field_name("", 1), "Tag1");
    }
}
