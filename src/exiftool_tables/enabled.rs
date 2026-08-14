//! Step 28 Gate B: the measured allowlist of tables the generic engine may
//! walk.
//!
//! # Opt-in, one line per table (design D1)
//!
//! 590 of the 613 transcribed tables have never executed. Assuming they work
//! inverts the burden of proof, and this project's doctrine is that absence
//! beats a confident wrong value (`AGENTS.md`, "never approximate a
//! conversion"). So a table is OFF until it is listed here, and listing it is
//! a one-line diff -- which makes the review artifact and the revert the same
//! object.
//!
//! # What a line here asserts
//!
//! Both gates, not one:
//!
//! * **Gate A** ([`super::GateA`], generated): every field ExifTool declares
//!   was transcribed or explicitly refused, every `PrintConv` reproduced
//!   exactly, and no `offsets_sound_until` hazard. [`is_enabled`] re-checks
//!   this at runtime, so a line added here for a table that does not pass
//!   Gate A does nothing -- the allowlist can only ever narrow.
//! * **Gate B** (measured, per table): enabling it on
//!   `/tmp/oxidex-exiftool-cache/combined-samples` (4,238 files) against the
//!   pinned 13.59 oracle moved MISSING to matched and produced *zero* new
//!   group-qualified VALUE and *zero* new EXTRA, under
//!   `tools/exiftool-tables/conformance.py`. That instrument is named because
//!   a bare-name comparison scores `AIFF:Comment` against `ID3:Comment` as a
//!   defect when both tools emit both (`AGENTS.md`, "name the instrument").
//!
//! A table that passes A but that the corpus never covers is NOT listed. That
//! is the stricter half of design D3's option (b), chosen over (a) because an
//! "enabled-unverified" table is exactly the thing this step exists to stop
//! shipping: it produces tags nobody has ever compared against ExifTool.
//! The count of such tables is published by
//! `tools/exiftool-tables/reachability.py` as `eligible`, so coverage held
//! hostage to corpus acquisition stays visible rather than becoming invisible.

use super::BinaryTable;

/// The allowlist. Sorted by `(module, table)`; [`is_enabled`] binary-searches
/// it, and a test pins the ordering.
///
/// Every entry carries the evidence that put it here, in the comment above
/// it. An entry with no measurement behind it is a coverage lie.
pub static ENABLED: &[(&str, &str)] = &[
    // APE::NewHeader -- `src/parsers/audio/ape.rs`'s `parse_new_header`, the
    // MAC 3.98+ audio header (APE.pm:65-78, reached from APE.pm:156-161).
    // Corpus carriers: `APE.ape` (the only MAC descriptor in all 4,238
    // files; `APE.mpc` is a Musepack tag trailer with no MAC header).
    //
    // Gate B, `tools/exiftool-tables/conformance.py` over
    // `/tmp/oxidex-exiftool-cache/combined-samples` --recursive --min-files
    // 3875 --min-tags 5000, pinned 13.59 oracle
    // (`exiftool-pinned.sh -ver` = 13.59, `OOXML.docx` -> `DOCX`), debug
    // binaries built at 38144a2c (control) and this branch (treatment):
    //
    //     control  TOTAL 4238 437050 match 21 rename 1671 value 11610 missing 10602 extra 97.0%
    //     enabled  TOTAL 4238 437050 match 21 rename 1671 value 11610 missing 10602 extra 97.0%
    //
    // Zero new VALUE, zero new EXTRA -- and zero of anything else: every
    // per-format row is byte-identical, `APE 1 22 0 0 0 0` on both sides.
    //
    // This one is a FOLD, not a coverage gain, and the comment says so
    // because a reader who assumed otherwise would mis-read the delta.
    // `ape.rs` already decoded all seven fields correctly from thirteen
    // hand-written `u16_at`/`u32_at` offsets; what the line buys is that
    // those offsets are gone and the record now reads through the one
    // `ReadValue` (ExifTool.pm:6286). The evidence a fold needs is exactly
    // the null result above: enabling it changed nothing an oracle can see.
    // Per-field against the pinned oracle on `APE.ape`: CompressionLevel
    // 3000, BlocksPerFrame 73728, FinalFrameBlocks 42662, TotalFrames 2,
    // BitsPerSample 16, Channels 2, SampleRate 44100 -- pinned by
    // `tests/ape_new_header_binary_table.rs` against the real carrier.
    ("APE", "NewHeader"),
    // Canon::CMP1 -- `src/parsers/raw/metadata.rs:4788`, the CR3 `CMP1` box.
    // Corpus carriers: `CanonRaw.cr3` plus the Canon vendor directory.
    ("Canon", "CMP1"),
    // ID3::v1 -- `src/parsers/audio/mp3.rs:499`, the 128-byte ID3v1 trailer.
    // This one is the clearest case for the shared `ReadValue`: every field
    // is a `string[N]` butted against the end of a fixed 128-byte record,
    // which is exactly where ExifTool.pm:6301-6303's count shortening and
    // the old all-or-nothing read can disagree. Corpus carrier: `MP3.mp3`.
    ("ID3", "v1"),
    // MPF::MPImage -- `src/parsers/jpeg/mpf_parser.rs:591`. The most heavily
    // exercised of the five: 689 corpus files report `MPImage1:*`.
    ("MPF", "MPImage"),
    // Pentax::MOV -- `src/parsers/quicktime/metadata_extractor.rs:3601`.
    // Carries a `string[24]` Make and a `string[24]` Model at the head of the
    // record, same shortening story as ID3::v1.
    ("Pentax", "MOV"),
    // Sony::Panorama -- `src/parsers/tiff/makernotes/sony/amount.rs:662`.
    // Corpus carrier: the Sony vendor directory (761 JPEGs).
    ("Sony", "Panorama"),
    //
    // NOT listed, and why -- these are the decisions, not the leftovers:
    //
    // * Ricoh::ImageInfo passes gate A and `ricoh.rs:215` NAMES
    //   `find_table("Ricoh","ImageInfo")` -- in a comment explaining that the
    //   module does not call it. It has no live call site, so a gate B run
    //   measures nothing about it. It was briefly listed here on the strength
    //   of that sentence, which is why `reachability.py` now strips comments
    //   before counting call sites.
    // * APE::OldHeader passes gate A and DOES have a live call site --
    //   `ape.rs`'s `parse_old_header`, reached from APE.pm:149-151 for MAC
    //   3.97 and earlier. It is still not listed, because the corpus holds
    //   exactly two APE files and both report version 3990 in their
    //   descriptor, so every byte of `OldHeader` is unexercised and a gate B
    //   run would measure nothing about it. It is the first table to sit in
    //   the state this file's header describes: coverage held hostage to
    //   corpus acquisition, published as `eligible` by `just reachability`
    //   rather than quietly enabled. (Its `APEVersion` also carries a
    //   `ValueConv => '$val / 1000'` the transcription refuses, so the
    //   division stays hand-written under `RawAccess` either way.)
    // * The other 348 gate-A-passing tables have no live call site at all
    //   (see `just reachability`). Enabling one would produce no tags and no
    //   measurement -- enablement on no evidence, which is the thing design
    //   D1 exists to prevent.
    // * Every table reachable through a compiled `SubdirEdge` from a live
    //   root is blocked by gate A. In the pinned 13.59 tree exactly one edge
    //   hangs off a hand-wired table -- `CanonVRD::Ver2 -> CanonVRD::DLOInfo`
    //   (`canon_vrd/ver2.rs:60`) -- and `DLOInfo` trips
    //   `tag_fmt_unsupported=1`. So subdirectory recursion, which Step 27
    //   built the edges for and this step built the walk for, enables nothing
    //   yet. That is a measurement, not an omission.
];

/// Whether the generic engine may walk `table`.
///
/// BOTH gates, checked here rather than trusted from the list: Gate A is a
/// property of the generated data and can change under an allowlist line when
/// the pinned release moves, so re-checking it is what stops a bump from
/// silently enabling a table that stopped being sound.
#[must_use]
pub fn is_enabled(table: &BinaryTable) -> bool {
    table.gate_a.passes()
        && ENABLED
            .binary_search_by(|(module, name)| (*module, *name).cmp(&(table.module, table.table)))
            .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool_tables::{ALL_BINARY_TABLES, find_table};

    /// `is_enabled` binary-searches, so an unsorted list would silently fail
    /// to find entries rather than fail loudly.
    #[test]
    fn allowlist_is_sorted_and_unique() {
        assert!(
            ENABLED.windows(2).all(|w| w[0] < w[1]),
            "ENABLED must be sorted by (module, table) and free of duplicates"
        );
    }

    /// An allowlist line for a table that does not exist is a typo that would
    /// otherwise be indistinguishable from a table that is simply off.
    #[test]
    fn every_allowlist_entry_names_a_real_table() {
        for (module, table) in ENABLED {
            assert!(
                find_table(module, table).is_some(),
                "{module}::{table} is on the Step 28 allowlist but no such \
                 table is generated"
            );
        }
    }

    /// The allowlist can only narrow: Gate A is re-checked at runtime, so a
    /// line here that Gate A blocks must not enable anything. If this ever
    /// fires, the fix is to remove the line, not to relax the gate.
    #[test]
    fn no_allowlist_entry_is_blocked_by_gate_a() {
        for (module, table) in ENABLED {
            let t = find_table(module, table).expect("checked above");
            assert!(
                t.gate_a.passes(),
                "{module}::{table} is allowlisted but gate A blocks it: {:?}",
                t.gate_a.blocked_by
            );
        }
    }

    /// Opt-in is the whole design (D1). If a future change makes `is_enabled`
    /// default to true, every one of the 590 never-executed tables starts
    /// producing tags at once and no delta is attributable -- the exact
    /// failure mode section 4 of the design argues against.
    #[test]
    fn everything_not_listed_is_off() {
        let enabled: Vec<_> = ALL_BINARY_TABLES
            .iter()
            .filter(|t| is_enabled(t))
            .map(|t| (t.module, t.table))
            .collect();
        assert_eq!(
            enabled.len(),
            ENABLED.len(),
            "exactly the allowlisted tables may be enabled, found {enabled:?}"
        );
    }
}
