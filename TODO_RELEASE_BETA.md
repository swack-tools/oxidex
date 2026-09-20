# TODO: Promote v2.0.0-beta.1 to `main`

This is the living release ledger for OxiDex `v2.0.0-beta.1`. Add notes,
evidence links, blocking defects, decisions, and exact SHAs here as the work
progresses.

The goal is to make `refactor/tag-machinery` and all supporting documentation,
tests, measurements, and delivery automation ready for a real integration into
`main`. The goal is **not** to rename `refactor/tag-machinery`, replace `main`
with an unreviewed snapshot, or tag a commit that is not reachable from
`origin/main`.

## Release contract

The release is complete only when all of these statements are true:

- [ ] The reconciled code has been merged into `main` through a reviewed PR.
- [ ] Every retained `main` change and every refactor change has an explicit
      disposition; no side of a conflict was accepted wholesale without review.
- [ ] The exact resulting `main` SHA passes all required tests, generated-file
      verification, parity gates, documentation checks, and release builds.
- [ ] Version strings, the changelog, release notes, installation instructions,
      benchmarks, parity claims, and known limitations describe that exact SHA.
- [ ] The macOS ARM64 artifact builds, is signed with the Developer ID identity,
      and the DMG is accepted, stapled, and validated by Apple notarization.
- [ ] The release workflow creates a GitHub **pre-release**, does not move the
      stable `Latest` release, and uploads every expected artifact.
- [ ] The Docker workflow publishes only the exact beta tags from a commit
      reachable from `main`; it does not move `latest` or other stable tags.
- [ ] Signed tag `v2.0.0-beta.1` points to the verified release commit on
      `main` and is not moved or recreated.

Until every box above is complete, `CHANGELOG.md` remains `Unreleased`; the
release notes are a checklist rather than a receipt; the branch/development
installation instructions are pre-tag only; and historical benchmark or parity
material must not be presented as v2.0.0-beta.1 evidence.

## Current snapshot

Refresh this section whenever the candidate changes. Historical green runs are
context, not evidence for a later release SHA.

**2026-09-19 release-ledger refresh (instrument: local Git object inspection,
`git show`/`git show-ref`; remote `git ls-remote origin` did not return within
the 30-second command window).** The controller integration tip is
`114f87cf5b5e5ea10c463e9225b925bca716ea06` on
`staging/beta1-functional-integration`. It is not a promotion branch and has
not been proposed to `main`. Do not treat the cached remote-tracking values
below as a live remote refresh; re-run the named remote probe before beginning
integration. Local `refs/tags/v2.0.0-beta.1` is absent and
`gh release view v2.0.0-beta.1 --repo swack-tools/oxidex` reported `release not
found`; remote tag state remains unverified because the bounded `git ls-remote`
probe did not return.

| Item | Current observation | Release implication |
|---|---|---|
| Controller integration tip | `114f87cf5b5e5ea10c463e9225b925bca716ea06` (`staging/beta1-functional-integration`) | Task integration continues; no promotion PR or `main` candidate exists. |
| PR #857 policy cleanup | Merged as `ca1eb126`; [CI run 35454301888](https://github.com/swack-tools/oxidex/actions/runs/35454301888) required checks passed (Benchmarks skipped) | Historical integration evidence only; see the GitHub observation and durable receipt below. |
| PR #858 Darwin linker flags | Merged as `f24a2130`; [CI run 35454699431](https://github.com/swack-tools/oxidex/actions/runs/35454699431) required checks passed (Benchmarks skipped), independently reviewed | Requested flags are present; release signing/notarization gates remain open. |
| PR #859 release-ledger refresh | Merged as `114f87cf`; [CI run 35456471578](https://github.com/swack-tools/oxidex/actions/runs/35456471578) required checks passed, including generated-table fanout (Benchmarks skipped) | The ledger is refreshed through the current integration tip; no promotion PR or `main` candidate exists. |
| `origin/main` | `4a38afde` | Refresh before integration. |
| `origin/refactor/tag-machinery` | Pre-integration historical baseline `67d58b95` | Not refreshed for `114f87cf`; rerun on the candidate before promotion. |
| Merge base | See the divergence report | The histories genuinely diverged. |
| Divergence | 45 main-only / 520 refactor-only commits | Requires deliberate reconciliation. |
| Merge simulation | 32 conflicted files: 30 content, 1 add/add, 1 modify/delete | A direct merge is not release-ready. |
| Current refactor CI | Pre-integration historical baseline: green at `67d58b95` | Not refreshed for `114f87cf`; rerun on the final reconciled SHA. |
| Current benchmark workflow | Pre-integration historical baseline: green at `67d58b95` | Not refreshed for `114f87cf`; rerun on the candidate before making release claims. |
| Beta tag/release | Neither currently exists | Do not create until the final `main` SHA is frozen. |

Current green-run references:

- CI: <https://github.com/swack-tools/oxidex/actions/runs/35423314373>
- Indicative benchmarks:
  <https://github.com/swack-tools/oxidex/actions/runs/35423314366>

Durable integration evidence:

- PR #857 / `ca1eb126`: [Claude-only policy receipt](/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/claude-only-policy/receipt.md).
- PR #858 / `f24a2130`: [macOS linker final validation](/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/macos-strip-linker/final-validation.txt) and [independent review](/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/macos-strip-linker/independent-review.md).
- PR #859 / `114f87cf`: release-ledger refresh; [CI run 35456471578](https://github.com/swack-tools/oxidex/actions/runs/35456471578) completed the required checks, including generated-table fanout (Benchmarks skipped).
- GitHub observation on 2026-09-19 (instrument: `gh pr view` / `gh pr checks`):
  [PR #857](https://github.com/swack-tools/oxidex/pull/857) merged as
  `ca1eb126` and [PR #858](https://github.com/swack-tools/oxidex/pull/858)
  merged as `f24a2130`; each CI run completed its required checks successfully,
  while its optional Benchmarks check was skipped. These GitHub runs—not the
  local receipts—are the provenance for the CI result.

The earlier temporary `v2.0.0-beta.1` run at `8f05ec44` is **not** release
evidence. The tag was deleted while Actions runners were checking it out, so
all platform jobs failed at checkout before compilation, signing, or
notarization. A future rehearsal must leave its tag or ref available until all
jobs finish.

## 1. Coordination and evidence discipline

- [ ] Refresh `origin/main` and `origin/refactor/tag-machinery`; record both
      full SHAs before starting each release wave.
- [ ] Use one dedicated worktree, branch, and `CARGO_TARGET_DIR` per task.
- [ ] Keep every worktree, target, source cache, corpus, toolchain, log,
      receipt, and recovery file under `/Users/allen/git` or
      `/Users/allen/oxidex-ops`; reject ephemeral temporary-directory paths.
- [ ] Run `tools/preflight.sh` before the first edit and before remote
      operations, then fetch and compare the task's literal base against its
      controller-owned remote target. Record `origin/main` divergence
      separately; never mask a preflight failure.
- [ ] Preserve unrelated dirty files and worktrees; never rewrite the protected
      checkout in place.
- [ ] Use the shared build lock for Cargo builds/tests/clippy and the exclusive
      lock for corpus sweeps and read-regression gates.
- [ ] Use only the pinned ExifTool release and pinned Perl runtime. Never use a
      bare `exiftool` invocation.
- [ ] Require both oracle probes: ExifTool version and
      `OOXML.docx -> FileType: DOCX`.
- [ ] Store durable receipts with the exact instrument, binary path and SHA,
      source SHA/dirty state, oracle versions, corpus path/file count, and
      metric floors.
- [ ] Update `HANDOFF.md` at each meaningful milestone with the exact next
      command and current PR/CI state.
- [ ] Keep the authoritative fleet snapshot, append-only event stream,
      canonical PRDs, process/session records, reports, reviews, and receipt
      index under
      `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/`;
      rehearse recovery after all controller and worker processes terminate.
- [ ] Create signed local checkpoint commits at meaningful milestones. The
      controller—not workers—pushes each task branch and opens or updates its
      draft PR against the controller-owned
      `staging/beta1-functional-integration` branch so recovery exists both
      locally and remotely.
- [ ] Require fresh review and all required CI checks before squash-merging
      each task PR. Fetch and fast-forward the controller mirror to the remote
      merge before releasing dependent tasks.
- [ ] Land the completed integration branch through one final reviewed PR into
      `refactor/tag-machinery`; rebase and rerun the complete frozen-candidate
      qualification if that target moved, then require a live strict
      up-to-date protection or merge-queue rule before merging so target
      movement cannot race the reviewed/tested base.
- [ ] Retain task worktrees and remote branches until the wave's post-merge
      gates pass and the PR, merge SHA, and resulting target SHA are recorded.

Notes:

> **2026-09-19 — controller/oracle remains NO-GO.** Task 0 is running
> Generation 12 corrections; the controller / oracle remains NO-GO pending the
> Generation 12 final receipt and independent review. The current evidence is
> retained under [Generation 12 correction evidence](/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap/gen12-correction/);
> no release waiver exists.
>
> **2026-09-19 — legacy fleet retirement is design-only.** Read-only discovery
> found the legacy runtime inactive on the inspected workstation, but `server`
> and `work2.oxidex.net` were unreachable; their services, hooks, state refs,
> schedules, and credentials are unverified. No retirement action is authorized.
> Keep `tools/release/fleet_controller.py` out of retirement scope: it is the
> new local durable release controller, not legacy fleet tooling. See the
> [discovery report](/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/legacy-fleet-retirement-discovery/report.md)
> and [design](/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/legacy-fleet-retirement-design/report.md).
>
> **2026-09-19 — promotion remains unstarted.** There is no actual promotion
> PR, merged `main` candidate, or release tag. A real signed tag still requires
> separate explicit maintainer authorization for the exact version, tag, and
> frozen `main` commit.
>
> **2026-09-19 — legacy Claude coverage loop quarantined.**
> `.claude/workflows/exiftool-coverage-loop.js` has no tracked executable
> caller and must not be run for beta work: its autonomous main-branch merges,
> shared-checkout/worktree model, ambient-oracle and ephemeral-evidence paths,
> and missing authorization boundaries conflict with the protected-main,
> integration, durable-oracle/evidence, worktree, and release-authorization
> rules. Delete or archive it only after external-consumer verification and
> explicit approval; do not fix it in place. See the [coverage-loop review](/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/claude-policy-audit/coverage-loop-review.md).

> **2026-09-19 — Claude-only boundary and legacy fleet status verified.**
> The final boundary audit records that `CLAUDE.md` is a non-duplicating
> routing pointer to authoritative shared policy in `AGENTS.md`; it is not an
> import surface and does not duplicate or override those rules. Its remaining
> content is Claude-specific skills, model-routing, and fast-mode guidance, and
> the legacy coverage loop is quarantined rather than an active beta workflow.
> The legacy runtime remains
> a retirement candidate only: the 102-file `tools/fleet/**` surface still has
> direct `justfile`, service/unit, hook, and Keel consumers. Keep
> `tools/release/fleet_controller.py` in scope as the new local durable release
> controller, not as legacy fleet code. No destructive retirement, deletion,
> extraction, credential revocation, or supervisor change is authorized until
> a durable zero-consumer proof covers live hosts and external consumers and a
> maintainer gives explicit approval.

## 2. Finish functional and ExifTool-parity work

Implementation design:
[`docs/superpowers/specs/2026-09-19-generated-runtime-release-functional-design.md`](docs/superpowers/specs/2026-09-19-generated-runtime-release-functional-design.md).

Execution plan:
[`docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md`](docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md).

Already landed on `refactor/tag-machinery`:

- [x] DJI float forward-port (`#844`).
- [x] Canon hand-subtable forward-port (`#849`).
- [x] CIFF-in-JPEG and Leica CameraIFD forward-port (`#851`).
- [x] Google HDRP, MakerNote, and GContainer forward-port (`#852`).
- [x] Sony MakerNote forward-port (`#853`).
- [x] Generated Exif::Main mixed-mode and byte-exact runtime work (`#848`,
      `#850`).

Still required:

- [ ] Finish the Olympus port on the generated path. Do not reintroduce the
      displaced hand-owned implementation merely to make a cherry-pick apply.
- [ ] Finish the long-tail and remaining DJI/main ports: Nikon, Pentax,
      Panasonic, Kodak, Casio, HP, Ricoh, Samsung, MediaJukebox, Vivo, and any
      residual composite or XMP dependencies.
- [x] Treat autogeneration Step 2a typed values as a beta blocker for routes
      promoted to the generated runtime; the detailed boundary is recorded in
      section 2.4.
- [ ] Re-run the main-divergence measurement against the latest candidate;
      replace the pre-port counts in
      `docs/reference/main-divergence-2026-09-18.md` with current facts or
      clearly mark the old measurement as historical.
- [ ] Run the occurrence-aware combined-corpus comparison against the pinned
      oracle. Record matched, MISSING, VALUE, EXTRA, RENAME, ceiling, file
      count, and oracle occurrence count.
- [ ] Require zero newly lost proven reads under the read-regression gate.
- [ ] Investigate every remaining case that `main` matches and the release
      candidate does not; classify it as ported, superseded, intentionally
      removed, or unresolved.
- [ ] Keep tag-definition counts, generated-table share, parser-route coverage,
      observed reads, write coverage, and output conformance as separate
      metrics.

### 2.1 Define the functional completion boundary

The beta does not require every ExifTool Perl callback to be generated. It does
require one explicit ownership model with no ambiguous overlap:

- generated declarations own source-derived tag names, IDs, formats, layouts,
  conditions, conversions, and enum maps that the generator can represent;
- hand-written container walkers own format traversal and byte acquisition;
- narrowly named residual handlers own only source constructs that the
  generator explicitly refuses; and
- the shared tag pipeline owns occurrence order, groups, stored values,
  `ValueConv`, `PrintConv`, filtering, and output insertion.

- [ ] Create a machine-readable ownership inventory for every enabled table.
      Each field must be `generated`, `walker-owned`, `residual`, `refused`, or
      `not applicable`; no field may silently fall between paths.
- [ ] Make duplicate ownership a verification error. A generated arm and a
      hand-written compatibility branch must not both claim the same source
      tag unless a documented transition test proves why both are temporarily
      required.
- [ ] Make every refusal carry the source release, module/table/tag identity,
      unsupported source expression or callback, and intended owning layer.
- [ ] Define the beta blocker: all enabled routes must use the shared pipeline,
      and every remaining hand path must be enumerated and tested. Complete
      generation of every ExifTool format may continue after the beta.

Current inventory snapshot at `67d58b95` (textual counts are navigation aids,
not coverage measurements or automatic deletion targets):

| Surface | Current observation | Required end state |
|---|---:|---|
| Generated conversion dispatch | `Exif::Main` only | Registry/codegen dispatches every enabled generated table |
| `Exif::Main` conversion ledger | 551 generated / 17 refused / 29 not-conversion | Every refusal resolved or explicitly residual/walker-owned |
| Generated helpers used by `Exif::Main` | 13 | Source-faithful helper library expands as tables require it |
| `exiftool_compat.rs` `base_name` branches | 51 | Zero unowned post-hoc formatting branches |
| Legacy `.insert(` textual matches under `src/` | 4,078 | Classified and migrated where they bypass the shared value pipeline |
| Occurrence-aware insertion textual matches | 163 | The normal insertion path, with narrow documented adapters |
| Conversion-capable engines | general, IFD, keyed, serial | One conversion pipeline behind walker-specific adapters |

Recalculate these inventories with a checked-in script before using them as a
release metric. A falling line count is not evidence of rising parity.

### 2.2 Generalize generated conversion code beyond `Exif::Main`

- [ ] Replace the hard-coded `Exif::Main` test in
      `src/exiftool_tables/conv/mod.rs::decoder()` with a generated registry or
      equivalent source-derived dispatch over every table with conversion
      arms.
- [ ] Generate one independently reviewable module/section and ledger per
      ExifTool module/table, rather than adding a hand-maintained vendor
      dictionary.
- [ ] Preserve per-field mixed mode: supported fields execute generated arms;
      refused fields decline to their named residual owner without disabling
      the rest of the table.
- [ ] Carry `Condition`, `RawConv`, `ValueConv`, and `PrintConv` as distinct
      stages. Do not collapse them into a display-only conversion.
- [ ] Generate stable source identities that survive unrelated row movement;
      tests and write profiles must not depend on incidental array position or
      a regenerated numeric expression ID.
- [ ] Add verifier coverage for generated registry completeness, stale modules,
      orphan ledgers, duplicate arms, and generated files not listed by the
      release-specific artifact manifest.
- [ ] Expand tables in measured payoff order using current corpus reads and
      refusal/helper frequency, not by vendor preference or raw declaration
      count.
- [ ] For each newly supported expression/helper, compare its behavior with the
      exact Perl source in the pinned ExifTool release. Never implement a
      plausible approximation.

### 2.3 Complete `Session` and source-faithful helper semantics

- [ ] Use one long-lived `Session` per input file. Directory walkers must not
      create isolated state that loses values or `DataMember` effects needed by
      later tables.
- [ ] Thread the session through all enabled walkers and conversion engines,
      including nested subdirectories and MakerNotes.
- [ ] Seed and update the ExifTool-visible state required by generated
      expressions: make/model, byte order, file and TIFF types, directory
      metadata, options, requested tags, values, and processed-state guards.
- [ ] Preserve source evaluation order. Conditions and conversions must see the
      same earlier values and data-member writes that ExifTool sees.
- [ ] Finish exact Perl scalar behavior needed by accepted expressions:
      byte strings, Unicode boundaries, numeric/string coercion, truthiness,
      `undef`, signed zero, list/scalar context, and source-specific formatting.
- [ ] Port helpers in measured refusal/use-count order. Select behavior by the
      pinned source body/hash when ExifTool changed a helper between releases;
      do not silently reuse a 13.59 implementation for an older source tree.
- [ ] Add cycle and recursion protection equivalent to ExifTool's processed
      state without suppressing legitimate duplicate occurrences.
- [ ] Test helper behavior at boundary values and with byte-exact fixtures,
      including malformed input and negative zero.

### 2.4 Finish Step 2a: typed values beside display values

Decision for beta:

> Treat Step 2a as a beta blocker for every route promoted to the generated
> runtime. A partially migrated route may remain only as a named residual with
> an owner and parity test. The public library must not require clients to
> reverse-parse a display string to recover the value.

- [ ] Define the canonical occurrence payload with separate representations
      for source/stored value, typed `ValueConv` result, and optional
      `PrintConv` display value.
- [ ] Preserve multiplicity, ordering, group identity, units, and undefined or
      suppressed states in that payload.
- [ ] Make CLI normal mode project the display value and numeric mode (`-n`)
      project the typed/non-printed value from the same occurrence.
- [ ] Make the Rust library, C ABI, JSON output, composites, and writers consume
      the correct value channel rather than whichever string was inserted
      first.
- [ ] Migrate insertion sites that bypass the occurrence-aware sink. Classify
      the remaining legacy `.insert(` calls as parser-local construction,
      explicit compatibility adapters, or defects.
- [ ] Remove post-hoc display reparsing where the ExifTool source consumes a
      typed `ValueConv` result. Keep and test intentional ExifTool behavior that
      itself stringifies and reparses a value.
- [ ] Add paired normal/`-n` oracle tests for numeric, rational, enum, date/time,
      binary, list, undefined, duplicate, unit-bearing, and signed-zero values.
- [ ] Prove Step 2a does not change occurrence order, group assignment, request
      filtering, write lookup identity, or suppression semantics.

### 2.5 Complete directory wiring and residual ownership

- [ ] Finish mixed-mode ownership for IFD0, IFD1, ExifIFD, and InteropIFD so
      every decoded row is offered to the generated runtime exactly once and a
      decline reaches at most one residual handler.
- [ ] Resolve the 17 current `Exif::Main` refusals one by one. In particular:
  - [ ] port or classify `SetPriorityDir` and `IdentifyRawFile` behavior;
  - [ ] model `SubDirectory`, offset, SubIFD, MakerNote, and DNG private-data
        edges in walker adapters rather than pretending they are conversions;
  - [ ] resolve `Copyright` ownership without IFD1 double insertion;
  - [ ] support or explicitly refuse the unmodelled shift used by
        `LearningOptOutIn`;
  - [ ] implement exact declaration/loop/rational semantics for
        `CompositeImageExposureTimes`;
  - [ ] implement or retain named residuals for `ConvertBinary` and
        `PrintOpcode` used by `OpcodeList1`/`2`/`3`; and
  - [ ] implement exact declaration/loop/`sprintf` behavior for `TimeCodes`.
- [ ] Replace the hand-kept `0x9400 AmbientTemperature` path only after the
      generated typed-value track preserves its negative-zero behavior.
- [ ] Move XP string decoding to the generated `Decode` path, then remove those
      IDs from `IFD0_HAND_KEPT` after byte-exact tests pass.
- [ ] Keep genuine structural tags such as IPTC, GeoTIFF, PrintIM, pointer, and
      MakerNote traversal walker-owned; do not delete traversal merely because
      the tag declaration is generated.
- [ ] Retire the ExifIFD-to-IFD0 yield rule when the generated engine owns the
      row and group semantics directly (the documented E-3 cleanup).
- [ ] Make edge suppression/request behavior request-aware so explicitly
      requested tags are not lost merely because a subdirectory edge is
      normally silent.
- [ ] Fold `ifd1_engine_rows` and equivalent replay/drain buffers into the
      shared directory path once ordering tests prove equivalence.
- [ ] Apply the same model to Canon, FujiFilm, Olympus, Sony, and subsequent
      generated tables; avoid a second vendor-specific pipeline.

### 2.6 Delete replaced code without deleting required walkers

Removal is an output of proven ownership transfer, not a preliminary cleanup.

- [ ] Add a deletion ledger with: old file/symbol, exact source tags or
      behavior, generated/new owner, oracle fixtures, generated-on result,
      generated-off attribution result, and deletion commit.
- [ ] Delete hand-written Exif::Main tag/conversion arms only after their
      generated or named residual owner passes occurrence-aware parity.
- [ ] Delete each `src/core/exiftool_compat.rs` branch once the owning generated
      table emits the same stored, typed, and printed forms. Do not retain a
      formatting fixup that masks a bad generated conversion.
- [ ] Delete duplicate tag-name, enum, lens, and conversion maps once the
      source-derived table demonstrably owns them. Keep product logic and
      format traversal that ExifTool does not express as table data.
- [ ] Consolidate the conversion portions of the general, IFD, keyed, and
      serial engines into the shared pipeline. Retain small decoding adapters
      where their byte layout or traversal is genuinely different.
- [ ] Remove temporary replay, drain, fallback, and double-insert shims after
      the shared pipeline owns ordering and occurrence insertion.
- [ ] Delete stale generated artifacts through the release-specific manifest;
      never use a broad filesystem deletion or assume every release produces
      the same module set.
- [ ] Add CI checks that reject new manual tag knowledge for source constructs
      the generator supports and reject an old residual whose ledger entry is
      now generated.
- [ ] Run generated-on/generated-off attribution tests. A parity result is not
      evidence for generated ownership if the hand path still produces it.
- [ ] Measure deleted manual rules and residuals separately from total source
      lines. Report any code that cannot yet be removed and why.

### 2.7 Wire all walkers into one tag pipeline

- [ ] Inventory every hand-written container walker (`ProcessJPEG`,
      `ProcessMOV`, RIFF, PDF, OLE, ZIP/container formats, and other enabled
      families) with its source callback, supported releases, and shared-pipeline
      entry point.
- [ ] Route walker-produced values through the same condition/conversion/sink
      stages used by generated IFD tables.
- [ ] Enumerate hand-ported ExifTool subs as versioned adapters with fixtures;
      do not describe them as generated coverage.
- [ ] Ensure keyed, serial, binary-data, and encrypted/specialized walkers can
      invoke generated field arms while retaining their required acquisition
      logic.
- [ ] Preserve duplicate occurrences, family groups, source order, requested
      tags, unknown-tag policy, and error/warning behavior across adapters.
- [ ] Add route-level tests proving a detected format is actually parsed and
      does not merely return identity tags.

### 2.8 Make upgrades between two ExifTool versions routine

Already landed as upgrade infrastructure and regression coverage:

- [x] Isolated BEFORE/AFTER upgrade transaction with distinct target
      directories, caller identity checks, conformance floors, and recovery.
- [x] Release-derived generated-artifact inventory, including modules present
      in one release and absent in another (`#846`).
- [x] Garmin/FIT/QuickTime fixes that removed known release-specific generator
      and test assumptions (`#846`).
- [x] Per-release ConvInv profiles and the 1,530/1,530 public write matrix for
      both rehearsal releases (`#842`).

Still required for an actual version-to-version upgrade:

- [ ] Keep `.exiftool-version` as the single release pin consumed by Rust,
      Python, CI, generation, tests, and release documentation.
- [ ] Make the transaction accept explicit old and new source trees, verify
      both version and capability probes, and record both source hashes.
- [ ] Generate the BEFORE state from the committed old pin and the AFTER state
      from the requested new pin in separate clean worktrees/target directories;
      never compare a new generator against stale old artifacts.
- [ ] Derive the full artifact manifest from each release so added, removed,
      split, or renamed modules are applied intentionally and stale outputs are
      removed only from the manifest delta.
- [ ] Make source facts in tests release-aware or regenerated from pinned
      fixtures. Eliminate hard-coded 13.59 expectations that caused the
      historical 113 failures on 11.78 and 79 failures on 12.64 (F3).
- [ ] Gate, version, or retire hand-written behavior that silently preserves
      newer 13.59 tags when running older sources. Rehearsal EXTRA counts from
      the hand layer are compatibility drift, not successful coverage.
- [ ] Generate per-release native write/readback expectations instead of using
      the 13.59-only authenticated contract (F6).
- [ ] Compare reads against each release's own pinned ExifTool oracle and
      writes against that release's own writable surface.
- [ ] Prove both upgrade and downgrade transactions, plus a same-pin
      reproducibility run with no tracked diff.
- [ ] For changed unsupported expressions or helper bodies, fail closed with a
      ledger entry and preserve the caller's old generated artifacts. Never
      reuse an apparently compatible implementation silently.
- [ ] Make promotion apply only the declared pin and generated/fixture manifest
      delta after all gates pass; preserve a durable recovery command and do
      not leave a mixed-version tree on failure.
- [ ] Re-run the fixed 11.78 -> 12.64 rehearsal end to end with zero code or
      fixture interventions between stages, then run 12.64 -> 11.78.
- [ ] Run one current-pin -> next-pin rehearsal before every future release and
      publish its refusal, generated-share, read, write, and manual-intervention
      receipts.

Version-transition acceptance matrix:

| Stage | Old release | New release | Required proof |
|---|---|---|---|
| Source acquisition | | | Version and capability probes; source hashes |
| Same-pin regeneration | | | Byte-identical or explained deterministic diff |
| Forward generation | | | Manifest delta, ledgers, verifier, no manual edit |
| Forward reads | | | Native-oracle conformance and zero lost proven reads |
| Forward writes | | | Per-release writable matrix and native readback |
| Reverse generation | | | Clean downgrade with removed artifacts accounted for |
| Reverse reads/writes | | | Old-release native expectations restored |
| Failure recovery | | | Original pin/artifacts/caller preserved |

### 2.9 Functional acceptance evidence

- [ ] Run unit, integration, doc, feature-matrix, ignored-test, and release-mode
      suites after generated wiring and deletion work.
- [ ] Run table/source verification and prove generated outputs are clean after
      a second regeneration.
- [ ] Run occurrence-aware combined-corpus conformance against the pinned
      oracle and report MISSING, VALUE, EXTRA, RENAME, ceiling, files, and
      occurrence count.
- [ ] Run the exclusive read-regression gate and require zero newly lost proven
      reads.
- [ ] Measure generated declaration count, accepted/refused rows, generated
      dependency of observed reads, walker/residual reads, write/readback
      coverage, and output conformance separately.
- [ ] Investigate every remaining case that `main` matches and the candidate
      does not; classify it as ported, superseded, intentionally removed, or a
      release blocker.
- [ ] Update `docs/AUTOGENERATION-PLAN.md`,
      `docs/AUTOGENERATION-V2-DESIGN.md`, upgrade rehearsal documentation, and
      the ExifTool parity skill to match the implemented pipeline and current
      commands.

Evidence:

| Measurement | Candidate SHA | Instrument/receipt | Result |
|---|---|---|---|
| Combined-corpus conformance | | | |
| `t/images` read-regression gate | | | |
| Generated-table verification | | | |
| Generated-on/off attribution | | | |
| Typed normal/`-n` parity | | | |
| Hand residual/deletion ledger | | | |
| Same-pin reproducibility | | | |
| 11.78 -> 12.64 rehearsal | | | |
| 12.64 -> 11.78 rehearsal | | | |
| Remaining main-only behavior | | | |

## 3. Fix and validate macOS release linking

The intended Darwin-specific Cargo configuration is:

```toml
[target.aarch64-apple-darwin]
rustflags = [
    "-C", "strip=none",
    "-C", "link-arg=-Wl,-S,-x",
]

[target.x86_64-apple-darwin]
rustflags = [
    "-C", "strip=none",
    "-C", "link-arg=-Wl,-S,-x",
]
```

- [x] Commit the `.cargo/config.toml` change from its dedicated worktree
      (PR #858, merged as `f24a2130`).
- [ ] Reproduce the original misaligned `LINKEDIT` string-pool failure on the
      appropriate baseline or retain a durable existing reproduction receipt.
- [ ] Prove the new configuration fixes the failing macOS release build.
- [ ] Inspect the produced Mach-O binary and dylib rather than treating a zero
      Cargo exit code as sufficient proof.
- [ ] Confirm the linker removed debug information and local symbols as
      intended without corrupting exports required by the CLI, C ABI, or
      dynamic library.
- [ ] Prove Linux remains target-isolated: its rustc invocation must still have
      exactly one `-C strip=symbols` and no Darwin link arguments.
- [ ] Validate both configured Darwin targets or explicitly document why only
      ARM64 is shipped and how x86_64 remains tested.
- [ ] Run the release workflow's actual `just build-release` path on macOS.
- [ ] Sign the binary and verify it with `codesign --verify --strict --verbose`.
- [ ] Build, notarize, staple, and validate the DMG.
- [ ] Record artifact sizes and symbol/export inspection results before and
      after the change.

Evidence:

`f24a2130` now configures both Darwin targets with `-C strip=none` and
`-C link-arg=-Wl,-S,-x`. GitHub's [PR #858 CI run](https://github.com/swack-tools/oxidex/actions/runs/35454699431)
completed its required checks successfully (Benchmarks was skipped), and an
independent review was retained. This establishes that the requested linker flags landed; it does
**not** establish Developer ID signing, Apple notarization, stapling, or
downloaded-artifact validation. The retained validation shows an ad-hoc ARM64
signature and an unsigned x86_64 local artifact, not a Developer ID release
artifact. Retain the linked receipts above and keep every remaining checkbox
open until it is proven on the final release candidate.

| Check | SHA | Command/run | Result |
|---|---|---|---|
| Requested Darwin flags / PR review | `f24a2130` | [PR #858 CI run](https://github.com/swack-tools/oxidex/actions/runs/35454699431) and independent review | Required checks successful; Benchmarks skipped; flags present in both Darwin target tables. |
| Developer ID signing | | | Unverified release-finalization gate. |
| Notarization/stapling | | | Unverified release-finalization gate. |
| macOS ARM64 release build | | | |
| macOS x86_64 build/config | | | |
| Mach-O/export inspection | | | |
| Linux isolation | | | |
| Signing | | | |
| Notarization/stapling | | | |

## 4. Version and package audit

- [ ] Confirm the intended release spelling is consistently
      `2.0.0-beta.1`; Python documentation may use `2.0.0b1` only when
      explaining PEP 440.
- [ ] Audit every Cargo workspace package version and every exact inter-crate
      dependency requirement.
- [ ] Confirm `oxidex-tags-shared` intentionally remains `0.1.0`, or change it
      with an explicit compatibility rationale.
- [ ] Regenerate and verify `Cargo.lock` after all manifest changes.
- [ ] Verify `cargo metadata` reports the intended package graph and versions.
- [ ] Verify the built CLI reports the intended version.
- [ ] Search all tracked files for stale `1.x`, beta, branch, installation,
      artifact-name, and release-channel claims; classify every hit.
- [ ] Keep the root crate `publish = false` unless the crates.io ownership and
      package-size blockers are deliberately resolved.
- [ ] Record the crates.io decision: transfer name, alternate package name, or
      no crates.io publication for this beta.
- [ ] Ensure every installation example matches that decision.
- [ ] Decide whether tag crates will be published manually; if yes, rehearse
      the complete dependency order with `cargo publish --dry-run`.

Decision:

> Record the crates.io and package-publication decision here.

## 5. Documentation and factual-release audit

- [ ] Replace `## [2.0.0-beta.1] - Unreleased` in `CHANGELOG.md` with the real
      release date only after the release candidate is frozen.
- [ ] Make the changelog readable as a user-facing summary, not a commit dump.
- [ ] Ensure the migration guide reflects the final API and output behavior.
- [ ] Update documentation that still tells users to switch to
      `refactor/tag-machinery`; after promotion it should point to the signed
      tag or `main` as appropriate.
- [ ] Update `docs/RELEASE-2.0.0-beta.1.md` so its workflow culminates on
      `main`, not a tag on the refactor branch.
- [ ] Remove the statement that Docker will be skipped because the tag is not
      on `main`; the final tag must be reachable from `main`.
- [ ] State known limitations plainly: measured partial ExifTool parity,
      detected-but-not-parsed formats, per-format write support, beta API
      instability, and any intentionally deferred regressions.
- [ ] Verify every numerical parity claim from a machine-readable receipt for
      the final candidate.
- [ ] Re-run committed benchmarks at the final candidate SHA, or prove the
      measured SHA has no relevant source, dependency, configuration, or
      harness differences.
- [ ] Keep noisy shared-runner numbers labelled indicative. Do not copy them
      into stable benchmark claims without appropriate controls.
- [ ] Confirm benchmark commands use the shipped release profile and the pinned
      ExifTool oracle with its capability probe.
- [ ] Build the documentation site exactly as CI/deployment does.
- [ ] Run link, stale-reference, and spelling checks.
- [ ] Verify badges, download URLs, GitHub Release links, artifact names,
      Homebrew status, crates.io status, and Docker instructions against
      reality.
- [ ] Review `README.md`, `AGENTS.md`, and `CLAUDE.md` for contradictory branch,
      release, measurement, or command guidance.

Evidence:

| Documentation check | SHA | Instrument/output | Result |
|---|---|---|---|
| Docs build | | | |
| Link/stale-reference checks | | | |
| Benchmark refresh | | | |
| Changelog review | | | |
| Version-string audit | | | |

## 6. Codify the repeated release workflow as skills

- [ ] Create a project-local release-engineering skill whose terminal state is
      a tested commit merged to `main`, signed tag created from that exact SHA,
      release workflows complete, and published artifacts verified.
- [ ] Include explicit gates for version coherence, test evidence, PR/main
      ancestry, GitHub Release creation, Docker publication, macOS signing, and
      Apple notarization.
- [ ] Create a separate release-documentation skill that requires provenance
      for changelog, benchmark, parity, installation, compatibility, and known-
      limitation claims.
- [ ] Update the `exiftool-parity` skill to forbid bare ExifTool invocation,
      require both oracle probes and corpus floors, use occurrence-aware keys,
      distinguish all coverage/conformance metrics, and emit a stable release
      receipt consumed by the documentation skill.
- [ ] Correct any stale paths, bare `exiftool` examples, obsolete generated-
      table references, and weak acceptance criteria in the current parity
      skill.
- [ ] Decide and document the canonical project-local skill location and how
      `.claude/skills` and `.agents/skills` remain synchronized without silent
      drift.
- [ ] Pressure-test each skill against representative release failures before
      treating it as operational guidance.
- [ ] Update `AGENTS.md` and `CLAUDE.md` routing so agents discover the skills
      at the correct points without weakening protected-branch or pinned-oracle
      rules.

Notes:

> Record the skill layout and validation decisions here.

## 7. Validate CI/CD before promotion

- [ ] Confirm pull requests into `main` run the full CI workflow.
- [ ] Add or verify required `main` status checks. The current active ruleset
      requires a PR, squash merge, and signed commits but does not presently
      report required CI checks through the ruleset API.
- [ ] Decide whether release-profile build, generated tables, and corpus read
      regression must all be required checks for the promotion PR.
- [ ] Ensure CI tests the exact PR merge result, not merely a stale branch head.
- [ ] Ensure a post-merge push run tests the exact resulting `main` SHA.
- [ ] Validate release-workflow YAML and its regression tests against the final
      intended behavior.
- [ ] Add a safe full-pipeline rehearsal mechanism, such as an explicit dry-run
      or release-candidate tag flow, that exercises checkout and all platform
      builds without accidentally publishing the final release.
- [ ] During a tag-based rehearsal, do not delete the tag until every workflow
      has reached a terminal state.
- [ ] Verify all referenced Actions secret names exist. Never print secret
      values.
- [ ] Verify Linux x86_64/ARM64, Windows x86_64, and macOS ARM64 release
      artifacts are produced with the expected names.
- [ ] Verify `create-release` waits for every platform build and publishes only
      after all have succeeded.
- [ ] Verify a SemVer prerelease becomes a GitHub prerelease with
      `make_latest: false`.
- [ ] Verify stable documentation deployment is intentionally skipped for the
      beta, or change that policy deliberately and test it.
- [ ] Verify the Docker ancestry gate accepts the final tag because its SHA is
      reachable from `origin/main`.
- [ ] Verify beta Docker publication creates only exact beta tags and does not
      move `latest`, major, or minor floating tags.
- [ ] Decide whether checksums, SBOMs, provenance attestations, or signatures
      for non-macOS artifacts are release requirements; document any deferral.

Evidence:

| Workflow/gate | Candidate SHA or tag | Run | Result |
|---|---|---|---|
| Promotion PR CI | | | |
| Post-merge `main` CI | | | |
| Release rehearsal | | | |
| macOS signing/notarization | | | |
| Docker rehearsal/gate | | | |

## 8. Curate history without rewriting the shared branch

Do not force-push a rewritten `refactor/tag-machinery`. Existing worktrees,
PRs, evidence receipts, signatures, and documentation refer to its current
SHAs.

- [ ] Freeze the release-ready refactor tip and create a signed archival ref or
      tag before any history curation.
- [ ] Inventory the 520 refactor-side commits and classify them as durable
      milestones, generated updates, parity ports, release work, fixups,
      reverts, experiments, or superseded work.
- [ ] Decide the desired history shape:
  - preserve the full refactor history as the second parent of one release
    integration merge, keeping `main` first-parent history clean; or
  - construct a separate cleaned promotion branch with a reviewed set of
    thematic, signed commits while retaining the archival ref.
- [ ] If using a curated branch, define squash boundaries by behavior and
      provenance, not merely by commit count.
- [ ] Preserve authorship, issue/PR references, generated-source provenance,
      and release-relevant evidence links in curated commit messages.
- [ ] Prove the curated candidate's tree is identical to the validated
      reconciled tree before accepting the cleanup.
- [ ] Re-run all gates after history reconstruction; earlier SHAs and receipts
      do not automatically transfer to rewritten commits.
- [ ] Never combine history cleanup with unreviewed behavior changes.

Decision:

> Record whether the release preserves full ancestry or uses a curated commit
> series, including the archival ref name.

## 9. Reconcile `main` and the release candidate

This is a real integration. It is not a rename, force-update, or exact-tree
replacement of `main`.

- [ ] Refresh and freeze the exact `origin/main` and release-candidate SHAs.
- [ ] Create a dedicated integration worktree and branch from the chosen
      release-candidate history.
- [ ] Merge current `origin/main` into that integration branch normally.
- [ ] Resolve every textual conflict deliberately. The current simulation has
      conflicts in comparison, composite, core helpers, timestamp handling,
      image/audio/raw/QuickTime parsers, MakerNotes, XMP, and tests.
- [ ] Audit automatically merged files as carefully as conflicted files; a
      clean textual merge can still restore obsolete code or duplicate a
      semantic forward-port.
- [ ] Produce a disposition ledger for all 45 main-only commits: retained,
      forward-ported, superseded, intentionally obsolete, or still unresolved.
- [ ] Confirm no stopped fleet tooling, obsolete comparison normalization, or
      displaced hand parser is reintroduced merely because it merged cleanly.
- [ ] Re-run formatting, clippy, workspace tests, release builds, generated
      verification, corpus read regression, occurrence-aware parity, docs, and
      benchmarks on the reconciled result.
- [ ] Open the promotion PR against `main` only after the reconciled branch is
      clean and all evidence refers to its exact SHA.
- [ ] Resolve the current policy mismatch: `main` allows only squash merges. If
      preserving both histories with a merge commit is chosen, deliberately
      update the ruleset for this promotion and restore the normal policy
      afterwards. Do not bypass the rules silently.
- [ ] Wait for every promotion PR check to finish successfully before merging.

Main-only commit disposition:

| Commit/range | Behavior | Disposition | Evidence/notes |
|---|---|---|---|
| `cec6d16a..4a38afde` | 45 commits to classify individually | | |

Conflict-resolution notes:

| Path | Resolution | Why it is correct | Test/evidence |
|---|---|---|---|
| | | | |

## 10. Qualify the resulting `main` SHA

- [ ] Record the merged `main` SHA and freeze it as the only tag candidate.
- [ ] Confirm GitHub reports that commit as signed and verified.
- [ ] Confirm the `main` push CI run completes successfully rather than being
      cancelled by another push.
- [ ] Inspect the `Corpus Read Regression Gate` log for an explicit PASS and
      zero lost proven reads.
- [ ] Confirm the generated-table aggregate and every shard pass.
- [ ] Confirm release-profile builds pass on the merged SHA.
- [ ] Re-run or transfer parity and benchmark receipts only when their source,
      dependencies, configuration, and instrument are proven identical.
- [ ] Confirm the documentation build and factual audit refer to this SHA.
- [ ] Run `OXIDEX_TAG_DRY_RUN=1 just tag 2.0.0-beta.1 <main-sha>` and retain the
      output.
- [ ] Confirm no local or remote `v2.0.0-beta.1` tag or GitHub release exists.
- [ ] Obtain the maintainer's explicit final publish decision.

Final candidate:

| Field | Value |
|---|---|
| `main` SHA | |
| GitHub verification | |
| CI run | |
| Parity receipt | |
| Benchmark receipt | |
| Docs receipt | |
| Tag dry run | |

## 11. Tag, publish, and verify

- [ ] Create signed tag `v2.0.0-beta.1` at the frozen `main` SHA using
      `just tag 2.0.0-beta.1 <main-sha>`.
- [ ] Verify the tag signature locally before pushing.
- [ ] Push the tag with the required SSH identity.
- [ ] Do not delete, move, or recreate the tag while workflows are running.
- [ ] Watch both Release and Docker workflows through terminal completion.
- [ ] Verify the GitHub release is named correctly, marked prerelease, and not
      marked Latest.
- [ ] Download and inspect every published artifact:
  - Linux x86_64 musl binary;
  - Linux ARM64 musl binary;
  - Windows x86_64 executable;
  - signed macOS ARM64 binary;
  - signed, notarized, and stapled macOS DMG.
- [ ] Run basic `--version` and metadata-reading smoke tests on applicable
      artifacts.
- [ ] Verify the downloaded macOS binary's signature and assess the downloaded
      DMG with `codesign`, `spctl`, and `stapler`, not only the build-directory
      originals.
- [ ] Verify exact beta Docker tags resolve and run, and verify stable floating
      tags did not move.
- [ ] Verify stable docs behavior matches the documented prerelease policy.
- [ ] Record final URLs, artifact checksums, workflow runs, and observed smoke
      results below.

Published release evidence:

| Artifact/result | URL or checksum | Verification |
|---|---|---|
| GitHub prerelease | | |
| Linux x86_64 | | |
| Linux ARM64 | | |
| Windows x86_64 | | |
| macOS ARM64 binary | | |
| macOS DMG | | |
| Docker beta tags | | |

## 12. Failure and rollback rules

- [ ] A failed or refused measurement is not a pass; preserve its logs and fix
      the instrument or defect before proceeding.
- [ ] Do not retag a different commit as `v2.0.0-beta.1` after publication.
      Fix post-publication defects in a later prerelease such as beta.2.
- [ ] Do not delete a remote tag or GitHub release without an explicit
      maintainer decision and an impact plan covering users, Docker tags, and
      cached artifacts.
- [ ] If promotion PR validation fails, leave the integration branch and
      worktree intact for diagnosis.
- [ ] If post-merge `main` validation fails, do not tag. Fix forward through a
      reviewed PR unless the maintainer explicitly chooses a revert.
- [ ] If a release workflow partially publishes, inventory every external
      artifact before taking cleanup action; do not assume workflow failure
      means nothing was published.

## Open decisions and notes

> **2026-09-19 — legacy fleet end state (design only).** OxiDex retains the
> local release controller. After live zero-consumer proof and separate explicit
> approval, legacy fleet source and evidence move to one separate read-only
> archived Git repository with a signed tag and checksummed, credential-free
> evidence. Actions, webhooks, deploy keys, runners, writable state refs, and
> service accounts must be disabled. The archive is forensic evidence, not a
> runnable fallback. `server` and `work2.oxidex.net` being unreachable remains
> a hard NO-GO for retirement; this note authorizes no retirement mutation.

| Date | Decision or blocker | Owner | Status/next action |
|---|---|---|---|
| | | | |

## Final sign-off

- [ ] Functional/parity sign-off
- [ ] macOS signing/notarization sign-off
- [ ] Documentation and benchmark sign-off
- [ ] CI/CD and artifact sign-off
- [ ] History and main-reconciliation sign-off
- [ ] Maintainer publish approval
- [ ] `v2.0.0-beta.1` verified on `main`
