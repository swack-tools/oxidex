# Hydrated catalog universe

`dump_hydrated_catalog.pl` records the pinned ExifTool catalog after its normal
table hydration. It is intentionally a small identity sidecar: the existing
`dump_tables.pl` remains the source of layout, conversions, conditions, and
processor facts.

The producer calls `LoadAllTables`, builds `BuildTagLookup`, and records every
full table name in `%Image::ExifTool::allTables`. It records `Extra` and the
runtime aggregate `Composite` as table kinds, and records the single
`Shortcuts::Main` macro table separately. Shortcut member names are not copied
into this identity inventory.

The reconciliation reader consumes only the existing dump's `modules`
projection. It structurally validates its root/module/table fields and streams
tag objects without loading the complete dump. Its categories conserve both
sets: matched and missing hydrated tables, matched and missing helper tables,
and extra dump identities.

Both tools require the repository `.exiftool-version` to match the loaded or
recorded version. The reconciliation CLI emits an instrument header, refuses a
dirty worktree unless `OXIDEX_ALLOW_DIRTY_TREE=1`, and records SHA-256 hashes of
the catalog and dump inputs.

## Verified 13.59 baseline

Using the gate-configured `EXIFTOOL_PERL` and `OXIDEX_PINNED_EXIFTOOL`, the
catalog contains 1,512 hydrated tables, 11 shortcut entries, 21,437 unique tag
names, and 33,487 catalog tag entries. `OXIDEX_PINNED_EXIFTOOL` may name either
the pinned source root containing `lib/` or the `lib/` directory itself. These
last two figures are BuildTagLookup catalog counts, not parser or writer
coverage.

Against the supplied 13.59 `source-family-goal-20260914/dump.json`, the
structurally checked reconciliation is: 1,445 matched hydrated tables, 67
missing hydrated tables, one matched shortcut helper, zero missing helpers, and
66 extra legacy-dump identities. Both identity universes therefore conserve.

This milestone does not alter `dump_tables.pl` or claim that the 67 missing
layouts are available to a generated reader. The next migration is to make the
layout dump enumerate this sidecar's full names through `GetTagTable`, retaining
full names as primary identities for nested package tables such as
`QuickTime::Stream` and `XMP::SVG`.

## Provenance and instrumentation limits

The producer verifies the pin and hashes only the four bootstrap/catalog source
files it directly names: `Image/ExifTool.pm`, `Image/ExifTool/Writer.pl`,
`Image/ExifTool/BuildTagLookup.pm`, and `Image/ExifTool/Shortcuts.pm`. It does
not attest every transitively loaded table module. It also does not emit a git
instrument header or refuse a dirty checkout; the reconciliation CLI is the
measurement instrument that records git state, input hashes, and the dirty-tree
refusal.

## Reproduce

Set `EXIFTOOL_PERL` to the capture-compatible Perl executable and
`OXIDEX_PINNED_EXIFTOOL` to the pinned ExifTool source tree. Set `DUMP` to the
preserved full dump, and choose new output filenames outside the checkout.
This does not recapture layouts or modify the pinned source tree.

```bash
EXIFTOOL_LIB="$OXIDEX_PINNED_EXIFTOOL"
if [[ -d "$EXIFTOOL_LIB/lib" ]]; then EXIFTOOL_LIB="$EXIFTOOL_LIB/lib"; fi
(
  set -o noclobber
  "$EXIFTOOL_PERL" tools/exiftool-tables/dump_hydrated_catalog.pl \
    "$EXIFTOOL_LIB" > "$CATALOG_OUTPUT"
)
python3 tools/exiftool-tables/hydrated_catalog_reconcile.py \
  --catalog "$CATALOG_OUTPUT" --dump "$DUMP" --output "$CATALOG_REPORT"
```

Run the reconciliation under the shared heavy-job lock. The native test uses
the same two environment variables; without an explicitly configured source
tree it skips the two native checks rather than finding an ambient ExifTool.
