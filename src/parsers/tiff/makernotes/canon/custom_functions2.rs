//! `%Image::ExifTool::CanonCustom::Functions2` (CanonCustom.pm:1198) -- the custom
//! function block every body from the EOS-1D Mark III on writes, under MakerNote tag
//! 0x0099.
//!
//! ExifTool models 26 of the 100 tag ids as an **ARRAY of `Condition` arms** rather
//! than a single hash. `GetTagInfo` (ExifTool.pm:9133) walks the arms in order and
//! takes the first whose `Condition` passes; `if ($condition)` means an arm with no
//! `Condition` passes unconditionally and therefore terminates the list. The arms are
//! selected on two things and two things only:
//!
//! * `$$self{Model}` -- a word-bounded alternation over body names, and
//! * `$count` -- the element count `ProcessCanonCustom2` read for that entry.
//!
//! Both are known here, so nothing about the dispatch is a guess: the model comes from
//! IFD0 (or `CanonImageType`) and the count is in the record. The 26 arrays were
//! previously dropped outright, which is why `ExposureLevelIncrements` was missing on
//! 110 corpus files and `ISOExpansion` on 68.
//!
//! The table in [`custom_functions2_tables`] is transcribed by
//! `scripts/gen_canon_custom_functions2.pl` from ExifTool's in-memory Perl hash, and
//! the generator hard-errors on any `Condition`, `ValueConv` or `PrintConv` construct
//! it has not seen rather than skipping the entry.
//!
//! Arm selection and rendering were checked against ExifTool's own `GetTagInfo` and
//! `GetValue` driving this same Perl table, over 118,700 probes covering all 100 ids,
//! 31 body names and value lists of 1 to 102 elements. All agree except for stored
//! numbers of 999 and above in one of the five exponential `ValueConv` slots, where
//! the result is 1e33 or larger: there `exp()` differs from Perl's libm in the last
//! ULP, and Perl's `sprintf("1/%d", ...)` wraps an overflowing IV to -1 where Rust
//! saturates. No Canon body stores a number in that range in those slots -- the
//! corpus has none -- and ExifTool labels the ISO decoding "an educated guess".
//!
//! [`custom_functions2_tables`]: super::custom_functions2_tables

use std::collections::HashMap;

use super::custom_functions2_tables::CUSTOM_FUNCTIONS2;
use super::print_exposure_time;
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// One `%CanonCustom::Functions2` tag id and its `Condition` arms, in ExifTool's order.
pub(super) struct Cf2Entry {
    pub tag: u32,
    pub arms: &'static [Cf2Arm],
}

/// One arm of a `Cf2Entry`: the tag ExifTool reports when this arm's condition passes.
pub(super) struct Cf2Arm {
    pub name: &'static str,
    pub cond: Cf2Cond,
    /// ExifTool's `ValueConv`, one slot per value. Empty when the entry has none.
    pub value_convs: &'static [Cf2ValueConv],
    pub print_conv: Cf2Print,
}

/// The dispatch condition on one arm.
///
/// `Model` alternations use `\b...\b` in Perl, so a match must sit on word boundaries:
/// `\b1D\b` picks the EOS-1D but not the EOS-1DS, and `\b1D X\b` does not match
/// "1D X Mark III"... except that it does, because the right boundary falls between
/// "X" and the following space.
pub(super) enum Cf2Cond {
    /// No `Condition`. Always passes, so it ends the arm list.
    Always,
    /// `$count == N`, where N is the entry's element count in the record.
    Count(usize),
    /// `$$self{Model} =~ /\b(a|b|...)\b/`. Two `=~` alternations joined by Perl's `or`
    /// are folded into one list here -- the union is exactly equivalent.
    Model(&'static [&'static str]),
    /// `$$self{Model} !~ /\b(a|b|...)\b/`.
    ModelNot(&'static [&'static str]),
    /// `$$self{Model} =~ /\bX/` -- left word boundary only, no right one. ExifTool
    /// writes exactly one of these (`/\b1D/` on `ISOSpeedRange`), and it deliberately
    /// matches the whole 1D family including "1Ds" and "1D X".
    ModelPrefix(&'static str),
}

/// ExifTool's `PrintConv` for one arm.
pub(super) enum Cf2Print {
    /// No `PrintConv`: the value prints as its raw space-joined list.
    None,
    /// A single conversion, applied to the whole (possibly space-joined) value --
    /// which is why `AEBShotCount`'s two-value arm is keyed `"3 0"` and not `3`.
    Scalar(Cf2PrintConv),
    /// A Perl ARRAY `PrintConv`: slot `i` converts value `i`, values past the end of
    /// the list print unconverted, and the results join with "; " (ExifTool.pm:3673).
    List(&'static [Cf2PrintConv]),
}

/// One `PrintConv` slot.
pub(super) enum Cf2PrintConv {
    /// No conversion for this slot.
    None,
    /// A lookup hash, keyed on the value's string form. A miss prints `Unknown ($val)`.
    Map(&'static [(&'static str, &'static str)]),
    /// A `BITMASK` hash, rendered by ExifTool's `DecodeBits`: set bit names joined with
    /// ", ", an unnamed set bit as `[n]`, and "(none)" when no bit is set.
    Bitmask(&'static [(u32, &'static str)]),
    /// `sprintf("Flags 0x%x",$val)`.
    Flags,
    /// `sprintf("<prefix>%.0f",$val)`.
    Round0(&'static str),
    /// `"<prefix>" . Image::ExifTool::Exif::PrintExposureTime($val)`.
    ExposureTime(&'static str),
    /// `sprintf("<prefix>%.2g",$val)`.
    TwoSigFigs(&'static str),
    /// `"<prefix>$val<suffix>"`.
    Interpolate(&'static str, &'static str),
}

/// One `ValueConv` slot. ExifTool writes five distinct expressions in this table.
pub(super) enum Cf2ValueConv {
    /// No conversion: the slot keeps its stored integer.
    None,
    /// `$val < 2 ? $val : ($val < 1000 ? exp(($val/8-9)*log(2))*100 : 0)`
    Iso,
    /// `exp(-($val/8-7)*log(2))`
    ShutterSpeedStops,
    /// `exp(-$val/(1600*log(2)))`
    ShutterSpeedLinear,
    /// `exp(($val/8-1)*log(2)/2)`
    ApertureStops,
    /// `exp($val/2400)`
    ApertureLinear,
}

/// A value after `ValueConv`: either the stored integer or a converted float.
#[derive(Clone, Copy)]
enum Cf2Value {
    Int(i32),
    Float(f64),
}

impl Cf2Value {
    fn as_f64(self) -> f64 {
        match self {
            Cf2Value::Int(v) => f64::from(v),
            Cf2Value::Float(v) => v,
        }
    }

    /// The way Perl stringifies the value when ExifTool joins the list back together.
    fn render_raw(self) -> String {
        match self {
            Cf2Value::Int(v) => v.to_string(),
            Cf2Value::Float(v) => format_perl_number(v),
        }
    }
}

/// Perl's default numeric stringification: up to 15 significant digits, trailing zeros
/// dropped, and an integral value printed without a decimal point.
fn format_perl_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    let mut rendered = format!("{:.*e}", 14, value);
    // Convert Rust's `1.5e-3` exponent form back to Perl's plain-decimal rendering for
    // the magnitudes this table produces (shutter speeds and apertures).
    if let Some((mantissa, exp)) = rendered.split_once('e') {
        let exp: i32 = exp.parse().unwrap_or(0);
        if (-6..15).contains(&exp) {
            let decimals = (14 - exp).clamp(0, 17) as usize;
            rendered = format!("{:.*}", decimals, value);
            let trimmed = rendered.trim_end_matches('0').trim_end_matches('.');
            return trimmed.to_string();
        }
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!(
            "{}e{}{:02}",
            mantissa,
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        );
    }
    rendered
}

/// C's `%.<precision>g`, which is what Perl's `sprintf` calls.
///
/// Deliberately not `super::format_g2`: that one never switches to exponential
/// notation, on the grounds that an aperture is never shown as `1.3e+02`. Here the
/// `Closed`/`Open` slots of `ApertureRange` run over a raw stored number that is not
/// bounded like an aperture, and ExifTool prints `sprintf("Closed %.2g", 724.08)` as
/// `Closed 7.2e+02`. Verified against `%CanonCustom::Functions2` driven through
/// ExifTool's own `GetValue` over 89,900 probes.
///
/// C's rule: with `X` the decimal exponent of the value rounded to `precision`
/// significant digits, use `%f` style when `precision > X >= -4` and `%e` style
/// otherwise, then strip trailing fractional zeros and a trailing point.
fn format_sprintf_g(value: f64, precision: usize) -> String {
    fn strip(s: String) -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    }
    let p = precision.max(1);
    // Formatting to `p - 1` fractional digits in scientific notation rounds first, so
    // the exponent read back is the one C would use (9.99 at %.2g becomes 1.0e+01).
    let scientific = format!("{:.*e}", p - 1, value);
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific;
    };
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if exponent >= -4 && exponent < p as i32 {
        let decimals = (p as i32 - 1 - exponent).max(0) as usize;
        return strip(format!("{:.*}", decimals, value));
    }
    format!(
        "{}e{}{:02}",
        strip(mantissa.to_string()),
        if exponent < 0 { '-' } else { '+' },
        exponent.abs()
    )
}

impl Cf2ValueConv {
    fn apply(&self, raw: i32) -> Cf2Value {
        let val = f64::from(raw);
        match self {
            Cf2ValueConv::None => Cf2Value::Int(raw),
            // '$val < 2 ? $val : ($val < 1000 ? exp(($val/8-9)*log(2))*100 : 0)'
            Cf2ValueConv::Iso => {
                if val < 2.0 {
                    Cf2Value::Int(raw)
                } else if val < 1000.0 {
                    Cf2Value::Float(((val / 8.0 - 9.0) * std::f64::consts::LN_2).exp() * 100.0)
                } else {
                    Cf2Value::Int(0)
                }
            }
            // 'exp(-($val/8-7)*log(2))'
            Cf2ValueConv::ShutterSpeedStops => {
                Cf2Value::Float((-(val / 8.0 - 7.0) * std::f64::consts::LN_2).exp())
            }
            // 'exp(-$val/(1600*log(2)))'
            Cf2ValueConv::ShutterSpeedLinear => {
                Cf2Value::Float((-val / (1600.0 * std::f64::consts::LN_2)).exp())
            }
            // 'exp(($val/8-1)*log(2)/2)'
            Cf2ValueConv::ApertureStops => {
                Cf2Value::Float(((val / 8.0 - 1.0) * std::f64::consts::LN_2 / 2.0).exp())
            }
            // 'exp($val/2400)'
            Cf2ValueConv::ApertureLinear => Cf2Value::Float((val / 2400.0).exp()),
        }
    }
}

/// ExifTool's `DecodeBits` (ExifTool.pm:6180) with the default 32 bits per word.
///
/// ```text
///     foreach $val (split ' ', $vals) {
///         for ($i=0; $i<$bits; ++$i) {
///             next unless $val & (1 << $i);
///             my $n = $i + $num;
///             ... push @bitList, $$lookup{$n} // "[$n]";
///         }
///         $num += $bits;
///     }
///     return '(none)' unless @bitList;
/// ```
///
/// Note the `[$n]` for a set bit the hash does not name -- dropping it silently would
/// under-report the value.
fn decode_bits(table: &[(u32, &str)], values: &[Cf2Value]) -> String {
    let mut names: Vec<String> = Vec::new();
    for (word, value) in values.iter().enumerate() {
        let bits = value.as_f64() as i64 as u32;
        for bit in 0..32u32 {
            if bits & (1 << bit) == 0 {
                continue;
            }
            let n = bit + 32 * word as u32;
            match table.iter().find(|(b, _)| *b == n) {
                Some((_, name)) => names.push((*name).to_string()),
                None => names.push(format!("[{}]", n)),
            }
        }
    }
    if names.is_empty() {
        "(none)".to_string()
    } else {
        names.join(", ")
    }
}

impl Cf2PrintConv {
    /// Renders one slot. `joined` is the value's string form, which for a scalar
    /// `PrintConv` over a multi-element record is the whole space-joined list.
    fn render(&self, value: Cf2Value, joined: &str, all: &[Cf2Value]) -> String {
        match self {
            Cf2PrintConv::None => joined.to_string(),
            Cf2PrintConv::Map(table) => table
                .iter()
                .find(|(key, _)| *key == joined)
                .map(|(_, label)| (*label).to_string())
                .unwrap_or_else(|| format!("Unknown ({})", joined)),
            Cf2PrintConv::Bitmask(table) => decode_bits(table, all),
            Cf2PrintConv::Flags => format!("Flags 0x{:x}", value.as_f64() as i64),
            Cf2PrintConv::Round0(prefix) => format!("{}{:.0}", prefix, value.as_f64()),
            Cf2PrintConv::ExposureTime(prefix) => {
                format!("{}{}", prefix, print_exposure_time(value.as_f64()))
            }
            Cf2PrintConv::TwoSigFigs(prefix) => {
                format!("{}{}", prefix, format_sprintf_g(value.as_f64(), 2))
            }
            Cf2PrintConv::Interpolate(prefix, suffix) => {
                format!("{}{}{}", prefix, value.render_raw(), suffix)
            }
        }
    }
}

impl Cf2Cond {
    fn matches(&self, model: &str, count: usize) -> bool {
        match self {
            Cf2Cond::Always => true,
            Cf2Cond::Count(n) => *n == count,
            Cf2Cond::Model(alts) => alts.iter().any(|alt| word_bounded_match(model, alt)),
            Cf2Cond::ModelNot(alts) => !alts.iter().any(|alt| word_bounded_match(model, alt)),
            Cf2Cond::ModelPrefix(alt) => left_bounded_match(model, alt),
        }
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// True when `needle` occurs in `haystack` at a position where Perl's `\b` holds on
/// both sides. Every alternative in this table starts and ends with a word character
/// (the generator asserts it), so the boundary test reduces to "the neighbouring byte
/// is absent or is not a word byte".
fn word_bounded_match(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    (0..=h.len() - n.len()).any(|i| {
        &h[i..i + n.len()] == n
            && (i == 0 || !is_word_byte(h[i - 1]))
            && (i + n.len() == h.len() || !is_word_byte(h[i + n.len()]))
    })
}

/// True when `needle` occurs at a left word boundary, with no constraint on its right.
/// This is ExifTool's `/\b1D/`, which is what pulls the whole 1D family -- 1D, 1Ds,
/// 1D X, 1DS -- onto `ISOSpeedRange`.
fn left_bounded_match(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    (0..=h.len() - n.len()).any(|i| &h[i..i + n.len()] == n && (i == 0 || !is_word_byte(h[i - 1])))
}

/// Renders one entry the way ExifTool's `GetValue` would, given the arm that matched.
///
/// ExifTool runs `ValueConv` then `PrintConv` over the same list. Each is applied slot
/// for slot, values past the end of a conversion list stay unconverted, and the results
/// join with " " after `ValueConv` and "; " after `PrintConv` (ExifTool.pm:3673).
fn render_arm(arm: &Cf2Arm, raw: &[i32]) -> Option<String> {
    if raw.is_empty() {
        return None;
    }

    let converted: Vec<Cf2Value> = raw
        .iter()
        .enumerate()
        .map(|(i, &v)| match arm.value_convs.get(i) {
            Some(conv) => conv.apply(v),
            None => Cf2Value::Int(v),
        })
        .collect();

    match &arm.print_conv {
        Cf2Print::None => Some(
            converted
                .iter()
                .map(|v| v.render_raw())
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Cf2Print::Scalar(conv) => {
            let joined = converted
                .iter()
                .map(|v| v.render_raw())
                .collect::<Vec<_>>()
                .join(" ");
            Some(conv.render(converted[0], &joined, &converted))
        }
        Cf2Print::List(convs) => Some(
            converted
                .iter()
                .enumerate()
                .map(|(i, value)| {
                    let joined = value.render_raw();
                    match convs.get(i) {
                        Some(conv) => conv.render(*value, &joined, std::slice::from_ref(value)),
                        // Past the end of the PrintConv list ExifTool leaves the value
                        // alone: `$conv = $$convList[$i]` is undef, so `$value = $val`.
                        None => joined,
                    }
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
    }
}

/// Picks the arm ExifTool would pick and renders it, or `None` when no arm claims the
/// body. An unclaimed model is the correct outcome, not a bug: emitting another body's
/// labels would be a fabricated value.
fn render_entry(entry: &Cf2Entry, model: &str, values: &[i32]) -> Option<(&'static str, String)> {
    let arm = entry
        .arms
        .iter()
        .find(|arm| arm.cond.matches(model, values.len()))?;
    render_arm(arm, values).map(|rendered| (arm.name, rendered))
}

/// Reads MakerNote tag 0x0099 the way `CanonCustom::ProcessCanonCustom2`
/// (CanonCustom.pm:2642) does.
///
/// The record is entirely self-describing, so no per-model offset table is involved:
///
/// ```text
///     int16u  len              # must equal the record size, and be >= 8
///     ...
///     int32u  groupCount       # at offset 4
///     # then, from offset 8, a run of groups:
///     int32u  recNum, recLen, recCount
///     # then, within each group, a run of entries:
///     int32u  tag, num
///     int32s  value[num]
/// ```
///
/// Only tags present in [`CUSTOM_FUNCTIONS2`] are emitted; the rest are stepped over,
/// exactly as ExifTool does without `-U`.
pub(super) fn parse_custom_functions2(
    bytes: &[u8],
    byte_order: ByteOrder,
    model: &str,
    tags: &mut HashMap<String, String>,
) {
    let reader = EndianReader::new(bytes, byte_order.to_io_byte_order());
    // "first entry in array must be the size"
    if reader.u16_at(0).map(usize::from) != Some(bytes.len()) || bytes.len() < 8 {
        return;
    }

    let mut pos = 8usize;
    while pos + 12 <= bytes.len() {
        let (Some(record_len), Some(_record_count)) =
            (reader.u32_at(pos + 4), reader.u32_at(pos + 8))
        else {
            return;
        };
        // "must be at least 8 bytes for recNum and recLen"
        let Some(payload_len) = (record_len as usize).checked_sub(8) else {
            return;
        };
        pos += 12;
        let Some(record_end) = pos.checked_add(payload_len) else {
            return;
        };
        if record_end > bytes.len() {
            return; // "Corrupted CanonCustom2 group"
        }

        let mut entry_pos = pos;
        while entry_pos + 8 < record_end {
            let (Some(tag_id), Some(count)) =
                (reader.u32_at(entry_pos), reader.u32_at(entry_pos + 4))
            else {
                return;
            };
            let mut count = count as usize;
            let Some(mut next_entry) = entry_pos
                .checked_add(8)
                .and_then(|p| count.checked_mul(4).and_then(|n| p.checked_add(n)))
            else {
                return;
            };
            if next_entry > record_end {
                break;
            }

            // EOS-1DXmkIII firmware 1.0.0 writes tag 0x70c one element short, which
            // shifts every entry after it in the group. ExifTool patches it at
            // CanonCustom.pm:2690:
            //
            // ```text
            //     if ($tag == 0x70c and $num == 0x66 and $nextRec + 8 < $recEnd) {
            //         my $tmp = Get32u($dataPt, $nextRec + 4);
            //         if ($tmp == 0x70f) {
            //             ++$num; # (count should be one greater)
            //         }
            //     }
            // ```
            //
            // Without it the 1DXmkIII reports `ShortReleaseTimeLag` "Disable" for
            // "Enable" and a 102-element `CustomControls` for a 103-element one.
            if tag_id == 0x70c
                && count == 0x66
                && next_entry + 8 < record_end
                && reader.u32_at(next_entry + 4) == Some(0x70f)
            {
                count += 1;
                next_entry += 4;
            }
            entry_pos += 8;

            if let Some(entry) = CUSTOM_FUNCTIONS2.iter().find(|e| e.tag == tag_id) {
                let values: Vec<i32> = (0..count)
                    .filter_map(|i| reader.i32_at(entry_pos + i * 4))
                    .collect();
                if values.len() == count
                    && let Some((name, rendered)) = render_entry(entry, model, &values)
                {
                    tags.insert(format!("CanonCustom:{}", name), rendered);
                }
            }

            entry_pos = next_entry;
        }
        pos = record_end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(tag: u32, model: &str, values: &[i32]) -> Option<(&'static str, String)> {
        let entry = CUSTOM_FUNCTIONS2.iter().find(|e| e.tag == tag)?;
        render_entry(entry, model, values)
    }

    #[test]
    fn table_covers_every_exiftool_tag_id() {
        // %CanonCustom::Functions2 has 100 numeric keys (ExifTool 13.55).
        assert_eq!(CUSTOM_FUNCTIONS2.len(), 100);
        // 26 of them are ARRAYs of Condition arms.
        let multi = CUSTOM_FUNCTIONS2
            .iter()
            .filter(|e| e.arms.len() > 1)
            .count();
        assert_eq!(multi, 26);
        // Ids are unique and ascending, so `find` cannot shadow an entry.
        for pair in CUSTOM_FUNCTIONS2.windows(2) {
            assert!(pair[0].tag < pair[1].tag);
        }
    }

    #[test]
    fn word_boundaries_follow_perl() {
        // `\b1D\b` matches the 1D but not the 1DS or the 1000D.
        assert!(word_bounded_match("Canon EOS-1D Mark III", "1D"));
        assert!(!word_bounded_match("Canon EOS-1DS MARK II", "1D"));
        assert!(!word_bounded_match("Canon EOS 1000D", "1D"));
        // `\b1D X\b` matches through a trailing "Mark III".
        assert!(word_bounded_match("Canon EOS-1D X Mark III", "1D X"));
        // `/\b1D/` has no right boundary, so it takes the whole 1D family.
        assert!(left_bounded_match("Canon EOS-1DS MARK II", "1D"));
        assert!(!left_bounded_match("Canon EOS 1000D", "1D"));
    }

    /// CanonCustom.pm:1216 -- 0x0101 arm 0 is `$$self{Model} =~ /\b1Ds?\b/`, arm 1 is
    /// unconditioned. The two arms label the same stored number differently, which is
    /// the reason the arm has to be chosen rather than guessed.
    #[test]
    fn exposure_level_increments_dispatches_on_model() {
        assert_eq!(
            render(0x0101, "Canon EOS-1D Mark III", &[0]),
            Some((
                "ExposureLevelIncrements",
                "1/3-stop set, 1/3-stop comp.".to_string()
            ))
        );
        assert_eq!(
            render(0x0101, "Canon EOS 40D", &[0]),
            Some(("ExposureLevelIncrements", "1/3 Stop".to_string()))
        );
    }

    /// CanonCustom.pm:1234 -- the same id is `ISOSpeedRange` on the 1D family and
    /// `ISOExpansion` everywhere else, so the arm decides the tag *name*.
    #[test]
    fn iso_speed_range_arm_changes_the_tag_name() {
        // 112 and 72 are what Canon1DmkIII.jpg stores; ExifTool's own GetValue over
        // this table returns "Disable; Max 3200; Min 100" for them.
        let (name, value) = render(0x0103, "Canon EOS-1D Mark III", &[0, 112, 72]).unwrap();
        assert_eq!(name, "ISOSpeedRange");
        assert_eq!(value, "Disable; Max 3200; Min 100");

        assert_eq!(
            render(0x0103, "Canon EOS 5D Mark II", &[1]),
            Some(("ISOExpansion", "On".to_string()))
        );
    }

    /// CanonCustom.pm:1345 -- the fallback arm's PrintConv is keyed on the *joined*
    /// two-value string, because ExifTool looks up `$val` and `$val` is "3 0".
    #[test]
    fn aeb_shot_count_keys_on_the_joined_value() {
        assert_eq!(
            render(0x0106, "Canon EOS 5D Mark III", &[3, 0]),
            Some(("AEBShotCount", "3 shots".to_string()))
        );
        // $count == 1 takes the middle arm, which is keyed on the bare number.
        assert_eq!(
            render(0x0106, "Canon EOS 5D Mark III", &[1]),
            Some(("AEBShotCount", "2 shots".to_string()))
        );
        // The 90D arm wins over both.
        assert_eq!(
            render(0x0106, "Canon EOS 90D", &[3]),
            Some(("AEBShotCount", "3 shots".to_string()))
        );
    }

    /// CanonCustom.pm:1400 -- `$count` picks between a 3-value and a 4-value layout,
    /// and each has its own ValueConv. Nothing about this is model-conditional.
    #[test]
    fn shutter_speed_range_dispatches_on_count() {
        assert_eq!(
            render(0x010c, "Canon EOS-1D Mark III", &[0, 160, 16]),
            Some(("ShutterSpeedRange", "Disable; Hi 1/8192; Lo 32".to_string()))
        );
        let (name, value) = render(0x010d, "Canon EOS-1D Mark III", &[0, 112, 8]).unwrap();
        assert_eq!(name, "ApertureRange");
        assert_eq!(value, "Disable; Closed 91; Open 1");
    }

    /// ExifTool.pm:3673 -- a value past the end of an ARRAY PrintConv prints
    /// unconverted. `ApplyShootingMeteringMode` has eight values and one converter.
    #[test]
    fn values_past_the_print_conv_list_print_raw() {
        assert_eq!(
            render(
                0x010e,
                "Canon EOS-1D Mark III",
                &[0, 0, 0, 3, 112, 48, 0, 0]
            ),
            Some((
                "ApplyShootingMeteringMode",
                "Disable; 0; 0; 3; 112; 48; 0; 0".to_string()
            ))
        );
    }

    /// ExifTool.pm:6180 -- `DecodeBits` names an unmatched bit `[n]` rather than
    /// dropping it, and prints "(none)" when nothing is set.
    #[test]
    fn bitmask_names_unknown_bits() {
        // 0x040a ViewfinderWarnings names bits 0-4, 6 and 7; bit 5 it does not.
        assert_eq!(
            render(0x040a, "Canon EOS 7D", &[0b0010_0001]).map(|(_, v)| v),
            Some("Monochrome, [5]".to_string())
        );
        assert_eq!(
            render(0x040a, "Canon EOS 7D", &[0]).map(|(_, v)| v),
            Some("(none)".to_string())
        );
    }

    /// An entry with no PrintConv at all prints its raw space-joined list.
    #[test]
    fn entries_without_a_print_conv_print_raw_values() {
        assert_eq!(
            render(0x0518, "Canon EOS 6D", &[1]),
            Some(("AccelerationTracking", "1".to_string()))
        );
    }

    /// CanonCustom.pm:1564 -- 0x0112 has two arms and *neither* carries a Condition,
    /// so ExifTool's own dispatch can never reach the second one. Mirroring that is
    /// the correct behaviour: guessing the EOS R arm would invent a value.
    #[test]
    fn unconditioned_first_arm_shadows_the_second() {
        let entry = CUSTOM_FUNCTIONS2.iter().find(|e| e.tag == 0x0112).unwrap();
        assert_eq!(entry.arms.len(), 2);
        assert!(matches!(entry.arms[0].cond, Cf2Cond::Always));
        assert_eq!(
            render(0x0112, "Canon EOS R", &[2]),
            Some(("SameExposureForNewAperture", "Shutter Speed".to_string()))
        );
    }

    /// No arm claims the body: ExifTool reports nothing, and so do we.
    #[test]
    fn an_unclaimed_model_emits_nothing() {
        // 0x010c only has $count == 3 and $count == 4 arms.
        assert_eq!(render(0x010c, "Canon EOS 7D", &[0, 1]), None);
    }

    /// Perl's `sprintf("%.2g")` switches to exponential notation once the exponent
    /// reaches the precision, which `super::format_g2` deliberately does not do.
    /// Every expectation here came out of ExifTool's own `GetValue` over this table.
    #[test]
    fn two_sig_figs_matches_perl_sprintf() {
        assert_eq!(format_sprintf_g(724.0779, 2), "7.2e+02");
        assert_eq!(format_sprintf_g(90.5097, 2), "91");
        assert_eq!(format_sprintf_g(1.4142, 2), "1.4");
        assert_eq!(format_sprintf_g(1.0, 2), "1");
        assert_eq!(format_sprintf_g(0.0, 2), "0");
        assert_eq!(format_sprintf_g(0.00012345, 2), "0.00012");
        assert_eq!(format_sprintf_g(0.000012345, 2), "1.2e-05");
        assert_eq!(format_sprintf_g(9.99, 2), "10");
        assert_eq!(
            render(0x010d, "Canon EOS-1D Mark III", &[1, 160, 16]),
            Some((
                "ApertureRange",
                "Enable; Closed 7.2e+02; Open 1.4".to_string()
            ))
        );
    }
}
