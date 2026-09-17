# Catalog-to-hydrated join

`tools/exiftool-tables/join_catalog_hydrated.py` produces the first complete
source-coordinate ledger for BuildTagLookup's ordinary entries. It requires the
 catalog, hydrated capture, ItemList replay inputs, and replay-bound Keys ledger/Rust inputs:

```sh
python3 tools/exiftool-tables/join_catalog_hydrated.py \
  --catalog "$CATALOG_SOURCE_JSON" \
  --hydrated "$HYDRATED_LAYOUT_JSON" \
  --quicktime-bounded-source "$QUICKTIME_BOUNDED_SOURCE_JSON" \
  --quicktime-itemlist-ledger "$QUICKTIME_ITEMLIST_LEDGER_JSON" \
  --quicktime-source-capabilities "$QUICKTIME_CAPABILITIES_JSON" \
  --quicktime-itemlist-rust "$QUICKTIME_ITEMLIST_RUST" \
  --quicktime-keys-ledger "$QUICKTIME_KEYS_LEDGER_JSON" \
  --quicktime-keys-rust "$QUICKTIME_KEYS_RUST" \
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

The same CI join also supplies the IFD compiler source dump, identity ledger,
rendered Rust artifact, and expression oracle ledger. This makes IFD schema
declaration accounting part of the deterministic source denominator; it remains
an unobserved declaration until an independent dispatch and fixture instrument
is available.

## Generated Garmin FIT reader

`--garmin-fit-source`, `--garmin-fit-ledger`, `--garmin-fit-rust` and
`--garmin-fit-protocol-fact` supply the bounded Garmin source fixture, the FIT
ledger, `src/exiftool_tables/fit_tables.rs` and a fresh
`capture_garmin_fit_fact.pl` capture of the pinned tree.
`catalog_garmin_fit.py` accepts them only when:

- the fixture's Garmin module equals the Garmin module of the authenticated full
  dump (`--ifd-source`);
- the fixture's protocol fact (base types, format sizes, integer width, code
  bodies) equals the fresh capture;
- the fixture's ProcessFIT source digest equals the catalog's pinned `Garmin.pm`;
- the ledger and Rust replay exactly through `garmin_fit_specs.generate`, with
  conversions admitted by that dump's source-bound expression ledger
  (`--ifd-expr-ledger`); and
- the replayed protocol is admitted.

Every Garmin catalog row must then match a replayed row by exact coordinate and
name. The FIT classification supersedes IFD schema candidacy for those rows:

| FIT row | `reader_implementation` |
| --- | --- |
| generated, reached in ExifTool's default mode | `generated_reader_declaration_unobserved` |
| generated, reached natively only with the Unknown option (not exposed by OxiDex) | `generated_reader_declaration_option_gated` |
| refused | `blocked_generated_reader_refusal`, with the ledger's exact reasons |

The per-table report's reader-declaration column excludes option-gated rows.

## Native writability and the write denominator

Every record carries `catalog.native_writable`, copied from the catalog
snapshot's native TagNames Writable class. It decides `writer_implementation`
for rows without a generated writer:

| Native class | `writer_implementation` | Write-parity work |
| --- | --- | --- |
| `writable` | `writer_not_declared` or `generated_writer_declaration_unobserved` | yes |
| `not_writable` | `native_not_writable` | no |
| `writable_protected` | `native_writable_protected_indirect` | no (ExifTool writes it only indirectly) |
| `not_listed` | `native_not_listed` | no |

A generated writer declaration on a row that is not natively `writable` is
refused. `counts.write_parity` reports the direct-write denominator, generated
declarations and matched write observations over `writable` entries only, and
each source table reports `native_writable_catalog_entries`.

## Corpus read receipts

`corpus_read_receipt.py` records authenticated public-CLI reads of a whole
corpus in three steps:

- `build` records a Cargo build proof from a clean commit, keeping the Cargo
  stdout/stderr bytes and the executable they name.
- `observe` records pinned ExifTool (`-j -a -G1:4 -s`, and `-n`) and OxiDex
  (`-j -a -G1`, and `--no-print-conv`) transcripts for every file. It also
  records a `capture_corpus_sources.pl` transcript naming the exact
  `(table, tag ID, variant index)` ExifTool used for every tag, plus version,
  DOCX-capability and Perl probes and a library fingerprint.
- `verify` checks the receipt against `.exiftool-version` and recomputes every
  count and credit from the stored bytes.

Matching rules:

- An identity is a `Group1:TagName`. Native `CopyN` segments and OxiDex ` (N)`
  suffixes fold into one identity per file carrying the multiset of its values.
- JSON with a duplicate key is refused. Values compare as parsed JSON with
  numbers kept as literal text.
- An identity matches in a file only when both modes agree. Default-output-only
  matches are reported and never credited.
- A source row is credited only when every file in which ExifTool read it
  matched. An identity whose captured rows do not account for exactly its
  native values is unattributable in that file.

`--corpus-read-evidence` joins a receipt only when three things hold: its runtime
input manifest equals the joined checkout's, its ExifTool version equals the
catalog pin, and every catalog producer source has the observed library's
digest. Credited rows that are catalog coordinates become `observed_matched_read`.
Rows outside the catalog, such as table entries ExifTool adds at run time, are
counted but not credited.

The corpus snapshot is published separately as
`docs/public/measurements/catalog-corpus-observed-13.59.json` with report
`docs/reference/catalog-corpus-observed.md`. CI verifies every published
snapshot. A squash merge makes the observed runtime commit unreachable from the
target branch, so it is kept at `staging/corpus-read-receipt-history`. The
runtime input manifest, not the commit, binds the observation to source.

## Historical native observations

`docs/public/measurements/catalog-hydrated-observed-13.59.json` is a separate
Pages artifact when authenticated native receipts are available. It contains
the full source-coordinate join with observed states, its exact source-join
SHA-256, the source and runtime commits, and the named native instrument. It is
explicitly historical evidence, not a claim that a newer source/runtime has
the same coverage. Its companion human report is
`docs/reference/catalog-hydrated-observed.md`.

Publish it only through the catalog join while that process is validating its
native receipts. `catalog_observed_snapshot.py` intentionally has no command
that accepts a standalone observed join as proof.

Append these options to the complete join command above:

```sh
  --quicktime-read-evidence "$AUTHENTICATED_QUICKTIME_RECEIPT" \
  --writer-read-evidence "$AUTHENTICATED_WRITE_RECEIPT" \
  --observed-snapshot docs/public/measurements/catalog-hydrated-observed-13.59.json \
  --observed-report docs/reference/catalog-hydrated-observed.md
```

CI validates the embedded historical source ledger, source/declaration axis,
receipt bindings, observation counts, and native provenance without requiring
historical native fixtures on every runner. It also reports whether the current
source ledger still matches. A current difference retains the receipt as
historical evidence; it cannot silently drop or re-credit observations.
