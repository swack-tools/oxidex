# Architecture

OxiDex reproduces ExifTool's output by reading ExifTool's own source. The
tag tables, byte layouts, selection conditions and value conversions are
dumped from the pinned Perl modules and generated into Rust. Procedural
code, such as the container walkers that find those tables inside a file,
is hand-ported from named Perl subroutines. Anything the generator cannot
model is refused and counted, never approximated.

This page describes the structure at the `refactor/tag-machinery` tip. The
measured state lives on the [status page](/status/) and in the
[autogeneration plan](/AUTOGENERATION-PLAN). This page does not repeat those
numbers except where a design decision depends on them.

## The read path

```
file ──▶ detection ──▶ format dispatch ──▶ walker (hand-ported)
          │                                     │
          │  src/filetype (generated from       │  finds directories, calls
          │  ExifTool's type and magic tables)  ▼
          │                           generated tables + engines
          │                           (src/exiftool_tables)
          ▼                                     │
   File:FileType, MIMEType ...                  ▼
                                   TagSink: every occurrence, with
                                   Group0 / Group1, raw and converted
                                                │
                              PrintConv formatting, Composite tags
                                                │
                            CLI output (text, -j, -csv) / library map
```

1. **Detection.** Signature detection lives in `src/parsers/detection`, and
   `src/filetype` holds identification tables generated from ExifTool's own
   extension and magic-number tables (`dump_filetypes.pl`,
   `codegen_filetypes.py`). An optional Magika detector is available with
   `--features magika` and `--detector=magika`.
2. **Format dispatch.** `src/core/format_dispatch.rs` routes each variant of
   the `FileFormat` enum (`src/core/file_format.rs`) to a parser. Camera RAW
   is a single variant covering 36 RAW sub-formats. A file that detection
   cannot map to a variant still gets its `File:` identity tags. This is the
   "detected is not parsed" case: a correct `FileType` with nothing else
   behind it. See [Supported formats](/reference/formats/) for which formats
   are in which state.
3. **Walkers.** Container walkers (JPEG segments, TIFF/IFD chains,
   QuickTime atoms, RIFF, PDF, OLE and others) are procedural Perl in
   ExifTool, not tables. They are hand-ported under `src/parsers/`, and each
   is a port of a named Perl subroutine. The MakerNote dispatcher
   (`src/parsers/tiff/makernote_dispatcher.rs`) chooses a vendor's tables by
   signature or `Make`.
4. **Generated tables and engines.** `src/exiftool_tables/` holds the
   transcribed tables, one file per ExifTool module (#823). Separate engines
   evaluate them: binary-data (`engine.rs`), IFD (`ifd_engine.rs`), keyed
   (`keyed_engine.rs`), serial (`serial_engine.rs`) and FIT. Each engine
   evaluates a table's `Condition`s and conversions. The `enabled*.rs` lists
   record which generated tables are live. The rest are generated and
   verified, but still shadowed by a hand parser.
5. **The sink.** `src/core/tag_sink.rs` keeps every occurrence of every tag
   with its family 0 and family 1 groups, and marks which occurrence wins
   priority. The CLI's `-a` shows every occurrence, and `-G`/`-G1` print the
   groups. Library results are keyed by group-qualified names such as
   `IFD0:Make`.
6. **Formatting and Composite.** ExifTool's `PrintConv` formatting is on by
   default (`--no-print-conv` turns it off). Composite tags are computed in
   `src/composite/`. Part of that computation is generated from ExifTool's
   Composite tables.

## Where the tables come from

The ExifTool release named in `.exiftool-version` (**13.59**) is the only
source of truth. The Rust oracle (`src/exiftool_oracle.rs`) compiles it in,
the Python oracle (`scripts/exiftool_oracle.py`) reads it, and CI and the
justfile fetch that exact tag. Grading against any other ExifTool is not
evidence. Two releases can disagree about which sub-table a given byte count
selects, so a version skew produces phantom regressions and phantom fixes
alike.

```
pinned ExifTool Perl source
   │  dump_tables.pl and the other dump_*.pl scripts load the real modules
   ▼
JSON dump of tables, layouts, conditions, conversions
   │  codegen*.py (tools/exiftool-tables/)
   ▼
src/exiftool_tables/**, src/filetype, src/composite, src/writers/generated_*
   │  verify.py, verify_exprs.py, verify_subdirs.py: re-evaluated against live Perl
   ▼
just verify-tables   (CI: "Verify Generated Tables")
```

- `just regen-tables` regenerates tier 1 (the core tables).
  `just regen-tables-all` also regenerates tier 2: MakerNote sub-directory
  tables, the Nikon AF-point grids and the `scripts/gen_*.pl` transcriptions.
- CI regenerates from the pin and fails on any difference, so a stale or
  hand-edited generated file cannot land.
- The generator obeys one rule: **never approximate a conversion.** A field
  it cannot model is left out and counted. A table that looks short is
  therefore evidence of an untranscribed field, not of a missing tag. Diff
  it against the `%Image::ExifTool::<Module>::<Table>` hash in the pinned
  source.

[Transcription](/TRANSCRIPTION) records the method and its history.
[BinaryData engine](/reference/binary-data-engine) records the engine's
original design.

### Tag definitions are not tag coverage

The six `oxidex-tags-*` crates hold 16,684 tag definitions, synced from
`exiftool -f -listx`. That is ExifTool's *documentation* view: name, ID,
type, writability and description, with no layout, `SubDirectory`,
`Condition` or conversion. The definitions say a tag exists, not that OxiDex
reads it. A rising definition count is not rising coverage. Only a
comparison run measures coverage. See [Tag database](/architecture/tag-database).

## Writing

`src/core/operations.rs` routes writes by format:

- **JPEG**: the EXIF APP1 segment only.
- **TIFF and TIFF-based RAW**: a surgical, in-place TIFF writer.
- **PNG**: text and `eXIf` chunks.
- **PDF**: an appended Info-dictionary revision.

Every write goes through a temporary file, an fsync and a rename
(`src/writers/atomic_writer.rs`). The `src/writers/generated_*` modules come
from ExifTool's write-side routines (`WriteValue`, `CheckValue`,
`SetNewValue`, the inverse conversions). They stay crate-private until public
file parity is proved. What is proven today is in
[Writing metadata](/guide/writing).

## Upgrading ExifTool

The goal is that a new ExifTool release flows in by regeneration, with no one
retyping a tag rule. Two instruments exercise that:

- `just bump-exiftool <version>` writes the pin, fetches and probes the new
  release, regenerates every tier and verifies it. It then classifies every
  declaration delta and runs a conformance double-run with floors.
  `--dry-run` reverts everything afterwards. The retrospective 13.55 → 13.59
  run passed with no source edits:
  [bump report](/reference/bump-reports/13.55-to-13.59).
- **The release-pair rehearsal** regenerates, builds, reads and writes
  against two much older releases, 11.78 and 12.64, each checked against
  its own native ExifTool. The first end-to-end run (#826) passed
  generation, expression verification and the release build, but only with
  two local interventions per release. It found two structural problems:
  the test suite asserts 13.59-specific facts, and hand parsers keep 13.59
  behaviour on an older pin. Both are now tracked as plan items. See
  [the rehearsal record](/reference/upgrade-rehearsal-11.78-12.64).

## The v2 direction

::: info In progress on `refactor/tag-machinery`
Approved 2026-09-18. Specified in the [v2 design](/AUTOGENERATION-V2-DESIGN),
sequenced in the [plan](/AUTOGENERATION-PLAN), and tracked on the
[progress page](/AUTOGENERATION-PROGRESS).
:::

The current mechanism cannot reach full generation. Its runtime context has
no equivalent of ExifTool's `$self`, so it cannot express a conversion that
reads the camera model or another tag. Its template translators grow one
expression at a time. Four engines each re-implement the conversion stage.
Formatting happens after extraction, keyed by bare tag name, in
`src/core/exiftool_compat.rs`. And a table with one unmodellable field
cannot be enabled at all.

v2 mirrors ExifTool's own structure. Thin per-walker code feeds **one**
shared tag pipeline, and generated conversions run over a **`Session`**:

- **A real grammar** for the Perl subset ExifTool's tables use. It replaces
  the string templates, and anything outside it is refused per field.
- **A `Session`** that models `$self`, with typed model, make and byte-order
  fields plus a map for the rest, and with Perl truthiness.
- **A helper library** of ExifTool subroutines, each ported once and each
  matched to its exact source.
- **Codegen to Rust `match` arms** per table and module. The same AST is
  verified against the Perl oracle by `verify_exprs`.
- **Per-field mixed mode.** The generated decoder takes every field it can
  prove, and the hand parser keeps only the listed, refused fields. That
  lets hand parsers retire field by field, without losing a proven read.
  The corpus read-regression gate enforces this on every PR.

Step 1 is in progress. #824 landed the `Session` and 17 helper ports,
proven byte-identical against the pinned Perl. #829 added `Decode`/`Encode`
over Perl byte strings. The `Session` is internal and is not part of the
public library API.

## Guarantees

CI protects the parts that are already correct:

- **Corpus read-regression gate.** A PR that loses a read the published
  snapshot proved fails, and the lost entries are named.
- **Parity ratchet.** 25 committed counts may move only in the good direction.
- **Generated-table verification.** A regeneration drift or a
  verification mismatch fails the check.
- **Benchmarks.** They run on every push to the tip. They are indicative
  only and do not block.

The details are on the [contributing page](/contributing/#what-ci-enforces).

## Library layout

| Path | Role |
| --- | --- |
| `src/main.rs`, `src/cli/` | the `oxidex` command line: argument parsing, tag resolution, output formatting, rename |
| `src/core/` | the public read and write API (`operations.rs`, `metadata.rs`), `MetadataMap`, `TagValue`, the sink, dispatch |
| `src/parsers/` | detection, walkers and per-format parsers |
| `src/exiftool_tables/` | generated tables, engines, `Session` and helpers |
| `src/composite/` | Composite tags |
| `src/writers/` | format writers and the generated write-side routines |
| `src/ffi/` | the C API (`exiftool_*` symbols, header generated by cbindgen) |
| `src/tag_db/`, `oxidex-tags-*` | tag definitions (the documentation view) |
| `tools/exiftool-tables/` | dump, codegen, verification, conformance and rehearsal tooling |
| `tools/ci/` | the ratchet, the read-regression gate and the CI guards |

## Further reading

- [Tag database](/architecture/tag-database) and [oxidex-tags-shared](/architecture/oxidex-tags-shared)
- [Parser shared infrastructure](/architecture/parser-shared-infrastructure) and the [parser migration guide](/architecture/parser-migration-guide)
- [Transcription](/TRANSCRIPTION), [Tag machinery status](/TAG_MACHINERY_STATUS)
