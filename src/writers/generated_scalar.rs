//! Inactive source-derived scalar CheckValue and WriteValue execution.

use crate::error::{ExifToolError, Result};

/// Perl's scalar storage state at the boundary of the proven helper branch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Scalar {
    Undefined,
    Bytes(Vec<u8>),
    Utf8(String),
}

/// One comparison operator carried from a source-derived recipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Comparison {
    Lt,
    Le,
    Eq,
    Ne,
    Ge,
    Gt,
}

impl Comparison {
    fn applies(self, left: i64, right: i64) -> bool {
        match self {
            Self::Lt => left < right,
            Self::Le => left <= right,
            Self::Eq => left == right,
            Self::Ne => left != right,
            Self::Ge => left >= right,
            Self::Gt => left > right,
        }
    }
}

/// The complete admitted scalar early-return branch of native `CheckValue`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScalarCheckRecipe {
    pub formats: [&'static str; 2],
    pub positive_count_operator: Comparison,
    pub positive_count_bound: i64,
    pub first_format: &'static str,
    pub first_limit_operator: Comparison,
    pub first_error: &'static str,
    pub second_limit_operator: Comparison,
    pub second_error: &'static str,
    pub padding_operator: Comparison,
    /// The compiler admits only the native NUL operand.
    pub padding_character: u8,
}

/// The complete admitted scalar early-return branch of native `WriteValue`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScalarWriteRecipe {
    pub formats: [&'static str; 2],
    pub terminated_format: &'static str,
    pub count_positive_operator: Comparison,
    pub count_positive_bound: i64,
    pub diff_negative_operator: Comparison,
    /// The compiler admits only the native NUL operand.
    pub terminator: u8,
}

/// A CheckValue result: its possibly padded scalar and native source error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CheckedScalar {
    pub value: Scalar,
    pub error: Option<&'static str>,
}

/// A WriteValue result before optional data-pointer mutation or CharsetEXIF.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SerializedScalar {
    pub value: Scalar,
    /// The source branch's scalar count. TIFF's u32 field-count admission is later.
    pub count: usize,
}

fn refused(reason: &str) -> ExifToolError {
    ExifToolError::unsupported_format(format!("generated scalar recipe refused: {reason}"))
}

fn ensure_format(formats: [&'static str; 2], format_name: &str) -> Result<()> {
    if formats.contains(&format_name) {
        Ok(())
    } else {
        Err(refused("format is outside the proven scalar branch"))
    }
}

fn scalar_length(value: &Scalar) -> Result<usize> {
    match value {
        Scalar::Undefined => Ok(0),
        Scalar::Bytes(bytes) => Ok(bytes.len()),
        // Perl's length/substr operate on characters for a UTF8-flagged scalar.
        Scalar::Utf8(text) => Ok(text.chars().count()),
    }
}

fn i64_length(value: &Scalar) -> Result<i64> {
    i64::try_from(scalar_length(value)?).map_err(|_| refused("scalar length overflows i64"))
}

fn positive_count(count: Option<i64>, operator: Comparison, bound: i64) -> bool {
    count.is_some_and(|value| value != 0 && operator.applies(value, bound))
}

fn checked_count(value: i64) -> Result<usize> {
    usize::try_from(value).map_err(|_| refused("count cannot be represented on this platform"))
}

fn append_byte(value: Scalar, byte: u8, amount: usize) -> Result<Scalar> {
    match value {
        Scalar::Undefined => {
            let mut bytes = Vec::new();
            bytes
                .try_reserve(amount)
                .map_err(|_| refused("scalar padding allocation failed"))?;
            bytes.resize(amount, byte);
            Ok(Scalar::Bytes(bytes))
        }
        Scalar::Bytes(mut bytes) => {
            bytes
                .try_reserve(amount)
                .map_err(|_| refused("scalar padding allocation failed"))?;
            bytes.resize(
                bytes
                    .len()
                    .checked_add(amount)
                    .ok_or_else(|| refused("scalar length overflow"))?,
                byte,
            );
            Ok(Scalar::Bytes(bytes))
        }
        Scalar::Utf8(mut text) => {
            if byte > 0x7f {
                return Err(refused("non-ASCII scalar padding is unsupported"));
            }
            text.try_reserve(amount)
                .map_err(|_| refused("scalar padding allocation failed"))?;
            text.extend(std::iter::repeat(char::from(byte)).take(amount));
            Ok(Scalar::Utf8(text))
        }
    }
}

fn truncate(value: Scalar, length: usize) -> Result<Scalar> {
    match value {
        Scalar::Undefined => Ok(Scalar::Undefined),
        Scalar::Bytes(mut bytes) => {
            bytes.truncate(length);
            Ok(Scalar::Bytes(bytes))
        }
        Scalar::Utf8(text) => {
            let byte_end = text
                .char_indices()
                .nth(length)
                .map_or(text.len(), |(index, _)| index);
            let truncated = &text[..byte_end];
            // Perl's `substr` downgrades an upgraded ASCII scalar, including
            // one containing embedded NULs. A source scalar that contained a
            // non-ASCII character retains its UTF8 flag even when the selected
            // substring happens to be ASCII or empty.
            if text.is_ascii() {
                Ok(Scalar::Bytes(truncated.as_bytes().to_vec()))
            } else {
                Ok(Scalar::Utf8(truncated.to_owned()))
            }
        }
    }
}

/// Execute the proven scalar `CheckValue` branch without tag lookup or conversion.
pub(crate) fn validate_scalar(
    recipe: &ScalarCheckRecipe,
    mut value: Scalar,
    format_name: &str,
    count: Option<i64>,
) -> Result<CheckedScalar> {
    ensure_format(recipe.formats, format_name)?;
    if recipe.padding_character != 0 {
        return Err(refused("scalar CheckValue padding is not NUL"));
    }
    if !positive_count(
        count,
        recipe.positive_count_operator,
        recipe.positive_count_bound,
    ) {
        return Ok(CheckedScalar { value, error: None });
    }
    let count = count.expect("positive_count requires Some");
    let length = i64_length(&value)?;
    let (limit, error) = if format_name == recipe.first_format {
        (recipe.first_limit_operator, recipe.first_error)
    } else {
        (recipe.second_limit_operator, recipe.second_error)
    };
    if limit.applies(length, count) {
        return Ok(CheckedScalar {
            value,
            error: Some(error),
        });
    }
    if recipe.padding_operator.applies(length, count) {
        // Perl repetition with a non-positive count produces the empty string.
        // Source changes may select this branch when length exceeds count.
        let padding = if count <= length {
            0
        } else {
            checked_count(count - length)?
        };
        value = append_byte(value, recipe.padding_character, padding)?;
    }
    Ok(CheckedScalar { value, error: None })
}

/// Execute the proven scalar `WriteValue` branch without data-pointer mutation.
pub(crate) fn serialize_scalar(
    recipe: &ScalarWriteRecipe,
    mut value: Scalar,
    format_name: &str,
    count: Option<i64>,
) -> Result<SerializedScalar> {
    ensure_format(recipe.formats, format_name)?;
    if recipe.terminator != 0 {
        return Err(refused("scalar WriteValue terminator is not NUL"));
    }
    if format_name == recipe.terminated_format {
        value = append_byte(value, recipe.terminator, 1)?;
    }
    let final_count = if positive_count(
        count,
        recipe.count_positive_operator,
        recipe.count_positive_bound,
    ) {
        let count = count.expect("positive_count requires Some");
        let length = i64_length(&value)?;
        let difference = count
            .checked_sub(length)
            .ok_or_else(|| refused("scalar count subtraction overflow"))?;
        if difference != 0 {
            if recipe.diff_negative_operator.applies(difference, 0) {
                let target = checked_count(count)?;
                value = if format_name == recipe.terminated_format {
                    let before_terminator = target
                        .checked_sub(1)
                        .ok_or_else(|| refused("terminated scalar count is zero"))?;
                    append_byte(truncate(value, before_terminator)?, recipe.terminator, 1)?
                } else {
                    truncate(value, target)?
                };
            } else {
                value = append_byte(value, recipe.terminator, checked_count(difference)?)?;
            }
        }
        checked_count(count)?
    } else {
        scalar_length(&value)?
    };
    Ok(SerializedScalar {
        value,
        count: final_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECK: ScalarCheckRecipe = ScalarCheckRecipe {
        formats: ["string", "undef"],
        positive_count_operator: Comparison::Gt,
        positive_count_bound: 0,
        first_format: "string",
        first_limit_operator: Comparison::Ge,
        first_error: "String too long",
        second_limit_operator: Comparison::Gt,
        second_error: "Data too long",
        padding_operator: Comparison::Lt,
        padding_character: 0,
    };

    const WRITE: ScalarWriteRecipe = ScalarWriteRecipe {
        formats: ["string", "undef"],
        terminated_format: "string",
        count_positive_operator: Comparison::Gt,
        count_positive_bound: 0,
        diff_negative_operator: Comparison::Lt,
        terminator: 0,
    };

    #[test]
    fn changed_check_comparisons_preserve_negative_repeat_semantics() {
        let changed = ScalarCheckRecipe {
            first_limit_operator: Comparison::Eq,
            second_limit_operator: Comparison::Eq,
            padding_operator: Comparison::Gt,
            ..CHECK
        };
        let result = validate_scalar(&changed, Scalar::Bytes(b"abc".to_vec()), "string", Some(2))
            .expect("negative repeat appends an empty string");
        assert_eq!(result.value, Scalar::Bytes(b"abc".to_vec()));
        assert_eq!(result.error, None);
    }

    #[test]
    fn checkvalue_uses_character_lengths_and_preserves_utf8() {
        let checked = validate_scalar(&CHECK, Scalar::Utf8("é".into()), "string", Some(3))
            .expect("scalar recipe accepts string");
        assert_eq!(checked.error, None);
        assert_eq!(checked.value, Scalar::Utf8("é\0\0".into()));

        let too_long = validate_scalar(&CHECK, Scalar::Bytes(b"abc".to_vec()), "string", Some(3))
            .expect("validated scalar returns source error");
        assert_eq!(too_long.error, Some("String too long"));
    }

    #[test]
    fn writevalue_nul_terminates_then_truncates_and_reports_encoded_count() {
        let encoded = serialize_scalar(&WRITE, Scalar::Bytes(b"abcd".to_vec()), "string", Some(3))
            .expect("scalar recipe serializes string");
        assert_eq!(encoded.value, Scalar::Bytes(b"ab\0".to_vec()));
        assert_eq!(encoded.count, 3);
    }

    #[test]
    fn writevalue_substr_downgrades_utf8_scalars_that_become_ascii() {
        let nul_only = serialize_scalar(&WRITE, Scalar::Utf8("\0".into()), "string", Some(1))
            .expect("scalar recipe serializes string");
        assert_eq!(nul_only.value, Scalar::Bytes(vec![0]));

        let ascii_with_nuls =
            serialize_scalar(&WRITE, Scalar::Utf8("a\0\0".into()), "undef", Some(2))
                .expect("scalar recipe serializes undef");
        assert_eq!(ascii_with_nuls.value, Scalar::Bytes(b"a\0".to_vec()));
    }

    #[test]
    fn absent_zero_and_negative_counts_follow_the_native_scalar_branches() {
        for count in [None, Some(0), Some(-1)] {
            let checked = validate_scalar(&CHECK, Scalar::Undefined, "undef", count)
                .expect("check accepts scalar count");
            assert_eq!(
                checked,
                CheckedScalar {
                    value: Scalar::Undefined,
                    error: None
                }
            );

            let encoded = serialize_scalar(&WRITE, Scalar::Undefined, "string", count)
                .expect("write accepts scalar count");
            assert_eq!(
                encoded,
                SerializedScalar {
                    value: Scalar::Bytes(vec![0]),
                    count: 1
                }
            );
        }
    }

    #[test]
    fn unproven_format_and_count_overflow_refuse() {
        assert!(validate_scalar(&CHECK, Scalar::Bytes(vec![]), "int8u", Some(1)).is_err());
        assert!(serialize_scalar(&WRITE, Scalar::Bytes(vec![]), "string", Some(i64::MAX)).is_err());
    }
}

/// Closed count-one numeric scalar specialization. Formats and bounds
/// are emitted only after CheckValue, WriteValue and their dependencies compile.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NumericScalarRecipe {
    pub formats: [&'static str; 2],
    pub maxima: [u64; 2],
    pub writer_source_sha256: &'static str,
    pub main_source_sha256: &'static str,
    pub rounding_offset: f64,
    pub rational_relative_error: f64,
    pub integer_range_exception: u64,
}

pub(crate) fn numeric_value(
    recipe: &NumericScalarRecipe,
    value: &Scalar,
    format: &str,
    count: Option<i64>,
) -> Result<(u32, u32)> {
    if count.is_some_and(|count| count != 1 && count != 0) {
        return Err(refused(
            "numeric scalar count is outside the proven specialization",
        ));
    }
    let index = recipe
        .formats
        .iter()
        .position(|item| *item == format)
        .ok_or_else(|| refused("numeric format has no compiled recipe"))?;
    let bytes = match value {
        Scalar::Bytes(bytes) => bytes.as_slice(),
        Scalar::Utf8(text) => text.as_bytes(),
        Scalar::Undefined => return Err(refused("numeric scalar is undefined")),
    };
    fn digits(bytes: &[u8]) -> Option<u64> {
        if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
            return None;
        }
        bytes.iter().try_fold(0u64, |value, byte| {
            value.checked_mul(10)?.checked_add(u64::from(byte - b'0'))
        })
    }
    fn unsigned(bytes: &[u8]) -> Option<u64> {
        digits(bytes.strip_prefix(b"+").unwrap_or(bytes))
    }
    fn float(bytes: &[u8]) -> Option<f64> {
        // Exact ASCII specialization of the compiled IsFloat alternatives.
        let body = bytes
            .strip_prefix(b"+")
            .or_else(|| bytes.strip_prefix(b"-"))
            .unwrap_or(bytes);
        let mut at = 0;
        while body.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        let mut digits_count = at;
        if body
            .get(at)
            .is_some_and(|byte| *byte == b'.' || *byte == b',')
        {
            at += 1;
            let start = at;
            while body.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
            digits_count += at - start;
        }
        if digits_count == 0 {
            return None;
        }
        if body
            .get(at)
            .is_some_and(|byte| *byte == b'E' || *byte == b'e')
        {
            at += 1;
            if body
                .get(at)
                .is_some_and(|byte| *byte == b'+' || *byte == b'-')
            {
                at += 1;
            }
            let start = at;
            while body.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
            if at == start {
                return None;
            }
        }
        if at != body.len() {
            return None;
        }
        std::str::from_utf8(bytes)
            .ok()?
            .replace(',', ".")
            .parse()
            .ok()
    }
    let maximum = recipe.maxima[index];
    if index == 0 {
        let signed = bytes
            .strip_prefix(b"+")
            .or_else(|| bytes.strip_prefix(b"-"))
            .unwrap_or(bytes);
        let number = if digits(signed).is_some() {
            float(bytes)
        } else {
            // Native checks IsHex before IsFloat: "1e3" is hexadecimal here.
            let hex = bytes
                .strip_prefix(b"0x")
                .or_else(|| bytes.strip_prefix(b"0X"))
                .unwrap_or(bytes);
            if !hex.is_empty() && hex.len() <= 8 && hex.iter().all(u8::is_ascii_hexdigit) {
                std::str::from_utf8(hex)
                    .ok()
                    .and_then(|text| u32::from_str_radix(text, 16).ok())
                    .map(f64::from)
            } else {
                float(bytes).map(|value| {
                    (value
                        + if value < 0.0 {
                            -recipe.rounding_offset
                        } else {
                            recipe.rounding_offset
                        })
                    .trunc()
                })
            }
        }
        .ok_or_else(|| refused("numeric input fails source integer validation"))?;
        if !number.is_finite()
            || number < 0.0
            || (number > maximum as f64 && number != recipe.integer_range_exception as f64)
        {
            return Err(refused("numeric input is outside source integer range"));
        }
        return Ok((number as u32, 1));
    }
    if bytes == b"inf" {
        return Ok((1, 0));
    }
    if bytes == b"undef" {
        return Ok((0, 0));
    }
    if let Some(slash) = bytes.iter().position(|byte| *byte == b'/') {
        // CheckValue accepts only an unsigned numerator and an unsigned
        // denominator; Rationalize returns the original operands without
        // reduction. Native Set32u packs the low 32 bits of these integers.
        let numerator = unsigned(&bytes[..slash])
            .or_else(|| {
                bytes[..slash]
                    .strip_prefix(b"-")
                    .and_then(digits)
                    .filter(|value| *value == 0)
            })
            .ok_or_else(|| refused("invalid unsigned rational numerator"))?;
        let denominator = digits(&bytes[slash + 1..])
            .ok_or_else(|| refused("invalid unsigned rational denominator"))?;
        return Ok((numerator as u32, denominator as u32));
    }
    let value = float(bytes)
        .ok_or_else(|| refused("numeric input fails source floating point validation"))?;
    if !value.is_finite() || value < 0.0 {
        return Err(refused("numeric input is not a finite unsigned rational"));
    }
    if value == 0.0 {
        return Ok((0, 1));
    }
    let mut fraction = value;
    let mut fractions: Vec<u64> = Vec::new();
    let mut saved = None;
    loop {
        let mut numerator = (fraction + recipe.rounding_offset)
            .trunc()
            .min((maximum + 1) as f64) as u64;
        let mut denominator = 1u64;
        // AssembleRational consumes the continued fractions deepest first.
        // Saturation beyond maxInt preserves its sole use: the bound test.
        for part in &fractions {
            let next = part
                .saturating_mul(numerator)
                .saturating_add(denominator)
                .min(maximum + 1);
            denominator = numerator;
            numerator = next;
        }
        if numerator > maximum || denominator > maximum {
            return Ok(saved.unwrap_or(if value < 1.0 {
                (1, maximum as u32)
            } else {
                (maximum as u32, 1)
            }));
        }
        saved = Some((numerator as u32, denominator as u32));
        let error = (numerator as f64 / denominator as f64 - value) / value;
        if error.abs() < recipe.rational_relative_error {
            return Ok(saved.unwrap());
        }
        let integer = fraction.trunc();
        fractions.insert(0, integer as u64);
        fraction -= integer;
        if fraction == 0.0 {
            return Ok(saved.unwrap());
        }
        fraction = 1.0 / fraction;
    }
}

pub(crate) fn serialize_numeric(
    recipe: &NumericScalarRecipe,
    value: &Scalar,
    format: &str,
    little: bool,
) -> Result<Vec<u8>> {
    let (number, denominator) = numeric_value(recipe, value, format, None)?;
    let index = recipe
        .formats
        .iter()
        .position(|item| *item == format)
        .ok_or_else(|| refused("numeric format has no compiled recipe"))?;
    // Compiler proves the ordered Set16u / SetRational64u dispatch and both
    // native byte-order maps. Validation and rationalization run above.
    match index {
        0 => {
            let number = number as u16; // Native Set16u also packs the range-exception sentinel.
            Ok(if little {
                number.to_le_bytes()
            } else {
                number.to_be_bytes()
            }
            .to_vec())
        }
        1 => {
            let mut bytes = if little {
                number.to_le_bytes()
            } else {
                number.to_be_bytes()
            }
            .to_vec();
            bytes.extend(if little {
                denominator.to_le_bytes()
            } else {
                denominator.to_be_bytes()
            });
            Ok(bytes)
        }
        _ => Err(refused("numeric packing operation is unsupported")),
    }
}

/// Captured native source intersection shared by caller operands.
pub(crate) struct NativeSourceCapture {
    pub exiftool_version: &'static str,
    pub main_source_sha256: &'static str,
    pub write_exif_source_sha256: &'static str,
    pub writer_source_sha256: &'static str,
    pub exif_source_sha256: &'static str,
}

/// Fully recognized SetNewValue caller branches with their source identity.
pub(crate) struct PublicSetNewValueCallerRecipe {
    pub directories: &'static [&'static str],
    pub undefined_value_bypasses_conversion: bool,
    pub capture: NativeSourceCapture,
}
