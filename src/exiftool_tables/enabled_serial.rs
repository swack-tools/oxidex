//! Gate B for source-described `ProcessSerialData` tables.
//!
//! A serial descriptor has no carrier by itself.  This allowlist is the
//! separate rollout decision that permits a supported carrier to execute one
//! table after the caller has already authenticated the parent edge.

use super::serial_schema::SerialTable;

/// Serial tables whose callers may execute them. Sorted and deliberately
/// narrow: the IFD-to-serial bridge currently has parent-carrier evidence only
/// for Canon::AFInfo2. Presence here is insufficient without the caller's
/// source-authenticated processor and validation facts.
pub static ENABLED_SERIAL: &[(&str, &str)] = &[("Canon", "AFInfo2")];

/// Gate B plus the generated table's Gate A.
#[must_use]
pub fn is_enabled(table: &SerialTable) -> bool {
    table.gate_a.passes() && owns(table.module, table.table)
}

/// The migration decision is independent of current source representability.
/// A later refused definition must not reactivate a retired manual producer.
#[must_use]
pub fn owns(module: &str, table: &str) -> bool {
    ENABLED_SERIAL
        .binary_search_by(|(candidate_module, name)| {
            (*candidate_module, *name).cmp(&(module, table))
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool_tables::find_serial_table;

    #[test]
    fn allowlist_is_sorted_and_resolves_to_gate_a_clean_tables() {
        assert!(ENABLED_SERIAL.windows(2).all(|pair| pair[0] < pair[1]));
        for (module, table) in ENABLED_SERIAL {
            let table = find_serial_table(module, table)
                .expect("serial Gate B entry must name a generated table");
            assert!(
                table.gate_a.passes(),
                "{module}::{table:?} is Gate A blocked"
            );
            assert!(is_enabled(table));
        }
    }
}
