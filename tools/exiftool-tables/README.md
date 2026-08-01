# exiftool-tables — mechanical transcription of ExifTool's tag tables

Generates Rust binary tag tables directly from ExifTool's own Perl data
structures, verified back against ExifTool.

```sh
just regen-tables          # fetch + extract + generate + verify
just verify-tables         # re-check committed output against ExifTool
```

## Why

ExifTool is not 16,000 hand-written tag implementations. It is a generic engine
plus roughly 1,300 declarative tag tables. Those tables are *data*, and data can
be transcribed rather than reimplemented.

OxiDex already reads `exiftool -f -listx` (`src/tag_sync`) to learn tag **names**.
That is why the project can say it knows 16,677 tags while extracting far fewer:
`-listx` is the documentation view. It gives you name, id, writability and
description, and it discards everything you need in order to actually read a
value out of a file:

| needed to read a tag        | in `-listx` | in the Perl tables |
| --------------------------- | ----------- | ------------------ |
| tag name / id               | yes         | yes                |
| `FORMAT`, `FIRST_ENTRY`     | **no**      | yes                |
| per-field `Format` override | **no**      | yes                |
| `SubDirectory` → `TagTable` | **no**      | yes                |
| `ValueConv` / `RawConv`     | **no**      | yes                |
| `Condition` variants        | **no**      | yes                |
| `Mask`, `DataMember`, `Hook`| **no**      | yes                |

The missing rows are exactly the MakerNote layout information. That is why
coverage lagged in JPEG and RAW formats specifically, and it is recoverable
mechanically — it was never a knowledge problem.

## How

`dump_tables.pl` does **not** parse Perl. ExifTool builds its tables at
`require` time: some are assembled in loops, some inherit by copying another
table, some are patched afterwards. Any regex over the `.pm` text sees the
source, not the structure ExifTool actually dispatches on. Instead the script
loads each module and walks the symbol table, so what it reads is the real
in-memory table. Full extraction of all 146 modules takes about 1.3 seconds.

```
dump_tables.pl   Perl symbol table  ->  tables.json     (146 modules, 1,281 tables)
analyze.py       tables.json        ->  coverage report (what is safe to emit)
codegen.py       tables.json        ->  binary_tables.rs
oracle.pl        Perl symbol table  ->  ground-truth TSV
verify.py        Rust + TSV         ->  PASS / FAIL
```

`verify.py` parses the **generated Rust back out** and compares it against a
fresh dump produced by `oracle.pl`, which shares no code with `dump_tables.pl`.
Comparing against the generator's own JSON would only prove self-consistency —
it would cheerfully confirm a bug both sides inherited.

## The rule: never approximate

A conversion is translated only if its exact expression is registered in
`exprs.py`. Anything else is dropped and counted.

This is deliberate under-claiming. A wrong `PrintConv` does not crash. It emits
a confident, plausible, wrong number under a genuine ExifTool tag name, into an
archival pipeline, and nothing downstream can detect it. A missing tag is loud
and recoverable. Given the asymmetry, the generator always chooses the loud
failure.

Soundness and completeness are reported **separately**, and neither number is
allowed to stand in for the other:

* `verify.py` measures soundness — is everything emitted correct?
* `codegen.py` measures completeness — how much was skipped, and why?

## Where the effort goes

Of 27,747 extracted tag entries:

| tier    | count  | share | meaning                              |
| ------- | ------ | ----- | ------------------------------------ |
| pure    | 18,690 | 67.4% | no conversions; pure transcription   |
| enum    | 4,480  | 16.1% | `PrintConv` lookup maps; pure data   |
| expr    | 3,993  | 14.4% | Perl expression; needs a translation |
| code    | 210    | 0.8%  | Perl code ref; needs real porting    |
| variant | 374    | 1.3%  | `Condition` dispatch; needs a port   |

**83.5% is mechanically safe** and should never have cost a model call.

The remaining tail is smaller than it looks: 3,993 expression tags share only
1,409 distinct expressions, and the 20 most common cover 1,535 tags. Adding one
entry to `exprs.py` fixes every tag sharing that expression, permanently, across
all 146 modules — so the marginal cost per tag *falls* as the registry grows.

That is the property to protect. Run `analyze.py`, work down the ranked list of
unsupported expressions, and let each fix compound.

## Scope

`codegen.py` currently emits only `ProcessBinaryData` tables — those with a
`FORMAT` and a field per offset. That is where the coverage gap lives and where
`-listx` helps least. The extractor already captures the subdirectory graph,
conditions and value conversions for everything else; extending the generator to
IFD-style tables is the obvious next step and needs no new extraction work.
