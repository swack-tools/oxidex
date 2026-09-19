# Generated Runtime Release-Functional Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> `superpowers:subagent-driven-development` for reviewed tasks and
> `superpowers:dispatching-parallel-agents` only for tasks whose file leases and
> dependencies are disjoint. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the generated metadata runtime, retire only proven-replaced
manual code, and prove repeatable ExifTool version transitions before
`v2.0.0-beta.1` promotion work begins.

**Architecture:** One controller integrates reviewed task commits into a named
integration worktree. Up to three Desktop subagents and six `codex --yolo exec`
workers operate concurrently in named task worktrees when their declared file
leases do not overlap. Contract and engine work is serialized; vendor and
container adapters fan out after those interfaces are frozen.

**Tech Stack:** Rust, Python 3, Perl 5.38.2, ExifTool 13.59 and fixed rehearsal
releases 11.78/12.64, Cargo, `uv`, `just`, Git worktrees, Codex Desktop
subagents, Codex CLI fast mode.

**Spec:**
[`docs/superpowers/specs/2026-09-19-generated-runtime-release-functional-design.md`](../specs/2026-09-19-generated-runtime-release-functional-design.md)

## Global Constraints

- Never edit `main` or `refactor/tag-machinery` directly.
- One task owns one branch, one named worktree, one absolute
  `CARGO_TARGET_DIR`, one file lease, and one root `HANDOFF.md`.
- Use `/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2` and
  `/tmp/oxidex-exiftool-cache/exiftool`; never invoke bare `exiftool`.
- Before any oracle measurement, require `-ver == 13.59` and
  `OOXML.docx -> FileType: DOCX`.
- Never approximate a conversion. Derive it from the selected ExifTool source
  or refuse it with a machine-readable reason.
- Use
  `python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared`
  for Cargo builds, tests, and Clippy. Use the same wrapper without `--shared`
  for corpus, read/write, transition, and timing gates.
- Every agent and CLI worker uses fast mode. When two models are adequate,
  choose the cheaper model.
- CLI launches use `codex --yolo exec`; the task PRD still prohibits remote
  pushes, destructive actions, shared-branch edits, and edits outside its
  lease.
- Workers update their worktree `HANDOFF.md` after every state transition and
  at least every 15 minutes. The controller updates the integration handoff and
  fleet ledger after every dispatch, result, review, fix, merge, gate, or
  blocker.
- Only the controller integrates reviewed commits. Workers never merge or push.
- Generated outputs, central registries, `.exiftool-version`, workspace
  manifests, lockfiles, and shared ledgers have one owner at a time.
- Generated declarations, generated accepted/refused rows, runtime-reachable
  routes, observed reads, writes, and conformance remain separate metrics.

## Review Focus

1. A generated `Decline` after session writes must not cause the residual path
   to apply the writes twice; Task 9 adds the rollback/commit test.
2. Non-UTF-8 and binary values must survive stored, typed, and printed channels;
   Tasks 2 and 7 add byte-exact tests.
3. Duplicate order, groups, requested-tag behavior, suppression, and instance
   identity must survive consumer and engine migration; Tasks 3, 9, and 11 pin
   these behaviors.
4. Same-pin and older-release runs must not reuse stale 13.59 artifacts or hand
   behavior; Tasks 5, 18, and 19 prove fresh native expectations.
5. Generated-on/off attribution must bind raw outputs, binary hashes, process
   results, corpus identity, and occurrence deltas; Task 8 makes those controls
   mandatory before Task 17 deletes code.

## Controller Workspace and Recovery Contract

Before dispatching Task 1, run from
`/Users/allen/git/claude-release-beta-todo-20260919`:

```bash
git fetch origin refactor/tag-machinery main
tools/preflight.sh
tools/preflight.sh --upstream || true
test "$(git rev-list --count HEAD..origin/refactor/tag-machinery)" = "0"
git worktree add -b staging/beta1-functional-integration \
  /Users/allen/git/oxidex-beta1-functional-integration \
  staging/release-beta-todo
```

The preflight output is retained even when it returns nonzero for the known,
intentional `origin/main` divergence. The explicit test must report zero
commits behind `origin/refactor/tag-machinery`; any protected-branch, dirty-
tree, fetch, or tag-machinery freshness failure still blocks setup. Otherwise
rebase the documentation branch in its existing worktree, revalidate this
plan, and only then create the integration worktree.

In the integration worktree, initialize the Superpowers workspace and controller
handoff:

```bash
bash /Users/allen/.codex/plugins/cache/openai-curated-remote/superpowers/6.4.1/skills/subagent-driven-development/scripts/sdd-workspace \
  docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md
tools/preflight.sh
```

The controller records the resolved plan workspace printed by
`sdd-workspace`. Its `progress.md` begins with the plan path, integration
branch/worktree, and the literal output of `git rev-parse HEAD`. Do not copy a
symbolic ref where the resolved SHA belongs.

The controller then creates each task branch/worktree from the current
integration HEAD and writes that resolved SHA into the task PRD and
`HANDOFF.md`.

For every dispatch, the controller copies into the task PRD: this plan's
Global Constraints, the complete task section, its resolved base SHA, the
absolute worktree/target/evidence paths, prerequisite commit/receipt IDs, the
file lease, the handoff cadence, and the exact launch command. The worker must
first pass `tools/preflight.sh`, then run and retain `tools/preflight.sh
--upstream` even if only its known `origin/main` comparison is nonzero, and
finally require the resolved task base with `git merge-base --is-ancestor
"$task_base" HEAD` in its own worktree. The known `origin/main` divergence is
recorded but is not that task's base-freshness gate. A PRD is not a
pointer to this plan: it is the self-contained execution contract that lets a
fresh CLI process resume without conversation history.

The controller creates each task worktree with the branch, worktree, and base
recorded in the task section and ledger:

```bash
task_base=$(git -C /Users/allen/git/oxidex-beta1-functional-integration rev-parse HEAD)
git -C /Users/allen/git/oxidex-beta1-functional-integration worktree add \
  -b "$task_branch" "$task_worktree" "$task_base"
```

`task_branch` and `task_worktree` are set to the literal values in the task
section before this command; the controller records the resulting HEAD before
launching the worker.

Every task uses a child directory named exactly after its task slug under:

```text
/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/
```

Every CLI task PRD lives under:

```text
/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/
```

For example, Task 5 launches from its named worktree as:

```bash
codex --yolo exec --enable fast_mode --model gpt-5.6-terra - \
  < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/05-upgrade-transaction.md
```

## Dependency and Dispatch Map

| Task | Slug | Worker/model | Depends on | May run with |
|---:|---|---|---|---|
| 1 | `ownership-inventory` | Desktop / Terra | controller setup | 2, 4, 5 |
| 2 | `typed-occurrence-core` | Desktop / Sol | controller setup | 1, 4, 5 |
| 3 | `typed-consumers` | CLI / Terra | 2 | 4, 5, 6 |
| 4 | `conv-registry` | Desktop / Sol | controller setup | 1, 2, 5 |
| 5 | `upgrade-transaction` | CLI / Terra | controller setup | 1, 2, 4 |
| 6 | `conformance-receipts` | CLI / Luna | controller setup | any non-conformance task |
| 7 | `file-session` | Desktop / Sol | 2, 4 | 3, 5, 6 |
| 8 | `generated-attribution` | CLI / Terra | 1, 3, 4, 7 | 5, 6 |
| 9 | `exif-shared-pipeline` | Desktop / Sol | 2, 4, 7, 8 | 5, 6 |
| 10 | `refusal-closure` | Desktop / Sol | 9 | 5, 6 |
| 11 | `olympus-pilot` | Desktop / Sol | 3, 8, 10 | 5, 6 |
| 12 | `nikon-port` | CLI / Terra | 11 | 13-16 |
| 13 | `pentax-panasonic-port` | CLI / Terra | 11 | 12, 14-16 |
| 14 | `dji-composite-xmp-port` | CLI / Terra | 11 | 12, 13, 15, 16 |
| 15 | `legacy-camera-tail` | CLI / Terra | 11 | 12-14, 16 |
| 16 | `trailer-tail` | CLI / Terra | 11 | 12-15 |
| 17 | `walker-engine-consolidation` | Desktop / Sol | 9, 11 | reviewed vendor tasks that do not touch engines |
| 18 | `proven-deletion` | Desktop / Sol | 8, 10-17 | documentation-only work |
| 19 | `version-transition-qualification` | CLI / Sol | 5, 6, 18 | documentation-only work |
| 20 | `frozen-candidate-evidence` | controller + Astra review | all prior tasks | none; writers frozen |

Initial dispatch:

- Desktop slots: Tasks 1, 2, and 4.
- CLI pool: Tasks 5 and 6.
- Task 3 launches as soon as Task 2 integrates.
- Task 7 launches after Tasks 2 and 4 integrate.
- Unused CLI capacity remains idle until a dependency-ready task exists; do
  not manufacture speculative work to fill it.

## Integration Procedure for Every Task

For each task, load `task_worktree`, `task_base`, and `task_head` from the
controller ledger:

1. Read the task `HANDOFF.md` and report.
2. Verify `git -C "$task_worktree" status --short` is empty.
3. Create a review package from `$task_base..$task_head` using the Superpowers
   `review-package` script.
4. Dispatch a fresh reviewer at the task's stated reviewer model.
5. Complete the Superpowers fix loop before integration.
6. Compare paths:

```bash
git -C "$task_worktree" diff --name-only "$task_base..$task_head" | sort -u
git -C /Users/allen/git/oxidex-beta1-functional-integration \
  diff --name-only "$task_base..HEAD" | sort -u
```

7. If the sets overlap, rebase the task in its own worktree onto the current
   integration HEAD, rerun covering tests, and re-review the rebased diff.
8. If disjoint and interfaces remain compatible, integrate as one signed
   commit:

```bash
git -C /Users/allen/git/oxidex-beta1-functional-integration merge --squash "$task_head"
git -C /Users/allen/git/oxidex-beta1-functional-integration commit -S \
  -m "$task_commit_message"
```

9. Record task base/head, review verdict, integrated commit, receipts, and
   released dependencies in the controller ledger and integration `HANDOFF.md`.

## Standard Candidate Acceptance Commands

The controller copies this section into every parser/runtime PRD and resolves
the three variables to the task section's literal paths. A worker may narrow
the red/green loop, but its final candidate uses the commands below. The
authenticated instruments refuse dirty trees, so formatting, tests, and
Clippy run first; the worker then commits the candidate; only that clean commit
may be measured. If a later gate fails, fix the code, create a new commit, and
rerun every receipt against the new HEAD. Never cite a receipt from an older
commit.

```bash
task_slug=nikon-port
task_target=/Users/allen/git/oxidex-beta1-targets/nikon-port
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/nikon-port
mkdir -p "$task_target" "$task_evidence"

cargo fmt --check
CARGO_TARGET_DIR="$task_target" \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  "$task_evidence/cargo-test.log" -- \
  cargo test --workspace --all-features
CARGO_TARGET_DIR="$task_target" \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  "$task_evidence/clippy.log" -- \
  cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_TARGET_DIR="$task_target" \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  "$task_evidence/release-build.log" -- \
  cargo build --release --bin oxidex
```

After committing the candidate and proving `git status --short` is empty, run
the combined-corpus receipt under the exclusive lock:

```bash
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  "$task_evidence/conformance.log" -- \
  python3 tools/exiftool-tables/conformance.py \
    /tmp/oxidex-exiftool-cache/combined-samples \
    --recursive --min-files 4000 --min-tags 400000 \
    --exiftool-dir /tmp/oxidex-exiftool-cache/exiftool \
    --oxidex "$task_target/release/oxidex" \
    --json-out "$task_evidence/conformance.json"
```

Run the authenticated `t/images` read receipt and ratchet. `build` uses the
shared Cargo lock; `observe`, `verify`, and the regression gate use the
exclusive measurement lock:

```bash
CARGO_TARGET_DIR="$task_target" \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  "$task_evidence/read-build.log" -- \
  python3 tools/exiftool-tables/corpus_read_receipt.py build \
    --output "$task_evidence/read-build"
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  "$task_evidence/read-observe.log" -- \
  python3 tools/exiftool-tables/corpus_read_receipt.py observe \
    --build-proof "$task_evidence/read-build/build-proof.json" \
    --perl /tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2 \
    --exiftool-dir /tmp/oxidex-exiftool-cache/exiftool \
    --corpus /tmp/oxidex-exiftool-cache/exiftool/t/images \
    --output "$task_evidence/read-observe"
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  "$task_evidence/read-verify.log" -- \
  python3 tools/exiftool-tables/corpus_read_receipt.py verify \
    --receipt "$task_evidence/read-observe/receipt.json"
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  "$task_evidence/read-gate.log" -- \
  python3 tools/ci/read_regression_gate.py \
    --receipt "$task_evidence/read-observe/receipt.json"
```

Every task substitutes its own literal slug, target, and evidence path from
its task section. The full conformance JSON must reconcile, and parser tasks
require zero previously matched occurrences lost, zero new VALUE rows, and
read-regression `lost 0`. Targeted gains are asserted by the task's named
carrier tests, not by grepping formatted conformance output.

---

### Task 1: Ownership Inventory and Duplicate-Owner Verifier

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/01-ownership-inventory.md`

**Worker:** Desktop subagent, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/ownership-inventory`
**Worktree:** `/Users/allen/git/oxidex-beta1-ownership-inventory`
**Target:** `/Users/allen/git/oxidex-beta1-targets/ownership-inventory`
**Commit:** `feat: verify generated and residual field ownership`

**Files:**

- Create: `tools/exiftool-tables/runtime_ownership.py`
- Create: `tools/exiftool-tables/runtime_ownership.json`
- Create: `tools/exiftool-tables/runtime_ownership.d/` source fragments
- Create: `tools/exiftool-tables/test_runtime_ownership.py`
- Modify: `justfile`
- Do not modify: `src/exiftool_tables/conv/**`, engine files, generated tables

**Interfaces:**

- Produces `runtime_ownership.json` schema 1 with source release/hash, stable
  module/table/field identity, owner category, symbol, refusal, and fixture.
  Hand-maintained ownership is split into deterministic family fragments under
  `runtime_ownership.d/` so later vendor tasks do not edit one shared file.
- Produces `runtime_ownership.py verify --root .`; exit 0 means every
  enabled field has exactly one owner and every residual symbol still exists.
- Consumes existing conversion ledgers, enabled-table registries, and explicit
  hand-residual arrays without assigning observation credit.

- [ ] **Step 1: Write failing schema and duplicate-owner tests**

Add fixtures in `test_runtime_ownership.py` that assert:

```python
self.assertEqual(row["owner"], "generated")
with self.assertRaisesRegex(ownership.Refused, "duplicate owner"):
    ownership.verify_rows([generated_row, residual_row])
with self.assertRaisesRegex(ownership.Refused, "unowned enabled field"):
    ownership.verify_rows([enabled_without_owner])
```

- [ ] **Step 2: Prove the tests fail before implementation**

Run:

```bash
uv run python -m unittest tools/exiftool-tables/test_runtime_ownership.py -v
```

Expected: import/file failure because `runtime_ownership.py` does not exist.

- [ ] **Step 3: Implement the inventory and verifier**

Use stable identity shaped as:

```python
{
    "module": "Exif",
    "table": "Main",
    "field": {"kind": "numeric", "value": "0x829a"},
    "owner": "generated",
    "symbol": "src/exiftool_tables/conv/exif_main.rs::decode",
    "source_release": "13.59",
    "source_sha256": hashlib.sha256(source_row_bytes).hexdigest(),
    "refusal": None,
    "fixture": str(carrier.relative_to(repository_root)),
}
```

Support `numeric`, `name`, and `index` field kinds. Infer generated rows from
committed ledgers; require explicit JSON entries for walker/residual/refused
ownership. Do not infer runtime coverage from declarations.

- [ ] **Step 4: Seed current Exif::Main and directory residual ownership**

Account for all 551 generated, 17 refused, and 29 non-conversion ledger fields,
plus `IFD0_HAND_KEPT`, `EXIF_IFD_HAND_KEPT`, IFD1 residuals, and structural
edge ownership. Every refusal records its current reason rather than being
silently categorized as residual.

- [ ] **Step 5: Add the verifier recipe and run focused tests**

Add `just verify-runtime-ownership`, then run the test and recipe. Expected:
all synthetic controls pass and the repository inventory reconciles.

- [ ] **Step 6: Update handoff, commit, and report**

Record counts by owner category and every intentionally unresolved refusal.

---

### Task 2: Canonical Typed-Occurrence Core

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/02-typed-occurrence-core.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/typed-occurrence-core`
**Worktree:** `/Users/allen/git/oxidex-beta1-typed-occurrence-core`
**Target:** `/Users/allen/git/oxidex-beta1-targets/typed-occurrence-core`
**Commit:** `refactor: define canonical metadata value channels`

**Files:**

- Modify: `src/core/tag_occurrence.rs`
- Modify: `src/core/tag_sink.rs`
- Modify: `src/core/metadata_map.rs`
- Modify focused tests in those files
- Do not modify CLI, FFI, composites, writers, sessions, or engines

**Interfaces:**

- Produces `ValueChannel::{Stored, ValueConv, PrintConv}`.
- Produces `TagOccurrence::project(channel) -> TagValue` with explicit fallback
  rules.
- Produces winner/occurrence projection APIs on `TagSink` and `MetadataMap` for
  downstream Task 3.

- [ ] **Step 1: Write failing channel tests**

Pin all combinations:

```rust
assert_eq!(occ.project(ValueChannel::Stored), stored);
assert_eq!(occ.project(ValueChannel::ValueConv), typed);
assert_eq!(occ.project(ValueChannel::PrintConv), printed);
```

Also test fallbacks, `undef`, byte strings, binary display, signed zero, lists,
duplicate winners, tombstones, and instance-specific winners.

- [ ] **Step 2: Run the focused library tests and observe failure**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/typed-occurrence-core \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-occurrence-core/red.log -- \
cargo test --lib core::
```

Expected: compile failure because `ValueChannel` and `project` do not exist.

- [ ] **Step 3: Implement the channel contract**

Use these semantics:

```rust
pub enum ValueChannel { Stored, ValueConv, PrintConv }

// Stored: stored.as_ref().unwrap_or(&raw)
// ValueConv: explicit value, then exact existing compatibility ValueConv, then raw
// PrintConv: explicit print, then ValueConv projection
```

Keep `raw` as decoded/pre-conversion input. Do not parse display strings to
manufacture typed values.

- [ ] **Step 4: Add sink/map projection APIs without migrating consumers**

Preserve compatibility for existing `get()` call sites in this task. Add
explicit projection methods used by Task 3 and occurrence enumeration that
does not collapse duplicates.

- [ ] **Step 5: Run focused tests, formatting, and Clippy**

Use the shared lock for Cargo. Require all affected core tests green and zero
Clippy warnings:

```bash
cargo fmt --check
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/typed-occurrence-core \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-occurrence-core/test.log -- \
  cargo test --lib core::
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/typed-occurrence-core \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-occurrence-core/clippy.log -- \
  cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- [ ] **Step 6: Update handoff, commit, and report**

The report lists every fallback rule and any compatibility conversion still
living in `exiftool_compat.rs`.

---

### Task 3: Migrate Typed-Occurrence Consumers

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/03-typed-consumers.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/typed-consumers`
**Worktree:** `/Users/allen/git/oxidex-beta1-typed-consumers`
**Target:** `/Users/allen/git/oxidex-beta1-targets/typed-consumers`
**Commit:** `refactor: project typed metadata values consistently`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-terra - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/03-typed-consumers.md`

**Files:**

- Modify: `src/cli/tag_resolution.rs`
- Modify: CLI JSON/output formatter modules selected by call-site search
- Modify: `src/ffi/**`
- Modify: `src/composite/compute.rs`
- Modify writer/copy call sites that consume occurrences
- Add: `tests/typed_value_projection_tests.rs`
- Add: `tools/exiftool-tables/fixtures/typed_value_projection.json`
- Do not modify Task 2 core files or generated/engine files

**Interfaces:** Consumes Task 2's `ValueChannel` and projection APIs.

- [ ] **Step 1: Add a normal/`-n` characterization matrix**

Cover integer, float, rational, enum, date/time, bytes, binary placeholder,
list, undefined/suppressed, duplicate/grouped occurrences, units, and negative
zero. Assert normal output uses PrintConv and `-n` uses ValueConv from the same
occurrence.

- [ ] **Step 2: Prove at least one pre-migration test fails**

Run the focused integration test under the shared lock:

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/typed-consumers \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-consumers/red.log -- \
  cargo test --test typed_value_projection_tests -- --nocapture
```

The generated IFD case must demonstrate the current wrong-channel behavior
rather than a fabricated unit-only failure.

- [ ] **Step 3: Migrate consumers by intent**

- CLI normal/JSON default: `PrintConv`.
- CLI `-n`: `ValueConv`.
- file-copy/rebuild: `Stored`.
- composite inputs: `ValueConv`, except source-proven stringify/reparse cases.
- FFI/library: expose explicit channel selection without silently changing ABI
  layout.

- [ ] **Step 4: Inventory remaining display reparsing**

Add a test-backed allowlist naming the ExifTool source location for each
intentional stringify/reparse. Unlisted parsing of printed units or fractions
fails the focused verifier.

- [ ] **Step 5: Run focused tests, commit clean candidate, and measure**

Use only the pinned Perl/ExifTool paths. Save paired raw outputs and hashes in
the task evidence directory. Rerun the exact focused command from Step 2 and
then the Standard Candidate Acceptance Commands with:

```bash
task_slug=typed-consumers
task_target=/Users/allen/git/oxidex-beta1-targets/typed-consumers
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/typed-consumers
```

Run formatting and Clippy before the signed task commit. Run the conformance
and read-receipt blocks only after that commit leaves the worktree clean.

- [ ] **Step 6: Update handoff and report**

---

### Task 4: Generalize the Generated Conversion Registry

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/04-conv-registry.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/conv-registry`
**Worktree:** `/Users/allen/git/oxidex-beta1-conv-registry`
**Target:** `/Users/allen/git/oxidex-beta1-targets/conv-registry`
**Commit:** `feat: generate conversion registry for enabled tables`

**Files:**

- Modify: `tools/exiftool-tables/conv_codegen.py`
- Modify: `tools/exiftool-tables/conv_oracle.py`
- Modify: related conversion codegen tests and artifact manifest
- Modify: `src/exiftool_tables/conv/mod.rs`
- Create generated registry/modules under `src/exiftool_tables/conv/`
- Modify: `src/exiftool_tables/conv/tests.rs`
- Do not modify IFD/session/core occurrence files

**Interfaces:** Produces table-identity registry entries containing decoder and
table-local claim function. Keeps `RawConv`, `ValueConv`, and `PrintConv`
distinct. Extends `conv_oracle.py` with `--all`, which checks every emitted
registry entry in deterministic identity order.

- [ ] **Step 1: Add failing multi-table registry tests**

Use a synthetic second table and assert `decoder()` and `claims()` select its
own functions rather than `exif_main::claims`. Add missing/orphan/stale and
duplicate table identity controls.

- [ ] **Step 2: Run conversion codegen and Rust tests red**

```bash
uv run python -m unittest tools/exiftool-tables/test_conv_codegen.py -v
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/conv-registry \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/conv-registry/red.log -- \
  cargo test --lib exiftool_tables::conv::tests
```

Expected: second-table dispatch fails because the registry is hard-coded to
`Exif::Main`.

- [ ] **Step 3: Generate the registry**

Emit a deterministic sorted registry such as:

```rust
pub struct Entry {
    pub module: &'static str,
    pub table: &'static str,
    pub decode: Decode,
    pub claims: fn(u16) -> bool,
}
```

Do not hand-maintain table names. Preserve per-field mixed mode and ledger
refusals.

- [ ] **Step 4: Generate per-table ledgers and stable identities**

Unrelated source row movement must not renumber write/test identities. Refuse
non-IFD key shapes until a typed key interface is implemented; do not coerce a
name/index into `u16`.

- [ ] **Step 5: Run codegen tests, pinned conversion oracle, and verifier**

Run generation twice and require `git diff --exit-code` for owned generated
paths after the second run. Use:

```bash
uv run python -m unittest tools/exiftool-tables/test_conv_codegen.py -v
uv run python tools/exiftool-tables/conv_oracle.py --check --all \
  --perl /tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2 \
  --exiftool-dir /tmp/oxidex-exiftool-cache/exiftool
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/conv-registry \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/conv-registry/regen-1.log -- \
  tools/exiftool-tables/regen-all.sh
git add src/exiftool_tables/conv tools/exiftool-tables
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/conv-registry \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/conv-registry/regen-2.log -- \
  tools/exiftool-tables/regen-all.sh
git diff --exit-code -- src/exiftool_tables/conv tools/exiftool-tables
```

- [ ] **Step 6: Run Rust tests, fmt, Clippy, handoff, commit, and report**

---

### Task 5: Fresh BEFORE/AFTER Upgrade Transaction

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/05-upgrade-transaction.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/upgrade-transaction`
**Worktree:** `/Users/allen/git/oxidex-beta1-upgrade-transaction`
**Target:** `/Users/allen/git/oxidex-beta1-targets/upgrade-transaction`
**Commit:** `fix: regenerate both sides of ExifTool upgrades`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-terra - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/05-upgrade-transaction.md`

**Files:**

- Modify: `tools/exiftool-tables/upgrade_transaction.py`
- Modify: `tools/exiftool-tables/test_upgrade_transaction.py`
- Modify only when tests require it: version-rehearsal stage/executor files and
  their focused tests
- Do not modify `.exiftool-version` or generated artifacts

- [ ] **Step 1: Add the same-pin stale-artifact regression test**

Assert the BEFORE variant invokes regeneration even when
`old == committed_pin`, records its source hash, and detects a stale committed
artifact.

- [ ] **Step 2: Run the focused test red**

```bash
uv run python -m unittest tools/exiftool-tables/test_upgrade_transaction.py -v
```

Expected: the new assertion fails at
`variant("before", self.old, self.old != self.pin)`.

- [ ] **Step 3: Remove the skip and preserve transaction recovery**

Generate BEFORE and AFTER in separate clean variant trees and target
directories. Do not mutate the caller until both variants, comparison, and
gates pass.

- [ ] **Step 4: Add failure controls**

Cover interrupted BEFORE generation, added/removed modules, changed helper
source, undeclared output, and recovery that restores the original pin and
artifacts.

- [ ] **Step 5: Run all transaction/rehearsal unit tests**

Save command/output in the task evidence directory:

```bash
mkdir -p /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/upgrade-transaction
set -o pipefail
uv run python -m unittest \
  tools/exiftool-tables/test_upgrade_transaction.py \
  tools/exiftool-tables/test_version_rehearsal.py -v \
  2>&1 | tee /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/upgrade-transaction/tests.log
```

If `test_version_rehearsal.py` does not exist at the task base, create it in
Step 1 rather than omitting that test target.

- [ ] **Step 6: Update handoff, commit, and report**

---

### Task 6: Complete Occurrence-Aware Conformance Receipts

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/06-conformance-receipts.md`

**Worker:** Codex CLI, `gpt-5.6-luna`, fast mode
**Reviewer:** `gpt-5.6-terra`, fast mode
**Branch:** `staging/beta1/conformance-receipts`
**Worktree:** `/Users/allen/git/oxidex-beta1-conformance-receipts`
**Target:** `/Users/allen/git/oxidex-beta1-targets/conformance-receipts`
**Commit:** `test: authenticate conformance occurrence totals`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-luna - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/06-conformance-receipts.md`

**Files:**

- Modify: `tools/exiftool-tables/conformance.py`
- Modify: `tools/exiftool-tables/test_conformance.py`
- Do not modify parsers, runtime, or generated files

- [ ] **Step 1: Add a failing JSON receipt test**

Require top-level raw oracle occurrence total, candidate occurrence total,
matched/value/missing/extra/rename reconciliation, instrument provenance,
binary hash, corpus roots/file count, oracle source/runtime, and floors.

- [ ] **Step 2: Run the focused Python test red**

```bash
uv run python -m unittest tools/exiftool-tables/test_conformance.py -v
```

Expected: the raw oracle occurrence field is absent.

- [ ] **Step 3: Add fields without changing matching behavior**

Keep formatted output stable except for an explicit instrument line. Maintain
byte-identical JSON under different hash seeds.

- [ ] **Step 4: Add vacuity and tampering controls**

Reject missing floors, changed binary after measurement, and totals that do not
reconcile.

- [ ] **Step 5: Run `test_conformance.py`, typos, handoff, commit, and report**

```bash
uv run python -m unittest tools/exiftool-tables/test_conformance.py -v
typos tools/exiftool-tables/conformance.py tools/exiftool-tables/test_conformance.py
```

---

### Task 7: Make Session File-Scoped

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/07-file-session.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/file-session`
**Worktree:** `/Users/allen/git/oxidex-beta1-file-session`
**Target:** `/Users/allen/git/oxidex-beta1-targets/file-session`
**Commit:** `refactor: retain ExifTool session state for each file`

**Files:**

- Modify: `src/exiftool_tables/session.rs`
- Modify: `src/exiftool_tables/ifd_engine.rs`
- Modify: `src/exiftool_tables/cond.rs` only for required session access
- Modify: `src/core/exif_dir_engine.rs`
- Modify: `src/core/tiff_helpers.rs` and `src/core/jpeg_helpers.rs` only at
  session-construction/call boundaries
- Add: `tests/generated_file_session.rs`
- Add focused Rust unit tests in those modules
- Do not change conversion registry generation or occurrence consumer APIs

**Interfaces:**

- One `Session` is created by the file-level caller and passed through every
  directory walk.
- Directory scope reset/restore covers `DIR_NAME`, `Compression`,
  `SubfileType`, byte order, count, and format while preserving file members,
  values, options, warnings, and processed state.

- [ ] **Step 1: Write failing cross-directory state tests**

Build a synthetic parent/child/next-sibling walk in which:

```rust
// Parent sets a DataMember.
// Child reads it and temporarily changes DIR_NAME and Compression.
// Sibling still reads the parent DataMember but not the child's directory state.
```

Add byte-string, `undef`, negative-zero, evaluation-order, recursion guard, and
duplicate-occurrence controls.

- [ ] **Step 2: Run the focused Session and IFD tests red**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/file-session \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/file-session/red.log -- \
  cargo test --test generated_file_session -- --nocapture
```

Expected: the cross-directory read fails because `ifd_engine` constructs a new
session per directory.

- [ ] **Step 3: Add explicit directory scope state**

Implement a snapshot/restore API whose types make directory-local and
file-global fields explicit. Restoration must occur on Report, Suppress,
Decline, and ordinary error return.

- [ ] **Step 4: Thread one mutable Session from file entry points**

Remove directory-local `Session::new()`/`for_ifd()` construction from the walk.
Callers seed only state they actually know; absent data remains unsupplied and
causes a generated decline rather than a guessed value.

- [ ] **Step 5: Run focused tests and the full Rust library suite**

Use the shared lock. Require no ordering, group, or warning regression:

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/file-session \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/file-session/test.log -- \
  cargo test --lib
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/file-session \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/file-session/integration.log -- \
  cargo test --test generated_file_session
```

- [ ] **Step 6: Run fmt, Clippy, handoff, commit, and report**

---

### Task 8: Validate Generated-On/Generated-Off Attribution

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/08-generated-attribution.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/generated-attribution`
**Worktree:** `/Users/allen/git/oxidex-beta1-generated-attribution`
**Target:** `/Users/allen/git/oxidex-beta1-targets/generated-attribution`
**Commit:** `test: authenticate generated route attribution`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-terra - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/08-generated-attribution.md`

**Files:**

- Modify: `tools/exiftool-tables/genshare/attribute.py`
- Modify: `tools/exiftool-tables/genshare/census.sh`
- Replace or update: `tools/exiftool-tables/genshare/probe.patch`
- Modify: `tools/exiftool-tables/genshare/README.md`
- Add: focused Python/shell tests under `tools/exiftool-tables/genshare/`
- Modify the minimum outward-boundary Rust seams needed for a maintained test
  hook; no behavior when the hook is disabled
- Do not change tag conversion semantics

**Interfaces:** Produces an authenticated paired control/probe receipt consumed
by Tasks 11 and 18. Replaces positional worktree arguments with this maintained
interface:

```bash
tools/exiftool-tables/genshare/census.sh \
  --repository /Users/allen/git/oxidex-beta1-generated-attribution \
  --target-dir /Users/allen/git/oxidex-beta1-targets/generated-attribution \
  --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/generated-attribution/census \
  --corpus /tmp/oxidex-exiftool-cache/combined-samples \
  --perl /tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2 \
  --exiftool-dir /tmp/oxidex-exiftool-cache/exiftool \
  --tokens engine,legacy-l1,legacy-l2,producers,serial,keyed
```

The script builds one maintained binary whose unset/empty hook is the control,
then runs each explicit token as a probe. It writes `receipt.json` and exits
nonzero on a refused token, process failure, floor miss, inertness difference,
hash mismatch, or attribution reconciliation failure.

- [ ] **Step 1: Add failing receipt-authentication tests**

Require control/probe binary SHA-256, source SHA/dirty state, exact process
return codes, raw stdout/stderr hashes, oracle/corpus identity, inertness result,
per-occurrence deltas, and token set. Refuse historical `attr-72ea`-style data
missing paired outputs.

- [ ] **Step 2: Run the focused tests red**

Create and run `tools/exiftool-tables/genshare/test_attribute.py` and
`tools/exiftool-tables/genshare/test_census.py`:

```bash
uv run python -m unittest \
  tools/exiftool-tables/genshare/test_attribute.py \
  tools/exiftool-tables/genshare/test_census.py -v
```

Expected: the current summarized inputs lack required authentication fields.

- [ ] **Step 3: Convert the temporary probe into a maintained test seam**

The seam may silence outward reporting but must retain session writes,
descents, cipher state, and other internal effects. Unknown tokens exit nonzero.
Unset/empty control output must be byte-identical to the ordinary binary for
all deterministic fields.

- [ ] **Step 4: Harden `census.sh`**

Use explicit `CARGO_TARGET_DIR` paths, pinned Perl/source probes, candidate
hashes, corpus floors, `pipefail`, and durable per-stage status. Never infer a
successful command from `tail` or another pipeline consumer.

- [ ] **Step 5: Run bounded controls, commit, and run a small real census**

Use the exclusive lock. Prove one known generated fixture loses its generated
occurrence, one hand-only fixture does not, and the inert control matches.
The test invokes `census.sh` with an explicit three-file manifest committed at
`tools/exiftool-tables/genshare/testdata/bounded-corpus.txt` and writes its
authenticated receipt under
`/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/generated-attribution/`.
Run the Python/shell tests, Rust tests for the maintained seam, formatting, and
Clippy first. Commit the signed task candidate and require a clean worktree.
Then run the full command above through the exclusive lock.

- [ ] **Step 6: Update handoff and report**

---

### Task 9: Exact-Once Exif Shared Pipeline

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/09-exif-shared-pipeline.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/exif-shared-pipeline`
**Worktree:** `/Users/allen/git/oxidex-beta1-exif-shared-pipeline`
**Target:** `/Users/allen/git/oxidex-beta1-targets/exif-shared-pipeline`
**Commit:** `refactor: route Exif directories through one tag pipeline`

**Files:**

- Modify: `src/exiftool_tables/conv/mod.rs`
- Modify: `src/exiftool_tables/ifd_engine.rs`
- Modify: `src/core/exif_dir_engine.rs`
- Modify: `src/core/tiff_helpers.rs`
- Modify: `src/core/jpeg_helpers.rs`
- Modify: `src/exiftool_tables/enabled_ifd.rs`
- Add focused tests in these modules
- Add: `tests/exif_shared_pipeline.rs`
- Do not edit vendor parser directories or delete compatibility branches

**Interfaces:** Consumes Tasks 2, 4, and 7. Produces one execution result with
staged session effects and one exact-once route for IFD0, IFD1, ExifIFD, and
InteropIFD.

- [ ] **Step 1: Add failing side-effect and exact-once tests**

Pin:

```rust
// Report commits generated writes exactly once.
// Suppress commits source-required writes, emits nothing, and never falls back.
// Decline discards staged writes before invoking exactly one residual.
```

Also test duplicate order, group 1, requested edge tags, nested directories,
and a non-UTF-8 reported scalar that currently declines at the text boundary.

- [ ] **Step 2: Run focused engine tests red**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/exif-shared-pipeline \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/exif-shared-pipeline/red.log -- \
  cargo test --test exif_shared_pipeline -- --nocapture
```

Expected: at least the decline/write and cross-directory route controls fail.

- [ ] **Step 3: Stage generated effects until outcome resolution**

Do not mutate the live session before Report/Suppress/Decline ownership is
known. Commit effects in source order only for outcomes whose ExifTool source
requires them.

- [ ] **Step 4: Route all four standard Exif directories once**

Replace replay/drain ownership ambiguity with one generated-first route and at
most one named residual. Preserve structural walker ownership for offset,
SubIFD, MakerNote, IPTC, GeoTIFF, and PrintIM edges.

- [ ] **Step 5: Make request-aware edge behavior explicit**

An explicitly requested edge tag is not discarded merely because its normal
directory behavior is silent. Add the paired default/requested test.

- [ ] **Step 6: Run tests, fmt, Clippy, commit, and measure**

Run `cargo test --test exif_shared_pipeline`, `cargo test --lib`, formatting,
and Clippy through the Standard Candidate Acceptance Commands with:

```bash
task_slug=exif-shared-pipeline
task_target=/Users/allen/git/oxidex-beta1-targets/exif-shared-pipeline
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/exif-shared-pipeline
```

Commit the signed task candidate after tests/formatting/Clippy and before the
conformance/read-receipt portions of the standard commands.

- [ ] **Step 7: Update handoff and report**

The report lists every remaining replay/drain/yield/residual path and its owner.

---

### Task 10: Close or Classify the 17 Exif::Main Refusals

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/10-refusal-closure.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/refusal-closure`
**Worktree:** `/Users/allen/git/oxidex-beta1-refusal-closure`
**Target:** `/Users/allen/git/oxidex-beta1-targets/refusal-closure`
**Commit:** `feat: close Exif conversion refusal ownership`

**Files:**

- Modify conversion grammar/codegen/runtime/helper files required by the 17
  rows
- Add: `tools/exiftool-tables/test_exif_main_refusal_closure.py`
- Add: `tests/exif_main_refusal_closure.rs`
- Modify focused conversion oracle captures and tests
- Modify ownership inventory entries
- Modify named residual handlers only for rows proven residual-owned
- Do not perform broad compatibility deletion

- [ ] **Step 1: Generate a checked refusal worklist**

The worklist contains the exact source expression/body/hash, probes, current
reason, required helper/grammar/walker behavior, and target owner for all 17.

- [ ] **Step 2: Split conversion work from traversal work**

Keep `SubDirectory`, offsets, SubIFD, MakerNote, and DNG private-data edges
walker-owned. Do not model traversal as a scalar conversion.

- [ ] **Step 3: Implement exact source-supported conversion slices**

Cover, where source proof permits:

- `SetPriorityDir` and `IdentifyRawFile` behavior;
- `LearningOptOutIn` shift semantics;
- declarations/loops/rationals for `CompositeImageExposureTimes`;
- `ConvertBinary`/`PrintOpcode` for OpcodeList1/2/3; and
- declarations/loops/`sprintf` for `TimeCodes`.

Each new helper is selected by source body/hash across supported releases.

- [ ] **Step 4: Resolve residual ownership hazards**

Pin `Copyright` without IFD1 double insertion, AmbientTemperature signed zero,
XP strings through `Decode`, and every retained structural residual.

- [ ] **Step 5: Run conversion oracle, ownership verifier, and affected Rust tests**

No refusal disappears unless generated output or an explicit residual owner
replaces it. Run:

```bash
uv run python -m unittest tools/exiftool-tables/test_exif_main_refusal_closure.py -v
uv run python tools/exiftool-tables/conv_oracle.py --check --all \
  --perl /tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2 \
  --exiftool-dir /tmp/oxidex-exiftool-cache/exiftool
uv run python tools/exiftool-tables/runtime_ownership.py verify --root .
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/refusal-closure \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/refusal-closure/test.log -- \
  cargo test --test exif_main_refusal_closure
```

- [ ] **Step 6: Regenerate twice and verify clean**

Run `tools/exiftool-tables/regen-all.sh`, stage intended generated changes,
run it again, and require `git diff --exit-code` to show no unstaged drift.

- [ ] **Step 7: Run fmt, Clippy, handoff, commit, and report**

---

### Task 11: Olympus End-to-End Pilot

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/11-olympus-pilot.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/olympus-pilot`
**Worktree:** `/Users/allen/git/oxidex-beta1-olympus-pilot`
**Target:** `/Users/allen/git/oxidex-beta1-targets/olympus-pilot`
**Commit:** `feat: complete Olympus generated runtime migration`

**Files:**

- Modify: `src/parsers/tiff/makernotes/olympus.rs`
- Modify: `src/parsers/tiff/makernotes/olympus/**`
- Modify: Olympus entries in generated/enabled table sources through the
  generator, never by hand-editing generated tables
- Modify: Olympus integration and pinned-oracle tests
- Add: `tests/olympus_main_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/olympus.json`
- Modify: Olympus deletion ledger entries
- Do not modify unrelated vendor parsers

- [ ] **Step 1: Reproduce the remaining Olympus gaps**

Using the pinned combined corpus, capture `StackedImage`, `CameraParameters`,
`Quality`, `ZoomedPreviewImage`, and `CameraType` normal/`-n`, group, and
duplicate behavior. Record carrier files and exact oracle occurrences.

- [ ] **Step 2: Write failing generated-route tests**

Require the generated route to produce the target rows with the expected typed
and printed forms. A test that passes through the old hand path is invalid;
include generated-on/off attribution.

- [ ] **Step 3: Re-express missing source facts on the generated path**

Use source-derived tables, conditions, and conversions. Retain only the three
documented structural/post-pass cases that the generated schema cannot express,
each with an ownership record.

- [ ] **Step 4: Delete only Olympus-local paths replaced in this task**

Require a deletion ledger row, passing attribution, and occurrence-aware oracle
comparison for each symbol. Do not touch central compatibility code.

- [ ] **Step 5: Test, format, verify generation, and commit the candidate**

Require target MISSING rows to become matched, zero matched-to-lost rows, zero
new VALUE rows, and read-regression `lost 0`. Run the focused carrier suite:

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/olympus-pilot \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/olympus-pilot/focused.log -- \
  cargo test --test olympus_main_forward_port -- --include-ignored --nocapture
```

Run generator verification, formatting, and Clippy, then create the signed
task commit and require a clean worktree.

- [ ] **Step 6: Run exclusive measurements, update handoff, and report**

Run the Standard Candidate Acceptance measurement blocks with:

```bash
task_slug=olympus-pilot
task_target=/Users/allen/git/oxidex-beta1-targets/olympus-pilot
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/olympus-pilot
```

The controller freezes the shared adapter interfaces after this task integrates.

---

### Task 12: Nikon Forward-Port on the Frozen Pipeline

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/12-nikon-port.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/nikon-port`
**Worktree:** `/Users/allen/git/oxidex-beta1-nikon-port`
**Target:** `/Users/allen/git/oxidex-beta1-targets/nikon-port`
**Commit:** `feat: forward-port remaining Nikon metadata parity`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-terra - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/12-nikon-port.md`

**Files:**

- Modify only: `src/parsers/tiff/makernotes/nikon.rs` and
  `src/parsers/tiff/makernotes/nikon/**`
- Modify Nikon-specific tests/fixtures
- Add: `tests/nikon_main_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/nikon.json`
- Do not modify shared engines, helpers, central registries, or generated output

- [ ] **Step 1: Re-measure Nikon gaps at the task base**

Use the current divergence report only as a hypothesis. Confirm
`FirmwareVersion56`, `PixelShiftActive`, `Converter`, `Focus`,
`WBBracketingSteps`, AFInfo2, and remaining rows against the pinned oracle.

- [ ] **Step 2: Add failing Nikon carrier tests**

Tests must fail for the named row/value/group, not silently return when a
fixture is absent. Corpus tests are explicitly ignored with a reason and are
run using `--include-ignored` in this task.

- [ ] **Step 3: Port exact source behavior using frozen interfaces**

If a missing shared helper is required, stop that slice and request a serialized
spine task. Do not copy a vendor-local approximation.

- [ ] **Step 4: Run focused tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/nikon-port \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/nikon-port/focused.log -- \
  cargo test --test nikon_main_forward_port -- --include-ignored --nocapture
```

Run the non-measurement portion of the Standard Candidate Acceptance Commands,
create the signed task commit, and require a clean worktree.

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run both authenticated measurement blocks with the literal Nikon task values.
Require target gains, zero lost matched rows, zero new VALUE rows, and read
gate `lost 0`.

---

### Task 13: Pentax and Panasonic Forward-Port

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/13-pentax-panasonic-port.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/pentax-panasonic-port`
**Worktree:** `/Users/allen/git/oxidex-beta1-pentax-panasonic-port`
**Target:** `/Users/allen/git/oxidex-beta1-targets/pentax-panasonic-port`
**Commit:** `feat: forward-port Pentax and Panasonic metadata parity`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-terra - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/13-pentax-panasonic-port.md`

**Files:**

- Modify: `src/parsers/tiff/makernotes/pentax.rs` and Pentax submodules
- Modify: `src/parsers/tiff/makernotes/panasonic.rs` and Panasonic submodules
- Modify only the Panasonic `LensType` composite arm in
  `src/composite/compute.rs`
- Modify family-specific tests/fixtures
- Add: `tests/pentax_panasonic_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/pentax-panasonic.json`
- Do not modify shared engines or registries

- [ ] **Step 1: Re-measure family gaps**

Confirm Pentax continuous-autofocus selections/focus, external flash guide
number, city codes, ISO, Panasonic MakerNoteType/Gain, and the Panasonic
LensType composite.

- [ ] **Step 2: Add failing carrier and composite-cascade tests**

Prove LensType consumes a typed MakerNote input rather than parsing display
text. Include normal/`-n` output.

- [ ] **Step 3: Port exact source behavior**

Keep family edits within the lease. Escalate shared helper needs.

- [ ] **Step 4: Run focused tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/pentax-panasonic-port \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/pentax-panasonic-port/focused.log -- \
  cargo test --test pentax_panasonic_forward_port -- --include-ignored --nocapture
```

Run the non-measurement portion of the Standard Candidate Acceptance Commands,
create the signed task commit, and require a clean worktree.

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run both authenticated measurement blocks with the literal Pentax/Panasonic
task values. Require zero lost reads and zero new VALUE rows.

---

### Task 14: Remaining DJI, Composite, and XMP Forward-Port

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/14-dji-composite-xmp-port.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/dji-composite-xmp-port`
**Worktree:** `/Users/allen/git/oxidex-beta1-dji-composite-xmp-port`
**Target:** `/Users/allen/git/oxidex-beta1-targets/dji-composite-xmp-port`
**Commit:** `feat: complete DJI and dependent metadata parity`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-terra - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/14-dji-composite-xmp-port.md`

**Files:**

- Modify: DJI MakerNote/debug parser files
- Modify: current XMP family/group implementation, not deleted
  `namespace_mapping.rs`
- Modify: only directly source-required composite arms
- Modify family-specific tests/fixtures
- Add: `tests/dji_main_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/dji-composite-xmp.json`
- Do not re-port already landed DJI float rows

- [ ] **Step 1: Re-measure remaining DJI/XMP/composite gaps**

Separate already landed floats from `FlightSpeed`, debug tags, embedded
`XMP-drone-dji`, and true composite dependencies.

- [ ] **Step 2: Add failing group-aware tests**

Pin family-1 XMP groups, typed numeric values, and composite cascade behavior.

- [ ] **Step 3: Port source behavior on current XMP machinery**

Do not resurrect the deleted namespace mapping implementation merely to apply
an old commit.

- [ ] **Step 4: Run focused tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/dji-composite-xmp-port \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/dji-composite-xmp-port/focused.log -- \
  cargo test --test dji_main_forward_port -- --include-ignored --nocapture
```

Run the non-measurement portion of the Standard Candidate Acceptance Commands,
create the signed task commit, and require a clean worktree.

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run both authenticated measurement blocks with the literal DJI task values.
Require zero lost reads and zero new VALUE rows.

---

### Task 15: Legacy Camera Long Tail

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/15-legacy-camera-tail.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/legacy-camera-tail`
**Worktree:** `/Users/allen/git/oxidex-beta1-legacy-camera-tail`
**Target:** `/Users/allen/git/oxidex-beta1-targets/legacy-camera-tail`
**Commit:** `feat: forward-port legacy camera metadata parity`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-terra - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/15-legacy-camera-tail.md`

**Files:**

- Modify only family files for Kodak, Casio, HP, Ricoh, and JVC
- Modify their registries/tests/fixtures
- Add: `tests/legacy_camera_tail_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/legacy-camera-tail.json`
- Do not modify shared engines or unrelated vendor files

- [ ] **Step 1: Re-measure exact remaining rows**

Cover KodakMaker/DateTimeStamp/TimeCreated, Casio BestShotMode/ArtMode/Quality,
HP CameraDateTime/ISO, Ricoh make/model, and JVC CPUVersions/Quality.

- [ ] **Step 2: Add one real-carrier failing test per family**

- [ ] **Step 3: Port source-derived values and routing**

- [ ] **Step 4: Run family tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/legacy-camera-tail \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/legacy-camera-tail/focused.log -- \
  cargo test --test legacy_camera_tail_forward_port -- --include-ignored --nocapture
```

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run the non-measurement portion of the Standard Candidate Acceptance Commands
before the signed commit. After the worktree is clean, run both authenticated
measurement blocks with the literal legacy-camera task values.

---

### Task 16: Samsung, MediaJukebox, and Vivo Trailers

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/16-trailer-tail.md`

**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode
**Reviewer:** `gpt-5.6-sol`, fast mode
**Branch:** `staging/beta1/trailer-tail`
**Worktree:** `/Users/allen/git/oxidex-beta1-trailer-tail`
**Target:** `/Users/allen/git/oxidex-beta1-targets/trailer-tail`
**Commit:** `feat: parse remaining metadata trailers`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-terra - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/16-trailer-tail.md`

**Files:**

- Modify/create Samsung/MediaJukebox/Vivo parser modules and dispatch only
- Modify family tests/fixtures
- Add: `tests/trailer_tail_forward_port.rs`
- Modify: `tools/exiftool-tables/runtime_ownership.d/trailer-tail.json`
- Do not modify shared engines, tag comparison harnesses, or unrelated parsers

- [ ] **Step 1: Re-measure trailer detection and rows**

Confirm Samsung embedded audio name/data, MediaJukebox fields, and Vivo JSONInfo
on exact carriers.

- [ ] **Step 2: Write failing detection, bounds, and malformed-trailer tests**

Include truncated length, invalid UTF-8, absent terminator, and false-positive
controls.

- [ ] **Step 3: Implement bounded source-faithful parsing and dispatch**

Preserve occurrence order and groups. Do not accept a parser that only emits
identity tags.

- [ ] **Step 4: Run family tests, fmt, Clippy, and commit the candidate**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/trailer-tail \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/trailer-tail/focused.log -- \
  cargo test --test trailer_tail_forward_port -- --include-ignored --nocapture
```

Run the non-measurement portion of the Standard Candidate Acceptance Commands,
create the signed task commit, and require a clean worktree.

- [ ] **Step 5: Run exclusive measurements, update handoff, and report**

Run both authenticated measurement blocks with the literal trailer task
values. Require zero lost reads and zero new VALUE rows.

---

### Task 17: Consolidate Walker Conversion Stages

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/17-walker-engine-consolidation.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/walker-engine-consolidation`
**Worktree:** `/Users/allen/git/oxidex-beta1-walker-engine-consolidation`
**Target:** `/Users/allen/git/oxidex-beta1-targets/walker-engine-consolidation`
**Commit:** `refactor: share metadata conversion across walkers`

**Files:**

- Modify: `src/exiftool_tables/engine.rs`
- Modify: `src/exiftool_tables/ifd_engine.rs`
- Modify: `src/exiftool_tables/keyed_engine.rs`
- Modify: `src/exiftool_tables/serial_engine.rs`
- Create focused shared pipeline module if needed
- Modify adapter call sites named in the walker inventory only after their
  owning vendor task has integrated; never edit a live Task 12-16 worktree
- Create: `docs/reference/generated-runtime-walker-inventory.json`
- Add: `tests/generated_runtime_walker_contract.rs`
- Do not delete compatibility code in this task

**Interfaces:** Acquisition adapters produce a stable field identity, stored
value, groups/provenance, and session reference; one shared pipeline owns
condition and conversion stages.

- [ ] **Step 1: Inventory walkers, duplicated stages, and adapter contracts**

Record every enabled hand walker with its ExifTool source callback, acquisition
kind, supported releases, current conversion path, target shared entry point,
real carrier fixture, and detected-versus-parsed status. Then write adapter
contract tests.

Cover IFD numeric key, keyed name, serial index, binary bytes, list, group,
unknown, request filtering, and an encrypted walker. Each fixture must produce
the same occurrence shape through the shared contract.

- [ ] **Step 2: Run focused tests red against a shared entry point**

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/walker-engine-consolidation \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/walker-engine-consolidation/red.log -- \
  cargo test --test generated_runtime_walker_contract -- --nocapture
```

Expected: shared entry point is absent and engines still duplicate stages.

- [ ] **Step 3: Extract the smallest shared pipeline**

Do not force unlike byte acquisition into one decoder. Preserve keyed/serial
identity without coercing it to an IFD ID.

- [ ] **Step 4: Convert engines one at a time**

After each adapter, run its focused tests. Keep each intermediate commit
buildable for review even though integration will squash the task. If a
Task 12-16 vendor branch still owns an adapter call site, record it as queued
in both handoffs and wait for controller integration before editing it.

- [ ] **Step 5: Add route-level detected-versus-parsed controls**

For JPEG/QuickTime, RIFF/audio, PDF/OLE/ZIP, TIFF/MakerNotes, and one keyed and
serial carrier, require real metadata beyond FileType identity tags.

- [ ] **Step 6: Run tests, fmt, Clippy, commit, and measure**

Run the focused contract suite and the Standard Candidate Acceptance Commands
with:

```bash
task_slug=walker-engine-consolidation
task_target=/Users/allen/git/oxidex-beta1-targets/walker-engine-consolidation
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/walker-engine-consolidation
```

Create the signed task commit before any authenticated measurement block and
require a clean worktree.

- [ ] **Step 7: Update handoff and report**

Report the remaining walker-specific conversion code and why it cannot yet use
the shared stage.

---

### Task 18: Delete Proven-Replaced Compatibility Code

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/18-proven-deletion.md`

**Worker:** Desktop subagent, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/proven-deletion`
**Worktree:** `/Users/allen/git/oxidex-beta1-proven-deletion`
**Target:** `/Users/allen/git/oxidex-beta1-targets/proven-deletion`
**Commit:** `refactor: remove metadata paths replaced by generated runtime`

**Files:**

- Modify: `src/core/exiftool_compat.rs`
- Modify: residual arrays and replay/drain/yield code in Exif directory helpers
- Modify: duplicate tag/enum/lens/conversion maps proven replaced
- Modify: engine code only to remove now-unused duplicate stages
- Create: `docs/reference/generated-runtime-deletion-ledger.json`
- Create: `tools/exiftool-tables/runtime_deletion_ledger.py`
- Create: `tools/exiftool-tables/test_runtime_deletion_ledger.py`
- Add: `tests/generated_runtime_deletion_controls.rs`
- Add deletion-verifier CI/just recipe

- [ ] **Step 1: Generate the deletion candidate ledger**

Each entry contains:

```json
{
  "old_symbol": "path::symbol",
  "source_fields": ["Exif::Main:0x9400"],
  "new_owner": "generated or named residual symbol",
  "oracle_receipt": "/absolute/durable/receipt.json",
  "attribution_receipt": "/absolute/durable/receipt.json",
  "generated_on": "matched",
  "generated_off": "missing-or-residual",
  "deletion_commit": null
}
```

- [ ] **Step 2: Write failing verifier and generated-off tests**

Reject deletion entries without both receipts, stale binary hashes, mismatched
source fields, or a still-live duplicate owner. Run:

```bash
uv run python -m unittest tools/exiftool-tables/test_runtime_deletion_ledger.py -v
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/proven-deletion \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/proven-deletion/red.log -- \
  cargo test --test generated_runtime_deletion_controls -- --nocapture
```

- [ ] **Step 3: Delete branches in small reviewed groups**

Order:

1. Exif::Main hand conversion arms;
2. XP/AmbientTemperature/Copyright paths now proven generated or residual;
3. `exiftool_compat.rs` post-hoc formatters;
4. replay/drain/double-insert/yield shims;
5. duplicate maps and engine conversion stages.

After each group, run its focused tests and attribution control. Never delete
structural traversal required for pointers, MakerNotes, IPTC, GeoTIFF, PrintIM,
encryption, or container walking.

- [ ] **Step 4: Add the no-new-manual-tag-knowledge gate**

The gate rejects a new hand owner for a source construct supported by the
generator and rejects residual entries whose source identity is now generated.

- [ ] **Step 5: Test, verify, commit, and run attribution**

Use shared locks for tests and exclusive lock for attribution. Run:

```bash
uv run python tools/exiftool-tables/runtime_ownership.py verify --root .
uv run python tools/exiftool-tables/runtime_deletion_ledger.py verify --root .
```

Then run `cargo test --test generated_runtime_deletion_controls` and the
Standard Candidate Acceptance Commands with:

```bash
task_slug=proven-deletion
task_target=/Users/allen/git/oxidex-beta1-targets/proven-deletion
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/proven-deletion
```

Run the authenticated generated-attribution control/probe from Task 8 against
the committed clean candidate under the exclusive lock. Create that signed
commit after all ordinary tests/formatting/Clippy and before attribution. Every
deletion ledger receipt must name this candidate's source and binary hashes.

- [ ] **Step 6: Update handoff and report**

Report deleted manual rules separately from source-line changes and list every
retained compatibility symbol with its reason.

---

### Task 19: Qualify ExifTool Version Transitions

**PRD:** `/Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/19-version-transition-qualification.md`

**Worker:** Codex CLI, `gpt-5.6-sol`, fast mode
**Reviewer:** `gpt-6-astra`, fast mode
**Branch:** `staging/beta1/version-transition-qualification`
**Worktree:** `/Users/allen/git/oxidex-beta1-version-transition-qualification`
**Target:** `/Users/allen/git/oxidex-beta1-targets/version-transition-qualification`
**Commit:** `test: prove reversible ExifTool version regeneration`

**Launch:**
`codex --yolo exec --enable fast_mode --model gpt-5.6-sol - < /Users/allen/git/oxidex-beta1-functional-integration/.superpowers/fleet/prds/19-version-transition-qualification.md`

**Files:**

- Create: `tools/exiftool-tables/version_transition_qualification.py`
- Create: `tools/exiftool-tables/test_version_transition_qualification.py`
- Create: `tools/exiftool-tables/version_transition_matrix.json`
- Modify version-rehearsal fixtures/configuration and focused tests
- Modify release-aware source facts and write/readback expectations
- Modify upgrade/rehearsal documentation with actual receipt paths
- Do not manually edit generated artifacts or change the release pin permanently

- [ ] **Step 1: Add concrete rehearsal configurations**

Create checked configurations for:

- same-pin 13.59 -> 13.59;
- 11.78 -> 12.64; and
- 12.64 -> 11.78.

Every stage requires native source hash, version/DOCX probes, explicit
read fixtures, mandatory write fixtures/readback, artifact manifest, separate
target directory, and durable result path.

`version_transition_qualification.py` is the single non-promoting entry point:

```bash
python3 tools/exiftool-tables/version_transition_qualification.py \
  --matrix tools/exiftool-tables/version_transition_matrix.json \
  --repository /Users/allen/git/oxidex-beta1-version-transition-qualification \
  --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification
```

It calls the existing planner/executor/stage adapter, refuses an unclean
caller, runs all three matrix rows, and restores/verifies the caller without a
promotion action.

- [ ] **Step 2: Make tests release-aware**

Replace hard-coded 13.59 facts responsible for the historical 113 failures on
11.78 and 79 failures on 12.64 with pinned per-release facts. Do not weaken a
generic assertion to accommodate drift.

- [ ] **Step 3: Add hand-behavior retention controls**

An older-release run fails if newer hand behavior silently retains tags absent
from that release. Generated refusal is acceptable only when explicit and
counted.

- [ ] **Step 4: Commit a clean candidate and run same-pin exclusively**

Require both variants freshly generated, second regeneration clean, identical
pin/artifact manifest, and no tracked caller diff. Invoke the qualification
entry point through the exclusive lock:

```bash
git add tools/exiftool-tables/version_transition_qualification.py \
  tools/exiftool-tables/test_version_transition_qualification.py \
  tools/exiftool-tables/version_transition_matrix.json \
  tools/exiftool-tables/version_rehearsal*.py \
  tools/exiftool-tables/test_version_rehearsal*.py \
  docs/reference/upgrade-rehearsal-11.78-12.64.md
git commit -S -m "test: prove reversible ExifTool version regeneration"
test -z "$(git status --short)"
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification/same-pin.lock.log -- \
  python3 tools/exiftool-tables/version_transition_qualification.py \
    --matrix tools/exiftool-tables/version_transition_matrix.json \
    --repository /Users/allen/git/oxidex-beta1-version-transition-qualification \
    --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification \
    --only same-pin-13.59
```

If the worktree is not clean after the signed commit, do not start the
rehearsal.

- [ ] **Step 5: Run 11.78 -> 12.64 without intervention**

Require generation, verification, native read conformance, write/readback,
manifest delta, and recovery controls. No code or fixture edit is allowed after
the run starts. Invoke the same entry point with `--only 11.78-to-12.64`.

```bash
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification/forward.lock.log -- \
  python3 tools/exiftool-tables/version_transition_qualification.py \
    --matrix tools/exiftool-tables/version_transition_matrix.json \
    --repository /Users/allen/git/oxidex-beta1-version-transition-qualification \
    --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification \
    --only 11.78-to-12.64
```

- [ ] **Step 6: Run 12.64 -> 11.78 without intervention**

Apply the same gates and prove removed artifacts are handled only by manifest
delta. Invoke the same entry point with `--only 12.64-to-11.78`.

```bash
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification/reverse.lock.log -- \
  python3 tools/exiftool-tables/version_transition_qualification.py \
    --matrix tools/exiftool-tables/version_transition_matrix.json \
    --repository /Users/allen/git/oxidex-beta1-version-transition-qualification \
    --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/version-transition-qualification \
    --only 12.64-to-11.78
```

- [ ] **Step 7: Restore/verify the 13.59 caller and run all rehearsal tests**

```bash
uv run python -m unittest \
  tools/exiftool-tables/test_upgrade_transaction.py \
  tools/exiftool-tables/test_version_rehearsal.py \
  tools/exiftool-tables/test_version_rehearsal_catalog.py \
  tools/exiftool-tables/test_version_rehearsal_executor.py \
  tools/exiftool-tables/test_version_rehearsal_native_oracle.py \
  tools/exiftool-tables/test_version_rehearsal_stage_adapter.py \
  tools/exiftool-tables/test_version_transition_qualification.py -v
git diff --exit-code
test "$(tr -d '\n' < .exiftool-version)" = "13.59"
```

- [ ] **Step 8: Update handoff, commit receipts/documentation, and report**

The source tree must end at its original pin and clean state.

---

### Task 20: Freeze and Qualify the Functional Candidate

**PRD:** Controller-owned; no implementation worker
**Reviewer:** `gpt-6-astra`, fast mode
**Branch/worktree:** `staging/beta1-functional-integration` at
`/Users/allen/git/oxidex-beta1-functional-integration`
**Target:** `/Users/allen/git/oxidex-beta1-targets/final`
**Commit:** `docs: record beta functional qualification evidence`

**Files:**

- Modify only after gates pass: `TODO_RELEASE_BETA.md`
- Modify: autogeneration/upgrade documentation and ExifTool parity skill to
  match implemented commands and facts
- Add/update machine-readable public measurements only from authenticated
  receipts
- No runtime writer may change the frozen candidate during this task

- [ ] **Step 1: Freeze and record the candidate**

Record full SHA, clean state, branch, binary path/hash, pin, pinned source hash,
Perl hash/version, corpus roots/counts, and lock status. Stop all implementation
workers.

- [ ] **Step 2: Run clean full regeneration twice**

```bash
mkdir -p /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final
test -z "$(git status --short)"
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final/regen-1.log -- \
  tools/exiftool-tables/regen-all.sh
python3 tools/exiftool-tables/artifacts.py diff --tier all
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final/regen-2.log -- \
  tools/exiftool-tables/regen-all.sh
python3 tools/exiftool-tables/artifacts.py diff --tier all
test -z "$(git status --short)"
```

Both diffs and the final status must be empty.

- [ ] **Step 3: Run formatting, lint, and complete tests**

Run the repository's CI recipe, the complete ignored suite, and doctests under
the shared lock. Store separate logs and exit statuses:

```bash
cargo fmt --check
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final/ci.log -- \
  just ci
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final/ignored.log -- \
  cargo test --release --workspace --all-features -- --include-ignored
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final/doc.log -- \
  cargo test --doc --workspace --all-features
```

- [ ] **Step 4: Run typed-value and ownership gates**

Run the normal/`-n` oracle matrix, runtime ownership verifier, no-new-manual
knowledge gate, and deletion ledger verifier:

```bash
CARGO_TARGET_DIR=/Users/allen/git/oxidex-beta1-targets/final \
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final/typed-values.log -- \
  cargo test --test typed_value_projection_tests
uv run python tools/exiftool-tables/runtime_ownership.py verify --root .
uv run python tools/exiftool-tables/runtime_deletion_ledger.py verify --root .
```

The Task 18 `just` recipe for no-new-manual knowledge also runs here; its exact
recipe name is `verify-runtime-deletions`.

- [ ] **Step 5: Run combined occurrence conformance exclusively**

Run the Standard Candidate Acceptance conformance block with:

```bash
task_slug=final
task_target=/Users/allen/git/oxidex-beta1-targets/final
task_evidence=/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final
```

Record matched, MISSING, VALUE, EXTRA, RENAME, ceiling, files, and raw oracle
occurrences. Require at least 4,000 files and 400,000 oracle tags, plus the raw
occurrence floor recorded by Task 6.

- [ ] **Step 6: Run read regression and generated attribution exclusively**

Run the Standard Candidate Acceptance read-receipt block using the final values
above, then:

```bash
python3 ~/oxidex-ops/evidence/20260917-group1-batch2/locked.py \
  /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final/genshare.log -- \
  tools/exiftool-tables/genshare/census.sh \
    --repository /Users/allen/git/oxidex-beta1-functional-integration \
    --target-dir /Users/allen/git/oxidex-beta1-targets/final \
    --output /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/final/genshare \
    --corpus /tmp/oxidex-exiftool-cache/combined-samples \
    --perl /tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2 \
    --exiftool-dir /tmp/oxidex-exiftool-cache/exiftool \
    --tokens engine,legacy-l1,legacy-l2,producers,serial,keyed
```

Require read-regression `lost 0`, no public failures, authenticated generated
control/probe receipts, and reconciled per-occurrence deltas.

- [ ] **Step 7: Re-verify all version-transition receipts**

Validate same-pin, forward, reverse, native read, and native write/readback
receipts against the frozen code/tool hashes. If `TODO_RELEASE_BETA.md` names
an intended ExifTool pin after 13.59, run that current-to-next matrix row with
the same Task 19 entry point; otherwise record `not selected` rather than
inventing or silently skipping a version.

- [ ] **Step 8: Refresh main divergence**

Fetch without editing protected branches. Classify every remaining case that
`main` matches and the candidate does not as ported, superseded, intentionally
removed, or unresolved. Any unresolved user-visible regression blocks the
functional program.

- [ ] **Step 9: Update release ledger and documentation**

Record exact receipts and results in `TODO_RELEASE_BETA.md`. Update
`docs/AUTOGENERATION-PLAN.md`, `docs/AUTOGENERATION-V2-DESIGN.md`, rehearsal
documentation, and `.agents/skills/exiftool-parity/SKILL.md` so commands and
claims match reality.

- [ ] **Step 10: Commit the evidence-only update**

```bash
git add TODO_RELEASE_BETA.md docs .agents/skills/exiftool-parity/SKILL.md
git commit -S -m "docs: record beta functional qualification evidence"
```

- [ ] **Step 11: Dispatch final whole-branch review**

Build a review package from the integration merge base through the committed
candidate HEAD. Give the Astra reviewer the spec, plan, controller ledger,
deferred minors, parked rulings, receipts index, documentation updates, and
full diff. One consolidated fix worker and one scoped re-review are allowed if
findings remain. Any runtime fix invalidates Steps 1-8 and requires a complete
rerun; a documentation-only fix reruns formatting, typos, links, and the
scoped review.

The result is ready for the separate `main` reconciliation, packaging, CI/CD,
signing, notarization, and tagging portions of `TODO_RELEASE_BETA.md`. This plan
does not authorize those actions.

## Execution Handoff

Use the controller model described in the spec:

1. Create the integration worktree and ledger.
2. Dispatch the initial five tasks exactly as listed.
3. Keep at most three Desktop and six CLI workers active.
4. Integrate each reviewed non-overlapping task as it returns.
5. Rebase and re-review overlapping returns.
6. Persist every state transition in the task handoffs and controller ledger.
7. Continue without asking between tasks unless an operation is destructive,
   security-sensitive, externally mutating, or the plan is structurally broken.

The chosen execution method is the user-requested hybrid: Superpowers-reviewed
Desktop subagents plus parallel `codex --yolo exec` workers, with the main
session acting as integration controller.
