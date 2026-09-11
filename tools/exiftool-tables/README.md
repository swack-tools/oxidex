# ExifTool table generation and verification

For completed work, known gaps and priorities, start with
[Tag machinery status](../../docs/TAG_MACHINERY_STATUS.md). This file describes
the tooling landed through [PR #740](https://github.com/swack-tools/oxidex/pull/740)
at [`2cea1e41`](https://github.com/swack-tools/oxidex/commit/2cea1e4194ce7fc1aeed4b110d77ad436ccd7837);
the pin remains 13.59.

Conservative IFD-aware upgrade classification, one generated-output inventory,
verified Canon CODE references, isolated upgrade orchestration and all 28 declared
outputs are implemented and validated. See the
[execution plan](../../docs/UPGRADE-NEXT-STEPS.md) for original validation evidence
and the remaining work. The
[13.55-to-13.59 retrospective rehearsal](../../docs/reference/bump-reports/13.55-to-13.59.md)
passed at `4fb705da` on 2026-09-11: both variants regenerated all 28 outputs and
built fresh binaries, with no source-edit intervention and unchanged caller
source/index/pin. Its 193-file A/B uses current handwritten runtime on both
sides; classifier AUTO/HAND percentages do not measure upgrade coding effort.

## Commands and scope

```sh
just regen-tables          # primary generation, including binary and IFD tables
just verify-tables         # independent checks of committed table declarations
just regen-tables-all      # primary generation plus all declared downstream tables
just regen-tables-tier2    # downstream generation only
```

Run generation in an owned worktree with the intended pin and a capability-probed
Perl/ExifTool pair. Generation modifies tracked artifacts; ordinary measurement
scripts refuse a dirty tree unless the explicit override is provided and recorded.
These direct generation commands modify the caller's artifacts. Use the bump
command below when comparing versions in isolated source trees.

`regen.sh` generates binary and IFD tables, file identification, Composite
definitions and compiled Composite expressions, FITS names, and expression/value-
conversion ledgers. `regen-all.sh` adds vendor subdirectory tables, Nikon AF-point
grids, bespoke transcriptions, recovered Sony/Minolta generators, Macintosh
CJK charset tables, GeoTIFF key maps, DICOM dictionaries and lens alternatives.
`artifacts.py` is the single output inventory: 8 tier-1 and 20 tier-2 artifacts.
Both scripts resolve
their output paths and formatting sets from it; the bump's promotion/recovery sets
and CI's tier-2 comparison use the same inventory. The bump classifier also
reads it when identifying the four vendor files still lacking producers. Composite's second output
and all four implicit charset outputs are included. Nikon AF points and Leica
lens data preserve handwritten sections and are accounted for as whole mixed
files, including during backup.

`just bump-exiftool <version>` invokes `upgrade_transaction.py` through the
existing shell entry point. See the transaction contract below and the historical
[13.58 to 13.59 exercise](../../docs/reference/bump-reports/13.58-to-13.59.md).

Four generated-origin files still have no committed generator: Sony
`plain_tables.rs`/`enciphered_tables.rs` and Nikon
`settings_tables.rs`/`encrypted_tables.rs`. The two previously orphaned
Sony main-extra/Minolta outputs now have generators. `regen-all.sh` names the
remaining limits; generated once does not mean automatically refreshable.

## Generated-output inventory and write checks

```sh
python3 tools/exiftool-tables/artifacts.py paths                 # all 28 outputs
python3 tools/exiftool-tables/artifacts.py paths --tier 2        # downstream outputs
python3 tools/exiftool-tables/artifacts.py paths --tier 1 --kind rust --absolute
python3 tools/exiftool-tables/artifacts.py path composite-compute
python3 tools/exiftool-tables/artifacts.py diff --tier 2         # requires HEAD counterparts
python3 -m unittest discover -s tools/exiftool-tables -p test_artifacts.py
```

`regen.sh` and `regen-all.sh` snapshot repository state before generation and
check it on exit, including when a producer or formatter fails. The check hashes
file contents and modes, records symlink entries without following them, and
compares HEAD and logical index entries. Unexpected additions, deletions or
changes fail, even if a file was already dirty before entry. Required outputs
must remain regular files without symlink components. A tier-2 run cannot write
tier-1 artifacts. CI additionally requires every selected output to exist in
HEAD and compares staged plus unstaged changes against it.

The guarantee is **final net repository state**, not a trace of physical writes.
Writing the same bytes, creating then removing a temporary harness, changes
outside the repository, Git metadata, Cargo `target` trees, Python `__pycache__`
directories and explicitly selected cache/build directories are outside the
content comparison. Cache exclusions overlapping tracked files or declared
outputs are rejected. Ignored source files outside these exclusions are still
checked. The guard does not prove that a permitted mixed file's handwritten
portion stayed correct; generator-specific tests and the existing oracles remain
necessary. Formatting runs directly on manifest Rust paths with the package's
2024 edition and fails if rustfmt fails.

Write accounting does not establish cross-Perl reproducibility. An earlier
same-pin run omitted `CanonCustom::ConvertPfn` and 29 uses under Perl 5.34. The
implemented Canon repair recognizes both audited deparse bodies and requires the
named verification entry and correct input domain. Fresh native Perl 5.34 and
5.38 oracles each passed 16,789 probe comparisons; generated binary and IFD Rust
match each other and the committed files. See the execution plan for scope.

The EXIT check preserves a failing producer's status and keeps its entry snapshot
outside the checkout for diagnosis. It detects violations **without rollback**.
The separate upgrade transaction supplies build isolation and checked promotion.
The runner includes GeoTIFF, DICOM and lens alternatives. After formatting, it runs
`verify_geotiff.py` (compiled names, map dispatch and lookup over the complete
u16 domain) and `verify_dicom_dict.py` (all dictionary/UID facts compared with
the loaded Perl tables), and `verify_lens_alternatives.py` (complete Canon EF/RF
identity rows and Pentax alternatives, with explicit absence checks for RF and
Olympus fractional alternatives). All use the same selected source and Perl as
generation; CI executes them before checking drift. These checks certify the
declared facts, not file parsing. Canon EF/RF selection preserves raw identity
with the selected occurrence; RF uses its own lookup table. Catalog
synchronization and validation-baseline refresh remain separate work.

The inventory counts refreshable files, not extraction coverage or the fraction
of the codebase that is generated. These producers can regenerate supported
declaration changes and check their emitted facts automatically. A new Perl
shape, conversion or runtime dependency deliberately refuses and requires a
reviewed generator/engine change. The Canon ID collision discovered during
wiring is an example of runtime work that generation alone cannot solve. Four
Sony/Nikon outputs still lack producers. Only the planned release-delta exercise
can establish which manual interventions that particular upgrade needs.

## Isolated upgrade transaction

Start from a clean owned branch, including ordinary untracked files. The command
captures that commit, source contents and index before creating two private
clones in a unique external report directory. It does not register Git worktrees.
An ordinary BEFORE builds committed artifacts; `--from` on a retrospective dry
run regenerates every selected tier for BEFORE. AFTER always regenerates every
selected tier. Both variants use exact copied ExifTool sources, the selected
capable Perl, fresh dumps, separate oracle/build targets and the fresh executable
reported by Cargo. Corpus A/B uses one target-version oracle and the same inputs.

```sh
bash tools/exiftool-tables/bump-exiftool.sh 13.59 --dry-run \
  --old-exiftool-dir /absolute/path/to/ExifTool-13.59 \
  --new-exiftool-dir /absolute/path/to/ExifTool-13.59 \
  --perl /usr/bin/perl --corpus /absolute/path/to/corpus \
  --report-dir /absolute/path/to/external/reports
```

Use `--help` for selection and population-floor options. `--skip-conformance`
produces incomplete evidence, exits nonzero, and cannot be used for promotion.
The report directory retains stage logs, input identities, generated payloads,
comparison results and `transaction.json`. A same-pin dry run validates the
workflow only; it does not measure the cost or compatibility of a release change.
For a retrospective `--from` exercise, both variants use the current source
commit with their respective generated artifacts. This isolates the generator
transition; it does not reconstruct an old released OxiDex implementation.

Dry runs and failures before promotion leave caller source and index untouched.
A live run rechecks caller identity and promotes only the 28 manifest outputs
plus the pin, using journaled payloads and atomic file replacement. It preserves
the index. Interrupted promotion can be resumed as checked restoration with:

```sh
bash tools/exiftool-tables/bump-exiftool.sh --recover
```

Recovery refuses altered payloads, changed caller HEAD/index, unknown concurrent
source changes, and partially written or changed staging files. It reports the
conflicting path and retains evidence; manual preservation/review is required
before retrying that case. Do not delete recovery evidence or overwrite the
caller to force a retry. The transaction is final-state accounting, not a sandbox
against arbitrary generator writes or a guarantee of unattended upgrades.

## What is generated

ExifTool combines a generic engine with declarative tables and procedural
behavior. `dump_tables.pl` loads Perl modules and walks their in-memory tables,
including tables assembled dynamically; it does not try to parse the Perl source
with regular expressions.

`src/tag_sync` separately consumes the `-listx` documentation view. That catalog
provides names and descriptions, but not byte layouts, subdirectory dispatch,
conditions or conversion semantics. Catalog size is not extraction coverage.

```text
ExifTool loaded tables -> dump_tables.pl -> tables.json
  -> verify_exprs.py -> expression PASS ledger
  -> codegen.py -> binary_tables.rs + ifd_tables.rs + conversion accounting
  -> codegen_composite.py -> Composite definitions and expression computations
  -> downstream generators -> vendor-specific artifacts

ExifTool loaded tables -> oracle.pl -> independent declaration facts
  -> verify.py compares the generated Rust against those facts
```

`codegen.py` handles both `ProcessBinaryData` and `ProcessExif`-style IFD tables.
It does not implement arbitrary custom processing procedures. Unsupported
semantics require explicit refusal or further implementation; an ungenerated
table is not evidence that the upstream table does not exist.

`codegen_subdirs.py` targets the separate vendor subdirectory interpreter in
`src/parsers/tiff/makernotes/shared/binary_subdir.rs`. It takes an explicit table
list and raises on unsupported constructs; any intentional `--allow-skip` must
remain visible in the resulting evidence.

## Translation and verification

Conversions use reviewed exact translations or the closed expression compiler
in `exprs.py`. Oracle-gated ValueConv/PrintConv expressions must appear in the
matching PASS ledger. Unsupported semantics are refused and counted, not
approximated. Named helper implementations in `src/exiftool_tables/exprs.rs`
remain maintained Rust.

Keep these checks separate:

- `verify.py` compares generated binary/IFD declaration facts with the independent
  Perl oracle. It is not the expression execution checker.
- `verify_exprs.py` differentially evaluates translated scalar/list expressions
  on its defined probes. Generated Composite computations use a different domain
  excluded from this oracle; they need their own validation.
- `verify_cond.py` and `verify_subdirs.py` check conditions and Start/Base semantics
  within their declared scopes.
- Codegen reports identify omissions inside the scopes they enumerate. They do
  not yet constitute an exhaustive declaration-to-runtime producer inventory.
- `reachability.py` reports emitted, enabled, eligible and refused binary/IFD
  tables from static artifacts. Its lookup scan is not observed runtime execution.
- Engine/carrier tests and `conformance.py` compare actual behavior. A finite
  probe suite does not prove equivalence for every possible input.

## Where to spend effort

Use the [remaining-work backlog](../../docs/AUTOMATION-AND-TESTER-PLAN.md).
Check whether a missing output is caused by absent declarations, an unsupported
shared rule, disabled routing or a hand-specific procedure before writing a new
parser. Prefer a shared rule when it unblocks several relevant tables, then
validate the resulting runtime migration.

Run `expr_coverage.py`/`analyze.py` against the current pinned dump to rank refused
forms. Historical percentages in [Transcription](../../docs/TRANSCRIPTION.md)
describe their recorded snapshots, not today's extraction or upgrade automation.

### Expression-oracle execution limits

`verify_exprs.py --build-timeout 1800 --timeout 180` gives Cargo compilation a
separate deadline from each Perl/Rust oracle execution. The harness is built
with two Cargo jobs and run at the unique executable path Cargo reports for
its exact source file. Probe IDs must be complete and unique before any ledger
is written; an empty or partial pair of outputs cannot establish equivalence.
An existing harness is preserved and refused. Managed process groups are cleaned
on failure and interruption; the upgrade journal retains the command status
and any additional cleanup failure. These limits apply to this oracle, not an
overall wall-clock limit for the upgrade transaction.
