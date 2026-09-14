# Hydrated reader source baseline

This capture makes every table in the pinned ExifTool 13.59 registry available
to source selectors. It is source inventory, not evidence of reading or writing
those tags with OxiDex. The separate catalog/runtime/implementation/evidence join
remains in progress.

| Measurement | Count |
| --- | ---: |
| Hydrated tables available/requested/emitted | 1,512 / 1,512 / 1,512 |
| Raw tag keys, excluding native table properties | 34,897 |
| Source-order variants | 35,886 |
| Native ordinary catalog entries | 33,487 |
| Native legacy unique-name counter | 21,437 |
| Actual distinct lowercase ordinary catalog names | 21,373 |
| Shared object definitions | 41,818 |
| References to catalog tables | 36,151 |
| Unresolved object or table references | 0 |
| Loaded ExifTool source files authenticated | 182 |

The native legacy counter is not the distinct-name denominator; see
[catalog denominator definitions](catalog-baseline.md). Raw keys and variants
include contexts excluded from the ordinary public catalog count.

The coordinate cross-check finds every one of the 33,487 ordinary catalog
entries at its exact table/raw key/variant with the same source name. All 168
source files recorded by the catalog have matching hashes in this capture.
This validates source identity; implementation and observed read/write status
still require their own joins.

The [machine-readable graph audit](/measurements/hydrated-reader-layout-audit-13.59.json)
records the capture hash, producer hash, source-file identities and counts.
The full external capture is 99,132,291 bytes, SHA-256
`34f6bc9efc38be6a04d2f8710a522d26e941a945c967a777020e886879072164`. Its evidence directory is
`source-family-goal-20260914/hydrated-reader-landing-1789408680/` under the session evidence root.

## Reproduce

Use the repository-pinned tree and compatible Perl under the shared host job lock.
Choose new output files so previous evidence remains intact.

```sh
"$EXIFTOOL_PERL" tools/exiftool-tables/dump_tables.pl \
  --reader-only --hydrated-layouts "$EXIFTOOL_TREE/lib" > "$HYDRATED_DUMP"
python3 tools/exiftool-tables/audit_hydrated_layouts.py \
  --dump "$HYDRATED_DUMP" --output "$HYDRATED_AUDIT" \
  --expected-audit "docs/public/measurements/hydrated-reader-layout-audit-13.59.json" \
  --catalog "docs/public/measurements/catalog-source-13.59.json"
```

The opt-in projection preserves runtime `Table`/`TagID` bindings and other row
properties, including values of properties outside the current generator grammar.
Shared references avoid unbounded recursive expansion. Native tag IDs beginning
with an underscore are retained. CODE values remain explicitly opaque source
facts; their presence is not an executable Rust capability.

Loaded files must resolve inside the selected library. Hashes of already loaded
files are checked before and after table serialization; source drift fails.
The audit rejects partial captures and unresolved or incorrectly typed references.
CI also compares semantic totals and complete source/producer/Perl provenance
against the committed audit, then reconciles every ordinary catalog coordinate
and exact name. The baseline requires Perl 5.38.2; a different capture environment
must be reviewed as a baseline update, not silently accepted. This catches
self-consistent producer row loss as well as source identity drift.
Five native projection tests, six graph-audit tests and denied-warning Clippy
pass. Runtime Rust is unchanged by this PR.

Earlier captures exposed recursive expansion and lost bindings/special keys;
their evidence is retained as superseded. This reader-only command deliberately
omits final writer sidecars: full writer capture and behavior remain unfinished.
