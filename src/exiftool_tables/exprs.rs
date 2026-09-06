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

/// `Image::ExifTool::CanonCustom::ConvertPfn($val)`.
///
/// ExifTool (`CanonCustom.pm:2624-2628`, pinned 13.59):
/// ```perl
/// sub ConvertPfn($)
/// {
///     my $val = shift;
///     return $val ? ($val==1 ? 'On' : "On ($val)") : "Off";
/// }
/// ```
/// The `PrintConv` half of the `%convPFn` pair (`CanonCustom.pm:36`) that
/// every one of `CanonCustom::PersonalFuncs`' 29 fields carries. A pure
/// function of `$val`: no ExifTool object, no option, no other tag. Perl's
/// truthiness test on a number is `!= 0`, which is what the first branch is.
#[must_use]
pub fn convert_pfn(val: f64) -> String {
    if val == 0.0 {
        "Off".to_string()
    } else if val == 1.0 {
        "On".to_string()
    } else {
        format!("On ({})", perl_num(val))
    }
}

/// `Image::ExifTool::ConvertFileSize($val)`, the default `ByteUnit` branch.
///
/// ExifTool (`ExifTool.pm:6851-6871`, pinned 13.59) selects between a binary
/// and an SI branch on `$$et{OPTIONS}{ByteUnit}`; `ByteUnit` defaults to
/// `'SI'` (`ExifTool.pm:1115`) and nothing in this pipeline ever sets it, so
/// the SI branch is the whole function here:
/// ```perl
/// $val < 2000 and return "$val bytes";
/// $val < 10000 and return sprintf('%.1f kB', $val / 1000);
/// $val < 2000000 and return sprintf('%.0f kB', $val / 1000);
/// $val < 10000000 and return sprintf('%.1f MB', $val / 1000000);
/// $val < 2000000000 and return sprintf('%.0f MB', $val / 1000000);
/// $val < 10000000000 and return sprintf('%.1f GB', $val / 1000000000);
/// return sprintf('%.0f GB', $val / 1000000000);
/// ```
/// The `Binary` branch is deliberately absent rather than approximated: it is
/// reachable only through an API option this crate never passes, and a
/// half-implemented option is worse than an unimplemented one.
///
/// [`crate::core::value_formatter::format_file_size`] is the same conversion
/// over a `u64` (the `File:FileSize` call site) and delegates here, so the
/// two cannot drift.
#[must_use]
pub fn convert_file_size(val: f64) -> String {
    if val < 2000.0 {
        // `"$val bytes"` is Perl string interpolation, not a `sprintf` --
        // it goes through `%.15g`, which is what `perl_num` reproduces.
        format!("{} bytes", perl_num(val))
    } else if val < 10_000.0 {
        format!("{:.1} kB", val / 1_000.0)
    } else if val < 2_000_000.0 {
        format!("{:.0} kB", val / 1_000.0)
    } else if val < 10_000_000.0 {
        format!("{:.1} MB", val / 1_000_000.0)
    } else if val < 2_000_000_000.0 {
        format!("{:.0} MB", val / 1_000_000.0)
    } else if val < 10_000_000_000.0 {
        format!("{:.1} GB", val / 1_000_000_000.0)
    } else {
        format!("{:.0} GB", val / 1_000_000_000.0)
    }
}

/// `Image::ExifTool::Nikon::PrintAFPointsLeftRight($col, $ncol)`.
///
/// ExifTool (`Nikon.pm:13420-13428`, pinned 13.59):
/// ```perl
/// sub PrintAFPointsLeftRight($$)
/// {
///     my ($col, $ncol) = @_;
///     my $center = ($ncol + 1) / 2;
///     return 'n/a' if $col == 0;   #out of focus
///     return 'C' if $col == $center;
///     return sprintf('%d', $center - $col) . 'L of Center' if $col < $center;
///     return sprintf('%d', $col - $center) . 'R of Center' if $col > $center;
/// }
/// ```
/// `ncol` is a literal at every call site (19, 21 or 29 in the pinned tree),
/// so with it fixed this is a pure function of the value.
///
/// The Perl falls off the end -- returning `undef` -- when none of the four
/// guards fire, which for a numeric `col` is only possible at NaN.
/// `ProcessBinaryData` reads this field as an integer, so NaN cannot reach
/// here and the final branch is `$col > $center`, not a catch-all standing in
/// for one.
#[must_use]
pub fn print_af_points_left_right(col: f64, ncol: f64) -> String {
    let center = (ncol + 1.0) / 2.0;
    if col == 0.0 {
        "n/a".to_string()
    } else if col == center {
        "C".to_string()
    } else if col < center {
        format!("{}L of Center", perl_int(center - col))
    } else {
        format!("{}R of Center", perl_int(col - center))
    }
}

/// `Image::ExifTool::Nikon::PrintAFPointsUpDown($row, $nrow)`.
///
/// ExifTool (`Nikon.pm:13434-13442`, pinned 13.59):
/// ```perl
/// sub PrintAFPointsUpDown($$)
/// {
///     my ($row, $nrow) = @_;
///     my $center = ($nrow + 1) / 2;
///     return 'n/a' if $row == 0;     #out of focus
///     return 'C' if $row == $center;
///     return sprintf('%d', $center - $row) . 'U from Center' if $row < $center;
///     return sprintf('%d', $row - $center) . 'D from Center' if $row > $center;
/// }
/// ```
/// Same shape, and the same NaN note, as
/// [`print_af_points_left_right`]; `nrow` is 11, 13 or 17 at the pinned
/// tree's call sites.
#[must_use]
pub fn print_af_points_up_down(row: f64, nrow: f64) -> String {
    let center = (nrow + 1.0) / 2.0;
    if row == 0.0 {
        "n/a".to_string()
    } else if row == center {
        "C".to_string()
    } else if row < center {
        format!("{}U from Center", perl_int(center - row))
    } else {
        format!("{}D from Center", perl_int(row - center))
    }
}

// ---------------------------------------------------------------------------
// Named helpers that already have a shared port under `crate::core::formatters`.
//
// These four are the compiler's entry points for `ConvertDuration($val)`,
// `ConvertBitrate($val)`, `Image::ExifTool::Exif::PrintFraction($val)` and
// `Image::ExifTool::Canon::CanonEv($val)`. Three of them delegate to the
// existing shared formatter rather than carrying a second copy of the logic
// -- the same arrangement `convert_file_size` has with
// `core::value_formatter::format_file_size`, and for the same reason: two
// ports of one Perl sub drift apart, and the drift is silent. The generated
// tables and `verify_exprs.py`'s harness both reach these functions only by
// this module's path, so the oracle run that gates every translation is
// checking the shared port, not a stand-in for it.
//
// `canon_ev` is the exception: the pre-existing `canon.rs::canon_ev` took an
// `i32`, and the Perl keeps the fractional part of a non-integer input
// (`12.7 & 0x1f` is 12, but `$val -= $frac` leaves 0.7 behind) -- see the
// function's own doc. So the faithful f64 port lives here and `canon.rs`
// delegates to it, not the other way round.
// ---------------------------------------------------------------------------

/// `Image::ExifTool::ConvertDuration($val)` (ExifTool.pm:6877-6895, pinned
/// 13.59). Delegates to [`crate::core::formatters::duration::convert_duration`],
/// which carries the Perl and its `$h > 24` (not `>= 24`) note.
#[must_use]
pub fn convert_duration(secs: f64) -> String {
    crate::core::formatters::duration::convert_duration(secs)
}

/// `Image::ExifTool::ConvertBitrate($val)` (ExifTool.pm:6902-6913, pinned
/// 13.59). Delegates to [`crate::core::formatters::bitrate::convert_bitrate`].
///
/// The `%.3g` inside it is C's, exponent form included: real Perl prints
/// `ConvertBitrate(-1000)` as `-1e+03 bps` (a negative never scales past
/// `bps`, and `%.3g` of -1000 switches to `%e` form) and `1e-6` as
/// `1e-06 bps`. A `%g` that only ever emits fixed notation agrees with the
/// oracle on every realistic bitrate and disagrees on exactly those probes.
#[must_use]
pub fn convert_bitrate(bps: f64) -> String {
    crate::core::formatters::bitrate::convert_bitrate(bps)
}

/// `Image::ExifTool::Exif::PrintFraction($val)` (Exif.pm:5516-5535, pinned
/// 13.59). Delegates to
/// [`crate::core::formatters::exif_print_conv::print_fraction`].
#[must_use]
pub fn print_fraction(val: f64) -> String {
    crate::core::formatters::exif_print_conv::print_fraction(val)
}

/// `Image::ExifTool::Canon::CanonEv($val)` (Canon.pm:10650-10670, pinned
/// 13.59).
///
/// ```perl
/// my $sign;
/// if ($val < 0) { $val = -$val; $sign = -1; } else { $sign = 1; }
/// my $frac = $val & 0x1f;
/// $val -= $frac;      # remove fraction
/// if ($frac == 0x0c) { $frac = 0x20 / 3; }      # 1/3 stop
/// elsif ($frac == 0x14) { $frac = 0x40 / 3; }   # 2/3 stop
/// return $sign * ($val + $frac) / 0x20;
/// ```
///
/// Two Perl details decide the shape of this port:
///
/// - `&` converts its operand to an integer first (truncation toward zero),
///   so `12.7 & 0x1f` is 12 -- but `$val -= $frac` then subtracts that integer
///   from the *original* NV, which keeps its fraction. So `CanonEv(12.7)` is
///   `(0.7 + 32/3) / 32 = 0.35520833`, not `CanonEv(12) = 0.33333333`. Every
///   binary-table field that reaches this is an integer, so the distinction
///   never shows in real output; it shows on `verify_exprs.py`'s fractional
///   probes, which is where an `i32` port was caught.
/// - The sign is stripped before the mask and restored after, so the low five
///   bits are always read off the magnitude: `CanonEv(-20)` is `-(64/3)/32 =
///   -0.66666667`, not the two's-complement mask of -20.
///
/// The magnitude is truncated `as u64` for the mask. That saturates above
/// `u64::MAX`, a magnitude no Canon field can encode and none of the
/// composed call sites (`$val-24`, `$val*4-32`, `$val*4`) can reach from an
/// integer field; it is stated rather than guarded so the function stays a
/// pure port of the seven Perl lines above.
#[must_use]
pub fn canon_ev(val: f64) -> f64 {
    let (sign, mag) = if val < 0.0 { (-1.0, -val) } else { (1.0, val) };
    let raw_frac = ((mag as u64) & 0x1f) as f64;
    let whole = mag - raw_frac;
    let frac = if raw_frac == 12.0 {
        32.0 / 3.0
    } else if raw_frac == 20.0 {
        64.0 / 3.0
    } else {
        raw_frac
    };
    sign * (whole + frac) / 32.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ConvertPfn` (CanonCustom.pm:2624-2628). Every value that is neither
    /// 0 nor 1 goes through Perl string interpolation, so the digits are
    /// `perl_num`'s, not Rust `Display`'s -- the `1e18` row is that
    /// distinction, and it is the one a hand-written `format!("{}", v)` gets
    /// wrong.
    #[test]
    fn convert_pfn_matches_exiftool() {
        assert_eq!(convert_pfn(0.0), "Off");
        assert_eq!(convert_pfn(1.0), "On");
        assert_eq!(convert_pfn(2.0), "On (2)");
        assert_eq!(convert_pfn(255.0), "On (255)");
        assert_eq!(convert_pfn(-1.0), "On (-1)");
        assert_eq!(convert_pfn(1e18), "On (1e+18)");
    }

    /// `ConvertFileSize` (ExifTool.pm:6851-6871, SI branch). The boundaries
    /// are the ones the Perl states, and the point of the 1500/1_000_000 rows
    /// is that the unit does NOT change at each power of 1000.
    #[test]
    fn convert_file_size_matches_exiftool() {
        assert_eq!(convert_file_size(500.0), "500 bytes");
        assert_eq!(convert_file_size(1999.0), "1999 bytes");
        assert_eq!(convert_file_size(2000.0), "2.0 kB");
        assert_eq!(convert_file_size(9999.0), "10.0 kB");
        assert_eq!(convert_file_size(10_000.0), "10 kB");
        assert_eq!(convert_file_size(1_000_000.0), "1000 kB");
        // Palm.mobi's real UncompressedTextLength -- see
        // `exiftool_tables::tests::recovered_conversions_match_the_pinned_
        // oracle_on_real_carriers` for the oracle run this came from.
        assert_eq!(convert_file_size(171_966.0), "172 kB");
        assert_eq!(convert_file_size(2_500_000_000.0), "2.5 GB");
    }

    /// The `u64` file-size formatter and the `f64` `PrintConv` one must be
    /// the same function, because they are: `format_file_size` delegates
    /// here. Asserted rather than assumed, so a future edit that re-inlines
    /// the branch chain into `value_formatter` fails a test instead of
    /// quietly forking the port in two.
    #[test]
    fn format_file_size_is_this_same_conversion() {
        for bytes in [
            0u64,
            1,
            500,
            1999,
            2000,
            9999,
            10_000,
            999_999,
            1_000_000,
            171_966,
            2_000_000,
            9_999_999,
            10_000_000,
            2_000_000_000,
            10_000_000_000,
        ] {
            assert_eq!(
                crate::core::value_formatter::format_file_size(bytes),
                convert_file_size(bytes as f64),
                "file size {bytes}",
            );
        }
    }

    /// `PrintAFPointsLeftRight`/`PrintAFPointsUpDown` (Nikon.pm:13420-13428,
    /// :13434-13442). The values are the ones the pinned oracle reports for
    /// real Z-series carriers; `ncol`/`nrow` is the literal the matching
    /// `_variants` alternative passes.
    #[test]
    fn print_af_points_relative_matches_exiftool() {
        // NikonZ7_2.jpg: Z 7_2 -> 29 columns, 17 rows.
        assert_eq!(print_af_points_left_right(16.0, 29.0), "1R of Center");
        assert_eq!(print_af_points_up_down(12.0, 17.0), "3D from Center");
        // NikonZ6_2.jpg: Z 6_2 -> 21 columns, 13 rows.
        assert_eq!(print_af_points_left_right(5.0, 21.0), "6L of Center");
        assert_eq!(print_af_points_up_down(10.0, 13.0), "3D from Center");
        // NikonZ30.jpg: Z 30 -> 19 columns, 11 rows; both dead centre.
        assert_eq!(print_af_points_left_right(10.0, 19.0), "C");
        assert_eq!(print_af_points_up_down(6.0, 11.0), "C");
        // `$col == 0` is ExifTool's out-of-focus marker, not a column.
        assert_eq!(print_af_points_left_right(0.0, 19.0), "n/a");
        assert_eq!(print_af_points_up_down(0.0, 11.0), "n/a");
    }

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

    // The four values below each named helper are quoted from the pinned
    // 13.59 Perl itself (`perl -I<pinned lib> -MImage::ExifTool
    // -MImage::ExifTool::Exif -MImage::ExifTool::Canon -e ...`), not from a
    // reading of the sub. verify_exprs.py's oracle run is the authority;
    // these keep `cargo test` red on a regression when Perl is not to hand.

    /// `ConvertDuration` (ExifTool.pm:6877-6895): the `< 30` decimal branch,
    /// the `+= 0.5` round-to-nearest-second (59.5 -> 0:01:00), the `$h > 24`
    /// (not `>= 24`) day split, and the sign.
    #[test]
    fn convert_duration_matches_exiftool() {
        assert_eq!(convert_duration(0.0), "0 s");
        assert_eq!(convert_duration(29.999), "30.00 s");
        assert_eq!(convert_duration(30.0), "0:00:30");
        assert_eq!(convert_duration(59.5), "0:01:00");
        assert_eq!(convert_duration(3600.0), "1:00:00");
        assert_eq!(convert_duration(86400.0), "24:00:00");
        assert_eq!(convert_duration(90000.0), "1 days 1:00:00");
        assert_eq!(convert_duration(-3600.0), "-1:00:00");
        assert_eq!(convert_duration(1e-6), "0.00 s");
        assert_eq!(convert_duration(1e18), "11574074074074 days 1:46:56");
    }

    /// `ConvertBitrate` (ExifTool.pm:6902-6913), including the two `%.3g`
    /// exponent-form tails a fixed-notation `%g` gets wrong.
    #[test]
    fn convert_bitrate_matches_exiftool() {
        assert_eq!(convert_bitrate(0.0), "0 bps");
        assert_eq!(convert_bitrate(999.0), "999 bps");
        assert_eq!(convert_bitrate(1000.0), "1 kbps");
        assert_eq!(convert_bitrate(99_999.0), "100 kbps");
        assert_eq!(convert_bitrate(1e6), "1 Mbps");
        assert_eq!(convert_bitrate(1e-6), "1e-06 bps");
        assert_eq!(convert_bitrate(1e18), "1000000000 Gbps");
        assert_eq!(convert_bitrate(-1000.0), "-1e+03 bps");
    }

    /// `PrintFraction` (Exif.pm:5516-5535): the four branches, and the
    /// `%+d` of a huge value -- Perl prints the IV of the `* 1.00001`'d
    /// double, `+1000010000000000128`, and so does this.
    #[test]
    fn print_fraction_matches_exiftool() {
        assert_eq!(print_fraction(0.0), "0");
        assert_eq!(print_fraction(0.5), "+1/2");
        assert_eq!(print_fraction(1.0 / 3.0), "+1/3");
        assert_eq!(print_fraction(1.0), "+1");
        assert_eq!(print_fraction(-2.0), "-2");
        assert_eq!(print_fraction(1.326429536), "+1.33");
        assert_eq!(print_fraction(-0.7), "-0.7");
        assert_eq!(print_fraction(1e-6), "+1e-06");
        assert_eq!(print_fraction(1e18), "+1000010000000000128");
    }

    /// `CanonEv` (Canon.pm:10650-10670). The 1/3 and 2/3 stop codes, the
    /// sign, and -- the row that decides the port's type -- a fractional
    /// input: Perl's `$val -= ($val & 0x1f)` keeps the NV's fraction, so
    /// `CanonEv(12.7)` is `(0.7 + 32/3) / 32`, not `CanonEv(12)`.
    #[test]
    fn canon_ev_matches_exiftool() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
        assert_eq!(canon_ev(0.0), 0.0);
        assert!(close(canon_ev(0x0c as f64), 0.333_333_333_333_333_3));
        assert!(close(canon_ev(0x14 as f64), 0.666_666_666_666_666_7));
        assert_eq!(canon_ev(0x20 as f64), 1.0);
        assert!(close(canon_ev(-(0x14 as f64)), -0.666_666_666_666_666_7));
        assert_eq!(canon_ev(-32.0), -1.0);
        assert!(close(canon_ev(44.0), 1.333_333_333_333_333_3));
        assert!(close(canon_ev(12.7), 0.355_208_333_333_333_3));
        assert_eq!(canon_ev(200.0), 6.25);
    }
}
