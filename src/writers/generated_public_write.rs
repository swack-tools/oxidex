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
    rules.lookup.iter().any(|candidate| {
        !candidate.source_identity_present
            && candidate.name.eq_ignore_ascii_case(name)
            && candidate
                .groups
                .iter()
                .any(|native| native.value.eq_ignore_ascii_case(group))
    })
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
            let mut matches = migrations.iter().filter(|entry| same_identity(row, entry));
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
    let whole_exif_clear = desired.iter().all(|(key, _)| {
        !matches!(
            key.split_once(':').map(|(group, _)| group),
            Some("EXIF" | "IFD0" | "IFD1" | "ExifIFD" | "GPS" | "InteropIFD")
        )
    });
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
            .find(|entry| same_identity(row, entry))
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
        if !matches!((selected.next(), selected.next()), (Some(item), None) if item.source_control_sha256 == entry.source_control_sha256)
        {
            return Err(refused(
                "public migration does not join current final control",
            ));
        }
        let scalar = match value {
            None => Scalar::Undefined,
            Some(TagValue::String(text)) => Scalar::Utf8(text.clone()),
            Some(TagValue::Binary(bytes)) => Scalar::Bytes(bytes.clone()),
            _ => return Err(refused("public scalar representation is not yet admitted")),
        };
        generated.push(super::generated_write_dispatch::resolved_scalar_request(
            row, scalar,
        )?);
    }
    let is_generated =
        |key: &str| match resolve_public(key, &rules, PUBLIC_SET_NEW_VALUE_MIGRATIONS) {
            Resolution::Resolved(row) => plan
                .generated
                .iter()
                .any(|(planned, _)| planned.index == row.index),
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
    Ok(result.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrated_alias_delta_keeps_legacy_and_generated_edits_separate() {
        let mut original = MetadataMap::new();
        original.insert("IFD0:HostComputer", TagValue::new_string("old"));
        original.insert("IFD0:Artist", TagValue::new_string("artist"));
        let mut desired = original.clone();
        desired.remove("IFD0:HostComputer");
        desired.insert("EXIF:HostComputer", TagValue::new_string("new"));
        desired.insert("IFD0:Artist", TagValue::new_string("changed"));
        let plan = plan_public_write(&original, &desired, &[]).unwrap();
        assert_eq!(plan.generated.len(), 1);
        assert_eq!(plan.generated[0].value, Scalar::Utf8("new".into()));
        assert_eq!(
            plan.legacy_metadata.get("IFD0:HostComputer"),
            original.get("IFD0:HostComputer")
        );
        assert!(plan.legacy_metadata.get("EXIF:HostComputer").is_none());
        assert_eq!(
            plan.legacy_metadata.get("IFD0:Artist"),
            desired.get("IFD0:Artist")
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
        let plan = plan_public_write(
            &original,
            &MetadataMap::new(),
            &["EXIF:HostComputer".into()],
        )
        .unwrap();
        assert!(!plan.generated.is_empty());
        assert!(plan.whole_exif_clear);
    }

    #[test]
    fn source_inventory_only_names_keep_legacy_routing() {
        let mut desired = MetadataMap::new();
        desired.insert("IFD0:Artist", TagValue::new_string("artist"));
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
}
