//! STUB -- replaced by codegen output at integration.
//!
//! `tools/exiftool-tables/codegen.py` emits this file (design spec
//! `docs/superpowers/specs/2026-09-06-ifd-tables-design.md`, section 2): one
//! `pub static IFD_<MODULE>_<TABLE>: IfdTable = IfdTable { ... };` per
//! `ProcessExif` table of the pinned 13.59 tree, plus `ALL_IFD_TABLES` sorted
//! by `(module, table)`. On the engine branch (`staging/ifd-engine`) the
//! generator does not exist yet, so this file carries only the empty
//! registry: enough for `super::find_ifd_table` and `super::ifd_engine` to
//! compile against the schema, and for `super::enabled_ifd`'s Gate-B tests to
//! run vacuously until the generated set replaces it.
//!
//! Nothing here is hand-maintained; the integrator overwrites the whole file.

use super::ifd_schema::IfdTable;

/// Every generated `ProcessExif` table. Empty until the generator's output
/// lands (see the module doc).
pub static ALL_IFD_TABLES: &[&IfdTable] = &[];
