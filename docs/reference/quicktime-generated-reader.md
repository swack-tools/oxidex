# Generated ItemList reader progress

This implementation is being prepared as a reviewed, bounded reader PR; it does
not complete the full-parity goal. Source pin: ExifTool 13.59.

## What is connected

`quicktime_generated_specs.py` consumes the hydrated ItemList source table and
emits FourCC, name, group, format and safe enum operands. `regen.sh` supplies its
fresh dump, so ordinary supported rows generate without adding Rust name lists.
A synthetic new-source-row test checks that path. The bounded source snapshot is
only the deterministic test/check input.

The generator emits 92 of 105 ItemList rows. Thirteen remain explicitly refused.
The ledger also retains 210 UserData alternatives and 81 Keys rows with their
separate protocol refusals: 396 source records, 92 generated and 304 refused.
These counts describe generated specs, not complete observed support for every
value or file carrying those rows.

The main QuickTime `moov/udta/meta/ilst` caller invokes one generic executor
for accepted specs. Their old handwritten name and enum branches are removed.
Canonical keys use family 0 `QuickTime`, with source-derived family 1 `ItemList`;
this avoids storing two alias copies of the same generated occurrence. Both
printed and no-print-conversion forms are retained. The executor handles text
encodings, implicit scalar types, explicit unsigned widths, unsigned 64-bit
values, complete numeric arrays and partial tails according to the native
`ProcessMOV`, `QuickTimeFormat` and `ReadValue` behavior inspected for this pin.

## Observed reading

Instrument: `verify_quicktime_reader.py`. Its committed fixture builder creates
25 behavior fixtures and compares ItemList-only projections against the pinned
native CLI, both with PrintConv and without it. Both tools use `-j -a -G1`;
ExifTool uses `-n` for raw values and oxidex uses `--no-print-conv`.

All **50/50 observations matched**. The original five canaries now match 5/5,
including unsigned 16-bit, AlbumID 4294967297, and AlbumID 18446744073709551615.
The historical pre-migration ItemList projection matched 2/5. This is a bounded
fixture improvement, not full-corpus conformance or a global generated percentage.
Four of the 50 comparisons check absent output for unknown atoms; they are not
newly observed tag identities. No writing behavior was added or measured.

The [machine-readable report](quicktime-generated-reading-13.59.json) records
source state, binary hash, tool/helper hashes, oracle commit and every observation.
The run built the exact compiler-reported binary and checked the 247-file oracle
source manifest before and after measurement. Its dirty-tree state is explicit.
The latest run rebuilt and verified the current runtime including source group
overrides.

Validation so far: 31 focused Python tests, a fresh CLI build, Clippy with denied
warnings, 50 native comparisons, and the full workspace test suite pass.
An isolated before/after `conformance.py` run on 11 available containers and
canaries compares base `b027ce0b` with candidate `ab522539`. Matched rows changed
622 to 620, VALUE 2 to 1, MISSING 234 to 240, and EXTRA 114 to 86. This is not a
clean aggregate parity pass. Inspecting the native and candidate `-j -a -G0:1:4`
output shows the generated ItemList values retained with correct family-0/1
identity. The older implementation emitted duplicate ItemList/QuickTime aliases;
name/value matching could credit those aliases against equal UserData/Keys
occurrences in the native MOV. Those other producer paths remain unresolved.
Do not restore fabricated aliases to improve the score.

Evidence run: `itemlist-isolated-conformance-1789409005`. Separate Cargo target
directories and distinct binary hashes bind the two commits. The preceding
`itemlist-container-conformance-1789408875` run is invalid: a shared target
returned the same overwritten binary for both checkouts. Share sccache, but use
separate target directories for paired measurements.

## Reproduce

Set `EXIFTOOL_TREE` to the pinned source tree and `EXIFTOOL_PERL` to the compatible
Perl executable. Run under the host shared heavy-job lock, with a new output
directory outside the worktree:

```sh
python3 tools/exiftool-tables/verify_quicktime_reader.py \
  --exiftool-dir "$EXIFTOOL_TREE" --out "$QUICKTIME_READER_OUTPUT"
python3 tools/exiftool-tables/quicktime_generated_specs.py --check
```

## Remaining work toward full source-derived reading

- Finish other ItemList locations and verify duplicate/group behavior on real
  containers. The AAC/M4A entry point now delegates to the shared QuickTime parser.
- Unknown ItemList entries now retain raw identifiers and child payloads in
  `MetadataMap::raw_blocks()`, outside named-tag JSON. Existing source-declared
  date, track/disc, cover and genre readers remain hand-driven until their shared
  capabilities are migrated. Raw retention is not a file-writing guarantee.
- Add source-derived language identities. Non-default locales are currently
  omitted rather than mislabeled as the default; this is a known protocol gap.
- Extend malformed-text and non-finite-number evidence as shared capabilities
  expand; continue regression/conformance gates on each change.

UserData, Keys, remaining ItemList refusals, other source families and writing
remain part of the full goal. Passing these fixtures does not remove them.

## Workspace validation follow-up

The full `cargo test --workspace` run passes after updating MP4 integration
assertions to the canonical family-0 storage keys. The separate classic
`UserData:Title` value is checked explicitly. `cargo fmt --check` and
`cargo clippy --lib -- -D warnings` also pass. The 25 behavior fixtures and
50 printed/raw native comparisons remain the observed reading evidence;
workspace tests do not extend that measurement to the whole catalog.

Protocol guards now include QuickTimeFormat, ReadValue, Decode, and Charset
helper bodies, LoadCharset, reachable csType values and the loaded ShiftJIS
mapping. Map-only changes and missing maps refuse generation; ordinary new
rows retain the existing protocol. Literal source family-0 overrides are now
propagated into generated specs rather than silently replaced with QuickTime.
No generated writer is enabled here.

## Shared carrier and unknown-entry follow-up

The unreachable private AAC ItemList decoder and its name/enum maps were removed.
The M4A signature check now reads `ftyp` at offset four and uses the existing
QuickTime parser. Direct integration tests assert unsigned values and repeated
occurrences, including 4294967297 and 18446744073709551615, plus source groups.
Unknown ASCII and binary identifiers preserve exact child payloads through map
merge, cloning, normalization and raw-value projection without invented names.

`PrintConvColumns` is catalog presentation metadata (BuildTagLookup enum HTML
column layout), so it no longer blocks an otherwise accepted enum row. All
other unknown properties still refuse generation. Native MediaType reading now
exercises the same generated numeric/enum executor. No tag-specific Rust map was
added. Formatting, denied-warning Clippy, workspace tests and all 50 native
printed/raw comparisons pass on this follow-up.

## Stacked source capture

Rebasing onto the full hydrated graph capture leaves all Rust source, tests and
Cargo files byte-identical to the validated reader commit. A fresh canonical
Perl capture retains exactly the same selected QuickTime tables and graph counts.
Its five helper deparses have reviewed cosmetic declaration/undef spellings; the
guard accepts only the exact bounded and fully hydrated body hashes for each
named helper. All other changed bodies remain refused. The capture tool now
retains the reader protocol directly, so rebuilding the bounded fixture needs no
manual source-fact patch. Thirty-two focused Python tests pass after regeneration.
The generated Rust is byte-identical. Source provenance is updated; the earlier
50/50 runtime observations remain bound to their recorded tested commit.
