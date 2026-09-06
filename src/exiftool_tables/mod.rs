//! ExifTool tag tables transcribed mechanically from ExifTool itself.
//!
//! # Why this exists
//!
//! OxiDex already knows most ExifTool tag *names*: `src/tag_sync` parses
//! `exiftool -f -listx`, which lists every documented tag. Knowing a name is
//! not enough to read one. `-listx` is the documentation view, and it omits
//! the things a reader needs:
//!
//! * `SubDirectory`/`TagTable` -- the edges between tables
//! * `FORMAT` / `FIRST_ENTRY` -- the byte layout of binary records
//! * `Format` overrides, `Mask`, `DataMember`, `Condition`
//! * `ValueConv` / `RawConv`
//!
//! That missing layout is precisely what MakerNote extraction depends on, and
//! it is why tag *coverage* has trailed tag *knowledge*. This module closes
//! that gap by reading ExifTool's tables out of the Perl interpreter's symbol
//! table, where the real structures live, and generating Rust from them.
//!
//! # Guarantees
//!
//! The generator refuses to approximate. A `PrintConv` it cannot reproduce
//! exactly is dropped, not guessed, and the drop is counted and reported. This
//! is a deliberate bias toward under-claiming: a wrong conversion does not
//! crash, it emits a confident wrong number under a real ExifTool tag name,
//! and an archival pipeline downstream cannot tell. A missing tag is loud and
//! recoverable; a wrong one is neither.
//!
//! `tools/exiftool-tables/verify.py` checks every emitted field and enum entry
//! back against ExifTool through an independent code path, and is wired up as
//! `just verify-tables`.
//!
//! # Regenerating
//!
//! ```sh
//! just regen-tables            # extract + generate + verify
//! ```

pub mod binary_tables;
pub mod cond;
pub mod enabled;
pub mod engine;
pub mod exprs;
pub mod runtime;
pub mod subdir;

pub use binary_tables::{
    ALL_BINARY_TABLES, BinaryTable, EXIFTOOL_VERSION, ExprId, ExprValue, Field, Fmt, GateA,
    HookCond, HookDelta, HookEffect, Mask, Omitted, OtherId, PrintConv, TagGroups, VarFmt, VarKind,
};
pub use cond::{CmpOp, Cond, Ctx, EffectSource, MemberValue, VariantGroup, first_match};
pub use enabled::{ENABLED, is_enabled};
pub use engine::{Cursor, Dir, Emitted, Step, process_binary_data, read_value};
pub use runtime::{
    Acknowledged, DecodedField, DecodedValue, FractionalCensus, PerlCitation, RawAccess,
    RefusalCounts, TableDecode, all_fractional_census, apply_value_conv, decode_binary_table,
    decode_binary_table_variants, decode_bits, fractional_census, to_tag_value, unknown_fallback,
};
pub use subdir::{BaseExpr, ByteOrderRule, Start, StartExpr, SubdirEdge};

/// Look up a generated table by ExifTool module and table name,
/// e.g. `("Canon", "CameraSettings")`.
#[must_use]
pub fn find_table(module: &str, table: &str) -> Option<&'static BinaryTable> {
    ALL_BINARY_TABLES
        .iter()
        .copied()
        .find(|t| t.module == module && t.table == table)
}

/// A table a live call site asks [`find_table`] for that the generator
/// deliberately never emits.
///
/// [`find_table`] answering `None` is ambiguous on its own: it means either
/// "ExifTool has no such table" or "ExifTool has it and `codegen.py` refused
/// it". A caller cannot tell the two apart, so a lookup that can never succeed
/// looks exactly like one that merely missed -- and a `.or_else(...)` fallback
/// written for the second case silently swallows the first. That is how
/// `raw/metadata.rs::canon_crw_tag_key`'s `find_table("Canon", "AFInfo")` sat
/// dead: it returned `None` on every CRW ever read, the fallback produced a
/// plausible answer anyway, and nothing counted the miss. The corpus-synthesis
/// harness only found it by grepping call sites against the emitted set (613
/// tables, 22 call sites, 21 live -- see `docs/reference/corpus-synthesis.md`).
///
/// This registry is the disambiguation. An entry here asserts, and
/// [`unemitted_tables_are_genuinely_absent`] tests, that the table really is
/// missing from [`ALL_BINARY_TABLES`], and records why in terms of the
/// generator's own refusal counter. The test fails the day a regeneration
/// starts emitting the table, which is exactly when the call site should stop
/// consulting this list and start using the real thing.
///
/// [`unemitted_tables_are_genuinely_absent`]: self::tests::unemitted_tables_are_genuinely_absent
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnemittedTable {
    /// ExifTool module, as [`find_table`]'s first argument.
    pub module: &'static str,
    /// ExifTool table name, as [`find_table`]'s second argument. This is the
    /// *real* Perl name -- an entry here is not a spelling correction.
    pub table: &'static str,
    /// `GROUPS => { 0 => ... }` transcribed from the Perl. Callers need a
    /// group prefix and there is no [`BinaryTable::group0`] to read it from;
    /// taking it from a *different* table that happens to agree is a
    /// coincidence, not a derivation.
    pub group0: &'static str,
    /// `<file>:<line>` of the table's `%Image::ExifTool::...` assignment in
    /// the ExifTool release named by [`EXIFTOOL_VERSION`].
    pub perl: &'static str,
    /// The `stats` key `tools/exiftool-tables/codegen.py` increments when it
    /// refuses this table, so the count here and the count there are the same
    /// number.
    pub refusal: &'static str,
    /// What the generator would have to learn to emit this table.
    pub unblocked_by: &'static str,
}

/// Every [`UnemittedTable`] a non-test call site in `src/` depends on.
///
/// One entry today. `%Image::ExifTool::Canon::AFInfo` is a real table under
/// exactly that name (Canon.pm:6433), but its `PROCESS_PROC` is
/// `\&ProcessSerialData` (Canon.pm:6434, sub at Canon.pm:10518), not
/// `ProcessBinaryData`, so `is_binary_table` (codegen.py:177) rejects it and
/// `gen_table` (codegen.py:568) counts it under `table_not_binary`.
///
/// That refusal is correct and must not be relaxed to make the lookup succeed.
/// In a serial record the keys are *sequence numbers*, not offsets: key 8
/// (`AFAreaXPositions`) is `int16s[$val{0}]`, so key 9 begins wherever key 8
/// ended, which depends on the value of key 0 in the file being read. A flat
/// `BinaryTable` would place every field at `index * 2` and report confident
/// integers from meaningless offsets under real ExifTool tag names --
/// codegen.py's comment at line 550 names this very table as the reason the
/// `PROCESS_PROC` check exists at all.
pub const UNEMITTED_TABLES: &[UnemittedTable] = &[UnemittedTable {
    module: "Canon",
    table: "AFInfo",
    group0: "MakerNotes",
    perl: "Canon.pm:6433",
    refusal: "table_not_binary",
    unblocked_by: "a serial-record table kind in codegen.py: sequence-numbered \
                   keys whose widths resolve against earlier decoded values, \
                   which the fixed `index * increment` BinaryTable shape cannot \
                   express",
}];

/// Look up a deliberately-unemitted table, e.g. `("Canon", "AFInfo")`.
///
/// A `Some` here and a `Some` from [`find_table`] are mutually exclusive by
/// construction; see [`UNEMITTED_TABLES`].
#[must_use]
pub fn find_unemitted_table(module: &str, table: &str) -> Option<&'static UnemittedTable> {
    UNEMITTED_TABLES
        .iter()
        .find(|t| t.module == module && t.table == table)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables are only meaningful relative to a specific ExifTool release,
    /// so the stamp has to survive regeneration. Without it `verify.py` cannot
    /// tell a transcription error from a mismatched oracle, which is the one
    /// failure mode that makes every other check in this module unreadable.
    #[test]
    fn tables_record_their_exiftool_release() {
        assert!(
            EXIFTOOL_VERSION
                .split('.')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())),
            "expected a dotted ExifTool version, found {EXIFTOOL_VERSION:?}"
        );
    }

    /// The stamp must be the release the rest of the repo grades against.
    ///
    /// This is the cheap layer of a three-part guard, and the only one that
    /// needs neither Perl nor Python: `tools/exiftool-tables/verify.py` refuses
    /// a stamp that isn't the pin, and `regen.sh` refuses to transcribe any
    /// other release. Before those existed the check was circular -- verify.py
    /// took its expected release from this very stamp, and both the justfile
    /// recipe and CI chose which ExifTool to fetch the same way -- so these
    /// tables sat at 13.30 for 29 releases while every published coverage
    /// number was measured against the pinned 13.59, and every check passed.
    #[test]
    fn tables_are_transcribed_from_the_pinned_release() {
        let pinned = crate::exiftool_oracle::repo_pin();
        assert_eq!(
            EXIFTOOL_VERSION, pinned,
            "binary_tables.rs was transcribed from ExifTool {EXIFTOOL_VERSION}, but \
             .exiftool-version pins {pinned}; regenerate with `just regen-tables`"
        );
    }

    /// Each [`UNEMITTED_TABLES`] entry must name a table that really is
    /// absent, and this test is how the entry retires itself.
    ///
    /// The registry exists to let a caller say "this lookup can never succeed"
    /// out loud. That claim is only worth anything while it is true: if a
    /// regeneration teaches `codegen.py` to emit `Canon::AFInfo`, an entry
    /// still asserting its absence would route the caller past a table that
    /// now exists -- the same silent-fallback failure the registry was written
    /// to end, just pointing the other way. So the assertion is deliberately
    /// two-sided: absence is checked, and the failure message says to delete
    /// the entry rather than to restore the absence.
    #[test]
    fn unemitted_tables_are_genuinely_absent() {
        for entry in UNEMITTED_TABLES {
            assert!(
                find_table(entry.module, entry.table).is_none(),
                "{}::{} is listed in UNEMITTED_TABLES (refused by codegen as \
                 `{}`, {}) but ExifTool {EXIFTOOL_VERSION}'s tables now emit \
                 it; drop the entry and have the caller use find_table",
                entry.module,
                entry.table,
                entry.refusal,
                entry.perl,
            );
            assert!(
                !entry.group0.is_empty() && !entry.perl.is_empty(),
                "{}::{} must carry the group and the Perl citation it stands in for",
                entry.module,
                entry.table,
            );
        }
    }

    /// `Canon::AFInfo` is refused for a reason that is not "it does not exist".
    ///
    /// Three sibling modules do emit an `AFInfo`, so a bare
    /// `find_table(_, "AFInfo") == None` proves nothing about the name -- it is
    /// specifically Canon's that `ProcessSerialData` keeps out. Pinning both
    /// halves keeps a future reader from "fixing" the Canon miss by copying a
    /// sibling's layout, which would decode sequence numbers as byte offsets.
    #[test]
    fn canon_afinfo_is_refused_while_its_siblings_are_emitted() {
        let entry = find_unemitted_table("Canon", "AFInfo").expect("registered");
        assert_eq!(entry.group0, "MakerNotes", "Canon.pm:6437 GROUPS");
        assert_eq!(entry.refusal, "table_not_binary");
        assert!(find_table("Canon", "AFInfo").is_none());
        // Same table name, different modules, genuinely ProcessBinaryData.
        for module in ["Nikon", "Olympus", "Pentax"] {
            assert!(
                find_table(module, "AFInfo").is_some(),
                "{module}::AFInfo is emitted; only Canon's is serial"
            );
        }
    }

    #[test]
    fn tables_are_present() {
        assert!(
            ALL_BINARY_TABLES.len() > 200,
            "expected the generated table set, found {}",
            ALL_BINARY_TABLES.len()
        );
    }

    #[test]
    fn canon_camera_settings_matches_exiftool() {
        // Spot-check against Canon.pm: MacroMode sits at index 1 of a table
        // whose FORMAT is int16s, so ProcessBinaryData reads it at byte
        // offset 1 * 2 = 2 (the int16u at offset 0 is the record length,
        // which the table deliberately defines no tag for). FIRST_ENTRY is
        // 1, but ExifTool only uses that to bound the Unknown>1 auto-scan
        // -- it never shifts the index-to-offset arithmetic.
        let t = find_table("Canon", "CameraSettings").expect("Canon::CameraSettings");
        assert_eq!(t.default_format, Fmt::Int16s);
        assert_eq!(t.first_entry, 1);

        let f = t
            .fields
            .iter()
            .find(|f| f.name == "MacroMode")
            .expect("MacroMode");
        assert_eq!(f.index, 1);
        assert_eq!(t.byte_offset(f), 2);
        assert_eq!(f.print_conv.apply(1).as_deref(), Some("Macro"));
        assert_eq!(f.print_conv.apply(2).as_deref(), Some("Normal"));
        // A value absent from the enum must not invent a rendering.
        assert_eq!(f.print_conv.apply(99), None);
    }

    #[test]
    fn byte_offsets_scale_with_format_width() {
        let t = find_table("Canon", "CameraSettings").expect("Canon::CameraSettings");
        for f in t.fields {
            // int16s table: every field lands on an even byte boundary.
            assert_eq!(t.byte_offset(f) % 2, 0, "field {} misaligned", f.name);
        }
    }

    #[test]
    fn int_enums_are_sorted_for_binary_search() {
        // `PrintConv::apply` uses `binary_search_by_key`; an unsorted table
        // would silently return wrong or missing values rather than fail.
        for t in ALL_BINARY_TABLES {
            for f in t.fields {
                if let PrintConv::IntEnum(m) = f.print_conv {
                    assert!(
                        m.windows(2).all(|w| w[0].0 < w[1].0),
                        "{}::{} field {} enum is not strictly sorted",
                        t.module,
                        t.table,
                        f.name
                    );
                }
            }
        }
    }

    #[test]
    fn no_empty_names() {
        for t in ALL_BINARY_TABLES {
            for f in t.fields {
                assert!(
                    !f.name.is_empty(),
                    "{}::{} has an unnamed field",
                    t.module,
                    t.table
                );
            }
        }
    }

    /// `Omitted::any()` is the single gate a caller is meant to check before
    /// trusting a decoded value; if it stops noticing a flag, that flag's
    /// semantic starts leaking through as if it were reproduced.
    #[test]
    fn omitted_any_covers_hook_and_subdirectory() {
        assert!(
            Omitted {
                hook: true,
                ..Omitted::NONE
            }
            .any()
        );
        assert!(
            Omitted {
                subdirectory: true,
                ..Omitted::NONE
            }
            .any()
        );
        assert!(!Omitted::NONE.any());
    }

    /// `Omitted::print_conv` is the sixth flag, and the one whose absence
    /// was a *wrong value* rather than a missing one: a field whose ExifTool
    /// `PrintConv` the generator would not reproduce used to be emitted
    /// anyway, carrying `PrintConv::None`, so it reported a raw number where
    /// ExifTool prints a string. `any()` must see it, or that is exactly
    /// what happens again.
    #[test]
    fn omitted_any_covers_print_conv() {
        assert!(
            Omitted {
                print_conv: true,
                ..Omitted::NONE
            }
            .any()
        );
    }

    /// The census sibling of `hook_and_subdirectory_census_matches_the_13_59_
    /// dump`, for the flag that closed Step 28's `conv_dropped` refusal
    /// class. 23 fields across 11 tables carry an ExifTool `PrintConv` --
    /// always a Perl CODE or ARRAY ref -- that
    /// `tools/exiftool-tables/exprs.py`'s `CODE_REFS` registry does not
    /// recognise, and every one of them is now WITHHELD rather than reported
    /// raw. The number is `codegen.py`'s own `conv_dropped` REPORT line.
    ///
    /// The second assertion is the one that matters most: a flagged field
    /// must never also carry a `PrintConv`. The flag says "we refused to
    /// reproduce the conversion"; a conversion sitting next to it would mean
    /// the generator both refused and guessed.
    #[test]
    fn print_conv_refusal_census_matches_the_13_59_dump() {
        let mut refused = 0usize;
        let mut tables = 0usize;
        for t in ALL_BINARY_TABLES {
            let mut here = 0usize;
            for f in t.fields.iter().chain(
                t.variants
                    .iter()
                    .flat_map(|g| g.alternatives.iter().map(|(_, f)| f)),
            ) {
                if !f.omitted.print_conv {
                    continue;
                }
                here += 1;
                assert!(
                    matches!(f.print_conv, PrintConv::None),
                    "{}::{} {} is flagged print_conv-refused but carries a conversion",
                    t.module,
                    t.table,
                    f.name
                );
            }
            refused += here;
            tables += usize::from(here > 0);
        }
        assert_eq!(refused, 23, "fields whose PrintConv the generator refused");
        assert_eq!(tables, 11, "tables carrying at least one such field");
    }

    /// The other half of the `conv_dropped` story: the conversions that were
    /// dropped and are now REPRODUCED, pinned against real carrier files.
    ///
    /// Instruments, per `AGENTS.md`:
    ///
    ///   oracle   `/tmp/oxidex-exiftool-cache/exiftool-pinned.sh` -- ExifTool
    ///            13.59, `-ver` and OOXML.docx-to-DOCX probed. `-n` gives the
    ///            value a `PrintConv` receives, the same run without `-n`
    ///            gives what it must produce; both columns below are that
    ///            pair, quoted, per file.
    ///   carriers `/tmp/oxidex-exiftool-cache/combined-samples/...` -- real
    ///            camera and e-book files, not synthesised bytes.
    ///   subject  the shipped `PrintConv` in `binary_tables.rs`, reached
    ///            through `runtime::render` -- the same call
    ///            `DecodedField::emit` makes, not a re-derivation of it.
    ///
    /// This is deliberately NOT the same evidence as
    /// `tools/exiftool-tables/verify_exprs.py`'s probe battery, which runs
    /// ExifTool's own subroutine over synthetic inputs. That answers "is the
    /// translation right?"; this answers "is it right on the numbers real
    /// files actually contain?", which is the question a corpus run would ask
    /// if any of these tables had a live call site. None of them does today
    /// (`reachability.py`: only `Sony::CameraInfo` among the affected tables
    /// is hand-wired at all), so this test is the corpus check standing in
    /// for a corpus that cannot reach them yet.
    ///
    /// The Canon rows are all `0 -> "Off"`: every 1D-series carrier in the
    /// corpus has every personal function disabled, so the `'On'` and
    /// `"On ($val)"` branches are exercised by the Perl oracle's probes and
    /// by `exprs.rs`'s unit tests, not by a carrier. Said rather than papered
    /// over -- a branch no file reaches is not a branch this test covers.
    #[test]
    fn recovered_conversions_match_the_pinned_oracle_on_real_carriers() {
        // (module, table, field, ExifTool `-n` value, ExifTool printed value,
        //  carrier)
        const CASES: &[(&str, &str, &str, i64, &str, &str)] = &[
            // exiftool-pinned.sh -s -G1 [-n] -UncompressedTextLength Palm.mobi
            //   [MOBI] UncompressedTextLength : 171966   (-n)
            //   [MOBI] UncompressedTextLength : 172 kB
            // Palm.pm:121-124 -> ExifTool.pm:6851-6871.
            (
                "Palm",
                "MOBI",
                "UncompressedTextLength",
                171_966,
                "172 kB",
                "Palm.mobi",
            ),
            // exiftool-pinned.sh -s -G1 [-n] -PF* Canon/CanonEOS-1DmkII.jpg
            //   [CanonCustom] PF0CustomFuncRegistration : 0   (-n)
            //   [CanonCustom] PF0CustomFuncRegistration : Off
            // CanonCustom.pm:1100/1119/1131 -> :36 -> :2624-2628.
            (
                "CanonCustom",
                "PersonalFuncs",
                "PF0CustomFuncRegistration",
                0,
                "Off",
                "Canon/CanonEOS-1DmkII.jpg",
            ),
            (
                "CanonCustom",
                "PersonalFuncs",
                "PF19ContinuousShootSpeed",
                0,
                "Off",
                "Canon/CanonEOS-1DS.jpg",
            ),
            (
                "CanonCustom",
                "PersonalFuncs",
                "PF31OriginalDecisionData",
                0,
                "Off",
                "Canon/CanonEOS-1DSmkII.jpg",
            ),
        ];
        for (module, table, name, raw, want, carrier) in CASES {
            let t = find_table(module, table)
                .unwrap_or_else(|| panic!("{module}::{table} is not in the generated set"));
            let f = t
                .fields
                .iter()
                .find(|f| f.name == *name)
                .unwrap_or_else(|| panic!("{module}::{table} has no field {name}"));
            assert_eq!(
                runtime::render(f.print_conv, &DecodedValue::Integer(*raw)).as_deref(),
                Some(*want),
                "{module}::{table} {name} on {carrier}: ExifTool 13.59 prints {want:?} \
                 for the raw value {raw}",
            );
        }

        // Nikon's FocusPosition fields are `_variants` alternatives -- one per
        // sensor geometry, picked by a `Condition` on `$$self{Model}`. Each
        // alternative's own `PrintConv` is checked here against the carrier
        // whose model selects it; which alternative wins is `cond.rs`'s
        // question, not this test's.
        //
        // exiftool-pinned.sh -s -G1 [-n] -FocusPosition{Horizontal,Vertical}
        //   NikonZ7_2.jpg  H: 16 -> "1R of Center"   V: 12 -> "3D from Center"
        //   NikonZ6_2.jpg  H:  5 -> "6L of Center"   V: 10 -> "3D from Center"
        //   NikonZ30.jpg   H: 10 -> "C"              V:  6 -> "C"
        // Nikon.pm:13420-13428 (LeftRight) and :13434-13442 (UpDown).
        const NIKON: &[(&str, &str, i64, &str, &str)] = &[
            (
                "AFInfo2V0300",
                "FocusPositionHorizontal",
                16,
                "1R of Center",
                "NikonZ7_2.jpg",
            ),
            (
                "AFInfo2V0300",
                "FocusPositionVertical",
                12,
                "3D from Center",
                "NikonZ7_2.jpg",
            ),
            (
                "AFInfo2V0300",
                "FocusPositionHorizontal",
                5,
                "6L of Center",
                "NikonZ6_2.jpg",
            ),
            (
                "AFInfo2V0300",
                "FocusPositionVertical",
                10,
                "3D from Center",
                "NikonZ6_2.jpg",
            ),
            (
                "AFInfo2V0300",
                "FocusPositionHorizontal",
                10,
                "C",
                "NikonZ30.jpg",
            ),
            (
                "AFInfo2V0300",
                "FocusPositionVertical",
                6,
                "C",
                "NikonZ30.jpg",
            ),
        ];
        for (table, name, raw, want, carrier) in NIKON {
            let t = find_table("Nikon", table).expect("Nikon table is in the generated set");
            let rendered: Vec<String> = t
                .variants
                .iter()
                .flat_map(|g| g.alternatives.iter().map(|(_, f)| f))
                .filter(|f| f.name == *name)
                .filter_map(|f| runtime::render(f.print_conv, &DecodedValue::Integer(*raw)))
                .collect();
            assert!(
                rendered.iter().any(|r| r == want),
                "Nikon::{table} {name} on {carrier}: ExifTool 13.59 prints {want:?} for \
                 the raw value {raw}, but no alternative's PrintConv rendered it \
                 (got {rendered:?})",
            );
        }
    }

    /// Step 9's accounting identity: the count of fields the generator flags
    /// `hook`/`subdirectory` must equal what a census of the ExifTool 13.59
    /// dump found (measured independently with `tools/exiftool-tables/
    /// codegen.py`'s own report, and cross-checked again by `verify.py`
    /// against the live Perl hashes). Pinning the numbers here means a future
    /// regen that silently stops reading `Hook`/`SubDirectory` -- reopening
    /// the exact silent-drop class this step closed -- fails a test instead
    /// of only showing up as a diff nobody reads.
    #[test]
    fn hook_and_subdirectory_census_matches_the_13_59_dump() {
        let (mut hooks, mut subdirs) = (0usize, 0usize);
        for t in ALL_BINARY_TABLES {
            for f in t.fields {
                hooks += usize::from(f.omitted.hook);
                subdirs += usize::from(f.omitted.subdirectory);
            }
        }
        assert_eq!(hooks, 35, "Hook-carrying emitted fields");
        assert_eq!(subdirs, 63, "SubDirectory-carrying emitted fields");
    }

    /// Step 27's accounting identity, the sequel to the one above: every
    /// `SubDirectory`-carrying field (63 in `fields:` + 5 inside `_variants`
    /// groups = 68, matching `codegen.py`'s `omitted_subdirectory` REPORT
    /// line) either gets a modeled [`subdir::SubdirEdge`] or is refused with
    /// a reason -- never silently neither. At 13.59 the only refusal reason
    /// live is a `ProcessProc` override (Panasonic `PANA`'s three
    /// `Image::ExifTool::ProcessTIFF`-routed `ExifData` fields plus its
    /// `ProcessLeicaLEIC`-routed `MakerNoteLeica5` field -- `PANA`'s fifth
    /// ProcessProc-routed field, `JPEG-likeData`, never reaches this check at
    /// all: its `Format => 'undef[$size-0x10]'` is a data-dependent width
    /// this generator already refuses on unrelated grounds
    /// (`tag_fmt_unsupported`), so it is not among the 68 flagged fields to
    /// begin with. `subdir.rs`'s module doc has the full citation). A future
    /// regen that starts silently dropping edges it used to model, or
    /// silently modeling one it should refuse (e.g. a table that starts
    /// declaring `ByteOrder`/`Validate`, which this schema does not compile
    /// -- see `subdir.rs`), fails this test instead of only showing up as an
    /// unread diff.
    #[test]
    fn subdir_edges_cover_every_subdirectory_flagged_field() {
        let (mut flagged, mut modeled, mut process_proc_refused) = (0usize, 0usize, 0usize);
        let mut check = |f: &Field| {
            if !f.omitted.subdirectory {
                return;
            }
            flagged += 1;
            match f.subdir {
                Some(_) => modeled += 1,
                // The census below only distinguishes "refused" from
                // "modeled" -- see the module-doc citation for why the sole
                // observed refusal reason is a ProcessProc override; a
                // different reason showing up here (rather than as a
                // `modeled` bump) would still pass this loop but should
                // change the assertion below, which is the point of pinning
                // the exact counts rather than just "modeled + refused ==
                // flagged".
                None => process_proc_refused += 1,
            }
        };
        for t in ALL_BINARY_TABLES {
            for f in t.fields {
                check(f);
            }
            for group in t.variants {
                for (_, f) in group.alternatives {
                    check(f);
                }
            }
        }
        assert_eq!(
            flagged, 68,
            "SubDirectory-carrying fields (fields + variants)"
        );
        assert_eq!(modeled, 64, "fields that got a modeled SubdirEdge");
        assert_eq!(
            process_proc_refused, 4,
            "fields refused for a custom ProcessProc (Panasonic PANA ExifData x3, MakerNoteLeica5 x1)"
        );
    }

    /// Step 28's accounting identity, the third in this series: every
    /// emitted table lands in exactly one of enabled / eligible / refused.
    ///
    /// This used to also pin the exact split (`613` tables, `403`
    /// eligible+enabled, `210` refused, `8` enabled) so a regeneration that
    /// silently moved it would fail a test instead of only changing a report
    /// nobody reads. That was the right goal and the wrong mechanism: every
    /// one of those four numbers is a GLOBAL that any table-wiring branch
    /// (an `enabled.rs` addition) or any table-regenerating branch (a
    /// `binary_tables.rs` regen) bumps, so *every* such branch is green in
    /// isolation and *any two* land on the same tree and collide -- a
    /// per-branch assertion cannot see a sibling branch's edit by
    /// construction. That happened four times in one session: `enabled`
    /// 5 -> 7 across two independent table-wiring branches (APE::NewHeader,
    /// H264::RecInfo), 7 -> 8 for a third (Font::PFM), and
    /// eligible+enabled 380 -> 403 / refused 233 -> 210 for a rebase-time
    /// regeneration (Step 24 -- see `enabled.rs`'s and this file's git
    /// history for the citations). `cargo test --workspace` stayed red for
    /// hours after the first collision before anyone noticed it wasn't a
    /// real regression.
    ///
    /// So this test now asserts INVARIANTS -- properties that hold for every
    /// branch, not a value that holds for none:
    ///
    /// 1. every table lands in exactly one class (enabled xor eligible xor
    ///    refused) -- the actual thing the census exists to protect: a table
    ///    that stops landing anywhere, or lands in two classes at once, is
    ///    the generator silently losing track of a table, which is a real
    ///    bug no legitimate branch would ever trigger.
    /// 2. no table is both enabled and refused -- the gates are supposed to
    ///    be a strict narrowing (Gate B can only restrict what Gate A
    ///    already allowed), so if this ever fires, Gate B started overriding
    ///    Gate A instead of composing with it.
    /// 3. every enabled table passes gate A -- restated explicitly here
    ///    (`enabled.rs`'s own `no_allowlist_entry_is_blocked_by_gate_a` test
    ///    enforces it from the allowlist side) so a refactor of either file
    ///    can't quietly drop the guarantee from one side.
    /// 4. `enabled` equals the Gate B allowlist's size (`ENABLED.len()`) --
    ///    i.e. enabled is exactly the allowlist, neither a subset (a listed
    ///    table gate A silently started blocking) nor a superset (a table
    ///    enabled without a corresponding allowlist line). `ENABLED.len()`
    ///    is itself derived from `enabled.rs`, not a number anyone has to
    ///    remember to bump here, so growing the allowlist never touches this
    ///    test.
    ///
    /// What this NO LONGER catches, and why that's acceptable: it will not
    /// notice a regeneration that moves tables between `eligible` and
    /// `refused` (Gate A's static soundness reclassifying tables without
    /// changing who's enabled), because that shift is real, expected
    /// churn -- exactly the thing that kept conflicting. The exact
    /// point-in-time split belongs to a GENERATED report instead:
    /// `just reachability` prints it fresh from these same two files (gate A
    /// out of `binary_tables.rs`, gate B out of `enabled.rs`), and
    /// `just reachability docs/reference/step28-reachability.json` writes
    /// the per-table detail -- legitimate to diff against IF the JSON is
    /// regenerated in the same run, never as a number frozen into a test.
    #[test]
    fn every_table_lands_in_exactly_one_enablement_class() {
        let (mut enabled, mut eligible, mut refused) = (0usize, 0usize, 0usize);
        for table in ALL_BINARY_TABLES {
            match (table.gate_a.passes(), table.enabled()) {
                (true, true) => enabled += 1,
                (true, false) => eligible += 1,
                (false, false) => refused += 1,
                // `is_enabled` re-checks gate A, so this is unreachable by
                // construction; asserting it is what keeps that true.
                (false, true) => panic!(
                    "{}::{} is enabled despite gate A blocking it",
                    table.module, table.table
                ),
            }
        }

        // INVARIANT 1: every table lands in exactly one class. Holds for
        // every branch -- wiring a table or regenerating the tables only
        // moves a table between classes, never drops it or double-counts
        // it.
        assert_eq!(
            enabled + eligible + refused,
            ALL_BINARY_TABLES.len(),
            "every table must land in exactly one class"
        );

        // INVARIANT 2 and 3, restated explicitly per-table (both already
        // hold structurally from the match above, via the panicking
        // `(false, true)` arm; asserting them again here, independently of
        // that match, is what stops a future refactor of the match from
        // silently dropping either guarantee).
        for table in ALL_BINARY_TABLES {
            let passes_a = table.gate_a.passes();
            let is_on = table.enabled();
            assert!(
                !(is_on && !passes_a),
                "{}::{} is both enabled and refused",
                table.module,
                table.table
            );
            if is_on {
                assert!(
                    passes_a,
                    "{}::{} is enabled but gate A blocks it: {:?}",
                    table.module, table.table, table.gate_a.blocked_by
                );
            }
        }

        // INVARIANT 4: `enabled` is exactly the Gate B allowlist's size.
        // Every allowlisted table must pass gate A
        // (`enabled.rs::no_allowlist_entry_is_blocked_by_gate_a` enforces
        // that from the allowlist side), so the count this census finds
        // enabled must equal `ENABLED.len()` -- derived from the allowlist,
        // not hand-counted, so this line never needs editing when the
        // allowlist grows.
        assert_eq!(
            enabled,
            ENABLED.len(),
            "enabled tables must equal the Gate B allowlist size"
        );

        // The exact point-in-time split (enabled / eligible / refused of
        // ALL_BINARY_TABLES.len()) is intentionally NOT asserted here --
        // see the doc comment above. Run `just reachability` for the live
        // numbers.
    }

    /// A refused table must say WHY, in the generator's own counter names.
    /// A bare `false` would make the reachability report's
    /// "refused-with-reason" column unbuildable and put us back to a hand
    /// audit -- which is how `docs/reference/corpus-synthesis.md`'s 22-vs-21
    /// discrepancy survived.
    #[test]
    fn every_gate_a_refusal_names_its_reason() {
        for table in ALL_BINARY_TABLES {
            if table.gate_a.passes() {
                assert!(table.gate_a.blocked_by.is_empty());
                continue;
            }
            assert!(
                !table.gate_a.blocked_by.is_empty(),
                "{}::{} is refused with no reason recorded",
                table.module,
                table.table
            );
            for (reason, count) in table.gate_a.blocked_by {
                assert!(
                    *count > 0,
                    "{}::{} reason {reason} has count 0",
                    table.module,
                    table.table
                );
            }
        }
    }

    /// ExifTool's table-level `PRIORITY` (ExifTool.pm:9471) reached the
    /// schema in Step 28. 86 of the pinned tree's tables declare
    /// `PRIORITY => 0` and one declares `PRIORITY => 2`; of those, the ones
    /// that survive to an emitted ProcessBinaryData table are counted here.
    /// Before this the key was dropped and every engine hardcoded its own
    /// copy of "CameraInfo is priority zero".
    #[test]
    fn table_priority_is_transcribed_not_hardcoded() {
        let zero = ALL_BINARY_TABLES
            .iter()
            .filter(|t| t.priority == Some(0))
            .count();
        assert!(
            zero > 0,
            "PRIORITY => 0 tables must reach the schema; found none"
        );
        // Canon's CameraInfo tables are the named example: `camera_info.rs`
        // documents "Every CameraInfo table is priority zero" and enforced it
        // by hand.
        let t = find_table("Canon", "CameraInfo5D").expect("Canon::CameraInfo5D");
        assert_eq!(t.priority, Some(0), "Canon.pm declares PRIORITY => 0");
    }

    /// `offsets_sound_until` must be set on exactly the tables where a
    /// `var_*` field actually sits before an emitted one -- not on every
    /// table that merely contains a `var_*` field ExifTool declares.
    ///
    /// At 13.59 that is 7 tables covering 88 emitted fields whose static
    /// `index * increment` offset is no longer trustworthy. Step 26 raised
    /// both numbers (from 4/81) for two reasons, and neither is a loosened
    /// guard:
    ///
    /// * the new scalar formats (int64u, fixed32u, extended, ...) mean fields
    ///   that used to be refused for their format are now emitted, and some
    ///   of them sit past an existing hazard boundary -- they were always at
    ///   an unsound offset, they just were not emitted to be counted;
    /// * `var_*` fields are now modeled as data (`Fmt::Var`), so a table with
    ///   several of them emits the second and third, which by definition sit
    ///   past the first one's boundary. BPG::Main, PNG::SubjectScale and
    ///   Photoshop::VersionInfo are exactly that case.
    ///
    /// A `Fmt::Var` field is never decoded (`runtime::decode_field` refuses
    /// it), so counting it here is the conservative direction.
    #[test]
    fn offsets_sound_until_marks_exactly_the_tables_with_a_live_hazard() {
        let mut affected_tables = 0usize;
        let mut affected_fields = 0usize;
        for t in ALL_BINARY_TABLES {
            let Some(bound) = t.offsets_sound_until else {
                continue;
            };
            affected_tables += 1;
            let hit = t.fields.iter().filter(|f| f.index > bound).count();
            assert!(
                hit > 0,
                "{}::{} sets offsets_sound_until but no field sits past it",
                t.module,
                t.table
            );
            affected_fields += hit;
        }
        assert_eq!(affected_tables, 7, "tables with a live var_* offset hazard");
        assert_eq!(affected_fields, 88, "fields past the hazard boundary");

        let expect = [
            ("BPG", "Main", 6),
            ("CanonVRD", "Ver2", 88),
            ("DNG", "ImageSeq", 0),
            ("FLAC", "Picture", 1),
            ("PNG", "SubjectScale", 1),
            ("Photoshop", "SliceInfo", 20),
            ("Photoshop", "VersionInfo", 5),
        ];
        for (module, table, bound) in expect {
            let t = find_table(module, table).unwrap_or_else(|| panic!("{module}::{table}"));
            assert_eq!(
                t.offsets_sound_until,
                Some(bound),
                "{module}::{table} offsets_sound_until"
            );
        }
    }
}
