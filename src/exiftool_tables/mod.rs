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
    ALL_BINARY_TABLES, BinaryTable, EXIFTOOL_VERSION, ExprId, Field, Fmt, GateA, Mask, Omitted,
    OtherId, PrintConv,
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
    /// split is 8 enabled / 372 eligible / 233 refused of 613 -- see
    /// `enabled.rs` for why `eligible` is not `enabled`.
    ///
    /// Step 25 moved 25 tables refused -> eligible (350 -> 375) and none the
    /// other way: `enum_int_partial` (50 tables) and `enum_str_partial` (16)
    /// stopped being gate A blockers once a `BITMASK`/`OTHER`-carrying enum
    /// became fully transcribable, and what is left of that population is
    /// the narrower `other_unregistered` (29).
    ///
    /// `enabled` has since moved 5 -> 8, one measured allowlist line at a
    /// time and never as a side effect of regeneration: `APE::NewHeader`
    /// (85598504) and `H264::RecInfo` (aa9b47ad) each added a line without
    /// updating this count, so it read 5 while the artifacts said 7 -- the
    /// exact drift this test exists to catch, caught late. `Font::PFM`
    /// (`enabled.rs`) is the eighth. `eligible + enabled` is unchanged at
    /// 380 throughout, because enabling a table moves it between classes
    /// rather than creating one.
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
        assert_eq!(eligible + enabled, 380, "tables passing gate A");
        assert_eq!(refused, 233, "tables gate A blocks");
        // Raised 5 -> 7 by APE::NewHeader and H264::RecInfo, then -> 8 by
        // Font::PFM. Every one of those branches was green alone; this line is
        // the only place their combination is visible, which is why it keeps
        // conflicting and why a per-branch gate cannot protect it.
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
