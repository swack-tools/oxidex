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
