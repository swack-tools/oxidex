# Recorded source-to-artifact join

[`source-artifact-join-13.59.json`](source-artifact-join-13.59.json) joins all
1,512 table identities in the recorded ExifTool 13.59 Perl 5.38 dump to the
binary, IFD, and keyed generated-artifact shapes at one immutable commit. The
full 1,512-row result is an external replay artifact; the committed JSON is a
compact, relocatable provenance and conservation record.

The table universe always comes from the recorded source dump. A missing table
literal remains an `absent` row, and an absent keyed artifact is reported as
`absent_at_commit`. Binary sidecar-only tables, zero-row definitions, and
unclassified processors remain in the row stream. The join also records
literal `GateA.blocked_by` reasons when that artifact exposes them.

Artifact presence is only a source-to-generated-artifact fact. It does not
show runtime reachability, manual maintenance, output parity, or an automation
percentage.

To reproduce the committed report, run from a checkout containing this tool.
The tool archives source selectors and reads artifact files from commit
`18a8ef172e2744b6511f883945846f135c12ad62`; the caller checkout itself does
not need to be at that commit. Supply the recorded dump and a new output
location through environment variables.

```sh
DUMP=${OXIDEX_TABLES_JSON:?set to the recorded Perl 5.38 table dump}
OUT=${SOURCE_ARTIFACT_JOIN_OUT:?set to a new output directory}
COMMIT=18a8ef172e2744b6511f883945846f135c12ad62

/usr/bin/python3 tools/exiftool-tables/join_source_artifacts.py \
  --dump "$DUMP" \
  --repo . \
  --selector-ref "$COMMIT" \
  --expected-dump-sha 193cf4e91326f53c7bdfd674cb10da0937c8bcdb4c0285ad4f507fd9f96a8fb8 \
  --expected-selector-commit "$COMMIT" \
  --artifact-ref "$COMMIT" \
  --expected-artifact-commit "$COMMIT" \
  --out "$OUT"
```

The command requires both immutable commit identities and the dump hash before
writing output. It validates the recorded dump, the binary and IFD artifact
registries, every generated artifact identity against the source universe, and
all parsed omission-sidecar entries. Its stderr header identifies the
instrument, runner, source selector, artifact commit, and dump. Stdout stays
machine-readable.
