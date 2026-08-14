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
    ALL_BINARY_TABLES, BinaryTable, EXIFTOOL_VERSION, ExprId, ExprValue, Field, Fmt, GateA, Mask,
    Omitted, OtherId, PrintConv,
};
pub use cond::{CmpOp, Cond, Ctx, EffectSource, MemberValue, VariantGroup, first_match};
pub use enabled::{ENABLED, is_enabled};
pub use engine::{Cursor, Dir, Emitted, Step, process_binary_data, read_value};
pub use runtime::{
    Acknowledged, DecodedField, DecodedValue, FractionalCensus, PerlCitation, RawAccess,
    RefusalCounts, TableDecode, all_fractional_census, decode_binary_table,
    decode_binary_table_variants, decode_bits, fractional_census, unknown_fallback,
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
    /// emitted table lands in exactly one of enabled / eligible / refused,
    /// and the split is pinned so a regeneration that silently moves it
    /// fails a test rather than only changing a report nobody reads.
    ///
    /// The three counts come from the SAME generated data
    /// `tools/exiftool-tables/reachability.py` reports from, which is the
    /// point: the reachability census is generated, not hand-audited, and
    /// this test is what stops the two from drifting apart. At 13.59 the
    /// split is regenerated from the binary tables -- see
    /// `enabled.rs` for why `eligible` is not `enabled`.
    ///
    /// Step 28's `conv_dropped` class moves tables to eligible when it is
    /// their only blocker. A
    /// field in that class carried a `PrintConv` the generator would not
    /// reproduce and was emitted ANYWAY, reporting a raw number under
    /// ExifTool's own tag name. Those tables are eligible now not because
    /// anything was assumed, but because the drop became an explicit
    /// `Omitted { print_conv: true }` that `DecodedField::emit` withholds
    /// and `RefusalCounts::print_conv` counts -- a counted absence in place
    /// of a silent wrong value.
    ///
    /// `enabled` has since moved 5 -> 8, one measured allowlist line at a
    /// time and never as a side effect of regeneration: `APE::NewHeader`
    /// (85598504) and `H264::RecInfo` (aa9b47ad) each added a line without
    /// updating this count, so it read 5 while the artifacts said 7 -- the
    /// exact drift this test exists to catch, caught late. `Font::PFM`
    /// (`enabled.rs`) is the eighth. Enabling a table moves it between
    /// classes rather than creating one, so it never changes
    /// `eligible + enabled`; closing `conv_dropped` does, by moving tables
    /// in from `refused`.
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
        assert_eq!(
            enabled + eligible + refused,
            ALL_BINARY_TABLES.len(),
            "every table must land in exactly one class"
        );
        assert_eq!(ALL_BINARY_TABLES.len(), 613, "tables emitted");
        assert_eq!(eligible + enabled, 411, "tables passing gate A");
        assert_eq!(refused, 202, "tables gate A blocks");
        // Raised 5 -> 7 when APE::NewHeader and H264::RecInfo were wired to live
        // call sites. Each of those branches was green in isolation; the assertion
        // only broke once both were on the same tree, which is precisely the
        // cross-branch interaction a per-branch gate cannot see.
        //
        // 380 -> 403 eligible+enabled (233 -> 210 refused) is Step 24's own
        // rebase-time regeneration, not a hand edit: carrying oracle-verified
        // ValueConv ExprIds shares `exprs.py`'s TRANSLATIONS/grammar compiler
        // with PrintConv, so the same ledger growth that unlocked ValueConv
        // coverage also translated PrintConv expressions that previously hit
        // `expr_unsupported`/`conv_dropped` -- both GATE_A_DISQUALIFYING. See
        // `tools/exiftool-tables/codegen.py`'s `GATE_A_DISQUALIFYING`.
        assert_eq!(enabled, 8, "tables both gates enable");
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
    /// refused `var_*` field actually sits before an emitted one -- not on
    /// every table that merely contains a `var_*` field ExifTool declares.
    /// At 13.59 that is 4 tables covering 81 already-emitted fields whose
    /// static `index * increment` offset is no longer trustworthy.
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
        assert_eq!(affected_tables, 4, "tables with a live var_* offset hazard");
        assert_eq!(affected_fields, 81, "fields past the hazard boundary");

        let expect = [
            ("CanonVRD", "Ver2", 88),
            ("DNG", "ImageSeq", 0),
            ("FLAC", "Picture", 1),
            ("Photoshop", "SliceInfo", 20),
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
