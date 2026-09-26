# OxiDex Development Guide

## Overview
Rust implementation of ExifTool - high-performance metadata parsing for 140+ formats.

## Use rust uutils coreutils when you can like ripgrep and LSP's as well as claude-mem
to decrease the amount of time grepping. We also have things like hyperfine.

We have: coreutils bat eza fd ripgrep hyperfine dust bottom tokei procs sd zoxide starship gitui git-delta lsd tealdeer broot bandwhich grex xh just watchexec typos-cli nushell yazi atuin mprocs hurl

## Where you can utilize sccache to speed up builds especially when dealing with multiple worktrees
building the same thing basically

## Commands
```bash
cargo build                    # Build debug
cargo build --release          # Build release
cargo test --workspace         # Run all tests
just test                      # Run tests (CI config)
just check                     # Quick check without build
cargo clippy                   # Lint
just build-bin-release         # Build release binary
```

## Release test-profile collision

Because this crate exposes `lib`, `staticlib`, and `cdylib`, release test
targets (which require `panic=unwind`) and release binaries (which use
`panic=abort`) can race to write the same un-hashed `liboxidex.rlib`. Thus a
bare `cargo test --workspace --release` may report bogus `panic strategy` or
duplicate-`chrono` errors, especially after another release-profile command has
shared the target directory. This is an output filename collision, not proof of
a source regression. Prefer `just test`, which supplies the scoped unwind
override; if reproducing the bare command is necessary, clear the colliding
artifacts first with `cargo clean --release -p chrono -p oxidex`.

## Rust toolchain pin (build gotcha)

`rust-toolchain.toml` pins the compiler (`channel = "1.97.1"`; CI builds with
it), but **only rustup's proxies read that file**. Any other `rustc` earlier on
`PATH` ignores it without a word. On the maintainer's Mac, `/opt/homebrew/bin`
comes before `~/.cargo/bin`, so `rustc` and `cargo` are Homebrew's 1.98.1 and
every local build silently used it. Even rustup's own cargo
(`~/.cargo/bin/cargo`, or `rustup run 1.97.1 cargo build`) compiles with
Homebrew's rustc there, because cargo runs the first `rustc` it finds on
`PATH`. **`rustup run` is not a fix.** Checked: a crate built that way embeds
Homebrew's `/rustc/48a229ce…` std paths.

Fix it once per shell (put it in the profile):

```bash
export PATH="$HOME/.cargo/bin:$PATH"   # rustup proxies first; they honour the pin
```

For a single command, use `PATH="$HOME/.cargo/bin:$PATH" cargo …`. Setting
`$RUSTC` alone is not enough: cargo must be the pinned one too, and
preflight, the corpus receipt and the rehearsal stages all check
`cargo -V`. So if you pin by path, pin both:
`RUSTC="$(rustup which --toolchain 1.97.1 rustc)" "$(rustup which --toolchain 1.97.1 cargo)" …`.
Check with
`tools/preflight.sh`, which exits 6 when `rustc` (or `$RUSTC`) or `cargo`
resolves to anything but the pinned channel. It also exits 6 when that
rustc's `commit-hash` is not the one rustup reports for the pin, or when
rustup cannot resolve the pin at all. `OXIDEX_ALLOW_TOOLCHAIN_SKEW=1`
turns that failure into a printed warning. Use it only for work that builds
nothing you will measure.

To see which compiler built a binary, read its own bytes. What `PATH` resolves
today can differ from what built it:
`grep -aoE '/rustc/[0-9a-f]{40}' target/release/oxidex | sort -u`, compared
with `rustup run 1.97.1 rustc -vV | grep commit-hash`. Every instrument
header does that comparison and warns on a mismatch (see incident 12 below).

**Measurements from 2026-09-23 are off-pin.** Every local build and corpus
measurement on this Mac that day came from a 1.98.1-built binary: each
worktree binary checked embeds `/rustc/48a229ce…`, Homebrew 1.98.1. CI used
the pinned 1.97.1. Before comparing those numbers with CI or with later runs,
rebuild on the pin and measure again.

## Structure
- `src/` - Core library and CLI
- `src/exiftool_tables/` - Binary tag layouts transcribed from ExifTool's Perl tables (generated)
- `oxidex-tags-*` - Tag definition crates (auto-generated from ExifTool)
- `tests/` - Integration tests
- `benches/` - Performance benchmarks
- `bindings/` - C FFI bindings
- `docs/` - Documentation

## Closing an ExifTool coverage gap

**Check whether the answer is already transcribed before writing parser code.**
`src/exiftool_tables::find_table(module, table)` carries ExifTool's real byte
layout — `FORMAT`, `FIRST_ENTRY`, per-field `Format`, `Mask`, `SubDirectory`
edges, and enum `PrintConv` maps. Re-deriving by hand a binary record ExifTool
already declares is the expensive way to close a gap. See `docs/TRANSCRIPTION.md`.

**Tag knowledge is not tag coverage.** `src/tag_sync` ingests `exiftool -f -listx`,
which is the *documentation* view: it carries `count encoding id index lang name
type version writable` and nothing else. It has no `SubDirectory`, `FORMAT`,
`FIRST_ENTRY`, `ValueConv`, `Condition` or `DataMember` — that is, no layout. So
it can tell you a tag exists but never how to read one, and a rising
`oxidex-tags-*` count is **not** evidence of rising extraction coverage. Only a
comparison run measures coverage.

**Measure the gap by kind before costing the work.**
`python3 tools/exiftool-tables/conformance.py <corpus> --exiftool-dir <src> --oxidex <bin>`
classifies each difference as RENAME / MISSING / VALUE / EXTRA and prints a
`ceiling` column. A wide score-to-ceiling spread means free coverage (renames),
not parsing work.

**Never grade against an unpinned ExifTool.** `.exiftool-version` at the repo
root names the release the transcriptions come from, and it is the only source
of truth — the Rust oracle (`src/exiftool_oracle.rs`) compiles it in, the Python
one (`scripts/exiftool_oracle.py`) reads it, and CI and the justfile both fetch
that exact tag. Never invoke a bare `exiftool`: `PATH` resolved to 13.55 while
the tables were transcribed from 13.59, and the two disagree about which
sub-table a given byte count selects, so sixteen correct Canon R6 Mark III tags
were reported as regressions. The failure is symmetric — the same skew
manufactures phantom *fixes* — and neither is distinguishable from the real
thing afterwards.

**A matching `-ver` is not a working oracle.** The pinned tree's `exiftool`
starts `#!/usr/bin/env perl`, which finds a Homebrew perl with no
`Archive::Zip`; ExifTool then reports `FileType: ZIP` for a `.docx` and every
container format degrades at once, *while `-ver` still prints the right
release*. The oracle therefore also probes capability, and any corpus sweep
should assert a file-count and tag-count floor — a degraded run does not crash,
it reports a confident, precisely-formatted, completely wrong number.

**Never approximate a conversion.** A plausible-but-wrong value under a real
ExifTool tag name is worse than an absent tag: it does not crash, and nothing
downstream can tell. Omit and count it instead — that is the rule the generator
follows, and `just verify-tables` (also a CI job) enforces it for the generated
tables against an independent oracle.

**A gap in a transcribed table is not evidence the tag does not exist.** The
generator obeys the rule above, so a field it cannot model is simply absent —
its silence means "not transcribed", never "not a tag". `AIFF::Common` carries
NumChannels, NumSampleFrames, SampleSize and CompressionType but no
`SampleRate`, because SampleRate is an 80-bit IEEE `extended` and the generator
will not guess at one. Reading the table alone concludes AIFF has no sample
rate; ExifTool reports 22050. When a table looks short, diff it against the
`%Image::ExifTool::<Module>::<Table>` hash in the pinned tree — what is missing
there is a pointer to hand-implement against the Perl, with a test pinning the
decode, not a stop sign.

**Detected is not parsed.** A format can produce a perfectly correct
`File:FileType`, `FileTypeExtension` and `MIMEType` and still extract nothing:
`read_metadata` falls back to `add_identity_tags` for the ~40 formats with no
parser, which emits those three and the filesystem tags and returns success.
`oxidex -j` on such a file looks healthy while 100% of its real tags are
missing, and no error is raised anywhere. Six formats sat in exactly that state
— AIFF was 21 missing tags behind a correct `FileType: AIFF`. Grep
`format_dispatch` for the variant before assuming a format is covered, or run
`just compare-file` on one of its samples and read the MISSING count.

**Name the instrument, or the measurement is not evidence.** Every number here
is a claim about the tool that produced it, and the wrong tool fails silently
and confidently in whichever direction you were already leaning. Three in one
afternoon: bare `git apply --check` rejected all 14 truncated diffs while
`git_apply_with_rung` — which normalizes headers and passes `--recount` on
every rung — accepted 7 of them, so "they can never apply" was exactly backwards;
`cargo test --lib <filter>` matched a neighbouring test and passed green while
the full suite would have failed; and a bare-name comparison scored
`AIFF:Comment` against `ID3:Comment` as a defect when both tools emit both. So
state the instrument alongside the number in commits, PRs and review comments —
"MISSING 2 under `just compare-file`" rather than "2 tags missing" — and when a
result argues *against* adding a safety check, re-run it with the tool the
harness itself uses before believing it.

Five more, one session, each a proxy standing in for the thing being
measured, made mechanical below rather than left as a story. Five more again
followed, another session, same shape — the list keeps growing because the
instrument keeps lying in a new way, not because the old ways stopped:

1. **Implicit binary resolution.** `jpeg-tag-matrix` resolved oxidex from
   `$repo/target/release/oxidex` by convention while `CARGO_TARGET_DIR` was
   redirected. The path never existed; every subprocess spawn failed closed,
   and the run reported `readable 2702 -> 0` on nine consecutive gate runs
   before anyone checked which binary ran. Fix: `resolve_binary()`
   (`scripts/instrument.py`, `src/bin/jpeg-tag-matrix/instrument.rs`) exits
   loudly the moment a resolved path is not a file, before any subprocess
   call — never silently proceed with a binary that doesn't exist.
2. **A stale prebuilt binary.** A duplicate-loss scan graded a conveniently
   already-built binary from an old commit, on top of an already-dirty tree.
   Nothing said so; a bisect agent went looking for a regression that could
   not exist. Fix: `staleness_note()` compares the binary's mtime against
   HEAD's commit time and every dirty file's mtime, and the header warns when
   the binary predates the source it should reflect.
3. **A filter that ate the answer.** `grep -E "^TOTAL|files|match"` on
   `conformance.py` output matched per-format rows containing "files" and
   dropped the `TOTAL` line the run existed to produce. No general fix here
   beyond the obvious: anchor greps (`^TOTAL\b`), or better, read
   `--json-out` instead of grepping formatted text.
4. **A stale supplied baseline.** Three separate agents credited a branch's
   own pre-existing drift to their own change because the baseline they
   diffed against was old. No tool catches this by itself; regenerate the
   baseline from the same commit you are comparing against, don't reuse one
   handed to you.
5. **`&&` after a pipe tests the wrong command.**
   `git push ... 2>&1 | tail -1 && echo "preserved"` printed "preserved"
   on five consecutive failed pushes — `&&` sees `tail`'s exit status, which
   is almost always 0, never `git push`'s. Check `${PIPESTATUS[0]}` (bash) or
   avoid the pipe.
6. **A global start-method flip masqueraded as a hang.** A test module's
   import-time `multiprocessing.set_start_method("fork", force=True)` flipped
   the whole suite process to fork; under the server fixture the thread-heavy
   parent then deadlocked in a semaphore cloned while held, hanging the suite
   past the gate's wall-clock budget. A green suite and a hung suite were the
   same code — the gate result depended on which module got imported first.
   Fix: no global start-method flips; every pool passes its own `mp_context`;
   a test asserts that importing the suite does not change
   `multiprocessing.get_start_method()`.
7. **Hermetic tests inherited the ambient environment.** Fixtures read
   `FLEET_HUB_URL` et al. from the real environment instead of their temp
   repos — green on the developer's laptop, red only on the gate host, and
   the failure text blamed a merge conflict. Fix: `tools/fleet/tests/_env.py` scrubs
   `FLEET_*`/`KEEL_*`/`EXIFTOOL_CACHE_DIR`/`GIT_SSH_COMMAND` for every
   fixture, plus a fence test that runs the suite with hostile values
   exported and requires green.
8. **A macOS-only green suite hid two Linux-deterministic bugs.** GNU vs BSD
   `stat` argument order, and `/bin/sh` being dash, not bash. The suite had
   never run on Linux because the gate stage that runs it did not exist yet.
   Fix: dual-platform before "green" means anything; spell `bash` explicitly
   when using bash syntax.
9. **A `ps` column that lies on one platform.** `ps -o sess=` prints 0 for
   every process on macOS (a masked kernel address), so a session-based
   process check silently no-ops there while working on Linux. Fix: use the
   `getsid(2)` syscall; never trust a `ps` column not verified on both
   platforms.
10. **A mangled refspec produced a confident, wrong diagnosis.**
    `"$SHA:refs/..."` in zsh applies the `:r` modifier and mangles the
    refspec, which produced a confident "the credential lacks write
    permission" — the credential was fine. A second attempt failed because
    the probe ran in a repo that did not contain the object being pushed. Two
    broken instruments in a row, both wrong in the direction already
    expected. Fix: brace refspec variables (`"${SHA}:refs/heads/x"`); and
    when a probe reports a permission failure, first prove the probe itself
    is well-formed against a known-good case.
11. **Two spellings of one identity, and an acceptance check that only
    asserted the value existed.** On the i7, `fleetd` recorded
    `platform_id b2bdf493…` while the gate it had itself just spawned
    wrote its verdict under `b6613b19…` — same host, same minute, same
    rustc. `platform_id` is a third of the verdict cache key, so the
    component that PAYS for a gate could not read the result of the gate
    it started: `verdict.lookup` missed, `classify_branch` never returned
    AWAITING_TRAIN, and the host re-gated the identical merge tree every
    ~21 minutes forever while a correct PASS sat unread. The formula was
    spelled three times (`gate.sh`, `claim.py`, `verdict.py`) and no two
    agreed on both fields; the difference was one trailing newline —
    `$(rustc -vV)` strips it, `subprocess.run().stdout` keeps it — not
    which compiler. It survived because the acceptance bullet checked
    that `git ls-remote 'refs/fleet/verdicts/*'` listed **a**
    `platform_id`, which was true throughout: the gate's key was
    perfectly well formed. **An assertion that a value exists cannot
    catch two components disagreeing about the value; only an assertion
    that the two sides AGREE can.** Fix: one resolver
    (`tools/fleet/toolchain.py`, carried into shell by
    `units/fleet-toolchain.sh`, which `gate.sh` sources, so there is one
    implementation rather than a reference one and some copies), the
    runner refuses to start when its own `platform_id` differs from the
    one its gate command computes, and
    `tools/fleet/tests/test_toolchain_seam.py` drives the real `gate.sh`
    lines against the Python side rather than re-spelling the formula in
    the test — a test that spells the formula itself proves only that its
    author repeated the mistake.

12. **An implicit toolchain resolution.** `rust-toolchain.toml` said
    1.97.1, but a Homebrew `rustc` ahead of rustup's proxies on `PATH`
    ignored it. Every local binary and corpus measurement on 2026-09-23 came
    from 1.98.1 while CI used the pin. Nothing reported it, because nothing
    asked which compiler ran. Fix: `tools/preflight.sh` fails (exit 6) on a
    `rustc`/`cargo` that is not the pinned channel, or on a rustc whose
    commit is not rustup's pin. Every instrument header
    names the compiler that built the binary under test. It reads the
    `/rustc/<commit>/` std paths embedded in the binary, because the
    compiler on `PATH` today may not be the one that built a prebuilt binary,
    and it warns loudly on a mismatch. `corpus_read_receipt.py build` passes
    the pinned rustc to Cargo as `$RUSTC`, records it, and refuses anything
    else. Two version-rehearsal stages compile: the build and the release
    test suite. Each re-proves its compiler with its own environment right
    before Cargo runs and records it. Each refuses any compiler other than
    the one the built checkout's own `rust-toolchain.toml` pins, and the
    tests must use the build's exact rustc. The qualification replays both.
    The build is also checked against the binaries' fingerprints. The pin's
    identity comes only from rustup (`rustup which`/`rustup run`), never from
    a PATH compiler that merely reports the same release. A matching
    release string is not an identity: preflight, the corpus receipt build and both
    rehearsal stages require the running rustc's `commit-hash` to equal the
    commit rustup reports for the pin. They refuse, failing closed, when
    rustup cannot resolve the pin. Instrument headers report `unverified`
    in that case instead. See "Rust toolchain pin" above.

Every measurement script under `tools/exiftool-tables/` and
`src/bin/jpeg-tag-matrix/` prints an `=== instrument: <tool> ===` header
before its first number: which oxidex (path, and a staleness warning per
#2 above), which rustc compiled that binary (by its embedded fingerprint)
against the pin, per #12, which git commit and whether the tree is dirty, which ExifTool
and its capability-probe result, and the corpus path and file count. A dirty
tree refuses to measure at all unless `OXIDEX_ALLOW_DIRTY_TREE=1` is set,
in which case the header says so. See `scripts/instrument.py`'s module
docstring for the full rationale; `src/bin/jpeg-tag-matrix/instrument.rs`
mirrors it for the one harness that isn't Python.

## Release engineering

Release work has three ordered, receipt-producing owners: use
`exiftool-parity` first, `oxidex-release-documentation` second, and
`oxidex-release-finalization` last. Ordinary development never touches
`main`; only an explicitly maintainer-authorized release promotion may use a
reviewed PR whose base is `main`. Authorization to prepare or merge that PR
does not authorize the real release tag: pushing the signed tag requires
separate explicit maintainer authorization for the exact version, tag, and
`main` commit.

Operational paths must be portable: never hardcode a user's absolute home.
Use `OXIDEX_OPS_DIR`, defaulting to `$HOME/oxidex-ops` (`Path.home()` in Python),
through `scripts/ops_paths.py`. Keep durable evidence, caches, and configuration
below that root. Fleet worktrees retain `OXIDEX_WORKTREE_ROOT` (default
`$HOME/git`) and targets retain `OXIDEX_TARGET_ROOT` (default
`$OXIDEX_WORKTREE_ROOT/oxidex-beta1-targets`) for existing-ledger compatibility.
Never put durable state or worktrees in `/tmp`, `/private/tmp`, or other
system temporary storage. Hosted CI scratch caches are a separate,
explicit channel and cannot serve as maintainer release-qualification evidence.

## Before the first edit, and before the first remote command

The rules above are about trusting a *measurement*. These are about trusting the
*place you are working* — the same failure one layer down, and the cheapest of
them to check. `tools/preflight.sh` performs the mechanical half; run it first.

**Know which checkout you are in.** `tools/preflight.sh` prints the worktree
root, whether it is the main checkout or a linked worktree, the branch, and the
uncommitted-file count, and it exits non-zero on a protected branch (`main`,
`refactor/tag-machinery`), on a dirty tree, or when `rustc`/`cargo`
resolves to a compiler other than `rust-toolchain.toml`'s (exit 6; see "Rust
toolchain pin"). Never edit the main checkout while
operating from a worktree, and never edit a worktree another agent owns: several
agents sharing one tree is not hypothetical here — a live acceptance run found
its tree gone dirty 58 s in, from a sibling's staged edits, and everything
measured after that point was measuring an unknown tree. One agent, one
worktree, one branch:
`git -C <repo> worktree add -b <branch> "$OXIDEX_OPS_DIR/worktrees/<dir>" <base>`.

**Start from a base you have just verified, not one you were handed.** Before
implementing: `tools/preflight.sh --upstream` (fetches origin and reports how far
behind the base is), then re-confirm the defect still reproduces *at that HEAD*
and measure its scope with a named instrument. Re-check before opening a PR. If
upstream already contains the fix, stop and report it superseded — this is
incident 4 above ("a stale supplied baseline") in its other form: there, an old
baseline manufactured credit for someone else's work; here, a stale base
manufactures work that no longer exists.

**Leave a handoff at every milestone.** `HANDOFF.md` (repo root, untracked) is
the one place a successor — or you, after a context reset — reads to resume:
branch and base SHA, PR/CI state, what landed, what is still broken, what was
validated with which instrument, and the exact next command. A session that ends
without it has to be re-derived from git log and guesswork; sessions here have
been interrupted mid-wave by outages, restarts and quota limits often enough
that this is the difference between resuming and restarting.

**Verify reach before remote work, and plan before destroying.**
`tools/preflight.sh --host <h> --github --k8s` proves ssh reachability, GitHub
identity and the current kube context *before* a command depends on them. For
anything destructive — deleting refs, resources, or state; rewriting shared
history; reinstalling units — produce a dry-run plan plus the dependency and
reference checks first, and wait for the maintainer's approval. Dry-run-by-
default is already the pattern in `tools/fleet/rollout/` (`install_hook.sh`,
`seed_desired.py`, `rulesets.py` all require an explicit `--execute` and refuse
when a precondition is missing); match it rather than inventing a new shape.

**Config that a container owns must not be edited under it.** Stop the
container, edit, restart, then confirm the change actually persisted — a running
container may rewrite or simply outlive the edit, and the edit that vanished on
restart looks exactly like the edit that was never made. Never delete a
Kubernetes identity or RBAC object before listing the workloads that reference
it. Reject any manifest still carrying a placeholder: a template applied
verbatim fails in whichever direction is hardest to see.

**Long runs must survive being interrupted.** Corpus sweeps, fleet checks and
CI/PR polling: persist state to a file as you go, cap parallelism (this laptop
has 10 cores; more than about two concurrent heavy waves degrades the timing-
sensitive measurements everything else depends on, and a starved measurement is
a corrupted instrument), isolate every worker in its own explicit worktree, and
report a blocked item as blocked instead of retrying it forever. A 45-minute
poll loop that could never exit, and a watcher that died with its ssh
connection, are both in this repo's history.

**Delete build directories you will not need again.** A separate
`CARGO_TARGET_DIR` per worktree keeps parallel builds from colliding, but each
one grows to 20–50 GB, and a single release push once left about 1 TB of them
behind. Build output can always be regenerated, so delete a target directory
once you are confident nothing will use it again:
- its worktree is removed;
- its branch has merged or been closed;
- the one-off measurement it served has been recorded, together with the
  binary's fingerprint and sha256.

Keep it while its binary is still evidence someone may re-check, or while a
live agent or an open PR still builds there. Delete only directories you
created, by exact path, never by glob across other agents' directories.

A scratch clone or bundle is not build output: it can hold the only copy of
a commit graph. Delete one only after proving that every ref it carries is
reachable from `origin`. For a clone, run
`git rev-list --all --not --remotes=origin` in the clone. For a bundle, list
its heads with `git bundle list-heads` and check each one with
`git merge-base --is-ancestor <sha> origin/<branch>`, or check that
`git branch -r --contains <sha>` names a remote branch. Anything unreachable
must be pushed or archived first.

A worktree is removable only when all of these hold:
- it has no uncommitted or untracked files (`git status --porcelain` is empty);
- it has no commits missing from `origin`
  (`git rev-list HEAD --not --remotes=origin` prints nothing);
- its ignored files hold nothing worth keeping. `git worktree remove` deletes
  ignored files without asking, and `git status` does not show them, so list
  them with `git status --porcelain --ignored`. `HANDOFF.md` is ignored, and it
  is the resume record, so copy it and any ignored evidence to
  `~/oxidex-ops/evidence/<run>/` before removing the worktree, unless the
  maintainer has said it is disposable.

Use `git worktree remove`, never `rm -rf`.

**The stash is shared by every worktree.** `refs/stash` belongs to the
repository, not to a worktree. A bare `git stash pop` or `git stash apply`
therefore takes whatever was stashed last in any worktree, possibly another
agent's work-in-progress from a different branch. It has already happened
here once. Git kept the other stash only because the pop conflicted. Prefer a
WIP commit on your own branch, or a patch file in your evidence directory,
over the stash. If you must stash, give it a message naming your branch
(`git stash push -m "<branch>: <why>"`), and record its object ID at once
(`git rev-parse stash@{0}` right after the push). To apply it later, use that
recorded ID: `git stash apply <sha>`. `stash@{N}` is a reflog position, and
another agent's push renumbers it between your `git stash list` and your
`apply`. If you didn't record the ID, find the entry with `git stash list`,
resolve it with `git rev-parse stash@{N}`, re-read its message with
`git log -1 --format=%s <sha>`, and only then apply that ID. Never run a bare
`pop`, `apply` or `drop`, never apply by position, and never `git stash clear`.

## How work lands

Ordinary development reaches `refactor/tag-machinery` through reviewed PRs,
one change per PR:

1. One agent, one worktree, one `staging/<slug>` branch off the current tip
   of `refactor/tag-machinery`. A stacked child is the exception: branch it
   from its parent's current head (see "Stacking dependent PRs"). Run
   `tools/preflight.sh --upstream` first.
2. Verify with the instrument named for the change (a corpus comparison for
   tag work; see "Closing an ExifTool coverage gap"). A change that touches
   readers must show 0 proven reads lost (`tools/ci/read_regression_gate.py`,
   which CI also runs on every PR).
3. Open the PR against `refactor/tag-machinery`, name the instrument beside
   every number, and let CI run: build and tests, lint, generated-table
   verification, the corpus read-regression gate and the parity ratchet.
4. Squash-merge only when CI is green, no review thread is unresolved, and the
   PR's central claim has been verified independently of the agent that made
   it: re-run its instrument yourself, don't trust its report. The maintainer
   may waive the CI wait explicitly, and only for the PR they name.

**`main` is the maintainer's decision alone.** During ordinary development,
never push to it, merge into it, or rebase onto it. Release promotion is the
only path to `main` (see "Release engineering"). `refactor/tag-machinery` and
`main` carry rulesets that reject force-pushes and deletions.

**Keep the branch on the current tip.** Other sessions land competing fixes
often. Before starting, and again before merging, fetch and bring the branch
up to `origin/refactor/tag-machinery` (not `origin/main`). Rebase only before
the first push; once a branch is pushed, merge the tip with a signed merge,
because a pushed branch must not be force-pushed. To decide whether upstream
already fixed the defect, reproduce it against the fetched
`origin/refactor/tag-machinery` itself (a clean base worktree or a binary
built from it), never against your own branch, which contains your fix. Then
verify the corrected behaviour separately on your branch's head. If the tip
already contains the fix, or the branch's scope has shrunk to nearly nothing,
report it superseded and stop rather than landing an empty change.

## Stacking dependent PRs

Stack instead of queueing when review loops pile up or too many PRs are open
at once, and in particular when a branch depends on a PR that has not landed.
Open the dependent PR now, with the parent's `staging/...` branch as its base,
rather than parking finished work until the parent merges. CI and the PR
reviewer then see only the child's own diff and start immediately; a queued
branch instead waits out every one of the parent's review rounds and then
takes one large conflict at the end.

- **Keep children current.** Each time the parent's head moves, merge it into
  every child with a signed merge (`git merge -S --no-ff origin/<parent>`), not
  a rebase — a pushed branch must not be force-pushed. Resolve conflicts in
  favour of the parent's structure.
- **Land only on the integration branch.** Never merge a child into its parent
  branch. After the parent squash-merges into `refactor/tag-machinery`:
  1. retarget the child first (`gh pr edit <n> --base refactor/tag-machinery`);
  2. then merge the new tip into it and push.

  The order matters. Retargeting is a PR `edited` event, which CI's
  `pull_request` trigger does not subscribe to, so only the push that follows
  starts a CI run against the new base. If the tip merge was already pushed
  before retargeting, re-run CI explicitly. Then squash-merge under the rules
  in "How work lands", which apply unchanged to every PR in the stack.
- **Keep unapproved work out of any automatic integration queue.** The
  multi-host fleet is stopped. Its train, however, treats every unclaimed,
  non-withdrawn `staging/*` ref as a landing candidate
  (`tools/fleet/workqueue.py`), and it squash-commits and pushes those refs
  straight onto the integration tip (`tools/fleet/train.py`). It does this
  whatever the ref's PR base, review state or CI status. If the fleet ever
  runs again, it would therefore bypass the landing rules above for every PR,
  not only for stacked children. Before restarting it, give the train an
  explicit readiness filter (CI green, approved, no unresolved threads,
  parent landed). Until then, withdraw every not-yet-approved branch from the
  queue.
- **Prefer a stack to a roll-up.** One combined PR means a larger diff for every
  review round, one defect blocking all of it, and no way to verify each
  change's central claim on its own.
- **Record the stack.** List the parent/child chain in `HANDOFF.md` and in each
  child's PR body, so a successor knows the retarget order.

## Architecture
Hexagonal (ports/adapters) with three layers:
- **Application**: CLI, C FFI bindings
- **Domain**: Format-agnostic metadata models
- **Infrastructure**: Format-specific parsers, I/O

## Style
- Run `cargo clippy` before commits
- Use `cargo fmt` for formatting
