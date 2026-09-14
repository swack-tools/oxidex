//! Source-derived scalar tail of ExifTool `ConvInv`.
//!
//! The writer route supplies already-captured row properties and the exact
//! CHECK_PROC recipe. This module does no tag lookup.
use crate::error::{ExifToolError, Result};
use crate::writers::generated_checkexif::{
    CheckExifInput, CheckExifRecipe, CheckExifResult, check_exif,
};
use crate::writers::generated_scalar::Scalar;

#[derive(Clone, Copy)]
pub(crate) struct ConvInvRecipe {
    pub default_type: &'static str,
    pub error_separator: &'static str,
}
/// Mirrors `defined $tagInfo{...}`: false defined values remain active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConversionProperty {
    Absent,
    Undefined,
    Defined,
}
#[derive(Clone, Copy)]
pub(crate) struct ConvInvRow<'a> {
    pub print_conv: ConversionProperty,
    pub print_conv_inv: ConversionProperty,
    pub value_conv: ConversionProperty,
    pub value_conv_inv: ConversionProperty,
    pub list: bool,
    pub raw_join: bool,
    pub write_check: bool,
    pub raw_conv_inv: bool,
    /// Exact source-selected table CHECK_PROC. `None` cannot be overridden by a caller.
    pub check_proc: Option<(&'a CheckExifRecipe, &'a CheckExifInput<'a>)>,
    pub requested_tag: &'a str,
    pub actual_tag: &'a str,
    pub write_group: &'a str,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConvInvResult {
    pub value: Scalar,
    pub error: Option<String>,
}
fn refused(reason: &str) -> ExifToolError {
    ExifToolError::unsupported_format(format!("generated ConvInv scalar path refused: {reason}"))
}
fn defined(property: ConversionProperty) -> bool {
    property == ConversionProperty::Defined
}
fn inactive(row: ConvInvRow<'_>) -> Result<()> {
    if defined(row.print_conv)
        || defined(row.print_conv_inv)
        || defined(row.value_conv)
        || defined(row.value_conv_inv)
    {
        return Err(refused("active PrintConv/ValueConv property"));
    }
    if row.list {
        return Err(refused("List splitting"));
    }
    if row.raw_join {
        return Err(refused("RawJoin"));
    }
    if row.write_check {
        return Err(refused("WriteCheck"));
    }
    Ok(())
}
/// Executes ConvInv's no-conversion tail. RawConvInv only suppresses CHECK_PROC
/// in native ConvInv, so it preserves the scalar without executing it.
pub(crate) fn conv_inv_scalar(
    recipe: &ConvInvRecipe,
    value: Scalar,
    row: ConvInvRow<'_>,
    conversion_type: Option<&str>,
    object_conv_type: Option<&str>,
) -> Result<ConvInvResult> {
    // `$convType or $convType = $$self{ConvType} || 'PrintConv'`: Perl falsey
    // values use the object member, then the source literal. The no-conversion
    // tail is identical for PrintConv, ValueConv, and Raw once both property
    // lookups have fallen through.
    let effective_type = conversion_type
        .filter(|v| !v.is_empty() && *v != "0")
        .or_else(|| object_conv_type.filter(|v| !v.is_empty() && *v != "0"))
        .unwrap_or(recipe.default_type);
    if effective_type != "PrintConv" && effective_type != "ValueConv" {
        return Err(refused("unrepresented conversion type property lookup"));
    }
    inactive(row)?;
    if row.raw_conv_inv || row.check_proc.is_none() {
        return Ok(ConvInvResult { value, error: None });
    }
    let (check_recipe, input) = row.check_proc.expect("checked above");
    match check_exif(check_recipe, input, value)? {
        CheckExifResult::Bypassed(value) => Ok(ConvInvResult { value, error: None }),
        CheckExifResult::Checked(checked) if checked.error.is_none() => Ok(ConvInvResult {
            value: checked.value,
            error: None,
        }),
        CheckExifResult::Checked(checked) => check_error(
            checked.value,
            checked.error.expect("checked above"),
            row,
            recipe.error_separator,
        ),
        CheckExifResult::Rejected { value, error } => {
            check_error(value, error, row, recipe.error_separator)
        }
    }
}
fn check_error(
    value: Scalar,
    error: &'static str,
    row: ConvInvRow<'_>,
    separator: &str,
) -> Result<ConvInvResult> {
    // Perl false values are the empty string *and* the string "0".
    if error.is_empty() || error == "0" {
        return Ok(ConvInvResult {
            value,
            error: Some(error.to_owned()),
        });
    }
    Ok(ConvInvResult {
        value: Scalar::Undefined,
        error: Some(format!(
            "{error}{separator}{}:{}",
            row.write_group, row.actual_tag
        )),
    })
}
