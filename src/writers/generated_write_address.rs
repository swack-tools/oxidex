//! Resolve public spellings through authenticated, generated native addresses.
//!
//! This shared mechanism contains no tag names, IDs, or native defaults. The
//! source compiler currently admits ordinary EXIF/IFD0 names and supplies operands.
//! Extended native address grammar is pending; this module is not yet public routing.
//! A caller must propagate `Unsupported`; it must not retry a legacy writer.

use super::generated_setnewvalue_address_rules::{
    StaticNativeLookupCandidate, StaticSetNewValueAddress, StaticSetNewValueQualifierScope,
};

pub(crate) struct AddressRules<'a> {
    pub rows: Option<&'a [StaticSetNewValueAddress]>,
    pub qualifier_scope: &'a [StaticSetNewValueQualifierScope],
    pub lookup: &'a [StaticNativeLookupCandidate],
    /// Includes retained ownership of removed/renamed upstream names.
    pub owned_names: &'a [&'a str],
}

/// Load operands emitted from the selected native source. Missing recipes stay
/// explicit; callers must distinguish migration ownership before public routing.
pub(crate) fn generated_rules() -> AddressRules<'static> {
    use super::generated_setnewvalue_address_rules::*;
    AddressRules {
        rows: SET_NEW_VALUE_ADDRESSING,
        qualifier_scope: SET_NEW_VALUE_ADMITTED_QUALIFIER_SCOPE,
        lookup: SET_NEW_VALUE_LOOKUP,
        owned_names: SET_NEW_VALUE_OWNED_NAMES,
    }
}

pub(crate) enum Resolution<'a> {
    Resolved(&'a StaticSetNewValueAddress),
    OutsideMigratedScope,
    Unsupported(&'static str),
}

fn ordinary(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn same_physical(a: &StaticSetNewValueAddress, b: &StaticSetNewValueAddress) -> bool {
    a.module == b.module
        && a.table == b.table
        && a.full_name == b.full_name
        && a.raw_id == b.raw_id
        && a.write_group == b.write_group
}

pub(crate) fn resolve<'a>(key: &str, rules: &AddressRules<'a>) -> Resolution<'a> {
    let (group, name) = key
        .split_once(':')
        .map_or((None, key), |(group, name)| (Some(group), name));
    if !ordinary(name) || group.is_some_and(|group| !ordinary(group)) {
        return Resolution::Unsupported("native address syntax is not admitted");
    }
    let owned = rules
        .owned_names
        .iter()
        .any(|owned| owned.eq_ignore_ascii_case(name));
    let Some(rows) = rules.rows else {
        return if owned {
            Resolution::Unsupported("source-owned address rules are unavailable")
        } else {
            Resolution::OutsideMigratedScope
        };
    };
    let has_source_rows = rows.iter().any(|row| row.name.eq_ignore_ascii_case(name));
    let mut selected: Option<&StaticSetNewValueAddress> = None;
    let mut external = false;
    let mut source_identity_present = false;
    let mut matches = 0;
    for candidate in rules
        .lookup
        .iter()
        .filter(|candidate| candidate.name.eq_ignore_ascii_case(name))
    {
        if group.is_some_and(|group| {
            !candidate
                .groups
                .iter()
                .any(|native| native.value.eq_ignore_ascii_case(group))
        }) {
            continue;
        }
        matches += 1;
        source_identity_present |= candidate.source_identity_present;
        let Some(index) = candidate.row_index else {
            external = true;
            continue;
        };
        let Some(row) = rows.get(index) else {
            return Resolution::Unsupported("generated lookup index is out of bounds");
        };
        if row.index != index || !candidate.source_identity_present {
            return Resolution::Unsupported("generated lookup identity is inconsistent");
        }
        if selected.is_some_and(|prior| !same_physical(prior, row)) {
            return Resolution::Unsupported("native lookup selects multiple physical fields");
        }
        selected = Some(row);
    }
    let beyond_scope = group.is_some_and(|group| {
        !rules
            .qualifier_scope
            .iter()
            .any(|native| native.group.eq_ignore_ascii_case(group))
    });
    if beyond_scope {
        // With no admitted row for the name, an omitted source identity still
        // belongs to this migration. With admitted rows, an exclusively
        // external match is left to the other writer, as the compiler defines.
        if matches > 0 && selected.is_none() && (has_source_rows || !source_identity_present) {
            return Resolution::OutsideMigratedScope;
        }
        return if owned || has_source_rows {
            Resolution::Unsupported("qualifier is outside the source-admitted grammar")
        } else {
            Resolution::OutsideMigratedScope
        };
    }
    if !has_source_rows {
        return if owned {
            Resolution::Unsupported("source-owned name has no admitted native address")
        } else {
            Resolution::OutsideMigratedScope
        };
    }
    if external {
        return Resolution::Unsupported(
            "native lookup includes fields outside the generated route",
        );
    }
    match selected {
        Some(row) => Resolution::Resolved(row),
        None => Resolution::Unsupported("qualifier does not select a generated row"),
    }
}

pub(crate) struct AddressPlan<'a, 'k, V> {
    pub generated: Vec<(&'a StaticSetNewValueAddress, V)>,
    pub outside: Vec<(&'k str, V)>,
}

/// Plan a whole batch before any mutation. Aliases with equal values coalesce;
/// conflicting values or unsupported addresses return no partial plan.
pub(crate) fn plan_requests<'a, 'k, V: PartialEq>(
    requests: impl IntoIterator<Item = (&'k str, V)>,
    rules: &AddressRules<'a>,
) -> Result<AddressPlan<'a, 'k, V>, &'static str> {
    let mut plan = AddressPlan {
        generated: Vec::new(),
        outside: Vec::new(),
    };
    for (key, value) in requests {
        match resolve(key, rules) {
            Resolution::Resolved(row) => {
                if let Some((_, prior)) = plan
                    .generated
                    .iter()
                    .find(|(prior, _)| same_physical(prior, row))
                {
                    if *prior != value {
                        return Err("conflicting aliases address the same physical field");
                    }
                } else {
                    plan.generated.push((row, value));
                }
            }
            Resolution::OutsideMigratedScope => plan.outside.push((key, value)),
            Resolution::Unsupported(reason) => return Err(reason),
        }
    }
    Ok(plan)
}

/// Recover authored changes from a whole metadata map. Unchanged display values
/// are carried through without re-validating them as new writer input. A key
/// replaced by another spelling of the same physical field is not a deletion.
/// Explicit removals remain requests even when the reader produced no row.
pub(crate) fn plan_metadata_delta<'a, 'k, V: PartialEq>(
    baseline: &[(&'k str, &'k V)],
    desired: &[(&'k str, &'k V)],
    removed: &[&'k str],
    rules: &AddressRules<'a>,
) -> Result<AddressPlan<'a, 'k, Option<&'k V>>, &'static str> {
    let mut requests = Vec::new();
    for &(key, value) in desired {
        let unchanged = baseline.iter().any(|&(old_key, old_value)| {
            if old_value != value {
                return false;
            }
            if old_key == key {
                return true;
            }
            matches!((resolve(old_key, rules), resolve(key, rules)),
                (Resolution::Resolved(old), Resolution::Resolved(new)) if same_physical(old, new))
        });
        if !unchanged {
            requests.push((key, Some(value)));
        }
    }
    for &(key, _) in baseline {
        if desired.iter().any(|&(new_key, _)| new_key == key) {
            continue;
        }
        let replacement = match resolve(key, rules) {
            Resolution::Resolved(old) => desired.iter().any(|&(new_key, _)| {
                matches!(resolve(new_key, rules), Resolution::Resolved(new) if same_physical(old, new))
            }),
            _ => false,
        };
        if !replacement {
            requests.push((key, None));
        }
    }
    requests.extend(removed.iter().map(|&key| (key, None)));
    plan_requests(requests, rules)
}

#[cfg(test)]
mod tests {
    use super::super::generated_setnewvalue_address_rules::StaticNativeLookupFamily;
    use super::*;
    const FAMILY: &[StaticNativeLookupFamily] = &[
        StaticNativeLookupFamily {
            family: 0,
            value: "Family",
        },
        StaticNativeLookupFamily {
            family: 1,
            value: "Directory",
        },
    ];
    const EXTERNAL: &[StaticNativeLookupFamily] = &[
        StaticNativeLookupFamily {
            family: 0,
            value: "External",
        },
        StaticNativeLookupFamily {
            family: 1,
            value: "Other",
        },
    ];
    const SCOPE: &[StaticSetNewValueQualifierScope] = &[
        StaticSetNewValueQualifierScope {
            family: 0,
            group: "Family",
        },
        StaticSetNewValueQualifierScope {
            family: 1,
            group: "Directory",
        },
    ];

    const fn row(index: usize, name: &'static str, id: &'static str) -> StaticSetNewValueAddress {
        StaticSetNewValueAddress {
            index,
            module: "Fixture",
            table: "Main",
            full_name: "Image::ExifTool::Fixture::Main",
            raw_id: id,
            name,
            group0: "Family",
            group1: "Directory",
            write_group: "Directory",
        }
    }

    const fn candidate(
        index: Option<usize>,
        groups: &'static [StaticNativeLookupFamily],
    ) -> StaticNativeLookupCandidate {
        StaticNativeLookupCandidate {
            name: "field",
            row_index: index,
            source_identity_present: index.is_some(),
            groups,
        }
    }

    #[test]
    fn source_names_groups_and_case_resolve_without_a_tag_allowlist() {
        let rows = [row(0, "Field", "123")];
        let lookup = [candidate(Some(0), FAMILY)];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["field"],
        };
        for key in ["field", "FiElD", "FAMILY:Field", "directory:FIELD"] {
            assert!(
                matches!(resolve(key, &rules), Resolution::Resolved(row) if row.raw_id == "123")
            );
        }
        assert!(matches!(
            resolve("Unknown", &rules),
            Resolution::OutsideMigratedScope
        ));
        assert!(matches!(
            resolve("Wrong:Field", &rules),
            Resolution::Unsupported(_)
        ));
        assert!(matches!(
            resolve("Directory:Field:Extra", &rules),
            Resolution::Unsupported(_)
        ));
    }

    #[test]
    fn qualified_external_names_do_not_hide_unqualified_ambiguity() {
        let rows = [row(0, "Field", "123")];
        let lookup = [candidate(Some(0), FAMILY), candidate(None, EXTERNAL)];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["field"],
        };
        assert!(matches!(
            resolve("Field", &rules),
            Resolution::Unsupported(_)
        ));
        assert!(matches!(
            resolve("Family:Field", &rules),
            Resolution::Resolved(_)
        ));
        assert!(matches!(
            resolve("External:Field", &rules),
            Resolution::OutsideMigratedScope
        ));
    }

    #[test]
    fn missing_rules_and_retired_names_cannot_fall_back() {
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: None,
            lookup: &[],
            owned_names: &["retired"],
        };
        assert!(matches!(
            resolve("Retired", &rules),
            Resolution::Unsupported(_)
        ));
        let rows = [row(0, "Renamed", "456")];
        let lookup = [StaticNativeLookupCandidate {
            name: "renamed",
            row_index: Some(0),
            source_identity_present: true,
            groups: FAMILY,
        }];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["retired", "renamed"],
        };
        assert!(matches!(
            resolve("Retired", &rules),
            Resolution::Unsupported(_)
        ));
        assert!(
            matches!(resolve("Renamed", &rules), Resolution::Resolved(row) if row.raw_id == "456")
        );
    }

    #[test]
    fn duplicate_physical_candidates_and_invalid_indexes_refuse() {
        let rows = [row(0, "Field", "123"), row(1, "Field", "456")];
        let lookup = [candidate(Some(0), FAMILY), candidate(Some(1), FAMILY)];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["field"],
        };
        assert!(matches!(
            resolve("Field", &rules),
            Resolution::Unsupported(_)
        ));
        let lookup = [candidate(Some(2), FAMILY)];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["field"],
        };
        assert!(matches!(
            resolve("Field", &rules),
            Resolution::Unsupported(_)
        ));
    }

    #[test]
    fn batch_aliases_coalesce_and_conflicts_return_no_partial_plan() {
        let rows = [row(0, "Field", "123")];
        let lookup = [candidate(Some(0), FAMILY)];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["field"],
        };
        let plan = plan_requests(
            [
                ("Field", Some("")),
                ("Family:Field", Some("")),
                ("Unknown", Some("carry")),
            ],
            &rules,
        )
        .unwrap();
        assert_eq!(plan.generated.len(), 1);
        assert_eq!(plan.generated[0].1, Some(""));
        assert_eq!(plan.outside, vec![("Unknown", Some("carry"))]);
        // Defined empty and deletion are distinct, even through aliases.
        assert!(plan_requests([("Field", Some("")), ("Family:Field", None)], &rules).is_err());
        assert!(plan_requests([("Field", Some("value")), ("Bad:Field", None)], &rules).is_err());
    }
    #[test]
    fn whole_map_alias_replacement_is_an_update_not_a_delete() {
        let rows = [row(0, "Field", "123")];
        let lookup = [candidate(Some(0), FAMILY)];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["field"],
        };
        let plan = plan_metadata_delta(
            &[("Directory:Field", &"old")],
            &[("Family:Field", &"new")],
            &[],
            &rules,
        )
        .unwrap();
        assert_eq!(plan.generated.len(), 1);
        assert_eq!(plan.generated[0].1, Some(&"new"));
        assert!(plan.outside.is_empty());
    }

    #[test]
    fn unchanged_unsupported_display_values_do_not_become_authored_input() {
        let rows = [row(0, "Field", "123")];
        let lookup = [candidate(Some(0), FAMILY)];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["field", "retired"],
        };
        let baseline = [("Retired", &"old display")];
        let desired = [("Retired", &"old display"), ("Field", &"")];
        let plan = plan_metadata_delta(&baseline, &desired, &[], &rules).unwrap();
        assert_eq!(plan.generated[0].1, Some(&""));
        assert!(plan_metadata_delta(&baseline, &[], &[], &rules).is_err());
    }

    #[test]
    fn explicit_rowless_deletion_and_conflicting_aliases_are_preserved() {
        let rows = [row(0, "Field", "123")];
        let lookup = [candidate(Some(0), FAMILY)];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["field"],
        };
        let plan = plan_metadata_delta::<&str>(&[], &[], &["Family:Field"], &rules).unwrap();
        assert_eq!(plan.generated.len(), 1);
        assert_eq!(plan.generated[0].1, None);
        assert!(
            plan_metadata_delta(
                &[],
                &[("Field", &"one"), ("Family:Field", &"two")],
                &[],
                &rules
            )
            .is_err()
        );
        assert!(plan_metadata_delta(&[], &[("Field", &"")], &["Family:Field"], &rules).is_err());
    }
    #[test]
    fn same_value_alias_replacement_is_not_an_authored_write() {
        let rows = [row(0, "Field", "123")];
        let lookup = [candidate(Some(0), FAMILY)];
        let rules = AddressRules {
            qualifier_scope: SCOPE,
            rows: Some(&rows),
            lookup: &lookup,
            owned_names: &["field"],
        };
        let plan = plan_metadata_delta(
            &[("Field", &"old")],
            &[("Family:Field", &"old")],
            &[],
            &rules,
        )
        .unwrap();
        assert!(plan.generated.is_empty());
        assert!(plan.outside.is_empty());
    }
}
