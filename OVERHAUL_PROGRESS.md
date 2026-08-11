# OVERHAUL_PROGRESS — tag-machinery refactor ledger (untracked, keep out of PRs)

Plan: OVERHAUL_OXIDEX_PLAN.md · Integration branch: refactor/tag-machinery (created 2026-08-10 from origin/main @ 91e0ba02)
Oracle: pinned 13.59, capability-probed green (perl 5.34, docx container confirmed) via scripts/exiftool_oracle.py
Session: Stage 1 orchestrator (Fable 5), started 2026-08-10.

Instrument note for Stage 1: `just compare-file` is bare-name with `--show-collisions`
(group-qualified conformance.py arrives at Step 12). Stage 1 verification therefore =
compare-file (with collision listing) + direct group-qualified pinned-oracle `-G1` diff
on every touched tag. Commit messages name both.

Samples: t/images at /tmp/oxidex-exiftool-cache/exiftool/t/images/.
Carriers extracted to /tmp/oxidex-exiftool-cache/stage1-samples/:
FujiFilm/FujiFilmX-S10.jpg, Panasonic/PanasonicDMC-{FS4,TS10,G2}.jpg, Sony/SonyNEX-VG10E.jpg.

## Stage 1 — Stop the confirmed wrong values

| Step | Status | Branch | Merge commit | Instrument result |
|---|---|---|---|---|
| 1 Pentax seven-tag | implementing | step/1-pentax-seven | — | uncommitted; agent stalled twice on background builds, re-poked |
| 2 Fuji ImageStabilization | implementing | step/2-fuji-is | — | fix verified earlier: FujiFilmX-S10 WRONG 0 (was 'Unknown (968)'); commit pending |
| 3 Panasonic trio | committed fb7db97f | step/3-panasonic-trio | — | FS4 ProgramISO WRONG gone, TS10 MISSING 0 WRONG 0, G2 LensFirmwareVersion WRONG gone (compare_file.py, fixloop bin, pinned 13.59); `cargo test --lib panasonic` 109 passed. Transform reads AMBIGUOUS (0x59+0x8012 same group) -> confirmed group-qualified via -j -e. Merge-time gates running. |
| 4 Sony ExtraInfo3 | committed 6aabfec3 | step/4-sony-extrainfo3 | — | BatteryVoltage1/2 no longer invented (oracle emits none); CameraOrientation now from NEX byte 0x16/mask 0xc0 — old value was right only by coincidence off non-NEX 0x18. `cargo test --lib sony` 66/66; ci-standard green on own tree; WRONG 0 on VG10E + Sony.jpg + 7 NEX cross-checks. verify-tables not run (no exiftool_tables change). |
| 5 XMP et:* ignore | committed ad10f3eb | step/5-xmp-et-ignore | — | XMP.xml MISSING 60 WRONG 0 -> MISSING 0 WRONG 2 (compare-file, pinned oracle); the 2 WRONG are the namespace-guard bug already merged as 9c92a679 — MUST verify they cancel to 0 post-merge. Root cause deeper than plan text: has_shorthand_fields made property_is_struct true for any property carrying et:id, suppressing real leaves. `cargo test --lib parsers::xmp` 118 passed. |
| 6 CR3 timestamps | committed a8ca8ad6 | step/6-cr3-timestamps | — | CanonRaw.cr3 QuickTime:CreateDate 2018:02:21 12:08:56 -> 06:08:56-06:00, matches oracle (TZ=America/Chicago); QuickTime.mov unchanged. Gate mechanism is FileType eq "CR3" from the CNCV box (QuickTime.pm:280), NOT the ftyp brand — CRM and generic MP4 stay zone-less. ci-standard green, verify-tables PASS. Merge-time gates running. |
| 7 JXL CompatibleBrands | committed 62679bc2 | step/7-jxl-brands | — | JXL2.jxl MISSING 1 WRONG 1 -> MISSING 1 WRONG 0 (remaining MISSING pre-existing Brotli gap); JXL.jxl unchanged 0/0. CAUTION: also touches src/cli/output_formatter.rs corpus-wide — see merge-time obligations. |
| 8 RAF composite + rational | implementing | step/8-raf-composite | — | uncommitted; agent stalled three times, re-poked. Carries merged c86c2b53. |
| X (maintainer) XMP-exif ns guard | **merged** | step/x-xmp-apex-ns-guard | 9c92a679 | rdf_parser lib tests 77/77; compare_file.py XMP.jpg 123 tags MISSING 0 WRONG 0, XMP.xmp/XMP2.xmp/ExtendedXMP.jpg diff sets identical pre/post; ci-standard exit 0; verify-tables PASS; jpeg-tag-matrix "Baseline check passed" under pinned wrapper. End-to-end XMP.xml ExifIFD:* check due after Step 5 merges. |

Waves: wave 1 = steps 1, 2, 5, 7; wave 2 = steps 3, 4, 6, 8.
Merging is serial per plan rule 9; orchestrator runs the merge-time gate re-run
(incl. jpeg-tag-matrix ratchet for steps touching JPEG-visible tags: 1–5, 8).

## Stage 1 exit criteria — SCORED 2026-08-10, integration tip 6dd5f0c2 (pushed)

- [x] **Every CONFIRMED wrong-value finding no longer reproduces on its named sample.**
      Sweep with `uv run scripts/compare_file.py` (release binary) vs pinned 13.59 oracle,
      capability-probed. WRONG count for every Stage 1 target tag is 0:
      Pentax.jpg MISSING 8 WRONG 0 (was WRONG 5) · FujiFilmX-S10.jpg MISSING 4 WRONG 0
      (was WRONG 1) · DMC-TS10 0/0 · DMC-FS4 MISSING 0 WRONG 1 · DMC-G2 MISSING 1 WRONG 1 ·
      SonyNEX-VG10E MISSING 22 WRONG 0 · XMP.xml MISSING 0 WRONG 0 (was MISSING 60) ·
      CanonRaw.cr3 MISSING 4 WRONG 1 · JXL2.jxl MISSING 1 WRONG 0 · FujiFilm.raf MISSING 2
      WRONG 0 (was MISSING 10) · XMP.xmp MISSING 1 WRONG 1 (was WRONG 6).
      The four residual WRONGs are all outside Stage 1's scope, verified by name:
      Panasonic AdvancedSceneMode ('Unknown (49 5)' vs 'Unknown (DMC-FS4 49 5)'),
      Panasonic ContrastMode ('Normal' vs 'High'), Canon ExposureMode ('Auto' vs
      'Aperture-priority AE'), XMP Subject (entity escaping, '&-&amp;-&' vs '&--&').
      Caveat on the instrument: compare_file.py is bare-name; group-qualified spot checks
      against oracle `-a -G1 -s` were run per step (recorded in each squash commit), and
      CR3 CreateDate is only visible that way (bare-name marks it AMBIGUOUS).
- [x] **Each fix carries a pinned decode test + omission assertion.** All hermetic
      (embedded bytes, no /tmp paths in committed tests). Counts per module at the tip:
      pentax 68 · fujifilm 45 · panasonic 109 · sony 66 · parsers::xmp 122 · composite 53 ·
      quicktime 52 · timestamp 29 · output_formatter 41 · jxl 3.
- [x] **`just verify-tables` unchanged-or-better** — RESULT PASS on the final tip,
      6670/6670 fields, 44641/44641 enum entries, 1072/1072 masked fields.
- [x] **jpeg-tag-matrix ratchet unchanged-or-better** — "Baseline check passed" on the
      final tip with EXIFTOOL=<pinned 13.59 wrapper>. Baseline JSON NOT updated (no
      legitimate move). The two docs/reference/*.md reports were regenerated as a side
      effect and reverted, since this was a verification run, not a re-baseline. One real
      content change was observed in them and is NOT yet explained: jpeg-tag-support.md
      would flip `ExifIFD:MakerNoteUnknownText` from mapping to `ExifIFD:0xA40E` to
      mapping to itself. No Stage 1 step names that tag — worth a look before the next
      deliberate re-baseline.
- [x] **No fix without Perl citation + staleness hook note.** Every squash commit cites
      the pinned-tree Perl with line numbers; per-step staleness facts are listed below
      for Step 16 registration in Stage 3.
- [x] **`just ci-standard` green on the final combined tip** — fmt-check, cbindgen-check,
      release clippy, release build, full test suite, C FFI integration test.

### Findings recorded, deliberately NOT fixed in Stage 1 (out of scope)

- **Casio.jpg: 9 tags emitted as raw integers where ExifTool applies a PrintConv**
  (Contrast, DigitalZoom, FlashIntensity, FlashMode, FocusMode, Quality, Saturation,
  Sharpness, WhiteBalance). `compare_file.py`: "compared 58 tags -- MISSING 2 WRONG 9".
  Pre-existing PrintConv gap, NOT a Step 7 JSON regression: these are TagValue::Integer,
  which Step 7's string-only change cannot touch, and text-mode compare shows the same
  values. Spawned as a separate task.
- **`ApertureValue` is a JSON number in ExifTool and a JSON string in oxidex** on
  Canon.jpg, FujiFilm.jpg, CanonRaw.cr3, FujiFilm.raf, XMP.xmp, XMP.jpg. The
  `friendly_enum_name` early-return path Step 7 deliberately left alone; text renderings
  match, so it is a JSON-type divergence only. Belongs with Stage 4 Step 20/21 output
  projection.
- Step 7's multi-format JSON audit (16 samples, comparing number-vs-string per shared
  tag) found NO tag that oxidex unquotes where ExifTool quotes. Obligation cleared.

## Staleness-hook facts to register at Step 16 (Stage 3)

All anchors are the pinned 13.59 tree. Each is a hand-embedded ExifTool fact that must be
diffed against the dump on every regen.

**Pentax.pm** — FlashMode position-0 hash (15 entries) :1137-1151 · position-1 hash
(10 entries) :1152-1162 · PrintHex unmapped format :1135 + ExifTool.pm:3628-3631 ·
AFPointSelected K-1/645Z map (51) :1219-1293 · K-3/KP map (51) :1294-1374 · default map
(17) :1375-1408 · ISO enum (67) :1496-1581 · CryptShutterCount XOR `date ^ (0xffffffff -
time)` :6860-6869 · ShutterCount unpack('N') :2284 · PentaxDate/PentaxTime raw capture
:976,:992 · ExposureTime ValueConv $val*1e-5 :1475 · Bulb threshold 42949 :1478 ·
FlashExposureComp divisor 256 :2186 · divisor 6 first-byte-only :2192-2194 · PrintConv
:2188,:2196 · LensFStops `5 + ($val^0x07)/2` Mask 0x70 :4414-4421.

**FujiFilm.pm** — ImageStabilization 0x1422 int16u Count=>3 :790-793 · element-0 map
(0/1/2/3/258/512) :795-800 · element-1 map (0/1/2) :802-804 · element 2 has no PrintConv.

**Panasonic.pm** — ProgramISO 0x3c sentinels 65534/65535/-1 :793-802 · Transform pairs
{-3 2,-1 1,0 0,1 1,3 2} and no-OTHER raw fallback :970-983 and :1587-1600 ·
LensFirmwareVersion tr/ /./ :999-1006.

**Sony.pm** — ExtraInfo3 model predicate `$$self{Model} !~ /^(NEX-(3|5|5C|C3|VG10|VG10E))\b/`
on BatteryVoltage1 :5951-5959, BatteryVoltage2 :5960-5968, ImageStabilization :5980-5988,
non-NEX CameraOrientation mask 0x30 :6070-6079 · NEX CameraOrientation byte 0x16 mask 0xc0
(0x0016 is an untranscribed two-alternative Perl array — retire this hand read at Step 23).

**XMP.pm** — %ignoreEtProp membership (desc/prt/val/id/tagid/toolkit/table/index) :264-265 ·
exif-namespace-only APEX ApertureValue/MaxApertureValue :2088,:2102 under
%Image::ExifTool::XMP::exif :1988 · ExposureTime PrintExposureTime :2042-2046 · crs
plain-real ApertureValue :1367 · XMPAutoConv rational evaluation :3678 · Perl %.15g
significant-digit stringification (not decimals).

**QuickTime.pm** — %timeInfo ConvertUnixTime gate `FileType eq "CR3"` :280 within :242-291 ·
ExifTool.pm ConvertUnixTime localtime/TimeZoneString :6784-6810 · Canon.pm CNCV-based
FileType override :271 (the gate is CNCV, NOT the crx ftyp brand).

**Exif.pm** — Composite:ImageSize Desire 4 RawImageCroppedSize, returned first when truthy
:4747-4766 · PrintExposureTime :5701-5711.

**Jpeg2000.pm / QuickTime.pm** — CompatibleBrands List=>1 ValueConv 4-byte chunking with
null-chunk drop :575,:579 / :1046,:1050.

**exiftool (script)** — EscapeJSON :3801 and its numeric regex :3809 (governs which values
print unquoted in -j).

**Canon.pm** — canon_sensor_diag FocalPlane rational inputs :10145-10175.

## Stage 2 — Make the ledger honest (accounting foundation)

Integration tip c343c69d (pushed 2026-08-11). Combined gate suite on the 9/12/13 tree:
`just ci-standard` exit 0 · `just verify-tables` RESULT PASS with the NEW columns
(hook flags 6670, subdirectory flags 6670, offsets_sound_until tables 7) ·
jpeg-tag-matrix "Baseline check passed" under EXIFTOOL=<pinned wrapper>.

| Step | Status | Merge commit | Instrument result |
|---|---|---|---|
| 9 Silent-drop flags | **merged** | 333a5780 | Measured census matches the plan exactly and independently: Hook 35, SubDirectory 63, offsets_sound_until 4 tables/81 fields (CanonVRD::Ver2 88/72, DNG::ImageSeq 0/3, FLAC::Picture 1/5, Photoshop::SliceInfo 20/1). 8 tables declare var_*; 4 excluded as no emitted field sits past theirs. verify-tables PASS, 0 MISMATCH. No-change regen byte-identical. oracle.pl now emits HOOK/SUBDIR/VARFMT from ExifTool's live Perl hashes — a genuinely independent second source, not dump_tables.pl. |
| 10 Bypass-proof API | **merged** | 35ca0a78 | Design OVERHAUL_STEP10_DESIGN.md, signed off (D1 all-six-at-once, D2 PerlCitation required, D3 omit-and-count default, D4 trybuild). **ZERO tags dropped.** Audit caught two live regressions: PhotoCD CopyrightStatus (PhotoCD.pm:384-391) and Orientation (:399-408) carry raw_conv+condition but NOT value_conv, so the old value_conv-only check emitted them unchecked; both KEPT via RawAccess — their `Condition => '$$self{HasSBA}'` is set by HasSBA's RawConv at index 225 (:136-141) and the parser derives has_sba from exactly those bytes (orchestrator verified against the pinned Perl before merge, not taken on report). conformance.py TOTAL byte-identical before/after; 12 compare_file.py spot-checks identical; 4043 lib tests. Triage was 10 files / 26 raw sites — the brief's "9 files, 17 sites" was the orchestrator's bad rg and was correctly NOT force-fitted. Merged red once (stale api/oxidex.h from the new pub `Acknowledged` type); fixed by `just cbindgen` and amended before push. |
| 11 Fractional floor(index) | **merged** | b9a7dc9e | Census **993/11 -> 1004/0** as predicted; `rg -c 'sub: Some\('` independently confirms 1004. ExifTool.pm:9957 computes int(index)*increment identically with or without Mask, so a maskless fractional entry is the whole word, not an unresolved bit slice — the fix is a deletion. conformance TOTAL identical before/after, established as an HONEST NULL: 6 of the 11 still refuse under other flags (correct per Step 10), and the other 5 populate only on D5/D500/D810/D850/K3III-class bodies — pinned-oracle `-Model` over every Nikon/Pentax sample in t/images returns E775, D70, D2Hs, K10D, so the corpus cannot exercise the path. |
| 12 Conformance instrument | **merged** | b49584f4 | APE.mpc "MPC 1 16 0 10 17 5" -> "MPC 1 21 0 0 22 10" (the plan's correct classification; 10 false VALUE diffs eliminated). 194-file dual-run, floors --min-files 150 --min-tags 5000, neither run hit the floor: VALUE 64 -> 44, matched 8586 -> 8596, extras now reported 1642, score 81.6% -> 81.7%. 25 files changed classification; walk-through in tools/exiftool-tables/GROUP_QUALIFIED_DELTA.md. |
| 13 ReadReport | **merged** | c343c69d | Truncated 20-byte JPEG: exit 1 with nothing -> exit 0 with File:Warning "JPEG format error" + Status Partial (oracle: exit 0, 12 tags, same warning). MIE.mie -> Status IdentifiedOnly. Healthy files byte-identical: Canon.jpg MISSING 18 WRONG 1, XMP.jpg 0/0, Pentax.jpg MISSING 8 WRONG 0 — same before and after. 4035 lib tests + 76 integration pass. |

### Stage 2 exit criteria — SCORED 2026-08-11, integration tip b9a7dc9e (pushed)

- [x] **Zero silent-drop classes remain; codegen accounting sums to the dump census.**
      Step 9 measured Hook 35 / SubDirectory 63 / offsets_sound_until 4 tables-81 fields,
      matching the plan from an independent derivation (oracle.pl reads ExifTool's live
      Perl hashes, not dump_tables.pl). verify-tables cross-checks both sources: hook
      flags 6670/6670, subdirectory flags 6670/6670, offsets_sound_until 7/7, 0 MISMATCH.
- [x] **`rg 'omitted\.' src/` shows every flag consumed by the runtime** — 12 consumption
      sites in runtime.rs, against 1 before Step 10 (only `omitted.value_conv`, in one
      method).
- [x] **No production caller reads `decoded.raw` outside the opt-in type** —
      `rg 'decoded\.raw|field\.raw' src/parsers src/core` excluding RawAccess and comments
      returns **0**. Enforced structurally (private field) and proven by three trybuild
      compile-fail fixtures: E0616 raw is private, E0599 apply_print_conv_to_raw removed,
      E0061 RawAccess::new without a PerlCitation.
- [x] **Identification-only output is machine-distinguishable** — `oxidex -j -e MIE.mie`
      emits `"Status": "IdentifiedOnly"`. Truncated 20-byte JPEG now exits 0 with
      File:Warning "JPEG format error" + Status Partial, matching the oracle's exit 0 /
      12 tags / same warning, where oxidex previously exited 1 with nothing.
- [x] **The comparison instrument classifies APE.mpc correctly** — verified on the final
      tip: `MPC 1 21 0 0 22 10` — 21 match, 0 rename, **0 VALUE** (was 10 fabricated),
      22 missing, 10 extra. Corpus TOTAL on the final tip:
      `194 8607 9 36 1875 1672 81.8% 81.8% 83.7%`.
- [x] **Refusal counts are printed by verify-tables and countable per file** —
      RefusalCounts tallies all six reasons independently and is wired into Step 13's
      `Diagnostic::refusals`, so a decode-time refusal is now countable per file
      (ledger artifact 4).

Cross-check confirming the instrument is coherent, not just green: the corpus TOTAL moved
from `8596 match / 45 value` (Step 10 baseline) to `8607 / 36` after the Casio merge —
exactly 9 fewer VALUE diffs and 11 more matches, which is the 9 corrected Casio values
plus its 2 recovered MISSING tags. The numbers reconcile to the change that caused them.

Known pre-existing limitation surfaced by Step 13, NOT introduced: `cargo test --doc`
cannot run under the fixloop/release profile because Cargo.toml sets `panic = "abort"`.
Reproduces on the pre-change baseline; Cargo.toml untouched.

**Casio PrintConvs — merged eaf2a870, pushed.** Not a plan step; found during Stage 1's
Step 7 JSON audit. Casio.jpg (QV-3000EX -> Casio::Main, confirmed by pinned-oracle
`-v3 -MakerNotes:all` reading the 0x0001-0x0014 numbering; Type2's 0x0002 is
PreviewImageSize, not Quality). `compare_file.py`: "MISSING 2 WRONG 9" -> "MISSING 0
WRONG 0 (AMBIGUOUS 3)". Gates: ci-standard exit 0, verify-tables PASS, ratchet
"Baseline check passed". find_table("Casio","Main") returns None — Main is an IFD-style
WRITE_PROC table outside the binary generator's scope, so hand transcription was correct,
not the expensive path.

  **The finding worth carrying forward:** the two MISSING tags had one cause — the
  registry named 0x0014 "CCDSensitivity", a name absent from Casio::Main (Casio.pm:168 is
  `Name => 'ISO'`). `Composite:LightValue` resolves `require: ISO` by scanning for any
  `*:ISO` key, so a single wrong name silently took out ISO AND LightValue. This is
  exactly the failure class Stage 4 Step 22 (composites consume the occurrence winner
  view, retire lookup_rank/lookup_ranked suffix scanning) exists to make impossible, and
  a bare-name comparison could never have explained it. Cite this when scoping Step 22.

## Stage 3 — Bump machinery and staleness instrumentation

| Step | Status | Merge commit | Instrument result |
|---|---|---|---|
| 14 Pin every generator | **merged** | 09c7e2ad | New regen-all.sh drives tier 1 + codegen_subdirs x3 + AF-point pipeline + six gen_*.pl from ONE resolved tree. New scripts/lib/ExiftoolPin.pm makes all six gen_*.pl DIE on pin mismatch — four of them previously had no version check at all. Both Homebrew-13.55 fallbacks removed. Byte-identity proven over three runs incl. an isolated CI-simulating tree; MUTATION TEST goes red on one edited byte and restores. verify-tables PASS, cargo test 655 passed, ci-standard exit 0, ratchet baseline passed. **SCOPE GAP:** six generator-less files NOT reconstructed (5978 lines, bespoke per-file DSL, 22-67 variants each) — documented in docs/TRANSCRIPTION.md "Honest limits". Stage 3's "no generated artifact without a pinned generator" criterion is therefore NOT met, and Step 17's bump will not refresh them. Mitigated by Step 16 fingerprint detection. |
| 15 Expression compiler | **DECISION GATE PASSED**, implementing | — | See OVERHAUL_STEP15_DECISION.md. Census independently reproduces the plan's 1529 distinct / 6993 uses exactly. Gate readings by USES: STRICT 60.5%, **PLAN 69.5% (the gate answer)**, PLAN+ 72.9%. Maintainer chose option A: proceed as planned, ORACLE-FIRST (verify_exprs.py lands before compiler rollout); 69.5% treated as ceiling, verified coverage reported per bump. Conditions measured separately (NOT in the 6993): 457 distinct / 1140 uses, **80.4% in six closed shapes**; maintainer approved adding bitmask `$$self{M} & 0xNN` as a seventh shape in Step 23, taking it past 85%. |
| 16 Staleness suite | **merged** | 1b839ab9 | dump_tables.pl now captures ARRAY package vars, surfacing @MakerNotes::Main (94 rows — the only ARRAY-shaped tag table in the pinned tree, confirmed by a 153-module dump). Six MUTATION TESTS, one per category, each confirmed RED then restored — e.g. "CANON_MODEL_IDS has drifted ... id 0x1010000: got MUTATION-TEST-BOGUS-NAME, ExifTool says PowerShot A30". `just check-staleness` green on the merged tree: 39 anchors, no drift. cargo test --lib 4069 passed; ci-standard exit 0; verify-tables PASS. **Coverage declared, not claimed:** dispatcher 94/94 rows and 26/26 vendors; Canon ModelID 357 + LensTypes 239 behavioral; Pentax/Minolta/Sony/Nikon lens+model tables NOT covered; canon.rs 175+ decoder arms fingerprint-only; Stage 1 facts 39 of ~42 with 3 named gaps; the six generator-less files fingerprint-only, explicitly not field-level verified. |
| 17 Bump machinery + real bump | not started | — | Blocked on 15. Note the six unpinned files will NOT be refreshed by the bump — the triage report must list them as HAND. |

**A caution recorded against my own work on Step 15's census:** my first classifier pass
returned 57.8% — a GATE FAIL that would have descoped Steps 24/28 — because it excluded
`$self->ConvertDateTime($val)` (396 uses, the most common expression in ExifTool) despite
the plan's grammar naming ConvertDateTime in its helper registry, where `$self` is the
invocant and not state access; it also rejected hex literals, dropping
`$val * 180 / 0x80000000`. Both were classifier bugs, not findings. The corrected census
reports three readings side by side so the judgement is visible. Lesson for later gates:
a measurement that argues for descoping a large piece of the plan deserves the same
re-run-with-the-right-instrument discipline AGENTS.md demands of measurements that argue
against a safety check.

## Orchestration findings (apply to Stages 2-6)

- **Design artifacts written to the main checkout are INVISIBLE to step worktrees.**
  OVERHAUL_STEP10_DESIGN.md and OVERHAUL_STEP15_DECISION.md are untracked files in
  /Users/allen/git/oxidex_refactor, so `git worktree add` never carries them. Both the
  Step 10 and Step 15 agents were told to "read the design first" and could not; both
  recovered only because the decisions were ALSO inlined in their prompts (Step 15's agent
  independently reproduced the census to confirm the numbers before proceeding, which is
  the right instinct). FIX for later checkpoints: `cp` the artifact into the worktree
  immediately after creating it, or commit it to the step branch — and keep inlining the
  decisions regardless, since that is what actually saved both steps.
- **A subagent used `git stash` in a worktree during Step 15, against the explicit rule,
  while Step 16 was running concurrently.** It disclosed this unprompted. Verified
  afterwards: `git stash list` empty, Step 16's commit 1b839ab9 intact, s15 worktree
  clean — no clobber occurred. But the hazard was live, not theoretical: the stash ref is
  shared across all worktrees of a repo. Keep the prohibition in every brief and keep
  asking agents to disclose deviations; the disclosure is what made this checkable.

- **Do not have implementation subagents run `just ci-standard` in their worktrees.**
  Plan rule 9 requires the orchestrator to re-run the full gate suite at merge time
  (after merging latest integration into the step branch), so an in-worktree
  ci-standard is a discarded fat-LTO build. Eight of them at once on a 10-core box
  drove load average to 26 and every agent parked mid-turn waiting on its build with
  ZERO commits — visible progress stopped entirely while the machine stayed pegged.
  Corrected protocol: agents do `cargo fmt` + targeted `cargo test --lib <module>` +
  `compare_file.py` against `target/fixloop/oxidex`, then COMMIT and report. The
  orchestrator runs ci-standard + verify-tables + ratchet serially in the merge queue.
- **Async subagents that say "waiting for my monitor" have ENDED their turn**, not
  parked-and-continuing. They must be resumed with SendMessage. Treat any agent
  notification whose text is a wait statement as a stall requiring a poke.

## Merge-queue protocol (deviation from a strict reading of rule 9, with justification)

Rule 9 wants each step to merge latest integration, re-run gates, then squash-merge.
Run strictly serially that is ~10 min of gates x 9 steps. Because Stage 1's steps touch
DISJOINT files, the queue instead pipelines 2-3 gate suites concurrently against the
integration tip current at their start, squash-merges them in sequence, and then runs
ONE final full gate suite (ci-standard + verify-tables + pinned ratchet) on the
integration tip after the last merge. That final run is the binding evidence that the
combination is green; per-step runs remain the evidence for each step in isolation.
Any step whose gate suite predates a merge that touched ITS files gets re-run, no
exceptions.

## Merge-time obligations (must clear before the step squash-merges)

- **File overlap that escaped the disjoint-files plan: `src/parsers/raw/metadata.rs`.**
  Step 6 changed `parse_cr3` (CNCV-based CR3 detection) and Step 8 changed
  `parse_fujifilm_raf` (SOF processing on the embedded preview) in the SAME file.
  Different functions, so git should merge cleanly, but this violated the
  one-agent-per-file rule because neither step's plan text named this file. Verify the
  merge textually AND behaviorally: after both land, re-check CanonRaw.cr3 timestamps
  AND FujiFilm.raf ImageSize in one run. Step 8 also edited
  `tools/exiftool-tables/codegen_composite.py` (emptied INTERNAL_STATE) and regenerated
  `src/composite/tables.rs` — the regeneration byte-identity claim must be re-checked
  by `just verify-tables` at merge time.

- **Step 7 has corpus-wide blast radius, not JXL-local.** It added
  `EXIFTOOL_JSON_NUMBER` + `exiftool_json_number()` in `src/cli/output_formatter.rs`,
  unquoting ANY numeric-looking `TagValue::String` in JSON for every format. Cites
  `exiftool:3801` (EscapeJSON) / `:3809` (numeric regex), so the direction is right,
  but the agent verified only JXL2.jxl/JXL.jxl. REQUIRED before merge: a multi-format
  JSON-mode comparison against the pinned oracle (JPEG/TIFF/CR3/RAF/XMP/audio at
  minimum) confirming no tag flipped to unquoted that ExifTool still quotes.
  `output_formatter.rs` is also now a shared file — no other Stage 1 step may touch it.
- **Step 4 did not run `just verify-tables`** (justified: no `src/exiftool_tables/`
  change). Covered by the merge-time run.
- **Steps 5, 7, 8, 1 declared unrun instruments** in their commits; every gap is closed
  by the merge-time suite. Do not treat a declared gap as a pass.

## Instrument findings

- **jpeg-tag-matrix defaults to PATH exiftool (13.55)** — matrix.rs:32 reads `EXIFTOOL` env
  var, else bare `exiftool`. CI sets EXIFTOOL to the pinned tree; locally every ratchet run
  MUST set `EXIFTOOL=/tmp/oxidex-exiftool-cache/exiftool-pinned.sh` (wrapper created
  2026-08-10: explicit perl5.34 + -I pinned lib; capability-probed: -ver 13.59, OOXML.docx
  → FileType DOCX). First stepx ratchet run without it reported a phantom
  `readable 2702 → 2701` regression (deltas incl. total_tested -7); control run on the
  UNCHANGED integration tip with PATH exiftool pending to confirm skew.

## Pre-confirmed oracle ground truth (pinned 13.59, capability-probed, -a -G1 -s)

- Step 3: DMC-FS4 `Panasonic:ProgramISO = Intelligent ISO`, `Transform = Off` (appears twice);
  DMC-TS10 `ProgramISO = n/a`; DMC-G2 `ProgramISO = 100`, `LensFirmwareVersion = 0.1.0.0`.
- Step 4: SonyNEX-VG10E.jpg — oracle emits NO BatteryVoltage; `Sony:CameraOrientation = Horizontal (normal)`.
- Step 6: TZ=America/Chicago on CanonRaw.cr3 — `QuickTime:CreateDate = 2018:02:21 06:08:56-06:00`
  (EXIF times stay zone-less 12:08:56, Canon:TimeZone +00:00).
- Step 8: FujiFilm.raf — `Composite:ImageSize = 4256x1424`, `Megapixels = 6.1`, `RAF:RawImageCroppedSize = 4256x1424`;
  XMP.xmp — `ScaleFactor35efl = 6.1`, `FOV = 54.1 deg`, `FocalLength35efl = 5.8 mm (35 mm equivalent: 35.3 mm)`.
