# Plan: make ExifTool upgrades drive the tags

Updated 2026-09-13. This is the main plan for deciding what to do next.
The [technical execution record](./UPGRADE-NEXT-STEPS.md) and
[earlier backlog](./AUTOMATION-AND-TESTER-PLAN.md) provide supporting detail.
They do not override the goals or progress rules here.

See the [working scoreboard](AUTOGENERATION-PROGRESS.md) for the current
milestone, measured baseline, completed checks and remaining work.

## The goal and the current phase

A change to ExifTool's tag definitions should flow into OxiDex by regeneration,
without someone retyping the tag name, byte location, camera-selection rule or
conversion in Python or Rust.

Handwritten code will still read files and provide shared operations such as
reading numbers, evaluating expressions and decrypting blocks. Tag-specific
knowledge must come from the pinned ExifTool source. **Moving a hard-coded tag
rule into a generator or a shared helper does not count as automating it.**

The target is tag-specific reading and writing behavior derived from ExifTool's
Perl source across its formats and upstream releases. The current working pin
is 13.59; it does not limit the requested version scope. Every generated build
must conform to the native release it came from. Native read-only tags have no
required write path; native writable tags need a separately verified one. The
4,238-file read corpus is a test population, not a way to exclude unexercised
behavior or untested versions. The [read/write and version execution plan](reference/read-write-version-plan.md)
records the expanded finish line, first writer pilot and periodic release tests.

The current implementation batches primarily migrate reading. Completing those
batches does not establish generated write support. Shared source definitions
must account for both directions now, while reader and writer execution are
migrated and verified separately. Write-side inventory and costing are in
progress; the earlier reading estimate is not a full read/write estimate.

We are finished when all tag-specific rules in that target come
from that source, the required behavior works, the replaced manual rules are
removed, and an upgrade demonstrates this. Unsupported behavior and unmeasured
areas remain unfinished; they cannot disappear from the denominator.

## How it works, in plain English

The inputs are ExifTool's actual Perl tables and executable rules, not comments
or documentation. We load the pinned source's real table structures and
translate supported conditions, conversions and processing behavior into
shared Rust definitions and operations. Unsupported Perl behavior remains
explicitly unfinished; this is not an arbitrary Perl-to-Rust translator.

A sample is not required to generate a supported tag definition. Samples test
execution. Independent checks compare generated definitions with the native
source, constructed files exercise rare rules and boundaries, and real files
test complete parsing. Track generated, independently verified and exercised
in a file separately. Absence from the corpus must not erase a native tag.

## Reading and writing share definitions, with separate execution checks

Capture the tag identity, type, placement, permissions, forward conversions and
available inverse conversions together. Avoid duplicating tag knowledge in a
second generator for writing. Missing inverse behavior is an explicit refusal,
not permission to guess a reversal of the read conversion.

OxiDex already has write paths for JPEG, PNG, PDF and walkable TIFF-based
files, including some RAW containers, in `src/core/operations.rs`. These are
container routes, not evidence that every native-writable tag is supported.
The generated Canon reader migration does not activate generated writing.

The source dump captures facts such as `Writable`, `PrintConvInv`,
`ValueConvInv` and write-processing declarations. Inventory which facts are
retained, translated, consumed by a writer, manual, unsupported or unclassified
before counting progress. Writing needs encoding, insertion/deletion, placement
and offset handling as well as preservation of unrelated metadata and file data.

In parallel with the current reader acceptance, audit the existing writers
and the shared schema. Select a small native-writable family already understood
by the reader, generate its write-specific rules through the same source model,
and retire the duplicate manual rules only after actual write/read-back proof.
Do not create another vendor-specific translator or defer writer needs until
the entire reading migration is finished.

Track native writable rules accounted for, generated write rules, manual write
rules and unsupported/unclassified behavior separately from reading. Test
create/update/delete on disposable copies, read the results with pinned
ExifTool, and verify preservation of unmodified data. A source-change rehearsal
must update write behavior without tag-specific Rust/Python edits. A successful
read census proves none of these write requirements.

The JPEG matrix is a useful starting instrument. Its committed report is dated
August 12; it is not a refreshed measurement of this candidate or all formats.
Native read-only tags are explicitly ineligible for writing, not implementation
gaps. No current generated-writing percentage has been established.

## Version upgrades and periodic tests

Use the same compiler and shared readers/writers to regenerate for each selected
native release. Keep an immutable upstream release catalog, reproducible random
seeds and a per-version result ledger. The existing bump promotion compares both
binaries against the newer oracle; add a separate non-promoting rehearsal that
regenerates both releases and checks each against its own native read/write
behavior. Selection or successful generation alone is not conformance.

Once that runner passes its own tests, exercise a randomly selected distinct
release pair after three relevant merged batches or one week, whichever comes
first. The existing hourly continuation records this cadence without launching
a heavy test on every wake-up. Persist failures, manual interventions,
unsupported/untested releases and unexercised behavior. Replay known failures
alongside new selections. Random samples discover gaps; they cannot certify
all ExifTool versions. No full read/write, all-version completion estimate has
been established.

## Where we are now

| Work | Verified state | What it means |
| --- | --- | --- |
| Shared table compiler and reader | Already exist; some families use them | We have a foundation to extend instead of building a new interpreter for every camera brand. |
| Sony focus-table pilot | Merged in PR #746 at `04eaf6e1`, including removal of 17 duplicate entries. Final CI is green at `a1626cb6`; the 4,238-file pair records ten raw-ID fixes and no other per-file changes. | The shared route now replaces the duplicate Sony producer. Remaining source-inventory work is broader than this pilot. |
| Shared binary strings | Merged in PR #747 at `8f0fdaf4`. It preserves raw bytes in saved state, distinguishes a one-byte default from a remainder string, and carries the bounded CameraInfo repair. | This is a shared capability, not a Canon migration. CameraInfo remains a legacy text-domain adapter, and no Canon manual reader has been removed. The prior full-pair supervisor-status limitation remains recorded. |
| Keyed-directory schema and compiler | Merged in PR #748 at `ebbe1ece`; all final hosted checks passed at `a422e8de`. | Native parent facts and expression declarations are checked. Shared reporting policy merged in #750; the inactive reader merged in #752 at `634e5616`. No production route is active. |
| Shared word-directory processor | Merged in PR #754 at `1138a880`; nine tables, 132 rows, 698 Python tests and all five hosted jobs pass. | Four of five unsupported child processors now have generated descriptors. Canon production routing and manual-reader retirement remain unfinished. |
| Shared serial processor | Foundation merged in #757; #759 at `8887e5d9` expands the eight-table total to 122 emitted alternatives and 10 omissions, with all five hosted checks passing. Canon AFInfo 14/14 and AFInfo2 16/16 are generated and natively replayed. | Real AudioV4 is a validated production caller. Canon's next delivery is complete parent routing and manual AFInfo2 reader retirement; definition readiness alone does not count as that migration. |
| Real AudioV4 retirement | Merged in #758 at `19cb7650`; one manual reader and its 31-slot sequence removed. Full corpus: one Copyright correction, 4,237 other files unchanged. All five hosted checks pass. | Supported source-name changes reach actual output after regeneration with no tag-specific Python/Rust edit. Native occurrence groups, warning output and V3/V5 remain explicit residuals. |
| Recorded source inventory | Merged in PR #749 at `18a8ef17`; all final hosted checks passed at `72e8e664`. The report accounts for 1,512 table identities and retains 119 tables with no named rows. | This establishes the captured source population. Classifying which rules are generated, manual, unsupported or unclassified remains open; source shape is not automation. |
| Sony plain generator recovery | PR #745 merged; six tables and 193 rows reproduced | These tables can be rebuilt. This alone does not prove that their behavior is fully automatic. |
| Sony enciphered recovery | Producer and independent verifier preserved; M4 review found five blockers; not landed | The draft still has a Sony-specific translation layer. Its review remains useful, but it is not the architecture target. |
| Nikon encrypted recovery | Producer committed on a work branch; not landed or independently accepted | It reproduces the meaning of 2,317 existing rows with deterministic ordering. It is recovery work, not removal of the custom runtime. |
| Sony raw-ID correctness repair | Merged with the Sony pilot in PR #746. Local and Linux paired 4,238-file evidence, native-table checks and JPEG matrix stages pass. | Correct rows rose from 468,002 to 468,012 with ten ImageNumber gains and no other per-file residual changes. Earlier failed attempts and the aggregate-only pair remain recorded below. |
| Current autogenerated percentage | Not established for the current candidate | An older percentage must not be presented as today's measurement. The familiar 97.3% measures matching output, not automation. |

The Sony draft explicitly maps 53 raw/value/print expressions; the shared
compiler already recognizes 35 of them. That is evidence of duplicated
translation work. Recognition still needs the correct input types, execution
order and independent native validation.

## What we will do, in order

| Step and goal | How we will achieve it | Evidence required to call it done |
| --- | --- | --- |
| **1. Establish an honest starting point.** | On one recorded commit, inventory native tag rules and the code that supplies them. Reuse the existing table inventory, conversion records and corpus tools. Classify generated rules, manually maintained rules, unsupported rules and unclassified rules separately. | Every rule in the named inventory is accounted for; no unexplained omissions. Publish the counts, source version and commit. Run the normal and generated-routes-disabled corpus measurements on that same source. Label the latter a dependency floor, not an exact origin count. |
| **2. Prove one complete migration.** | Move Sony's 17-entry focus-point table, `Tag202a`, through the shared machinery. Reuse the existing ability to save a value and evaluate conditions on it; extend it for binary tables. Preserve native selection rules and list any remaining parent-routing work. | All 17 entries and their conditions are accounted for. Real-file and synthetic boundary comparisons agree with pinned ExifTool. Change a name, enum, offset and supported new row in a copied native source: regeneration must reflect each change without a new handwritten tag rule. Stale output must fail verification. |
| **3. Remove what the pilot replaces.** | Switch the validated family to the shared path, then remove its duplicate custom declarations and handling. Preserve shared file-reading/decryption mechanisms and explicitly list any remaining tag-specific routing or helper rules. | A merged change identifies the exact manual rules removed, shows the shared path actually executed, and has zero unexplained per-file regressions. Keeping the old path as the real producer does not pass this step. |
| **4. Expand by shared capability.** | Choose the next unsupported behavior that serves several families. Candidates include saved-value effects, suppression rules, lookup conversions and verified handling of encrypted blocks. Generate other families through the same machinery; use one difficult neighboring family to test that the design generalizes. | Each batch lists the native families unlocked, manual rules retired, remaining unsupported rules, actual output gain and validation scope. Add no new vendor-specific copy of an already supported expression. The first additional family must reuse the new capability without adding another interpreter. |
| **5. Prove an upgrade needs less intervention.** | Run the existing isolated upgrade tool against another pinned release. Record every manual edit and its cause. Turn repeated causes into shared capabilities, then rerun. | Supported native changes regenerate with zero tag-specific Python/Rust edits. Unknown semantics fail visibly and preserve working output. Report elapsed time, build time, manual interventions and corpus changes. A failed or unexercised change stays open. |
| **6. Close the full remaining inventory.** | Repeat the capability/migration/retirement cycle until no native rule in scope depends on manual tag knowledge. Broaden fixtures for unexercised formats and keep testing later release changes. | Zero manually maintained tag-specific rules, zero unclassified rules, and zero required unsupported behaviors remain in scope. All required parity and upgrade checks pass. A corpus percentage alone cannot certify this finish line. |

Step 1 measurement and Step 2 implementation can proceed in parallel. We will
finish the pilot before expanding another large vendor-specific generator.
If the pilot exposes a missing shared behavior, implement and test that behavior
instead of adding a table-name exception to make the pilot pass.

## The progress report at each checkpoint and landed batch

| Measure | What we will show | Target |
| --- | --- | --- |
| Manual tag knowledge | Rules before and after; exact rules removed; any new exceptions | Falls to zero. A new exception is visible debt. |
| Shared-path migration | Families actually running through the shared reader; old producers removed | Both execution and retirement are demonstrated. |
| Correct output | Matched, missing, wrong-value and extra rows, plus per-file regressions on the same inputs | Correctness improves or stays intact; no unexplained regression. |
| Generated dependency | Correct rows lost when generated routes are disabled, including the count with file-identity rows separated | Reported as an instrument-specific floor; never confused with overall automation. |
| Upgrade effort | Tag-specific source edits, causes, elapsed time and build time for a named version pair | Zero edits for supported native changes. |
| Unfinished work | Unsupported and unclassified rules; unexercised behavior; blocked gates | Remains visible until resolved and verified. |

A rule is a named upstream tag or table property: a name, layout, selection
condition, conversion or output rule. Use its source identity when counting,
so copying it into both Python and Rust does not create two units of progress.
Keep shared execution mechanisms separate from hard-coded tag knowledge.

The report will say **planned, implemented, validated or merged** for each
item, with a commit and evidence. It will include the next deliverable and any
blocker. Generated line counts, agent activity, table existence and passing
syntax checks are not substitutes for these outcomes.

Commit and push reviewable work-branch checkpoints as they are ready, with
unfinished checks visible. Publishing a checkpoint does not have to wait for
the complete merge gate. Merging still requires the relevant completed checks.

The current directory-validation checkpoint now completes normal regeneration:
31 declared outputs, including the inactive keyed definitions, with every
previous table artifact unchanged. Seven native validation calls have generated
operands and independent reader proof. That baseline had five unsupported
child edges.
The current word-directory checkpoint generates nine tables and all 132 native
rows; independent definition and reader-binding checks pass. Its generated
descriptors clear four of those five child-processing refusals. These are
source-level results. Native/Rust replay, including verbose reporting, and
real generated child dispatch pass at `f613820d`. Official regeneration passes
with zero declared changes at `b9c7f206`. The corrected full Python suite passes
698 tests with zero skips; all five hosted jobs pass on `443c8458`, merged in
PR #754 at `1138a880`. The next small milestones are to implement the remaining
dynamic-length processor and resolve the four omitted parent rows. Each completion must
reduce a named blocker count while preserving native behavior. Canon carrier
activation and removal of the duplicate readers come after those checks;
publishing these definitions alone earns no runtime automation percentage.

## How the machines and agents will work

Terra workers take independent tasks: shared compiler changes, independent
native verification, source-driven change tests, and migration review. Each
owns one checkout and a small deliverable. They run quick checks while writing
code. The coordinator integrates a reviewed batch before expensive builds.

Start with three Terra workers and one coordinator. Assign useful work before
adding workers; agents waiting for the same source or build do not accelerate
delivery.

| Owner | Parallel deliverable | Completion evidence |
| --- | --- | --- |
| Implementation worker | One reusable compiler/reader capability | Published source checkpoint and focused checks; no copied tag-specific rule |
| Native-evidence worker | Independent fixtures and expected results from pinned ExifTool | Recorded source/interpreter, raw outputs, boundary cases and source-change checks |
| Review worker | Review the checkpoint as soon as it is published | Concrete findings or a scoped acceptance record, with unproved behavior explicit |
| Coordinator | Integrate accepted work, regenerate, build, compare and publish | One combined validation record and a ready PR; retirement counts after merge |

After an author publishes a checkpoint, that slot can prepare the next
independent test contract while review and integration finish. Keep at most
one implementation batch awaiting expensive validation. Do not accumulate
several unbuilt branches that change the same shared interface.

Use the local Mac for coordination, the M4 for independent CLI workers and
queued build/test work, and the i7 for Linux and pinned-native validation.
Every heavy i7 job uses the shared lock. Reuse compatible caches and avoid a
full build per agent edit. Required final gates still run.

Remote allocation is a plan until host reachability, current ownership and
queue state are checked. CLI workers can write code and run focused tests
without starting full builds. One coordinator owns each compatible build
cache; Linux and macOS artifacts cannot be shared interchangeably.

Measure speed over the next three integrated batches: time from assignment
to published checkpoint, checkpoint to review, queue wait, build/test time,
rework and merge time. Also report manual rules removed and source-driven
behavior proved. Use those measurements to decide whether another worker
helps; lines of code and agent count are not throughput targets.

Current recovery work is preserved rather than deleted. We have stopped trying
to reproduce arbitrary historic table numbering and stopped promoting the
unfinished Sony recovery pipeline as the next milestone. Existing useful
correctness fixes continue through their current gates.

## The next checkpoint

The **shared binary string capability** is merged in PR #747 at `8f0fdaf4`.
Keep its first full-pair regression, bounded repair proof, and the
full-pair supervisor-status limitation as historical evidence. The Sony pilot
is already merged; its failed gate attempts and corrections remain below.

The keyed-directory schema/compiler and shared reporting policy are merged in
PRs #748 and #750. The separate reader merged in #752 at `634e5616`, including
reviewed fixes for legacy parent continuation and directory-state restoration.
It has no production caller. Directory validation merged in #753 and translates seven
native validation calls into common size comparisons and independently verifies
their numeric-reader source and byte-order state. Actual native replay clears
all seven validation-proof blockers; that milestone left five child edges with
unsupported processing rules. A reader-only source mutation makes all seven checks refuse
again, and the verifier rejects the stale artifact.

Canonical artifacts and ledgers were regenerated successfully with local Perl
5.38.2. The four word-processor edges are resolved at the definition level in
#754. The remaining work is ordered as follows:

1. Compile sequential fields and lengths that depend on earlier raw values from
   the native serial processor. The [native probe](reference/serial-processor-checkpoint.md)
   is merged in #755. The reviewed JSON inventory captures all eight selected
   tables and preserves every refusal. The combined Rust emitter and reader
   merged in #757 after native Real AudioV3/V4 replay and all final hosted
   checks. The V4 carrier passes bounded/native and supported source-change
   execution proof and merged in #758 after full corpus and hosted acceptance.
   Complete the [two autofocus definitions](reference/serial-afinfo-plan.md)
   next, then prove generic parent routing and validators before Canon activation.
   Account for every condition and conversion before reducing the final
   unsupported-child count from one to zero.
2. Resolve the four omitted parent rows using shared byte handling. An explicit
   one-byte `undef` field differs from an unformatted inline payload containing
   all eight bytes. Raw image data also needs the native absolute-span hash
   behavior. Completion means 61 represented parent rows and zero omissions,
   with independent native checks; the serial-child blocker is separate.
3. Prove real CRW and JPEG carrier behavior, enable the verified generated
   route, and remove the duplicate manual readers. Report those removals and
   the per-file comparison separately from generated definitions.

Use Real AudioV4 as the first additional retirement test for the same serial
capability: replace its manual 31-entry sequence only after native/Rust output,
groups, real fixtures, source-change propagation and corpus comparisons pass.
Real AudioV3 supplies a second native-table control; adding its support is new
coverage, not removal of existing manual code. Track this separately from the
Canon parent omissions so neither task hides the other's unfinished work.

Neither camera names nor parent tag IDs belong in the new shared execution
mechanisms. No remote host availability is assumed: refresh reachability and
the shared lock before any future i7 job. No i7 work was needed for #754's
canonical regeneration or the serial-probe checkpoint.

The source inventory merged in PR #749 preserves every captured table identity,
including unclassified shapes. PR #751 merged at `35487962` and joins all 1,512 identities to an
immutable generated-artifact snapshot. Extend that join to runtime producers
and manual rules. Keep unknown classifications
visible instead of treating artifact presence as completed generation.

The next migration uses the common string capability in both standalone Canon
raw files and Canon metadata embedded in JPEG. It has three distinct outcomes:

1. Validate common string behavior against native source, including a non-Canon
   table, and preserve existing output across the complete corpus.
2. Generate the parent directory's tag selection, child-table target and saved
   state rules. A shared reader must follow those rules in file order and use
   the carrier's byte order. A copied native parent-ID or target change must
   change both readers after regeneration.
3. Replace and remove the two manual Make/Model decoders and their numeric
   parent dispatches. Report any remaining Canon-specific conditions and other
   child producers separately; this narrow retirement does not finish Canon.

Each outcome gets its own implemented, validated and merged status. The source
inventory currently identifies eight explicit remainder-string fields in eight
tables. Seven Canon validation calls and canonical artifacts are verified and
merged; production routing remains unfinished. These are
implementation milestones, not current output gains.

We do not yet have an evidence-based date for 100%. Record implementation,
review and build effort for the pilot and the next family before forecasting
the remaining work. Keep unusual native programs visible when estimating;
they can be much harder than copying a new tag or enum entry.
