//! Inactive source-derived `CheckExif` composition over generated CheckValue.

use crate::error::{ExifToolError, Result};
use crate::writers::generated_scalar::{CheckedScalar, Scalar, ScalarCheckRecipe, validate_scalar};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SelectorSource {
    Tag,
    Table,
    TagGroup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Selector {
    pub source: SelectorSource,
    pub property: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SourceTable {
    pub module: &'static str,
    pub table: &'static str,
    pub full_name: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MissingFormat {
    pub equals_literal: &'static str,
    pub group_source: Selector,
    pub maker_notes_literal: &'static str,
    pub other_error: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CheckExifRecipe {
    pub source_tables: &'static [SourceTable],
    pub format_selectors: &'static [Selector],
    pub missing_format: MissingFormat,
    pub count_selector: Selector,
    pub check_value: ScalarCheckRecipe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PropertyValue<'a> {
    Undefined,
    Integer(i64),
    Text(&'a str),
    Bytes(&'a [u8]),
}

impl PropertyValue<'_> {
    fn perl_truthy(self) -> bool {
        match self {
            Self::Undefined => false,
            Self::Integer(value) => value != 0,
            Self::Text(value) => !value.is_empty() && value != "0",
            Self::Bytes(value) => !value.is_empty() && value != b"0",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Property<'a> {
    pub name: &'a str,
    pub value: PropertyValue<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CheckExifInput<'a> {
    pub tag_properties: &'a [Property<'a>],
    pub table_properties: &'a [Property<'a>],
    pub tag_groups: &'a [Property<'a>],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CheckExifResult {
    /// The MakerNotes branch did not invoke CheckValue and leaves the scalar intact.
    Bypassed(Scalar),
    /// The source error branch did not invoke CheckValue and leaves the scalar intact.
    Rejected {
        value: Scalar,
        error: &'static str,
    },
    Checked(CheckedScalar),
}

fn refused(reason: &str) -> ExifToolError {
    ExifToolError::unsupported_format(format!("generated CheckExif recipe refused: {reason}"))
}

fn find<'a>(properties: &'a [Property<'a>], name: &str) -> PropertyValue<'a> {
    properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| property.value)
        .unwrap_or(PropertyValue::Undefined)
}

fn select<'a>(input: &CheckExifInput<'a>, selector: Selector) -> PropertyValue<'a> {
    match selector.source {
        SelectorSource::Tag => find(input.tag_properties, selector.property),
        SelectorSource::Table => find(input.table_properties, selector.property),
        SelectorSource::TagGroup => find(input.tag_groups, selector.property),
    }
}

/// The admitted scalar string coercion shared by native `eq` operands.
fn scalar_string(value: PropertyValue<'_>, non_ascii_reason: &str) -> Result<String> {
    match value {
        PropertyValue::Undefined => Ok(String::new()),
        PropertyValue::Integer(value) => Ok(value.to_string()),
        PropertyValue::Text(value) => Ok(value.to_owned()),
        PropertyValue::Bytes(value) if value.is_ascii() => Ok(std::str::from_utf8(value)
            .map_err(|_| refused("ASCII scalar is invalid UTF-8"))?
            .to_owned()),
        PropertyValue::Bytes(_) => Err(refused(non_ascii_reason)),
    }
}

fn selected_format(recipe: &CheckExifRecipe, input: &CheckExifInput<'_>) -> Result<Option<String>> {
    for selector in recipe.format_selectors {
        let value = select(input, *selector);
        if !value.perl_truthy() {
            continue;
        }
        return Ok(Some(scalar_string(
            value,
            "truthy source format bytes are not ASCII",
        )?));
    }
    Ok(None)
}

fn selected_count(recipe: &CheckExifRecipe, input: &CheckExifInput<'_>) -> Result<Option<i64>> {
    match select(input, recipe.count_selector) {
        PropertyValue::Undefined => Ok(None),
        PropertyValue::Integer(count) => Ok(Some(count)),
        _ => Err(refused("source count is not an integer or undef")),
    }
}

/// Execute the source-derived CheckExif recipe with no tag lookup or writer route.
pub(crate) fn check_exif(
    recipe: &CheckExifRecipe,
    input: &CheckExifInput<'_>,
    value: Scalar,
) -> Result<CheckExifResult> {
    let format = selected_format(recipe, input)?;
    if format.is_none() || format.as_deref() == Some(recipe.missing_format.equals_literal) {
        if scalar_string(
            select(input, recipe.missing_format.group_source),
            "missing-format group bytes are not ASCII",
        )? == recipe.missing_format.maker_notes_literal
        {
            return Ok(CheckExifResult::Bypassed(value));
        }
        return Ok(CheckExifResult::Rejected {
            value,
            error: recipe.missing_format.other_error,
        });
    }
    if !recipe
        .check_value
        .formats
        .contains(&format.as_deref().expect("handled missing format"))
    {
        let numeric = super::generated_scalar_rules::NUMERIC_SCALAR
            .as_ref()
            .ok_or_else(|| refused("numeric CheckValue source is unsupported"))?;
        super::generated_scalar::numeric_value(
            numeric,
            &value,
            format.as_deref().expect("handled missing format"),
            selected_count(recipe, input)?,
        )?;
        return Ok(CheckExifResult::Checked(CheckedScalar {
            value,
            error: None,
        }));
    }
    Ok(CheckExifResult::Checked(validate_scalar(
        &recipe.check_value,
        value,
        format.expect("handled missing format").as_str(),
        selected_count(recipe, input)?,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writers::generated_scalar::Comparison;
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
    const SELECTORS: &[Selector] = &[
        Selector {
            source: SelectorSource::Tag,
            property: "Format",
        },
        Selector {
            source: SelectorSource::Tag,
            property: "Writable",
        },
        Selector {
            source: SelectorSource::Table,
            property: "WRITABLE",
        },
    ];
    const RECIPE: CheckExifRecipe = CheckExifRecipe {
        source_tables: &[],
        format_selectors: SELECTORS,
        missing_format: MissingFormat {
            equals_literal: "1",
            group_source: Selector {
                source: SelectorSource::TagGroup,
                property: "0",
            },
            maker_notes_literal: "MakerNotes",
            other_error: "No writable format",
        },
        count_selector: Selector {
            source: SelectorSource::Tag,
            property: "Count",
        },
        check_value: CHECK,
    };
    #[test]
    fn source_order_and_perl_falsey_values_select_the_first_truthy_format() {
        let tag = [
            Property {
                name: "Format",
                value: PropertyValue::Text("0"),
            },
            Property {
                name: "Writable",
                value: PropertyValue::Text("string"),
            },
            Property {
                name: "Count",
                value: PropertyValue::Integer(3),
            },
        ];
        let table = [Property {
            name: "WRITABLE",
            value: PropertyValue::Text("undef"),
        }];
        let groups = [Property {
            name: "0",
            value: PropertyValue::Text("EXIF"),
        }];
        let result = check_exif(
            &RECIPE,
            &CheckExifInput {
                tag_properties: &tag,
                table_properties: &table,
                tag_groups: &groups,
            },
            Scalar::Bytes(b"a".to_vec()),
        )
        .unwrap();
        assert_eq!(
            result,
            CheckExifResult::Checked(CheckedScalar {
                value: Scalar::Bytes(b"a\0\0".to_vec()),
                error: None
            })
        );
    }
    #[test]
    fn source_group_and_error_branch_do_not_call_scalar_helper() {
        let groups = [Property {
            name: "0",
            value: PropertyValue::Text("MakerNotes"),
        }];
        let input = CheckExifInput {
            tag_properties: &[],
            table_properties: &[],
            tag_groups: &groups,
        };
        assert_eq!(
            check_exif(&RECIPE, &input, Scalar::Bytes(b"unchanged".to_vec())).unwrap(),
            CheckExifResult::Bypassed(Scalar::Bytes(b"unchanged".to_vec()))
        );
        let other = CheckExifInput {
            tag_properties: &[],
            table_properties: &[],
            tag_groups: &[],
        };
        assert_eq!(
            check_exif(&RECIPE, &other, Scalar::Bytes(b"unchanged".to_vec())).unwrap(),
            CheckExifResult::Rejected {
                value: Scalar::Bytes(b"unchanged".to_vec()),
                error: "No writable format",
            }
        );
    }
    #[test]
    fn scalar_coercion_and_unrepresentable_counts_are_explicit() {
        let bytes = [Property {
            name: "Format",
            value: PropertyValue::Bytes(b"string"),
        }];
        assert!(matches!(
            check_exif(
                &RECIPE,
                &CheckExifInput {
                    tag_properties: &bytes,
                    table_properties: &[],
                    tag_groups: &[],
                },
                Scalar::Undefined,
            )
            .unwrap(),
            CheckExifResult::Checked(_)
        ));

        let undef = [Property {
            name: "Format",
            value: PropertyValue::Bytes(b"undef"),
        }];
        assert!(matches!(
            check_exif(
                &RECIPE,
                &CheckExifInput {
                    tag_properties: &undef,
                    table_properties: &[],
                    tag_groups: &[],
                },
                Scalar::Undefined,
            )
            .unwrap(),
            CheckExifResult::Checked(_)
        ));

        for writable in [PropertyValue::Integer(1), PropertyValue::Bytes(b"1")] {
            let one = [Property {
                name: "Writable",
                value: writable,
            }];
            assert_eq!(
                check_exif(
                    &RECIPE,
                    &CheckExifInput {
                        tag_properties: &one,
                        table_properties: &[],
                        tag_groups: &[],
                    },
                    Scalar::Undefined,
                )
                .unwrap(),
                CheckExifResult::Rejected {
                    value: Scalar::Undefined,
                    error: "No writable format",
                }
            );
        }

        let bad_count = [
            Property {
                name: "Format",
                value: PropertyValue::Bytes(b"undef"),
            },
            Property {
                name: "Count",
                value: PropertyValue::Text("1"),
            },
        ];
        assert!(
            check_exif(
                &RECIPE,
                &CheckExifInput {
                    tag_properties: &bad_count,
                    table_properties: &[],
                    tag_groups: &[],
                },
                Scalar::Undefined,
            )
            .is_err()
        );
    }

    #[test]
    fn missing_format_group_eq_uses_the_same_scalar_coercion() {
        let numeric_group_recipe = CheckExifRecipe {
            missing_format: MissingFormat {
                maker_notes_literal: "1",
                ..RECIPE.missing_format
            },
            ..RECIPE
        };
        let numeric_group = [Property {
            name: "0",
            value: PropertyValue::Integer(1),
        }];
        assert_eq!(
            check_exif(
                &numeric_group_recipe,
                &CheckExifInput {
                    tag_properties: &[],
                    table_properties: &[],
                    tag_groups: &numeric_group,
                },
                Scalar::Bytes(b"unchanged".to_vec()),
            )
            .unwrap(),
            CheckExifResult::Bypassed(Scalar::Bytes(b"unchanged".to_vec()))
        );

        let empty_group_recipe = CheckExifRecipe {
            missing_format: MissingFormat {
                maker_notes_literal: "",
                ..RECIPE.missing_format
            },
            ..RECIPE
        };
        assert_eq!(
            check_exif(
                &empty_group_recipe,
                &CheckExifInput {
                    tag_properties: &[],
                    table_properties: &[],
                    tag_groups: &[],
                },
                Scalar::Undefined,
            )
            .unwrap(),
            CheckExifResult::Bypassed(Scalar::Undefined)
        );

        let non_ascii_group = [Property {
            name: "0",
            value: PropertyValue::Bytes(b"\x80"),
        }];
        assert!(
            check_exif(
                &RECIPE,
                &CheckExifInput {
                    tag_properties: &[],
                    table_properties: &[],
                    tag_groups: &non_ascii_group,
                },
                Scalar::Undefined,
            )
            .is_err()
        );
    }
}
