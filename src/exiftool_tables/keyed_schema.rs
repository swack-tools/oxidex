//! Generated schema for keyed binary directories.
//!
//! This describes a native directory layout and its source rows.  It has no
//! reader or caller yet: an emitted table is data for inventory/review, never
//! an activation request.

use super::ifd_schema::RawConvEffect;
use super::{Cond, ExprId, Fmt, GateA, Omitted, PrintConv, TagGroups};

#[derive(Clone, Copy, Debug)]
pub enum KeyedLayout {
    /// Canon CIFF's ten-byte directory entries.  The carrier supplies II/MM.
    Ciff10,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyedDirectoryTable {
    pub module: &'static str,
    pub table: &'static str,
    pub group0: &'static str,
    pub group1: &'static str,
    pub group2: &'static str,
    pub layout: KeyedLayout,
    pub gate_a: GateA,
    pub tags: &'static [KeyedTag],
    pub variants: &'static [KeyedVariantGroup],
}

#[derive(Clone, Copy, Debug)]
pub struct KeyedTag {
    /// Native source key, not an offset. The layout derives its default format
    /// from the raw entry type at read time.
    pub raw_id: u16,
    pub name: &'static str,
    pub format: Option<Fmt>,
    /// Verbatim numeric Count when the source declares one. `None` is
    /// undefined; `Some(0)` is false and must trigger ProcessCanonRaw's
    /// size/format fallback. A reader must not replace either with one.
    pub count: Option<usize>,
    pub condition: Option<Cond>,
    pub raw_conv: Option<RawConvEffect>,
    pub omitted: Omitted,
    pub value_conv: Option<ExprId>,
    pub print_conv: PrintConv,
    pub groups: TagGroups,
    pub edge: Option<KeyedEdge>,
    /// Verbatim native properties retained for independent source inventory.
    /// A future reader uses the typed members above, never this audit record.
    pub native: KeyedNativeFacts,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyedVariantGroup {
    pub raw_id: u16,
    pub alternatives: &'static [(Cond, KeyedTag)],
}

#[derive(Clone, Copy, Debug)]
pub enum KeyedEdge {
    /// A 0x28/0x30 CIFF directory opens the enclosing keyed table again.
    SameTableDirectory,
    /// A ProcessCanonRaw value block. `unwalked` names every unsupported
    /// source property rather than silently treating it as an ordinary value.
    BoundedValue {
        module: &'static str,
        table: &'static str,
        start: KeyedStart,
        unwalked: &'static [&'static str],
    },
}

#[derive(Clone, Copy, Debug)]
pub enum KeyedStart {
    Zero,
}

/// Native keyed rows completely absent from the generated table facts.
/// This is distinct from [`Omitted`], which describes semantics withheld on a
/// row that still has a schema literal.
#[derive(Clone, Copy, Debug)]
pub struct OmittedKeyedNativeRow {
    pub module: &'static str,
    pub table: &'static str,
    pub raw_id: &'static str,
    pub variant: bool,
    pub name: Option<&'static str>,
    pub native: KeyedNativeFacts,
    pub reasons: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
pub struct KeyedNativeFacts {
    pub format: Option<&'static str>,
    pub count: Option<&'static str>,
    pub condition: Option<&'static str>,
    pub groups: TagGroups,
    pub subdir: Option<KeyedNativeSubdir>,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyedNativeSubdir {
    pub tag_table: Option<&'static str>,
    pub start: Option<&'static str>,
    pub validate: bool,
    pub process_proc: bool,
}
