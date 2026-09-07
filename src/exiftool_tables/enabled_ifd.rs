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

    /// `is_enabled` binary-searches, so an unsorted list would silently fail
    /// to find entries rather than fail loudly.
    #[test]
    fn allowlist_is_sorted_and_unique() {
        assert!(
            ENABLED_IFD.windows(2).all(|w| w[0] < w[1]),
            "ENABLED_IFD must be sorted by (module, table) and free of duplicates"
        );
    }
}
