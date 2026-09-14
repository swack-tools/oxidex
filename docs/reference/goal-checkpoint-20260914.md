# QuickTime source and reading baseline

Status: resumed in the full-parity goal on 2026-09-14. This milestone preserves a
reproducible baseline and does not add runtime reading or writing support.

## Source inventory

Pinned ExifTool 13.59, hydrated by the dump tool at `304d6339`, has 105 ItemList
rows, 186 UserData rows (210 alternatives), and 81 Keys rows. The selector finds
91 preliminary ItemList candidates and refuses 14. UserData and Keys remain
blocked by their distinct protocols. These are source capability candidates,
not generated Rust support or observed coverage. Table-level processor behavior
must still be validated by the generic-reader implementation.

The short report is `docs/reference/quicktime-source-baseline.json`. The complete
identity/refusal ledger is `tools/exiftool-tables/quicktime_source_capabilities.json`.
The verbatim three-table snapshot, including processor metadata, is
`tools/exiftool-tables/fixtures/quicktime_source_13_59.json`; its capture scope
records the parent dump/tool hashes, base commit and Perl version. This snapshot
covers three tables, not the complete ExifTool universe.

## Reproduce and check staleness

From the repository root:

```sh
python3 tools/exiftool-tables/quicktime_atom_tables.py \
  --dump tools/exiftool-tables/fixtures/quicktime_source_13_59.json \
  --output tools/exiftool-tables/quicktime_source_capabilities.json \
  --summary docs/reference/quicktime-source-baseline.json --check
python3 tools/exiftool-tables/quicktime_baseline.py --check-fixtures
python3 -m unittest discover -s tools/exiftool-tables -p 'test_quicktime*.py'
```

Replace `--check` with `--replace` to regenerate both reports. Existing outputs
require explicit replacement; outputs can never alias the input snapshot. The
selector prints the standard instrument header and refuses an unexplained dirty
tree. Its generated content remains deterministic, keyed by input and tool hashes. To refresh the source, run `dump_tables.pl` against the repository-pinned
library, preserving its capture command, source commit, Perl version and output.
Then extract the selected tables with the reproducible command below, using a
new output file. The source commit must be the commit used to make the full dump:

```sh
python3 tools/exiftool-tables/capture_quicktime_baseline.py \
  --dump "$FULL_DUMP" --source-commit "$DUMP_SOURCE_COMMIT" \
  --perl-version "$DUMP_PERL_VERSION" --output "$NEW_QUICKTIME_SNAPSHOT"
```

The extractor records the full dump hash, checks the source tool blob, preserves
table bodies verbatim, and records the selected versus parent table counts.
The original capture command remains evidence for which tool produced the full
dump; a supplied source-commit argument alone cannot establish that history. Never derive reader
layouts from the TagNames catalog. A supported synthetic source-row addition
appears in the selector test without any handwritten tag-name list; this proves
selector behavior only. Generated-Rust regeneration remains the next milestone.

## Native reading replay

With `EXIFTOOL_TREE` pointing to the pinned checkout and `EXIFTOOL_PERL` selecting
a capable Perl, run under the host's shared heavy-job lock:

```sh
python3 tools/exiftool-tables/quicktime_baseline.py \
  --exiftool-dir "$EXIFTOOL_TREE" --out "$QUICKTIME_BASELINE_OUTPUT" \
  --check-reading-baseline docs/reference/quicktime-reading-baseline.json
```

The output directory must be new and outside the worktree. The command builds
this checkout, resolves the binary from Cargo's compiler output, checks the oracle
version against `.exiftool-version` and checks its module capabilities. It records
raw oracle/oxidex results, build output, source state and binary/fixture hashes.
A dirty tree refuses unless the standard explicit dirty-tree override is set;
the override and exact dirty paths are then reported.

The fixed corpus is `tests/fixtures/quicktime/source_family_baseline`: five
synthetic parser/format behaviors. The committed pre-migration reading baseline
matches text and enum and fails unsigned 16-bit plus two unsigned 64-bit values:
2/5 ItemList-only fixture projections. This is deliberately a failing baseline,
not an assertion that all fixtures pass, and not overall corpus conformance.
There is no before/after runtime improvement in this PR. Writing is unmeasured.

## Next implementation milestone

Generate Rust specs, ledger and generic ItemList execution together; verify the
processor contract and replace handwritten atom-name mappings. Include native
format/language/duplicate/error behavior, unknown-atom handling, no-regression
comparisons and supported-source-row regeneration. UserData and Keys need separate
protocol support. Broader family accounting continues in #776. The full goal
continues until the complete pinned catalog scope is implemented and verified.

Validation: focused Python tests include source/ledger staleness, fixture bytes,
source/output alias protection and observation comparison. A fresh Cargo replay
reproduced every original native and oxidex projection. Rust/Cargo source is
unchanged from the base; Clippy passed on that identical source.

The committed replay records the exact binary hash, instrument hash, source HEAD
and dirty paths. The explicit dirty override covered baseline tools/docs;
`git diff HEAD -- src Cargo.toml Cargo.lock build.rs` was empty. Cargo validated
its cached executable against that unchanged runtime. The general timestamp
warning remains visible in the raw log because documentation is newer than the
executable; the build log is the validation evidence. The focused test suite passes.


Oracle source identity is pinned beyond the version string. The committed
`quicktime_oracle_sources_13_59.json` manifest records all 247 script/library files
from upstream commit `2200871d9cef988051d2a99d67df3bda6cbb30a8` (tag 13.59), plus
the downloaded archive hash. The local oracle matched all 247 file hashes.
Replay verifies this source-file universe before and after comparison and records
the manifest hash. Same-version local source edits are refused. The source
fingerprint covers tracked differences and untracked file contents, detecting
changes even when the dirty-path list stays the same. Fixture-check mode also
prints its instrument header and enforces the standard dirty-tree policy.

## Subsequent implementation

The baseline above is historical. Current ItemList integration and remaining
work are recorded in [generated ItemList progress](quicktime-generated-reader.md).

The earlier writer checkpoint is preserved separately in
[writer checkpoint](writer-checkpoint-20260914.md). Its stop instruction describes
the historical session, not the active full-parity goal.

The [upgrade rehearsal checkpoint](upgrade-checkpoint-20260914.md) likewise
records historical evidence rather than a passing combined-tree gate.

The [Nikon checkpoint](nikon-checkpoint-20260914.md) preserves that branch’s
source generation evidence and outstanding regeneration limits.
