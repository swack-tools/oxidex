# Native write-definition catalog

`tools/exiftool-tables/write_coverage_catalog.py` turns one captured
`dump_tables.pl` JSON document into a deterministic inventory of native
ExifTool write declarations. It is the foundation for measuring writing
honestly; it is not a writer test and it never reports a percentage as though
source declarations were successful OxiDex operations.

Run it against an already captured dump:

```sh
python3 tools/exiftool-tables/write_coverage_catalog.py \
  --dump /path/to/tables.json \
  --output /path/to/write-definition-catalog.json
```

The catalog hashes both the source bytes and canonical decoded JSON.
`source.root_module_facts` retains the capture's `modules`, `modules_ok`,
`modules_failed`, and `load_errors` values. `source.capture_completeness`
reports global module capture separately from the native writer context: a
resolved writer context does not make a capture with failed modules complete.
Even a capture with no reported module failures remains scope-unknown unless
the capture declares a complete requested module population.

`source.native_tiff_type_registry` is the captured `Exif.pm` TIFF field-type
registry. It is not a file-format registry and it does not assert that a tag
can be written to any carrier format.

Each record has a stable identity of module, table, full source table name, raw
row id, and conditional-variant path. `table_contexts` stores every table's
properties, controls, unknown controls and procedure provenance once;
`source_entries` stores every raw row once. Records point to both by stable
reference and retain the selected branch's SHA-256. Array alternatives become
separate records; empty or malformed arrays produce an explicit unresolved
record. This keeps duplicate raw identities, conditional branches, unknown
controls, non-literal write groups, unresolved effective rows, and non-row
source shapes visible without repeating large procedure bodies per record.

`native_definition.effective_writable` reports only the native source fact.
`native_definition.effective_write_directory` reports a literal effective
`WriteGroup` only when the captured effective row contains one. Table defaults,
unknown controls, WriteProc/CheckProc provenance, and capture resolution stay
as source evidence; none admits an OxiDex route.

Every record begins with:

```json
{
  "operation_coverage": {
    "state": "unexercised_no_validated_operation",
    "carrier_route": "not_inferred_from_definition_inventory",
    "validated_observations": []
  }
}
```

Accordingly, `metrics.coverage_percentage` is `null`. A future observation may
be counted only after OxiDex executes the public operation on a concrete carrier
and pinned ExifTool verifies the changed identity/value plus preservation of
non-target payload. The eventual denominator must include carrier profile,
native full table identity, raw tag id, effective write directory, accepted
public spelling/alias, and operation/value class. The catalog intentionally
does not manufacture any of those missing carrier or execution facts.
