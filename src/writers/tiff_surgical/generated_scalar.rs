//! Internal composition of generated scalar rules with the TIFF carrier.
//!
//! Public SetNewValue option/name resolution remains a separate admission task.
//! This boundary accepts exact generated names, default options, and typed
//! scalars. It never looks up a tag in the handwritten registry.

use super::entry_edits::{EntryMutation, ScopedEntryEdit, apply_entry_edits};
use super::{ByteOrder, IfdKind, scan_tiff};
use crate::error::{ExifToolError, Result};
use crate::parsers::common::exif_types::ExifType;
use crate::writers::generated_checkexif::CheckExifRecipe;
use crate::writers::generated_convinv::{ConvInvRecipe, StaticConvInvRow, conv_inv_static};
use crate::writers::generated_sanitize::{
    EscapeOption, SanitizeInput, SanitizeOptions, SanitizeRecipe, sanitize,
};
use crate::writers::generated_scalar::Scalar;
use crate::writers::tiff_scalar_final_stage::{
    NativeTiffFormatRegistry, ResolvedTiffScalarEdit, ScalarWriteOperation, TiffByteOrder,
    TiffScalarFinalStageRecipe, resolve_tiff_scalar_final_stage,
};

/// All operands must be emitted from the same selected native capture.
pub(crate) struct ScalarWriteRules<'a> {
    pub sanitize: Option<&'a SanitizeRecipe>,
    pub conv_inv: Option<&'a ConvInvRecipe>,
    pub checks: &'a [CheckExifRecipe],
    pub rows: &'a [StaticConvInvRow],
    pub finals: &'a [TiffScalarFinalStageRecipe],
    pub formats: Option<&'a NativeTiffFormatRegistry>,
}

pub(crate) fn generated_rules() -> ScalarWriteRules<'static> {
    use crate::writers::{
        generated_checkexif_rules, generated_convinv_rows, generated_convinv_rules,
        generated_sanitize_rules, generated_tiff_scalar_final_rules,
    };
    ScalarWriteRules {
        sanitize: generated_sanitize_rules::SANITIZE_RECIPE.as_ref(),
        conv_inv: generated_convinv_rules::CONV_INV_RECIPE.as_ref(),
        checks: generated_checkexif_rules::CHECK_EXIF_RECIPES,
        rows: generated_convinv_rows::CONV_INV_ROWS,
        finals: generated_tiff_scalar_final_rules::TIFF_SCALAR_FINAL_RECIPES,
        formats: generated_tiff_scalar_final_rules::TIFF_SCALAR_FINAL_FORMAT_REGISTRY.as_ref(),
    }
}

pub(crate) struct ScalarWriteRequest<'a> {
    /// Exact source tag name, optionally qualified by its generated family-0
    /// or physical write group. No public alias/case-folding promise here.
    pub key: &'a str,
    /// Undefined is deletion; a defined empty value stays defined.
    pub value: Scalar,
}

#[derive(Debug)]
pub(crate) struct ScalarWriteOutput {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

fn refused(reason: &str) -> ExifToolError {
    ExifToolError::unsupported_format(format!(
        "generated TIFF scalar composition refused: {reason}"
    ))
}

fn raw_id(raw: &str) -> Option<u16> {
    if let Some(hex) = raw.strip_prefix("0x") {
        u16::from_str_radix(hex, 16).ok()
    } else if raw.bytes().all(|byte| byte.is_ascii_digit()) {
        raw.parse().ok()
    } else {
        None
    }
}

/// Join by complete captured identity, not tag spelling or numeric ID alone.
fn conversion_row<'a>(
    final_rule: &TiffScalarFinalStageRecipe,
    rows: &'a [StaticConvInvRow],
) -> Result<&'a StaticConvInvRow> {
    let mut matches = rows.iter().filter(|row| {
        row.module == final_rule.module
            && row.table == final_rule.table
            && row.full_name == final_rule.full_name
            && raw_id(row.raw_id) == Some(final_rule.raw_tag_id)
            && row.name == final_rule.tag_name
            && row.write_group == final_rule.physical_write_group
    });
    let row = matches
        .next()
        .ok_or_else(|| refused("no conversion row for final source identity"))?;
    if matches.next().is_some() {
        return Err(refused(
            "ambiguous conversion rows for final source identity",
        ));
    }
    Ok(row)
}

fn final_rule<'a>(
    key: &str,
    rules: &'a [TiffScalarFinalStageRecipe],
) -> Result<&'a TiffScalarFinalStageRecipe> {
    let (group, name) = key
        .split_once(':')
        .map_or((None, key), |(group, name)| (Some(group), name));
    let mut matches = rules.iter().filter(|rule| {
        name == rule.tag_name
            && group.is_none_or(|group| {
                group == rule.table_group0 || group == rule.physical_write_group
            })
    });
    let rule = matches
        .next()
        .ok_or_else(|| refused("no generated final rule for requested name/group"))?;
    if matches.next().is_some() {
        return Err(refused(
            "ambiguous generated final rules for requested name/group",
        ));
    }
    Ok(rule)
}

fn physical_ifd(group: &str) -> Result<IfdKind> {
    [IfdKind::Ifd0, IfdKind::ExifIfd, IfdKind::Gps]
        .into_iter()
        .find(|ifd| ifd.prefix() == group)
        .ok_or_else(|| refused("physical directory is unsupported by the TIFF carrier"))
}

/// Compose one batch atomically in memory. An unsupported source rule or
/// conversion error returns no output; in particular it cannot become Delete.
pub(crate) fn rewrite_generated_scalars(
    file: &[u8],
    requests: Vec<ScalarWriteRequest<'_>>,
    rules: &ScalarWriteRules<'_>,
) -> Result<ScalarWriteOutput> {
    let selected = requests
        .into_iter()
        .map(|request| Ok((final_rule(request.key, rules.finals)?, request.value)))
        .collect::<Result<Vec<_>>>()?;
    rewrite_selected_scalars(file, selected, rules)
}

/// A public-name resolver must pass the complete selected identity and native
/// source hashes. An identically named field from another table or release
/// cannot silently inherit a final serialization rule.
pub(crate) struct ResolvedScalarWriteRequest<'a> {
    pub module: &'a str,
    pub table: &'a str,
    pub full_name: &'a str,
    pub raw_id: &'a str,
    pub name: &'a str,
    pub write_group: &'a str,
    pub write_proc_source_sha256: &'a str,
    pub registry_source_sha256: &'a str,
    pub writer_source_sha256: &'a str,
    pub value: Scalar,
}

pub(crate) fn rewrite_resolved_generated_scalars(
    file: &[u8],
    requests: Vec<ResolvedScalarWriteRequest<'_>>,
    rules: &ScalarWriteRules<'_>,
) -> Result<ScalarWriteOutput> {
    let mut selected = Vec::new();
    for request in requests {
        let mut matches = rules.finals.iter().filter(|rule| {
            rule.module == request.module
                && rule.table == request.table
                && rule.full_name == request.full_name
                && Some(rule.raw_tag_id) == raw_id(request.raw_id)
                && rule.tag_name == request.name
                && rule.physical_write_group == request.write_group
                && rule.write_proc_source_sha256 == request.write_proc_source_sha256
                && rule.registry_source_sha256 == request.registry_source_sha256
                && rule.writer_source_sha256 == request.writer_source_sha256
        });
        let rule = matches
            .next()
            .ok_or_else(|| refused("resolved address has no matching final source identity"))?;
        if matches.next().is_some() {
            return Err(refused("resolved address has ambiguous final rules"));
        }
        selected.push((rule, request.value));
    }
    rewrite_selected_scalars(file, selected, rules)
}

fn rewrite_selected_scalars(
    file: &[u8],
    selected: Vec<(&TiffScalarFinalStageRecipe, Scalar)>,
    rules: &ScalarWriteRules<'_>,
) -> Result<ScalarWriteOutput> {
    let scan = scan_tiff(file)?;
    let byte_order = match scan.byte_order {
        ByteOrder::LittleEndian => TiffByteOrder::LittleEndian,
        ByteOrder::BigEndian => TiffByteOrder::BigEndian,
    };
    let sanitize_rule = rules
        .sanitize
        .ok_or_else(|| refused("Sanitize source is unsupported"))?;
    let conv_rule = rules
        .conv_inv
        .ok_or_else(|| refused("ConvInv source is unsupported"))?;
    let formats = rules
        .formats
        .ok_or_else(|| refused("native format registry is unresolved"))?;
    let mut addressed = Vec::new();
    let mut edits = Vec::new();
    let mut warnings = Vec::new();
    for (final_rule, requested_value) in selected {
        let row = conversion_row(final_rule, rules.rows)?;
        let ifd = physical_ifd(final_rule.physical_write_group)?;
        let identity = (ifd, final_rule.raw_tag_id);
        if addressed.contains(&identity) {
            return Err(refused("multiple requests address the same physical tag"));
        }
        addressed.push(identity);
        let existing = scan
            .entries
            .iter()
            .filter(|entry| entry.ifd == ifd && entry.tag_id == final_rule.raw_tag_id)
            .count();
        if existing > 1 {
            return Err(refused("duplicate existing physical tag entries"));
        }
        let operation = if existing == 0 {
            ScalarWriteOperation::Create
        } else {
            ScalarWriteOperation::Update
        };
        let value = sanitize(
            sanitize_rule,
            SanitizeInput::Direct(requested_value),
            SanitizeOptions {
                encode_hangs: false,
                escape: EscapeOption::Disabled,
            },
        )?;
        let converted = conv_inv_static(conv_rule, value, row, rules.checks, None, None)?;
        if let Some(error) = converted.error {
            // Native SetNewValue checks definedness first: false defined
            // errors go to WriteAlso without setting this value. They must
            // never reach serialization, nor turn an existing tag into Delete.
            if error.is_empty() || error == "0" {
                continue;
            }
            return Err(refused(&error));
        }
        match resolve_tiff_scalar_final_stage(
            final_rule,
            formats,
            converted.value,
            operation,
            byte_order,
        )? {
            ResolvedTiffScalarEdit::NoEdit => {}
            ResolvedTiffScalarEdit::NoOverwrite { native_warning } => warnings.push(native_warning),
            ResolvedTiffScalarEdit::Delete { raw_tag_id } => edits.push(ScopedEntryEdit {
                ifd,
                tag_id: raw_tag_id,
                mutation: EntryMutation::Delete,
            }),
            ResolvedTiffScalarEdit::Write {
                raw_tag_id,
                wire_format,
                count,
                entry,
                out_of_line_value,
            } => {
                let native_format = formats.resolve(final_rule.wire_format)?;
                let carrier_width = ExifType::from_u16(wire_format)
                    .ok_or_else(|| refused("native TIFF format has no carrier support"))?
                    .size_in_bytes();
                if usize::try_from(native_format.size).ok() != Some(carrier_width) {
                    return Err(refused(
                        "native format width differs from TIFF carrier support",
                    ));
                }
                let size = usize::try_from(count)
                    .ok()
                    .and_then(|count| count.checked_mul(carrier_width))
                    .ok_or_else(|| refused("encoded TIFF value length overflows"))?;
                let bytes = match out_of_line_value {
                    Some(bytes) if bytes.len() == size && size > 4 => bytes,
                    None if size <= 4 => entry[8..8 + size].to_vec(),
                    _ => {
                        return Err(refused(
                            "final scalar length does not match TIFF type/count",
                        ));
                    }
                };
                edits.push(ScopedEntryEdit {
                    ifd,
                    tag_id: raw_tag_id,
                    mutation: EntryMutation::Set {
                        field_type: wire_format,
                        count,
                        bytes,
                    },
                });
            }
        }
    }
    Ok(ScalarWriteOutput {
        bytes: apply_entry_edits(file, &edits)?,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicitly invoked by the native comparison instrument after building
    /// this exact checkout. Normal tests report it ignored without inputs.
    #[test]
    #[ignore = "requires OXIDEX_SCALAR_WRITE_REQUESTS and OXIDEX_SCALAR_WRITE_RESULTS"]
    fn generated_scalar_fixture_driver() {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            input: std::path::PathBuf,
            output: std::path::PathBuf,
            carrier: Option<String>,
            key: String,
            scalar: String,
            value: Option<String>,
        }
        fn apply(request: &Request) -> std::result::Result<Vec<String>, String> {
            let value = match (request.scalar.as_str(), request.value.as_deref()) {
                ("undefined", None) => Scalar::Undefined,
                ("utf8", Some(value)) => Scalar::Utf8(value.to_owned()),
                ("bytes", Some(hex)) if hex.is_ascii() && hex.len().is_multiple_of(2) => {
                    Scalar::Bytes(
                        (0..hex.len())
                            .step_by(2)
                            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16))
                            .collect::<std::result::Result<Vec<_>, _>>()
                            .map_err(|error| error.to_string())?,
                    )
                }
                _ => return Err("invalid typed scalar request".into()),
            };
            let input = std::fs::read(&request.input).map_err(|error| error.to_string())?;
            let requests = vec![ScalarWriteRequest {
                key: &request.key,
                value,
            }];
            let result = match request.carrier.as_deref() {
                None | Some("tiff_little" | "tiff_big") => {
                    rewrite_generated_scalars(&input, requests, &generated_rules())
                }
                Some("jpeg") => crate::writers::jpeg_writer::rewrite_generated_exif_scalars(
                    &crate::test_support::TestReader::new(input),
                    requests,
                    &generated_rules(),
                ),
                _ => return Err("unsupported test carrier".into()),
            }
            .map_err(|error| error.to_string())?;
            std::fs::write(&request.output, result.bytes).map_err(|error| error.to_string())?;
            Ok(result.warnings)
        }
        let input = std::env::var("OXIDEX_SCALAR_WRITE_REQUESTS")
            .expect("explicit input requests required");
        let output =
            std::env::var("OXIDEX_SCALAR_WRITE_RESULTS").expect("explicit result path required");
        let requests: Vec<Request> =
            serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
        assert!(!requests.is_empty(), "empty fixture driver is not evidence");
        let results: Vec<_> = requests
            .iter()
            .map(|request| match apply(request) {
                Ok(warnings) => {
                    serde_json::json!({"output": request.output, "ok": true, "warnings": warnings})
                }
                Err(error) => {
                    serde_json::json!({"output": request.output, "ok": false, "error": error})
                }
            })
            .collect();
        std::fs::write(output, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    }

    fn host() -> TiffScalarFinalStageRecipe {
        *final_rule("IFD0:HostComputer", generated_rules().finals).unwrap()
    }

    #[test]
    fn resolved_public_identity_joins_sources_before_editing() {
        let rules = generated_rules();
        let rule = host();
        let id = rule.raw_tag_id.to_string();
        let request = || ResolvedScalarWriteRequest {
            module: rule.module,
            table: rule.table,
            full_name: rule.full_name,
            raw_id: &id,
            name: rule.tag_name,
            write_group: rule.physical_write_group,
            write_proc_source_sha256: rule.write_proc_source_sha256,
            registry_source_sha256: rule.registry_source_sha256,
            writer_source_sha256: rule.writer_source_sha256,
            value: Scalar::Bytes(b"resolved".to_vec()),
        };
        let empty = b"II\x2a\x00\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let resolved = rewrite_resolved_generated_scalars(empty, vec![request()], &rules).unwrap();
        let named = rewrite_generated_scalars(
            empty,
            vec![ScalarWriteRequest {
                key: rule.tag_name,
                value: Scalar::Bytes(b"resolved".to_vec()),
            }],
            &rules,
        )
        .unwrap();
        assert_eq!(resolved.bytes, named.bytes);
        for changed in [
            ResolvedScalarWriteRequest {
                module: "Other",
                ..request()
            },
            ResolvedScalarWriteRequest {
                table: "Other",
                ..request()
            },
            ResolvedScalarWriteRequest {
                full_name: "Other::Main",
                ..request()
            },
            ResolvedScalarWriteRequest {
                raw_id: "0",
                ..request()
            },
            ResolvedScalarWriteRequest {
                name: "Other",
                ..request()
            },
            ResolvedScalarWriteRequest {
                write_group: "Other",
                ..request()
            },
            ResolvedScalarWriteRequest {
                write_proc_source_sha256: "different",
                ..request()
            },
            ResolvedScalarWriteRequest {
                registry_source_sha256: "different",
                ..request()
            },
            ResolvedScalarWriteRequest {
                writer_source_sha256: "different",
                ..request()
            },
        ] {
            // Invalid carrier deliberately proves identity refusal precedes byte editing.
            let err = rewrite_resolved_generated_scalars(&[], vec![changed], &rules).unwrap_err();
            assert!(
                err.to_string()
                    .contains("no matching final source identity"),
                "{err}"
            );
        }
    }

    #[test]
    fn false_defined_conversion_errors_preserve_absent_and_existing_tags() {
        use crate::writers::generated_checkexif::{Property, PropertyValue};
        const PROPERTIES: &[Property<'static>] = &[
            Property {
                name: "Name",
                value: PropertyValue::Text("HostComputer"),
            },
            Property {
                name: "Writable",
                value: PropertyValue::Text("string"),
            },
            Property {
                name: "Count",
                value: PropertyValue::Integer(1),
            },
        ];
        let base = generated_rules();
        let empty = b"II\x2a\x00\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let seeded = rewrite_generated_scalars(
            empty,
            vec![ScalarWriteRequest {
                key: "IFD0:HostComputer",
                value: Scalar::Bytes(b"seed-host".to_vec()),
            }],
            &base,
        )
        .unwrap()
        .bytes;
        let rows = [StaticConvInvRow {
            tag_properties: PROPERTIES,
            ..*conversion_row(&host(), base.rows).unwrap()
        }];
        for error in ["", "0"] {
            let checks: Vec<_> = base
                .checks
                .iter()
                .map(|check| CheckExifRecipe {
                    check_value: crate::writers::generated_scalar::ScalarCheckRecipe {
                        first_error: error,
                        second_error: error,
                        ..check.check_value
                    },
                    ..*check
                })
                .collect();
            let changed = ScalarWriteRules {
                rows: &rows,
                checks: &checks,
                ..generated_rules()
            };
            for file in [empty.as_slice(), seeded.as_slice()] {
                let result = rewrite_generated_scalars(
                    file,
                    vec![ScalarWriteRequest {
                        key: "EXIF:HostComputer",
                        value: Scalar::Bytes(b"too-long".to_vec()),
                    }],
                    &changed,
                )
                .unwrap();
                assert_eq!(
                    result.bytes, file,
                    "false defined error {error:?} must quietly keep file"
                );
                assert!(result.warnings.is_empty());
            }
        }
    }

    #[test]
    fn source_join_refuses_cross_table_and_duplicate_rows() {
        let rules = generated_rules();
        let final_rule = host();
        let row = *conversion_row(&final_rule, rules.rows).unwrap();
        for changed in [
            StaticConvInvRow {
                module: "Other",
                ..row
            },
            StaticConvInvRow {
                table: "Other",
                ..row
            },
            StaticConvInvRow {
                full_name: "Image::ExifTool::Other::Main",
                ..row
            },
            StaticConvInvRow {
                raw_id: "317",
                ..row
            },
            StaticConvInvRow {
                name: "Other",
                ..row
            },
            StaticConvInvRow {
                write_group: "ExifIFD",
                ..row
            },
        ] {
            assert!(conversion_row(&final_rule, &[changed]).is_err());
        }
        assert!(conversion_row(&final_rule, &[row, row]).is_err());
    }

    #[test]
    fn source_names_and_groups_select_one_identity() {
        let rule = host();
        for key in ["HostComputer", "EXIF:HostComputer", "IFD0:HostComputer"] {
            assert_eq!(final_rule(key, &[rule]).unwrap(), &rule);
        }
        assert!(final_rule("GPS:HostComputer", &[rule]).is_err());
        assert!(final_rule("EXIF:HostComputer", &[rule, rule]).is_err());
        let renamed = TiffScalarFinalStageRecipe {
            tag_name: "SourceRenamedTag",
            ..rule
        };
        assert!(final_rule("EXIF:HostComputer", &[renamed]).is_err());
        assert!(final_rule("EXIF:SourceRenamedTag", &[renamed]).is_ok());
    }
}
