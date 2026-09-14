//! Public-operation planning over the generated migration ledger.
//!
//! This prepares a complete transaction before any writer or file mutation.
//! Container execution is separate; a source-owned but never migrated tag
//! remains on its existing route. Historical public migrations never fall back.
use super::generated_scalar::Scalar;
use super::generated_setnewvalue_address_rules::StaticSetNewValueAddress;
use super::generated_setnewvalue_public_migration_rules::{
    PUBLIC_SET_NEW_VALUE_MIGRATION_CAPTURE, PUBLIC_SET_NEW_VALUE_MIGRATIONS,
    StaticPublicSetNewValueMigration as Migration,
};
use super::generated_write_address::{self, AddressRules, Resolution};
use super::tiff_surgical::generated_scalar::ResolvedScalarWriteRequest;
use crate::core::{metadata_map::MetadataMap, tag_value::TagValue};
use crate::error::{ExifToolError, Result};

fn refused(reason: &str) -> ExifToolError {
    ExifToolError::unsupported_format(format!("generated public write refused: {reason}"))
}

fn raw_id(text: &str) -> Option<u16> {
    match text.strip_prefix("0x") {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn same_identity(row: &StaticSetNewValueAddress, entry: &Migration) -> bool {
    row.module == entry.module
        && row.table == entry.table
        && row.full_name == entry.full_name
        && raw_id(row.raw_id) == Some(entry.raw_tag_id)
        && row.name == entry.name
        && row.group0 == entry.group0
        && row.group1 == entry.group1
        && row.write_group == entry.write_group
}

/// Classify spelling ownership without admitting unsupported syntax. Language,
/// conversion and compound-group variants of a migrated name must still reach
/// the generated resolver's explicit refusal, never the manual lookup.
fn owns_name(key: &str, entry: &Migration) -> bool {
    let name = key.rsplit(':').next().unwrap_or(key);
    let base = name.split(['#', '-']).next().unwrap_or(name);
    base.eq_ignore_ascii_case(entry.name)
}

fn proven_external(key: &str, rules: &AddressRules<'_>) -> bool {
    let Some((group, name)) = key.split_once(':') else {
        return false;
    };
    let Some(rows) = rules.rows else {
        return false;
    };
    let mut matched = false;
    for candidate in rules.lookup.iter().filter(|candidate| {
        candidate.name.eq_ignore_ascii_case(name)
            && candidate
                .groups
                .iter()
                .any(|native| native.value.eq_ignore_ascii_case(group))
    }) {
        matched = true;
        // A fully identified native table may still belong to another writer.
        // An unmapped row in our own table is not evidence for legacy fallback.
        if candidate.row_index.is_some()
            || (candidate.source_identity_present
                && (candidate.module.is_none()
                    || candidate.table.is_none()
                    || candidate.full_name.is_none()
                    || rows.iter().any(|row| {
                        candidate.module == Some(row.module)
                            && candidate.table == Some(row.table)
                            && candidate.full_name == Some(row.full_name)
                    })))
        {
            return false;
        }
    }
    matched
}

pub(crate) fn resolve_public<'a>(
    key: &str,
    rules: &AddressRules<'a>,
    migrations: &[Migration],
) -> Resolution<'a> {
    let owned = migrations.iter().any(|entry| owns_name(key, entry));
    if !owned {
        return Resolution::OutsideMigratedScope;
    }
    match generated_write_address::resolve(key, rules) {
        Resolution::Resolved(row) => {
            let mut matches = migrations.iter().filter(|entry| same_identity(&row, entry));
            match (matches.next(), matches.next()) {
                (Some(entry), None) if !entry.removed_or_unsupported => Resolution::Resolved(row),
                (Some(_), _) => Resolution::Unsupported(
                    "migrated identity is removed, unsupported or ambiguous",
                ),
                (None, _) => {
                    Resolution::Unsupported("migrated spelling has no current composed identity")
                }
            }
        }
        Resolution::Unsupported(reason) => Resolution::Unsupported(reason),
        Resolution::OutsideMigratedScope if proven_external(key, rules) => {
            Resolution::OutsideMigratedScope
        }
        Resolution::OutsideMigratedScope => {
            Resolution::Unsupported("previously migrated spelling has no current address")
        }
    }
}

fn validate_capture() -> Result<()> {
    let address = super::generated_setnewvalue_address_rules::SET_NEW_VALUE_ADDRESS_CAPTURE
        .ok_or_else(|| refused("selected address capture is absent"))?;
    let migration = &PUBLIC_SET_NEW_VALUE_MIGRATION_CAPTURE;
    if address.exiftool_version != migration.exiftool_version
        || address.main_source_sha256 != migration.main_source_sha256
        || address.write_exif_source_sha256 != migration.write_exif_source_sha256
        || address.writer_source_sha256 != migration.writer_source_sha256
        || address.exif_source_sha256 != migration.exif_source_sha256
    {
        return Err(refused("migration and address source captures differ"));
    }
    Ok(())
}

pub(crate) struct PublicWritePlan {
    pub generated: Vec<ResolvedScalarWriteRequest<'static>>,
    /// Generated changes are reset to their original values in this map so
    /// the legacy planner cannot perform the same tag operation a second time.
    pub legacy_metadata: MetadataMap,
    pub legacy_removed: Vec<String>,
    pub has_legacy_changes: bool,
    /// Preserve a whole-EXIF clear before generated rows are masked from legacy.
    pub whole_exif_clear: bool,
}

pub(crate) fn plan_public_write(
    baseline: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
) -> Result<PublicWritePlan> {
    // The public clear operation supplies an empty replacement map without
    // named removals. A targeted delete may also leave the map empty, but must
    // preserve entries the reader did not expose (including IFD1 defaults).
    let whole_exif_clear = desired.is_empty() && removed.is_empty();
    if whole_exif_clear {
        // Clearing the carrier is not a series of individual tag writes. Do
        // not resolve an unsupported spelling merely to remove the carrier.
        return Ok(PublicWritePlan {
            generated: Vec::new(),
            legacy_metadata: desired.clone(),
            legacy_removed: Vec::new(),
            has_legacy_changes: true,
            whole_exif_clear,
        });
    }
    let rules = generated_write_address::generated_rules();
    let baseline_rows: Vec<_> = baseline
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect();
    let desired_rows: Vec<_> = desired
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect();
    let removals: Vec<_> = removed.iter().map(String::as_str).collect();
    let plan = generated_write_address::plan_metadata_delta_with(
        &baseline_rows,
        &desired_rows,
        &removals,
        |key| resolve_public(key, &rules, PUBLIC_SET_NEW_VALUE_MIGRATIONS),
    )
    .map_err(refused)?;
    if !plan.generated.is_empty() {
        validate_capture()?;
    }
    let mut generated = Vec::new();
    let finals = super::tiff_surgical::generated_scalar::generated_rules();
    for (row, value) in &plan.generated {
        let entry = PUBLIC_SET_NEW_VALUE_MIGRATIONS
            .iter()
            .find(|entry| same_identity(&row, entry))
            .ok_or_else(|| refused("planned identity is absent from migration ledger"))?;
        let mut selected = finals.finals.iter().filter(|item| {
            item.module == row.module
                && item.table == row.table
                && item.full_name == row.full_name
                && Some(item.raw_tag_id) == raw_id(row.raw_id)
                && item.tag_name == row.name
                && item.table_group0 == row.group0
                && item.physical_write_group == row.write_group
        });
        let final_rule = match (selected.next(), selected.next()) {
            (Some(item), None) if item.source_control_sha256 == entry.source_control_sha256 => item,
            _ => {
                return Err(refused(
                    "public migration does not join current final control",
                ));
            }
        };
        let numeric = super::generated_scalar_rules::NUMERIC_SCALAR
            .as_ref()
            .is_some_and(|recipe| recipe.formats.contains(&final_rule.conversion_format));
        let scalar = match value {
            None => Scalar::Undefined,
            Some(TagValue::Integer(number)) if numeric => Scalar::Utf8(number.to_string()),
            Some(TagValue::Float(number)) if numeric && number.is_finite() => {
                Scalar::Utf8(number.to_string())
            }
            Some(TagValue::Rational {
                numerator,
                denominator,
            }) if numeric => Scalar::Utf8(format!("{numerator}/{denominator}")),
            Some(TagValue::String(text)) => Scalar::Utf8(text.clone()),
            Some(TagValue::Binary(bytes)) => Scalar::Bytes(bytes.clone()),
            _ => return Err(refused("public scalar representation is not yet admitted")),
        };
        generated.push(super::generated_write_dispatch::resolved_scalar_request_at(
            row.row,
            scalar,
            row.selected_group,
        )?);
    }
    let is_generated =
        |key: &str| match resolve_public(key, &rules, PUBLIC_SET_NEW_VALUE_MIGRATIONS) {
            Resolution::Resolved(row) => plan.generated.iter().any(|(planned, _)| {
                planned.index == row.index && planned.selected_group == row.selected_group
            }),
            _ => false,
        };
    let mut legacy_metadata = desired.clone();
    for (key, _) in &desired_rows {
        if is_generated(key) {
            legacy_metadata.remove(key);
        }
    }
    for &(key, value) in &baseline_rows {
        if is_generated(key) {
            legacy_metadata.insert(key, value.clone());
        }
    }
    let legacy_removed = removed
        .iter()
        .filter(|key| !is_generated(key))
        .cloned()
        .collect();
    Ok(PublicWritePlan {
        generated,
        legacy_metadata,
        legacy_removed,
        has_legacy_changes: !plan.outside.is_empty(),
        whole_exif_clear,
    })
}

/// Execute a prepared transaction entirely in memory. Container callers commit
/// the resulting bytes once, after both legacy and generated operations succeed.
pub(crate) fn rewrite_tiff_transaction(
    bytes: &[u8],
    baseline: &MetadataMap,
    plan: PublicWritePlan,
) -> Result<Vec<u8>> {
    let legacy = if plan.has_legacy_changes {
        super::tiff_surgical::rewrite_tiff_file_with_removals(
            bytes,
            baseline,
            &plan.legacy_metadata,
            &plan.legacy_removed,
        )?
    } else {
        bytes.to_vec()
    };
    if plan.generated.is_empty() {
        return Ok(legacy);
    }
    let result = super::tiff_surgical::generated_scalar::rewrite_resolved_generated_scalars(
        &legacy,
        plan.generated,
        &super::tiff_surgical::generated_scalar::generated_rules(),
    )?;
    cleanup_source_mandatory_ifd1(&result.bytes)
}

/// Apply only the generated WriteExif mandatory-only predicate after a scalar
/// deletion has shrunk IFD1. Tag IDs and encodings come from the capture.
fn cleanup_source_mandatory_ifd1(bytes: &[u8]) -> Result<Vec<u8>> {
    use crate::writers::exif_surgical::{IfdKind, scan_exif_entries};
    use crate::writers::tiff_surgical::entry_edits::{EntryMutation, ScopedEntryEdit};
    use crate::writers::{
        generated_mandatory_defaults::MANDATORY_DEFAULTS, mandatory_defaults_runtime as mandatory,
    };
    mandatory::require_ifd1_mandatory_cleanup(&MANDATORY_DEFAULTS)
        .map_err(ExifToolError::unsupported_format)?;
    let scan = scan_exif_entries(bytes)?;
    let order = match scan.byte_order {
        crate::io::ByteOrder::LittleEndian => mandatory::TiffByteOrder::Little,
        crate::io::ByteOrder::BigEndian => mandatory::TiffByteOrder::Big,
    };
    let defaults = MANDATORY_DEFAULTS
        .directories
        .iter()
        .find(|d| d.directory == "IFD1")
        .ok_or_else(|| refused("captured mandatory IFD1 defaults missing"))?;
    let encoded =
        mandatory::encode_mandatory_defaults(&MANDATORY_DEFAULTS, defaults.defaults, order)
            .map_err(ExifToolError::unsupported_format)?;
    let edits: Vec<_> = encoded
        .into_iter()
        .map(|e| ScopedEntryEdit {
            ifd: IfdKind::Ifd1,
            tag_id: e.tag_id,
            mutation: EntryMutation::Set {
                field_type: e.tiff_type,
                count: e.count,
                bytes: e.bytes,
            },
        })
        .collect();
    super::tiff_surgical::entry_edits::remove_ifd1_if_only_mandatory(bytes, &edits, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory_only_key() -> String {
        // Choose a current source declaration outside every current or retained
        // public migration. The fixture follows the migration boundary as it grows.
        let rules = generated_write_address::generated_rules();
        rules
            .rows
            .unwrap()
            .iter()
            .map(|row| format!("{}:{}", row.group0, row.name))
            .find(|key| {
                !PUBLIC_SET_NEW_VALUE_MIGRATIONS
                    .iter()
                    .any(|entry| owns_name(key, entry))
            })
            .expect("source inventory has an unmigrated declaration")
    }

    #[test]
    fn migrated_alias_delta_keeps_legacy_and_generated_edits_separate() {
        let legacy_key = inventory_only_key();
        let mut original = MetadataMap::new();
        original.insert("IFD0:HostComputer", TagValue::new_string("old"));
        original.insert(&legacy_key, TagValue::new_string("artist"));
        let mut desired = original.clone();
        desired.remove("IFD0:HostComputer");
        desired.insert("EXIF:HostComputer", TagValue::new_string("new"));
        desired.insert(&legacy_key, TagValue::new_string("changed"));
        let plan = plan_public_write(&original, &desired, &[]).unwrap();
        assert_eq!(plan.generated.len(), 1);
        assert_eq!(plan.generated[0].value, Scalar::Utf8("new".into()));
        assert_eq!(
            plan.legacy_metadata.get("IFD0:HostComputer"),
            original.get("IFD0:HostComputer")
        );
        assert!(plan.legacy_metadata.get("EXIF:HostComputer").is_none());
        assert_eq!(
            plan.legacy_metadata.get(&legacy_key),
            desired.get(&legacy_key)
        );
        assert!(plan.has_legacy_changes);
    }

    #[test]
    fn retained_migration_refuses_after_source_rule_removal() {
        let entry = &PUBLIC_SET_NEW_VALUE_MIGRATIONS[0];
        let retired = Migration {
            removed_or_unsupported: true,
            ..*entry
        };
        let key = format!("{}:{}", entry.group0, entry.name);
        assert!(matches!(
            resolve_public(
                &key,
                &generated_write_address::generated_rules(),
                &[retired]
            ),
            Resolution::Unsupported(_)
        ));
    }

    #[test]
    fn conflicting_aliases_return_no_partial_transaction() {
        let mut desired = MetadataMap::new();
        desired.insert("EXIF:HostComputer", TagValue::new_string("one"));
        desired.insert("IFD0:HostComputer", TagValue::new_string("two"));
        desired.insert("IFD0:Artist", TagValue::new_string("legacy"));
        assert!(plan_public_write(&MetadataMap::new(), &desired, &[]).is_err());
    }

    #[test]
    fn rowless_delete_and_defined_empty_remain_distinct() {
        let original = MetadataMap::new();
        let deletion =
            plan_public_write(&original, &original, &["EXIF:HostComputer".into()]).unwrap();
        assert_eq!(deletion.generated[0].value, Scalar::Undefined);
        assert!(deletion.legacy_removed.is_empty());
        let mut desired = MetadataMap::new();
        desired.insert("EXIF:HostComputer", TagValue::new_string(""));
        let empty = plan_public_write(&original, &desired, &[]).unwrap();
        assert_eq!(empty.generated[0].value, Scalar::Utf8(String::new()));
    }

    #[test]
    fn whole_exif_clear_survives_generated_masking() {
        let mut original = MetadataMap::new();
        original.insert("EXIF:HostComputer", TagValue::new_string("old"));
        original.insert("IFD0:Artist", TagValue::new_string("legacy"));
        let plan = plan_public_write(&original, &MetadataMap::new(), &[]).unwrap();
        assert!(plan.generated.is_empty());
        assert!(plan.whole_exif_clear);

        let mut generated_only = MetadataMap::new();
        generated_only.insert("EXIF:HostComputer", TagValue::new_string("old"));
        let targeted = plan_public_write(
            &generated_only,
            &MetadataMap::new(),
            &["EXIF:HostComputer".into()],
        )
        .unwrap();
        assert!(!targeted.whole_exif_clear);
        assert_eq!(targeted.generated.len(), 1);

        let mut thumbnail_only = MetadataMap::new();
        thumbnail_only.insert(
            "IFD1:Artist",
            TagValue::new_string("retained thumbnail metadata"),
        );
        assert!(
            !plan_public_write(&thumbnail_only, &thumbnail_only, &[])
                .unwrap()
                .whole_exif_clear
        );
    }

    #[test]
    fn source_inventory_only_names_keep_legacy_routing() {
        let legacy_key = inventory_only_key();
        let mut desired = MetadataMap::new();
        desired.insert(&legacy_key, TagValue::new_string("artist"));
        let plan = plan_public_write(&MetadataMap::new(), &desired, &[]).unwrap();
        assert!(plan.generated.is_empty());
        assert!(plan.has_legacy_changes);
        assert_eq!(plan.legacy_metadata, desired);
    }

    #[test]
    fn proven_foreign_namespace_keeps_legacy_writer() {
        let mut desired = MetadataMap::new();
        desired.insert("XMP:CameraLabel", TagValue::new_string("outside"));
        let plan = plan_public_write(&MetadataMap::new(), &desired, &[]).unwrap();
        assert!(plan.generated.is_empty());
        assert_eq!(plan.legacy_metadata, desired);
    }

    #[test]
    fn unmapped_same_table_candidate_is_not_a_foreign_namespace() {
        let mut rules = generated_write_address::generated_rules();
        let candidate = rules
            .lookup
            .iter()
            .find(|candidate| {
                candidate.name.eq_ignore_ascii_case("CameraLabel")
                    && candidate.groups.iter().any(|group| group.value == "XMP")
            })
            .unwrap();
        assert!(candidate.source_identity_present);
        assert!(proven_external("XMP:CameraLabel", &rules));
        let source = rules
            .rows
            .unwrap()
            .iter()
            .find(|row| row.name == "CameraLabel")
            .unwrap();
        let lookup = [
            super::super::generated_setnewvalue_address_rules::StaticNativeLookupCandidate {
                module: Some(source.module),
                table: Some(source.table),
                full_name: Some(source.full_name),
                row_index: None,
                ..*candidate
            },
        ];
        rules.lookup = &lookup;
        assert!(!proven_external("XMP:CameraLabel", &rules));
        assert!(matches!(
            resolve_public("XMP:CameraLabel", &rules, PUBLIC_SET_NEW_VALUE_MIGRATIONS),
            Resolution::Unsupported(_)
        ));
    }

    #[test]
    fn migrated_unsupported_spellings_cannot_escape_to_legacy() {
        for key in [
            "EXIF:HostComputer#",
            "EXIF:IFD0:HostComputer",
            "HostComputer-fr",
            "UnknownGroup:HostComputer",
            "EXIF:HostComputer#-fr",
        ] {
            let mut desired = MetadataMap::new();
            desired.insert(key, TagValue::new_string("value"));
            assert!(
                plan_public_write(&MetadataMap::new(), &desired, &[]).is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn public_integer_and_rational_values_keep_source_format_and_directory() {
        for (key, value, scalar, directory) in [
            ("EXIF:XResolution", TagValue::Integer(300), "300", "IFD0"),
            (
                "IFD1:XResolution",
                TagValue::Rational {
                    numerator: 3,
                    denominator: 2,
                },
                "3/2",
                "IFD1",
            ),
            (
                "EXIF:YResolution",
                TagValue::Rational {
                    numerator: 1,
                    denominator: 0,
                },
                "1/0",
                "IFD0",
            ),
        ] {
            let mut desired = MetadataMap::new();
            desired.insert(key, value);
            let plan = plan_public_write(&MetadataMap::new(), &desired, &[]).unwrap();
            assert!(
                !plan.whole_exif_clear,
                "{key} must retain its EXIF directory"
            );
            assert!(!plan.has_legacy_changes);
            assert_eq!(plan.generated.len(), 1);
            assert_eq!(plan.generated[0].value, Scalar::Utf8(scalar.into()));
            assert_eq!(plan.generated[0].selected_group, directory);
        }
        let mut desired = MetadataMap::new();
        desired.insert("EXIF:XResolution", TagValue::Float(1.5));
        assert_eq!(
            plan_public_write(&MetadataMap::new(), &desired, &[])
                .unwrap()
                .generated[0]
                .value,
            Scalar::Utf8("1.5".into())
        );
    }
}
