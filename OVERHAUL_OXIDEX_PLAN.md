# OxiDex Overhaul Plan: wire tag handling up like ExifTool, with coverage as a computable ledger

**Provenance.** This is Part VI of `~/git/MERGED_EXIFTOOL_OXIDEX_TAG_REVIEW.md` (2026-08-10), extracted verbatim as the operative plan. It was produced by merging two independent reviews — the in-house architecture review (Parts I–IV of that document: ExifTool 13.59 Perl machinery vs oxidex, recommendations R1–R9) and an independent ChatGPT review whose 38 claims were re-verified at oxidex tip `d4456ebc` (36 confirmed, 2 already fixed by #676/#678, 0 refuted). Every work item below rests on a verified finding or on Parts I–IV analysis; the evidence and citations live in that document (Part V holds the verified runtime-correctness findings; Appendix B holds the per-claim disposition).

## Execution rules for agent sessions running this plan

1. **Read `AGENTS.md` at the repo root first.** The four rules that most often bite: (a) never invoke bare `exiftool` from PATH — always the tree pinned by `.exiftool-version` via the oracle plumbing (`scripts/exiftool_oracle.py` / a capability-probed explicit-perl invocation); (b) never approximate a conversion — omit and count; (c) name the instrument next to every number in commits/PRs ("VALUE 0 under group-qualified `just compare-file`", never bare counts); (d) bare-name comparison is group-blind — always compare group-qualified, per file.
2. **Line numbers below are pinned at commit `d4456ebc`.** They will drift; locate code by content (`rg`), not by trusting the line number.
3. **One PR per step.** Steps within Stage 1 are independent and parallelizable across agents (but the Pentax seven are one file — one agent). Later stages have explicit `Deps:`; do not start a step whose deps or whose stage's entry conditions aren't met. Do not start Stage N+1's gated work before Stage N's exit criteria hold.
4. **Gates are hard.** No step is done because the diff "looks right" — it is done when its named *Verify* instrument passes and the acceptance-gate checklist (bottom of this file) is satisfied. When a measurement argues against adding a safety check, re-run it with the instrument the harness itself uses before believing it.
5. **Supporting artifacts.** The full merged review: `~/git/MERGED_EXIFTOOL_OXIDEX_TAG_REVIEW.md`. Raw verification memos (commands, outputs, per-claim rationale): `.claude/worktrees/exiftool-oxidex-tag-review-4ea82c/target/tag-review/verify-*.md` — worktree-local and gitignored, may not survive; Part V of the merged review preserves their substance. The pinned ExifTool 13.59 source: fetchable per `.exiftool-version` (see `tools/exiftool-tables/regen.sh` for the cache layout); oracle samples at `<exiftool-tree>/t/images/`.
6. **Sequencing intent.** Stage 1 stops active data corruption (small independent fixes). Stage 2 is the heart of the plan — it makes coverage a computable ledger. Stage 3 builds the version-bump machinery and executes a real bump. Stage 4 is the occurrence-store refactor. Stage 5 is the schema/engine build. Stage 6 is routing, retirement, and evidence-driven coverage. Design-review checkpoints (maintainer sign-off before proceeding): Step 10's bypass-proof API shape, Step 15's grammar scope + its decision gate, Step 18's `TagOccurrence` type design, Step 28's enablement policy.
7. **Session topology and models.** One orchestrator session per stage — Fable 5 (or Opus) at high effort — and never two orchestrator sessions running this plan concurrently (the progress ledger is the race point; parallelism lives *inside* a session as subagents). Implementation steps are delegated to Sonnet subagents (pass the model override on the agent call; reasoning effort inherits from the session). The design checkpoints in rule 6 are produced by the orchestrator itself — never delegated to an implementation subagent — and stop for maintainer sign-off before any implementation begins.
8. **Progress ledger.** `OVERHAUL_PROGRESS.md` at the repo root, local and deliberately untracked: one line per step — status, branch, PR #, instrument result. Every session reads it before starting and updates it after every step and at stage end. Keep it out of step PRs (it would conflict across parallel branches).
9. **Ship flow per step.** Branch off latest origin/main → implement → run the step's named Verify instrument → merge origin/main and re-verify → PR with the instrument result quoted in the body → CI green → squash-merge. Note: `gh pr merge` can print a cosmetic `fatal: 'main' is already used by worktree` checkout error *after* a successful merge — verify with `gh pr view --json state,mergedAt`, do not retry the merge.
10. **Session prerequisites.** A permission mode that auto-accepts edits (a multi-PR stage stalls on prompts otherwise); `gh` authenticated; `RUSTC_WRAPPER=sccache` for builds; never `git stash` in worktrees (the stash ref is shared across all worktrees and a parallel session can silently clobber it).
11. **Work from a clean tree at origin/main.** If the session's checkout is on another branch or has uncommitted/staged changes (the maintainer's main checkout often carries WIP — e.g. `model-fix-sweep-local`), do NOT switch branches there and do NOT commit files you did not author: create a fresh `git worktree` off latest `origin/main` and do all work in it.

---
## Organizing principle: static accounting, not corpus sampling

The maintainer's goal, verbatim: *"refactoring to wire this up like exiftool ... so we know if we have all tags / enums / etc... or not — not just guessing by analyzing a sample file set."*

Every ExifTool construct at the pinned release — tag, enum entry, mask, condition, conversion, hook, subdirectory edge, composite — must end in exactly one of three states:

1. **Modeled** — represented in the transcription schema and verified against the independent oracle (`verify.py`/`oracle.pl`);
2. **Executed** — compiled/decoded at runtime through an engine whose enablement is instrument-gated;
3. **Refused with a counter** — visibly absent, its class and count reported.

The accounting identity `modeled + executed-only + refused = dumped` is checked in CI over the dump census. Corpus conformance runs are demoted from *discovery* instrument to *validation* instrument: they confirm the ledger, they do not substitute for it. This is the in-house review's premise (never approximate; omit-and-count; the two silent-drop classes are the only breaches of that contract) fused with ChatGPT's "stop emitting fields when required semantics are unresolved."

## Resolved disagreements between the two plans

**D1 — TagOccurrence store vs the in-house "deliberate output-model divergence" stance. Resolution: build the occurrence store (ChatGPT PR group C wins), with a narrowed out-of-scope list.** The in-house "What to deliberately NOT build" section excluded FoundTag's `"Tag (N)"` duplicate keys and families 3–8 as output-mode luxuries. The verified evidence shows the flat map is not merely an output divergence — it corrupts *internal* semantics and the *measurement itself*: irreversible duplicate loss (tagmodel/1.2: ≈209–215 repeated group:name cases in 53/194 files, ≈89–94 with distinct values — instrument-sensitive, Part V §1.1); wrong default winners and dead `-G*`/`-a`/group-qualified requests (1.3/1.4: `-EXIF:Make` returns nothing, `-a` is a no-op); `--no-print-conv` unable to restore raw because formatting happens before storage (1.5); family-1 `System` unrepresentable (1.6); composites fed by a hard-coded rank + suffix scan instead of priority-arbitrated occurrences (composites/6.4, and 6.3's destroyed rationals are a value-forms instance); QuickTime tracks flattened into invented `_N` names (jxl-cr3-warn/6b); ICC groups dependent on the outer container (xmp-icc/7.2); and group-blind comparison misclassifying defects (output-contract/8.5, plus project memory "compare-file is group-blind"). A store that retains group families 0/1 (+ per-instance track/document identity), raw/ValueConv/PrintConv forms, priority, and file order is also *required by the accounting goal*: "every emitted field traces to a fully-resolved schema entry" needs per-occurrence provenance. What survives of the in-house stance: families 4–8, `-u` unknown-tag synthesis, the two-pass `$valPt` retry protocol, and writer-side machinery stay out of scope.

**D2 — Patch hand parsers now (ChatGPT group A) vs wait for the typed engine (R4). Resolution: both, sequenced.** The seven Pentax tags, Fuji ImageStabilization, Panasonic registry trio, and Sony ExtraInfo3 are P0 data-integrity defects (confirmed by verify-2/3/4 with oracle repro) — plausible wrong values under real ExifTool names, worse than absence by the project's own rule. They get fixed immediately in the hand parsers, but each fix must (a) cite the transcribed table/Perl definition it implements and (b) land with an R9(a)-style staleness test so the fix does not become new shadowing. The durable home for these decodes is Stage 5's engine; the staleness tests are the bridge.

**D3 — Ordering: instrumentation-first (in-house phase 1) vs fixes-first (group A). Resolution: fixes are Stage 1 because they are small, independent, and stop active data corruption; the accounting foundation is Stages 2–3 and gates *everything else*; coverage widening (group E) comes last — both plans already agree on that.**

**D4 — Fractional maskless indices.** In-house framed the 11 runtime refusals as "an under-claim, not a bug"; verify-4 confirmed ChatGPT's correction that ExifTool decodes them as the whole word at `int(index)` (ExifTool.pm:9957), so they are recoverable logical alternatives. Resolution: decode at `floor(index)`, keep the fractional identity as order/discriminator metadata (Stage 2, step 11).

**D5 — Composite layer health.** In-house rated it "partial-faithful"; verify-6 confirmed the unqualified-input resolution is a hard-coded rank + suffix scan whose misses get patched one exception at a time (the tip IIQ/EIP fix is itself an instance). Resolution: keep R6's mechanical additions (Inhibit/Override, expression composites) but rebase input selection on the occurrence winner view in Stage 4 — the ChatGPT assessment stands.

**D6 — Numbers.** Use **154** eligible fields across 67 unsupported format spellings (verify-4: two independent instruments at pinned 13.59), not ChatGPT's 155.

**Already done at tip (no work items):** JXL SizeHeader bit decode + File-qualified dimensions — fixed by `d4456ebc` (#678, verify-5); IIQ/EIP composite ImageSize preference — fixed by `0609dd23` (#676, verify-6). Residuals from both remain as steps (CompatibleBrands typing; RawImageCroppedSize desire; `Canon 1D RAW` unreachable-from-file_type note).

---

## Stage 1 — Stop the confirmed wrong values (independent small PRs)

Each PR: cite the Perl definition, include the pinned real sample, assert both the positive value and the omissions, and name the instrument in the PR body ("VALUE 0 under group-qualified `just compare-file`", never bare counts).

**Step 1 — Pentax seven-tag fix.** *Change:* `src/parsers/tiff/makernotes/pentax.rs:964-967` (FlashMode: read `Count=-1` as int16u list, per-element PrintConv joined `"; "`), `:1100-1105` (AFPointSelected: model-conditioned map, K10D default branch), `:1114-1117` (ISO enum map), `:1158-1161` (ShutterCount: `unpack N` + `CryptShutterCount` XOR with PentaxDate/PentaxTime members), `:1277-1282` (ExposureTime: `$val*1e-5` + PrintExposureTime), `:1497-1503` (FlashExposureComp: count dispatch /256 vs /6, PrintConv `%+.1f`-or-`0`, no ` EV` suffix), `:2020-2022` (LensFStops: float division). *Why:* pentax/* — all seven CONFIRMED, six NEW. *Verify:* unit tests pinning each decode on `t/images/Pentax.jpg` bytes; `just compare-file Pentax.jpg` group-qualified shows the seven VALUE diffs gone, zero new EXTRA; the ShutterCount test embeds the arithmetic proof (4096191885 ⊕ date/time = 1648). *Deps:* none. *Effort:* M (seven small fixes, one file).

**Step 2 — Fuji ImageStabilization as int16u[3].** *Change:* `src/parsers/tiff/makernotes/fujifilm.rs:1380-1386` — read the out-of-line 3×int16u array (6 bytes never fit inline; `value_offset` is a pointer), apply the two per-element PrintConv maps; **also correct the enum map** at `:659-668` (3⇒`OIS Lens`, 258⇒`IBIS/OIS + DIS`, 512⇒`Digital`; drop the invented 256 key) — the bonus defect verify-3 found. *Why:* fuji-pana/3.1 CONFIRMED NEW. *Verify:* oracle-diff on a corpus carrier (e.g. FujiFilmX-S10.jpg → `OIS Lens; On (mode 1, continuous); 0`); note the local `t/images` has no carrier — check carrier existence first (memory: local corpus is small). *Deps:* none. *Effort:* S.

**Step 3 — Panasonic ProgramISO / Transform / LensFirmwareVersion.** *Change:* `src/parsers/tiff/makernotes/registries/panasonic.rs:100,121,123` + the scalar fallback at `panasonic.rs:994-999` — carry count/format (int16s×2, int8u×4) and the three conversions (65534/65535/-1 sentinels; joined-pair enum; `tr/ /./`). *Why:* fuji-pana/3.2 CONFIRMED NEW. *Verify:* oracle-diff on DMC-FS4/TS10/G2 corpus carriers (`Intelligent ISO`/`n/a`, `Off`, `0.1.0.0`). *Deps:* none. *Effort:* S.

**Step 4 — Sony ExtraInfo3: enforce conditions, kill the bypass.** *Change:* `src/parsers/tiff/makernotes/sony/amount.rs:688-741` — delete the BatteryVoltage arm that hand-converts `decoded.raw` (the bypass verify-4 flagged as a defect class); refuse every field whose `omitted.condition` is set until the NEX model predicate (Sony.pm:5951-5987) and the NEX 0x16/0xc0 orientation variant are explicitly supplied. Absence is correct output here. *Why:* tables-runtime/4.1, 4.2 CONFIRMED. *Verify:* on `SonyNEX-VG10E.jpg`, oxidex emits neither BatteryVoltage tag (omission assertion) and CameraOrientation matches the oracle from the correct byte/mask; `rg 'decoded\.raw' src/parsers` shows no conversion-bypass sites. *Deps:* none (this is the vanguard of Step 10's general rule). *Effort:* S.

**Step 5 — XMP `et:*` control-attribute ignore set.** *Change:* `src/parsers/xmp/struct_flatten.rs` — port `%ignoreEtProp` (XMP.pm:264-265: et:desc/prt/val/id/tagid/toolkit/table/index), keyed on the resolved ExifTool namespace URI, applied *before* `has_fields`/struct detection. *Why:* xmp-icc/7.1 CONFIRMED NEW — 80 invented `*Id` tags and near-total leaf suppression on `XMP.xml`. *Verify:* golden test on the full pinned `t/images/XMP.xml` asserting zero `*Id` tags and presence of the real leaves (`Make=NIKON` etc.); keep `target/tag-review/et-probe.xmp` as the minimal-repro unit test. *Deps:* none. *Effort:* S.

**Step 6 — CR3 QuickTime timestamps are UTC → local with offset.** *Change:* thread file-type into `src/io/timestamp.rs:102-115` / `src/parsers/quicktime/metadata_extractor.rs`; for CR3, convert the stored UTC instant to local time and render with offset (QuickTime.pm:242-290 rule); keep zone-less rendering for generic QuickTime. Preserve the raw instant (Stage 4 will store it as the raw form). *Why:* jxl-cr3-warn/6a CONFIRMED NEW. *Verify:* `TZ=America/Chicago` test on `CanonRaw.cr3` asserting `2018:02:21 06:08:56-06:00` against the pinned oracle under the same TZ. *Deps:* none. *Effort:* S.

**Step 7 — JXL CompatibleBrands as a typed list.** *Change:* `src/parsers/image/jxl.rs:219-234` — emit a `TagValue` list, not a `"[\"jxl \"]"` string. *Why:* jxl-cr3-warn/3b CONFIRMED NEW (residual of #678; also fix the Megapixels string-vs-number nit noted there). *Verify:* JSON-mode test asserting a real array, matching oracle `-j` output on JXL2.jxl. *Deps:* none. *Effort:* S.

**Step 8 — RAF composite ImageSize + tactical rational carriage.** *Change:* (a) add `RawImageCroppedSize` as desire index 4 in `src/composite/tables.rs:303-310` / `codegen_composite.py` and honor `return $val[4] if $val[4]` first in `src/composite/compute.rs:650-663` (Exif.pm:4747-4766); (b) tactically preserve the unreduced `n/d` string for FocalPlaneX/YResolution through the XMP path (`src/parsers/xmp/rdf_parser.rs:3077,3993`) into the existing `value_forms` sidecar so `canon_sensor_diag` (`compute.rs:457-482`) gets its input. Mark (b) explicitly as superseded by Stage 4's value-form retention. *Why:* composites/6.2, 6.3 CONFIRMED. *Verify:* `FujiFilm.raf` → `Composite:ImageSize 4256x1424`, `Megapixels 6.1`; `XMP.xmp` → ScaleFactor35efl 6.1 / FOV 54.1 / 35.3 mm, all against the pinned oracle. *Deps:* none. *Effort:* M.

**Stage 1 exit criteria:** every CONFIRMED wrong-value finding from verify-2/3/4/5/6/7 no longer reproduces on its named sample under group-qualified `just compare-file` against the pinned 13.59 oracle; each fix carries a pinned decode test plus an omission assertion; `just verify-tables` and the jpeg-tag-matrix ratchet unchanged-or-better; no fix landed without a Perl citation and a staleness hook (see Step 16).

---

## Stage 2 — Make the ledger honest (accounting foundation)

This stage converts every silent behavior into a counted one. It is the heart of the maintainer's ask.

**Step 9 — Close the two silent-drop classes: `Omitted.hook`, `Omitted.subdirectory`, `offsets_sound_until`.** *Change:* `tools/exiftool-tables/codegen.py` (reads neither `Hook` nor `SubDirectory` today — verified), the `Omitted`/`Field` schema in generated `binary_tables.rs`, and refusal counters in the codegen accounting output. Mark the 35 Hook-carrying and 63 SubDirectory-carrying emitted fields; add per-table `offsets_sound_until: Option<i64>` for the 4 tables whose offsets go static-wrong after a refused `var_*` field (81 fields). *Why:* in-house R3 item 1; tables-runtime/4.4, 4.5 CONFIRMED (both ALREADY_COVERED in-house — this is the agreed fix); these are the only breaches of the "recorded, never silently dropped" contract. Verify-4's liveness audit (only `CanonVRD::Ver2` intersects, gated at 0x54) confirms the hazard is latent — land this before any new caller makes it live. *Verify:* `verify.py` gains columns for the new flags; accounting identity: hook-flagged + subdir-flagged counts equal the dump census counts (35/63 at 13.59); `rg 'Hook|SubDirectory' tools/exiftool-tables/codegen.py` is no longer empty. *Deps:* none. *Effort:* S — "closes the layer's only silent-wrong holes; costs days."

**Step 10 — Runtime refuses every unresolved semantic stage; make bypass impossible by API.** *Change:* `src/exiftool_tables/runtime.rs` — `decode_binary_table` withholds fields with unresolved `condition`, `raw_conv`, `value_conv`, `hook`, `subdirectory`, or past `offsets_sound_until`; restructure so callers cannot reach `decoded.raw` without acknowledging omissions (e.g. `DecodedField::emit()` is the only value accessor and consults all flags; an explicit opt-in type carries a caller-supplied predicate/conversion plus an oracle-test obligation). *Why:* tables-runtime/4.2 CONFIRMED — today only `omitted.value_conv` is consulted anywhere, and callers can bypass even that (the Sony arm was live proof). ChatGPT PR group B, in-house R3/R4 preconditions. *Verify:* `rg 'omitted\.(raw_conv|condition|hook|subdirectory)' src/exiftool_tables/runtime.rs` shows consumers; a compile-fail/doc test demonstrating raw access requires the opt-in type; runtime refusal counters exposed (see Step 13) so refusals are countable, not silent. *Deps:* Step 9 (flags must exist). *Effort:* M.

**Step 11 — Decode maskless fractional indices at `floor(index)`.** *Change:* `runtime.rs:171-173` — decode the whole word at the integer offset, retain the fractional suffix as logical identity/order (ExifTool.pm:9957). *Why:* tables-runtime/4.3 CONFIRMED — 11 refused entries incl. Pentax `AFInfoK3III 0.1 AFMode`, NikonCustom `12.1 MaxContinuousRelease` (D4 resolution). *Verify:* the census test's 993/11 split becomes 1004/0; unit tests for both named examples against Perl-computed expected bytes. *Deps:* Step 10 (so the newly decodable fields still respect other omission flags). *Effort:* S.

**Step 12 — Upgrade `conformance.py` from recall-biased bare-name matching to group-qualified, precision-scored, per-file-detailed.** *Change:* `tools/exiftool-tables/conformance.py:168-247, 394-461` — (a) group-qualified matching with cross-group fallback only when the name is unique on both sides; (b) extras enter the report as a precision axis (kept out of the recall score, but budgeted — see gates); (c) `--json-out` carries per-file VALUE/EXTRA identities; (d) severity classes (identity, structural, numeric, date/time, binary, display-only); (e) parser-status counts once Step 13 lands (IdentifiedOnly per format); (f) family-0 compatibility and family-1 structural views once Stage 4 stores families. *Why:* output-contract/8.5 CONFIRMED — the APE.mpc cascade (10 false VALUE diffs + 1 false cross-group match) shows the current instrument manufactures wrong classifications; the measurement must be fixed before it gates Stages 4–6. *Verify:* regression test on the single-file APE.mpc corpus asserting the new classification (11 MPC:* + 11 APE:* MISSING, ID3v1 EXTRA, zero VALUE); dual-run against the old matcher on the breadth corpus with a written delta note. *Deps:* none (benefits from Stage 4 later). *Effort:* M.

**Step 13 — `ReadReport`: machine-readable parse status + structured warning sink.** *Change:* `src/core/operations.rs:341-411` — return `ReadReport { metadata, status: Parsed|Partial|IdentifiedOnly|Unsupported, diagnostics }`; thread a diagnostic sink through parsers replacing the `let _ =` / `if let Ok` / `eprintln!` swallow sites (`src/core/jpeg_helpers.rs:150,210,365,441,480,516`, `src/parsers/png/mod.rs:368-415`); on damage, keep filesystem/identity tags and emit `ExifTool`-style `Warning` tags (ExifTool.pm:5610-5642, 8483 model); strict mode opts back into fail-fast; also count runtime refusals from Step 10 into diagnostics. *Why:* output-contract/8.1 CONFIRMED (identification-only indistinguishable from parsed — the in-house "detected is not parsed" trap made machine-readable); jxl-cr3-warn/8 CONFIRMED (20-byte truncated JPEG: oracle exits 0 with 14 tags + Warning; oxidex exits 1 with nothing). *Verify:* truncated-JPEG gate test (exit 0, filesystem+identity+`Warning: JPEG format error` equivalent); `MIE.mie` reports `IdentifiedOnly`; JSON output carries status; conformance.py consumes it (Step 12e). *Deps:* none. *Effort:* M.

**Stage 2 exit criteria (accounting properties):** zero silent-drop classes remain — every construct in the 13.59 dump is modeled, executed, or refused-with-counter, and the codegen accounting output sums to the dump census (checked by a CI assertion); `rg 'omitted\.' src/` shows every flag consumed by the runtime; no production caller reads `decoded.raw` outside the opt-in type; identification-only output is machine-distinguishable (`status` field present in JSON for every read); the comparison instrument classifies APE.mpc correctly; refusal counts are printed by `just verify-tables` (extended per R3: Format, count, table format, Hook/dynamic presence, omitted semantics, SubDirectory edges).

---

## Stage 3 — Bump machinery and staleness instrumentation

**Step 14 — R1: wire and pin the entire second generation tier.** *Change:* `tools/exiftool-tables/regen.sh` (or sibling `regen-all.sh`) invokes every committed generator — `codegen_subdirs.py` outputs, `dump_af_points.pl`+`codegen_af_points.py`, the six `scripts/gen_*.pl` (removing the 13.55 Homebrew fallbacks in `gen_leica_lens_types.pl:23-25`, `gen_canon_custom_functions2.pl:28`) — all reading the pin, refusing PATH fallbacks, byte-identical on no-change rerun. Reconstruct the six generator-less files (`sony/enciphered_tables.rs`, `sony/plain_tables.rs`, `sony/main_extra_tables.rs`, `nikon/encrypted_tables.rs`, `nikon/settings_tables.rs`, `minolta_a100_tables.rs`) as `dump_tables.pl` consumers. Extend CI `verify-tables` job to re-run each generator and diff against committed output. *Why:* in-house R1 — a bump refreshing tier 1 but not tier 2 creates intra-repo mixed-release skew that nothing reports. *Verify:* CI self-consistency diff green; a mutation test (touch one committed byte → CI red). *Deps:* none. *Effort:* S wiring + M reconstruction.

**Step 15 — R2: expression compiler spike + differential expression oracle → the decision gate.** *Change:* `tools/exiftool-tables/exprs.py` grows a `compile()` path over the closed grammar ($val arithmetic, ternaries, sprintf, interpolation, tr///, IsInt/abs/int/log/exp/sqrt, name-registry of helpers: ConvertDateTime, PrintExposureTime, PrintFNumber, GPS::ToDMS, Decode-UCS2); new `tools/exiftool-tables/verify_exprs.py` executes each translation's Perl original (pinned, capability-probed interpreter) against the Rust evaluation over a probe input set; Rust side `src/exiftool_tables/exprs.rs`. Anything outside the grammar refused, counted. *Why:* in-house R2 — 1,529 distinct untranslated expressions / 6,993 uses, top 20 cover 2,312, ConvertDateTime 396; the biggest open architecture bet, and TRANSCRIPTION.md's own declared unverified surface (ExprId semantics never executed against ExifTool). *Verify:* run the compiler against the full 1,529-expression census; **decision gate:** ≥60–70% of uses within grammar+helpers ⇒ the table-driven Stage 5 holds; below ⇒ descope Step 24/28 toward more hand porting. Differential oracle green on every translated expression; label results "sampled equivalence" with the instrument named. *Deps:* none. *Effort:* M.

**Step 16 — R5 stage 1 + R9(a): staleness/consistency tests for hand-embedded facts.** *Change:* extend `dump_tables.pl` to capture array package variables (currently hashes only) so `@MakerNotes::Main` (94 rows) is dumped; generate a consistency test diffing generated routes against `makernote_dispatcher.rs` arms; same pattern for lens databases (`lens_data.rs`, `canon_lens_database.rs`), canon.rs `const_decoder!`/`bitfield_decoder!` arms, model-ID maps — and the Stage 1 fixes (Steps 1–4) register their facts here. *Why:* in-house R5 stage 1/R9(a) — the #636 regen found 401 stale discrepancies and "nothing could report it"; this is the D2 bridge that keeps Stage 1's hand fixes honest. *Verify:* the tests fail when a dump fact changes and the hand code doesn't (mutation-test one arm); CI job added. *Deps:* Step 14 (dump extension). *Effort:* M (stage-1 slice S).

**Step 17 — R8: `just bump-exiftool <ver>` + structural triage report — then execute a real 13.59→13.60 bump.** *Change:* one recipe + CI workflow: write pin → fetch/cache → capability-probe (Archive::Zip + docx probe) → snapshot old dump JSON → regen all tiers (Steps 14 + 30 when it lands) → verify (widened columns) → `tools/exiftool-tables/triage_bump.py` classifying every JSON-to-JSON delta as AUTO / EXPR / COND / HAND → conformance double-run with floors (`--min-files 200 --min-tags 10000`), zero group-qualified VALUE regressions, MISSING growth ≤ EXPR+COND+HAND count → re-baseline ratchets in the same PR. *Why:* in-house R8 — the repo has never bumped; the triage report is the work queue that replaces fleet rediscovery. *Verify:* the bump PR itself, carrying the triage report as review artifact; AUTO-share computed and recorded. *Deps:* Step 14 hard; Steps 9, 12 make the gates meaningful. *Effort:* M.

**Stage 3 exit criteria:** one real bump executed end-to-end; triage report AUTO-share computable and recorded per bump; every generator pinned, byte-identical on no-change rerun, CI-diffed; staleness tests cover the MakerNotes dispatcher rows and every Stage 1 fix site; no generated artifact exists without a committed, pinned generator (`rg -l 'generated' src/parsers | xargs`-style audit list is empty of orphans).

---

## Stage 4 — The occurrence store and the output contract (D1 resolution)

**Step 18 — Add `TagOccurrence` + `TagSink` behind a compatibility projection.** *Change:* new core type carrying canonical tag ID/name, group families 0/1 (+2 where known) plus per-instance document/track identity, raw / ValueConv / PrintConv forms, priority/Avoid, list flag, file order, and table/byte-range provenance; `TagSink` collects occurrences; a winner-projection reproduces today's `MetadataMap` exactly (zero intended CLI change). `src/core/metadata_map.rs` stays as the projected view. *Why:* tagmodel/1.1–1.6 CONFIRMED (D1); the FoundTag contract (ExifTool.pm:9448-9628) is the model. *Verify:* full-corpus A/B: projection output byte-identical to pre-change output (instrument: conformance.py dual-run, zero deltas); unit tests for winner selection/priority/file-order semantics ported from FoundTag's rules. *Deps:* Stage 2 (Step 12's instrument must be trustworthy before it gates this). *Effort:* L.

**Step 19 — Migrate the exemplar duplicate/group families.** *Change:* JPEG COM segments (`src/core/jpeg_helpers.rs:1226-1237`, `app_parsers.rs:112-134`) emit occurrences (first-arrival winner per Extra-table `Priority => 0`, both retained); QuickTime tracks emit family-1 `Track1..TrackN` instead of `_N` suffixes (`metadata_extractor.rs:158,238,266,856,972,1076,1575`); warnings; filesystem tags get family-1 `System` (`file_metadata.rs:107-188` vs %Extra overrides); one MakerNote family (Pentax, hot from Stage 1). *Why:* tagmodel/1.2, 1.6; jxl-cr3-warn/6b — these exercise duplicates, order, groups, priority. *Verify:* `ExifTool.jpg`: default `Comment` = first Unicode comment, `-a` returns both in file order; `CanonRaw.cr3`: `Track1:TrackID`…`Track4:TrackID` group-qualified match against oracle `-G1 -a`; the Part V §1.1 duplicate scan (≈209–215/53/≈89–94, instrument-sensitive) re-run shows retained occurrences. *Deps:* Step 18. *Effort:* M.

**Step 20 — Output projection: `-a`, `-G*`, group-qualified requests, `-n` from stored forms.** *Change:* `src/cli/args.rs:127-135, 235-246, 330-333` (stop no-op'ing `-G*`, wire `all_tags`), `src/cli/output_formatter.rs:87-96` (family-aware request resolution replacing exact/suffix match); `--no-print-conv` selects the stored raw/ValueConv form; parse-time formatting sites (`file_metadata.rs:134-136`, `canon.rs:~5964`) store raw + conversion instead of a fused string. *Why:* tagmodel/1.3, 1.4, 1.5 CONFIRMED. *Verify:* the ChatGPT/verify-1 matrix as tests: `-Make ExifTool.jpg` → priority winner `Canon`; `-EXIF:Make` → `FUJIFILM`; `Canon.jpg -FNumber` → one winner; `-n` FileSize → `2697`, Canon FocalLength → `34`; `-G0:1` renders `File:System` for filesystem tags. Gate 5 (output-mode matrix) enters CI here. *Deps:* Steps 18–19. *Effort:* M.

**Step 21 — Request-aware extraction + compat/extended output split.** *Change:* `ReadOptions`/`RequestSet` threaded into dispatch (ExifTool.pm:5157-5170, 7682-7692 model); gate `JPEGQualityEstimate` (`jpeg_helpers.rs:1245-1263`) on request; move the eleven default extras (`YCbCrSubSampling_1..3`, `ComponentID_1..3`, `SamplingFactors`, `JPEG:Width/Height`, duplicate `JPEG:ColorComponents` — `app_parsers.rs:355-393`), raw `ExifIFD:0x927C` (`tiff_helpers.rs:836-855`), unconditional hex-named unknowns, and ZIP forensic per-entry tags behind an explicit extended-output mode in a documented namespace; default mode is ExifTool-equivalent. *Why:* output-contract/8.4 CONFIRMED; 59,439 deep-sweep extras make precision unscoreable and the in-house §13 hex-name noise is the same class. *Verify:* default-mode `Canon.jpg` run has zero EXTRA under Step 12's instrument; `-JPEGQualityEstimate` on request matches oracle (61); extended mode documented + tested separately. *Deps:* Step 18 (request state rides the sink); partial can precede. *Effort:* M.

**Step 22 — Composites consume the winner view; ICC groups from provenance.** *Change:* `src/composite/mod.rs:62-165` — retire `lookup_rank`/`lookup_ranked` suffix scanning; dependencies resolve against occurrence winners (bare-key arbitration incl. ValueConv forms — supersedes Step 8(b)'s tactical sidecar); ICC extraction assigns `ICC-header`/`ICC_Profile`/`ICC-cicp` from table provenance at extraction time, retiring the JPEG-only `normalize_metadata_map` (`tag_normalization.rs:18-47,104-152`; PNG `Profile:` leak at `png/mod.rs:398-400`). *Why:* composites/6.4, xmp-icc/7.2 CONFIRMED. *Verify:* `XMP.xmp` sensor-diag path passes via stored rational form (no sidecar); PNG-iCCP probe (`target/tag-review/PNG-iccp.png`) emits `ICC-header:*`+`ICC_Profile:*` matching oracle `-G1`; `rg 'lookup_ranked' src/composite` empty. *Deps:* Steps 18–20. *Effort:* M.

**Stage 4 exit criteria:** every emitted tag is a projection of a `TagOccurrence` with families and forms (no parser inserts a fused display string as the only stored form: `rg 'format!' src/parsers | <audit list>` reviewed to zero known sites); the output-mode matrix (default, `-a`, `-G0:1`, group-qualified request, `-n`, JSON) is a CI gate on the pinned samples; default-mode EXTRA budget on the deep JPEG slice ≈ 0 (measured by Step 12's instrument, precision axis); duplicate-loss scan shows zero irrecoverable losses on `t/images`.

---

## Stage 5 — Schema and engine (the big build)

Order follows in-house R3's numbered slices; each construct extends `oracle.pl`/`verify.py` columns in lockstep so a transcription bug cannot pass silently.

**Step 23 — Conditional variant arrays + member effects (R3 items 2–3).** *Change:* `Variant = { cond, field }` arrays (first-match-wins, GetTagInfo contract), `Cond` compiled from the idiom census with a vetted `regex-lite` subset differentially tested against Perl over the dump's own model-name corpus; `SetMember`/`SetMemberFlag` effects with ExifTool's evaluation-order contract (earlier entries' effects run even when a later entry wins). Re-admits the 112 refused variant arrays — the modal release delta. *Verify:* verify.py variant columns; differential matcher test; Sony ExtraInfo3's 0x16-vs-0x18 orientation becomes model-dispatched data (Step 4's hand predicate retired). *Deps:* Step 15 grammar. *Effort:* L (sliceable).

**Step 24 — ValueConv carriage + R2 rollout (ConvertDateTime first).** *Change:* store compiled ExprId/AST instead of `value_conv: true` (1,127 flagged); helpers by census rank. *Verify:* differential expression oracle green per regen; refusal counter shrinks and is reported per bump. *Deps:* Steps 15, 23 (member-referencing exprs). *Effort:* M.

**Step 25 — BITMASK, PartialEnum, OTHER registry.** *Change:* `PrintConv::Bitmask` + one shared `decode_bits` (retiring ≥4 divergent local copies), `PrintConv::PartialEnum` with the `Unknown (0x%x)`/`Unknown ($val)` fallback chain, deparse-keyed OTHER registry. *Verify:* verify-tables columns; DecodeBits unit tests against ExifTool.pm:6385-6407 semantics. *Deps:* Step 15 for expression PrintConvs. *Effort:* S–M.

**Step 26 — var_*/Hook as data + missing scalar formats + effective groups.** *Change:* `Fmt::Var*` variants + expression counts; `HookEffect` enum (the two census idioms — varSize shift, format switch — lifting `camera_info.rs:239-259`); add int64u/s, fixed16/32, `extended` (AIFF SampleRate hand-port tested against Perl), int32uRev, pstring — the **154-field/67-spelling** backlog; emit effective groups (GetTagTable defaulting + per-tag overrides). *Verify:* backlog counter reaches 0-or-refused-with-reason; AIFF SampleRate = 22050 pinned test. *Deps:* Steps 15, 23. *Effort:* M.

**Step 27 — SubDirectory edges in the main schema.** *Change:* `SubdirEdge { module, table, start, base, byte_order, validate }` over the closed Start/Base grammar; the 63 outer pointers stop being unmarked plain fields (Step 9's flag becomes an edge). *Deps:* Step 15. *Effort:* M.

**Step 28 — R4: one generic BinaryData engine, instrument-gated enablement.** *Change:* grow `decode_binary_table` into the full ProcessBinaryData port (variants/members from `binary_subdir.rs`, varSize+Hook+PRIORITY=>0 from `camera_info.rs`, negative indices, subdir recursion, ReadValue string rules); fold the three engines; enablement per table via two generated gates — static soundness (every field transcribed or flagged) and dynamic conformance (enabling table T yields only MISSING→matched transitions, zero new group-qualified VALUE/EXTRA under Step 12's instrument). *Why:* attacks the 22-of-612 reachability seam; changes bump economics from "write a parser" to "data flows through the engine". *Verify:* the allowlist diff per enabled table is the review artifact; per-table revert is one line. *Deps:* Steps 23–27, 15. *Effort:* L — highest risk on the board, contained by the gates.

**Step 29 — R6 composites: Inhibit/Override, expression composites, triage line.** *Change:* carry Inhibit/Override in the generated graph (LensID/LensID-2 test case); route pure-`@val` expression composites through the compiler; "new composite with no registered computation" becomes a named triage-report line instead of silently never firing. *Deps:* Steps 15, 17. *Effort:* S–M.

**Step 30 — R7: fold tag_sync/-listx into the dump; retire the second extraction path.** *Change:* generate the six-domain YAML from `dump_tables.pl` output (same shape, zero consumer churn); kill the 1,029-row carry-forward and its corrupt rows; wire into regen.sh; retire `sync_tags`; delete the dead `GENERATED_TAG_REGISTRY` stub and unreachable index rows. *Verify:* `tests/tag_registry_invariants.rs` unchanged and green; jpeg-tag-matrix ratchet guards writer regressions; the swap is a same-shape data diff. *Deps:* none hard (schedulable earlier if a bump needs it). *Effort:* M.

**Stage 5 exit criteria:** every emitted field traces to a fully-resolved schema entry (engine-decoded with all semantics compiled, or a hand implementation registered with a staleness test); the three-engine split is gone; reachability is a generated report (tables enabled / eligible / refused-with-reason), not a hand-audit list; verify-tables verifies every schema dimension it ships (formats, counts, groups, variants, masks, hooks-as-effects, edges); one extraction pipeline, one pin gate.

---

## Stage 6 — Routing, retirement, and evidence-driven coverage (group E)

**Step 31 — R5 stage 2: generated MakerNotes routing live, vendor-by-vendor; FixBase engine when conformance demands.** *Deps:* Steps 16, 23, 28. *Effort:* M.

**Step 32 — MPC + standalone MIE (the two verified zero-extraction formats).** *Change:* route `FileFormat::MPC` (port the 32-byte v7 bit header, MPC.pm:79-117), refactor `src/parsers/audio/ape.rs` trailer parsing into a container-independent helper (drop the `MAC ` descriptor requirement for the trailer path), preserve leading/trailing ID3 order as occurrences; add `FileFormat::MIE` dispatch to the existing `src/parsers/mie.rs` (today reachable only as a JPEG trailer, `operations.rs:998`). *Why:* output-contract/8.2, 8.3 CONFIRMED NEW — both are parsers-exist-but-unrouted, the cheapest real coverage on the board. *Verify:* `APE.mpc` → 11 MPC:* + 11 APE:* matching oracle under Step 12's instrument; `MIE.mie` status flips IdentifiedOnly→Parsed with 56 MIE:* tags; `just compare-file` on both. *Deps:* Step 13 (status flip is the metric); occurrence store for ID3 ordering. *Effort:* S–M.

**Step 33 — Ranked format backlog + hand-parser data-half migration (R9b).** *Change:* rank MRC, WTV, JP2, RM, CRW, DICOM, FIT by user value × parser cost using Step 13's IdentifiedOnly census; migrate MakerNote main tables to the engine in descending high-severity VALUE count (Pentax first — Stage 1 already pinned its semantics); rename-only work is filler (the score-to-ceiling spread shows it is not the opportunity). *Deps:* Steps 28, 31. *Effort:* ongoing M.

**Stage 6 exit criteria:** the fleet/human work queue is exactly the triage report's HAND residue; per-format IdentifiedOnly counts monotonically fall and are published per bump; no format ships a parser whose facts lack a staleness test.

---

## Acceptance-gate checklist (merged: ChatGPT gates × repo instruments)

No tag-processing change is complete unless the applicable gates pass, with the instrument named in the PR:

1. **Pinned oracle + capability probe** — `.exiftool-version` tree via `scripts/exiftool_oracle.py`/`src/exiftool_oracle.rs`; never PATH `exiftool`; Archive::Zip/container probe green (a matching `-ver` is not a working oracle).
2. **`just verify-tables`** green, with its scope stated (transcription soundness, not runtime behavior) — scope grows with Steps 9, 23–27.
3. **Targeted real-sample conformance** — exact MATCH/RENAME/VALUE/MISSING/EXTRA before/after via `conformance.py` (post-Step-12: group-qualified, severity-classed), floors `--min-files/--min-tags` on corpus runs.
4. **Group-qualified duplicate/family comparison** where the format can repeat tags (memory: compare-file is group-blind — always the group-qualified mode, per file).
5. **Output-mode matrix** (post-Stage-4): default, `-a`, `-G0:1`, group-qualified request, `-n`/raw, JSON — pinned-sample CI job.
6. **No emitted field with unresolved condition/conversion/layout semantics** — enforced structurally by Step 10's API; spot-checked by `rg` gates in CI.
7. **File-count and tag-count floors** on every corpus sweep (degraded-oracle protection).
8. **Malformed/truncated sample gate** — partial metadata + structured Warning, exit 0 (Step 13's fixture).
9. **Deep-corpus regression with both budgets** — missing-recall AND extra-precision (extras budgeted once Step 21 lands).
10. **jpeg-tag-matrix ratchet** — re-baselined only deliberately, in the same PR as the change that legitimately moves it (bump PRs included).
11. **Severity note** for any remaining VALUE differences — counts alone are not evidence.
12. **Per-bump:** triage report attached; AUTO/EXPR/COND/HAND classified; MISSING growth ≤ classified new-construct debt; zero group-qualified VALUE regressions.

---

## How we will know we have all tags/enums

The end state replaces "run the corpus and eyeball the score" with a closed set of **ledger artifacts**, each generated from the pin, each diffable across bumps:

1. **The dump census** (`dump_tables.pl` → tables.json at the pinned release) — the universe: every table, tag, variant, enum entry, mask, condition, conversion expression, hook, subdirectory edge, composite that ExifTool itself declares. This is the denominator for every claim.
2. **The transcription account** (codegen counters + widened `verify-tables`) — for every construct class: modeled count + refused count, with refusal reasons, summing to the census. After Stage 2 there is no third bucket: `rg`-provable absence of silent drops.
3. **The expression ledger** (R2 census + differential oracle) — translated expressions (each differentially executed against Perl) vs refused, ranked by use count.
4. **The reachability report** (R4 gates) — tables statically sound / dynamically enabled / refused, generated, not hand-audited; plus the runtime refusal counters surfaced through `ReadReport` diagnostics so a decode-time refusal is countable per file.
5. **The staleness suite** (R9a) — every hand-embedded ExifTool fact (dispatcher rows, lens DBs, enum arms, the Stage 1 fixes) diffed against the dump on every regen; a failure is a named work item, never invisible drift.
6. **The bump triage diff** (`triage_bump.py`) — every upstream delta classified AUTO/EXPR/COND/HAND; the AUTO share is the single number that says how much of a release the machinery absorbed unaided.
7. **The occurrence provenance** (Stage 4) — every emitted tag traces to a schema entry or registered hand implementation, with group families and value forms, so output-side completeness is checkable per tag, not per corpus.

**The invariant tying them together:** for every construct in the dump census, exactly one of — *modeled and oracle-verified*, *executed through an instrument-gated engine*, or *refused with a counter and a reason* — holds, and CI asserts the sum. Under that invariant, "do we have all tags/enums?" is answered by reading artifacts 2–4 and 6: the refused counters *are* the todo list, the AUTO share *is* the automation level, and a corpus run can only confirm what the ledger already claims — its job is catching engine bugs, not discovering coverage. Sampling becomes the validation instrument; the ledger is the discovery instrument. That is the maintainer's ask, made mechanical.

---

## Appendix: session brief (paste into a new orchestrator session)

Recommended session setup: **Fable 5, high effort**, auto-accept-edits permission mode, `gh` authenticated. Paste the block below verbatim; edit only the `STAGE =` line on later sessions.

```text
Read these two files in full before doing anything else:
1. OVERHAUL_OXIDEX_PLAN.md   (repo root — the operative plan: 33 steps, 6 stages,
                              gates, and binding execution rules)
2. AGENTS.md                 (repo root — repo rules; non-negotiable)

You are the orchestrator for executing this plan. The plan's "Execution rules for
agent sessions" section is binding in full; the load-bearing points:

STAGE = 1
- First, secure a clean base: fetch origin. If this checkout is on a branch other
  than main or has uncommitted/staged changes, do NOT switch branches or commit
  anything here — create a fresh git worktree off latest origin/main and do ALL
  work there (the maintainer's checkout may carry unrelated WIP; never mix it
  into a PR).
- Execute the stage above, step by step. If every one of its exit criteria passes
  and no question is pending for me, roll into the next stage — but STOP at the
  first design checkpoint you reach (Steps 10, 15, 18, 28, and Step 15's decision
  gate): produce the design artifact yourself and wait for my sign-off. Never
  delegate a checkpoint design to an implementation subagent.
- Spawn Sonnet subagents for implementation steps (model override: sonnet).
  Parallelize only steps that touch disjoint files; the Pentax seven-tag fix
  (Step 1) is one file, so one agent. Never two agents in the same file; never
  use git stash in worktrees.
- One step = one branch off latest origin/main = one PR = squash-merge after CI
  green. Quote the step's Verify instrument and its result in every PR body
  ("VALUE 0 under group-qualified `just compare-file` on Pentax.jpg" — never
  bare counts).
- Never invoke bare `exiftool` from PATH — only the tree pinned by
  .exiftool-version, capability-probed; a matching -ver is not a working oracle.
  Never approximate a conversion: if a required semantic (format/count/
  condition/conversion) is unresolved, omit and count — absence is correct
  output. cargo clippy + cargo fmt before commits; `just verify-tables` and the
  jpeg-tag-matrix ratchet must be unchanged-or-better unless the step says
  otherwise.
- If a gate fails: fix it or report the failing output. Never merge red; never
  weaken a gate to pass it.
- Track everything in OVERHAUL_PROGRESS.md (repo root; keep it untracked and out
  of PRs): read it before starting, update it after every step, and at stage end
  record per-criterion pass/fail against the stage's exit criteria.
- Line numbers in the plan are pinned at commit d4456ebc and will have drifted —
  locate code by content with rg. Deeper evidence when a step's rationale needs
  it: ~/git/MERGED_EXIFTOOL_OXIDEX_TAG_REVIEW.md (Part V findings, Appendix B
  claim disposition).

Start now: confirm you have read both files, list this stage's steps with your
planned order and parallelism, then execute without waiting for further input.
```
