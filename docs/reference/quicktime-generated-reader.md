# Generated ItemList reader progress

This implementation is local and under validation; it is not merged and does
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
A generic timestamp warning remained because the new Python verifier was newer
than the cached Rust binary; Cargo validated the runtime before the comparison.

Validation so far: 65 QuickTime Rust tests, a fresh CLI build, Clippy with denied
warnings, and the 44 native comparisons. Workspace-wide regression checks and
real-container conformance remain pending.

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

- Capture and guard source dependencies of the processor, including
  `QuickTimeFormat` and string encodings. Checking `ProcessMOV` alone does not
  detect a helper-only parsing change. A whole-module hash must not reject an
  ordinary newly added row whose protocol is unchanged.
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
