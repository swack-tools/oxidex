//! Generated schema for ExifTool `ProcessSerialData` tables.
//!
//! The native processor walks numeric table keys in order while carrying raw
//! values from earlier slots into later `Format => "type[$val{N}]"` count
//! expressions.  This module is deliberately only the data contract.  A
//! generated table is not a production route; [`super::serial_engine`] still
//! requires a caller-supplied Gate B policy before it reads any bytes.

use super::{Cond, ExprId, Fmt, GateA, IfdFlags, Omitted, PrintConv, RawConvEffect, TagGroups};

/// Source provenance for the shared native `ProcessSerialData` body selected
/// by a table.  The generator obtains these facts from the captured CODE ref;
/// the reader never dispatches on a module, table, or processor name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialProcessorFacts {
    pub name: &'static str,
    pub source_file: &'static str,
    pub source_sha256: &'static str,
    pub source_body_sha256: &'static str,
}

/// A generated native serial table.
#[derive(Clone, Copy, Debug)]
pub struct SerialTable {
    pub module: &'static str,
    pub table: &'static str,
    pub group0: &'static str,
    pub group1: &'static str,
    pub group2: &'static str,
    /// The table `FORMAT` after ExifTool's defaulting.  Every entry also
    /// carries its resolved runtime format so this remains audit data rather
    /// than an implicit reader fallback.
    pub default_format: Fmt,
    pub processor: SerialProcessorFacts,
    /// Aggregate table and row refusals.  A nonempty gate prevents partial
    /// execution: a skipped serial slot would change all later offsets.
    pub gate_a: GateA,
    /// Sorted by `serial_index`; a gap is malformed for this processor.
    pub entries: &'static [SerialEntry],
}

/// One native numeric key in a serial table.
#[derive(Clone, Copy, Debug)]
pub struct SerialEntry {
    pub serial_index: usize,
    /// Native `_variants` in source order. A scalar source row has one
    /// unconditional alternative.
    pub alternatives: &'static [SerialTag],
}

/// The count expression ProcessSerialData evaluates after GetTagInfo chose a
/// row but before `ReadValue` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerialCount {
    Fixed {
        value: usize,
    },
    PriorRaw {
        serial_index: usize,
    },
    FloorDivPriorRaw {
        serial_index: usize,
        add: usize,
        divisor: usize,
        /// Literal `+ T` evaluated after native Perl's integer division.
        /// This is distinct from `add`, which is inside the numerator.
        trailing_add: usize,
    },
    /// Explicit per-field bare `Format => 'string'`: consume `$size - $pos`.
    RemainingBytes,
}

/// How an absent `$self` member participates in a compiled serial condition.
///
/// `ProcessSerialData` evaluates Conditions through Perl, where an undefined
/// scalar used by a string regex/equality operation behaves as `""`. This is
/// deliberately a per-condition contract: it must not manufacture a defined
/// `MemberValue` for `defined`, numeric, or later unrelated conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerialMissingMember {
    SharedDefault,
    EmptyStringForStringOps,
}

/// A serial Condition plus the source-derived missing-member policy its native
/// string operators need. The `Cond` grammar and its normal evaluation remain
/// shared; this wrapper prevents ProcessSerialData's Perl coercion from
/// changing IFD and binary-table semantics.
#[derive(Clone, Copy, Debug)]
pub struct SerialCondition {
    pub cond: Cond,
    pub missing_member: SerialMissingMember,
}

impl SerialCondition {
    #[must_use]
    pub const fn needs_value_context(self) -> bool {
        self.cond.needs_value_context()
    }

    #[must_use]
    pub fn eval(self, ctx: &mut super::cond::Ctx) -> bool {
        match self.missing_member {
            SerialMissingMember::SharedDefault => self.cond.eval(ctx),
            SerialMissingMember::EmptyStringForStringOps => {
                self.cond.eval_with_missing_empty_string(ctx)
            }
        }
    }
}

/// Serial-only rendering operations accepted from the native processor facts.
///
/// `Shared` is the ordinary, typed PrintConv shared with binary/IFD tables.
/// `DecodeBitsWords` is deliberately separate because native DecodeBits sees
/// ProcessSerialData's space-joined multiword scalar and has no lookup hash.
#[derive(Clone, Copy, Debug)]
pub enum SerialPrintConv {
    None,
    Shared(PrintConv),
    DecodeBitsWords { bits_per_word: u8 },
}

/// A selected serial row's resolved read operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialFormat {
    pub format: Fmt,
    pub count: SerialCount,
}

/// One source alternative selected by `GetTagInfo`.
#[derive(Clone, Copy, Debug)]
pub struct SerialTag {
    pub name: &'static str,
    pub format: SerialFormat,
    /// `None` is unconditional only when `omitted.condition` is false.
    pub condition: Option<SerialCondition>,
    pub flags: IfdFlags,
    pub raw_conv: Option<RawConvEffect>,
    pub omitted: Omitted,
    pub value_conv: Option<ExprId>,
    pub print_conv: SerialPrintConv,
    pub groups: TagGroups,
}

/// A native source alternative deliberately not represented by [`SerialTag`].
///
/// The generator emits one record for every withheld alternative; this keeps
/// source inventory and generated output independently reconcilable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OmittedSerialNativeRow {
    pub module: &'static str,
    pub table: &'static str,
    pub serial_index: usize,
    pub variant: bool,
    pub alternative: usize,
    pub name: Option<&'static str>,
    pub reasons: &'static [&'static str],
}

/// A selected native serial table with a table-level refusal or no generated
/// descriptor.  Row omissions belong in [`OmittedSerialNativeRow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OmittedSerialNativeTable {
    pub module: &'static str,
    pub table: &'static str,
    pub reasons: &'static [&'static str],
}
