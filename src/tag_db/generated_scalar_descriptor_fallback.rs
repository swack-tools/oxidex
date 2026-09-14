//! Generic descriptor and reverse-name facts composed from generated writer operands.
//!
//! This is deliberately a consumer of the three already-published writer
//! artifacts: a public migration must still bind one address row and one final
//! scalar recipe under the same captured native sources.  It contains no tag
//! names, numeric IDs, or handwritten type choices.  If an upgraded generated
//! artifact cannot satisfy that complete join, that public migration becomes
//! terminal rather than retaining an older descriptor or reverse spelling.

use std::sync::LazyLock;

use crate::core::FormatFamily;
use crate::writers::generated_setnewvalue_address_rules::{
    SET_NEW_VALUE_ADDRESS_CAPTURE, SET_NEW_VALUE_ADDRESS_ROWS, StaticSetNewValueAddress,
};
use crate::writers::generated_setnewvalue_public_migration_rules::{
    PUBLIC_SET_NEW_VALUE_MIGRATION_CAPTURE, PUBLIC_SET_NEW_VALUE_MIGRATIONS,
    StaticPublicSetNewValueMigration,
};
use crate::writers::generated_tiff_scalar_final_rules::{
    TIFF_SCALAR_FINAL_FORMAT_REGISTRY, TIFF_SCALAR_FINAL_RECIPES,
};
use crate::writers::tiff_scalar_final_stage::{
    NativeTiffFormatRegistry, TiffScalarFinalStageRecipe,
};

/// A descriptor class is admitted only after the final source recipe selected
/// the corresponding native TIFF format.  The current scalar compiler can
/// authenticate only the source literal `string`; a later literal must make
/// that migration terminal until a source-derived mapping exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceValueClass {
    String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GeneratedScalarDescriptorFact {
    pub raw_tag_id: u16,
    pub name: &'static str,
    pub group0: &'static str,
    /// The exact physical source group.  Reverse lookup is scoped to it.
    pub physical_group: &'static str,
    pub value_class: SourceValueClass,
    pub source_control_sha256: &'static str,
    pub semantics_sha256: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TerminalScalarIdentity {
    raw_tag_id: u16,
    name: &'static str,
    group0: &'static str,
    physical_group: &'static str,
}

#[derive(Debug)]
struct ComposedFacts {
    current: Vec<GeneratedScalarDescriptorFact>,
    terminal: Vec<TerminalScalarIdentity>,
}

fn raw_id(text: &str) -> Option<u16> {
    match text.strip_prefix("0x") {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn same_capture() -> bool {
    let Some(address) = SET_NEW_VALUE_ADDRESS_CAPTURE else {
        return false;
    };
    let migration = &PUBLIC_SET_NEW_VALUE_MIGRATION_CAPTURE;
    address.exiftool_version == migration.exiftool_version
        && address.main_source_sha256 == migration.main_source_sha256
        && address.write_exif_source_sha256 == migration.write_exif_source_sha256
        && address.writer_source_sha256 == migration.writer_source_sha256
        && address.exif_source_sha256 == migration.exif_source_sha256
}

fn same_address(
    row: &StaticSetNewValueAddress,
    migration: &StaticPublicSetNewValueMigration,
) -> bool {
    row.module == migration.module
        && row.table == migration.table
        && row.full_name == migration.full_name
        && raw_id(row.raw_id) == Some(migration.raw_tag_id)
        && row.name == migration.name
        && row.group0 == migration.group0
        && row.group1 == migration.group1
        && row.write_group == migration.write_group
}

fn same_final(
    recipe: &TiffScalarFinalStageRecipe,
    migration: &StaticPublicSetNewValueMigration,
    registry: NativeTiffFormatRegistry,
) -> bool {
    recipe.module == migration.module
        && recipe.table == migration.table
        && recipe.full_name == migration.full_name
        && recipe.raw_tag_id == migration.raw_tag_id
        && recipe.tag_name == migration.name
        && recipe.table_group0 == migration.group0
        && recipe.physical_write_group == migration.write_group
        && recipe.source_control_sha256 == migration.source_control_sha256
        && recipe.write_proc_source_sha256
            == PUBLIC_SET_NEW_VALUE_MIGRATION_CAPTURE.write_exif_source_sha256
        && recipe.writer_source_sha256
            == PUBLIC_SET_NEW_VALUE_MIGRATION_CAPTURE.writer_source_sha256
        && recipe.registry_source_sha256
            == PUBLIC_SET_NEW_VALUE_MIGRATION_CAPTURE.exif_source_sha256
        && registry.source_file == "Image/ExifTool/Exif.pm"
        && registry.source_sha256 == recipe.registry_source_sha256
        && registry
            .facts
            .iter()
            .any(|fact| fact.name == recipe.wire_format && fact.number != 0 && fact.size != 0)
        && recipe.write_value.is_some()
}

fn source_value_class(recipe: &TiffScalarFinalStageRecipe) -> Option<SourceValueClass> {
    match (recipe.conversion_format, recipe.wire_format) {
        ("string", "string") => Some(SourceValueClass::String),
        _ => None,
    }
}

fn terminal_identity(migration: &StaticPublicSetNewValueMigration) -> TerminalScalarIdentity {
    TerminalScalarIdentity {
        raw_tag_id: migration.raw_tag_id,
        name: migration.name,
        group0: migration.group0,
        physical_group: migration.write_group,
    }
}

fn terminal_projection(migrations: &[StaticPublicSetNewValueMigration]) -> ComposedFacts {
    ComposedFacts {
        current: Vec::new(),
        terminal: migrations.iter().map(terminal_identity).collect(),
    }
}

fn compose_with_capture(
    capture_valid: bool,
    address_rows: Option<&[StaticSetNewValueAddress]>,
    migrations: &[StaticPublicSetNewValueMigration],
    recipes: &[TiffScalarFinalStageRecipe],
    registry: Option<NativeTiffFormatRegistry>,
) -> Option<ComposedFacts> {
    if migrations.is_empty() {
        return Some(terminal_projection(migrations));
    }
    // A global capture or operand failure must not revive legacy descriptor or
    // reverse-name facts for a public migrated identity.  It is terminal for
    // the generated migration set only; unrelated legacy names still resolve.
    if !capture_valid || address_rows.is_none() || registry.is_none() {
        return Some(terminal_projection(migrations));
    }
    let address_rows = address_rows.expect("checked above");
    let registry = registry.expect("checked above");
    let mut current: Vec<GeneratedScalarDescriptorFact> = Vec::with_capacity(migrations.len());
    let mut terminal = Vec::new();
    for migration in migrations {
        // Historical public ownership is terminal only for its own identity.
        // Do not republish a stale descriptor, but do not make one removed row
        // suppress independently authenticated current migrations.
        if migration.removed_or_unsupported
            || migration.group0 != "EXIF"
            || migration.group1 != migration.write_group
        {
            terminal.push(terminal_identity(migration));
            continue;
        }
        let addresses: Vec<_> = address_rows
            .iter()
            .filter(|row| same_address(row, migration))
            .collect();
        let finals: Vec<_> = recipes
            .iter()
            .filter(|recipe| same_final(recipe, migration, registry))
            .collect();
        let (Some(address), Some(recipe)) = (addresses.first(), finals.first()) else {
            terminal.push(terminal_identity(migration));
            continue;
        };
        if addresses.len() != 1
            || finals.len() != 1
            || address.write_group != recipe.physical_write_group
        {
            terminal.push(terminal_identity(migration));
            continue;
        }
        let Some(value_class) = source_value_class(recipe) else {
            terminal.push(terminal_identity(migration));
            continue;
        };
        if let Some(index) = current.iter().position(|fact| {
            (fact.raw_tag_id == migration.raw_tag_id
                && fact.physical_group == migration.write_group)
                || (fact.group0 == migration.group0
                    && fact.name.eq_ignore_ascii_case(migration.name))
        }) {
            let previous = current.remove(index);
            terminal.push(TerminalScalarIdentity {
                raw_tag_id: previous.raw_tag_id,
                name: previous.name,
                group0: previous.group0,
                physical_group: previous.physical_group,
            });
            terminal.push(terminal_identity(migration));
            continue;
        }
        current.push(GeneratedScalarDescriptorFact {
            raw_tag_id: migration.raw_tag_id,
            name: migration.name,
            group0: migration.group0,
            physical_group: migration.write_group,
            value_class,
            source_control_sha256: migration.source_control_sha256,
            semantics_sha256: migration.semantics_sha256,
        });
    }
    Some(ComposedFacts { current, terminal })
}

fn compose(
    address_rows: Option<&[StaticSetNewValueAddress]>,
    migrations: &[StaticPublicSetNewValueMigration],
    recipes: &[TiffScalarFinalStageRecipe],
    registry: Option<NativeTiffFormatRegistry>,
) -> Option<ComposedFacts> {
    compose_with_capture(same_capture(), address_rows, migrations, recipes, registry)
}

fn terminal_descriptor_name_in(facts: &ComposedFacts, name: &str) -> bool {
    let Some((group0, name)) = name.split_once(':') else {
        return false;
    };
    facts
        .terminal
        .iter()
        .any(|fact| fact.group0 == group0 && fact.name.eq_ignore_ascii_case(name))
}

fn terminal_reverse_in(facts: &ComposedFacts, raw_tag_id: u16, physical_group: &str) -> bool {
    facts
        .terminal
        .iter()
        .any(|fact| fact.raw_tag_id == raw_tag_id && fact.physical_group == physical_group)
}

static CURRENT: LazyLock<Option<ComposedFacts>> = LazyLock::new(|| {
    compose(
        Some(SET_NEW_VALUE_ADDRESS_ROWS),
        PUBLIC_SET_NEW_VALUE_MIGRATIONS,
        TIFF_SCALAR_FINAL_RECIPES,
        TIFF_SCALAR_FINAL_FORMAT_REGISTRY,
    )
});

pub(crate) fn facts() -> Option<&'static [GeneratedScalarDescriptorFact]> {
    Some(&CURRENT.as_ref()?.current)
}

pub(crate) fn descriptor_fact(name: &str) -> Option<&'static GeneratedScalarDescriptorFact> {
    let (group, name) = name.split_once(':')?;
    facts()?
        .iter()
        .find(|fact| fact.group0 == group && fact.name == name)
}

pub(crate) fn terminal_descriptor_name(name: &str) -> bool {
    CURRENT
        .as_ref()
        .is_some_and(|facts| terminal_descriptor_name_in(facts, name))
}

pub(crate) fn terminal_reverse(
    raw_tag_id: u16,
    family: FormatFamily,
    physical_group: &str,
) -> bool {
    family == FormatFamily::EXIF
        && CURRENT
            .as_ref()
            .is_some_and(|facts| terminal_reverse_in(facts, raw_tag_id, physical_group))
}

pub(crate) fn reverse_name(
    raw_tag_id: u16,
    family: FormatFamily,
    physical_group: &str,
) -> Option<&'static str> {
    (family == FormatFamily::EXIF).then_some(())?;
    facts()?.iter().find_map(|fact| {
        (fact.raw_tag_id == raw_tag_id && fact.physical_group == physical_group)
            .then_some(fact.name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_state(
        source: &StaticPublicSetNewValueMigration,
        removed_or_unsupported: bool,
    ) -> StaticPublicSetNewValueMigration {
        StaticPublicSetNewValueMigration {
            module: source.module,
            table: source.table,
            full_name: source.full_name,
            raw_tag_id: source.raw_tag_id,
            name: source.name,
            group0: source.group0,
            group1: source.group1,
            write_group: source.write_group,
            source_control_sha256: source.source_control_sha256,
            semantics_sha256: source.semantics_sha256,
            removed_or_unsupported,
        }
    }

    #[test]
    fn selected_generated_operands_publish_the_complete_current_intersection() {
        let facts = facts().expect("selected generated artifact join must be complete");
        assert_eq!(facts.len(), PUBLIC_SET_NEW_VALUE_MIGRATIONS.len());
        assert!(
            facts
                .iter()
                .all(|fact| fact.value_class == SourceValueClass::String)
        );
    }

    #[test]
    fn target_printer_reverse_fact_is_scoped_to_its_native_ifd() {
        assert_eq!(
            reverse_name(0x0151, FormatFamily::EXIF, "IFD0"),
            Some("TargetPrinter")
        );
        assert_eq!(reverse_name(0x0151, FormatFamily::EXIF, "IFD1"), None);
    }

    #[test]
    fn unsupported_final_format_refuses_the_whole_fallback() {
        let mut changed = TIFF_SCALAR_FINAL_RECIPES[0];
        changed.wire_format = "int16u";
        assert!(
            compose(
                Some(SET_NEW_VALUE_ADDRESS_ROWS),
                &PUBLIC_SET_NEW_VALUE_MIGRATIONS[..1],
                &[changed],
                TIFF_SCALAR_FINAL_FORMAT_REGISTRY,
            )
            .is_some_and(|facts| facts.current.is_empty() && facts.terminal.len() == 1)
        );
    }

    #[test]
    fn retained_historical_migration_refuses_a_stale_descriptor() {
        let current = &PUBLIC_SET_NEW_VALUE_MIGRATIONS[0];
        let retired = with_state(current, true);
        assert!(
            compose(
                Some(SET_NEW_VALUE_ADDRESS_ROWS),
                &[retired],
                &TIFF_SCALAR_FINAL_RECIPES[..1],
                TIFF_SCALAR_FINAL_FORMAT_REGISTRY,
            )
            .is_some_and(|facts| facts.current.is_empty() && facts.terminal.len() == 1)
        );
    }

    #[test]
    fn terminal_row_does_not_suppress_another_fully_joined_identity() {
        let live = with_state(&PUBLIC_SET_NEW_VALUE_MIGRATIONS[0], false);
        let retired = with_state(&PUBLIC_SET_NEW_VALUE_MIGRATIONS[1], true);
        let result = compose(
            Some(SET_NEW_VALUE_ADDRESS_ROWS),
            &[live, retired],
            &TIFF_SCALAR_FINAL_RECIPES[..1],
            TIFF_SCALAR_FINAL_FORMAT_REGISTRY,
        )
        .expect("global generated provenance remains valid");
        assert_eq!(result.current.len(), 1);
        assert_eq!(result.current[0].name, "DocumentName");
        assert_eq!(result.terminal.len(), 1);
        assert_eq!(result.terminal[0].name, "PageName");
    }

    #[test]
    fn global_capture_failure_keeps_only_migrated_identities_terminal() {
        let live = with_state(&PUBLIC_SET_NEW_VALUE_MIGRATIONS[0], false);
        let retired = with_state(&PUBLIC_SET_NEW_VALUE_MIGRATIONS[1], true);
        let migrations = [live, retired];
        let result = compose_with_capture(false, None, &migrations, &[], None)
            .expect("migration ownership must survive a global capture failure");

        assert!(result.current.is_empty());
        for migration in &migrations {
            let descriptor_name = format!("{}:{}", migration.group0, migration.name);
            assert!(terminal_descriptor_name_in(&result, &descriptor_name));
            assert!(terminal_reverse_in(
                &result,
                migration.raw_tag_id,
                migration.write_group
            ));
        }
        assert!(!terminal_descriptor_name_in(&result, "EXIF:UnownedName"));
        assert!(!terminal_reverse_in(&result, 0xffff, "IFD0"));
    }
}
