# Catalog-to-hydrated join

`tools/exiftool-tables/join_catalog_hydrated.py` produces the first complete
source-coordinate ledger for BuildTagLookup's ordinary entries. It requires the
catalog, hydrated capture, and the four replay-bound QuickTime inputs:

```sh
python3 tools/exiftool-tables/join_catalog_hydrated.py \
  --catalog "$CATALOG_SOURCE_JSON" \
  --hydrated "$HYDRATED_LAYOUT_JSON" \
  --quicktime-bounded-source "$QUICKTIME_BOUNDED_SOURCE_JSON" \
  --quicktime-itemlist-ledger "$QUICKTIME_ITEMLIST_LEDGER_JSON" \
  --quicktime-source-capabilities "$QUICKTIME_CAPABILITIES_JSON" \
  --quicktime-itemlist-rust "$QUICKTIME_ITEMLIST_RUST" \
  --output "$JOIN_JSON" \
  --report "$JOIN_REPORT_MD"
```

The join key is `(table full name, raw key, variant index)`. It checks the
catalog public-name identity against the hydrated row `Name` after the exact
coordinate match. It never joins a row on its name alone.

Every native ordinary catalog entry produces one ledger record with three independent
axes:

- `source_layout_status` records whether its exact hydrated row exists and has
  the same public name.
- `source_derived_implementation` records `generated_reader_declaration_unobserved`
  or `blocked_generated_reader_refusal` when authenticated QuickTime artifacts
  match the exact source identity. Other entries remain `source_row_not_yet_consumed`.
  A name conflict cannot inherit a generated declaration from a different tag.
- `observed_read` and `observed_write` remain `not_observed_yet` until pinned
  ExifTool fixture evidence exists.

The tool replays `quicktime_generated_specs.compile_document` and
`quicktime_atom_tables.report` from the supplied bounded QuickTime source, then
requires the supplied ledger, capabilities, and Rust artifact to match that
complete replay. The joined output records SHA-256 digests for those four inputs.
It refuses version skew, malformed native denominators or unique-name sets,
duplicate catalog coordinates, malformed hydrated table identities, and any absent
or mismatched catalog producer source in the hydrated source manifest. The catalog
manifest is intentionally a subset of the hydrated reader manifest (168 versus
182 sources in the pinned 13.59 artifacts). Missing coordinates and public-name
conflicts remain explicit records as `source_row_absent` and
`source_row_name_conflict`; a coordinate/name match is `source_row_joined`.

Each joined or conflicting record includes canonical SHA-256 hashes of its hydrated
row and containing table. A separate selector hash normalizes only proven
reader-inert wrappers: inherited group defaults, matching variant Index metadata,
and the dumper's shorthand marker. Full hashes retain the original evidence.
The generated-declaration classification does not establish observed reading;
write observations require an actual write followed by pinned native read-back.

`--check` reads and compares both existing rendered outputs without writing them.
Normal execution refuses existing destinations; `--replace` is the explicit update
mode. Output and report must be distinct and may not alias either input, including
through a hard link. Both outputs are staged with unique temporary files before
either destination is replaced.

## Published baseline and gate

The Pages report `docs/reference/catalog-hydrated-join.md` and machine ledger
`docs/public/measurements/catalog-hydrated-join-13.59.json` preserve all 33,487
ordinary entries. Every entry retains its exact source coordinate. The generated
QuickTime implementation/refusal join is separate from the remaining unconsumed
source rows, and every observed read/write state remains `not_observed_yet` until
fixture evidence is attached. Unconsumed or unobserved is not a claim that OxiDex
cannot read a tag through an existing route.

CI rebuilds the ledger from the fresh audited full capture and compares every
identity, status and row/table hash against the committed ledger. Only the full
dump serialization hash is excluded from this semantic comparison; both ledger
files retain that hash as evidence provenance. The preceding graph audit checks
the complete source manifest, producer, Perl version and expected graph counts.
