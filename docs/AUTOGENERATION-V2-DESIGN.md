# Autogeneration v2: generated conversions over a session

Decided 2026-09-18 (maintainer, after two external architecture reviews).
Supersedes the *mechanism* in `AUTOGENERATION-PLAN.md`; that document's goal
(a change to ExifTool flows in by regeneration, with no retyping) is unchanged.
Numbers below name their instrument. The expression-coverage spike
(`tools/exiftool-tables/spike/run_spike.py`, draft PR #817) landed 2026-09-18
and its numbers are filled in; they reproduce byte-for-byte on a clean tree
at `ed404074` against the 13.59 dump (sha256 `536386691b0d…`). The build order
below is read off its curves, not chosen.

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

What is *also* not the problem: the grammar. The spike shows it is small and
closed (§1). What is *not* the problem: speed. `benches/benchmark_results.md` already shows
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

**Measured (spike, 13.59 dump, 13,290 uses / 3,448 distinct):** the grammar
is small (1,301 lines) and effectively closed -- **99.7% of uses parse**
(99.9% of `expr_coverage.py`'s narrower 6,993-use frame). The residue is
**bounded, not a long tail**: 36 uses / 11 distinct, and an independent
cross-check (pinned perl 5.38.2 compiling every expression,
`spike/perl_syntax_check.pl`) shows **zero grammar holes** -- every residual
is invalid Perl in ExifTool's own source (`PrintConv => '$val m'` MXF.pm:681,
unbalanced parens in LNK.pm/CanonCustom.pm, a stray quote in JPEG.pm:143).
A further 32 uses / 20 distinct parse but call the engine (`ProcessBinaryPLIST`,
`FoundTag`, `ImageInfo`) and are not conversions; 13 uses need regex
lookahead/backreferences the `regex` crate cannot compile.

The parser is therefore **not the work**. A bare interpreter with no ExifTool
knowledge covers only 69.6% (PURE `$val` + builtins) -- *less* than today's
`exprs.py` at 75.4% (re-measured; the old 66.6% figure was stale), because
`exprs.py` already inlines about a dozen helpers. The coverage lives in §2
and §3.

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

**Measured (spike):** 285 distinct session keys. With the helper library in
place, **13 / 53 / 192 keys reach 90 / 95 / 99%** of uses. By use count:
`$$self{Model}` 734, `$self` as object 270, `FacesDetected` 166, `Make` 117,
`BitM` 96, `$count` 71, `$format` 66. Typed fields: `model`, `make`,
`byte_order`, `count`, `format`; module-specific members (`FacesDetected`,
`BitM`, …) live in the map. All session keys alone (no helpers) lift Frame A
from 69.6% to 77.5% -- the session matters, but chiefly as what helpers and
conditions read through.

### 3. Helper library

ExifTool expressions call ExifTool subs (`PrintExposureTime`,
`ConvertUnixTime`, `PrintFraction`, `PrintAFPoints*`, …). Each is ported to
Rust **once** and selected by exact match of the sub's source, never by version
label -- the mechanism already landed for `ConvertUnixTime` (#805) and the
AF-point helpers, including per-release variants where ExifTool's own
behaviour changed between releases.

**Measured (spike):** **157 distinct helper subs** (155 resolvable in the
pinned source): 103 pure functions of their arguments, 52 read `$self`, 16
drive the engine; plus 41 module-level data tables. With the session model,
**6 / 22 / 113 helper ports reach 90 / 95 / 99%** of uses; session + top 25
helpers takes Frame A from 75.4% to **97.1%** and distinct expressions from
53.0% to **91.8%** -- the win is the long tail of one-off expressions. oxidex
has **10 complete + 4 partial** ports today (partial = one branch only:
`ConvertDateTime` as identity, `Decode`/UCS2, `ConvertFileSize` default
ByteUnit, `ToDMS` default CoordFormat). The spike caught and removed a wrong
mapping (`Exif::ConvertFraction → print_fraction` is the inverse direction),
which would have credited a port for the #2 helper (191 uses) that does not
exist -- exactly the "confident wrong number" the doctrine is about.

**This is the work.** The build order is: session (§2) + the top 22 helpers,
each ported once and verified differentially against the oracle.

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

1. ~~Spike lands → fill the TBD slots.~~ Done 2026-09-18 (#817). Build
   order: session with typed `model`/`make`, then the top 22 helpers by use.
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
