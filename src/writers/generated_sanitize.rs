//! Shared input normalization selected by generated native source recipes.
//!
//! This is not connected to public writing until complete operation routing is
//! proven. Generation validates the native encoding contract before emitting a
//! recipe; callers cannot supply a flag to declare Encode available.

use crate::error::{ExifToolError, Result};
use crate::writers::generated_scalar::Scalar;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EncodingPrimitive {
    Utf8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SanitizeRecipe {
    /// Selected native interpreter version and source guards, scaled by 1e6.
    pub perl_version: u64,
    pub downgrade_at_or_after: u64,
    pub manual_pack_before: u64,
    /// Whether the captured source consults the EncodeHangs option.
    pub encode_hangs_guard: bool,
    pub encoding: EncodingPrimitive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EscapeOption {
    Disabled,
    Xml,
    Html,
    OtherTruthy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SanitizeOptions {
    pub encode_hangs: bool,
    pub escape: EscapeOption,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SanitizeInput {
    Direct(Scalar),
    ScalarReference(Scalar),
    OtherReference,
}

fn refused(reason: &str) -> ExifToolError {
    ExifToolError::unsupported_format(format!("generated Sanitize recipe refused: {reason}"))
}

pub(crate) fn sanitize(
    recipe: &SanitizeRecipe,
    input: SanitizeInput,
    options: SanitizeOptions,
) -> Result<Scalar> {
    let mut value = match input {
        SanitizeInput::Direct(value) | SanitizeInput::ScalarReference(value) => value,
        SanitizeInput::OtherReference => return Err(refused("non-SCALAR reference")),
    };
    if recipe.perl_version >= recipe.downgrade_at_or_after {
        if recipe.encode_hangs_guard && options.encode_hangs {
            return Err(refused("EncodeHangs manual packing is not implemented"));
        }
        // The validated native primitive returns false for bytes and undef.
        // Only an UTF8-flagged scalar enters the source's encoding branch.
        if let Scalar::Utf8(text) = value {
            value = match recipe.encoding {
                EncodingPrimitive::Utf8 => Scalar::Bytes(text.into_bytes()),
            };
        }
    }
    match options.escape {
        EscapeOption::Disabled | EscapeOption::OtherTruthy => Ok(value),
        EscapeOption::Xml => Err(refused("XML unescape is not implemented")),
        EscapeOption::Html => Err(refused("HTML unescape is not implemented")),
    }
}
