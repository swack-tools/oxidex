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
    // Olympus::CameraSettings -- slice I-3 of
    // `docs/superpowers/specs/2026-09-06-ifd-tables-design.md`, the second
    // of the six sub-table lines (all six share the evidence layout below;
    // the corpus A/B was run once for the six together, and the numbers a
    // line quotes are its own tags' share of that run). The table is
    // `%Image::ExifTool::Olympus::CameraSettings` (Olympus.pm:1777-2795 in
    // the pinned 13.59 tree), reached from `Olympus::Main`'s 0x2020 edge
    // (Olympus.pm:1196-1216: the inline `CameraSettings` variant with
    // `ByteOrder => 'Unknown'`, or the `SubIFD` `CameraSettingsIFD` pointer)
    // -- which the engine follows during the Main walk once this line is in
    // force (`ifd_engine::descend`; the hand call site is
    // `olympus.rs::parse_located`'s `sub_table_rows` for "CameraSettings").
    // Corpus carriers: 204 of the 315 JPEGs under
    // `/tmp/oxidex-exiftool-cache/combined-samples/Olympus` write a 0x2020
    // directory (`exiftool-pinned.sh -a -G1 -s -CameraSettingsVersion` over
    // the directory), plus ExifTool's own `t/images/OlympusE1.jpg` and
    // `Olympus2.jpg`, which `tests/olympus_sub_tables_ifd.rs` pins per tag.
    //
    // What the line changes: the 43 rows the generated
    // `IFD_OLYMPUS_CAMERASETTINGS` reports (integer hashes, `BITMASK`
    // hashes, the `"$v[0] (min $v[1], max $v[2])"` and `$val ? $val : "Auto"`
    // expressions, plain values) come from the engine, in ExifTool's entry
    // order relative to every other Main row; the hand
    // `tables::CAMERA_SETTINGS` walk is replaced by
    // `tables::CAMERA_SETTINGS_RESIDUAL` -- the 22 withheld rows (the
    // `PrintConv => [...]` list forms, the `q{}` subs, `AFAreas`,
    // `AFPointSelected`, `ManometerReading`, the `RawConv` version string),
    // the two `IsOffset` preview rows the generator does not transcribe, and
    // two `CAMERA_SETTINGS_ENGINE_MISRENDERS` overrides (`NoiseFilter`,
    // `PictureModeEffect`: joined-key `StrEnum` hashes the engine cannot key
    // on a fixed-count array, the `Main` `WBMode` defect; cited in
    // tables.rs). 0x030a `AFTargetInfo` / 0x030b `SubjectDetectInfo` are
    // edges to tables no line carries and stay refused; 0x0804
    // `StackedImage`, 0x0903 `RollAngle`, 0x0904 `PitchAngle` are withheld
    // and had no hand row, so they stay MISSING.
    //
    // Local pre-measurement (a prediction of the i7 run, not the gate):
    // `tools/exiftool-tables/conformance.py` over the 315-file Olympus
    // directory only, pinned 13.59 oracle (`exiftool-pinned.sh -ver` ->
    // 13.59, `-s3 -FileType t/images/OOXML.docx` -> DOCX), release binaries
    // built at d4d6528b (control) and at this branch with all six lines and
    // every residual in force (treatment; `OXIDEX_ALLOW_DIRTY_TREE=1`, the
    // tree carried these edits uncommitted):
    //
    //     control    TOTAL 315 38210 0 15 410 186 98.9%
    //     treatment  TOTAL 315 38210 0 15 410 186 98.9%
    //
    // Per-file diff of the two `--json-out` files: 0 MISSING -> matched, 0
    // matched -> MISSING, 0 new VALUE, 0 new EXTRA, 0 VALUE fixed -- every
    // row this table reports was already produced by the hand walk, so on
    // this corpus the six lines are a pure producer switch (a FOLD), and
    // `scripts/compare_file.py` on OlympusE1.jpg, OlympusE-M5.jpg,
    // OlympusE-P1.jpg, OlympusSP510UZ.jpg, OlympusXZ-1.jpg and t/images
    // OlympusE1.jpg / Olympus2.jpg is byte-identical before and after. The
    // first treatment build, without `RAW_DEVELOPMENT2_ENGINE_MISRENDERS`'
    // 0x0108 row, measured 2 new VALUE (RawDevMemoryColorEmphasis, see the
    // RawDevelopment2 line); the joined-key overrides here were measured by
    // a build with all six override rows removed (tables.rs).
    //
    // GATE B A/B of record (i7, `/tmp/i7-missing-census.sh` = release build +
    // `conformance.py --json-out` over combined-samples, 4238 files, against
    // the pinned 13.59 oracle, both probes OK; per-file diff
    // `/tmp/i7-ab-diff.py`). The six sub-table lines were measured together:
    // control = census `ifd3c` @ c3a6ec27 (the same tree with no sub-table
    // line), treatment = `ifd3d` @ 35064ec7 (all six lines with their
    // residuals):
    //
    //     control    TOTAL 4238 444743 21 538 5050 10461 98.8%
    //     treatment  TOTAL 4238 444743 21 538 5050 10461 98.8%
    //
    // 0 files touched -- a producer switch: the hand table already produced
    // every row the generated table reports.
    // Per table on the 315-file Olympus directory (the worker's local
    // `conformance.py` A/B at 329dd16f, control = d4d6528b): 204 carriers,
    // 0 MISSING -> matched, 0 matched -> MISSING, 0 new VALUE, 0 new EXTRA. The retirement of this table's two override rows (0x0527
    // NoiseFilter, 0x052d PictureModeEffect; 164cb80e, once the engine keyed
    // a fixed-count value by its joined elements) was measured the same way:
    // census `ifd3e` @ 164cb80e identical to `ifd3d`.
    ("Olympus", "CameraSettings"),
    // Olympus::Equipment -- slice I-3, the first sub-table line; see the
    // CameraSettings entry above for the shared layout. The table is
    // `%Image::ExifTool::Olympus::Equipment` (Olympus.pm:1593-1776), reached
    // from `Olympus::Main`'s 0x2010 edge (Olympus.pm:1175-1195: the inline
    // `Equipment` variant with `ByteOrder => 'Unknown'`, or the `SubIFD`
    // `EquipmentIFD` pointer). Corpus carriers: 184 of the 315 Olympus JPEGs
    // (`-EquipmentVersion` census as above), plus `t/images/OlympusE1.jpg`
    // and `Olympus2.jpg`, pinned in `tests/olympus_sub_tables_ifd.rs`.
    //
    // What the line changes: the 16 rows `IFD_OLYMPUS_EQUIPMENT` reports
    // (`CameraType2`'s string hash, the `sqrt(2)**($val/256)` apertures,
    // `LensProperties`' `sprintf("0x%x")`, the flash hashes, the plain
    // strings and lengths) come from the engine; the hand
    // `tables::EQUIPMENT` walk is replaced by `tables::EQUIPMENT_RESIDUAL`
    // -- the 9 withheld rows (the `sprintf("%x")`+substitution firmware
    // versions, the `s/\s+$//` serial numbers, the `split`/`sprintf`
    // `LensType` and `Extender`, the `RawConv` version string) plus one
    // `EQUIPMENT_ENGINE_MISRENDERS` override, `FocalPlaneDiagonal` (the
    // `rational64u` 10-significant-digit defect `Main` 0x0205 carries, and
    // the ordering against `MAIN_RESIDUAL`'s own override; tables.rs).
    //
    // Local pre-measurement (a prediction of the i7 run, not the gate):
    // `tools/exiftool-tables/conformance.py` over the 315-file Olympus
    // directory only, pinned 13.59 oracle (`exiftool-pinned.sh -ver` ->
    // 13.59, `-s3 -FileType t/images/OOXML.docx` -> DOCX), release binaries
    // built at d4d6528b (control) and at this branch with all six lines and
    // every residual in force (treatment; `OXIDEX_ALLOW_DIRTY_TREE=1`, the
    // tree carried these edits uncommitted):
    //
    //     control    TOTAL 315 38210 0 15 410 186 98.9%
    //     treatment  TOTAL 315 38210 0 15 410 186 98.9%
    //
    // Per-file diff of the two `--json-out` files: 0 MISSING -> matched, 0
    // matched -> MISSING, 0 new VALUE, 0 new EXTRA, 0 VALUE fixed -- every
    // row this table reports was already produced by the hand walk, so on
    // this corpus the six lines are a pure producer switch (a FOLD), and
    // `scripts/compare_file.py` on OlympusE1.jpg, OlympusE-M5.jpg,
    // OlympusE-P1.jpg, OlympusSP510UZ.jpg, OlympusXZ-1.jpg and t/images
    // OlympusE1.jpg / Olympus2.jpg is byte-identical before and after. No carrier
    // in the directory has a non-terminating Equipment FocalPlaneDiagonal
    // quotient or two differing copies (23 files carry both Main's and
    // Equipment's, equal in all 23), so the override is unobservable here
    // and stands on the shape and the order (tables.rs).
    //
    // GATE B A/B of record (i7, `/tmp/i7-missing-census.sh` = release build +
    // `conformance.py --json-out` over combined-samples, 4238 files, against
    // the pinned 13.59 oracle, both probes OK; per-file diff
    // `/tmp/i7-ab-diff.py`). The six sub-table lines were measured together:
    // control = census `ifd3c` @ c3a6ec27 (the same tree with no sub-table
    // line), treatment = `ifd3d` @ 35064ec7 (all six lines with their
    // residuals):
    //
    //     control    TOTAL 4238 444743 21 538 5050 10461 98.8%
    //     treatment  TOTAL 4238 444743 21 538 5050 10461 98.8%
    //
    // 0 files touched -- a producer switch: the hand table already produced
    // every row the generated table reports.
    // Per table on the 315-file Olympus directory (the worker's local
    // `conformance.py` A/B at 329dd16f, control = d4d6528b): 184 carriers,
    // 0 MISSING -> matched, 0 matched -> MISSING, 0 new VALUE, 0 new EXTRA. The retirement of this table's override row (0x0103
    // FocalPlaneDiagonal; 164cb80e, once the engine read a rational as
    // RoundFloat 10) was measured the same way: census `ifd3e` @ 164cb80e
    // identical to `ifd3d`.
    ("Olympus", "Equipment"),
    // Olympus::FocusInfo -- slice I-3's seventh sub-table line, its own
    // commit after the six (whose shared layout is under CameraSettings).
    // The table is `%Image::ExifTool::Olympus::FocusInfo` (Olympus.pm:3302-
    // 3632), reached from `Olympus::Main`'s 0x2050 edge (Olympus.pm:1280-
    // 1300: the inline `FocusInfo` variant with `ByteOrder => 'Unknown'`,
    // or the `SubIFD` `FocusInfoIFD` pointer). It has passed gate A since
    // the first I-3 regen (the slice's condition forms compiled its
    // `_variants`: `$$self{Model} =~ /E-(3|5|30)\b/` and its siblings,
    // `$count != 1`) and was held back until this line, because its hand
    // walk was `tables::FOCUS_INFO` PLUS two model-conditional passes, and
    // the residual had to be derived from the generated `omitted` flags
    // first. Corpus carriers: 119 of the 315 Olympus JPEGs write a 0x2050
    // directory (`exiftool-pinned.sh -j -G1 -Olympus:ZoomStepCount` census
    // over the directory: the row every carrier has), plus
    // `t/images/OlympusE1.jpg` and `Olympus2.jpg`, pinned per tag in
    // `tests/olympus_sub_tables_ifd.rs`.
    //
    // What the line changes: the 11 reported plain rows of
    // `IFD_OLYMPUS_FOCUSINFO` -- `SceneDetect`, the four step counts, the
    // `ExternalFlash` / `InternalFlash` joined-key hashes (`enum_key` keys a
    // fixed-count `Array` since the I-3 engine fixes, so no override:
    // 101 carriers each, 0 new VALUE below), `ExternalFlashBounce`,
    // `ExternalFlashZoom`, `MacroLED`, and 0x2100 `AntiShockWaitingTime`, a
    // row the hand table never had (21 corpus carriers, all E-M / OM / TG /
    // PEN-F bodies) -- come from the engine, as do the two `_variants`
    // alternatives it does not withhold (`AFPoint` on E-Mxxx / OM-x bodies
    // and `AFPointDetails` on every other body, both printed raw: 15 and 35
    // carriers, every one a single integer the hand pass rendered the same
    // way). The hand `tables::FOCUS_INFO` walk is replaced by
    // `tables::FOCUS_INFO_RESIDUAL`, the two withheld `TagDef` rows
    // (`FocusInfoVersion`'s `RawConv`, `ManualFlash`'s `q{}` PrintConv), and
    // the two hand passes `parse_focus_info_sensor_temperature` /
    // `parse_focus_info_model_conditional` keep producing what the table
    // withholds -- `FocusDistance` (whose ValueConv side channel
    // `Composite:DOF` / `FOV` / `HyperfocalDistance` read), the other three
    // `AFPoint` alternatives, the E-M/OM `AFPointDetails`, both
    // `SensorTemperature` alternatives -- and skip the two the engine
    // reports (`olympus::focus_info_engine_reports`, pinned against the
    // generated `omitted` flags by
    // `focus_info_hand_pass_emits_exactly_the_withheld_alternatives`). No
    // override. 0x0328 `AFInfo` is an edge to `Olympus::AFInfo`, which no
    // line carries: refused, as the hand walk never followed it. 0x1600
    // `ImageStabilization` (`Condition => 'not defined
    // $$self{ImageStabilization}'`, withheld) never had a hand row and stays
    // MISSING (36 files in `conformance.py`'s census, the `On, Mode 1/2`
    // form or `Off`; also `t/images/Olympus2.jpg`) -- a follow-up.
    //
    // Local pre-measurement (a prediction of the i7 run, not the gate):
    // `tools/exiftool-tables/conformance.py` over the 315-file Olympus
    // directory only, pinned 13.59 oracle (`exiftool-pinned.sh -ver` ->
    // 13.59, `-s3 -FileType t/images/OOXML.docx` -> DOCX). Control = a
    // release build of the base tree 35064ec7, run on the clean tree (the
    // binary's mtime, 15:18:53, precedes the base commit's 15:20:43 by two
    // minutes -- the build of the tree that was then committed -- and the
    // instrument header flags exactly that); treatment = a release build of
    // this branch with the line, the residual and the hand-pass split in
    // force (`OXIDEX_ALLOW_DIRTY_TREE=1`, the tree carried these five files
    // uncommitted; the header records both):
    //
    //     control    TOTAL 315 38404 0 15 216 186 99.4%
    //     treatment  TOTAL 315 38425 0 15 195 186 99.5%
    //
    // Per-file diff of the two `--json-out` files (the I-3 diff script's
    // classes): 21 MISSING -> matched, all `AntiShockWaitingTime`
    // (OlympusE-M1MarkII.jpg and OlympusE-M5MarkIII.jpg `2000`, nineteen
    // more E-M / OM / TG / PEN-F bodies `0`); 0 matched -> MISSING, 0 new
    // VALUE, 0 new EXTRA, 0 VALUE fixed. Every other row this table reports
    // was already produced by the hand walk or the hand passes (a FOLD),
    // including the 101 `ExternalFlash` and 101 `InternalFlash` joined-key
    // renderings and the 50 raw `AFPoint` / `AFPointDetails` alternatives
    // the engine now owns. `scripts/compare_file.py` on corpus
    // OlympusE1.jpg, OlympusE-M5.jpg, OlympusE-P1.jpg and t/images
    // OlympusE1.jpg / Olympus2.jpg is byte-identical before and after (E1
    // `MISSING 0 WRONG 1`, the pre-existing `CustomSaturation`; E-M5
    // `MISSING 1 WRONG 0`, `MakerNoteByteOrder`; E-P1 `0 / 0`; Olympus2
    // `MISSING 1`, the 0x1600 `ImageStabilization` above), and
    // `Composite:DOF` / `FOV` / `HyperfocalDistance` on the three corpus
    // carriers are unchanged and equal to the oracle's -- they read
    // `FocusDistance`'s ValueConv side channel, which the hand pass still
    // fills.
    //
    // GATE B A/B of record (i7, `/tmp/i7-missing-census.sh` = release build +
    // `conformance.py --json-out` over combined-samples, 4238 files, against
    // the pinned 13.59 oracle, both probes OK; per-file diff
    // `/tmp/i7-ab-diff.py`). Control = census `ifd3e` @ 164cb80e (the six
    // other lines in force, FocusInfo hand-walked), treatment = `ifd3f` @
    // de2f2baf (this line, residual FocusInfoVersion + ManualFlash, the hand
    // model-conditional pass skipping the two variants the engine reports):
    //
    //     control    TOTAL 4238 444743 21 538 5050 10461 98.8%
    //     treatment  TOTAL 4238 444764 21 538 5029 10461 98.8%
    //
    // 21 files; 21 MISSING -> matched, all 0x2100 AntiShockWaitingTime (a row
    // the hand table never had: `2000` on E-M1MarkII / E-M5MarkIII, `0` on 19
    // more E-M / OM / TG / PEN-F bodies); 0 matched -> MISSING; 0 new VALUE;
    // 0 new EXTRA. The worker's local `conformance.py` A/B over the 315-file
    // Olympus directory (control = 35064ec7) had predicted exactly those 21
    // rows; Composite DOF / FOV / HyperfocalDistance on the E-1, E-M5 and
    // E-P1 carriers are unchanged (FocusDistance's value form stays with the
    // hand pass, whose row the generator withholds).
    ("Olympus", "FocusInfo"),
    // Olympus::ImageProcessing -- slice I-3; the shared layout is under
    // CameraSettings. The table is `%Image::ExifTool::Olympus::
    // ImageProcessing` (Olympus.pm:3059-3301), reached from `Olympus::Main`'s
    // 0x2040 edge (Olympus.pm:1259-1279). Corpus carriers: 184 of the 315
    // Olympus JPEGs (`-ImageProcessingVersion` census), plus
    // `t/images/OlympusE1.jpg` and `Olympus2.jpg`.
    //
    // What the line changes: the 58 reported rows of
    // `IFD_OLYMPUS_IMAGEPROCESSING` (the white-balance level arrays, the
    // `Format => 'int16s'` `ColorMatrix` and `CameraTemperature`, the
    // `Binary => 1` face-detect blocks, the crop and calibration values, the
    // integer hashes) come from the engine; the hand
    // `tables::IMAGE_PROCESSING` walk is replaced by
    // `tables::IMAGE_PROCESSING_RESIDUAL` -- two withheld rows
    // (`ImageProcessingVersion`, `MultipleExposureMode`'s list form) and two
    // `IMAGE_PROCESSING_ENGINE_MISRENDERS` overrides (`AspectRatio`,
    // `KeystoneCompensation`: joined-key `StrEnum` hashes over `int8u[2]`,
    // the `WBMode` defect; tables.rs). The four `Unknown => 1`
    // `UnknownBlockN` rows are not reported without `-u`.
    //
    // Local pre-measurement (a prediction of the i7 run, not the gate):
    // `tools/exiftool-tables/conformance.py` over the 315-file Olympus
    // directory only, pinned 13.59 oracle (`exiftool-pinned.sh -ver` ->
    // 13.59, `-s3 -FileType t/images/OOXML.docx` -> DOCX), release binaries
    // built at d4d6528b (control) and at this branch with all six lines and
    // every residual in force (treatment; `OXIDEX_ALLOW_DIRTY_TREE=1`, the
    // tree carried these edits uncommitted):
    //
    //     control    TOTAL 315 38210 0 15 410 186 98.9%
    //     treatment  TOTAL 315 38210 0 15 410 186 98.9%
    //
    // Per-file diff of the two `--json-out` files: 0 MISSING -> matched, 0
    // matched -> MISSING, 0 new VALUE, 0 new EXTRA, 0 VALUE fixed -- every
    // row this table reports was already produced by the hand walk, so on
    // this corpus the six lines are a pure producer switch (a FOLD), and
    // `scripts/compare_file.py` on OlympusE1.jpg, OlympusE-M5.jpg,
    // OlympusE-P1.jpg, OlympusSP510UZ.jpg, OlympusXZ-1.jpg and t/images
    // OlympusE1.jpg / Olympus2.jpg is byte-identical before and after. The two
    // joined-key overrides were measured by the build with all six override
    // rows removed (tables.rs: AspectRatio on 78 carriers,
    // KeystoneCompensation on 18).
    //
    // GATE B A/B of record (i7, `/tmp/i7-missing-census.sh` = release build +
    // `conformance.py --json-out` over combined-samples, 4238 files, against
    // the pinned 13.59 oracle, both probes OK; per-file diff
    // `/tmp/i7-ab-diff.py`). The six sub-table lines were measured together:
    // control = census `ifd3c` @ c3a6ec27 (the same tree with no sub-table
    // line), treatment = `ifd3d` @ 35064ec7 (all six lines with their
    // residuals):
    //
    //     control    TOTAL 4238 444743 21 538 5050 10461 98.8%
    //     treatment  TOTAL 4238 444743 21 538 5050 10461 98.8%
    //
    // 0 files touched -- a producer switch: the hand table already produced
    // every row the generated table reports.
    // Per table on the 315-file Olympus directory (the worker's local
    // `conformance.py` A/B at 329dd16f, control = d4d6528b): 184 carriers,
    // 0 MISSING -> matched, 0 matched -> MISSING, 0 new VALUE, 0 new EXTRA. The retirement of this table's two override rows (0x1112
    // AspectRatio, 0x1900 KeystoneCompensation; 164cb80e) was measured the
    // same way: census `ifd3e` @ 164cb80e identical to `ifd3d`.
    ("Olympus", "ImageProcessing"),
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
    //
    // Slice I-3 amends one sentence of the paragraph above: the sub-tables
    // are now listed, one line each below and above, and the engine follows
    // their edges; FocusInfo, hand-walked through the first six lines,
    // followed as the seventh (its entry above).
    ("Olympus", "Main"),
    // Olympus::RawDevelopment -- slice I-3; the shared layout is under
    // CameraSettings. The table is `%Image::ExifTool::Olympus::
    // RawDevelopment` (Olympus.pm:2856-2935), reached from `Olympus::Main`'s
    // 0x2030 edge (Olympus.pm:1217-1237). Corpus carriers: 103 of the 315
    // Olympus JPEGs (`-RawDevEditStatus` census: the row only this table
    // has), plus `t/images/OlympusE1.jpg` and `Olympus2.jpg`.
    //
    // What the line changes: the 13 reported rows of
    // `IFD_OLYMPUS_RAWDEVELOPMENT` (plain values, the colour-space / engine /
    // edit-status hashes, the `BITMASK` noise-reduction and settings hashes)
    // come from the engine; the hand `tables::RAW_DEVELOPMENT` walk is
    // replaced by `tables::RAW_DEVELOPMENT_RESIDUAL`, the one withheld row
    // (`RawDevVersion`, `RawConv`). No override. A body writing both 0x2030
    // and 0x2031 (`OlympusXZ-1.jpg`, `OlympusE-M1.jpg`) has the
    // RawDevelopment2 copy of every shared name reported last, as ExifTool
    // does, on both the engine path (entry order) and the residual loop.
    //
    // Local pre-measurement (a prediction of the i7 run, not the gate):
    // `tools/exiftool-tables/conformance.py` over the 315-file Olympus
    // directory only, pinned 13.59 oracle (`exiftool-pinned.sh -ver` ->
    // 13.59, `-s3 -FileType t/images/OOXML.docx` -> DOCX), release binaries
    // built at d4d6528b (control) and at this branch with all six lines and
    // every residual in force (treatment; `OXIDEX_ALLOW_DIRTY_TREE=1`, the
    // tree carried these edits uncommitted):
    //
    //     control    TOTAL 315 38210 0 15 410 186 98.9%
    //     treatment  TOTAL 315 38210 0 15 410 186 98.9%
    //
    // Per-file diff of the two `--json-out` files: 0 MISSING -> matched, 0
    // matched -> MISSING, 0 new VALUE, 0 new EXTRA, 0 VALUE fixed -- every
    // row this table reports was already produced by the hand walk, so on
    // this corpus the six lines are a pure producer switch (a FOLD), and
    // `scripts/compare_file.py` on OlympusE1.jpg, OlympusE-M5.jpg,
    // OlympusE-P1.jpg, OlympusSP510UZ.jpg, OlympusXZ-1.jpg and t/images
    // OlympusE1.jpg / Olympus2.jpg is byte-identical before and after. The 103
    // carriers move nothing.
    //
    // GATE B A/B of record (i7, `/tmp/i7-missing-census.sh` = release build +
    // `conformance.py --json-out` over combined-samples, 4238 files, against
    // the pinned 13.59 oracle, both probes OK; per-file diff
    // `/tmp/i7-ab-diff.py`). The six sub-table lines were measured together:
    // control = census `ifd3c` @ c3a6ec27 (the same tree with no sub-table
    // line), treatment = `ifd3d` @ 35064ec7 (all six lines with their
    // residuals):
    //
    //     control    TOTAL 4238 444743 21 538 5050 10461 98.8%
    //     treatment  TOTAL 4238 444743 21 538 5050 10461 98.8%
    //
    // 0 files touched -- a producer switch: the hand table already produced
    // every row the generated table reports.
    // Per table on the 315-file Olympus directory (the worker's local
    // `conformance.py` A/B at 329dd16f, control = d4d6528b): 103 carriers,
    // 0 MISSING -> matched, 0 matched -> MISSING, 0 new VALUE, 0 new EXTRA.
    ("Olympus", "RawDevelopment"),
    // Olympus::RawDevelopment2 -- slice I-3; the shared layout is under
    // CameraSettings. The table is `%Image::ExifTool::Olympus::
    // RawDevelopment2` (Olympus.pm:2936-3050), reached from `Olympus::Main`'s
    // 0x2031 edge (Olympus.pm:1238-1258). Corpus carriers: 2 of the 315
    // Olympus JPEGs, `OlympusXZ-1.jpg` and `OlympusE-M1.jpg`
    // (`-RawDevPictureMode` census: the row only this table has); no
    // `t/images` JPEG carries it, so the pin in
    // `tests/olympus_sub_tables_ifd.rs` is the corpus (`#[ignore]`d) one.
    //
    // What the line changes: the 22 reported rows of
    // `IFD_OLYMPUS_RAWDEVELOPMENT2` come from the engine; the hand
    // `tables::RAW_DEVELOPMENT2` walk is replaced by
    // `tables::RAW_DEVELOPMENT2_RESIDUAL`, the two withheld rows
    // (`RawDevVersion`, `RawDevArtFilter`'s list form). No override. 0x8000
    // `RawDevSubIFD` is an edge to `Olympus::RawDevSubIFD`, which no line
    // carries: refused, as the hand walk never followed it either.
    //
    // Local pre-measurement (a prediction of the i7 run, not the gate):
    // `tools/exiftool-tables/conformance.py` over the 315-file Olympus
    // directory only, pinned 13.59 oracle (`exiftool-pinned.sh -ver` ->
    // 13.59, `-s3 -FileType t/images/OOXML.docx` -> DOCX), release binaries
    // built at d4d6528b (control) and at this branch with all six lines and
    // every residual in force (treatment; `OXIDEX_ALLOW_DIRTY_TREE=1`, the
    // tree carried these edits uncommitted):
    //
    //     control    TOTAL 315 38210 0 15 410 186 98.9%
    //     treatment  TOTAL 315 38210 0 15 410 186 98.9%
    //
    // Per-file diff of the two `--json-out` files: 0 MISSING -> matched, 0
    // matched -> MISSING, 0 new VALUE, 0 new EXTRA, 0 VALUE fixed -- every
    // row this table reports was already produced by the hand walk, so on
    // this corpus the six lines are a pure producer switch (a FOLD), and
    // `scripts/compare_file.py` on OlympusE1.jpg, OlympusE-M5.jpg,
    // OlympusE-P1.jpg, OlympusSP510UZ.jpg, OlympusXZ-1.jpg and t/images
    // OlympusE1.jpg / Olympus2.jpg is byte-identical before and after. The first
    // treatment build (no 0x0108 override) measured exactly 2 new VALUE,
    // `RawDevMemoryColorEmphasis` on OlympusXZ-1.jpg and OlympusE-M1.jpg
    // (oracle `''`, oxidex `0`: the engine withholds the zero-count 0x2031
    // copy and the 0x2030 copy shows through); the override in
    // `RAW_DEVELOPMENT2_RESIDUAL` returns both to matched, which is the
    // treatment line above.
    //
    // GATE B A/B of record (i7, `/tmp/i7-missing-census.sh` = release build +
    // `conformance.py --json-out` over combined-samples, 4238 files, against
    // the pinned 13.59 oracle, both probes OK; per-file diff
    // `/tmp/i7-ab-diff.py`). The six sub-table lines were measured together:
    // control = census `ifd3c` @ c3a6ec27 (the same tree with no sub-table
    // line), treatment = `ifd3d` @ 35064ec7 (all six lines with their
    // residuals):
    //
    //     control    TOTAL 4238 444743 21 538 5050 10461 98.8%
    //     treatment  TOTAL 4238 444743 21 538 5050 10461 98.8%
    //
    // 0 files touched -- a producer switch: the hand table already produced
    // every row the generated table reports.
    // Per table on the 315-file Olympus directory (the worker's local
    // `conformance.py` A/B at 329dd16f, control = d4d6528b): 2 (OlympusXZ-1.jpg, OlympusE-M1.jpg) carriers,
    // 0 MISSING -> matched, 0 matched -> MISSING, 0 new VALUE, 0 new EXTRA. The retirement of this table's override row (0x0108
    // RawDevMemoryColorEmphasis; 164cb80e, once a zero-count entry read as the
    // empty value, ExifTool.pm:6296-6297) was measured the same way: census
    // `ifd3e` @ 164cb80e identical to `ifd3d`.
    ("Olympus", "RawDevelopment2"),
    // Olympus::RawInfo -- slice I-3; the shared layout is under
    // CameraSettings. The table is `%Image::ExifTool::Olympus::RawInfo`
    // (Olympus.pm:3653-3760), reached from `Olympus::Main`'s 0x3000 edge
    // (Olympus.pm:1507-1527). Corpus carriers: NONE -- no JPEG under
    // `combined-samples/Olympus` or `t/images` writes a 0x3000 directory
    // (`exiftool-pinned.sh -a -G1 -s -RawInfoVersion` over the 315 files
    // finds none; it is an ORF-only block and the cache holds no ORF), so
    // this line moved nothing in the local A/B and cannot be pinned on a
    // real carrier here. It is listed so the ORF path reaches the engine's
    // 35 reported rows (14 of them, 0x1000-0x2023, the hand table never had)
    // and so the residual pin in tables.rs guards a regeneration.
    //
    // What the line changes: the hand `tables::RAW_INFO` walk (whose 0x0131
    // and 0x0611 names were stale against 13.59 and are corrected in the
    // same commit) is replaced by `tables::RAW_INFO_RESIDUAL`, the one
    // withheld row (`RawInfoVersion`, `RawConv`). No override.
    //
    // Local pre-measurement (a prediction of the i7 run, not the gate):
    // `tools/exiftool-tables/conformance.py` over the 315-file Olympus
    // directory only, pinned 13.59 oracle (`exiftool-pinned.sh -ver` ->
    // 13.59, `-s3 -FileType t/images/OOXML.docx` -> DOCX), release binaries
    // built at d4d6528b (control) and at this branch with all six lines and
    // every residual in force (treatment; `OXIDEX_ALLOW_DIRTY_TREE=1`, the
    // tree carried these edits uncommitted):
    //
    //     control    TOTAL 315 38210 0 15 410 186 98.9%
    //     treatment  TOTAL 315 38210 0 15 410 186 98.9%
    //
    // Per-file diff of the two `--json-out` files: 0 MISSING -> matched, 0
    // matched -> MISSING, 0 new VALUE, 0 new EXTRA, 0 VALUE fixed -- every
    // row this table reports was already produced by the hand walk, so on
    // this corpus the six lines are a pure producer switch (a FOLD), and
    // `scripts/compare_file.py` on OlympusE1.jpg, OlympusE-M5.jpg,
    // OlympusE-P1.jpg, OlympusSP510UZ.jpg, OlympusXZ-1.jpg and t/images
    // OlympusE1.jpg / Olympus2.jpg is byte-identical before and after. This
    // line has no carrier at all in the directory (see above) and moved
    // nothing by construction.
    //
    // GATE B A/B of record (i7, `/tmp/i7-missing-census.sh` = release build +
    // `conformance.py --json-out` over combined-samples, 4238 files, against
    // the pinned 13.59 oracle, both probes OK; per-file diff
    // `/tmp/i7-ab-diff.py`). The six sub-table lines were measured together:
    // control = census `ifd3c` @ c3a6ec27 (the same tree with no sub-table
    // line), treatment = `ifd3d` @ 35064ec7 (all six lines with their
    // residuals):
    //
    //     control    TOTAL 4238 444743 21 538 5050 10461 98.8%
    //     treatment  TOTAL 4238 444743 21 538 5050 10461 98.8%
    //
    // 0 files touched -- a producer switch: the hand table already produced
    // every row the generated table reports.
    // This table has NO corpus carrier (no JPEG under combined-samples or
    // t/images writes a 0x3000 directory; ORF-only), so the line is
    // unmeasured by construction: its evidence is the residual pin test, the
    // engine's own tests, and the census above moving nothing.
    ("Olympus", "RawInfo"),
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
