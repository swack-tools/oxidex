# Generated reading, writing and ExifTool upgrades

Updated September 14, 2026. This records the expanded read/write and version
objective alongside the [main plan](../AUTOGENERATION-PLAN.md).

## Current status

The upgrade requirement is newer-native behavior: parsing fixes, new tags,
changed types and formatting must come from the selected release. The complete
automatic upgrade path is not finished.

| Work | Evidence now | Remaining acceptance |
| --- | --- | --- |
| Source facts and shared writer helpers | PRs #761–#766 merged; #766 is `82f97ae6`, with all five required checks passing at `d505d179` | Integrate and validate the next complete writer operation |
| Internal generated TIFF/JPEG scalar writes | All nine final recipes passed 432/432 native file comparisons under `generated_tiff_write_matrix.py` at the September 14 frozen source checkpoint | Public operations, new EXIF blocks and additional native-writable rule classes |
| Full-library final writer compilation | Official 44-artifact regeneration passed; all 153 decoded reader modules identical to control; Python, Rust workspace and Clippy passed | Expand unsupported writer semantics without changing reader behavior |
| Public generated writing and manual lookup removal | Nine scalar identities use generated descriptors; three manual descriptors and one reverse-name exception removed in `58c0bdcf`. Existing, fresh and mixed public gates passed in the named run below | Repair native mandatory-default batch instrument, complete expanded regeneration, review and merge |
| Release upgrades | Random pair 11.78/12.64 selected and sources verified | Generate/build both versions and test each against its own native reader and writer |

The first September 14 regeneration revealed that creating the native writer
context before read capture installed runtime properties in EXIF tables and
changed reader admission. Initialization now occurs after read capture. The
constructor-mutation regression verifies this separation, and the completed
regeneration preserved all 153 reader modules exactly.

Validation completed on the frozen development tree based on `b6b7f8c1`:
`regen-all.sh` regenerated 44 artifacts; the full Python run executed 1,004
tests with 14 skipped because its Perl environment used a relative filename.
A replay with the absolute selected Perl executable passed all 14 missing tests
and repeated one standalone Rust test. Thus 1,004 distinct Python tests were
verified across the two runs; the original run itself was not skip-free.
`cargo test --workspace --all-features`, the 432-case native TIFF/JPEG matrix,
and workspace Clippy with warnings denied all passed. The matrix covers nine
emitted scalar identities, eight operations, two name spellings, and three
carriers. These are internal entry points, not public API write conformance.

The full regeneration attempt also exposed a valid empty QuickTime hash key
that the row compiler incorrectly rejected. The corrected row compiler emits
191 data rows from the full capture and records unsupported rows explicitly.
That count does not measure completed writers or generated-output share.

The writer compiler currently recognizes a bounded function grammar. It adopts
supported data changes such as a native tag's writable type; arbitrary changes
to Perl control flow still require extending shared compiler support. Refusing
an unknown rule prevents a stale implementation from silently winning, but is
not completion of automatic adoption. The upgrade ledger must retain that gap.

Later sections preserve earlier checkpoints; this table is the current status.

## Current integration checkpoint, September 14

PR #768 is merged as `4498ae67`. Its complete native writer capture,
reader regeneration, build/test, release, lint and docs checks passed at the
reviewed head. The final review-thread audit found no unresolved threads.

The development branch now registers 55 generated artifacts (33 tier 1 and
22 tier 2), including raw JFIF properties captured from executable native
writer dispatch and table definitions. This does not mean 55 tag families or
55 percent coverage. Full real regeneration of the expanded inventory remains
outstanding; the last full local regeneration covered 44 artifacts.

Fresh JPEG creation now consumes raw JFIF properties only before the creation
boundary selected by the native writer. Native fixtures cover multiple APP0
segments, intervening APP2 segments, existing empty EXIF, zero values and
partial JFIF records. The public API performs the generated and legacy edits
in memory before one atomic file commit.

At commit `58c0bdcf`, the three remaining manual scalar descriptors
(DocumentName, PageName and TargetPrinter) and the handwritten TargetPrinter
reverse-name exception were removed. The nine migrated scalar identities now
obtain writable descriptors from joined generated address, final-writer and
public-migration facts. Existing YAML definitions cannot override these facts.
Removal or unsupported source changes remain terminal for migrated identities;
they must not reactivate stale manual behavior.

The named `manual-retirement-gate-r2` completed 4,861 library tests (four explicit
instrument drivers ignored), 432 existing-file public cases, 135 mixed batches,
270 fresh/empty JPEG cases, 216 JFIF timing cases, 49 instrument tests and
workspace all-feature Clippy. The 135 mixed cases include intentional atomic
refusals, so they are not all successful native writes. The combined gate was
not green: the fresh mandatory-default batch instrument stopped while checking
a native operand before comparing generated output. Its repair and rerun remain
required before this migration is ready to merge.

The next measurable completion is the repaired mandatory-default batch proof,
then official full regeneration of all 55 registered artifacts with an explicit
reader projection comparison. Broader writer rules and the persisted
11.78/12.64 read/write upgrade rehearsal remain open. No new overall
generated-output percentage is claimed by this work.

Global generated-source validation failure is now terminal for the retained
migration identities as well. `descriptor-terminal-gate-r2` passed all 4,862
library tests (four ignored) and workspace all-feature Clippy after integrating
the failure-path fix. Unrelated legacy identities remain available.

The sections below are historical checkpoints, not current open-work status.
Their gate results retain their original scope; the table and integration
checkpoint above supersede their pending-work statements.

## Address-to-file checkpoint, September 14

The generated address resolver now joins a complete captured row identity to
its generated scalar writer. Dispatch takes source hashes only from the selected
generated capture and verifies every row identity field against that capture
before the TIFF/JPEG writer checks the final recipe. The native
`generated_tiff_write_matrix.py --route resolved-address` run passed **432/432**
cases: nine emitted identities, eight operations, two group spellings and three
carriers. This covers existing EXIF blocks through internal entry points; it
still does not prove public API admission or new JPEG EXIF creation.

The same source checkpoint passed 193 Rust writer tests (two explicitly ignored
instrument drivers). Combined manifest, regeneration-shell and ownership
controls passed 31 Python tests. Workspace Clippy with all features and warnings
denied passed. The broader all-targets lint attempt failed on pre-existing
test-only unused-import and duplicate-module findings; that attempt is not green.
The official manifest now contains 49 artifacts,
including generated address operands, their report/history and source-derived
mandatory defaults. A full real regeneration of all 49 is still required; the
previous complete regeneration covered 44 artifacts.

The integrated public-migration ledger passed 11 tests with selected native
Perl/ExifTool, including a copied-source rename and a mismatched final Writer
source refusal. The public transaction planner and official artifact registration are now
implemented. Registration totals 51 artifacts (29 tier 1, 22 tier 2), with
27 manifest and regeneration-shell tests passing. The planner joins the
migration ledger to current final controls, partitions generated and legacy
edits, retains aliases, and refuses removed or unsupported migrated rules.
The writer test instrument passed 200 tests (two explicit fixture drivers
ignored); workspace/all-features Clippy with warnings denied passed. File
execution is not yet connected, so these are planning checks, not public
write conformance. Full real regeneration of all 51 remains outstanding.

Public migration must use the generated intersection of complete address and
final-writer identities, with retained history for previously migrated entries.
The broader 561-name source inventory is not the public migration set. Tags
never migrated retain their existing route; migrated tags whose newer native
rules are removed or unsupported must not silently use old manual rules.

Mandatory default encoding and an internal minimal-IFD0 carrier are integrated
but inactive. Eleven focused tests pass, including per-field JFIF presence and
source-binding refusals. Review found that the relevant numeric WriteValue
executable grammar still needs compilation; a source hash and nonempty body
are not proof of automatic behavior adoption. New JPEG EXIF creation stays
unfinished until this is repaired and tested through native file operations.

At this historical checkpoint, PR #768 was open at `7ff727aa` (it subsequently merged as recorded above). Earlier table-check attempts ended in
runner shutdown during the tier-2 dump. The explicit reader-only tier-2
capture now passes regeneration and drift checks in 40 seconds; its separate
cache leaves normal writer-complete capture intact. A later verification
step failed in hosted job `103929195894` and is being diagnosed. All review threads must be addressed and resolved, alongside passing
required checks on the exact head, before merge.

The persisted 11.78/12.64 rehearsal has not passed. Its first 11.78 dump succeeded,
but the following expression verifier rejected the deliberately changed version
pin as a dirty tree. The adapter correction allows that change only during the
sanctioned generation stage. The failed journal is retained; both-version
build/read/write results remain outstanding. Do not select a different pair to
avoid this failure.



## Public API file checkpoint, September 14

The development branch now routes the nine composed scalar identities through
public `modify_tag` and `remove_tag` operations. Generated changes are masked
from the legacy planner; both phases work in memory, and only the successful
complete transaction reaches the atomic file commit.

`generated_tiff_write_matrix.py --route public-api` passed **432/432** native
comparisons against selected ExifTool 13.59: nine identities, eight operations,
two spellings, and little-endian TIFF, big-endian TIFF and JPEG carriers with
existing EXIF. This is new public API evidence, distinct from the earlier
internal dispatch run. The exact source build passed 200 writer tests, then the
complete library test executable passed **4,852 tests, four ignored**.
Workspace/all-features Clippy with warnings denied passed.

This development checkpoint is not ready to merge: creating fresh or empty
JPEG EXIF currently refuses while mandatory numeric and byte-order executable
source checks are repaired. These refusals can reject writes supported by the
old route, so they must be closed before public migration lands. The 432-case
matrix does not cover fresh/empty EXIF or mixed generated/legacy batches;
whole-map mixed-batch native tests and atomic failure controls remain required.
Manual rule removal is still pending. Unsupported or removed migrated source
identities must remain explicit refusals instead of silently reverting to old
handwritten semantics.

PR #768 is separate from this public wiring. At that checkpoint its head `8b8be088` kept
the full module-complete writer capture and moves table verification to the
existing larger runner after SIGTERM on the previous runner. That hosted
run was then pending; the termination cause has not been established.

## Integrated source contracts after the public checkpoint

Numeric mandatory packing now admits a complete, finite executable grammar
for count-one u16/u32 integer inputs. Its validation and packing dependencies,
byte-order maps and native TIFF format registry must join the selected source.
Copied-native statement and helper mutations must refuse when unsupported;
boundary values are compared with actual native and rendered Rust results.
The integrated compiler/capture suite passed **53 tests without skips**, and
workspace/all-features Clippy passed after regenerating the mandatory operand.

Fresh JPEG byte-order generation now checks the complete recognized caller
body, its reachable byte-order helpers and their common source identity. A
supported native fallback change can change the generated order; unsupported
caller/helper executable changes refuse. These are finite recognized-body
contracts, not general Perl translation. Byte-order, manifest and selected
regeneration-shell tests passed **14 tests without skips**; workspace Clippy
passed. The merged development inventory is **53 artifacts: 31 tier 1 and
22 tier 2**. A real full regeneration of this expanded inventory is outstanding.

These helpers still await fresh-JPEG activation. Raw JFIF inputs must come from
the actual native binary table and writer path, including undefined versus zero,
not display values or newly handwritten offsets. The next parallel work items
are source-derived raw JFIF operands, fresh/empty-JPEG native fixtures and mixed
public whole-map transaction fixtures. Public creation and manual-rule removal
remain explicit acceptance requirements before landing the migration.

## Finish line

OxiDex should derive all tag-specific reading and native-writable tag behavior
from the selected ExifTool Perl source. Shared Rust mechanisms still perform
file access, arithmetic, encoding and safe file changes. A tag-specific rule
retyped into Python or Rust is unfinished automation.

The version used to build generated definitions is the version used to check
those definitions and their behavior. ExifTool 13.59 is the current working pin,
not a limit on the requested version scope. Keep an explicit upstream release
catalog and per-version results. Missing source, unsupported semantics,
unexercised behaviors and failed versions remain visible. Native read-only
fields are explicitly ineligible for writes, not failed writer implementations.

## Newer native behavior takes precedence

An upgrade adopts the newer native release's parsing corrections, new tags,
type and formatting changes, and writable behavior. Do not preserve an old
result merely because it once matched an older oracle. Test three relationships:

- Old generated OxiDex against the old native ExifTool.
- New generated OxiDex against the new native ExifTool.
- Native old versus native new, to identify intentional upstream changes.

Cross-version equality is not a passing requirement. The new build must match
the new oracle. If the generator cannot represent a changed rule, record that
unsupported behavior explicitly and extend the shared machinery. Do not hide it
by retaining the old tag-specific implementation. A sampled pair passing does
not certify other releases or behaviors.

For every upgrade, produce a change ledger separating upstream parsing fixes,
added or removed tags, changed types/layouts, changed conversions/formatting,
and changed write rules from OxiDex mismatches. Preserve old-version fixtures
and expectations under their old release; add new-version expectations from
actual new-native output. An expected output change is accepted only after the
new generated build reproduces it. Never copy old expected values into the new
release merely to keep tests green.

Report changed upstream behaviors reproduced, remaining mismatches,
unsupported changed rules, untested changes and required manual tag-specific
edits. The upgrade goal is zero mismatches in the declared tested scope and
zero manual tag-specific edits, with unsupported and untested scope stated
separately. New tags without samples may still receive generated rules, but
must remain marked unexercised until tested. Selecting the new release updates
the pin, generated artifacts and matching oracle together after validation.

## Work in order, with independent tasks in parallel

1. **Done:** Canon AFInfo2/AFInfo3 reader retirement merged in PR #760 at
   `4a3eb26c`, with all five required hosted checks passing on `2630ded8`.
   Preserve the failed legacy fixture, native correction and acceptance record.
2. Capture one complete source model for both directions. Retain permissions,
   actual writer/checker functions, placement, forward and inverse conversions,
   validation, insertion/deletion and ordering rules. Keep unknown property
   values and distinguish absent, undefined, zero and empty. Preserve existing
   read admission while expanding write facts.
3. Prove the first complete generated writer using HostComputer `0x013c` and the
   generic Exif::Main scalar rule class. Generate its actual physical IFD0
   placement and write type; use the existing JPEG/TIFF surgical mechanisms.
   Verify insert, update, growth, shrinkage and deletion against native ExifTool
   in both byte orders, preserving unrelated metadata and image/file payload.
   Include default UTF-8 text and embedded NUL through a generic exact-value
   input, and keep defined-empty values distinct from deletion. Both EXIF family names
   and physical IFD names must address the same generated identity; the
   pre-migration EXIF-qualified deletion silently succeeds without deleting,
   while TIFF deletion is explicitly unsupported. Preserve these as baseline
   failures until the complete generated operation is implemented.
   A copied native name/type/placement change must propagate without another
   hand-written rule. Retire the replaced manual lookup after proof.
4. Expand shared capabilities and migrate eligible read/write families through
   them. Publish manual rules removed and remaining unsupported behavior for
   each direction; table/line counts alone are not completion.
5. Add a non-promoting version-rehearsal runner around existing generation and
   isolated build mechanisms. The existing promotion bump compares both builds
   against the new oracle, so it is not proof of each version's native behavior.
   Regenerate/build BOTH selected versions and compare each with ITS native
   reader/writer, using the same declared fixtures and corpus where applicable.
6. Exercise upgrades repeatedly, then close the remaining historical-version
   and semantic inventory. Random tests are a discovery tool; they cannot
   certify untested versions or behaviors.

Earlier writer checkpoint: PRs #761–#765 merged the source-fact foundation,
readiness tooling, inactive scalar helper compiler/runtime, CheckExif
composition and generated input sanitization. The latest merge is #765 at
`e2df687b`, with all five required hosted checks passing at `56d3cbe5`.
Step 3 remains incomplete: inverse conversion controls, charset/count handling,
physical operation integration and manual lookup retirement still need complete
native file proof. See the [input pipeline](writer-input-pipeline.md) for that
order. These helper merges do not count as a complete generated writer or as a
successful full upgrade rehearsal.

## Reproducible random version rehearsals

Snapshot the actual official release tags with immutable commit/archive
identities. Pick two distinct releases from that catalog with a persisted seed,
record selection before work starts, then test older-to-newer regeneration.
Do not silently restrict selection to versions already known to pass. Missing
native prerequisites or unsupported old syntax must be recorded as failures or
explicit unsupported scope, not skipped out of the success denominator.

Run after every three relevant merged generator/reader/writer batches or one
week since the last rehearsal, whichever comes first, once the runner exists
and passes its own tests.
Keep a persisted counter and last run identity so interruption cannot reset the
cadence or accidentally launch duplicate runs. The existing hourly continuation now records this cadence; it must finish
and validate the runner before launching expensive rehearsals. Do not create
a duplicate schedule or run a rehearsal on every heartbeat.
All heavy work uses its host queue/lock and survives disconnects.

For each version record native source/interpreter/capability identity, generated
artifact hashes, clean code revision, binary hash, fixture hashes, all commands
and exits, and raw native/OxiDex outputs. Compare create/update/delete plus
unmodified-data preservation separately from reads. Record every manual edit
needed to make the upgrade work. Successful automation requires zero
tag-specific edits; unsupported semantics require shared implementation work.

Use prior failures as fixed regression cases alongside new random selections.
Report the complete catalog population, tested versions/pairs, failures,
untested releases and unexercised read/write behaviors. Keep corpus attribution,
read conformance, write conformance and source-rule automation as separate
measurements. No exact completion date follows from the current partial data.


## Implementation checkpoint after the reader merge

The native write-fact sidecar and offline seeded planner are integrated on
`codex/read-write-upgrade-integration-20260913`. Independent review accepted the
complete loader-token grammar after rejecting two earlier bypasses. The full
153-module read projection is unchanged. Planner repair checks exact catalog
selection, matching-version oracle bindings, selected journal membership and
untested scope; 17 focused tests and five independent altered-plan checks pass.
The inactive writer compiler is integrated through `10c89560`. On the recorded
native dump it emits one table with 22 string candidates; those rows have no
production write route. Captured identities, procedure provenance and effective
native groups are preserved. Independent mutations reject malformed identities
and change the candidate when supported source facts change. Procedure hashes
are evidence of origin, not proof that the procedure is translated.

The official release capture is integrated through `4c584dd8`. The saved capture
contains four pages and 388 numeric release tags with resolved commit identities.
Review found skipped-page and duplicate-next-link acceptance; both repairs and
failed probes are retained. Selection and immutable source identity are now
implemented in this branch. A separately reviewed materializer at `05729f1b`
has downloaded, extracted and verified both selected source archives. The first
persisted random pair is **11.78 to 12.64**, selected once from the complete
388-entry catalog. Both report the expected version under canonical Perl 5.38.2.
This proves source identity only. Both-version regeneration/builds, native
capability checks, old-to-new comparison and real read/write comparisons remain
unfinished. The materializer is not integrated in this foundation branch.

The inactive raw TIFF editing primitive is integrated through `059f56d1`.
It accepts resolved directory/tag/type/value operations and deletion without a
manual tag-name lookup. Sixteen focused tests and full Clippy pass; independent
review accepted directory-graph and size-bound fixes. Complete-carrier proof,
source helper translation and public generated identity routing remain separate
requirements. No production writing behavior is changed by this checkpoint.

The first combined official regeneration stopped at its workspace guard because
an untracked handoff changed during the run. Tier-1 oracle checks completed;
tier 2 and the full Python suite did not run. Preserve that failed attempt, and
freeze all workspace files, including handoff notes, for the retry. The recorded
expression ledger changed only its native dump identity after write-fact capture.
The frozen retry at `16432502` passed on September 13 at 17:12:18 UTC:
all 32 artifacts regenerated with zero changes (236.627 seconds), the native
processor oracle passed, all 831 Python tests passed with zero skips
(528.143 seconds), and `cargo test --workspace --all-features` passed 6,022
tests with 124 ignored (136.323 seconds). Formatting, full Clippy and diff
checks also passed. The 124 ignored tests are not counted as exercised.
Evidence relative to the continuation evidence root is
`shared-pilot/write-upgrade-integration-20260913/retry-16432502/validation-state.json`
and its stage logs. This is local foundation validation; hosted review and
landing remain pending. No successful release upgrade is claimed by a saved
plan, a verified archive, or the 388-entry catalog.

Independent foundation review then found two source-identity gaps. Repair
`66f28379` retains each requested dependency binding alongside its final
callable provenance, including anonymous callables, and rejects release pairs
that resolve to the same source commit during selection and verification.
All 57 affected Python tests pass. An independent binding-only mutation changes
the rendered candidate, and that rendered Rust compiles. The saved 11.78/12.64
plan still verifies without reselection. No runtime Rust or canonical generated
artifact changed after the full gate above. PR #761 subsequently passed all five required hosted checks at `675a736e`
and squash-merged as `bd71e392` at 01:14:17 UTC on September 14
(20:14:17 CDT on September 13).

## Native readiness checkpoint, September 13

The rehearsal CLI can verify a selected materialized release, its explicit
Perl interpreter and required native capability, then read and set/delete
metadata on private fixture copies. Reports bind commands and results to the
verified archive/source tree, disable ambient user configuration, and publish
without replacing an existing report,
including a concurrent publisher. File-move/link pseudo-tags require a separate
containment contract; they are not metadata cases in this readiness instrument.

The portable command-line tests create their own archived source fixtures and
controlled executable stand-in. They exercise read/set/delete success, wrong
version, changed plan/source, missing fixtures, filesystem-action refusal and
report publication races without private paths or conditional skips. These
tests establish the instrument's behavior, not ExifTool compatibility.

The persisted random pair, 11.78 and 12.64, has also passed actual native CLI
readiness for JPEG FileType and Comment set/delete, including UTF-8 input.
This does not establish OxiDex/native conformance. Generating and building
OxiDex for both versions, comparing both readers and writers with their own
native release, and accounting for unsupported behavior remain the next
rehearsal steps. A ready native oracle must never mark those steps passed.

## Writer row integration checkpoint, September 14

Unmerged work on `codex/writer-row-integration-20260914` now generates static
ConvInv inputs from native GetTagInfo in explicit write context. Conditional
selection is a named omission. The first Exif capture contains 191 data rows;
that is not a writable-tag count. Root integrated the worker with official
row-output registration and preserved the earlier helper tests. Six focused
suites passed 52 tests without skips; full regeneration and gates for this
branch remain required.

The internal final scalar stage consumes a complete parsed WriteExif token
template and native row operands. Review rejected two earlier implementations
that accepted inserted output-changing assignments. Four independent insertion
probes now refuse; 12 native/standalone-Rust tests passed in the integration
checkout. The oracle writes an actual TIFF through native WriteExif and reads
its entry fields. Final-stage registry emission, effective table-binding checks,
physical route composition and manual-rule retirement remain unfinished.
These are internal component proofs, not complete generated file writing.

The following integration update adds effective native Table-pointer identity:
a row cannot borrow its containing table's CHECK_PROC when native GetTagInfo
redirects it elsewhere. Generated final-stage output now also contains the
canonical native format registry and aliases; its Rust proof consumes those
constants and freshly generated scalar helpers from the same source document.
Both row and final-stage outputs are registered (44 artifacts total). The
registration's 13 native/standalone final-stage tests, 19 manifest tests and
five shell controls pass; four additional registry/freshness tests pass.
Full official regeneration and full gates for this unmerged branch remain due.

The next acceptance milestone is complete operations: join generated inputs and
final recipes by full source identity, normalize and validate values, then feed
resolved type/count/value edits into the existing TIFF carrier. Compare actual
TIFF/JPEG insert/update/grow/shrink/delete/empty/UTF-8/NUL operations with native
ExifTool, including unrelated metadata and image preservation. Pass that matrix
before public routing and manual lookup retirement; internal helper tests do
not satisfy it. Then repeat against the saved random releases using each
release's own generated artifacts and native oracle.

## September 14: carrier composition under validation

The 40-artifact helper checkpoint passed official regeneration, all 957 Python tests (zero skips), all-feature workspace Rust tests and CI Clippy. It is pushed as ready PR #766 at `045b739d`; hosted checks and merge remain pending.

The next local branch joins generated source identities through normalization, conversion and final encoding into raw TIFF edits. Defined false conversion errors quietly preserve the file, following native SetNewValue. The new actual-file instrument declares 32 TIFF operations across two byte orders, two qualified names and eight scalar operations. The named `generated_tiff_write_matrix_v1` instrument matched all 32 against actual ExifTool 13.59 files using canonical Perl 5.38.2; type/count/value bytes, unrelated tags and image payload preservation passed. Focused composition tests and CI Clippy also passed. This was an explicitly dirty development checkpoint, using bounded generated inputs; full official 44-artifact regeneration remains required before landing. Public routing, JPEG composition, manual lookup retirement and full version rehearsals remain unfinished.

## September 14: JPEG file proof and full-capture correction

The internal path now handles existing JPEG EXIF blocks through the same
generated scalar rules and shared byte-preserving carrier. The actual-file
comparison covers 48 operations across TIFF little/big endian and JPEG, two
qualified names and eight scalar actions. The first JPEG score was 32/48 because
the instrument compared ExifIFD physical offsets; native legitimately relocated
the directory. The corrected instrument follows and compares directory targets,
rejects cycles and changed target data, and checks all non-EXIF JPEG bytes.
Regrading the saved outputs with the original binary hash and 570 unchanged
runtime-source hashes verified 48/48. This proves the internal default-option
HostComputer class against native 13.59, not public writes or general version
compatibility. JPEG unit tests and CI Clippy pass.

The full 44-artifact regeneration exposed a separate compiler bug: a valid empty
Perl hash key in QuickTime::eeBox was treated as malformed. The correction keeps
the exact key and records its unsupported writer rule as an omission. Eight
row-compiler tests and a complete saved-native-dump row compilation pass. Full
regeneration and whole-project gates must now rerun before this work lands.
Public caller admission, creating JPEG EXIF blocks from source-defined defaults,
manual lookup retirement and complete version rehearsals remain unfinished.
