# Generated Runtime Release-Functional Completion Design

Status: approved design direction, 2026-09-19

## 1. Goal

Complete the functional work in `TODO_RELEASE_BETA.md` section 2 so the
`refactor/tag-machinery` release candidate has one source-derived metadata
pipeline, explicit ownership for every retained hand-written path, safe removal
of replaced compatibility code, and a reproducible upgrade path between two
ExifTool releases.

The work will be executed by a fast, concurrent fleet. The main Codex session
is the controller and integrator. Subagents implement isolated tasks in named
worktrees, produce reviewed commits, and maintain recoverable progress records.
The controller integrates tasks as they finish whenever their file leases and
dependencies permit.

## 2. Success criteria

The functional program is complete when all of the following are true:

1. Every enabled metadata field has one machine-checkable owner:
   `generated`, `walker-owned`, `residual`, `refused`, or `not-applicable`.
2. Duplicate ownership and unowned enabled fields fail verification.
3. Stored/source values, typed `ValueConv` values, and displayed `PrintConv`
   values retain distinct meanings throughout parsing and output.
4. Normal CLI output and `-n` output are projections of the same occurrence,
   not independently reconstructed strings.
5. One long-lived `Session` carries required ExifTool state across all
   directories in an input file.
6. Generated conversion dispatch is not hard-coded to `Exif::Main`.
7. IFD, binary, keyed, and serial acquisition paths feed one conversion and
   occurrence pipeline.
8. A decoded field reaches generated conversion exactly once and at most one
   named residual handler.
9. Replaced manual code is deleted only after generated-on/generated-off
   attribution and occurrence-aware oracle parity prove ownership transfer.
10. Same-pin regeneration, forward upgrade, and reverse downgrade complete
    without hand-editing code, fixtures, or generated artifacts between stages.
11. The fixed rehearsal pair, ExifTool 11.78 and 12.64, passes native read and
    write expectations in both directions.
12. The frozen release candidate passes the full functional evidence suite
    with receipts naming the exact source SHA, binary, oracle, runtime, corpus,
    command, and measurement floors.
13. Every implementation task is preserved as local signed checkpoint commits
    and a remote draft PR, then lands through reviewed, green, squash-merged CI
    into the controller-owned `staging/beta1-functional-integration` branch.
    One final whole-branch PR lands that branch into `refactor/tag-machinery`
    only through a verified strict up-to-date or merge-queue guard.
14. No release worktree, target, source cache, corpus, toolchain, receipt, log,
    ledger, or recovery artifact depends on an ephemeral temporary directory.

## 3. Non-goals

- Generating every ExifTool Perl callback before the beta.
- Deleting container traversal merely because its tag declarations are
  generated.
- Treating generated declarations, accepted source rows, observed reads,
  writes, or conformance as interchangeable coverage metrics.
- Approximating unsupported ExifTool conversions.
- Editing `main` or `refactor/tag-machinery` directly.
- Publishing a release, tagging, merging to `main`, or rewriting shared
  history as part of this functional program. Reviewed task PRs into the
  controller-owned integration branch and its final PR into
  `refactor/tag-machinery` are part of the program.
- Running several agents against one worktree or one Cargo target directory.

## 4. Binding operating constraints

### 4.1 Oracle and evidence

- `.exiftool-version` is the source of truth for the active release.
- ExifTool 13.59 is invoked from
  `/Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool` with Perl 5.38.2 at
  `/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2`.
  A bare `exiftool` invocation invalidates the result.
- Every oracle run must pass both the version probe and
  `OOXML.docx -> FileType: DOCX` capability probe.
- Values are derived from the pinned ExifTool source or refused explicitly;
  they are never approximated.
- Measurements state their instrument and preserve machine-readable receipts.
- Worktrees, targets, source caches, corpora, toolchains, logs, receipts,
  ledgers, and recovery state live only under `/Users/allen/git` or
  `/Users/allen/oxidex-ops`; ephemeral temporary-directory storage is refused.
- Shared build/test jobs use the shared lock. Corpus, read-regression,
  transition, and other heavyweight evidence jobs use the exclusive lock.

### 4.2 Worktree isolation

- One implementation task owns one branch, one worktree, and one absolute
  `CARGO_TARGET_DIR`.
- The integration worktree is:
  `/Users/allen/git/oxidex-beta1-functional-integration` on branch
  `staging/beta1-functional-integration`.
- Task worktrees use `/Users/allen/git/oxidex-beta1-<task-slug>` and branches
  `staging/beta1/<task-slug>`.
- Task target directories use
  `/Users/allen/git/oxidex-beta1-targets/<task-slug>`; the controller ledger
  binds that stable path to the task's resolved base SHA and refuses reuse by
  another live task.
- The implementation plan assigns every task a literal slug and all three
  literal paths. Agents do not invent or reuse names.
- Every task first passes `tools/preflight.sh`, fetches the controller-owned
  remote integration ref, and verifies its frozen base SHA explicitly. No
  preflight failure is masked. The known `origin/main` divergence is recorded
  separately for later reconciliation, not treated as the task base.

### 4.3 Fleet capacity and speed

- The current Codex Desktop environment has four active-agent slots: the main
  controller and at most three subagents.
- Noninteractive Codex CLI sessions are external workers and do not consume
  those three Desktop subagent slots. They share the same account quota,
  service limits, machine resources, repository locks, and integration rules.
- The controller does not occupy a subagent slot and remains available to
  integrate completed work, maintain ledgers, prepare briefs, and dispatch the
  next independent task.
- All agents use fast mode.
- When two models are both adequate, the cheaper and faster model is selected.
- At most three Desktop implementation or review subagents are live
  concurrently.
- The initial Codex CLI pool is capped at six live workers, for nine concurrent
  workers plus the controller. The controller may lower this cap immediately
  when quota, service throttling, memory pressure, or lock contention appears.
  Raising it requires a clean capacity receipt and an implementation-plan
  amendment so recovery state remains accurate.
- At most two shared-lock Cargo jobs run concurrently on this laptop.
- Only one exclusive evidence job or central regeneration runs at a time.
- Idle worker slots are filled from the next dependency-ready, non-overlapping
  task. A queued heavy test does not prevent an agent from returning its code
  state and freeing the slot when the plan permits validation to be completed
  by the controller.

### 4.4 Codex CLI worker protocol

The controller creates the named worktree, target directory, and a
self-contained task PRD. Its durable `launch` command starts this argv with
`subprocess.Popen(start_new_session=True)`, a unique process token and PRD path
in the prompt argv, and append-only JSONL/final-message files:

```bash
codex --yolo exec --enable fast_mode --model gpt-5.6-terra --json \
  -o /absolute/durable/process/final.md -C /absolute/task/worktree \
  "Process token TOKEN. Execute the canonical PRD at /absolute/path/to/task-prd.md"
```

`--yolo` is user-authorized so CLI workers do not stop for edit approvals. The
PRD supplies the task goal, file lease, tests, progress updates, commit/report
contract, and prohibitions on worker remote pushes and destructive actions.
Only the controller performs authenticated pushes and PR operations. The plan
uses the literal model and PRD path for each task.

CLI workers are first-class task workers: they receive the same task review,
fix-loop, handoff, and integration requirements as Desktop subagents. Their
terminal output is not the progress record; commits, reports, receipts, and
`HANDOFF.md` are.

The detached worker survives controller-shell death. `monitor`, `status`, and
`heartbeat` track it by PID, process start time, executable, and the
exact token-bearing argv plus `thread.started` session ID. If the worker dies,
`resume` first reconciles the
worktree and remote state, then runs `codex --yolo exec resume ... SESSION_ID
-` with a durable recovery prompt; it never uses `--last`. A quota or
rate-limit response stops new dispatches.

## 5. Current architectural facts

The plan must begin from the existing implementation rather than restart the
earlier design:

- `src/exiftool_tables/conv/mod.rs::decoder()` and `claims()` dispatch only
  `Exif::Main`.
- The `Exif::Main` conversion ledger currently records 551 generated fields,
  17 refused fields, and 29 non-conversion fields.
- `src/exiftool_tables/ifd_engine.rs` constructs a new `Session` for a
  directory rather than sharing one file-scoped session.
- `TagOccurrence` already contains raw, value, print, and stored channels.
  Their producer and consumer semantics are inconsistent; Step 2a is a
  contract and migration task, not a field-addition task.
- Generated IFD rows can place displayed data into the primary value channel,
  while CLI resolution still reads the raw channel in important paths.
- The general, IFD, keyed, and serial engines duplicate conversion-stage
  responsibilities.
- `src/core/exiftool_compat.rs` contains post-hoc formatting that can mask an
  incomplete generated conversion.
- No validated repository-wide generated-on/generated-off attribution harness
  currently proves which outputs depend on generated execution.
- The upgrade transaction can skip fresh BEFORE generation when the old
  release equals the committed pin. That behavior does not satisfy same-pin
  reproducibility.
- Existing version-rehearsal, artifact-manifest, ConvInv, read-regression, and
  write/readback infrastructure must be extended rather than replaced.

## 6. Runtime ownership model

### 6.1 Source-derived ownership

Generated code owns representable source facts:

- tag identity and name;
- type, count, byte layout, and groups;
- conditions and variants;
- `RawConv`, `ValueConv`, and `PrintConv` expressions;
- enum and bitmask maps; and
- source-derived subdirectory descriptions.

Hand-written walkers own byte acquisition, container traversal, encryption,
offset resolution, and callbacks whose behavior is not expressible in the
generated schema. Residual handlers own only source constructs that have an
explicit refusal record.

Every owner record contains:

- ExifTool release and source hash;
- module, table, and stable field identity;
- owner category;
- generated expression or hand symbol;
- refusal reason when applicable;
- oracle fixture or observed carrier; and
- replacement/deletion status.

Stable identity cannot depend solely on numeric IFD tag IDs because keyed and
serial formats use non-IFD identities.

### 6.2 Conversion results

Generated execution returns one of three outcomes:

- `Report`: commit staged session effects and emit one typed occurrence.
- `Suppress`: commit source-required effects but emit no occurrence and do not
  invoke a fallback.
- `Decline`: do not emit an occurrence and transfer control to one named
  residual owner.

The implementation plan must define whether each session effect is staged or
immediate. A declining generated arm must not cause a residual handler to
repeat an effect. `Suppress` must never be interpreted as `Decline`.

### 6.3 Occurrence channels

One canonical occurrence carries:

- the decoded source/stored representation;
- the typed result after `ValueConv`;
- an optional displayed result after `PrintConv`;
- group and source identity;
- occurrence ordering and duplicate identity;
- suppression/undefined/binary state; and
- units when the source assigns them.

Normal CLI and JSON output prefer the displayed result when present. Numeric
mode uses the typed/non-printed result. Libraries and writers receive explicit
channels and never infer a typed value by parsing display text. A composite
may stringify and reparse only when the pinned ExifTool implementation itself
does so.

### 6.4 Session lifetime

There is one `Session` per input file. Directory entry and exit use explicit
scope operations so temporary directory state is restored without discarding
file-wide state.

File-wide state includes values, data members, options, requested tags,
make/model, file and TIFF types, byte order where applicable, warnings, and
processed/cycle guards. Generated conditions and conversions observe the same
evaluation order as the pinned ExifTool source.

### 6.5 Shared pipeline

IFD, binary-data, keyed, serial, and specialized walkers retain their decoding
adapters but call one shared stage sequence:

1. resolve source identity and owner;
2. evaluate condition/variant;
3. decode stored representation;
4. apply `RawConv` and data-member effects;
5. apply typed `ValueConv`;
6. apply optional `PrintConv`;
7. resolve Report/Suppress/Decline;
8. insert one occurrence with groups and ordering preserved.

Structural tags such as pointer, MakerNote, IPTC, GeoTIFF, and PrintIM edges
remain walker-owned when they control traversal.

## 7. Work decomposition by code ownership

Parallelism is decided from declared file leases, not from task names.

### 7.1 First independent wave

These tasks may start concurrently because their primary code leases are
disjoint:

| Task slug | Responsibility | Primary lease |
|---|---|---|
| `ownership-inventory` | Ownership ledger schema, enabled-field inventory, and duplicate-owner verifier | New ownership tool/fixtures/tests under `tools/exiftool-tables/`; no engine or conversion-generator files |
| `typed-occurrence` | Canonical occurrence semantics and output consumers | `src/core/tag_occurrence.rs`, sink/metadata code, CLI/output, FFI, composites, writer-facing APIs |
| `conv-registry` | Generated registry and per-table conversion ledgers | `conv_codegen.py`, its oracle/tests, and `src/exiftool_tables/conv/` |
| `upgrade-transaction` | Fresh BEFORE/AFTER generation, per-release facts, transition recovery | Upgrade-transaction and version-rehearsal scripts/tests only |
| `conformance-receipts` | Oracle occurrence totals and durable receipt completeness | `conformance.py` and its focused tests only |

Only three run at once in the current app. The controller dispatches the next
ready task as soon as one slot becomes free.

### 7.2 Serialized runtime spine

The following work is serialized because it shares central runtime files and
interfaces:

1. file-scoped `Session` and scope operations;
2. shared Report/Suppress/Decline execution contract;
3. the runtime switch and receipt capture needed by the
   generated-on/generated-off attribution harness;
4. IFD0/IFD1/ExifIFD/InteropIFD exact-once ownership;
5. closure or explicit residual ownership of the 17 current refusals; and
6. shared conversion stages behind general, IFD, keyed, and serial adapters.

The implementation plan may split these into several tasks, but only one task
may lease the shared engine/session boundary at a time.

### 7.3 Pilot and fan-out

Olympus is the first end-to-end pilot after the runtime spine lands. It proves
typed values, session propagation, generated dispatch, occurrence behavior,
attribution, and deletion receipts before broad fan-out.

After the pilot, vendor and container tasks with disjoint parser directories
and tests run concurrently. The initial task partition is:

- Nikon;
- Pentax;
- Panasonic;
- DJI;
- Kodak, Casio, HP, and Ricoh;
- Samsung, MediaJukebox, and Vivo;
- JPEG and QuickTime;
- RIFF and audio containers;
- PDF, OLE, ZIP, and remaining containers; and
- remaining TIFF/MakerNote adapters.

If a vendor task discovers a missing shared helper or interface change, it
records the dependency and stops editing that portion. The controller creates
a serialized spine task, integrates it, rebases affected vendor worktrees, and
then resumes them.

### 7.4 Deletion and transition qualification

Central compatibility deletion begins after migrated paths have attribution
receipts. Vendor-local deletion may land with its vendor task when it does not
touch shared compatibility or engine files.

Transition qualification begins early for tooling changes but its final
same-pin, forward, and reverse runs occur only after the converged runtime and
deletion work have been integrated.

## 8. File-overlap and integration protocol

Each task brief declares:

- exact branch, worktree, target directory, and base SHA;
- allowed file globs;
- prohibited shared files;
- interfaces consumed and produced;
- focused tests and evidence requirements; and
- dependency task commits.

An agent may inspect any repository file but edits only its lease. When an
undeclared shared edit is needed, the agent records it in `HANDOFF.md` and
returns `NEEDS_CONTEXT` or `BLOCKED`; it does not expand its own lease.

When a task returns, the controller compares:

```bash
git diff --name-only <task-base>..<task-head>
git diff --name-only <task-base>..<integration-head>
```

If the path sets are disjoint and the task's declared interfaces remain
compatible, review and integration may proceed immediately. If they overlap,
the task is queued, rebased onto the integration head in its own worktree, its
covering tests are rerun, and its review package is regenerated.

Task branches may contain several signed checkpoint commits. After the first
meaningful clean checkpoint, the controller pushes the branch and opens a
draft PR against `staging/beta1-functional-integration`. It pushes later checkpoints and
updates the PR evidence summary so local and remote recovery state advance
together.

After task review is clean, the controller rebases the task onto the current
remote target when required, reruns affected gates, marks the PR ready, waits
for required CI, and squash-merges it. The controller fetches and fast-forwards
its integration worktree to the resulting remote target before releasing
dependent tasks. The task worktree remains available until the remote merge
SHA and post-merge target SHA are recorded.

Before final qualification, the controller rebases its single-writer
integration branch onto the exact current `refactor/tag-machinery` SHA. Task 20
authenticates that combined tree. If the target moves before the final merge,
the integration branch is rebased again and every Task 20 gate, receipt,
documentation update, and whole-branch review is rerun; strict protection or a
merge queue then closes the last target-movement race.

Generated outputs, central registries, artifact manifests, workspace manifests,
lockfiles, `.exiftool-version`, and the integration ledger have one owner at a
time. Parallel agents may produce local generated diffs as evidence but do not
commit central generated files unless their task explicitly owns regeneration.

## 9. Durable progress and recovery

The fleet must be recoverable after agent quota exhaustion, process loss,
context compaction, or controller restart.

### 9.1 Per-task state

Every task worktree has a root `HANDOFF.md`. The worker updates it:

1. after preflight and reproduction/baseline capture;
2. after the first failing test or characterization receipt;
3. before and after a long build or evidence run;
4. after each meaningful implementation checkpoint;
5. after every test result;
6. when blocked or when it needs a shared-interface change;
7. after every commit; and
8. immediately before returning to the controller.

An actively working agent writes a durable update at least every 15 minutes if
no listed transition occurs. The file records:

- task ID, branch, worktree, target directory, base SHA, and current HEAD;
- owned and prohibited paths;
- completed work and remaining steps;
- exact commands and receipt paths;
- test results with pass/fail counts;
- blockers and requested controller rulings; and
- the exact next command.

### 9.2 Controller state

The integration worktree contains:

- root `HANDOFF.md` for the human-readable current state;
- the Superpowers workspace ledger for machine-oriented task/review/fix-loop
  state; and
- `TODO_RELEASE_BETA.md` for release-level milestones and durable evidence.

The authoritative controller snapshot, append-only event stream, canonical
PRDs, process records, reports, reviews, and receipt index live below
`/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller/`.
The ignored Superpowers workspace and root `HANDOFF.md` are working views, not
the only recovery copy.

The controller updates its state after every dispatch, worker message, worker
completion, review verdict, fix round, local checkpoint, remote push, PR/CI
transition, squash merge, gate result, blocker, and model escalation. During a
long wait it records live agents and the exact next integration action before
waiting.

The controller ledger maps every task to:

- model and fast-mode selection;
- worker kind (`desktop` or `cli`) and agent/process identity;
- worktree/branch/target paths;
- base and task-head commits;
- current state: queued, running, review, fix, pushed, CI, merged, or blocked;
- review package and report paths;
- local checkpoint and remote merge commits;
- evidence receipts;
- remote branch, PR number/URL, latest pushed SHA, CI state, merge SHA, and
  post-merge `origin/staging/beta1-functional-integration` SHA; and
- dependencies released by integration.

For CLI workers the ledger also records PID plus process start time, Codex
session/thread identifier, launch count, PRD hash, stdout/stderr/JSONL paths,
heartbeat, exit status, and exact resume command. Recovery verifies process
identity, reconciles worktree and remote state, and never treats PID reuse or a
missing agent panel entry as proof that work must be dispatched again.

Local commits and durable receipts are authoritative for detailed recovery.
The pushed task branch, draft PR, PR comments/checks, and merged target SHA are
the remote recovery layer. An agent panel is never treated as proof of
progress, and a local-only task is not considered safely checkpointed.

## 10. Model-selection policy

Every dispatch names the model explicitly and requests fast mode.

| Work | Default model | Escalation |
|---|---|---|
| Literal fixtures, manifest fragments, one-file mechanical tests | `gpt-5.6-luna` | `gpt-5.6-terra` only after demonstrated reasoning need |
| Isolated vendor ports, routine tooling, small reviews | `gpt-5.6-terra` | `gpt-5.6-sol` for multi-file/source-semantic difficulty |
| Shared runtime, Session, typed occurrence contract, generator registry, upgrade transaction | `gpt-5.6-sol` | `gpt-6-astra` for architectural conflict or failed fix rounds |
| High-risk deletion review, architectural adjudication, final whole-branch review | `gpt-6-astra` | no higher model; stop only when every safe path is a guess |

When the controller is uncertain between two adjacent models, it chooses the
cheaper model. Escalation changes the model, not fast mode. Reviewer floor is
Terra. Luna does not approve shared runtime, deletion, or upgrade changes.

The controller prefers Desktop slots for central runtime work and interactive
fix loops. CLI workers take well-specified parallel implementation, fixture,
tooling, vendor, and review tasks. This is a scheduling preference rather than
a correctness distinction; either worker type must satisfy the same brief.

## 11. Review and correction

Each implementation task receives a fresh review after the worker commits:

1. The worker writes a task report and self-review.
2. The controller creates a complete base-to-head review package.
3. A fresh reviewer checks specification compliance and code quality.
4. Important, critical, specification, or confirmed unverifiable findings
   return to the original worker for up to three fix rounds.
5. Rounds four and five use a fresh worker on the next capable model tier.
6. Each correction receives a scoped re-review.
7. Minor findings remain in the controller ledger for final review.
8. Five failed rounds require an explicit controller ruling; a load-bearing
   unresolved defect stops dependent integration.

Reviews run concurrently with unrelated implementation tasks when a slot is
available. A task is not merged merely because its tests pass.

## 12. Evidence gates

### 12.1 Per-task gates

Each task runs the narrowest tests that prove its behavior, plus formatting and
lint applicable to changed Rust or Python code. Generator tasks prove a second
regeneration is clean. Runtime tasks include paired generated and residual
controls. Deletion tasks require attribution receipts before removal.

### 12.2 Per-wave gates

After every integration cohort:

- regenerate centrally when generated inputs changed;
- prove the next regeneration is clean;
- run affected unit and integration suites;
- run focused occurrence-aware oracle comparisons;
- run the read-regression gate when observable reads changed; and
- conduct an integration review over the cohort diff.

### 12.3 Final candidate gates

After writers stop and the candidate SHA is frozen:

- full generator/source verification;
- full clean second regeneration;
- unit, integration, doc, feature-matrix, ignored, and release-mode tests;
- typed normal/`-n` oracle matrix;
- generated-on/generated-off attribution;
- occurrence-aware combined-corpus conformance with oracle occurrence total;
- zero newly lost proven reads;
- per-release public write/readback matrices;
- same-pin regeneration;
- 11.78 to 12.64 transition;
- 12.64 to 11.78 transition;
- current-pin to intended-next-pin rehearsal when available;
- main-divergence remeasurement and classification; and
- final Astra whole-branch review followed by at most one consolidated fix
  wave and one scoped re-review.

## 13. Stop conditions

An affected task or measurement stops when:

- preflight, source identity, version, Perl identity, or capability probe fails;
- any release path resolves beneath an ephemeral temporary-directory root;
- the worktree is protected, dirty before work, stale against its declared
  base, or shares its target directory;
- a bare or incorrect ExifTool is invoked;
- the task edits outside its file lease;
- two live tasks edit the same leased path;
- generated output is hand-edited or differs after a second regeneration;
- a lock is absent or lost;
- measurement floors or provenance are incomplete;
- a value is approximated instead of derived or refused;
- a newly lost proven read appears;
- occurrence order, groups, request behavior, suppression, or duplicate
  semantics regress;
- an upgrade stage needs a manual code or fixture intervention;
- an important review finding remains unresolved; or
- a worker attempts a push; work requires shared-history rewriting,
  publication, a destructive action, merge to `main`, or another external side
  effect beyond the controller's authorized task-branch PR workflow.

## 14. Implementation-plan requirements

The implementation plan derived from this specification must:

- be one plan containing the complete dependency graph;
- assign a literal task slug, branch, worktree, target directory, file lease,
  worker kind, model, fast-mode instruction, tests, receipt paths, and
  dependencies to every task;
- identify the initial three Desktop tasks, initial CLI pool, and the dispatch
  queue behind them;
- include a self-contained task PRD and literal `codex exec` launch/relaunch
  command for every CLI-assigned task;
- include actual test names, commands, expected failing conditions, minimal
  implementation steps, and commit messages;
- designate the controller-owned regeneration and integration tasks;
- specify when a returned commit may proceed directly to final PR review and
  when it must be rebased;
- include the per-task `HANDOFF.md` contract in every worker brief;
- provide controller ledger entries for dispatch, review, integration, and
  recovery;
- define an executable path-policy fence, atomic snapshot/event protocol,
  canonical PRD materialization, CLI/Desktop process tracking, and a tested
  total-process-loss recovery path;
- specify local checkpoint, remote draft-PR, CI, squash-merge, and post-merge
  synchronization steps for every task; and
- end with the frozen-candidate evidence sequence and release-TODO update.

The plan is executed with Superpowers subagent-driven development plus parallel
dispatch only for tasks whose file leases and dependencies are disjoint.
