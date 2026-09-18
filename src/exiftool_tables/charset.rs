//! `Image::ExifTool::Charset` (Charset.pm, pinned 13.59): `Decompose` and
//! `Recompose`, the two subs `Image::ExifTool::Decode` calls, ported over
//! Perl byte strings and a [`Session`].
//!
//! The tables (`%csType`, every `%Image::ExifTool::Charset::<Name>` hash and
//! Charset.pm's pre-loaded `%unicode2byte`) are GENERATED from the pinned
//! source into [`super::charset_tables`] by
//! `tools/exiftool-tables/codegen_charsets.py`; nothing here is typed by
//! hand. The digests of `Decompose`/`Recompose` this port was proven
//! against are [`super::helpers::DECODE_DEPENDENCIES`].
//!
//! Two pieces of Perl's own machinery are part of the behaviour and are
//! modelled exactly, each pinned by the helper oracle's probes:
//!
//! - `unpack('C0U*', $val)` on a byte string ([`perl_utf8_decode`]): each
//!   character goes through perl's `utf8n_to_uvchr_msgs` (5.38 `inline.h`),
//!   whose fast path walks `PL_strict_utf8_dfa_tab` (generated from the
//!   pinned perl's `perl.h`) and returns whatever it accepts -- including
//!   the values it accepts after starting in its own reject state, which a
//!   lead byte of class 1 (`C0`, `C1`, `ED`, `F5`-`FF`) followed by another
//!   lead byte does (`F7 C0 80` is 0x77000, with no warning). Anything the
//!   DFA refuses falls to perl's helper: lax UTF-8 -- surrogates,
//!   non-characters, code points above U+10FFFF and Perl's extended
//!   `\xFE`/`\xFF` forms decode to their value; every malformation (a stray
//!   continuation byte, a sequence cut short by a non-continuation byte or
//!   the end, an overlong form, a value above `IV_MAX`) decodes to 0 and
//!   resumes at the first byte not consumed. The helper (`utf8.c`) is not
//!   installed with perl; its model here is pinned by the oracle's probes,
//!   including a seeded random set.
//! - `pack('C0U*', @uni)` ([`perl_utf8_encode`]): Perl's extended UTF-8,
//!   one to thirteen bytes.
//!
//! Code points are `u64` (Perl's UV): UTF-8 input reaches `IV_MAX`.

use std::collections::HashMap;
use std::sync::LazyLock;

use super::charset_tables::{CS_TYPE, PERL_STRICT_UTF8_DFA_TAB, TABLES, UNICODE2BYTE_LATIN};
use super::helpers::HelperError;
use super::session::{ByteOrder, MemberVal, Session};

/// One value of a `%Image::ExifTool::Charset::<Name>` hash.
#[derive(Clone, Copy, Debug)]
pub enum Conv {
    /// A single code point.
    One(u32),
    /// An ARRAY ref: several code points.
    Many(&'static [u32]),
    /// A HASH ref: the lead byte of a 2-byte code, keyed by the trail byte
    /// (whose values are only `One`/`Many`).
    Lead(&'static [(u32, Conv)]),
}

/// One translation table, entries sorted by key.
#[derive(Debug)]
pub struct CharsetTable {
    pub name: &'static str,
    pub entries: &'static [(u32, Conv)],
}

/// `$csType{$name}`: `None` for a name the hash does not hold.
#[must_use]
pub fn cs_type(name: &[u8]) -> Option<u32> {
    CS_TYPE
        .binary_search_by(|(n, _)| n.as_bytes().cmp(name))
        .ok()
        .map(|i| CS_TYPE[i].1)
}

fn table(name: &str) -> Option<&'static [(u32, Conv)]> {
    TABLES
        .binary_search_by(|t| t.name.cmp(name))
        .ok()
        .map(|i| TABLES[i].entries)
}

/// `$$conv{$key}`: a code point too large for any key is simply absent.
fn lookup(entries: &'static [(u32, Conv)], key: u64) -> Option<&'static Conv> {
    let key = u32::try_from(key).ok()?;
    entries
        .binary_search_by(|(k, _)| k.cmp(&key))
        .ok()
        .map(|i| &entries[i].1)
}

const IV_MAX: u64 = i64::MAX as u64;

/// The fast path of perl 5.38's `Perl_utf8n_to_uvchr_msgs` (`inline.h`) at
/// the start of `s`: `Some((value, length))` when its DFA accepts, `None`
/// when it hands over to the helper. Transcribed statement for statement:
///
/// ```c
/// type = PL_strict_utf8_dfa_tab[*s];
/// if (type == 0) { uv = *s; }             /* (length 1) */
/// else {
///     UV state = PL_strict_utf8_dfa_tab[256 + type];
///     uv = (0xff >> type) & NATIVE_UTF8_TO_I8(*s);
///     while (++s < send) {
///         type  = PL_strict_utf8_dfa_tab[*s];
///         state = PL_strict_utf8_dfa_tab[256 + state + type];
///         uv = UTF8_ACCUMULATE(uv, *s);
///         if (state == 0) goto success;
///         if (UNLIKELY(state == 1)) break;
///     }
///     return _utf8n_to_uvchr_msgs_helper(s0, curlen, ...);
/// }
/// ```
///
/// The initial `state` is never tested, so a class-1 lead starts from the
/// reject state and indexes the table one column off.
fn perl_dfa(s: &[u8]) -> Option<(u64, usize)> {
    let tab = PERL_STRICT_UTF8_DFA_TAB;
    let mut ty = usize::from(tab[usize::from(s[0])]);
    if ty == 0 {
        return Some((u64::from(s[0]), 1));
    }
    let mut state = usize::from(tab[256 + ty]);
    // C shifts the promoted int: a class of 8 or more leaves no bits
    let mut uv = u64::from((0xffu32 >> ty) & u32::from(s[0]));
    for (k, &c) in s.iter().enumerate().skip(1) {
        ty = usize::from(tab[usize::from(c)]);
        state = usize::from(tab[256 + state + ty]);
        uv = (uv << 6) | u64::from(c & 0x3f);
        if state == 0 {
            return Some((uv, k + 1));
        }
        if state == 1 {
            break;
        }
    }
    None
}

/// Perl's `unpack('C0U*', $bytes)` on a byte string (pinned perl 5.38.2),
/// and whether any malformation was met (Perl warns once per malformation;
/// `Decompose` only asks whether there was one).
#[must_use]
pub fn perl_utf8_decode(b: &[u8]) -> (Vec<u64>, bool) {
    let mut out = Vec::with_capacity(b.len());
    let mut malformed = false;
    let mut i = 0;
    while i < b.len() {
        if let Some((uv, len)) = perl_dfa(&b[i..]) {
            out.push(uv);
            i += len;
            continue;
        }
        // _utf8n_to_uvchr_msgs_helper
        let c = b[i];
        let (len, lead_bits, min): (usize, u64, u64) = match c {
            0x00..=0x7f => {
                out.push(u64::from(c));
                i += 1;
                continue;
            }
            0x80..=0xbf => {
                out.push(0);
                malformed = true;
                i += 1;
                continue;
            }
            0xc0..=0xdf => (2, u64::from(c & 0x1f), 0x80),
            0xe0..=0xef => (3, u64::from(c & 0x0f), 0x800),
            0xf0..=0xf7 => (4, u64::from(c & 0x07), 0x1_0000),
            0xf8..=0xfb => (5, u64::from(c & 0x03), 0x20_0000),
            0xfc..=0xfd => (6, u64::from(c & 0x01), 0x400_0000),
            0xfe => (7, 0, 0x8000_0000),
            0xff => (13, 0, 1 << 36),
        };
        let mut j = i + 1;
        while j < b.len() && j < i + len && (0x80..0xc0).contains(&b[j]) {
            j += 1;
        }
        if j - i < len {
            out.push(0);
            malformed = true;
            i = j;
            continue;
        }
        // Up to 72 payload bits (the \xFF form): accumulate in u128.
        let mut v = u128::from(lead_bits);
        for &cb in &b[i + 1..j] {
            v = (v << 6) | u128::from(cb & 0x3f);
        }
        if v < u128::from(min) || v > u128::from(IV_MAX) {
            out.push(0);
            malformed = true;
        } else {
            out.push(v as u64);
        }
        i = j;
    }
    (out, malformed)
}

/// Perl's `pack('C0U*', @uni)`: Perl's extended UTF-8 (a code point past
/// `0x7FFFFFFF` takes the 7-byte `\xFE` form, past `0xFFFFFFFFF` the
/// 13-byte `\xFF` form).
#[must_use]
pub fn perl_utf8_encode(uni: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(uni.len());
    for &cp in uni {
        let (len, lead): (usize, u8) = match cp {
            0..=0x7f => {
                out.push(cp as u8);
                continue;
            }
            0x80..=0x7ff => (2, 0xc0),
            0x800..=0xffff => (3, 0xe0),
            0x1_0000..=0x1f_ffff => (4, 0xf0),
            0x20_0000..=0x3ff_ffff => (5, 0xf8),
            0x400_0000..=0x7fff_ffff => (6, 0xfc),
            0x8000_0000..=0xf_ffff_ffff => (7, 0xfe),
            _ => (13, 0xff),
        };
        let conts = len - 1;
        let lead_payload = if len >= 7 {
            0
        } else {
            (cp >> (6 * conts)) as u8
        };
        out.push(lead | lead_payload);
        for k in (0..conts).rev() {
            let shift = 6 * k;
            let bits = if shift >= 64 { 0 } else { (cp >> shift) & 0x3f };
            out.push(0x80 | bits as u8);
        }
    }
    out
}

/// `GetByteOrder()`: the Session's byte order; never guessed when unset.
fn get_byte_order(session: &Session) -> Result<ByteOrder, HelperError> {
    session.byte_order.ok_or(HelperError::Refused(
        "GetByteOrder() with no byte order on the Session",
    ))
}

/// `$byteOrder eq 'MM'`: only the exact string is big-endian.
fn is_mm(order: &MemberVal) -> bool {
    order.perl_bytes().as_ref() == b"MM"
}

fn unpack_units(val: &[u8], width: usize, big: bool) -> Vec<u64> {
    val.chunks_exact(width)
        .map(|c| {
            c.iter().enumerate().fold(0u64, |acc, (k, &x)| {
                let pos = if big { width - 1 - k } else { k };
                acc | (u64::from(x) << (8 * pos))
            })
        })
        .collect()
}

const NON_SCALAR: HelperError = HelperError::Refused(
    "an ARRAY/HASH entry where Charset.pm assigns the reference itself as a code point",
);

/// `$_ = $$conv{$_} if defined $$conv{$_}` for a fixed-width table. The
/// generated fixed-width tables hold only scalars (the generator checks the
/// destination-capable ones; the 1-byte source-only MacArabic is scalar too),
/// so the reference case refuses rather than numifying an address.
fn map_fixed(conv: &'static [(u32, Conv)], u: u64) -> Result<Option<u64>, HelperError> {
    match lookup(conv, u) {
        None => Ok(None),
        Some(Conv::One(v)) => Ok(Some(u64::from(*v))),
        Some(_) => Err(NON_SCALAR),
    }
}

/// `Image::ExifTool::Charset::Decompose($et, $val, $charset, $byteOrder)`
/// for a `$charset` whose `%csType` entry exists (Decode's only caller
/// path). Returns the code points; sets `$$et{WarnBadUTF8}` /
/// `$$et{WrongByteOrder}` and requests `Warn` exactly as the Perl does.
pub fn decompose(
    session: &mut Session,
    val: &[u8],
    charset: &str,
    byte_order: &MemberVal,
) -> Result<Vec<u64>, HelperError> {
    let ty = cs_type(charset.as_bytes()).ok_or(HelperError::Refused(
        "Decompose of a charset absent from %csType",
    ))?;
    let mut conv = None;
    if ty & 0x001 != 0 {
        conv = Some(table(charset).ok_or(HelperError::Refused(
            "Invalid character set (LoadCharset failed)",
        ))?);
    } else if ty == 0x100 {
        let (uni, malformed) = perl_utf8_decode(val);
        if malformed && !session.member("WarnBadUTF8").is_truthy() {
            session.warn(MemberVal::Str("Malformed UTF-8 character(s)".into()));
            set(session, "WarnBadUTF8")?;
        }
        return Ok(uni);
    }
    if ty & 0x100 != 0 {
        // 1-byte fixed-width characters
        let conv = conv.expect("a 1-byte set other than UTF8/ASCII has a table");
        return val
            .iter()
            .map(|&b| Ok(map_fixed(conv, u64::from(b))?.unwrap_or(u64::from(b))))
            .collect();
    }
    if ty & 0x600 != 0 {
        return decompose_fixed(session, val, charset, ty, conv, byte_order);
    }
    // variable-width characters
    let conv = conv.expect("a variable-width set has a table");
    let mut uni = Vec::with_capacity(val.len());
    let mut i = 0;
    let push = |uni: &mut Vec<u64>, c: &Conv| match c {
        Conv::One(v) => uni.push(u64::from(*v)),
        Conv::Many(vs) => uni.extend(vs.iter().map(|&v| u64::from(v))),
        Conv::Lead(_) => unreachable!("the dump admits no nested lead table"),
    };
    while i < val.len() {
        let ch = val[i];
        i += 1;
        let cv = lookup(conv, u64::from(ch));
        // `$cv or push(@uni, $ch), next;` -- undef (and a zero) pass through
        let cv = match cv {
            None | Some(Conv::One(0)) => {
                uni.push(u64::from(ch));
                continue;
            }
            Some(c) => c,
        };
        let Conv::Lead(trail) = cv else {
            push(&mut uni, cv);
            continue;
        };
        // handle 2-byte character codes
        match val.get(i) {
            Some(&ch2) => match lookup(trail, u64::from(ch2)) {
                // `if ($$cv{$ch})`: a zero code point is false
                Some(c) if !matches!(c, Conv::One(0)) => {
                    push(&mut uni, c);
                    i += 1;
                }
                // encoding error: '?', and the byte is examined again
                _ => uni.push(u64::from(b'?')),
            },
            None => uni.push(u64::from(b'?')),
        }
    }
    Ok(uni)
}

/// `$$et{$key} = 1`.
fn set(session: &mut Session, key: &str) -> Result<(), HelperError> {
    session
        .set_member(key, MemberVal::Int(1))
        .map_err(|_| HelperError::Refused("member typed on Session"))
}

/// Decompose's 2-byte / 4-byte fixed-width branch.
fn decompose_fixed(
    session: &mut Session,
    val: &[u8],
    charset: &str,
    ty: u32,
    conv: Option<&'static [(u32, Conv)]>,
    byte_order: &MemberVal,
) -> Result<Vec<u64>, HelperError> {
    let mut unknown = false;
    let order_mm = if !byte_order.is_truthy() {
        get_byte_order(session)? == ByteOrder::BigEndian
    } else if byte_order.perl_bytes().as_ref() == b"Unknown" {
        unknown = true;
        get_byte_order(session)? == ByteOrder::BigEndian
    } else {
        is_mm(byte_order)
    };
    let mut big = order_mm;
    let width;
    let mut val = val;
    if ty & 0x400 != 0 {
        width = 4;
        // honour BOM if it exists
        if let Some(rest) = val.strip_prefix(b"\0\0\xfe\xff") {
            val = rest;
            big = true;
        } else if let Some(rest) = val.strip_prefix(b"\xff\xfe\0\0") {
            val = rest;
            big = false;
        }
        unknown = false;
    } else {
        width = 2;
        if let Some(rest) = val.strip_prefix(b"\xfe\xff") {
            val = rest;
            big = true;
            unknown = false;
        } else if let Some(rest) = val.strip_prefix(b"\xff\xfe") {
            val = rest;
            big = false;
            unknown = false;
        }
    }
    let mut uni = unpack_units(val, width, big);
    match conv {
        None => {
            if unknown {
                // the byte with more unique values should be the low byte,
                // else the one that is zero more often is the high byte
                let mut bh = std::collections::HashSet::new();
                let mut bl = std::collections::HashSet::new();
                let (mut zh, mut zl) = (0usize, 0usize);
                for &u in &uni {
                    bh.insert(u >> 8);
                    bl.insert(u & 0xff);
                    if u & 0xff00 == 0 {
                        zh += 1;
                    }
                    if u & 0x00ff == 0 {
                        zl += 1;
                    }
                }
                if bh.len() > bl.len() || (bh.len() == bl.len() && zl > zh) {
                    uni = unpack_units(val, width, !big);
                    set(session, "WrongByteOrder")?;
                }
            }
            if charset == "UTF16" {
                // handle surrogate pairs of UTF-16
                let mut i = 0;
                while i + 1 < uni.len() {
                    if uni[i] & 0xfc00 == 0xd800 && uni[i + 1] & 0xfc00 == 0xdc00 {
                        let cp = 0x10000 + ((uni[i] & 0x3ff) << 10) + (uni[i + 1] & 0x3ff);
                        uni.splice(i..i + 2, [cp]);
                    }
                    i += 1;
                }
            }
            Ok(uni)
        }
        Some(conv) => {
            let translate = |units: &mut Vec<u64>| -> Result<usize, HelperError> {
                let mut errors = 0;
                for u in units.iter_mut() {
                    match map_fixed(conv, *u)? {
                        Some(v) => *u = v,
                        None => errors += 1,
                    }
                }
                Ok(errors)
            };
            let e1 = translate(&mut uni)?;
            if unknown && e1 > 0 {
                // try the other byte order ($byteOrder is GetByteOrder here)
                let mut other = unpack_units(val, width, !order_mm);
                let e2 = translate(&mut other)?;
                if e2 < e1 {
                    set(session, "WrongByteOrder")?;
                    return Ok(other);
                }
            }
            Ok(uni)
        }
    }
}

/// The generic inverse Recompose builds (`$inv{$$conv{$char}} = $char` over
/// every scalar entry), cached like `%unicode2byte`. Latin uses Charset.pm's
/// pre-loaded table, which the generator proves identical.
static INVERSES: LazyLock<HashMap<&'static str, HashMap<u64, u64>>> = LazyLock::new(|| {
    let mut all = HashMap::new();
    for t in TABLES {
        let inv: HashMap<u64, u64> = if t.name == "Latin" {
            UNICODE2BYTE_LATIN
                .iter()
                .map(|&(u, b)| (u64::from(u), u64::from(b)))
                .collect()
        } else {
            t.entries
                .iter()
                .filter_map(|(k, c)| match c {
                    Conv::One(u) => Some((u64::from(*u), u64::from(*k))),
                    _ => None,
                })
                .collect()
        };
        all.insert(t.name, inv);
    }
    all
});

/// `Image::ExifTool::Charset::Recompose($et, \@uni, $charset, $byteOrder)`
/// for a `$charset` whose `%csType` entry exists. Sets
/// `$$et{EncodingError}` and requests `Warn` exactly as the Perl does.
pub fn recompose(
    session: &mut Session,
    mut uni: Vec<u64>,
    charset: &str,
    byte_order: &MemberVal,
) -> Result<Vec<u8>, HelperError> {
    let ty = cs_type(charset.as_bytes()).ok_or(HelperError::Refused(
        "Recompose to a charset absent from %csType",
    ))?;
    if ty == 0x100 {
        // UTF8 (also treat ASCII as UTF8): pack, then truncate at a NUL
        let mut out = perl_utf8_encode(&uni);
        truncate_at_nul(&mut out);
        return Ok(out);
    }
    let mut conv = None;
    let mut inv = None;
    if ty & 0x801 != 0 {
        conv = Some(
            table(charset).ok_or(HelperError::Refused("Missing charset (LoadCharset failed)"))?,
        );
        if ty & 0x802 != 0 {
            session.warn(MemberVal::Str(format!(
                "Invalid destination charset {charset}"
            )));
            return Ok(Vec::new());
        }
        inv = INVERSES.get(charset);
    }
    if ty & 0x100 != 0 {
        // 1-byte fixed-width
        let conv = conv.expect("a 1-byte destination other than UTF8 has a table");
        let inv = inv.expect("and an inverse");
        for u in &mut uni {
            if *u < 0x80 {
                continue;
            }
            if let Some(&b) = inv.get(u).filter(|&&b| b != 0) {
                *u = b;
                continue;
            }
            // tables omit bytes equal to their code point: pass those through
            // unless another character owns this byte value
            let owned = !matches!(lookup(conv, *u), None | Some(Conv::One(0)));
            if *u < 0x100 && !owned {
                continue;
            }
            *u = u64::from(b'?');
            if !session.member("EncodingError").is_truthy() {
                session.warn(MemberVal::Str(format!(
                    "Some character(s) could not be encoded in {charset}"
                )));
                set(session, "EncodingError")?;
            }
        }
        let mut out: Vec<u8> = uni.iter().map(|&u| u as u8).collect();
        truncate_at_nul(&mut out);
        return Ok(out);
    }
    // 2-byte and 4-byte fixed-width
    if let Some(inv) = inv {
        for u in &mut uni {
            if let Some(&b) = inv.get(u).filter(|&&b| b != 0) {
                *u = b;
            }
        }
    }
    if charset == "UTF16" {
        // generate surrogate pairs of UTF-16 (Perl's bound excludes 0x10FFFF)
        let mut i = 0;
        while i < uni.len() {
            let u = uni[i];
            if (0x10000..0x10ffff).contains(&u) {
                let t = u - 0x10000;
                let w1 = 0xd800 + ((t >> 10) & 0x3ff);
                let w2 = 0xdc00 + (t & 0x3ff);
                uni.splice(i..=i, [w1, w2]);
                i += 1;
            }
            i += 1;
        }
    }
    let big = if byte_order.is_truthy() {
        is_mm(byte_order)
    } else {
        get_byte_order(session)? == ByteOrder::BigEndian
    };
    let width = if ty & 0x400 != 0 { 4 } else { 2 };
    let mut out = Vec::with_capacity(uni.len() * width);
    for u in uni {
        // `pack` keeps the low 16 / 32 bits of a larger value
        let bytes = u.to_le_bytes();
        let low = &bytes[..width];
        if big {
            out.extend(low.iter().rev());
        } else {
            out.extend_from_slice(low);
        }
    }
    Ok(out)
}

/// `$outVal =~ s/\0.*//s`
fn truncate_at_nul(out: &mut Vec<u8>) {
    if let Some(p) = out.iter().position(|&b| b == 0) {
        out.truncate(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perl_utf8_round_trips_every_encoded_length() {
        for cp in [
            0x41u64,
            0xe9,
            0x20ac,
            0xd800,
            0x1_f600,
            0x11_0000,
            0x20_0000,
            0x400_0000,
            0x8000_0000,
            0xf_ffff_ffff,
            1 << 36,
            IV_MAX,
        ] {
            let enc = perl_utf8_encode(&[cp]);
            assert_eq!(perl_utf8_decode(&enc), (vec![cp], false), "{cp:#x}");
        }
        // pinned perl: pack('C0U*', 0x1000000000) -- the 13-byte form
        assert_eq!(
            perl_utf8_encode(&[1 << 36]),
            b"\xff\x80\x80\x80\x80\x80\x81\x80\x80\x80\x80\x80\x80"
        );
    }

    #[test]
    fn malformations_decode_to_zero_and_resume_where_perl_does() {
        // each from the pinned perl 5.38.2's unpack('C0U*', ...)
        for (bytes, want) in [
            (&b"\x80"[..], vec![0u64]),
            (b"\xc3\x41", vec![0, 0x41]),
            (b"\xe2\x82\x41", vec![0, 0x41]),
            (b"\xc3\xc3", vec![0, 0]),
            (b"\xc0\x80", vec![0]),
            (b"\xe0\x80\xaf", vec![0]),
            (b"\xed\xa0\x80", vec![0xd800]),
            (b"\xf4\x90\x80\x80", vec![0x11_0000]),
            // the DFA walked from its reject state
            (b"\xf7\xc0\x80", vec![0x77000]),
            (b"\xfc\xe2\x82\xac", vec![0x1f2_20ac]),
            (b"\xf7\xfe\xbf\xfe\x41", vec![0x7_7fbf, 0, 0x41]),
        ] {
            assert_eq!(perl_utf8_decode(bytes).0, want, "{bytes:02x?}");
        }
    }

    #[test]
    fn every_table_is_sorted_for_binary_search() {
        assert!(CS_TYPE.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(TABLES.windows(2).all(|w| w[0].name < w[1].name));
        for t in TABLES {
            assert!(t.entries.windows(2).all(|w| w[0].0 < w[1].0), "{}", t.name);
            for (_, c) in t.entries {
                if let Conv::Lead(trail) = c {
                    assert!(trail.windows(2).all(|w| w[0].0 < w[1].0), "{}", t.name);
                }
            }
        }
    }
}
