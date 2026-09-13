# Recorded source-processor inventory

[`source-processor-inventory-13.59.json`](source-processor-inventory-13.59.json)
is a source-shape report generated from the recorded ExifTool 13.59 Perl 5.38
table dump. It records the dump hash, the immutable selector commit, selector
file hashes, category counts, and conservation totals. It is not a measure of
generated acceptance, runtime reachability, manual maintenance, output parity,
or an automation percentage.

To reproduce the committed report, run from a checkout that contains this tool.
The tool archives selectors from commit `ebbe1ece906858b8c84590f59a0616f9f3675d73`;
the caller checkout itself does not need to be at that commit. Supply the
recorded dump and a new output location through environment variables.

```sh
DUMP=${OXIDEX_TABLES_JSON:?set to the recorded table dump}
OUT=${SOURCE_INVENTORY_OUT:?set to a new output directory}
COMMIT=ebbe1ece906858b8c84590f59a0616f9f3675d73

/usr/bin/python3 tools/exiftool-tables/inventory_source_processors.py \
  --dump "$DUMP" \
  --repo . \
  --selector-ref "$COMMIT" \
  --expected-dump-sha 193cf4e91326f53c7bdfd674cb10da0937c8bcdb4c0285ad4f507fd9f96a8fb8 \
  --expected-selector-commit "$COMMIT" \
  --out "$OUT"
```

The tool requires both expected identities, rejects an empty/incomplete dump
before writing output, checks every module and table count, and imports all
three selectors from the archived commit. Output paths in the report are
relative to `OUT`, so the committed JSON is relocatable.
