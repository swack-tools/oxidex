# Transcription: closing the ExifTool gap mechanically

OxiDex is a Rust reimplementation of ExifTool. This document describes the
**transcription methodology** — a way of closing the compatibility gap by
generating code from ExifTool's own data structures, and by measuring the gap
precisely enough to know which work is expensive and which is nearly free.

It is intended to be **the default way coverage work is done**, with
[the AI harness](./AI_HARNESS.md) reserved for the residue that genuinely
resists it. The reasoning behind that ordering is in
[Relationship to the AI harness](#relationship-to-the-ai-harness).

::: warning A note on numbers
Every figure below is followed by how it was produced. Coverage percentages are
measured against ExifTool's own 190-file test corpus (`t/images` in the ExifTool
distribution), which is deliberately exotic — it is a *harsher* yardstick than a
normal photo library, so these numbers are not comparable to the
[comparison report](/reference/comparison/), which uses a different corpus. What
matters is that the same corpus is used before and after every change.

Soundness and completeness are always reported separately. Neither is allowed to
stand in for the other. That conflation is how the project previously arrived at
"58% parity" while extracting 48.8%.
:::

## The premise

ExifTool is not 16,000 hand-written tag implementations. It is a generic engine
plus roughly 1,300 declarative tag tables, and those tables are *data*:

```
1,281 tables, 27,747 tag entries, across 146 modules
extracted in 1.3 seconds
```

Data can be transcribed. It does not have to be reimplemented, inferred from
sample files, or rediscovered a tag at a time. The central claim of this
methodology is that **most of the remaining gap is transcription, and
transcription should never cost a model call.**

Classifying all 27,747 extracted entries by how safely they can be generated:

| tier    | count  | share | meaning                              |
| ------- | ------ | ----- | ------------------------------------ |
| pure    | 18,690 | 67.4% | no conversions; pure transcription   |
| enum    | 4,480  | 16.1% | `PrintConv` lookup maps; pure data   |
| expr    | 3,993  | 14.4% | Perl expression; needs a translation |
| code    | 210    | 0.8%  | Perl code ref; needs real porting    |
| variant | 374    | 1.3%  | `Condition` dispatch; needs a port   |

**83.5% is mechanically safe.** And the remaining tail is shallower than it
looks: those 3,993 expression tags share only **1,409 distinct expressions**, and
the twenty most common cover 1,535 tags. Run `analyze.py` to see the ranked list.

## Why `-listx` was not enough

`src/tag_sync` already ingests `exiftool -f -listx`, which is why OxiDex can say
it knows 16,677 tag *names*. Knowing a name is not enough to read one. `-listx`
is the documentation view, and it discards exactly what a reader needs:

| needed to read a tag         | in `-listx` | in the Perl tables |
| ---------------------------- | ----------- | ------------------ |
| tag name / id                | yes         | yes                |
| `FORMAT`, `FIRST_ENTRY`      | **no**      | yes                |
| per-field `Format` override  | **no**      | yes                |
| `SubDirectory` → `TagTable`  | **no**      | yes                |
| `ValueConv` / `RawConv`      | **no**      | yes                |
| `Condition` variants         | **no**      | yes                |
| `Mask`, `DataMember`, `Hook` | **no**      | yes                |

The missing rows are the MakerNote byte layout. That is why tag *coverage*
trailed tag *knowledge*, and why the gap concentrated in JPEG and the RAW
formats specifically. It was never a knowledge problem.

## Extraction: load, don't parse

`tools/exiftool-tables/dump_tables.pl` does **not** parse Perl. ExifTool builds
its tables at `require` time — some are assembled in loops, some inherit by
copying another table, some are patched afterwards. Any regex over the `.pm`
text sees the source, not the structure ExifTool actually dispatches on.

Instead the script loads each module and walks the symbol table, so what it
reads is the real in-memory table. This is both more correct and far cheaper:
full extraction of all 146 modules takes about 1.3 seconds.

Two consequences worth knowing:

- **Code refs are resolved to their names.** `PROCESS_PROC` is how a table
  declares what kind of thing it is. Recording it as an opaque `CODE` loses
  that, and the generator then cannot tell a `ProcessBinaryData` table from an
  IFD. This mattered: detecting binary tables by the presence of `FORMAT`
  instead of by `PROCESS_PROC` silently skipped **365 tables / 4,844 tags**,
  because ExifTool does `$$tagTablePtr{FORMAT} || 'int8u'` and those tables
  simply omit it. BMP and ICO were among them.
- **One table is not reachable this way.** `%fileTypeExt` is a lexical `my`
  hash, invisible to the symbol table. Rather than retype its nine pairs by
  hand — the exact manual transcription this tooling exists to avoid — the
  literal is sliced out of `ExifTool.pm` and `eval`'d by Perl, with a hard error
  if the shape changes. The parsing is Perl's; the regex only finds the
  boundaries.

## The rule: never approximate

**A conversion is emitted only if its exact expression is registered in
`exprs.py`. Anything else is dropped and counted.**

This is deliberate under-claiming, and the reason is asymmetry. A wrong
`PrintConv` does not crash. It emits a confident, plausible, wrong number under
a genuine ExifTool tag name, into an archival pipeline, and nothing downstream
can tell. A missing tag is loud and recoverable. Given that asymmetry, the
generator always chooses the loud failure.

The same rule governs every layer:

- a Composite with no implementation in `compute.rs` never fires;
- a magic number with no corroborating extension does not identify a file;
- a tag whose `Format` cannot be reproduced is omitted, not guessed.

Every one of those refusals is counted and printed. A generator that silently
drops what it cannot handle produces a coverage number that is a lie, and the
whole argument for doing this mechanically rests on the numbers being
trustworthy.

## Verification: soundness and completeness are different questions

```
dump_tables.pl   Perl symbol table  ->  tables.json
codegen.py       tables.json        ->  binary_tables.rs
oracle.pl        Perl symbol table  ->  ground-truth TSV
verify.py        Rust + TSV         ->  PASS / FAIL
```

`verify.py` parses the **generated Rust back out** and diffs it against a fresh
dump from `oracle.pl`, which shares no code with `dump_tables.pl`. Comparing
against the generator's own JSON would only prove self-consistency — it would
cheerfully confirm a bug both sides inherited.

- **Soundness** — is everything emitted correct? Answered by `verify.py`.
  Currently 6,351/6,351 fields and 47,216/47,216 enum entries match ExifTool
  exactly.
- **Completeness** — how much was skipped, and why? Answered by `codegen.py`'s
  own accounting, printed on every run.

Reporting only the first would score well while emitting a tenth of the tables.
Reporting only the second says nothing about correctness. Both, always.

### Determinism is part of verification

Generated files are committed, so `just regen-tables` must produce byte-identical
output on every run. This is not tidiness. A generator whose output churns cannot
be reviewed in a diff, and two real bugs surfaced only because the output was
supposed to be stable and wasn't:

- **A verifier that stopped verifying.** Adding `rustfmt` to generation wrapped
  every `Field { .. }` across several lines, which the verifier's single-line
  patterns stopped matching. It then parsed zero fields, found zero mismatches,
  and read as a pass. It also double-counted enum pairs through an unanchored
  capture, inflating the reported total by 2.4%. `parse_rust` now asserts it
  accounted for every `Field {` in the file, so under-parsing fails loudly.
- **An identifier collision.** `$val` and `"$val%"` both reduced to the Rust
  identifier `Val`, aliasing two conversions onto one enum variant, with set
  iteration order picking the winner. It compiled, and it passed verification,
  because `verify.py` checks names and enum maps but not translated expressions.
  The visible symptom would have been Canon `CropLeft`/`CropTop` rendering as
  `50` instead of `50%`.
- **Formatting drift.** Committed files are `cargo fmt`-ed; freshly generated
  ones were not, so every run produced a 100k-line phantom diff. `rustfmt` is now
  part of generation, not an afterthought.

## Measuring the gap: classify, don't just count

The [comparison report](/reference/comparison/) says *which* formats score badly.
It does not say why, and the why decides what the work actually costs:

| class     | meaning                                       | cost              |
| --------- | --------------------------------------------- | ----------------- |
| `RENAME`  | value read correctly, under a different name  | a string edit     |
| `MISSING` | ExifTool emits it, OxiDex does not            | real parsing work |
| `VALUE`   | both emit it, values disagree                 | usually PrintConv |
| `EXTRA`   | OxiDex-only, no counterpart                   | investigate       |

A tag-at-a-time fix loop cannot distinguish these, so it pays full price for
renames — the cheapest class there is. `tools/exiftool-tables/conformance.py`
separates them for every format at once:

```sh
python3 tools/exiftool-tables/conformance.py <corpus> --exiftool-dir <exiftool-src>
```

**BMP's 0% was entirely renames.** OxiDex parses BMP fine and calls the tags
`Width`/`Height` where ExifTool says `ImageWidth`/`ImageHeight`. No parsing work
existed to do.

Rename inference requires the values to agree *and* the pairing to be
unambiguous in both directions, plus either matching normalised names or a
distinctive value. The first version matched on value alone and produced crossed
nonsense — `Blue -> RedTRC` *and* `Red -> BlueTRC` in the same file, because all
three ICC curves hold identical data, and `Height -> Aperture` because both
happened to be 8. Guessing a rename is worse than reporting nothing: it sends
someone to "fix" a correctly-named tag.

::: tip The corpus is free
ExifTool ships ~190 sample files in `t/images`. With its Perl source already
downloaded for table extraction, that is a full differential corpus *and* a
runnable oracle at no extra cost — `perl -Ilib ./exiftool` works straight from
the tarball, no installation.
:::

## Work the measurement pointed at

Following the ranked gap rather than intuition produced four layers, in order.

### 1. Composite tags

The ten most-missed tag names were all ExifTool **Composites** — `ImageSize`,
`Megapixels`, `Aperture`, `ShutterSpeed`, `FocalLength35efl`, `LightValue`,
`CircleOfConfusion`, `HyperfocalDistance`, `FOV`, `DOF`. None are read from a
file. All are derived from tags OxiDex already extracted correctly.

`codegen_composite.py` emits the dependency graph (104 definitions);
`src/composite/compute.rs` hand-ports the arithmetic. Nine functions moved the
corpus 52.0% → 55.4% and JPEG 62.6% → 68.1%, because a derivation layer pays off
in every format at once.

### 2. File identification

43 corpus files produced **no output at all** — an unparseable format made
`read_metadata` return `Err`, discarding even the file-system metadata already
gathered. `FileType`, `FileTypeExtension` and `MIMEType` came from ExifTool's
`%magicNumber` / `%fileTypeLookup` / `%mimeType`: 110 patterns, 343 extensions,
225 MIME types, transcribed.

### 3. `ExifByteOrder`

51 instances, and the information was always available where the TIFF header is
parsed — it was simply never recorded.

### 4. `FOV` and the optical chain

Completes `ScaleFactor35efl → CircleOfConfusion → HyperfocalDistance → FOV/DOF`.

### Result

| | start | after |
| --- | --- | --- |
| Corpus total | 52.0% | **57.1%** |
| JPEG | 62.6% | **69.2%** |
| CR2 | 82.1% | **86.5%** |
| DNG | 82.0% | **85.5%** |
| HEIF | 54.8% | **96.8%** |
| M4A | 58.1% | **85.5%** |
| AIFF/DPX/SWF/DICOM | 0% | identified exactly |

Matched tags 5,039 → 5,537; missing 3,740 → 3,219. Zero model calls.

## What the oracle caught that review would not

Every one of these compiled, and several passed the table verifier. They were
caught only by diffing against a real ExifTool. This is the strongest argument
for building the differential harness *before* the fixes.

- **Canon `ScaleFactor35efl` 29.3 against ExifTool's 1.6**, which then corrupted
  `CircleOfConfusion` and `HyperfocalDistance`. Cause: skipping ExifTool's
  Canon-specific `CalcSensorDiag`, so Canon files fell through to the generic
  focal-plane path. Canon encodes sensor size in the *denominator* of the
  `FocalPlaneX/YResolution` rationals — information the divided float has already
  destroyed. Three confidently wrong values under real tag names.
- **Collapsing `ValueConv` and `PrintConv`.** ExifTool keeps the full-precision
  value apart from the rounded display form. Merging them fed
  `HyperfocalDistance` the printed `0.019 mm` instead of `0.01926`, giving 4.35 m
  against 4.37 m — a quiet loss at *every* link in a chain.
- **Freezing a composite at a partial answer.** `FocalLength35efl` was computed
  before `ScaleFactor35efl` existed and never revisited, so it read `34.0 mm`
  instead of `34.0 mm (35 mm equivalent: 54.0 mm)`. Composites are now
  recomputed across passes; values read from the file are still never replaced.
- **Over-eager identification.** `%magicNumber` is ExifTool's *pre-filter*, not
  its answer — it follows a match by asking the format module to parse, and
  reports "Unknown file type" when that fails. `Font` matches any file starting
  `\0\x01`, so magic alone labelled 29 bytes of junk as a font.
- **Masking corruption.** Falling back to identification on *any* error turned
  three "malformed OLE must be rejected" tests green-to-red. Reporting a corrupt
  document as a successful read carrying three identity tags is strictly worse
  than failing. The fallback is now restricted to `UnsupportedFormat`.

## Adding work

The methodology is designed so that marginal cost per tag **falls** as the
registry grows. Protect that property.

1. **Measure first.** Run `conformance.py` and work the ranked list. Do not fix
   what you have not measured; renames and missing tags look identical from the
   inside.
2. **Prefer the layer over the instance.** If a tag is missing in one format,
   ask whether the mechanism is missing in all of them. One entry in `exprs.py`
   fixes every tag sharing that expression across all 146 modules, permanently.
   One entry in `compute.rs` fixes a Composite in every format at once.
3. **Quote the original.** Every hand-ported function carries ExifTool's source
   above it, so a reviewer can check the translation without opening `Exif.pm`.
4. **Reproduce quirks exactly.** ExifTool's `FOV` divides by a literal `3.14159`,
   not π. Substituting the more accurate constant shifts the result in the first
   decimal — where the printed value rounds — so "more correct" reads as a
   mismatch. Fidelity beats correctness when the goal is compatibility.
5. **Refuse rather than approximate**, and make the refusal visible in the run
   output.
6. **Verify against the oracle, not against your reasoning.** Three of the bugs
   above were cases where a *test expectation* was wrong and the code was right.
   Fix the expectation against ExifTool, never the other way round.

## Honest limits

- `verify.py` checks field names and enum maps. It does **not** verify
  translated expressions — that is the unverified surface, and it is where the
  identifier-collision bug hid.
- File identification deliberately under-claims. A JPEG named `.dat`, or a file
  with no extension, is not identified, because OxiDex cannot run ExifTool's
  confirming parse.
- The generator currently emits only `ProcessBinaryData` tables. The extractor
  already captures the subdirectory graph, conditions and value conversions
  needed to extend it to IFD-style tables; that work has not been done.
- Some inferred renames still want human confirmation — `LNK
  TargetFileAttributes -> RunWindow` looks doubtful.
- **What remains is genuinely not transcription.** CR3 (250 missing tags in the
  corpus), CRW (156) and DICOM (101) need real format implementations. This
  methodology has no shortcut for them, and claiming otherwise would repeat the
  mistake this document is written to avoid.

## Relationship to the AI harness

**This methodology is intended to be the default, and the
[AI harness](./AI_HARNESS.md) the exception.** The ordering is not a matter of
taste — it follows from the economics.

The harness delivered 67 tags over five days from 7,522 diffs: roughly **112
model calls per delivered tag**, with a 56% delivery rate on a pre-filtered
candidate list. Its cost per tag is flat, because each fix is independent and
nothing accumulates. Transcription extracted 27,747 tag entries in 1.3 seconds,
and its cost per tag *falls*, because every entry in `exprs.py` or `compute.rs`
serves every tag that shares it, across all 146 modules, permanently.

The expensive failure is reaching for search where transcription applies —
paying ~112 model calls to rediscover a table row that could have been
transcribed in bulk. Before invoking the harness on anything, the test is:

> Is this knowledge already written down in machine-readable form anywhere —
> ExifTool's tables, a published spec, an existing implementation?

If yes, it is transcription work, and the harness is the wrong tool no matter
how convenient it looks.

### Handling the residue without the harness

What remains after transcription — CR3, CRW, DICOM, the per-camera binary
quirks — is genuine implementation work. It does **not** follow that a search
loop is the way to do it. The same infrastructure built here supports doing it
directly, and more cheaply:

- **The oracle is free and local.** `perl -Ilib ./exiftool` runs from the
  ExifTool tarball with no installation. Any implementation can be diffed
  against real ExifTool output on every save, which is faster feedback than a
  gated fix loop and does not consume a rate limit.
- **ExifTool's source is the specification.** For CR3 and CRW the layout is
  described in `Canon.pm`/`CanonRaw.pm` and its accompanying comments. Reading
  the module is strictly better than having a model infer the layout from sample
  bytes, because the module *is* the answer, with the quirks already annotated.
- **`conformance.py` scopes the work honestly.** It reports exactly which tags a
  format is missing, so an implementation can be driven to a measurable target
  rather than declared done.
- **The corpus makes regressions visible.** 190 files, one command, every format
  at once.
- **Extend the extractor before writing a parser by hand.** The subdirectory
  graph, `Condition` variants and `ValueConv` expressions are already captured
  in `tables.json` and simply not yet code-generated. Emitting IFD-style tables
  is more leverage than any single format implementation.

If the harness is used at all, use it narrowly, on a named list of tags that
survived all of the above, and hold its output to the same rule as everything
else here: refuse rather than approximate, verify against the oracle, and report
soundness and completeness separately.

## Reference

```sh
just regen-tables            # fetch ExifTool, extract, generate, format, verify
just verify-tables           # re-check committed output against ExifTool
python3 tools/exiftool-tables/analyze.py <tables.json>
python3 tools/exiftool-tables/conformance.py <corpus> --exiftool-dir <src>
```

| file                    | role                                          |
| ----------------------- | --------------------------------------------- |
| `dump_tables.pl`        | tag tables, via the Perl symbol table         |
| `dump_filetypes.pl`     | `%magicNumber` / `%fileTypeLookup` / `%mimeType` |
| `analyze.py`            | safety tiers; ranked untranslated expressions |
| `codegen.py`            | binary tables → Rust                          |
| `codegen_composite.py`  | Composite dependency graph → Rust             |
| `codegen_filetypes.py`  | identification tables → Rust                  |
| `exprs.py`              | hand-verified Perl → Rust translations        |
| `oracle.pl`             | independent ground truth for verification     |
| `verify.py`             | soundness check                               |
| `conformance.py`        | gap measurement and classification            |
