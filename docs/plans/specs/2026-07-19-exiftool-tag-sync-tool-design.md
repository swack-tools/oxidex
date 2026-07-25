# ExifTool Tag Sync Tool — Design (Phase A)

## Context

OxiDex's tag definitions (`oxidex-tags-*/src/*_tags.yaml`, ~32,684 tags across
6 domain crates) are supposed to be generated from ExifTool's own tag
database. In practice, that pipeline is broken and has been for months:

- The generator lives inside the root `build.rs`, gated behind "skip if
  `src/tag_db/generated_tags.rs` already exists" — so it never runs during a
  normal `cargo build`.
- It downloads ExifTool's `master` branch as a zip from GitHub and parses the
  raw Perl source with ~825 lines of regex. This only extracts a tag's
  `Writable` type when the tag uses the explicit hash form
  (`0x0100 => { Name => 'X', Writable => 'int16u' }`). ExifTool commonly
  relies on table-level `WRITABLE => 'string'` defaults inherited by every tag
  in that table unless overridden — the regex parser cannot see this. Result:
  only 366 of 32,684 tags (1.1%) currently carry a `type` value at all.
- The one real scheduled run (`.github/workflows/sync-exiftool-tags.yml`,
  2025-12-07) silently fell back to the manual registry, produced a diff of
  zero YAML changes, committed a stray `build.log`, and its branch
  (`auto-sync-exiftool-tags`) was never merged.
- The committed YAML was last hand-edited (`2bac464`, 2025-12-08), not
  produced by any automated sync, and targets an older ExifTool than what's
  installed locally (13.55).

Separately, `scripts/generate_exiftool_manifest.py` already solves the "get
authoritative tag metadata out of ExifTool" problem correctly, for the JPEG
subset used by the tag matrix harness: it shells out to
`exiftool -f -listx`, ExifTool's own structured XML tag dump, which reports
each tag's fully-resolved `writable`/type/group — table-level inheritance
already applied by ExifTool itself. This design generalizes that proven
approach to the full tag database, replacing the Perl-regex parser entirely.

This is Phase A of a four-phase project (agreed with the user in
brainstorming):
- **A (this doc)**: standalone sync tool, `exiftool -listx`-based, enriched
  YAML with real type data.
- **B**: CI workflow that invokes the tool, verifies the diff, opens a PR.
- **C**: rewire the write path (validation/serialization) to consume
  YAML-derived type data instead of the manual `TAG_REGISTRY`; delete the
  manual registry once parity is confirmed.
- **D**: generalize read/write coverage verification (in the spirit of
  `jpeg_tag_matrix.py`) across all formats, wired into CI as a regression
  gate.

Phases B–D are out of scope for this document and will get their own specs.

## Goals

- Produce a tool that regenerates `oxidex-tags-*/src/*_tags.yaml` from a
  locally-installed `exiftool` binary, capturing `id`, `name`, `writable`,
  `type` (ExifTool's raw type string, verbatim), and `description` per tag.
- Make the tool explicit and side-effect-free with respect to `cargo build`
  — it is invoked deliberately, never implicitly during compilation.
- Make failures loud: no silent fallback that reports success with no real
  change.
- Track ExifTool by released version number, not an arbitrary `master` git
  SHA.

## Non-goals

- Rewiring the write path to consume the new `type` data (Phase C).
- Building/fixing the CI workflow that invokes this tool (Phase B).
- Broad read/write coverage verification across formats (Phase D).
- Preserving the old Rust-codegen path (`generated_tags.rs` as a real tag
  table) — it's already a thin compatibility facade delegating to
  `tag_registry::get_tag_descriptor`, and stays that way until Phase C.

## Architecture

A new binary, `src/bin/sync_tags.rs`, built and run explicitly:

```
cargo run --release --bin sync-tags
```

It has no build-time hook and is never invoked from `build.rs`. It requires
an `exiftool` binary on `PATH` and does nothing else automatically — no
network access, no downloading ExifTool source. Installing/upgrading the
`exiftool` binary itself (via package manager or release tarball) is treated
as a separate, prior step, owned by whatever invokes this tool (a developer
locally, or the Phase B CI workflow).

This removes the need for the current `build.rs` download/unzip/cache-in-
`OUT_DIR` machinery (~150 lines) and the 825-line Perl regex parser. Once
Phase A lands, `build.rs` goes back to being a normal build script with no
network or parsing responsibilities.

## Data flow

1. Run `exiftool -f -listx` and capture XML from stdout. The `-f` flag
   matches `generate_exiftool_manifest.py`'s existing behavior (includes tags
   otherwise excluded from `-listx`'s default output) — using it here keeps
   this tool's tag universe a superset of what the JPEG matrix harness
   already validates against.
2. Parse the XML with `quick-xml` — already a workspace dependency (used
   elsewhere for XMP parsing), so this adds no new crate. Like
   `generate_exiftool_manifest.py`'s use of `defusedxml`, it does not resolve
   DTDs or external entities, so it carries the same XXE-hardening by
   construction.
3. For each `<tag>` element, read: `name`, the enclosing `<table name="...">`
   (e.g. `Canon::Main`, `Exif::Main`), `writable` attribute (present and not
   `"false"`/`"0"` → writable), `type`/`g0`/`g1` attributes, and `desc`/`num`
   as available. ExifTool has already resolved table-level `WRITABLE`
   inheritance by the time it emits this XML.
4. Route each tag's table name to a domain crate (`core`, `camera`, `media`,
   `image`, `document`, `specialty`) using the domain-routing table ported
   verbatim from `build.rs`'s `get_domain_for_table()` — this logic is
   unrelated to the parsing-strategy change and doesn't need to be redesigned.
5. Emit YAML per domain into `oxidex-tags-{domain}/src/{domain}_tags.yaml`,
   matching the existing schema
   (`tables: [{name, tags: [{id, name, writable, type, description}]}]`,
   `oxidex-tags-shared::types::TagDatabase`). Table and tag ordering is
   sorted for deterministic, diffable output (matches current behavior).
6. Overwrite the checked-in YAML files in place. No separate staging/review
   file — the diff itself, produced by a normal `git diff` after running the
   tool, is the review surface (and, in Phase B, the CI-generated PR).

## Type field

The YAML `type:` field stores ExifTool's raw type string
(`int16u`, `rational64s`, `string`, `string[32]`, `date`, `undef`, etc.)
verbatim — no coarsening to oxidex's 7-variant `ValueType` enum
(`String`/`Integer`/`Float`/`Rational`/`Binary`/`DateTime`/`Struct`) at
generation time. That mapping belongs to Phase C (write-path rewire); doing
it here would throw away precision Phase C needs (e.g. distinguishing
`int16u` from `int32u` for correct serialization width).

## Version tracking

`.exiftool-version` is repurposed to hold a plain ExifTool release version
string (e.g. `13.55`), read via `exiftool -ver` on the binary used to
regenerate — replacing the current git-commit-SHA tracking of ExifTool's
`master` branch. This is both more reproducible (pinned releases instead of
an arbitrary moving target) and matches how ExifTool is actually installed
in practice (apt/homebrew/release tarball, all versioned by release number).

## Error handling

- `exiftool` missing from `PATH`, or `-listx` output empty/unparseable →
  hard error, non-zero exit, no fallback. This is a deliberate change from
  today's behavior, where generation failure silently fell back to the
  manual registry and still reported success.
- Per-tag parse failures (a malformed XML fragment, an unexpected attribute
  shape) → skip that tag, log a warning identifying table + tag name,
  continue. One bad tag must not abort the whole run.
- Post-generation sanity check: total tag count must be at least 90% of the
  count from the previous run (read from the existing committed YAML before
  overwriting). Below that threshold, the tool errors out before writing,
  flagging a likely parsing regression rather than silently committing a
  large drop in coverage.

## Testing

- Unit tests for domain routing (`get_domain_for_table`), ported from
  existing `build.rs` coverage if present.
- Unit tests for XML → tag-definition parsing against small fixture
  `-listx` snippets covering: hash-form tags, simple-form tags, table-level
  `writable` inheritance (the exact case the old parser missed), and
  missing/optional fields.
- Idempotency test: running the tool twice against the same installed
  `exiftool` version produces byte-identical YAML output.
- A smoke test, gated on `exiftool` being present on `PATH` (skipped
  otherwise), that runs the real binary and asserts both the total tag count
  and the type-coverage percentage exceed today's baseline (32,684 tags /
  1.1% typed). This is the concrete, measurable proof this phase worked.

Full read/write coverage verification across all formats (the general form
of `jpeg_tag_matrix.py`) is Phase D's responsibility, not this tool's — this
phase's testing only needs to prove the generator itself is healthy.
