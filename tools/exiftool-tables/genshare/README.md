# Generated share of correct output (authenticated census)

`genshare-probe/1` used a temporary, forward-ported patch. It is retired.
The maintained `src/exiftool_tables::attribution` seam reads
`OXIDEX_GENSHARE_SILENCE` once, and only drops an outward row after its reader
has completed stateful work. `census.sh` builds one binary, records paired
control/probe process hashes and return codes, validates the pinned oracle,
and refuses unknown or `conv` tokens with exit status 2 before traversal.

Use the maintained interface from the task PRD. The committed three-file
manifest is an authenticated smoke scope; the full-corpus command supplies
the production denominator. `receipt.json`, not a historical summary, is the
consumable artifact.

This directory is the instrument behind the "generated share of correct output"
figure on the status page (`/status/`) and in `docs/AUTOGENERATION-PLAN.md`. The
committed result is `docs/public/measurements/generated-share-13.59.json`, and
`tools/docs/render_status.py` reads the figure from that file.

The question it answers is: **of the rows oxidex gets right against the pinned
ExifTool, how many would be lost if the code generated from ExifTool's source
were switched off?**

## Method (`genshare-probe/1`)

1. **Probe build.** `probe.patch` adds `src/probe_silence.rs` and one guard at
   each point where generated data becomes an output row. The patch is never
   landed. The environment variable `OXIDEX_PROBE_SILENCE=<tokens>` drops the row
   at that point and keeps every side effect of the walk (member writes,
   `DataMember`s, cipher state, descents). If the variable is unset or empty,
   the output is byte-identical to the unpatched build. The census checks this
   on every corpus file (the *inertness* step).

   | token | boundary silenced |
   | --- | --- |
   | `engine` | the generic table engines: `engine.rs` and `ifd_engine.rs`'s `Emitted` pushes, including the v2 generated conversion arm (`generated_row`, #838) |
   | `legacy-l1` | `runtime::DecodedField::emit`, the decode API that hand parsers call over generated tables (`RawAccess` rows are kept, see below) |
   | `legacy-l2` | vendor walkers over tables from the generated manifest: binary_subdir, Canon CustomFunctions2, InfiRay, Sony main_extra, Sony plain and Minolta A100 walkers, Nikon settings |
   | `producers` | generated producers: File identity (`FileType`, `FileTypeExtension`, `MIMEType`), the three `generated_compute` Composite arms, DICOM, GeoTIFF and FITS names |
   | `legacy-l3` | walkers over hand-transcribed tables. These are **not** counted as generated. The token exists so that their size can be measured separately. |

   The headline uses the union token `engine,legacy-l1,legacy-l2,producers`.
   "Engine alone" uses `engine`.

2. **Census.** `census.sh` runs `tools/exiftool-tables/conformance.py
   --recursive --json-out` over the full corpus, which is
   `/tmp/oxidex-exiftool-cache/combined-samples` (4,238 files, floors
   `--min-files 3875 --min-tags 5000`). It grades against pinned ExifTool 13.59
   and asserts both `-ver` = 13.59 and the `OOXML.docx` → `DOCX` capability probe
   before the first number. It makes one census with the **unmodified** binary of
   the same commit (the control), the inertness check, and then one census per
   token with the probe binary. Every census runs under the exclusive
   measurement lock.

3. **Attribution.** `attribute.py` computes, per token:

   - **matched lost** = control `matched` − probe `matched`, summed from
     `per_format`. This is the authoritative number, and the share is
     matched lost ÷ control matched.
   - A per-row rebuild from `per_file`: for each (file, name), the lost count is
     ΔMISSING + ΔVALUE. The rebuild must reconcile with matched lost, up to the
     rename delta, with residual 0. `summarize.py` refuses any point where it
     does not.
   - Buckets, using the class index `class-names-eadb5884.json`:
     - **direct**: DIRECT plus DIRECT_NAME. The lost row is one the silenced
       class itself writes.
     - **Composite cascade**: Composite values that ExifTool computes from the
       dropped rows. oxidex computes them in hand code, from generated inputs.
     - **other cascade**: everything else, for example a PreviewImage built
       from a dropped Start/Length pair.

4. **Result.** `summarize.py` turns one `attribute.py --json-out` per measured
   commit into the committed JSON: the current commit, the earlier points, and
   the delta from the first point.

## Why it is a floor

The probe measures what the output *would lose*. Anything generated that
survives the probe is counted as hand:

- **Shared keys.** A row that both generated and hand code write under the
  same key survives the probe through the hand copy. Examples are the Olympus
  residual tables, Sony `resolve_duplicates`, Canon ShotInfo vs CameraInfo,
  and the InteropIFD hand arms that run whenever an engine row is absent
  (worth about 0.9 points at `72eae8a5`).
- **Conversions.** Generated PrintConv, enum and name maps looked up by hand
  walkers are not silenced. Silencing a lookup makes the value wrong instead of
  absent, so the conversion has no clean point to silence (the `conv` token is
  refused).
- **`RawAccess` rows** (L1b) and `sony/main_table.rs` rows are not silenced.
- **Generated routes the probe does not know about.** The probe's boundary
  list dates from `72eae8a5`. Generated routes added since then are not
  silenced, so they count as hand until a token covers them: the serial engine
  (`serial_engine.rs`, Canon AFInfo2, #760), the keyed engine and the generated
  Garmin FIT reader (#782). #838's generated conversion arm is covered, because
  it replaced a boundary the probe already silenced for the same fields.

The class index affects only the direct / cascade split, never matched lost.
It was built at `eadb5884` by `index/parse_tables.py` and `index/build_names.py`.
Those scripts read the pre-#823 layout, which had one `binary_tables.rs` and
one `ifd_tables.rs`. They are kept as a record of how the index was made.
Because the union token does not silence `composite-declared`, every lost
Composite row is a cascade whatever the index says, so the Composite cascade
does not depend on the index either.

## Reproducing a point

```sh
# per measured commit C: a clean control tree and a probe tree
git worktree add --detach ../gs-ctl-C   C
git worktree add --detach ../gs-probe-C C
git -C ../gs-probe-C am "$PWD/tools/exiftool-tables/genshare/probe.patch"   # see below for older C
(cd ../gs-ctl-C   && cargo build --release --bin oxidex)
(cd ../gs-probe-C && cargo build --release --bin oxidex)

unset PERL5LIB PERLLIB PERL5OPT
export EXIFTOOL_PERL=<perl 5.38.2 with Archive::Zip>
tools/exiftool-tables/genshare/census.sh <out-dir> ../gs-ctl-C ../gs-probe-C \
    engine,legacy-l1,legacy-l2,producers engine

tools/exiftool-tables/genshare/attribute.py \
    --class-names tools/exiftool-tables/genshare/class-names-eadb5884.json \
    --control <out-dir>/control.json \
    --probe engine,legacy-l1,legacy-l2,producers=<out-dir>/probe-engine+legacy-l1+legacy-l2+producers.json \
    --probe engine=<out-dir>/probe-engine.json --json-out <out-dir>/attr.json

tools/exiftool-tables/genshare/summarize.py --exiftool 13.59 \
    --corpus /tmp/oxidex-exiftool-cache/combined-samples --files 4238 --date <YYYY-MM-DD> \
    --point <sha1>=<attr1.json> ... --point <current sha>=<attr.json> \
    --out docs/public/measurements/generated-share-13.59.json
uv run tools/docs/render_status.py      # then update the plan's row to the new figure
```

`probe.patch` applies to `2d8ff775`. Its first three commits apply to
`8cceb4a7`, with two mechanical conflicts: `infiray.rs` gained
`insert_with_group1`, and `fits.rs` lost `naxis_values`. Keep the upstream
code and add the probe guard. The fourth commit silences #838's generated arm
and exists only from `2d8ff775` on. `probe-72eae8a5.patch` holds the probe
exactly as it was measured at `72eae8a5` (branch `staging/gen-share-probe-72ea`
@ `887bf764`).

**When porting the probe forward,** list every place the engines push a row
(`rg -n 'out\.push|sink\.emit|Emitted \{' src/exiftool_tables`). A new push
that carries rows a silenced class used to emit must get the same guard, or
the share falls for no real reason. A push for a new generated route goes on
the floor list above until it has its own token.

Checks that must hold before a point is trusted:

- the census header shows ExifTool 13.59 and the DOCX probe;
- the file count is 4,238;
- the probe's token sanity run exits 0 (conformance.py ignores oxidex's exit
  status, and a refused token scores as all-MISSING; `attribute.py` refuses a
  probe below 25% of control);
- inertness is 4,238/4,238, or any difference is shown to be nondeterminism
  in the control binary too (PentaxOptioL20.jpg's `PentaxModelID` duplicate
  order is a known case);
- `reconciliation_residual` is 0 for every token.
