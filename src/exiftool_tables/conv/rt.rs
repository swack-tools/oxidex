//! The runtime the generated conversions (`conv/<module>.rs`) are written
//! against: Perl's scalar operators over [`MemberVal`], one function per
//! operator the backend (`tools/exiftool-tables/conv_codegen.py`) emits.
//!
//! Every function here reproduces the pinned perl 5.38.2 exactly on the
//! inputs it accepts, or DECLINES ([`Decline`]) -- never an approximation.
//! It is proven only through the arms that call it: `conv_oracle.py` runs
//! each emitted arm's source through ExifTool's own `FoundTag`/`GetValue`
//! on a probe battery and `conv::tests` requires byte-identical output. An
//! operator no emitted arm uses is not in this file.
//!
//! Byte semantics: an ExifTool value from `ReadValue` is a Perl BYTE string
//! (no UTF-8 flag, no `use utf8`, no `unicode_strings`), so `\s`, `\d`, `\w`,
//! `uc`, `lc` and `/i` are ASCII-only and `.` matches one byte. Every
//! operator here reads the EXACT string context ([`MemberVal::perl_bytes`])
//! and builds its result with [`MemberVal::from_bytes`]: a value that is not
//! UTF-8 travels as [`MemberVal::Bytes`] end to end, never through a lossy
//! `String`. (Whether a final value can be SHOWN is the caller's question:
//! the IFD walk declines a row whose text is not UTF-8.) Every regex is
//! compiled `(?-u)` over the bytes.
//!
//! Perl's `$` (no `/m`) matches at the end OR before a final newline:
//! at position `p` iff `p == len`, or `p == len - 1` and the last byte is
//! `\n`. Two exact forms are emitted (`conv_codegen.py`
//! `translate_regex`): a `$` that ends the whole pattern becomes the
//! capture `(?P<eol>\n?)\z` ([`EOL`]) -- for a match only existence matters,
//! and for a single substitution the `eol` group is kept out of the replaced
//! span, which is exactly Perl's (the leftmost-first path through the rest
//! of the pattern is the same, and at each end point exactly one branch of
//! `\n?` can succeed). Any other `$` is emitted as `\z` with `dollar:
//! true`, which is exact whenever the subject does not END with `\n`; a
//! subject that does declines.

use regex::bytes::Regex;

use crate::exiftool_tables::helpers::{sprintf_d, sprintf_f};
use crate::exiftool_tables::session::{MemberVal, PerlNum, numify_bytes};

/// Why an arm did not produce a value: a branch this runtime does not model.
/// The caller's existing path runs instead (mixed mode, per entry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decline(pub &'static str);

pub type R<T> = Result<T, Decline>;

/// A conversion's result: a plain scalar, or a SCALAR reference (`\$val`),
/// which ExifTool prints as `(Binary data N bytes, use -b option to
/// extract)` and never runs a `PrintConv` over (ExifTool.pm:3522).
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Scalar(MemberVal),
    Binary(Vec<u8>),
}

const TWO_53: f64 = 9_007_199_254_740_992.0;
const TWO_63: f64 = 9_223_372_036_854_775_808.0;
const TWO_64: f64 = 18_446_744_073_709_551_616.0;

// ---------------------------------------------------------------------------
// Scalars
// ---------------------------------------------------------------------------

#[must_use]
pub fn int(i: i64) -> MemberVal {
    MemberVal::Int(i)
}

#[must_use]
pub fn float(f: f64) -> MemberVal {
    MemberVal::Float(f)
}

#[must_use]
pub fn string(s: &str) -> MemberVal {
    MemberVal::Str(s.to_string())
}

fn from_num(n: PerlNum) -> MemberVal {
    MemberVal::from(n)
}

/// A Perl byte string built by an operator.
fn bytes_val(b: Vec<u8>) -> MemberVal {
    MemberVal::from_bytes(b)
}

/// The capture name a pattern-final `$` compiles to (see the module doc).
pub const EOL: &str = "eol";

/// Perl boolean context.
#[must_use]
pub fn truthy(v: &MemberVal) -> bool {
    v.is_truthy()
}

/// `!$x` / `not $x`: `PL_sv_yes` / `PL_sv_no`.
#[must_use]
pub fn not(v: &MemberVal) -> MemberVal {
    MemberVal::Bool(!v.is_truthy())
}

/// `defined $x`.
#[must_use]
pub fn defined(v: &MemberVal) -> MemberVal {
    MemberVal::Bool(v.is_defined())
}

// ---------------------------------------------------------------------------
// Arithmetic (pp_add, pp_subtract, pp_multiply, pp_divide, pp_pow, pp_negate)
// ---------------------------------------------------------------------------

/// Perl keeps an IV result when both operands are IVs and the result does
/// not overflow; otherwise the NV result, over each operand's NV-context
/// value (`MemberVal::perl_nv`: `"-0"` is -0.0 there).
fn arith(
    a: &MemberVal,
    b: &MemberVal,
    iop: fn(i64, i64) -> Option<i64>,
    fop: fn(f64, f64) -> f64,
) -> MemberVal {
    match (a.perl_num(), b.perl_num()) {
        (PerlNum::Int(x), PerlNum::Int(y)) => match iop(x, y) {
            Some(r) => MemberVal::Int(r),
            None => MemberVal::Float(fop(x as f64, y as f64)),
        },
        _ => MemberVal::Float(fop(a.perl_nv(), b.perl_nv())),
    }
}

#[must_use]
pub fn add(a: &MemberVal, b: &MemberVal) -> MemberVal {
    arith(a, b, i64::checked_add, |x, y| x + y)
}

#[must_use]
pub fn sub(a: &MemberVal, b: &MemberVal) -> MemberVal {
    arith(a, b, i64::checked_sub, |x, y| x - y)
}

#[must_use]
pub fn mul(a: &MemberVal, b: &MemberVal) -> MemberVal {
    arith(a, b, i64::checked_mul, |x, y| x * y)
}

/// `pp_divide`: an exact quotient of two integers is an IV (64-bit perl
/// defines `PERL_TRY_UV_DIVIDE`); a zero divisor dies (`Illegal division by
/// zero`), which declines. Integers beyond 2**53 decline: the UV/IV paths
/// print digits a double would not.
pub fn div(a: &MemberVal, b: &MemberVal) -> R<MemberVal> {
    let (x, y) = (a.perl_num(), b.perl_num());
    if y.as_f64() == 0.0 {
        return Err(Decline("Illegal division by zero (Perl dies)"));
    }
    if let (PerlNum::Int(p), PerlNum::Int(q)) = (x, y) {
        if p.unsigned_abs() > (1 << 53) || q.unsigned_abs() > (1 << 53) {
            return Err(Decline("integer division beyond 2**53"));
        }
        if q != 0 && p % q == 0 {
            return Ok(MemberVal::Int(p / q));
        }
        return Ok(MemberVal::Float(p as f64 / q as f64));
    }
    Ok(MemberVal::Float(a.perl_nv() / b.perl_nv()))
}

/// `pp_pow`: always an NV (`2**50` prints `1.12589990684262e+15`). Two
/// integer operands take the integer branch, whose base is the IV (so `"-0"`
/// is +0 there); anything else is C `pow()` over the NV-context operands,
/// which Rust's `powf` calls on the same libm.
#[must_use]
pub fn pow(a: &MemberVal, b: &MemberVal) -> MemberVal {
    match (a.perl_num(), b.perl_num()) {
        (PerlNum::Int(x), PerlNum::Int(y)) => MemberVal::Float((x as f64).powf(y as f64)),
        _ => MemberVal::Float(a.perl_nv().powf(b.perl_nv())),
    }
}

/// Unary minus. On a string that is not a plain number Perl's pp_negate
/// flips the sign character instead (see `helpers::guard_string_negation`);
/// declined.
pub fn neg(a: &MemberVal) -> R<MemberVal> {
    if let MemberVal::Str(_) | MemberVal::Bytes(_) = a {
        let t = a.perl_bytes();
        let simple = !t.is_empty()
            && t.iter().all(|&c| c.is_ascii_digit() || c == b'.')
            && t.iter().filter(|&&c| c == b'.').count() <= 1
            && t.iter().any(u8::is_ascii_digit);
        if !simple {
            return Err(Decline("unary minus on a non-plain-number string"));
        }
    }
    Ok(match a.perl_num() {
        PerlNum::Int(i) if i != i64::MIN => MemberVal::Int(-i),
        n => MemberVal::Float(-n.as_f64()),
    })
}

/// `abs($x)`.
#[must_use]
pub fn abs(a: &MemberVal) -> MemberVal {
    match a.perl_num() {
        PerlNum::Int(i) if i != i64::MIN => MemberVal::Int(i.abs()),
        n => MemberVal::Float(n.as_f64().abs()),
    }
}

/// `int($x)` (pp_int): an IV when the truncation fits one; a UV-range value
/// declines (it prints digits, a double `%.15g`).
pub fn int_of(a: &MemberVal) -> R<MemberVal> {
    crate::exiftool_tables::helpers::perl_int(a.perl_num())
        .map(from_num)
        .map_err(|_| Decline("int() in the UV range"))
}

// ---------------------------------------------------------------------------
// Bitwise (UV semantics: no `use integer` in ExifTool's evals)
// ---------------------------------------------------------------------------

/// `SvUV`: an IV as its two's-complement UV, an NV truncated toward zero
/// (negative through IV, saturating at the ends).
fn uv(a: &MemberVal) -> u64 {
    match a.perl_num() {
        PerlNum::Int(i) => i as u64,
        PerlNum::Float(f) if f.is_nan() => 0,
        PerlNum::Float(f) if f < 0.0 => {
            if f <= -TWO_63 {
                i64::MIN as u64
            } else {
                (f as i64) as u64
            }
        }
        PerlNum::Float(f) => {
            if f >= TWO_64 {
                u64::MAX
            } else {
                f as u64
            }
        }
    }
}

fn from_uv(u: u64) -> R<MemberVal> {
    i64::try_from(u)
        .map(MemberVal::Int)
        .map_err(|_| Decline("UV result beyond IV_MAX"))
}

/// A bitwise operator on a string operand that is not numeric in Perl's
/// sense would be a STRING bitwise op when BOTH are strings; ExifTool's
/// expressions always have one numeric-literal operand, so the numeric form
/// is the only one reached. The backend refuses a string-string form.
pub fn band(a: &MemberVal, b: &MemberVal) -> R<MemberVal> {
    from_uv(uv(a) & uv(b))
}

pub fn bor(a: &MemberVal, b: &MemberVal) -> R<MemberVal> {
    from_uv(uv(a) | uv(b))
}

/// `>>` / `<<`: a shift of 64 or more is 0 (perl 5.24+); a negative count
/// shifts the other way.
pub fn shr(a: &MemberVal, b: &MemberVal) -> R<MemberVal> {
    shift(uv(a), b.perl_num(), true)
}

pub fn shl(a: &MemberVal, b: &MemberVal) -> R<MemberVal> {
    shift(uv(a), b.perl_num(), false)
}

fn shift(x: u64, n: PerlNum, right: bool) -> R<MemberVal> {
    let n = match n {
        PerlNum::Int(i) => i,
        PerlNum::Float(f) if f.is_finite() => f.trunc() as i64,
        PerlNum::Float(_) => return Err(Decline("shift by a non-finite count")),
    };
    let (right, n) = if n < 0 {
        (!right, n.unsigned_abs())
    } else {
        (right, n as u64)
    };
    let r = if n >= 64 {
        0
    } else if right {
        x >> n
    } else {
        x << n
    };
    from_uv(r)
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub enum Cmp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

/// `<`, `>`, `<=`, `>=`, `==`, `!=`: IV against IV exactly, anything else as
/// doubles (NaN compares false, `!=` true).
#[must_use]
pub fn num_cmp(op: Cmp, a: &MemberVal, b: &MemberVal) -> MemberVal {
    let r = match (a.perl_num(), b.perl_num()) {
        (PerlNum::Int(x), PerlNum::Int(y)) => match op {
            Cmp::Lt => x < y,
            Cmp::Gt => x > y,
            Cmp::Le => x <= y,
            Cmp::Ge => x >= y,
            Cmp::Eq => x == y,
            Cmp::Ne => x != y,
        },
        (x, y) => {
            let (x, y) = (x.as_f64(), y.as_f64());
            match op {
                Cmp::Lt => x < y,
                Cmp::Gt => x > y,
                Cmp::Le => x <= y,
                Cmp::Ge => x >= y,
                Cmp::Eq => x == y,
                Cmp::Ne => x != y,
            }
        }
    };
    MemberVal::Bool(r)
}

/// `lt`, `gt`, `le`, `ge`, `eq`, `ne`: byte-wise, as Perl compares two byte
/// strings.
#[must_use]
pub fn str_cmp(op: Cmp, a: &MemberVal, b: &MemberVal) -> MemberVal {
    let (x, y) = (a.perl_bytes(), b.perl_bytes());
    let (x, y) = (&*x, &*y);
    MemberVal::Bool(match op {
        Cmp::Lt => x < y,
        Cmp::Gt => x > y,
        Cmp::Le => x <= y,
        Cmp::Ge => x >= y,
        Cmp::Eq => x == y,
        Cmp::Ne => x != y,
    })
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

/// `.`
#[must_use]
pub fn concat(a: &MemberVal, b: &MemberVal) -> MemberVal {
    let mut s = a.perl_bytes().into_owned();
    s.extend_from_slice(&b.perl_bytes());
    bytes_val(s)
}

/// `length($x)`: bytes; `undef` for `undef`.
#[must_use]
pub fn length(a: &MemberVal) -> MemberVal {
    a.perl_length()
        .map_or(MemberVal::Undef, |n| MemberVal::Int(n as i64))
}

/// `uc` on a byte string: ASCII letters only.
#[must_use]
pub fn uc(a: &MemberVal) -> MemberVal {
    bytes_val(a.perl_bytes().to_ascii_uppercase())
}

/// `lc` on a byte string: ASCII letters only.
#[must_use]
pub fn lc(a: &MemberVal) -> MemberVal {
    bytes_val(a.perl_bytes().to_ascii_lowercase())
}

/// `unpack("H*", $x)`: lowercase hex of the bytes, high nybble first.
#[must_use]
pub fn unpack_hex(a: &MemberVal) -> MemberVal {
    use std::fmt::Write;
    let s = a.perl_bytes();
    let mut out = String::with_capacity(s.len() * 2);
    for &b in s.iter() {
        let _ = write!(out, "{b:02x}");
    }
    MemberVal::Str(out)
}

/// `split ' ', $x` (awk mode): leading whitespace dropped, runs of ASCII
/// whitespace separate, trailing empty fields removed.
#[must_use]
pub fn split_ws(a: &MemberVal) -> Vec<MemberVal> {
    a.perl_bytes()
        .split(|&c| crate::exiftool_tables::session::is_perl_space(c))
        .filter(|s| !s.is_empty())
        .map(|s| bytes_val(s.to_vec()))
        .collect()
}

/// `join($sep, LIST)`.
#[must_use]
pub fn join(sep: &MemberVal, items: &[MemberVal]) -> MemberVal {
    let sep = sep.perl_bytes();
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(&sep);
        }
        out.extend_from_slice(&item.perl_bytes());
    }
    bytes_val(out)
}

/// `++$x` on a numeric scalar: an IV (or `undef`, which is 0) increments as
/// an integer, an NV as a double. A string that is not a plain integer would
/// take Perl's magic string increment (`"aa"` -> `"ab"`) and declines.
pub fn preinc(v: &MemberVal) -> R<MemberVal> {
    Ok(match v {
        MemberVal::Undef => MemberVal::Int(1),
        MemberVal::Int(i) => i
            .checked_add(1)
            .map_or(MemberVal::Float(*i as f64 + 1.0), MemberVal::Int),
        MemberVal::Float(f) => MemberVal::Float(f + 1.0),
        MemberVal::Bool(b) => MemberVal::Int(i64::from(*b) + 1),
        MemberVal::Bytes(_) => return Err(Decline("++ on a non-UTF-8 byte string")),
        MemberVal::Str(s) => match s.parse::<i64>() {
            Ok(i) if i.to_string() == *s => i
                .checked_add(1)
                .map_or(MemberVal::Float(i as f64 + 1.0), MemberVal::Int),
            _ => return Err(Decline("++ on a string (Perl's magic increment)")),
        },
    })
}

/// `split /re/, $x` for a pattern that cannot match the empty string and
/// has no capture group (the backend checks the latter): the fields between
/// matches, a leading empty field kept, trailing empty fields removed.
pub fn split_re(re: &Regex, v: &MemberVal) -> R<Vec<MemberVal>> {
    if re.is_match(b"") {
        return Err(Decline("split pattern that can match the empty string"));
    }
    let b = v.perl_bytes();
    let mut fields = Vec::new();
    let mut at = 0;
    for m in re.find_iter(&b) {
        fields.push(b[at..m.start()].to_vec());
        at = m.end();
    }
    fields.push(b[at..].to_vec());
    while fields.last().is_some_and(Vec::is_empty) {
        fields.pop();
    }
    Ok(fields.into_iter().map(bytes_val).collect())
}

/// `$conv->{$key}` inside a hash's `OTHER` sub: the hash's own entry, or
/// `undef`.
#[must_use]
pub fn hash_get(map: &[(&'static str, &'static str)], key: &MemberVal) -> MemberVal {
    lookup(map, &key.perl_bytes()).map_or(MemberVal::Undef, string)
}

/// `$array[$i]` for a Perl array: the index is `SvIV` of the scalar --
/// truncated toward zero, a NaN 0, a double at or beyond 2**63 cast through
/// UV (`UV_MAX`, i.e. -1, for anything past 2**64 including `Inf`), below
/// -2**63 `IV_MIN` -- then negative from the end, out of range `undef`.
#[must_use]
pub fn index(items: &[MemberVal], i: &MemberVal) -> MemberVal {
    let i = match i.perl_num() {
        PerlNum::Int(i) => i,
        PerlNum::Float(f) if f.is_nan() => 0,
        PerlNum::Float(f) if f >= TWO_63 => {
            if f >= TWO_64 {
                -1
            } else {
                (f as u64) as i64
            }
        }
        PerlNum::Float(f) if f < -TWO_63 => i64::MIN,
        PerlNum::Float(f) => f.trunc() as i64,
    };
    let n = items.len() as i64;
    let k = if i < 0 { n + i } else { i };
    if (0..n).contains(&k) {
        items[k as usize].clone()
    } else {
        MemberVal::Undef
    }
}

// ---------------------------------------------------------------------------
// Regular expressions
// ---------------------------------------------------------------------------

/// The subject's bytes. With `dollar` (a `$` emitted as `\z`, module doc)
/// a subject ending in `\n` declines: only there does Perl's `$` also match
/// before the last byte.
fn subject(v: &MemberVal, dollar: bool) -> R<std::borrow::Cow<'_, [u8]>> {
    let s = v.perl_bytes();
    if dollar && s.last() == Some(&b'\n') {
        return Err(Decline(
            "`$` against a subject ending in a newline (Perl's before-final-newline match)",
        ));
    }
    Ok(s)
}

/// `$x =~ /re/`: yes / no.
pub fn re_match(re: &Regex, dollar: bool, v: &MemberVal) -> R<MemberVal> {
    let s = subject(v, dollar)?;
    Ok(MemberVal::Bool(re.is_match(&s)))
}

/// `$x =~ s/re/repl/[g]` with a literal replacement: `(new value, whether
/// it substituted)`. With no match the scalar is left exactly as it was
/// (Perl's pp_subst does not touch it, so a number stays a number); with one
/// it becomes the new byte string. A non-global pattern whose final `$`
/// compiled to the [`EOL`] group replaces the span WITHOUT that group, as
/// Perl's zero-width `$` would.
pub fn subst(
    re: &Regex,
    dollar: bool,
    v: &MemberVal,
    repl: &str,
    global: bool,
) -> R<(MemberVal, bool)> {
    let s = subject(v, dollar)?;
    let repl = repl.as_bytes();
    if global {
        if re.capture_names().any(|n| n == Some(EOL)) {
            return Err(Decline("s///g with a pattern-final `$`"));
        }
        if !re.is_match(&s) {
            return Ok((v.clone(), false));
        }
        let out = re.replace_all(&s, regex::bytes::NoExpand(repl));
        return Ok((bytes_val(out.into_owned()), true));
    }
    let Some(caps) = re.captures(&s) else {
        return Ok((v.clone(), false));
    };
    let whole = caps.get(0).expect("group 0");
    let end = caps.name(EOL).map_or(whole.end(), |m| m.start());
    let mut out = Vec::with_capacity(s.len() + repl.len());
    out.extend_from_slice(&s[..whole.start()]);
    out.extend_from_slice(repl);
    out.extend_from_slice(&s[end..]);
    Ok((bytes_val(out), true))
}

/// `s///`'s own value, non-global: 1 on a substitution, else `PL_sv_no`.
#[must_use]
pub fn subst_count(hit: bool) -> MemberVal {
    if hit {
        MemberVal::Int(1)
    } else {
        MemberVal::Bool(false)
    }
}

/// `$x =~ tr/from/to/` for literal byte lists of equal length (no ranges,
/// no flags): each byte of `from` becomes the byte at the same position of
/// `to`; the first occurrence of a repeated `from` byte wins. (pp_trans
/// forces the scalar to a string whether or not a byte changed.)
pub fn tr(v: &MemberVal, from: &[u8], to: &[u8]) -> R<MemberVal> {
    let out = v
        .perl_bytes()
        .iter()
        .map(|&b| from.iter().position(|&f| f == b).map_or(b, |i| to[i]))
        .collect();
    Ok(bytes_val(out))
}

/// `hex($x)` for a string of plain hex digits (an optional `0x`/`x`
/// prefix, at most 15 digits, so the UV fits an IV): the IV. Anything else
/// -- underscores, an illegal digit (Perl warns and stops), an empty string,
/// a value past `IV_MAX` (a UV prints unlike an IV) -- declines.
pub fn hex(v: &MemberVal) -> R<MemberVal> {
    let b = v.perl_bytes();
    let digits = b
        .strip_prefix(b"0x")
        .or_else(|| b.strip_prefix(b"0X"))
        .or_else(|| b.strip_prefix(b"x"))
        .or_else(|| b.strip_prefix(b"X"))
        .unwrap_or(&b);
    if digits.is_empty() || digits.len() > 15 || !digits.iter().all(u8::is_ascii_hexdigit) {
        return Err(Decline("hex() of a string that is not plain hex digits"));
    }
    let text = std::str::from_utf8(digits).expect("ASCII hex digits");
    Ok(MemberVal::Int(
        i64::from_str_radix(text, 16).expect("at most 15 hex digits"),
    ))
}

// ---------------------------------------------------------------------------
// sprintf
// ---------------------------------------------------------------------------

/// One piece of a literal `sprintf` format, split by the backend.
#[derive(Clone, Copy, Debug)]
pub enum Fmt {
    Lit(&'static str),
    /// `%[-][0][width][.prec]conv` with conv one of `s d f x X`.
    Spec {
        minus: bool,
        zero: bool,
        width: Option<usize>,
        prec: Option<usize>,
        conv: u8,
    },
}

/// Width padding. Perl pads by characters, which for a byte string are
/// bytes.
fn pad(body: Vec<u8>, minus: bool, zero: bool, width: Option<usize>) -> Vec<u8> {
    let Some(w) = width else { return body };
    if body.len() >= w {
        return body;
    }
    let fill = w - body.len();
    if minus {
        let mut out = body;
        out.resize(w, b' ');
        return out;
    }
    if zero {
        let (sign, digits): (&[u8], &[u8]) = match body.strip_prefix(b"-") {
            Some(rest) => (b"-", rest),
            None => (b"", &body),
        };
        if digits.iter().all(|&c| c.is_ascii_hexdigit() || c == b'.') {
            let mut out = sign.to_vec();
            out.resize(sign.len() + fill, b'0');
            out.extend_from_slice(digits);
            return out;
        }
    }
    let mut out = vec![b' '; fill];
    out.extend_from_slice(&body);
    out
}

/// `sprintf(FORMAT, LIST)` for a format the backend parsed. `%d` follows
/// Perl's IV cast (`helpers::sprintf_d`), `%.Nf` Perl's (and C's) exact
/// rounding (`helpers::sprintf_f`), `%x` the UV of the value; a precision on
/// `%x`/`%d` is a minimum digit count. Anything else the backend refuses.
pub fn sprintf(fmt: &[Fmt], args: &[MemberVal]) -> R<MemberVal> {
    let mut out: Vec<u8> = Vec::new();
    let mut next = 0;
    for piece in fmt {
        match *piece {
            Fmt::Lit(s) => out.extend_from_slice(s.as_bytes()),
            Fmt::Spec {
                minus,
                zero,
                width,
                prec,
                conv,
            } => {
                let arg = args.get(next).cloned().unwrap_or(MemberVal::Undef);
                next += 1;
                let body = match conv {
                    // A byte string: a precision counts bytes.
                    b's' => {
                        let s = arg.perl_bytes();
                        match prec {
                            Some(p) if p < s.len() => s[..p].to_vec(),
                            _ => s.into_owned(),
                        }
                    }
                    b'd' => {
                        let n = arg.perl_num();
                        if let PerlNum::Float(f) = n {
                            if f.abs() >= TWO_63 && f.is_finite() {
                                return Err(Decline("%d of a double beyond the IV range"));
                            }
                        }
                        let s = sprintf_d(n, false);
                        match prec {
                            Some(p) => {
                                let (sign, digits) = match s.strip_prefix('-') {
                                    Some(d) => ("-", d),
                                    None => ("", s.as_str()),
                                };
                                if digits.len() < p {
                                    format!("{sign}{}{digits}", "0".repeat(p - digits.len()))
                                } else {
                                    s.clone()
                                }
                            }
                            None => s,
                        }
                        .into_bytes()
                    }
                    b'f' => sprintf_f(prec.unwrap_or(6), arg.perl_nv()).into_bytes(),
                    b'x' | b'X' => {
                        if let PerlNum::Float(f) = arg.perl_num() {
                            if !f.is_finite() {
                                return Err(Decline("%x of a non-finite value"));
                            }
                        }
                        let u = uv(&arg);
                        let mut s = format!("{u:x}");
                        if let Some(p) = prec {
                            if s.len() < p {
                                s = "0".repeat(p - s.len()) + &s;
                            }
                        }
                        if conv == b'X' {
                            s = s.to_ascii_uppercase();
                        }
                        s.into_bytes()
                    }
                    _ => return Err(Decline("sprintf conversion not modelled")),
                };
                out.extend_from_slice(&pad(body, minus, zero && prec.is_none(), width));
            }
        }
    }
    Ok(bytes_val(out))
}

// ---------------------------------------------------------------------------
// Hash conversions (GetValue, ExifTool.pm:3610-3640) and DecodeBits
// ---------------------------------------------------------------------------

/// A `PrintConv`/`ValueConv` hash, as the backend emits it.
#[derive(Clone, Copy, Debug)]
pub struct HashConv {
    /// Sorted by key (byte order) for binary search. Keys are the Perl hash
    /// keys verbatim.
    pub map: &'static [(&'static str, &'static str)],
    /// `BITMASK => { bit => label }`, bit numbers as integers.
    pub bitmask: Option<&'static [(i64, &'static str)]>,
    /// The tag's `BitsPerWord` (`DecodeBits` defaults to 32).
    pub bits_per_word: Option<u32>,
    /// `OTHER => sub { ... }`, compiled by the backend: called as
    /// `&{$$conv{OTHER}}($val, undef, $conv)`.
    pub other: Option<fn(&MemberVal) -> R<MemberVal>>,
    /// The tag's `PrintHex` flag.
    pub print_hex: bool,
}

fn lookup(map: &[(&'static str, &'static str)], key: &[u8]) -> Option<&'static str> {
    map.binary_search_by(|(k, _)| k.as_bytes().cmp(key))
        .ok()
        .map(|i| map[i].1)
}

/// ExifTool.pm:3610-3632, for one scalar `$val`:
///
/// ```perl
/// if (not defined($value = $$conv{$val})) {
///     if ($$conv{BITMASK}) {
///         $value = DecodeBits($val, $$conv{BITMASK}, $$tagInfo{BitsPerWord});
///     } else {
///         if ($$conv{OTHER}) { $value = &{$$conv{OTHER}}($val, undef, $conv); }
///         if (not defined $value) {
///             if ($$tagInfo{PrintHex} and defined $val and IsInt($val) and
///                 $convType eq 'PrintConv') {
///                 $value = sprintf('Unknown (0x%x)',$val);
///             } else {
///                 $value = "Unknown ($val)";
///             }
///         }
///     }
/// }
/// ```
///
/// An `undef` `$val` is the hash key `""` (with a warning), as in Perl.
pub fn hash_conv(val: &MemberVal, conv: &HashConv, print_conv: bool) -> R<MemberVal> {
    let key = val.perl_bytes();
    if let Some(v) = lookup(conv.map, &key) {
        return Ok(string(v));
    }
    if let Some(bits) = conv.bitmask {
        return decode_bits(val, bits, conv.bits_per_word);
    }
    if let Some(other) = conv.other {
        let v = other(val)?;
        if v.is_defined() {
            return Ok(v);
        }
    }
    if conv.print_hex && print_conv && val.is_defined() {
        let int_like = {
            let b: &[u8] = &key;
            let b = b.strip_suffix(b"\n").unwrap_or(b);
            let digits = b
                .strip_prefix(b"+")
                .or_else(|| b.strip_prefix(b"-"))
                .unwrap_or(b);
            !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
        };
        if int_like {
            if matches!(numify_bytes(&key), PerlNum::Float(_)) {
                return Err(Decline("PrintHex of an integer string beyond the IV range"));
            }
            return Ok(MemberVal::Str(format!("Unknown (0x{:x})", uv(val))));
        }
    }
    let mut out = b"Unknown (".to_vec();
    out.extend_from_slice(&key);
    out.push(b')');
    Ok(bytes_val(out))
}

/// `DecodeBits($vals, $lookup, $bits)` (ExifTool.pm:6385-6407).
pub fn decode_bits(
    vals: &MemberVal,
    lookup: &[(i64, &'static str)],
    bits: Option<u32>,
) -> R<MemberVal> {
    let bits = match bits {
        Some(b) if b > 0 => i64::from(b),
        _ => 32,
    };
    if bits > 63 {
        return Err(Decline("BitsPerWord beyond 63"));
    }
    let mut out: Vec<String> = Vec::new();
    let mut num = 0i64;
    for word in split_ws(vals) {
        let w = uv(&word);
        for i in 0..bits {
            if w & (1u64 << i) == 0 {
                continue;
            }
            let n = i + num;
            match lookup.iter().find(|(k, _)| *k == n) {
                Some((_, label)) if !label.is_empty() && *label != "0" => {
                    out.push((*label).to_string());
                }
                _ => out.push(format!("[{n}]")),
            }
        }
        num += bits;
    }
    if out.is_empty() {
        return Ok(string("(none)"));
    }
    Ok(MemberVal::Str(out.join(", ")))
}

/// A list conversion (`PrintConv => [ ... ]`, ExifTool.pm:3563-3584,
/// 3681-3697): `$val` split on whitespace, the i-th item converted by the
/// i-th entry (an item past the list, or a `None` entry, passes through),
/// joined with `"; "` for `PrintConv` and `" "` for `ValueConv`. An empty
/// split returns nothing: the tag is not reported.
pub fn list_conv(
    val: &MemberVal,
    convs: &[Option<fn(&MemberVal) -> R<MemberVal>>],
    print_conv: bool,
) -> R<Option<MemberVal>> {
    let items = split_ws(val);
    if items.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let v = match convs.get(i).copied().flatten() {
            Some(f) => f(item)?,
            None => item.clone(),
        };
        if v.is_defined() {
            out.push(v);
        }
    }
    if out.is_empty() {
        return Ok(None);
    }
    Ok(Some(join(
        &string(if print_conv { "; " } else { " " }),
        &out,
    )))
}
