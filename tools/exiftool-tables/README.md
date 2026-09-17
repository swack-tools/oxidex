# ExifTool table generation and verification

For completed work, known gaps and priorities, start with
[Tag machinery status](../../docs/TAG_MACHINERY_STATUS.md). This file describes
the tooling based on [PR #740](https://github.com/swack-tools/oxidex/pull/740)
at [`2cea1e41`](https://github.com/swack-tools/oxidex/commit/2cea1e4194ce7fc1aeed4b110d77ad436ccd7837);
the pin remains 13.59.

Conservative IFD-aware upgrade classification, one generated-output inventory,
verified Canon CODE references and isolated upgrade orchestration are implemented.
The inventory contains 34 outputs, including inactive scalar helper, keyed and serial definitions and the
[Sony plain producer recovery](../../docs/reference/sony-plain-generator-recovery.md). See the
[execution plan](../../docs/UPGRADE-NEXT-STEPS.md) for original validation evidence
and the remaining work. The
[13.55-to-13.59 retrospective rehearsal](../../docs/reference/bump-reports/13.55-to-13.59.md)
passed at `4fb705da` on 2026-09-11: both variants regenerated all 28 outputs and
built fresh binaries, with no source-edit intervention and unchanged caller
source/index/pin. Its 193-file A/B uses current handwritten runtime on both
sides; classifier AUTO/HAND percentages do not measure upgrade coding effort.

## Commands and scope

```sh
just regen-tables          # primary generation, including binary, IFD, keyed and serial tables
just verify-tables         # independent checks of committed table declarations
just regen-tables-all      # primary generation plus all declared downstream tables
just regen-tables-tier2    # downstream generation only
```

Run generation in an owned worktree with the intended pin and a capability-probed
Perl/ExifTool pair. Generation modifies tracked artifacts; ordinary measurement
scripts refuse a dirty tree unless the explicit override is provided and recorded.
These direct generation commands modify the caller's artifacts. Use the bump
command below when comparing versions in isolated source trees.

`regen.sh` generates binary, IFD and inactive keyed/serial tables, file identification, Composite
definitions and compiled Composite expressions, FITS names, and expression/value-
conversion ledgers. `regen-all.sh` adds vendor subdirectory tables, Nikon AF-point
grids, bespoke transcriptions, recovered Sony/Minolta/Nikon generators, Macintosh
CJK charset tables, GeoTIFF key maps, DICOM dictionaries and lens alternatives.
`artifacts.py` is the single output inventory: 12 tier-1 and 22 tier-2 artifacts.
Both scripts resolve
their output paths and formatting sets from it; the bump's promotion/recovery sets
and CI's tier-2 comparison use the same inventory. The bump classifier also
reads it when identifying the two vendor files still lacking producers. Composite's second output
and all four implicit charset outputs are included. Nikon AF points and Leica
lens data preserve handwritten sections and are accounted for as whole mixed
files, including during backup.

Keyed definitions are generated, formatted and independently checked by the
normal tier-1 command. Their presence does not enable a parser: unsupported
child processors remain explicit blockers and runtime activation is separate.
Native reader diagnostics retain the error text and source line while using
authenticated library-relative source paths, so moving the selected library
does not change these captured facts.

`just bump-exiftool <version>` invokes `upgrade_transaction.py` through the
existing shell entry point. See the transaction contract below and the historical
[13.58 to 13.59 exercise](../../docs/reference/bump-reports/13.58-to-13.59.md).

Two generated-origin files still lack producers: Sony `enciphered_tables.rs`
and Nikon `encrypted_tables.rs`. Sony main-extra, Sony plain, Minolta A100 and
Nikon settings now have producers. `regen-all.sh` names the remaining
limits; generated once does not mean automatically refreshable.

The shared Sony focus-table migration removes the 17 `Tag202a` declarations
from the legacy enciphered artifact. `table_ownership.json` records table
ownership without copying tag rules. `retire_binary_tables.py` performs the
mechanical removal and checks the remaining table indices; it is not a
recovered native producer for the other legacy tables. Its identity ledger
preserves the original input/output hashes and index range. Supported native
changes to the shared table continue through the normal shared generator.

After a future accepted producer rebuilds the legacy artifact, apply the
ownership transform from the repository root:

```sh
python3 tools/exiftool-tables/retire_binary_tables.py \
  --manifest tools/exiftool-tables/table_ownership.json \
  --source src/parsers/tiff/makernotes/sony/enciphered_tables.rs \
  --input src/parsers/tiff/makernotes/sony/enciphered_tables.rs \
  --output src/parsers/tiff/makernotes/sony/enciphered_tables.rs \
  --shared-tables src/exiftool_tables/binary_tables.rs \
  --enabled-tables src/exiftool_tables/enabled.rs \
  --consumer-root src \
  --identity-out tools/exiftool-tables/table_ownership_identity.json
```

An already recorded in-place replay verifies the current route and references
without changing the artifact or ledger. A separate output path receives the
verified bytes. A regenerated retired root handle refuses rather than silently
restoring duplicate handling. The consumer scan recognizes documented literal
Rust forms; aliases, macros and general data flow require review. Native
inventory and runtime comparisons remain the proof of equivalent behavior.
The transform is not yet an automatically invoked `regen-all.sh` producer;
the remaining legacy producer is still unaccepted.

Nikon settings is checked against freshly loaded Perl by
`verify_nikon_settings.py` after regeneration. Its 197 rows and 131 maps are
unchanged. The custom handwritten processor remains; the verifier checks
transcribed facts, not whole-parser equivalence. `AFAreaMode` state propagation
and the existing `BracketProgram` mask behavior remain explicit residuals.

Sony plain uses `gen_sony_plain_tables.py` with the selected fresh dump, then
`verify_sony_plain.py --input <output> --exiftool-dir <source> --perl <interpreter>`
checks its six tables, 193 rows, 72 maps and one bitmap against live Perl.
The native verifier parses the whole Rust DSL independently of the producer.
It requires the generated `RAW_TAG_IDS` array and checks exact native ID order
and variant repetition. Shared byte offsets are reported separately from tag
identity. See the [runtime repair report](../../docs/reference/sony-raw-id-runtime.md)
for the restored field, bounded validation and remaining real-file acceptance.

## Corpus read receipts

`corpus_read_receipt.py build|observe|verify` measures OxiDex's public reads of a
corpus against pinned ExifTool with stored, replayable transcripts; see
`docs/CATALOG-HYDRATED-JOIN.md` ("Corpus read receipts") for the identity,
matching and catalog-attribution rules. Receipts are written outside the
checkout and refuse a dirty tree.

## Garmin FIT specs

`codegen.py` also emits the Garmin FIT message/field specs
(`src/exiftool_tables/fit_tables.rs`) and their ledger
(`garmin_fit_ledger.json`) from the same dump plus the protocol sidecar
captured by `capture_garmin_fit_fact.pl`. FIT conversions carry the domain
they were compiled for, because a FIT field's format arrives in the file.
See [GARMIN_FIT_READER.md](GARMIN_FIT_READER.md).

## Generated-output inventory and write checks

```sh
python3 tools/exiftool-tables/artifacts.py paths                 # all 34 outputs
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
wiring is an example of runtime work that generation alone cannot solve. Two
Sony/Nikon outputs still lack producers. The recorded release-delta rehearsal
establishes the interventions for that specific experiment; future releases
need their own measured refresh and refusal record.

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
  --old-exiftool-dir ../ExifTool-13.59 \
  --new-exiftool-dir ../ExifTool-13.59 \
  --perl /usr/bin/perl --corpus ../corpus \
  --report-dir ../upgrade-reports
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
A live run rechecks caller identity and promotes only the 30 manifest outputs
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
  -> codegen.py -> binary_tables.rs + ifd_tables.rs + keyed_tables.rs + conversion accounting
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

## Running the Python checks with native table data

The `test_codegen_ifd.WholeDump` checks require a real `dump_tables.pl` result.
An ordinary discovery run can skip the entire class if no dump is available;
that result does not validate the complete native table generation. Supply the
recorded dump explicitly when checking schema or generator changes:

```bash
OXIDEX_TABLES_JSON=../scratch/tables-13.59.json \
  python3 -m unittest discover -s tools/exiftool-tables -p 'test_*.py' -v
```

Use a real path to your recorded dump in place of this example. The dump must
match the pinned release, and conversion-ledger checks additionally require
its digest to match `expr_oracle_ledger.json`. Read the skipped-test messages:
a version match alone does not establish that the ledger checks ran. Keep the
dump digest and executed/skipped counts with the validation record.

The [serial-processor probe](../../docs/reference/serial-processor-checkpoint.md)
has eight native-only checks. Set `OXIDEX_PINNED_EXIFTOOL` to the pinned source
root and `EXIFTOOL_PERL` to Perl 5.38.2 to run them. They observe the actual
table-owned processor and reader binding; they do not validate a Rust serial
reader or final metadata output. An omitted input skips these optional local
checks; an explicitly wrong input fails. CI supplies the required inputs.

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
