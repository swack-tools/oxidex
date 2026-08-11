//! Hand-written Rust equivalents of ExifTool's named-helper Perl
//! subroutines, for the expressions `tools/exiftool-tables/exprs.py`'s
//! `compile()` recognises but cannot inline as bare arithmetic.
//!
//! # Why these exist
//!
//! Step 15's expression compiler translates a closed grammar of Perl
//! `ValueConv`/`PrintConv`/`RawConv` snippets to Rust source text spliced
//! directly into the generated `binary_tables.rs` (see
//! `tools/exiftool-tables/exprs.py`). Most of that grammar -- `$val`
//! arithmetic, ternaries, `sprintf`, string interpolation -- compiles to
//! inline expressions with no shared state. A handful of named helpers
//! (`ConvertDateTime`, `PrintExposureTime`, `PrintFNumber`, `GPS::ToDMS`,
//! `Decode`-UCS2, `tr///`) do real work instead, and inlining that work at
//! every call site would both bloat the generated file and make the logic
//! impossible to unit-test independently of codegen. This module is that
//! shared logic; `tools/exiftool-tables/exprs.py`'s compiler emits calls
//! into it by fully-qualified path (`crate::exiftool_tables::exprs::...`).
//!
//! # Verification
//!
//! Every function here is differentially checked against the pinned
//! ExifTool release's own Perl by `tools/exiftool-tables/verify_exprs.py`
//! (`just verify-exprs` is not wired up as a recipe by design -- run it
//! directly, it needs the pinned Perl interpreter and ExifTool source, not
//! just `cargo`). The unit tests below are a second, faster-running line of
//! defense and are not a substitute for that oracle run: they encode the
//! same expectations the oracle asserts, by hand, so a regression is caught
//! by `cargo test` without needing Perl on the machine running it.
//!
//! `ConvertDateTime` is not a function here: `tools/exiftool-tables/
//! exprs.py`'s compiler translates `$self->ConvertDateTime($val)` to the
//! identity function directly (`{v}.to_string()`), because ExifTool's real
//! `ConvertDateTime` only reformats its input when `$self->Options
//! ('DateFormat')` is set (a `-d` CLI flag nothing in this generator's
//! pipeline ever passes) and returns the date unchanged otherwise. See
//! `Image::ExifTool::ConvertDateTime` in the pinned ExifTool source.

/// Perl's default numeric-to-string conversion: `sprintf("%.15g", v)`
/// (`DBL_DIG` = 15 significant digits, the `Gconvert` macro Perl's own `sv_
/// 2pv_flags` uses for a double with no explicit format). This is what `"…
/// $val …"` string interpolation and a bare numeric result both go through
/// in real ExifTool, and it disagrees with Rust's `Display` for `f64` at
/// magnitudes outside roughly `[1e-4, 1e15)`: Rust's `{}` never switches to
/// scientific notation and always keeps every significant digit, so
/// `format!("{}", 1e18)` is `"1000000000000000000"` while Perl prints
/// `"1e+18"`. `verify_exprs.py`'s oracle run caught exactly this on `"$val
/// %"` at `$val = 1e18`; that is the incident this function fixes, not a
/// hypothetical one.
#[must_use]
pub fn perl_num(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "Inf".to_string()
        } else {
            "-Inf".to_string()
        };
    }

    const SIG: usize = 15;
    let neg = v.is_sign_negative();
    let av = v.abs();
    // 15 significant digits in scientific form: one digit before the point,
    // 14 after. Read the exponent back out of Rust's own `{:e}` rendering
    // rather than computing it independently (a hand-rolled log10 disagrees
    // with the formatter's rounding right at power-of-ten boundaries).
    let sci = format!("{:.*e}", SIG - 1, av);
    let (mantissa, exp_str) = sci.split_once('e').expect("`{:e}` always emits 'e'");
    let exp: i32 = exp_str
        .parse()
        .expect("`{:e}`'s exponent is a plain integer");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    debug_assert_eq!(digits.len(), SIG);

    let body = if exp < -4 || exp >= SIG as i32 {
        // %e form: d[.ddddd]e±NN, trailing mantissa zeros stripped.
        let frac = digits[1..].trim_end_matches('0');
        let m = if frac.is_empty() {
            digits[..1].to_string()
        } else {
            format!("{}.{}", &digits[..1], frac)
        };
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else if exp < 0 {
        // 0.000ddddd form.
        let zeros = "0".repeat((-exp - 1) as usize);
        format!("0.{zeros}{}", digits.trim_end_matches('0'))
    } else {
        // ddd[.ddd] form -- `exp + 1` digits land before the point.
        let int_len = (exp + 1) as usize;
        let (int_part, frac_part) = if int_len >= digits.len() {
            (format!("{digits:0<int_len$}"), String::new())
        } else {
            (digits[..int_len].to_string(), digits[int_len..].to_string())
        };
        let frac_trimmed = frac_part.trim_end_matches('0');
        if frac_trimmed.is_empty() {
            int_part
        } else {
            format!("{int_part}.{frac_trimmed}")
        }
    };
    if neg { format!("-{body}") } else { body }
}

/// Perl's stringification of an *integer* (IV) value -- what `int(EXPR)`
/// produces when it is later used as a string, as opposed to `perl_num`
/// above (a plain float/NV). Perl prints an IV as its exact decimal digits
/// unconditionally; it never switches to scientific notation the way `%.15g`
/// does for a float, because the %g-style precision limit is a floating-
/// point-formatting rule, not an integer one. `verify_exprs.py`'s oracle run
/// found this on `$val ? int($val + 0.5) : "n/a"` at `$val = 1e18`: Perl
/// prints `"1000000000000000000"`, and `perl_num` (correctly, for a float)
/// would have printed `"1e+18"`.
///
/// `v` is expected to already be integral (the output of `.trunc()`, which
/// is how `tools/exiftool-tables/exprs.py`'s compiler always calls this);
/// the `as i64` cast truncates toward zero as a second line of defense, not
/// because a fractional input is anything this function's callers produce.
#[must_use]
pub fn perl_int(v: f64) -> String {
    format!("{}", v as i64)
}

/// `Image::ExifTool::Exif::PrintExposureTime($secs)`.
///
/// ExifTool (`Exif.pm`):
/// ```perl
/// sub PrintExposureTime($) {
///     my $secs = shift;
///     return $secs unless Image::ExifTool::IsFloat($secs);
///     if ($secs < 0.25001 and $secs > 0) {
///         return sprintf("1/%d",int(0.5 + 1/$secs));
///     }
///     $_ = sprintf("%.1f",$secs);
///     s/\.0$//;
///     return $_;
/// }
/// ```
/// `secs` here is always finite (it is a decoded numeric tag value), so the
/// `IsFloat` guard -- which only matters for a non-numeric string input --
/// never turns false in this pipeline; it is omitted rather than faked.
#[must_use]
pub fn print_exposure_time(secs: f64) -> String {
    if secs < 0.25001 && secs > 0.0 {
        // Perl's `int()` truncates toward zero; `0.5 + 1/secs` is always
        // positive here, so truncation and floor agree.
        return format!("1/{}", (0.5 + 1.0 / secs).trunc() as i64);
    }
    let s = format!("{secs:.1}");
    match s.strip_suffix(".0") {
        Some(stripped) => stripped.to_string(),
        None => s,
    }
}

/// `Image::ExifTool::Exif::PrintFNumber($val)`.
///
/// ExifTool (`Exif.pm`):
/// ```perl
/// sub PrintFNumber($) {
///     my $val = shift;
///     if (Image::ExifTool::IsFloat($val) and $val > 0) {
///         $val = sprintf(($val<1 ? "%.2f" : "%.1f"), $val);
///     }
///     return $val;
/// }
/// ```
/// As with `print_exposure_time`, the `IsFloat` guard is always true for a
/// numeric input; only the `$val > 0` branch can actually vary.
#[must_use]
pub fn print_f_number(val: f64) -> String {
    if val > 0.0 {
        if val < 1.0 {
            format!("{val:.2}")
        } else {
            format!("{val:.1}")
        }
    } else {
        format!("{val}")
    }
}

/// `Image::ExifTool::GPS::ToDMS($et, $val, 1, $ref)`, restricted to
/// `doPrintConv == 1` (the only value the census's call sites ever pass) and
/// no `CoordFormat` option set (nothing in this generator's pipeline sets
/// one, same rationale as `ConvertDateTime` above).
///
/// ExifTool (`GPS.pm`) with those two conditions fixed, `$fmt` is always
/// `q{%d deg %d' %.2f"}` plus the reference suffix, `$ref` flips
/// N/E to S/W when `val` is negative and takes the sign with it, and a
/// round-off correction re-carries the seconds into minutes/degrees when
/// `sprintf("%.2f", seconds)` rounds up to `60.00`. `ref_letter` is the
/// reference passed at the call site (`N` or `E` in every case Step 15's
/// grammar accepts); `None` reproduces the no-`$ref`-argument call, which
/// drops the sign and the trailing reference letter entirely.
#[must_use]
pub fn gps_to_dms(val: f64, ref_letter: Option<char>) -> String {
    let (val, suffix) = match ref_letter {
        Some('N') => {
            if val < 0.0 {
                (-val, " S".to_string())
            } else {
                (val, " N".to_string())
            }
        }
        Some('E') => {
            if val < 0.0 {
                (-val, " W".to_string())
            } else {
                (val, " E".to_string())
            }
        }
        Some(_) | None => (val.abs(), String::new()),
    };

    let d = val.trunc();
    let m = ((val - d) * 60.0).trunc();
    let s_raw = (val - d - m / 60.0) * 3600.0;

    // ExifTool formats seconds first (`sprintf($fmt[-1], $c[-1])`) and only
    // then checks for a round-up to 60; the check has to run against the
    // *formatted* value; a raw 59.999 that rounds to "60.00" at 2dp must
    // still carry.
    let mut s_val: f64 = format!("{s_raw:.2}").parse().unwrap_or(s_raw);
    let mut m_val = m;
    let mut d_val = d;
    if s_val >= 60.0 {
        s_val -= 60.0;
        m_val += 1.0;
        if m_val >= 60.0 {
            m_val -= 60.0;
            d_val += 1.0;
        }
    }

    format!(
        "{} deg {}' {s_val:.2}\"{suffix}",
        d_val as i64, m_val as i64
    )
}

/// `$self->Decode($val, "UCS2", $order)` with the default `Charset` option
/// (`UTF8`, ExifTool's own default -- see the `Charset` entry in
/// `Image::ExifTool.pm`'s `@Image::ExifTool::availableOptions`). UCS2 is a
/// fixed-width 2-byte encoding for the Basic Multilingual Plane; decoding it
/// as UTF-16 code units (via `char::decode_utf16`) is exact for every
/// codepoint UCS2 itself can represent, and Rust's replacement-character
/// fallback for an unpaired surrogate matches decoding malformed input
/// rather than panicking on it, which is what a metadata reader needs.
///
/// `little_endian` is `true` for ExifTool's `"II"` byte-order argument
/// (Intel), `false` for `"MM"` (Motorola) -- the only two orders Step 15's
/// grammar accepts.
#[must_use]
pub fn decode_ucs2(bytes: &[u8], little_endian: bool) -> String {
    let units = bytes.chunks_exact(2).map(|pair| {
        if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        }
    });
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Perl `$val =~ tr/from/to/[d]; $val`, restricted to the literal
/// (non-range) character classes `tools/exiftool-tables/exprs.py`'s
/// `compile()` accepts -- no `tr/a-z/A-Z/`-style ranges appear anywhere in
/// the pinned release's census, so range expansion was never implemented
/// rather than implemented and left unverified.
///
/// Perl's `tr///` maps `from[i]` to `to[i]` position-wise. A character in
/// `from` past the end of `to` is deleted when the `/d` modifier is present
/// (`delete == true`); otherwise it is replaced with `to`'s last character
/// (Perl repeats the final replacement to cover the rest of `from`), or left
/// unchanged if `to` is empty. A character not in `from` at all passes
/// through unchanged.
#[must_use]
pub fn tr_translate(val: &str, from: &str, to: &str, delete: bool) -> String {
    let from_chars: Vec<char> = from.chars().collect();
    let to_chars: Vec<char> = to.chars().collect();
    let mut out = String::with_capacity(val.len());
    for c in val.chars() {
        match from_chars.iter().position(|&f| f == c) {
            None => out.push(c),
            Some(idx) => {
                if idx < to_chars.len() {
                    out.push(to_chars[idx]);
                } else if delete || to_chars.is_empty() {
                    // deleted (or `to` was empty and not `/d`: nothing to
                    // substitute, so drop the matched character rather than
                    // repeat a replacement that does not exist).
                } else {
                    out.push(*to_chars.last().expect("to_chars is non-empty here"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perl_num_plain_integers_and_fractions() {
        assert_eq!(perl_num(0.0), "0");
        assert_eq!(perl_num(100.0), "100");
        assert_eq!(perl_num(2.5), "2.5");
        assert_eq!(perl_num(-45.5), "-45.5");
        assert_eq!(perl_num(0.125), "0.125");
    }

    #[test]
    fn perl_num_switches_to_scientific_outside_1e_minus4_to_1e15() {
        assert_eq!(perl_num(1e-6), "1e-06");
        assert_eq!(perl_num(1e18), "1e+18");
        assert_eq!(perl_num(-1e18), "-1e+18");
        // The boundary itself (exponent == -4) stays fixed-notation.
        assert_eq!(perl_num(0.0001), "0.0001");
        assert_eq!(perl_num(0.00001), "1e-05");
    }

    #[test]
    fn perl_num_realistic_binary_field_magnitudes_stay_fixed() {
        assert_eq!(perl_num(2147483647.0), "2147483647");
        assert_eq!(perl_num(4294967295.0), "4294967295");
        assert_eq!(perl_num(655.345), "655.345");
    }

    #[test]
    fn perl_int_never_switches_to_scientific_notation() {
        // Where perl_num(1e18) is "1e+18" (a float, %.15g), an int()'d value
        // at the same magnitude stays exact decimal digits (an integer).
        assert_eq!(perl_int(1e18), "1000000000000000000");
        assert_eq!(perl_int(-1e6), "-1000000");
        assert_eq!(perl_int(100.0), "100");
    }

    #[test]
    fn exposure_time_sub_quarter_second_is_a_fraction() {
        assert_eq!(print_exposure_time(0.125), "1/8");
        assert_eq!(print_exposure_time(1.0 / 4000.0), "1/4000");
    }

    #[test]
    fn exposure_time_above_the_0_25001_boundary_is_decimal() {
        // The Perl guard is `$secs < 0.25001`, not `< 0.25` -- 0.25 itself
        // is still a fraction ("1/4"); the boundary sits just past it.
        assert_eq!(print_exposure_time(0.3), "0.3");
        assert_eq!(print_exposure_time(30.0), "30");
        assert_eq!(print_exposure_time(1.5), "1.5");
    }

    #[test]
    fn exposure_time_zero_is_not_a_fraction() {
        // The Perl guard is `$secs < 0.25001 and $secs > 0`; zero fails the
        // second half and falls through to the decimal branch.
        assert_eq!(print_exposure_time(0.0), "0");
    }

    #[test]
    fn f_number_rounds_by_magnitude() {
        assert_eq!(print_f_number(2.8), "2.8");
        assert_eq!(print_f_number(0.95), "0.95");
        assert_eq!(print_f_number(1.0), "1.0");
    }

    #[test]
    fn f_number_non_positive_passes_through() {
        assert_eq!(print_f_number(0.0), "0");
        assert_eq!(print_f_number(-1.0), "-1");
    }

    #[test]
    fn gps_dms_basic_north_east() {
        assert_eq!(gps_to_dms(45.5, Some('N')), "45 deg 30' 0.00\" N");
        assert_eq!(gps_to_dms(122.25, Some('E')), "122 deg 15' 0.00\" E");
    }

    #[test]
    fn gps_dms_negative_flips_reference_and_sign() {
        assert_eq!(gps_to_dms(-45.5, Some('N')), "45 deg 30' 0.00\" S");
        assert_eq!(gps_to_dms(-122.25, Some('E')), "122 deg 15' 0.00\" W");
    }

    #[test]
    fn gps_dms_no_ref_drops_sign_and_suffix() {
        assert_eq!(gps_to_dms(45.5, None), "45 deg 30' 0.00\"");
        assert_eq!(gps_to_dms(-45.5, None), "45 deg 30' 0.00\"");
    }

    #[test]
    fn gps_dms_seconds_round_up_carries_into_minutes() {
        // 1 deg 2' 59.999" rounds to "60.00" at 2dp and must carry.
        let val = 1.0 + 2.0 / 60.0 + 59.999 / 3600.0;
        assert_eq!(gps_to_dms(val, Some('N')), "1 deg 3' 0.00\" N");
    }

    #[test]
    fn decode_ucs2_little_endian_ascii_range() {
        // "Hi" as UCS2LE: 0x48 0x00, 0x69 0x00.
        let bytes = [0x48, 0x00, 0x69, 0x00];
        assert_eq!(decode_ucs2(&bytes, true), "Hi");
    }

    #[test]
    fn decode_ucs2_big_endian() {
        let bytes = [0x00, 0x48, 0x00, 0x69];
        assert_eq!(decode_ucs2(&bytes, false), "Hi");
    }

    #[test]
    fn tr_translate_single_char_map() {
        assert_eq!(tr_translate("2024 01 02", " ", ".", false), "2024.01.02");
        assert_eq!(tr_translate("2024-01-02", "-", ":", false), "2024:01:02");
    }

    #[test]
    fn tr_translate_delete_flag_drops_unmapped_source_chars() {
        assert_eq!(tr_translate("a\0b\0c", "\0", "", true), "abc");
    }

    #[test]
    fn tr_translate_leaves_unmatched_characters_alone() {
        assert_eq!(tr_translate("abc", "x", "y", false), "abc");
    }
}
