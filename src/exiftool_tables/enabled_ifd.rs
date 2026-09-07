//! Gate B for the generated IFD-style tables: the measured allowlist of
//! [`IfdTable`]s the IFD engine may walk.
//!
//! Same design as [`super::enabled`] (Step 28 D1, opt-in): a table is OFF
//! until one line here turns it on, and that line carries the corpus A/B that
//! justified it (`tools/exiftool-tables/conformance.py` over the combined
//! samples against the pinned 13.59 oracle: MISSING moved to matched, zero
//! new group-qualified VALUE, zero new EXTRA). [`is_enabled`] re-checks Gate A
//! at runtime, so a line for a table a regeneration stopped being sound for
//! does nothing -- the list can only narrow.

use super::ifd_schema::IfdTable;

/// The allowlist. Sorted by `(module, table)`; [`is_enabled`] binary-searches
/// it. Every entry carries the evidence that put it here, in the comment
/// above it, in the shape `enabled.rs` uses.
pub static ENABLED_IFD: &[(&str, &str)] = &[
    // Olympus::Main -- `src/parsers/tiff/makernotes/olympus.rs`'s
    // `parse_located` (the `find_ifd_table("Olympus", "Main")` block, and
    // the second `walk_main_through_engine` call that places the 0x4000
    // `MainInfo` re-walk last), slice I-2 of
    // `docs/superpowers/specs/2026-09-06-ifd-tables-design.md`. The table is
    // `%Image::ExifTool::Olympus::Main` (Olympus.pm:613-1620 in the pinned
    // 13.59 tree; the top-level MakerNote IFD every Olympus / OM Digital
    // Solutions body writes under the `OLYMP\0`, `OLYMPUS\0II`/`MM` and
    // `OM SYSTEM\0` headers, MakerNotes.pm:560-600). Corpus carriers: the
    // 315 JPEGs under `/tmp/oxidex-exiftool-cache/combined-samples/Olympus`
    // plus ExifTool's own `t/images/Olympus.jpg`, `Olympus2.jpg` and
    // `OlympusE1.jpg`, which `tests/olympus_main_ifd_table.rs` pins per tag
    // against the pinned oracle.
    //
    // What the line changes: the top-level Main tags and, through the
    // table's own 0x4000 edge (Olympus.pm:1528-1548, target = this table),
    // the `MainInfo` re-walk ExifTool performs last, are reported by the IFD
    // engine over the generated `IFD_OLYMPUS_MAIN`; the hand `tables::MAIN`
    // walk is replaced by `tables::MAIN_RESIDUAL` (the six rows the generator
    // withholds or refused: SpecialMode, DigitalZoom, PreviewImage,
    // ZoomedPreviewStart/Length, PreviewImageStart -- plus the three
    // `tables::ENGINE_MISRENDERS` overrides, WBMode / FocalPlaneDiagonal /
    // ManualFocusDistance, whose engine rendering differs from ExifTool
    // today and whose hand rendering therefore lands last; the defects are
    // cited in tables.rs and that list can only shrink). The seven sub-tables
    // (Equipment, CameraSettings, RawDevelopment(2), ImageProcessing,
    // FocusInfo, RawInfo) are NOT listed here, so the engine refuses their
    // edges and the hand walks of them run exactly as before (slice I-3).
    // The generator withholds 0x0201 Quality (PrintConv reads the
    // `CameraType` data member), 0x0207 CameraType (Condition + ValueConv)
    // and 0x0208 TextInfo (custom PROCESS_PROC), so
    // `parse_camera_type_and_quality` stays their only producer, unchanged.
    //
    // GATE B A/B of record (i7, `/tmp/i7-missing-census.sh` = release build
    // + `conformance.py --json-out` over combined-samples, 4238 files,
    // against the pinned 13.59 oracle, both probes OK; per-file diff
    // `/tmp/i7-ab-diff.py`). Control = staging/ifd-2-control @ 99890bec
    // (the same generated tables and grammar, no call site, no allowlist
    // line; its numbers equal the tip 83bff055's gate), treatment = the
    // call-site commit merged, 751a1093:
    //
    //     control    TOTAL 4238 443903 21 598 5830 10461 98.6%
    //     treatment  TOTAL 4238 444530 21 553 5248 10461 98.7%
    //
    // 123 files touched; 583 MISSING -> matched (SerialNumber 95, DataDump
    // 51, Macro 44, SceneMode 38, BWMode 37, DigitalZoom 37,
    // PreCaptureFrames 37, OneTouchWB 37, WhiteBoard 36, WhiteBalanceBias
    // 36, Firmware 32, FlashExposureComp 20, WBMode 20, BodyFirmwareVersion
    // 20, ApertureValue 18, ...); 46 VALUE rows fixed (WhiteBalanceBracket
    // 36, SceneMode 10: the MainInfo-last projection); 0 new EXTRA; and two
    // rows that were wrong DOWNSTREAM of a correct walk, each fixed by its
    // own measured commit on this branch:
    //   - 1 new VALUE, OlympusBrioD100.jpg FlashExposureComp (rational64s
    //     128/0, `inf` in the oracle): `core/exiftool_compat.rs` rule 17
    //     rewrote every infinity to `undef`. Fixed in aed1480f (rule 17
    //     follows ExifTool.pm:6111/6118). Its A/B, 751a1093 -> aed1480f:
    //         TOTAL 4238 444530 21 553 5248 10461 -> 4238 444545 21 538 5248 10461
    //     14 files, 15 VALUE rows fixed (CompressedBitsPerPixel 5,
    //     DigitalZoomRatio 3, ExposureCompensation 3, FlashExposureComp 1,
    //     FocusMode 1, ExposureTime 1, BrightnessValue 1), 0 new VALUE,
    //     0 new EXTRA, 0 matched -> MISSING.
    //   - 1 matched -> MISSING, OlympusD450Z.jpg CameraID (`string`, 32 x
    //     0xFF; the oracle prints 32 `?` through exiftool:3822
    //     `XMP::FixUTF8`, and the hand walk had printed the same): the
    //     engine withheld a non-UTF-8 string. Fixed in 701b9497
    //     (`runtime::fix_utf8`). Its A/B, aed1480f -> 701b9497:
    //         TOTAL 4238 444545 21 538 5248 10461 -> 4238 444547 21 538 5246 10461
    //     2 files, 2 MISSING -> matched (this CameraID, and OlympusFE-120.jpg
    //     SerialNumber), 0 new VALUE, 0 new EXTRA, 0 matched -> MISSING.
    //
    // Local pre-measurement (a prediction of the i7 run, not the gate):
    // `tools/exiftool-tables/conformance.py` over the 315-file Olympus
    // directory only, pinned 13.59 oracle (`exiftool-pinned.sh -ver` ->
    // 13.59, `-FileType t/images/OOXML.docx` -> DOCX), release binaries
    // built at 134008e9 (control) and at this branch (treatment):
    //
    //     control    TOTAL 315 37580 match 0 rename 61 value 994 missing 186 extra 97.3%
    //     treatment  TOTAL 315 38207 match 0 rename 16 value 412 missing 186 extra 98.9%
    //
    // 582 MISSING moved to matched, VALUE 61 -> 16 (the 36 WhiteBalanceBracket
    // and 10 SceneMode rows where ExifTool projects the MainInfo copy over
    // CameraSettings' now agree; one new: OlympusBrioD100.jpg
    // FlashExposureComp 128/0, `inf` in the oracle, rewritten to `undef` by
    // `core/exiftool_compat.rs` rule 17 after this parser produced `inf`),
    // zero new EXTRA. `scripts/compare_file.py` over the same files plus
    // t/images Olympus.jpg/Olympus2.jpg/OlympusE1.jpg: MISSING 938 -> 371,
    // WRONG 16 -> 16 (OlympusD450Z.jpg CameraID, non-UTF-8 bytes, moved from
    // WRONG `????...` to MISSING: the engine refuses rather than re-encodes).
    ("Olympus", "Main"),
];

/// Whether the IFD engine may walk `table`: Gate A (static soundness,
/// re-checked here rather than trusted from the list) AND Gate B (this list).
#[must_use]
pub fn is_enabled(table: &IfdTable) -> bool {
    table.gate_a.passes()
        && ENABLED_IFD
            .binary_search_by(|(module, name)| (*module, *name).cmp(&(table.module, table.table)))
            .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool_tables::{ALL_IFD_TABLES, find_ifd_table};

    // The four tests `enabled.rs` carries for the binary allowlist, over the
    // IFD one. They passed vacuously while `ALL_IFD_TABLES` was the engine
    // branch's empty stub; with the generated file and slice I-2's first
    // line in, they are the real Gate-B guard.

    /// `is_enabled` binary-searches, so an unsorted list would silently fail
    /// to find entries rather than fail loudly.
    #[test]
    fn allowlist_is_sorted_and_unique() {
        assert!(
            ENABLED_IFD.windows(2).all(|w| w[0] < w[1]),
            "ENABLED_IFD must be sorted by (module, table) and free of duplicates"
        );
    }

    /// An allowlist line for a table that does not exist is a typo that would
    /// otherwise be indistinguishable from a table that is simply off.
    #[test]
    fn every_allowlist_entry_names_a_real_table() {
        for (module, table) in ENABLED_IFD {
            assert!(
                find_ifd_table(module, table).is_some(),
                "{module}::{table} is on the IFD allowlist but no such table is generated"
            );
        }
    }

    /// The allowlist can only narrow: Gate A is re-checked at runtime, so a
    /// line here that Gate A blocks must not enable anything. If this ever
    /// fires, the fix is to remove the line, not to relax the gate.
    #[test]
    fn no_allowlist_entry_is_blocked_by_gate_a() {
        for (module, table) in ENABLED_IFD {
            let t = find_ifd_table(module, table).expect("checked above");
            assert!(
                t.gate_a.passes(),
                "{module}::{table} is allowlisted but gate A blocks it: {:?}",
                t.gate_a.blocked_by
            );
        }
    }

    /// Opt-in is the whole design: if `is_enabled` ever defaulted to true,
    /// every never-measured IFD table would start producing tags at once and
    /// no corpus delta would be attributable to any one line.
    #[test]
    fn everything_not_listed_is_off() {
        let enabled: Vec<_> = ALL_IFD_TABLES
            .iter()
            .filter(|t| is_enabled(t))
            .map(|t| (t.module, t.table))
            .collect();
        assert_eq!(
            enabled.len(),
            ENABLED_IFD.len(),
            "exactly the allowlisted tables may be enabled, found {enabled:?}"
        );
    }
}
