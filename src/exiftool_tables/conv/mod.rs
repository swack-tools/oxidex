//! Autogeneration v2 step 2 (`docs/AUTOGENERATION-V2-DESIGN.md` sections 4-5,
//! "The gate change"): ExifTool's own conversions -- `RawConv`, `ValueConv`
//! and `PrintConv` -- emitted from its source as Rust `match` arms over a
//! [`Session`], and run per field in MIXED MODE.
//!
//! # What an arm is
//!
//! `tools/exiftool-tables/conv_codegen.py` parses each field's conversion
//! strings with the spike's grammar (`spike/perl_subset.py`) and compiles the
//! AST against [`rt`] (Perl's operators) and [`crate::exiftool_tables::helpers`]
//! (ExifTool's helper subs, #824). One generated file per table:
//! [`exif_main`] for `%Image::ExifTool::Exif::Main`. Its `decode(session, id,
//! $val)` is a `match` on the tag id; each arm evaluates, in ExifTool's order
//! (`FoundTag` then `GetValue`, ExifTool.pm:9484-9505, 3440-3740), the
//! `RawConv`, the `ValueConv` (or the `\$val` of a `Binary` tag) and the
//! `PrintConv`, and returns an [`Arm`].
//!
//! # Mixed mode
//!
//! A field is either GENERATED (the backend compiled every conversion slot
//! it has) or REFUSED (something in it is outside the grammar, calls a helper
//! with no proven port, or reads state the session does not carry) --
//! `exif_main::REFUSED` lists each with its reason, and the committed ledger
//! `tools/exiftool-tables/conv_exif_main_ledger.json` says the same. A
//! generated arm can still DECLINE one entry at run time ([`Arm::Decline`]:
//! a value its runtime does not model, such as a subject with a newline
//! against a `$` anchor, or bytes that are not UTF-8). In both cases the
//! existing path produces that entry exactly as before, so no read is lost;
//! the walk (`ifd_engine::walk`) calls the arm first and falls through.
//!
//! # Transactional effects
//!
//! A decoder receives mutable [`Session`] state because ExifTool conversions
//! may assign data members before a later condition reads them. The IFD engine
//! runs each attempt against `ifd_engine::StagedEffects`, never the live
//! session: [`Arm::Report`] and [`Arm::Suppress`] commit source-required
//! writes in order, while [`Arm::Decline`] discards the attempt before the
//! caller invokes its one hand residual. This makes the outcome and its state
//! transition one ownership decision; a declined generated attempt cannot
//! leak warnings or members into the fallback path.
//!
//! # Proof
//!
//! `tools/exiftool-tables/conv_oracle.py` evaluates every generated arm's
//! SOURCE through the pinned ExifTool's own `FoundTag`/`GetValue` on a probe
//! battery and commits the capture
//! (`tools/exiftool-tables/testdata/conv_exif_main_outputs.json`);
//! `tests::every_generated_arm_matches_the_pinned_perl_capture` replays each
//! probe through the arm and requires the same bytes -- or a decline, which
//! is counted. A disagreement fails the build.

// BEGIN GENERATED CONVERSION REGISTRY
pub mod exif_main;

pub static REGISTRY: &[Entry] = &[Entry {
    module: "Exif",
    table: "Main",
    decode: exif_main::decode,
    claims: exif_main::claims,
}];
// END GENERATED CONVERSION REGISTRY
pub mod rt;

pub use rt::{Decline, Out, R};

use super::ifd_schema::{IfdTable, IfdTag};
use super::session::{MemberVal, Session};

/// A generated table's entry point: `decode(session, id, $val)`.
pub type Decode = fn(&mut Session, u16, &MemberVal) -> Arm;

/// One generated table, keyed by its exact ExifTool identity. The decoder and
/// claim predicate travel together so dispatch cannot accidentally borrow the
/// first table's claim set for another table.
pub struct Entry {
    pub module: &'static str,
    pub table: &'static str,
    pub decode: Decode,
    pub claims: fn(u16) -> bool,
}

fn entry_in(entries: &'static [Entry], table: &IfdTable) -> Option<&'static Entry> {
    entries
        .binary_search_by_key(&(table.module, table.table), |entry| {
            (entry.module, entry.table)
        })
        .ok()
        .map(|index| &entries[index])
}

#[cfg(test)]
fn decoder_in(entries: &'static [Entry], table: &IfdTable) -> Option<Decode> {
    entry_in(entries, table).map(|entry| entry.decode)
}

/// The generated decoder for `table`, if one was generated. Keyed by the
/// table's ExifTool identity, never by a caller's name for it.
#[must_use]
pub fn decoder(table: &IfdTable) -> Option<Decode> {
    entry_in(REGISTRY, table).map(|entry| entry.decode)
}

/// Whether `table`'s generated decoder takes plain tag `tag`: an arm exists
/// for its id, and the walk reaches the conversion stage for it at all (not
/// a `SubDirectory` edge, not `Unknown`, no `Condition` the walker cannot
/// resolve). Such a tag is reported by the engine even where the static
/// table withholds its conversion (`omitted`): the arm is the conversion.
#[must_use]
pub fn claims(table: &IfdTable, tag: &IfdTag) -> bool {
    claims_in(REGISTRY, table, tag)
}

fn claims_in(entries: &'static [Entry], table: &IfdTable, tag: &IfdTag) -> bool {
    entry_in(entries, table).is_some_and(|entry| (entry.claims)(tag.id))
        && tag.subdir.is_none()
        && !tag.flags.unknown
        && !tag.omitted.condition
        && !tag.omitted.subdirectory
        && !tag.omitted.hook
}

/// What one arm did with one entry.
#[derive(Clone, Debug, PartialEq)]
pub enum Arm {
    /// Not produced here; the caller's existing path runs for this entry.
    Decline(&'static str),
    /// ExifTool reports no tag: `RawConv` returned `undef` (`FoundTag`
    /// returns before storing it, ExifTool.pm:9505) or `ValueConv` did
    /// (`GetValue` returns the empty list, ExifTool.pm:3694).
    Suppress,
    Report(Report),
}

/// A reported tag, both of ExifTool's modes.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// The `-n` value. `None` when no conversion stage replaced `$val`
    /// (no `RawConv`, no `ValueConv`, not `Binary`): the caller keeps the
    /// typed value it read.
    pub value: Option<Out>,
    /// The default-mode value when a `PrintConv` ran; `None` when there is
    /// none (or the value is a SCALAR reference, which is never
    /// print-converted), so the default mode prints `value`.
    pub print: Option<Out>,
    /// `$$self{X} = ...` assignments the arm made, in order, for the caller
    /// to apply to its session (later `Condition`s read them).
    pub writes: Vec<(&'static str, MemberVal)>,
}

/// Turns an arm body's `R<Arm>` into an [`Arm`].
#[must_use]
pub fn finish(r: R<Arm>) -> Arm {
    r.unwrap_or_else(|Decline(why)| Arm::Decline(why))
}

#[cfg(test)]
mod tests;
