//! Gate B for the generated IFD-style tables: the measured allowlist of
//! [`IfdTable`]s the IFD engine may walk.
//!
//! Same design as [`super::enabled`] (Step 28 D1, opt-in): a table is OFF
//! until one line here turns it on, and that line carries the corpus A/B that
//! justified it (`tools/exiftool-tables/conformance.py` over the combined
//! samples against the pinned 13.59 oracle: MISSING moved to matched, zero
//! new group-qualified VALUE, zero new EXTRA). [`is_enabled`] re-checks Gate A
//! at runtime, so a line for a table a regeneration stopped being sound for
//! does nothing -- the list can only narrow.

use super::ifd_schema::IfdTable;

/// The allowlist. Sorted by `(module, table)`; [`is_enabled`] binary-searches
/// it. Empty until slice I-2 lands the first measured line (Olympus::Main).
pub static ENABLED_IFD: &[(&str, &str)] = &[];

/// Whether the IFD engine may walk `table`: Gate A (static soundness,
/// re-checked here rather than trusted from the list) AND Gate B (this list).
#[must_use]
pub fn is_enabled(table: &IfdTable) -> bool {
    table.gate_a.passes()
        && ENABLED_IFD
            .binary_search_by(|(module, name)| (*module, *name).cmp(&(table.module, table.table)))
            .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool_tables::{ALL_IFD_TABLES, find_ifd_table};

    // The four tests `enabled.rs` carries for the binary allowlist, over the
    // IFD one. On the engine branch `ALL_IFD_TABLES` is the empty stub in
    // `ifd_tables.rs`, so the three that iterate tables pass vacuously; they
    // become the real Gate-B guard the moment the generated file replaces the
    // stub, with no edit needed here.

    /// `is_enabled` binary-searches, so an unsorted list would silently fail
    /// to find entries rather than fail loudly.
    #[test]
    fn allowlist_is_sorted_and_unique() {
        assert!(
            ENABLED_IFD.windows(2).all(|w| w[0] < w[1]),
            "ENABLED_IFD must be sorted by (module, table) and free of duplicates"
        );
    }

    /// An allowlist line for a table that does not exist is a typo that would
    /// otherwise be indistinguishable from a table that is simply off.
    #[test]
    fn every_allowlist_entry_names_a_real_table() {
        for (module, table) in ENABLED_IFD {
            assert!(
                find_ifd_table(module, table).is_some(),
                "{module}::{table} is on the IFD allowlist but no such table is generated"
            );
        }
    }

    /// The allowlist can only narrow: Gate A is re-checked at runtime, so a
    /// line here that Gate A blocks must not enable anything. If this ever
    /// fires, the fix is to remove the line, not to relax the gate.
    #[test]
    fn no_allowlist_entry_is_blocked_by_gate_a() {
        for (module, table) in ENABLED_IFD {
            let t = find_ifd_table(module, table).expect("checked above");
            assert!(
                t.gate_a.passes(),
                "{module}::{table} is allowlisted but gate A blocks it: {:?}",
                t.gate_a.blocked_by
            );
        }
    }

    /// Opt-in is the whole design: if `is_enabled` ever defaulted to true,
    /// every never-measured IFD table would start producing tags at once and
    /// no corpus delta would be attributable to any one line.
    #[test]
    fn everything_not_listed_is_off() {
        let enabled: Vec<_> = ALL_IFD_TABLES
            .iter()
            .filter(|t| is_enabled(t))
            .map(|t| (t.module, t.table))
            .collect();
        assert_eq!(
            enabled.len(),
            ENABLED_IFD.len(),
            "exactly the allowlisted tables may be enabled, found {enabled:?}"
        );
    }
}
