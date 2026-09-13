# Staged native serial-layout inventory

`tools/exiftool-tables/serial_directory.py` compiles a captured native
`ProcessSerialData` table into a versioned JSON descriptor. It is a source
inventory only: it has no Rust literal, reader, enablement entry or carrier
route.

It can also inventory the complete source-selected population rather than a
hand-picked table:

```sh
python3 tools/exiftool-tables/serial_directory.py "$DUMP" --output "$OUT"
```

`$DUMP` is a recorded `dump_tables.pl` JSON capture from the pinned native
source. The population selects every table whose resolved `PROCESS_PROC` name
ends in `::ProcessSerialData`, before compiling any row. It keeps empty and
descriptor-refused tables in its report, so generated output cannot choose its
own denominator.

Descriptor version 1 preserves:

- library-relative processor name, source file/hash and full deparse hash;
- the native ordered effects needed by a future reader, including temporary
  unknown handling, raw-value storage, report ordering and restoration;
- table default format, groups and literal variables;
- serial index order and all source alternatives;
- a field format with one of `fixed`, `prior_raw_value`,
  `floor_div_prior_raw_value`, or `remaining_bytes` count operands;
- raw source conditions plus the shared condition spelling and whether a
  missing Perl member must be evaluated as an empty string;
- raw conversion facts, reporting flags and group overrides; and
- a Gate A list for every unsupported row property.

The compiler records but does not approximate native expression conversions.
For example, `Image::ExifTool::DecodeBits($val, undef, 16)` is emitted as a
source fact and blocks the table with `serial_decode_bits_words` until a shared
multi-word conversion is independently proved. A future reader must use raw
`ReadValue` values for `prior_raw_value` operands before any conversion or
output rendering.

A descriptor is stale when either the raw table facts or the complete captured
processor deparse changes. `serial_directory_facts.py` contains only canonical
fact hashing, so an independent verifier can recompute those identities without
importing the recognizer.

The committed 13.59 recorded-input report
`serial-layout-inventory-processserialdata.json` contains eight selected tables:
130 native entries and 132 native alternatives. Eight tables produce descriptor
records, 115 alternatives clear the descriptor's row gate, 17 retain named
refusals, four tables have no row-level Gate A blocker, and no selected table is
empty. `Real::MediaProps` also retains its native `PRIORITY => 0` as the named
table-level blocker `serial_table_priority`; this checkpoint does not discard
collision/reporting policy. `Real::AudioV3` is the non-AFInfo control: it uses the same resolved
processor and has 12 source rows with no row-level refusal. This describes
source facts; it does not prove a Rust reader, parent dispatch, or carrier
route.
