# exiftool-tables — mechanical transcription of ExifTool's tag tables

Generates Rust binary tag tables directly from ExifTool's own Perl data
structures, verified back against ExifTool.

```sh
just regen-tables          # fetch + extract + generate + verify (tier 1 only)
just verify-tables         # re-check committed tier-1 output against ExifTool
just regen-tables-all      # tier 1 AND tier 2, from one resolved ExifTool tree
just regen-tables-tier2    # tier 2 only -- what CI's verify-tables job runs
just bump-exiftool <ver>   # the whole release-bump pipeline: pin, fetch,
                            # capability-probe, regen-all, verify, triage
                            # every JSON-to-JSON delta AUTO/EXPR/COND/HAND,
                            # conformance double-run, gate check. --dry-run
                            # exercises it and reverts every change -- see
                            # tools/exiftool-tables/bump-exiftool.sh and
                            # docs/reference/bump-reports/13.58-to-13.59.md
                            # for a worked example.
```

`regen-tables` (this directory's `regen.sh`) only ever produced
`binary_tables.rs` and its three siblings. A second generation tier sits
downstream of it and `regen-tables` never called it: the MakerNote
sub-directory tables (`codegen_subdirs.py`), the Nikon AF-point name grids
(`dump_af_points.pl` + `codegen_af_points.py`), and six one-off
`scripts/gen_*.pl` transcriptions. `regen-all.sh` is the sibling that invokes
both tiers against the SAME resolved ExifTool tree, so a bump cannot refresh
one tier and silently leave the other on an older release -- see its own
header for the full rationale, and `docs/TRANSCRIPTION.md`'s "Honest limits"
section for six further files (Sony/Nikon/Minolta binary-data tables) that
are generated but still have no committed generator at all.

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
dump_tables.pl     Perl symbol table  ->  tables.json     (146 modules, 1,281 tables)
analyze.py         tables.json        ->  coverage report (what is safe to emit)
codegen.py         tables.json        ->  binary_tables.rs
codegen_subdirs.py tables.json        ->  a vendor's MakerNote sub-tables
oracle.pl          Perl symbol table  ->  ground-truth TSV
verify.py          Rust + TSV         ->  PASS / FAIL
```

`codegen_subdirs.py` is the narrow, strict sibling of `codegen.py`. It takes a
named list of tables and emits them for the `ProcessBinaryData` interpreter in
`src/parsers/tiff/makernotes/shared/binary_subdir.rs`, which the MakerNote
parsers use to descend into a `SubDirectory` tag instead of reading its pointer
as a value. Where `codegen.py` counts what it skipped, this one **raises** on any
construct it has not been taught and names the table, the tag and the construct;
`--allow-skip` downgrades that to a logged line, and the log is the deliverable.
The difference matters: these tables are wired into a parser one at a time, so an
unhandled field has to stop the run rather than land in an aggregate statistic.

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
