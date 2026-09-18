# Autogeneration v2: generated conversions over a session

Decided 2026-09-18 (maintainer, after two external architecture reviews).
Supersedes the *mechanism* in `AUTOGENERATION-PLAN.md`; that document's goal
(a change to ExifTool flows in by regeneration, with no retyping) is unchanged.
Numbers below name their instrument; slots marked **TBD(spike)** are filled by
`tools/exiftool-tables/spike/COVERAGE.md` when the expression-coverage spike
lands, so the build order comes from measurement, not preference.

## Why the current mechanism cannot reach 100%

Verified at `efe8c062`:

- **No `$self`.** The runtime `Ctx` (`src/exiftool_tables/cond.rs:97`) carries
  only same-table `members`, `$$valPt`, `format` and `count`. Any `Condition`,
  `ValueConv` or `PrintConv` that reads `$$self{Model}`, another tag, or
  directory state cannot be generated at all, however capable the transpiler.
- **Template transpilation.** `exprs.py` (~90 KB of string templates) and
  `conds.py` (seven closed condition shapes) grow coverage one expression at
  a time. Last recorded translated share: 66.6% of expression uses
  (`expr_coverage.py`, older commit; re-measured by the spike).
- **All-or-refuse at table granularity (Gate A).** A table with one
  unmodelable field cannot be enabled without dropping tags the hand parser
  already reads, so `conformance.py` (Gate B) rejects it. The workaround is
  replay shims such as `src/parsers/tiff/makernotes/canon/main_engine.rs`.
- **Four engines, one pipeline.** `engine.rs`, `ifd_engine.rs`,
  `keyed_engine.rs`, `serial_engine.rs` each re-implement the conversion
  stage. ExifTool itself has 230 distinct `PROCESS_PROC`s (`rg` over the
  pinned 13.59 `lib/`) but exactly one tag pipeline beneath them.
- **Post-hoc formatting.** `src/core/exiftool_compat.rs` (3,294 lines, 50
  `base_name == "..."` branches) formats by bare tag name after extraction;
  line 592 applies TIFF `Compression` names to any integer tag so named.
- **Half-migrated sink.** 3,157 legacy `.insert()` calls vs 164 structured
  (`insert_occurrence` + `insert_with_group1`), so most tags carry no
  ValueConv, and `src/composite/compute.rs` re-parses display strings (7
  sites).
- **Monoliths.** `binary_tables.rs` 6.8 MB, `ifd_tables.rs` 4.4 MB.

What is *not* the problem: speed. `benches/benchmark_results.md` already shows
16x (single JPEG) and 65x (1000-file batch) over Perl ExifTool, measured
against 13.36 at an early build; the dispatch-perf spike re-measures at the
pin. Compiling to `match` arms is chosen for coverage and idiom, not because
the table walk is known to be hot.

## The shape

Mirror ExifTool's own split: thin per-walker code, **one** shared tag
pipeline, generated conversions that take a session.

```
Perl tables ──dump──▶ declarations + expression ASTs (real grammar)
                                   │
                    ┌──────────────┴──────────────┐
                    ▼                             ▼
          codegen: Rust `match` arms      verify_exprs: same AST,
          per table, per module           evaluated against the Perl oracle
                    │
                    ▼
   walkers (ProcessExif, ProcessBinaryData, …) ──▶ tag pipeline(&mut Session)
                                                              │
                                                Condition / conversions / helpers
                                                              │
                                              TagSink (raw, ValueConv, PrintConv)
```

### 1. Grammar front end

A recursive-descent parser for the Perl subset ExifTool tables use, producing
an AST. It replaces `exprs.py`'s templates and `conds.py`'s shapes; coverage
grows per grammar production, shared by every expression. Anything outside the
grammar is refused per field and counted, never approximated.

**TBD(spike):** parseable share of uses; residue size and character (bounded
vs long tail).

### 2. `Session`

Passed by `&mut` through every walker; the `$self` equivalent.

- Typed fields for high-traffic members (`make`, `model`, `byte_order`,
  `tiff_type`, `file_type`, `dir_info { base, start, data_pos }`), plus a
  compact map for the rest: `enum MemberVal { Str, Int, Float, Bool, Undef }`.
- Perl truthiness is a method (`is_truthy`), not a Rust `bool` coercion:
  `0`, `""` and `undef` are false, everything else true.
- `values`: already-extracted tags, what `GetValue`/`$$self{VALUE}` reads.
- `processed`: the cycle guard.

`$$self{...}` slots are dynamically typed in Perl; the generator infers a type
per key from every use site and refuses a key used inconsistently.

**TBD(spike):** the session keys by use count; which get typed fields.

### 3. Helper library

ExifTool expressions call ExifTool subs (`PrintExposureTime`,
`ConvertUnixTime`, `PrintFraction`, `PrintAFPoints*`, …). Each is ported to
Rust **once** and selected by exact match of the sub's source, never by version
label -- the mechanism already landed for `ConvertUnixTime` (#805) and the
AF-point helpers, including per-release variants where ExifTool's own
behaviour changed between releases.

**TBD(spike):** helper count; the curve (top 10 / 25 / 50) to 90 / 95 / 99%
of uses; how many already have ports.

### 4. Backend: AST → Rust

Per table, a generated `fn decode(&mut Session, id, val, &mut TagSink) ->
bool` whose body is `match id { … }` with conversions inlined, helper calls
resolved to the library, one generated file per ExifTool module.

Regexes: `LazyLock<Regex>` by default. A fold to a native form
(`/^Canon/` → `starts_with`) is an optimization the backend may apply **only**
when `verify_exprs` proves the fold equivalent against the oracle for that
pattern. Folds are earned per pattern.

### 5. Extraction-time formatting

`PrintConv` runs inside the arm, from the table that owns the tag, and the
arm writes the (raw, ValueConv, PrintConv) triplet to the sink. Each table
that formats its own tags retires the `exiftool_compat.rs` branch that used to
guess for it; the file is deleted branch by branch, never rewritten.

Composite follows: once the sink carries real ValueConv, `compute.rs` becomes
arithmetic over numbers instead of re-parsing `"0.019 mm"`.

## The gate change

Gate A (table-level all-or-refuse) is replaced by **per-field mixed mode**:

```rust
for entry in ifd {
    if !generated::decode(&mut session, entry.id, entry.val, &mut sink) {
        residual::decode(&mut session, entry.id, entry.val, &mut sink);
    }
}
```

The generated decoder takes every field it can prove; the hand parser keeps
only the refused ones, listed explicitly and counted. Dispatch stays in native
IFD order, so there is nothing to buffer or replay. Gate B ("no proven read is
lost") is enforced on every PR by the corpus read-regression gate (#814).

## What stays hand-written, and how it is counted

Container walkers (`ProcessMOV`, `ProcessJPEG`, RIFF, PDF, OLE, …) are
procedural Perl, not tables. Each is ported 1:1 from a **named** Perl sub,
fenced in its own module, and enumerated. "100% generated" therefore means:
every tag definition, layout, condition and conversion generated from source;
walkers hand-ported and listed. The ratchet tracks both counts.

## Sequencing

1. Spike lands → fill the TBD slots; the helper and session-key build order is
   read off its curves.
2. Session + grammar + backend, exercised on **`Exif::Main`** end to end. It is
   already on the generated engine, dominates output (the 38.34% generated
   share at `72eae8a5` came mostly from ExifIFD), and touches Session and the
   sink directly.
3. Delete the hand code and compat branches `Exif::Main` makes redundant. #814
   proves zero proven reads lost; re-run the generated-share census
   (`~/oxidex-ops/genshare/`, probe method in memory `oxidex-generated-share-measured`).
4. If the share moves and nothing is lost, the architecture is validated; the
   remaining tables are throughput.

## Risks

- Per-key type inference for `$$self{...}` (see §2).
- `ProcessBinaryData` dynamics: `DataMember`, `Hook`, `RawConv` side effects,
  `%val` lookback. The spike's session-key list shows which matter.
- Encrypted/enciphered walkers (Nikon, Sony) genuinely differ across releases
  (#808); they stay per-release ports, honestly omitted where unmodelled.
- `keyed`/`serial` engines: fold into the shared pipeline last; their walkers
  are small.

## Independent tracks, not on the critical path

- Per-module split of the generated monoliths (mechanical, byte-identical;
  in progress).
- Zero-allocation `Tag`-enum sink. Good, orthogonal; after the pipeline exists.
