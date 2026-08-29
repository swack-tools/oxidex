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
   the failure text blamed a merge conflict. Fix: `tests/_env.py` scrubs
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

Every measurement script under `tools/exiftool-tables/` and
`src/bin/jpeg-tag-matrix/` prints an `=== instrument: <tool> ===` header
before its first number: which oxidex (path, and a staleness warning per
#2 above), which git commit and whether the tree is dirty, which ExifTool
and its capability-probe result, and the corpus path and file count. A dirty
tree refuses to measure at all unless `OXIDEX_ALLOW_DIRTY_TREE=1` is set,
in which case the header says so. See `scripts/instrument.py`'s module
docstring for the full rationale; `src/bin/jpeg-tag-matrix/instrument.rs`
mirrors it for the one harness that isn't Python.

## Architecture
Hexagonal (ports/adapters) with three layers:
- **Application**: CLI, C FFI bindings
- **Domain**: Format-agnostic metadata models
- **Infrastructure**: Format-specific parsers, I/O

## Style
- Run `cargo clippy` before commits
- Use `cargo fmt` for formatting
