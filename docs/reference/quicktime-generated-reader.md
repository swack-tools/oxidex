# Generated ItemList reader progress

This implementation is pushed and under validation; it is not merged and does
not complete the full-parity goal. Source pin: ExifTool 13.59.

## What is connected

`quicktime_generated_specs.py` consumes the hydrated ItemList source table and
emits FourCC, name, group, format and safe enum operands. `regen.sh` supplies its
fresh dump, so ordinary supported rows generate without adding Rust name lists.
A synthetic new-source-row test checks that path. The bounded source snapshot is
only the deterministic test/check input.

The generator emits 91 of 105 ItemList rows. Fourteen remain explicitly refused.
The ledger also retains 210 UserData alternatives and 81 Keys rows with their
separate protocol refusals: 396 source records, 91 generated and 305 refused.
These counts describe generated specs, not complete observed support for every
value or file carrying those rows.

The main QuickTime `moov/udta/meta/ilst` caller now invokes one generic executor
for accepted specs. Their old handwritten name and enum branches are removed.
Canonical keys use family 0 `QuickTime`, with source-derived family 1 `ItemList`;
this avoids storing two alias copies of the same generated occurrence. Both
printed and no-print-conversion forms are retained. The executor handles text
encodings, implicit scalar types, explicit unsigned widths, unsigned 64-bit
values, complete numeric arrays and partial tails according to the native
`ProcessMOV`, `QuickTimeFormat` and `ReadValue` behavior inspected for this pin.

## Observed reading

Instrument: `verify_quicktime_reader.py`. Its committed fixture builder creates
22 behavior fixtures and compares ItemList-only projections against the pinned
native CLI, both with PrintConv and without it. Both tools use `-j -a -G1`;
ExifTool uses `-n` for raw values and oxidex uses `--no-print-conv`.

All **44/44 observations matched**. The original five canaries now match 5/5,
including unsigned 16-bit, AlbumID 4294967297, and AlbumID 18446744073709551615.
The historical pre-migration ItemList projection matched 2/5. This is a bounded
fixture improvement, not full-corpus conformance or a global generated percentage.
No writing behavior was added or measured.

The [machine-readable report](quicktime-generated-reading-13.59.json) records
source state, binary hash, tool/helper hashes, oracle commit and every observation.
The run built the exact compiler-reported binary and checked the 247-file oracle
source manifest before and after measurement. Its dirty-tree state is explicit.
The latest run rebuilt and verified the current runtime including source group
overrides.

Validation so far: 34 focused Python tests, a fresh CLI build, Clippy with denied
warnings, 44 native comparisons, and the full workspace test suite pass.
Real-container conformance remains pending.

## Reproduce

Set `EXIFTOOL_TREE` to the pinned source tree and `EXIFTOOL_PERL` to the compatible
Perl executable. Run under the host shared heavy-job lock, with a new output
directory outside the worktree:

```sh
python3 tools/exiftool-tables/verify_quicktime_reader.py \
  --exiftool-dir "$EXIFTOOL_TREE" --out "$QUICKTIME_READER_OUTPUT"
python3 tools/exiftool-tables/quicktime_generated_specs.py --check
```

## Required before this implementation is ready to land

- Finish carrier/caller integration, including the older AAC ItemList path,
  other ItemList locations, and duplicate/group behavior on real containers.
- Preserve unknown atoms explicitly as unknown. The remaining legacy fallback
  still guesses a string interpretation; it is not the intended final behavior.
- Add source-derived language identities. Non-default locales are currently
  omitted rather than mislabeled as the default; this is a known protocol gap.
- Record unsupported malformed-text and non-finite-number behavior explicitly,
  run regression/conformance gates, and address review findings before merging.

UserData, Keys, remaining ItemList refusals, other source families and writing
remain part of the full goal. Passing these fixtures does not remove them.

## Workspace validation follow-up

The full `cargo test --workspace` run passes after updating MP4 integration
assertions to the canonical family-0 storage keys. The separate classic
`UserData:Title` value is checked explicitly. `cargo fmt --check` and
`cargo clippy --lib -- -D warnings` also pass. The 22 behavior fixtures and
44 printed/raw native comparisons remain the observed reading evidence;
workspace tests do not extend that measurement to the whole catalog.

Protocol guards now include QuickTimeFormat, ReadValue, Decode, and Charset
helper bodies, LoadCharset, reachable csType values and the loaded ShiftJIS
mapping. Map-only changes and missing maps refuse generation; ordinary new
rows retain the existing protocol. Literal source family-0 overrides are now
propagated into generated specs rather than silently replaced with QuickTime.
No generated writer is enabled here.
