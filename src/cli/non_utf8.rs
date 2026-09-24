//! Command-line arguments that are not valid UTF-8.
//!
//! An argument is bytes (Unix `argv`), and ExifTool treats it as bytes: a
//! path is opened and printed as the bytes it was given, and a tag value
//! reaches the tag's own inverse conversion unchanged. The CLI used to
//! collect its arguments with `std::env::args()`, which panics on the first
//! one that is not UTF-8. This module holds the three things the CLI now
//! does with such an argument instead:
//!
//! * a **path** stays an `OsStr` end to end and is printed as its own bytes
//!   ([`PathLine`]), or with each malformed byte replaced by `?` inside JSON
//!   ([`json_path`]) -- both what pinned ExifTool 13.59 prints;
//! * an **XP\* value** is encoded exactly as ExifTool encodes a typed value
//!   ([`xp_value`]);
//! * every **other value** is refused with a clear error
//!   ([`refuse_value`]): ExifTool's handling of those is format-specific
//!   (EXIF ASCII keeps the raw bytes, XMP substitutes `?`, IPTC truncates,
//!   numeric tags refuse), and oxidex's writers take UTF-8 text, so there is
//!   no value this CLI could pass them that would reproduce those bytes.
//!
//! Tag and option *names* that are not UTF-8 are refused by the argument
//! parser (`cli::args`); ExifTool refuses them too (`Invalid tag name`,
//! `Unknown option`).

use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;

/// The bytes of an argument or path exactly as the OS delivered them. On
/// Unix these are the `argv`/`readdir` bytes themselves, which is what
/// ExifTool prints and opens.
pub fn os_bytes(value: &OsStr) -> &[u8] {
    value.as_encoded_bytes()
}

/// The five Windows XP strings, 0x9c9b-0x9c9f, by the names the writer
/// routes to their UCS-2 serializer (`writers::xp_strings::is_xp_tag_key`).
const XP_TAG_NAMES: [&str; 5] = [
    "XPTitle",
    "XPComment",
    "XPAuthor",
    "XPKeywords",
    "XPSubject",
];

/// How `tag_name` (as typed after `-`, before `=`) relates to the XP strings.
#[derive(Debug, PartialEq, Eq)]
enum XpDestination {
    /// `IFD0:XPTitle` or `EXIF:XPTitle`: the spellings this module encodes,
    /// both of which reach the IFD0 entry (the tags' `WriteGroup`).
    Grouped,
    /// A bare `XPTitle`. ExifTool writes it to IFD0, but oxidex's writer
    /// does not reach the file for an ungrouped XP name -- `-XPTitle=Hi`
    /// reports `1 image files updated` and changes nothing -- so accepting
    /// bytes for it would report a write that never happens.
    Bare,
    /// Anything else, including an XP name under another group.
    Other,
}

fn xp_destination(tag_name: &str) -> XpDestination {
    match tag_name.rsplit_once(':') {
        Some(("IFD0" | "EXIF", name)) if XP_TAG_NAMES.contains(&name) => XpDestination::Grouped,
        None if XP_TAG_NAMES.contains(&tag_name) => XpDestination::Bare,
        _ => XpDestination::Other,
    }
}

/// Why a byte string could not be decoded as ExifTool decodes a typed value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DecodeError {
    /// Bytes Perl's own decoder also rejects (`unpack('C0U*')` warns
    /// `Malformed UTF-8 character` and ExifTool reports `Nothing to do.`):
    /// a stray continuation byte, an overlong form, a truncated sequence.
    Malformed { offset: usize },
    /// A sequence Perl's lax decoder accepts but this one does not: a code
    /// point above U+10FFFF, including Perl's own extended forms (lead bytes
    /// `f5`-`ff` with all their continuation bytes). ExifTool packs such a
    /// code point's low 16 bits; oxidex refuses instead of reproducing a
    /// value no Unicode text contains.
    AboveUnicode { offset: usize },
}

/// Decodes `bytes` as Perl's `unpack('C0U*', $val)` does for the code points
/// U+0000..=U+10FFFF: standard UTF-8, except that a surrogate code point
/// (`ed a0 80`..`ed bf bf`, the CESU-8/WTF-8 form of U+D800..U+DFFF) is
/// accepted as itself, as Perl accepts it. Everything Perl calls malformed
/// is [`DecodeError::Malformed`]; every form Perl accepts above U+10FFFF is
/// [`DecodeError::AboveUnicode`].
///
/// Pinned 13.59 oracle, `-IFD0:XPTitle=<bytes>`: `41 ed a0 80 42` is
/// accepted; `41 ff 42`, `41 80 42`, `41 c0 80 42`, `41 e0 80 80 42`,
/// `41 f0 80 80 80 42`, `41 e2 82` and a lone `e9` are all `Malformed UTF-8
/// character`; `41 f4 90 80 80 42` and `41 f8 88 80 80 80 42` are accepted.
pub(crate) fn decode_perl_code_points(bytes: &[u8]) -> std::result::Result<Vec<u32>, DecodeError> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let lead = bytes[i];
        let malformed = DecodeError::Malformed { offset: i };
        // (continuation bytes, second byte's range, the lead's payload bits)
        let (extra, second_min, second_max, init) = match lead {
            0x00..=0x7F => {
                out.push(u32::from(lead));
                i += 1;
                continue;
            }
            // A continuation byte with no lead, or an overlong 2-byte form.
            0x80..=0xC1 => return Err(malformed),
            0xC2..=0xDF => (1, 0x80, 0xBF, u32::from(lead & 0x1F)),
            // `e0 80..9f` is overlong; `ed a0..bf` is a surrogate, accepted.
            0xE0 => (2, 0xA0, 0xBF, 0),
            0xE1..=0xEF => (2, 0x80, 0xBF, u32::from(lead & 0x0F)),
            // `f0 80..8f` is overlong.
            0xF0 => (3, 0x90, 0xBF, 0),
            0xF1..=0xF3 => (3, 0x80, 0xBF, u32::from(lead & 0x07)),
            0xF4 => (3, 0x80, 0xBF, 4),
            // Perl's extended forms: 4 bytes up to 0x1FFFFF (`f5`-`f7`), then
            // 5, 6, 7 and 13 bytes. Perl accepts one whose continuation bytes
            // are all present, and calls it malformed otherwise.
            0xF5..=0xFF => {
                let extra = match lead {
                    0xF5..=0xF7 => 3,
                    0xF8..=0xFB => 4,
                    0xFC..=0xFD => 5,
                    0xFE => 6,
                    _ => 12,
                };
                return match bytes.get(i + 1..i + 1 + extra) {
                    Some(tail) if tail.iter().all(|b| (0x80..=0xBF).contains(b)) => {
                        Err(DecodeError::AboveUnicode { offset: i })
                    }
                    _ => Err(malformed),
                };
            }
        };
        let Some(tail) = bytes.get(i + 1..i + 1 + extra) else {
            return Err(malformed);
        };
        if !(second_min..=second_max).contains(&tail[0])
            || !tail.iter().all(|b| (0x80..=0xBF).contains(b))
        {
            return Err(malformed);
        }
        let code_point = tail
            .iter()
            .fold(init, |acc, b| (acc << 6) | u32::from(b & 0x3F));
        if code_point > 0x10FFFF {
            return Err(DecodeError::AboveUnicode { offset: i });
        }
        out.push(code_point);
        i += 1 + extra;
    }
    Ok(out)
}

/// The value ExifTool stores for a typed XP string given as `bytes`:
/// `ValueConvInv => Encode($val,"UCS2","II") . "\0\0"` (Exif.pm 13.59
/// 0x9c9b-0x9c9f). `Encode` decodes the typed bytes with Perl's lax UTF-8
/// decoder ([`decode_perl_code_points`]) and packs every code point with
/// `pack('v*')` (Charset.pm:387-390), keeping its low 16 bits -- so a lone
/// surrogate `ed a0 80` is the unit `D800`, and a code point above U+FFFF is
/// truncated, not split into a pair.
///
/// The bytes travel to the writer as a [`TagValue::Binary`], which
/// `writers::xp_strings::encode_xp_value` treats as stored UCS-2 and puts
/// through ExifTool's `ValueConvInv(ValueConv(..))` round trip. That round
/// trip is the identity on these units except in two cases, which are
/// refused here rather than written differently from ExifTool: a zero unit
/// inside the value (only a code point above U+FFFF can produce one; the
/// round trip ends the value there) and a first unit `FEFF`/`FFFE` (the
/// round trip consumes it as a byte-order mark).
pub(crate) fn xp_value(tag_name: &str, bytes: &[u8]) -> Result<TagValue> {
    let code_points = decode_perl_code_points(bytes).map_err(|error| match error {
        DecodeError::Malformed { offset } => invalid(
            tag_name,
            &format!(
                "Malformed UTF-8 character at byte {offset} ({})",
                hex_bytes(&bytes[offset..bytes.len().min(offset + 4)])
            ),
        ),
        DecodeError::AboveUnicode { offset } => invalid(
            tag_name,
            &format!(
                "code point above U+10FFFF at byte {offset} ({}); ExifTool writes its \
                 low 16 bits, which oxidex does not reproduce",
                hex_bytes(&bytes[offset..bytes.len().min(offset + 6)])
            ),
        ),
    })?;
    let units: Vec<u16> = code_points.iter().map(|&cp| cp as u16).collect();
    if units.contains(&0) {
        return Err(invalid(
            tag_name,
            "a code point above U+FFFF packs to a zero UCS-2 unit, which oxidex cannot \
             write inside the value",
        ));
    }
    if matches!(units.first(), Some(0xFEFF | 0xFFFE)) {
        return Err(invalid(
            tag_name,
            "a leading U+FEFF/U+FFFE would be stored as a byte-order mark, which \
             oxidex cannot write as ExifTool does",
        ));
    }
    let mut out: Vec<u8> = units.into_iter().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    Ok(TagValue::Binary(out))
}

/// The typed value for a non-UTF-8 `-TAG=VALUE`: ExifTool's bytes for an XP
/// string ([`xp_value`]), and a clear refusal for every other tag
/// ([`refuse_value`]).
pub(crate) fn tag_value(tag_name: &str, bytes: &[u8]) -> Result<TagValue> {
    match xp_destination(tag_name) {
        XpDestination::Grouped => xp_value(tag_name, bytes),
        XpDestination::Bare => Err(invalid(
            tag_name,
            &format!(
                "value is not valid UTF-8 ({}); oxidex writes non-UTF-8 bytes to an XP \
                 tag only when its group is given: -IFD0:{tag_name}=",
                hex_bytes(bytes)
            ),
        )),
        XpDestination::Other => Err(refuse_value(tag_name, bytes)),
    }
}

/// The error for a non-UTF-8 value oxidex cannot write as ExifTool would.
pub(crate) fn refuse_value(tag_name: &str, bytes: &[u8]) -> ExifToolError {
    invalid(
        tag_name,
        &format!(
            "value is not valid UTF-8 ({}); oxidex writes non-UTF-8 bytes only to the \
             XP* tags (IFD0:XPTitle, IFD0:XPComment, IFD0:XPAuthor, IFD0:XPKeywords, \
             IFD0:XPSubject)",
            hex_bytes(bytes)
        ),
    )
}

fn invalid(tag_name: &str, reason: &str) -> ExifToolError {
    ExifToolError::invalid_tag_value(tag_name, reason)
}

/// `value[range]` as an `OsStr`.
///
/// Every caller splits only immediately before or after an ASCII byte (`=`,
/// a quote) or a whitespace character decoded from a valid UTF-8 run.
fn os_sub(value: &OsStr, range: std::ops::Range<usize>) -> &OsStr {
    // SAFETY: `OsStr::from_encoded_bytes_unchecked` accepts encoded bytes
    // split "immediately before or immediately after any valid non-empty
    // UTF-8 substring", and each caller's bounds (see above) sit next to one.
    unsafe { OsStr::from_encoded_bytes_unchecked(&value.as_encoded_bytes()[range]) }
}

/// Splits a `-TAG=VALUE` argument at its first `=`: `(-TAG, VALUE)`.
pub fn split_at_equals(arg: &OsStr) -> Option<(&OsStr, &OsStr)> {
    let bytes = arg.as_encoded_bytes();
    let eq = bytes.iter().position(|&b| b == b'=')?;
    Some((os_sub(arg, 0..eq), os_sub(arg, eq + 1..bytes.len())))
}

/// `CliArgs::unquote` for a value that is not valid UTF-8: when the value,
/// trimmed of the whitespace `str::trim` would remove, is wrapped in a pair
/// of `"` or `'`, the text between them; otherwise the value unchanged. The
/// malformed bytes are never whitespace, so only the valid UTF-8 run at each
/// end can be trimmed.
pub fn unquote(value: &OsStr) -> &OsStr {
    let bytes = value.as_encoded_bytes();
    let lead = bytes.utf8_chunks().next().map_or(0, |chunk| {
        chunk.valid().len() - chunk.valid().trim_start().len()
    });
    let trail = match bytes.utf8_chunks().last() {
        Some(chunk) if chunk.invalid().is_empty() => {
            chunk.valid().len() - chunk.valid().trim_end().len()
        }
        _ => 0,
    };
    // A non-UTF-8 value has a malformed byte between the two trimmed runs,
    // so they cannot overlap.
    let end = bytes.len().saturating_sub(trail).max(lead);
    let trimmed = &bytes[lead..end];
    let quoted = trimmed.len() >= 2
        && matches!(
            (trimmed[0], trimmed[trimmed.len() - 1]),
            (b'"', b'"') | (b'\'', b'\'')
        );
    if quoted {
        os_sub(value, lead + 1..end - 1)
    } else {
        value
    }
}

/// `bytes` as space-separated lowercase hex, capped so a long value cannot
/// flood the message.
pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    const MAX: usize = 32;
    let mut out = bytes
        .iter()
        .take(MAX)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    if bytes.len() > MAX {
        out.push_str(" ...");
    }
    out
}

/// ExifTool's `FixUTF8` (XMP.pm 13.59:2943-2975) with its default `?`:
/// each byte that does not belong to a well-formed UTF-8 character is
/// replaced by `?`, one `?` per byte. Well-formed here is `IsUTF8`'s
/// definition, which is stricter than Rust's in one place: the
/// noncharacters U+FFFE and U+FFFF (`ef bf be`, `ef bf bf`) are replaced
/// too. The `exiftool` script applies it to every JSON string it prints, so
/// a file named `n\xff.jpg` is `"SourceFile": "n?.jpg"` under `-j`.
pub fn fix_utf8(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let lead = bytes[i];
        if lead < 0x80 {
            out.push(char::from(lead));
            i += 1;
            continue;
        }
        let n = match lead {
            0xC2..=0xDF => 1,
            0xE0..=0xEF => 2,
            0xF0..=0xF7 => 3,
            _ => 0,
        };
        let tail = bytes.get(i + 1..i + 1 + n);
        let well_formed = n > 0
            && tail.is_some_and(|tail| {
                tail.iter().all(|b| (0x80..=0xBF).contains(b))
                    && match n {
                        1 => true,
                        2 => {
                            !((lead == 0xE0 && tail[0] & 0xE0 == 0x80)
                                || (lead == 0xED && tail[0] & 0xE0 == 0xA0)
                                || (lead == 0xEF && tail[0] == 0xBF && tail[1] & 0xFE == 0xBE))
                        }
                        _ => {
                            !((lead == 0xF0 && tail[0] & 0xF0 == 0x80)
                                || (lead == 0xF4 && tail[0] > 0x8F)
                                || lead > 0xF4)
                        }
                    }
            });
        if well_formed {
            let char_bytes = &bytes[i..i + 1 + n];
            out.push_str(std::str::from_utf8(char_bytes).expect("checked well-formed above"));
            i += 1 + n;
        } else {
            // FixUTF8 replaces only the offending byte and resumes after it,
            // so the continuation bytes of a rejected sequence are replaced
            // one by one as they come up.
            out.push('?');
            i += 1;
        }
    }
    out
}

/// ExifTool's JSON rendering of a path: [`fix_utf8`] of its bytes.
pub fn json_path(path: &Path) -> String {
    fix_utf8(os_bytes(path.as_os_str()))
}

/// A line of output whose paths are written as their own bytes, the way
/// ExifTool prints them (`======== n\xff.jpg`, `Error: File not found -
/// n\xff.jpg`), rather than through `Path::display`, which substitutes
/// U+FFFD for every malformed byte and so names a file that does not exist.
#[derive(Debug, Default)]
pub struct PathLine(Vec<u8>);

impl PathLine {
    /// Starts a line with `text`.
    pub fn new(text: &str) -> Self {
        PathLine(text.as_bytes().to_vec())
    }

    /// Appends `text`.
    pub fn text(mut self, text: &str) -> Self {
        self.0.extend_from_slice(text.as_bytes());
        self
    }

    /// Appends `path` as its own bytes.
    pub fn path(mut self, path: &Path) -> Self {
        self.0.extend_from_slice(os_bytes(path.as_os_str()));
        self
    }

    /// The line's bytes, without a newline.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Writes the line and a newline to stdout.
    pub fn print(mut self) {
        self.0.push(b'\n');
        // As `println!` would, minus its panic on a closed pipe.
        let _ = std::io::stdout().lock().write_all(&self.0);
    }

    /// Writes the line and a newline to stderr.
    pub fn eprint(mut self) {
        self.0.push(b'\n');
        let _ = std::io::stderr().lock().write_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Oracle bytes for `-IFD0:XPTitle=<value>` (pinned 13.59).
    #[test]
    fn xp_values_match_the_oracle() {
        for (value, oracle) in [
            ("41eda08042", "410000d842000000"),
            ("41edb08042", "410000dc42000000"),
            ("41eda0bdedb88042", "41003dd800de42000000"),
            ("c3a9eda080e4b8ad", "e90000d82d4e0000"),
            ("f09f8e8ceda080", "8cf300d80000"),
            ("edbfbf5a", "ffdf5a000000"),
            ("41efbfbe42", "4100feff42000000"),
        ] {
            assert_eq!(
                xp_value("IFD0:XPTitle", &hex(value)).unwrap(),
                TagValue::Binary(hex(oracle)),
                "{value}"
            );
        }
    }

    #[test]
    fn malformed_bytes_are_refused_as_perl_refuses_them() {
        for value in [
            "41ff42",
            "41fe42",
            "636166e9",
            "636166e973",
            "418042",
            "41c08042",
            "41c1bf42",
            "41e0808042",
            "41f080808042",
            "41e282",
            "41e28242",
        ] {
            assert!(
                matches!(
                    decode_perl_code_points(&hex(value)),
                    Err(DecodeError::Malformed { .. })
                ),
                "{value}"
            );
            let error = xp_value("XPTitle", &hex(value)).unwrap_err().to_string();
            assert!(error.contains("Malformed UTF-8"), "{value}: {error}");
        }
        for value in ["41f490808042", "41f88880808042"] {
            assert!(matches!(
                decode_perl_code_points(&hex(value)),
                Err(DecodeError::AboveUnicode { offset: 1 })
            ));
        }
    }

    #[test]
    fn round_trip_hazards_are_refused() {
        // U+10000 packs to 0000; a leading FEFF/FFFE would read as a BOM.
        for value in ["41f090808042", "efbbbfeda080", "efbfbeeda080"] {
            assert!(xp_value("XPTitle", &hex(value)).is_err(), "{value}");
        }
    }

    #[test]
    fn only_xp_destinations_take_bytes() {
        for name in [
            "IFD0:XPTitle",
            "IFD0:XPComment",
            "EXIF:XPAuthor",
            "EXIF:XPKeywords",
        ] {
            assert!(tag_value(name, b"A\xed\xa0\x80").is_ok(), "{name}");
        }
        let error = tag_value("XPTitle", b"A\xed\xa0\x80")
            .unwrap_err()
            .to_string();
        assert!(error.contains("-IFD0:XPTitle="), "{error}");
        for name in [
            "Artist",
            "IFD0:Artist",
            "XMP:XPTitle",
            "ExifIFD:XPTitle",
            "xptitle",
            "XPTitleX",
        ] {
            let error = tag_value(name, b"A\xffB").unwrap_err().to_string();
            assert!(error.contains("not valid UTF-8"), "{name}: {error}");
            assert!(error.contains("41 ff 42"), "{name}: {error}");
        }
    }

    #[test]
    fn split_and_unquote_keep_the_value_bytes() {
        let arg = OsStr::new("-IFD0:XPTitle=A=B");
        let (name, value) = split_at_equals(arg).unwrap();
        assert_eq!(
            (name, value),
            (OsStr::new("-IFD0:XPTitle"), OsStr::new("A=B"))
        );
        assert!(split_at_equals(OsStr::new("-Make")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn unquote_matches_the_utf8_rule_on_non_utf8_values() {
        use std::os::unix::ffi::OsStrExt;
        let os = |b: &'static [u8]| OsStr::from_bytes(b);
        for (input, expected) in [
            (&b"\"A\xffB\""[..], &b"A\xffB"[..]),
            (b"  '\xff'  ", b"\xff"),
            (b"\xe2\x80\x83\"\xff\"\t", b"\xff"),
            (b"\"A\xffB", b"\"A\xffB"),
            (b" A\xffB ", b" A\xffB "),
            (b"\"\xff'", b"\"\xff'"),
        ] {
            assert_eq!(unquote(os(input)), os(expected), "{input:?}");
        }
    }

    /// FixUTF8 as the oracle applies it to `-j` output and to XMP values.
    #[test]
    fn fix_utf8_replaces_each_bad_byte_with_a_question_mark() {
        for (input, expected) in [
            (&b"n\xff.jpg"[..], "n?.jpg"),
            (b"A\xed\xa0\x80B", "A???B"),
            (b"caf\xe9", "caf?"),
            (b"A\xef\xbf\xbeB", "A???B"),
            (b"\xc3\xa9\xe4\xb8\xad", "é中"),
            (b"\xf0\x9f\x8e\x8c", "\u{1F38C}"),
            (b"\xf4\x90\x80\x80", "????"),
            (b"\xe2\x82", "??"),
            (b"plain", "plain"),
        ] {
            assert_eq!(fix_utf8(input), expected, "{input:?}");
        }
    }
}
