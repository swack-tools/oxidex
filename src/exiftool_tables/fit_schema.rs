//! The schema of the generated Garmin FIT specs (`fit_tables.rs`).
//!
//! Hand-written and stable; `tools/exiftool-tables/garmin_fit_specs.py`
//! emits `fit_tables.rs` against exactly these types, and
//! `parsers::specialized::fit` is the one executor that walks them. The
//! protocol it reproduces is `Image::ExifTool::Garmin::ProcessFIT` as
//! reviewed in `docs/reference/garmin-fit-source-review.md`.
//!
//! Unlike an IFD or binary table, a FIT field's format is not in the table:
//! the file's definition message names a base type for each field, so every
//! conversion here is a [`TypedConv`] whose compiled domain is checked
//! against the decoded value at run time.

use super::PrintConv;
use super::runtime::TypedConv;

/// The ExifTool format a FIT base type reads through (`ReadValue`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitFormat {
    Int8u,
    Int8s,
    Int16u,
    Int16s,
    Int32u,
    Int32s,
    Int64u,
    Int64s,
    Float,
    Double,
    String,
    Undef,
}

impl FitFormat {
    /// `FormatSize` for the format; the generator refuses a base type whose
    /// captured size differs.
    #[must_use]
    pub const fn size(self) -> usize {
        match self {
            Self::Int8u | Self::Int8s | Self::String | Self::Undef => 1,
            Self::Int16u | Self::Int16s => 2,
            Self::Int32u | Self::Int32s | Self::Float => 4,
            Self::Int64u | Self::Int64s | Self::Double => 8,
        }
    }
}

/// One entry of ProcessFIT's `%baseType` (Garmin.pm 25-43), captured from
/// the live pad.
#[derive(Clone, Copy, Debug)]
pub struct FitBaseType {
    pub id: u8,
    pub format: FitFormat,
    pub fit_name: &'static str,
    /// The invalid value as Perl text: ProcessFIT drops a value whose
    /// `lc $val` equals it (Garmin.pm 6557).
    pub invalid: &'static str,
    /// False when the generator refused this base type (for example a 64-bit
    /// type on a Perl without 64-bit integers). A field of a refused type is
    /// still sized and skipped exactly as ProcessFIT does; it is withheld.
    pub admitted: bool,
}

/// How a field's `PrintConv` is rendered.
#[derive(Clone, Copy, Debug)]
pub enum FitPrintConv {
    None,
    /// A hash conversion, rendered by the shared `runtime::render`.
    Table(PrintConv),
    /// An expression conversion with its compiled domain.
    Typed(TypedConv),
}

/// One field row of a FIT message table.
#[derive(Clone, Copy, Debug)]
pub struct FitField {
    pub num: u8,
    pub name: &'static str,
    /// The row's own `Groups => { 2 => ... }`, else the table's.
    pub group2: Option<&'static str>,
    pub raw_conv: Option<TypedConv>,
    pub value_conv: Option<TypedConv>,
    pub print_conv: FitPrintConv,
    /// `Some(reason)` when the generator refused the row. The row stays in the
    /// table so lookup finds it (and never falls through to `Common`), and the
    /// executor withholds it.
    pub withheld: Option<&'static str>,
}

/// One FIT message table (or `Garmin::Common`).
#[derive(Clone, Copy, Debug)]
pub struct FitTable {
    pub table: &'static str,
    pub group0: &'static str,
    pub group1: &'static str,
    pub group2: &'static str,
    /// Sorted by `num`, unique.
    pub fields: &'static [FitField],
}

impl FitTable {
    #[must_use]
    pub fn field(&self, num: u8) -> Option<&'static FitField> {
        self.fields
            .binary_search_by_key(&num, |field| field.num)
            .ok()
            .map(|index| &self.fields[index])
    }
}

/// One `Garmin::FIT` message edge.
#[derive(Clone, Copy, Debug)]
pub struct FitMessage {
    pub num: u16,
    pub name: &'static str,
    /// `Unknown => 1`: ProcessFIT builds no field list for this message unless
    /// the Unknown option is set (Garmin.pm 6381-6383).
    pub unknown: bool,
    /// `None` for an edge with no `SubDirectory`, for which ProcessFIT builds
    /// an empty table named after the message (Garmin.pm 6364-6375).
    pub table: Option<&'static FitTable>,
}

/// The whole generated FIT protocol.
#[derive(Clone, Copy, Debug)]
pub struct FitProtocol {
    /// `Some(reason)` when the generator refused the protocol (a changed
    /// ProcessFIT body or value reader); the executor then extracts nothing.
    pub refusal: Option<&'static str>,
    /// Sorted by `id`.
    pub base_types: &'static [FitBaseType],
    /// Sorted by `num`.
    pub messages: &'static [FitMessage],
    pub common: &'static FitTable,
    /// The name of the FIT table's `vers` row (the header's protocol
    /// version byte, Garmin.pm 6311); `None` if the generator refused it.
    pub header_name: Option<&'static str>,
    /// Groups of the `Garmin::FIT` table, which the header row reports under.
    pub header_group0: &'static str,
    pub header_group1: &'static str,
    pub header_group2: &'static str,
}

impl FitProtocol {
    #[must_use]
    pub fn base_type(&self, id: u8) -> Option<&'static FitBaseType> {
        self.base_types
            .binary_search_by_key(&id, |base| base.id)
            .ok()
            .map(|index| &self.base_types[index])
    }

    #[must_use]
    pub fn message(&self, num: u16) -> Option<&'static FitMessage> {
        self.messages
            .binary_search_by_key(&num, |message| message.num)
            .ok()
            .map(|index| &self.messages[index])
    }
}
