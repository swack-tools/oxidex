//! Generated schema for keyed binary directories.
//!
//! This describes a native directory layout and its source rows.  It has no
//! reader or caller yet: an emitted table is data for inventory/review, never
//! an activation request.

use super::ifd_schema::RawConvEffect;
use super::{Cond, ExprId, Fmt, GateA, IfdFlags, Omitted, PrintConv, TagGroups, U16SizeCheck};

#[derive(Clone, Copy, Debug)]
pub enum KeyedLayout {
    /// Canon CIFF's ten-byte directory entries.  The carrier supplies II/MM.
    Ciff10,
    /// Canon's source-authenticated length-prefixed u16 custom-function
    /// directory. The carrier supplies the bounded directory bytes and II/MM.
    LengthPrefixedU16Pairs(WordDirectory),
}

/// Native operands for one fixed-stride directory whose u16 words contain a
/// table key in their high bits and a source-selected masked integer value.
///
/// This is generated only after the compiler has authenticated the complete
/// native processor body and its `Get16u` binding. It remains a layout fact;
/// a caller must still pass Gate B through [`crate::exiftool_tables::KeyedEmissionSink`].
#[derive(Clone, Copy, Debug)]
pub struct WordDirectory {
    pub pair_start: usize,
    pub pair_stride: usize,
    pub key_shift: usize,
    pub value_mask: usize,
    pub header_adjustment: usize,
    pub model_condition: Cond,
    pub exact_length_first: bool,
    pub missing_model_as_empty: bool,
    pub short_u16_as_zero: bool,
    pub index_divisor: usize,
    pub index_bias: usize,
    /// The explicit `HandleTag` selection shape, rather than a CIFF type
    /// default. This does not re-read or narrow the processor-supplied
    /// integer: native `HandleTag` receives `$val` before it uses these
    /// operands to select tag information.
    pub value_format: Fmt,
    pub value_count: usize,
    pub value_size: usize,
    pub invalid_warning: &'static str,
    pub verbose_directory: &'static str,
    pub source_file: &'static str,
    pub source_sha256: &'static str,
    pub source_body_sha256: &'static str,
    pub reader_contract_sha256: &'static str,
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
    /// Shared native reporting policy, with table AVOID and PRIORITY already
    /// resolved. Do not apply those table overrides a second time. A selected
    /// disallowed Unknown alternative ends selection; it must not fall through
    /// to another alternative. Keep every alternative in native source order.
    pub flags: IfdFlags,
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
        /// A native false result skips only this child. Unavailable source
        /// remains an explicit `validate` blocker in `unwalked` instead.
        // Captured validation operands remain inactive while the native
        // reader contract is listed in `unwalked`.
        validation: Option<U16SizeCheck>,
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
    /// Expanded native flags with the same resolved table policy as KeyedTag.
    pub flags: IfdFlags,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyedNativeSubdir {
    pub tag_table: Option<&'static str>,
    pub start: Option<&'static str>,
    pub validate: bool,
    pub process_proc: bool,
}
