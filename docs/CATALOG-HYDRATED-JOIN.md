# Catalog-to-hydrated join

`tools/exiftool-tables/join_catalog_hydrated.py` produces the first complete
source-coordinate ledger for BuildTagLookup's ordinary entries. It requires all
four explicit paths:

```sh
python3 tools/exiftool-tables/join_catalog_hydrated.py \
  --catalog "$CATALOG_SOURCE_JSON" \
  --hydrated "$HYDRATED_LAYOUT_JSON" \
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
- `source_derived_implementation` remains `not_assessed` until a separate
  generated-reader/writer selector or refusal ledger supplies evidence.
- `observed_read` and `observed_write` remain `not_observed_yet` until pinned
  ExifTool fixture evidence exists.

The tool refuses version skew, malformed native denominators or unique-name sets,
duplicate catalog coordinates, malformed hydrated table identities, and any absent
or mismatched catalog producer source in the hydrated source manifest. The catalog
manifest is intentionally a subset of the hydrated reader manifest (168 versus
182 sources in the pinned 13.59 artifacts). Missing coordinates and public-name
conflicts remain explicit records as `source_row_absent` and
`source_row_name_conflict`; a coordinate/name match is `source_row_joined`.

Each joined or conflicting record includes canonical SHA-256 hashes of its hydrated
row and containing table. These are source evidence for a future implementation
decision, not evidence that the implementation exists today.

`--check` reads and compares both existing rendered outputs without writing them.
Normal execution refuses existing destinations; `--replace` is the explicit update
mode. Output and report must be distinct and may not alias either input, including
through a hard link. Both outputs are staged with unique temporary files before
either destination is replaced.

## Published baseline and gate

The Pages report `docs/reference/catalog-hydrated-join.md` and machine ledger
`docs/public/measurements/catalog-hydrated-join-13.59.json` preserve all 33,487
ordinary entries. All have exact source matches. Every implementation state is
`not_assessed`; every observed read/write state is `not_observed_yet`. These are
explicit outstanding joins, not statements that OxiDex cannot read those tags.

CI rebuilds the ledger from the fresh audited full capture and compares every
identity, status and row/table hash against the committed ledger. Only the full
dump serialization hash is excluded from this semantic comparison; both ledger
files retain that hash as evidence provenance. The preceding graph audit checks
the complete source manifest, producer, Perl version and expected graph counts.
