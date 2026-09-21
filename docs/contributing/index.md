# Contributing

This page describes how a change reaches `refactor/tag-machinery`, the
branch where the refactor happens. It also covers the checks the change must
pass and the measurement rules every claim must follow. The rules are
committed in the repository root: `AGENTS.md` is the shared policy and
`CLAUDE.md` is only the Claude adapter. This page summarises both. Shared rules
win; the adapter may not override them.

::: info Unreleased: `refactor/tag-machinery`
The refactor branch is far ahead of `main` and of the v1.2.1 release. All
day-to-day work lands on it. What goes to `main` is the maintainer's decision
alone. Never push to `main`, merge into it or rebase onto it.
:::

## Start from the plan

Before picking something up, read the
[autogeneration plan](/AUTOGENERATION-PLAN). It states the goal, the measured
state and the ordered next steps. The mechanism is described in the
[v2 design](/AUTOGENERATION-V2-DESIGN), and the working scoreboard is the
[progress page](/AUTOGENERATION-PROGRESS). The [project status](/status/)
page summarises the measured state for readers outside the project.
<!-- /status/ arrives with #837; see ignoreDeadLinks in config.mts -->

## Set up

```bash
git clone https://github.com/swack-tools/oxidex.git
cd oxidex
git switch refactor/tag-machinery
cargo build --release          # rust-toolchain.toml pins the toolchain (1.97.1)
```

To measure anything against ExifTool you also need:

- **The pinned ExifTool.** `.exiftool-version` names it. The durable local
  cache is `$OXIDEX_OPS_DIR/cache/exiftool/<pin>`, with `OXIDEX_OPS_DIR`
  defaulting to `~/oxidex-ops`. Follow the
  [storage and bootstrap guide](/reference/durable-release-storage).
  Never use an `exiftool` found on `PATH`.
- **The capability-checked Perl 5.38.2 installation.** Set `EXIFTOOL_PERL`
  to the interpreter selected by that guide; require `Archive::Zip` and the
  DOCX probe. A matching version alone is insufficient.
- **`just` and `uv`.** Most instruments are `just` recipes or `uv run`
  Python scripts.

## How a change lands

1. **One change, one worktree, one branch.** Branch
   `staging/<slug>` off the current tip, in its own worktree:

   ```bash
   git fetch origin
   git worktree add -b staging/<slug> ../oxidex-<slug> origin/refactor/tag-machinery
   cd ../oxidex-<slug> && tools/preflight.sh --upstream
   ```

   `tools/preflight.sh` prints the worktree root, the branch and the number
   of uncommitted files. It exits non-zero on a protected branch (`main`,
   `refactor/tag-machinery`), on a dirty tree, or on a stale base. Never edit
   a worktree that someone else is working in.

2. **Re-verify before you implement.** Other sessions land competing fixes
   often. Confirm that the defect still reproduces at the fresh tip, and
   measure its scope with a named instrument. If the tip already contains
   the fix, stop and report the work as superseded.

3. **Verify with the instrument for the change.** Tag work is verified by a
   comparison against the pinned ExifTool across the corpus, not by unit
   tests alone. Report before and after counts with the instrument named,
   for example "MISSING 2 → 0 under `just compare-file`".

4. **Open a PR against `refactor/tag-machinery`.** Before you open it and
   again before it merges, run
   `git fetch origin && git rebase origin/refactor/tag-machinery`. Rebase onto
   the tip, never onto `main`. The PR must name the instrument beside every
   number.

5. **CI, then independent verification.** The change merges, squash only,
   after CI and after the maintainer or coordinator has independently checked
   the central claim. The author's report of that claim is not enough.

Rulesets on `main` and on the tip reject force-pushes and deletions.

## Checks to run locally

```bash
cargo fmt --all --check
cargo clippy --release --all-features -- -D warnings
cargo test --workspace
python3 -m unittest discover -s tools/ci -p 'test_*.py'
uv run scripts/sync_tag_stats.py --check
```

`just ci-standard` runs the same set CI runs: formatting, the cbindgen
header check, clippy, a release build, the tests and the C FFI test.

Two known traps:

- `cargo test --workspace --release` fails even on an unmodified base with
  about 120 bogus `panic strategy` or duplicate-`chrono` errors. The cause is
  an output filename collision after `cargo clippy --all-features`. It is
  never caused by your change. Clear it with
  `cargo clean --release -p chrono -p oxidex`, or use `cargo test --workspace`.
- `cargo test --workspace` skips `#[ignore]`d tests. After a key rename, run
  `--ignored` per target and diff the result against a baseline worktree.

## What CI enforces

`ci.yml` runs on every pull request and on every push to `main` and
`refactor/tag-machinery`.

| Job | What it guarantees |
| --- | --- |
| **Lint & Audit** | `cargo fmt`, clippy with `-D warnings`, and a RustSec audit. It also runs the `tools/ci` unit tests and the corpus-guard checks, the **parity ratchet**, and `sync_tag_stats.py --check`. |
| **Build & Test** | Installs ExifTool from `.exiftool-version` and asserts both its version and `Archive::Zip`. Then it runs the full test suite (nextest, then doc tests), the native-Perl replay tests and the C header check. |
| **Corpus Read Regression Gate** | Builds `oxidex` and takes a read receipt over the pinned ExifTool's `t/images`. It then fails if any catalog entry that the published `catalog-corpus-observed-13.59.json` marks as a matched read is no longer credited. Lost entries are named. An untrusted measurement is *refused* (exit 2), never passed. |
| **Verify Generated Tables** | Regenerates the transcribed tables from the pinned Perl source and fails on drift. It proves the tables against live Perl and checks the catalog join and staleness. `just verify-tables` is the local equivalent. |

Two of these in more detail, and the workflows beside `ci.yml`:

- **Parity ratchet** (in Lint & Audit). `tools/ci/parity_ratchet.py` reads only committed JSON and compares 25 named counts
  in the committed measurement JSON with the floors in
  `tools/ci/parity_floors.json`. Proven reads may only rise and known
  defects may only fall, and denominators must match exactly. Floors move
  only through `parity_ratchet.py raise`.
- **Benchmarks.** `benchmarks.yml` times the release binary against the
  pinned ExifTool on every push to the tip. It is indicative and non-blocking
  (see [Benchmarks](/performance/benchmarks)).
- **Docs.** `docs-build.yml` builds the site from cold on every PR that
  touches `docs/`. See [Docs site](/contributing/docs-site).

## Measurement rules

Every number is a claim about the tool that produced it. The wrong tool fails
silently and confidently, in whichever direction you already expected. The
short form of the rules in `AGENTS.md`:

- **Name the instrument.** Write "MISSING 2 under `just compare-file`", not
  "2 tags missing". A number without its instrument and commit is not
  evidence.
- **Never grade against an unpinned ExifTool.** Use the oracle in
  `scripts/exiftool_oracle.py` (Python) or `src/exiftool_oracle.rs` (Rust),
  never a bare `exiftool`. Its `-ver` must print 13.59 **and** it must pass
  the `OOXML.docx` → `DOCX` capability probe. A perl without `Archive::Zip`
  prints the right version while every container format degrades.
- **Detected is not parsed.** A correct `File:FileType` can sit on top of an
  empty parse. Check `format_dispatch` for the format, or read the MISSING
  count from `just compare-file`.
- **Never approximate a conversion.** A plausible but wrong value under a
  real ExifTool tag name is worse than an absent tag. Omit the value and
  count it.
- **A gap in a transcribed table is not evidence the tag does not exist.**
  The generator omits whatever it cannot model. Diff the table against the
  `%Image::ExifTool::<Module>::<Table>` hash in the pinned source.
- **Tag knowledge is not tag coverage.** The `oxidex-tags-*` definition
  counts come from ExifTool's documentation view. They say a tag exists, not
  that OxiDex can read it. Only a comparison run measures coverage.
- **Trust the header.** Measurement scripts print an `=== instrument ===`
  header naming the binary (with a staleness warning), the commit, whether
  the tree is dirty, the oracle and the corpus. A dirty tree refuses to
  measure unless `OXIDEX_ALLOW_DIRTY_TREE=1` is set.
- **Regenerate baselines yourself.** Build a baseline from the same commit
  you compare against. Do not reuse one you were handed.

## Conventions

- Generated code and generated reports are never edited by hand. Regenerate
  them with their generator. That covers `src/exiftool_tables/`, the
  `oxidex-tags-*` data, the catalog reports under `docs/reference/` and
  `docs/tag-domains/`.
- Historical or hand-captured test data goes in
  `tools/exiftool-tables/testdata/`, never in `tests/fixtures/`. CI
  regenerates the fixtures from the pin and fails the lint job on a
  difference.
- Leave a `HANDOFF.md` (repository root, untracked) at each milestone of a
  long change. It should record the branch and base, what landed, what was
  validated with which instrument, and the exact next command.

## Further reading

- [Measuring coverage](/contributing/measuring-coverage): the conformance instrument and its classes
- [Docs site](/contributing/docs-site): how this site is built, previewed and deployed
- [Release checklist](/contributing/release-checklist)
- [Code quality patterns](/contributing/development/code-quality-patterns) and [TagRegistry refactoring](/contributing/development/tagregistry-refactoring)
- [Testing](/contributing/testing/) and [test failure triage](/contributing/testing/TEST_FAILURE_TRIAGE)
- [Transcription](/TRANSCRIPTION): how ExifTool's tables are transcribed
