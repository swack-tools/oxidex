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

Using canonical Perl
`/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2` and pinned library
`/tmp/oxidex-exiftool-cache/exiftool/lib`, the catalog contains 1,512 hydrated
tables, 11 shortcut entries, 21,437 unique tag names, and 33,487 catalog tag
entries. These last two figures are BuildTagLookup catalog counts, not parser or
writer coverage.

Against the supplied 13.59 `source-family-goal-20260914/dump.json`, the
structurally checked reconciliation is: 1,445 matched hydrated tables, 67
missing hydrated tables, one matched shortcut helper, zero missing helpers, and
66 extra legacy-dump identities. Both identity universes therefore conserve.

This milestone does not alter `dump_tables.pl` or claim that the 67 missing
layouts are available to a generated reader. The next migration is to make the
layout dump enumerate this sidecar's full names through `GetTagTable`, retaining
full names as primary identities for nested package tables such as
`QuickTime::Stream` and `XMP::SVG`.
