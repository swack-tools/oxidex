---
outline: 2
---

# Corpus Synthesis (Step 28 enablement gate)

::: info Manual measurement run, not auto-regenerated
Run 2026-08-13 against the pinned **exiftool 13.59** (`.exiftool-version`) on the
remote build host, capability-probed (`-ver` → `13.59`; `OOXML.docx` →
`FileType: DOCX`, confirming `Archive::Zip` is actually present, not just the
version string). Corpus: `/tmp/oxidex-exiftool-cache/combined-samples`,
4,238 files. oxidex: release build at commit `cbc6618f`. This is a one-off
measurement, not a CI job — re-run the two scripts below to refresh it.
:::

## The question

`codegen.py` transcribes 613 `ProcessBinaryData` tables from ExifTool's own
Perl tables. Only 22 of them are wired to a live `find_table()` call site at
runtime (independently re-derived here as **21** — see [Note on the 22 vs 21
discrepancy](#note-on-the-22-vs-21-discrepancy)) — the other ~591 are
transcribed but dead: nothing in the parser tree ever looks them up. Most of
those tables have no sample file in the corpus, so a conformance gate keyed on
"does a sample exist" is vacuous for them.

ExifTool can *write* into its own binary tables (`exiftool -Canon:SelfTimer=5
file.jpg`), which means real coverage can potentially be manufactured rather
than sourced: write a value into a carrier file of the right make, then check
whether oxidex reads it back. This harness measures how far that idea actually
reaches — it does not implement the Step 28 engine itself.

## How it works

Two scripts, run in sequence:

1. **`tools/exiftool-tables/synth_classify.py`** — for each of the 613 emitted
   tables, decide whether a sample is even *possible*: is the table writable
   (per `dump_tables.pl`'s own `WRITABLE`/tag-level `Writable` extraction —
   the same generator tier that feeds `codegen.py`, not a re-derivation), and
   does the corpus have a carrier of the right make/container
   (`synth_carriers.py`'s hand-built map, keyed off the corpus's actual
   directory listing: 13 manufacturer subdirectories of real-world JPEGs plus
   ~150 ExifTool-style single-exemplar files named `<Module>.<ext>`).
2. **`tools/exiftool-tables/synth_generate.py`** — for a chosen subset, write
   every eligible field (via `-n`/raw values, sidestepping ExifTool's
   PrintConvInv/BITMASK write syntax entirely) into a real carrier file with
   the pinned exiftool, confirm the write round-trips through exiftool itself
   (raw re-read), then run the built oxidex binary against the *same* file and
   check whether it reads the tag back (PrintConv'd form, lenient match —
   numeric tolerance, date normalization, single-letter enum abbreviation,
   ported from `jpeg-tag-matrix`'s `values_match`).

Field data for step 2 comes from **`src/exiftool_tables/binary_tables.rs`
itself** (`synth_rust_fields.py` parses the generated Rust literals directly),
not from a fresh, unverified read of `dump_tables.pl`'s JSON — that file is
what `just verify-tables` already checks field-by-field against ExifTool, so
every field this harness attempts to write is one whose format/count/enum
mapping is independently verified correct.

This deliberately extends [`jpeg-tag-matrix`](/reference/jpeg-tag-matrix)'s
write→read-back methodology rather than inventing a parallel one; it
generalizes two things that harness hard-codes to JPEG-only EXIF tags: the
carrier (any format/manufacturer file, not one fixed base JPEG) and the field
source (verified binary-table transcriptions, not `-listx`).

## Measured starting point (verified, not re-derived)

Re-running the "how many `ProcessBinaryData` tables are writable" measurement
against this session's own `dump_tables.pl` output (153 modules loaded, 0
failed) found **651 total `ProcessBinaryData` tables, 361 (55.5%) with
table-level `WRITABLE`/`WRITE_PROC`** — close to, but not exactly, the
726/424 (58%) figure quoted as the starting point for this task. Both
runs used the same pinned 13.59 release and the same
`is_binary_table()`/`PROCESS_PROC`-suffix test as `codegen.py`; the gap is
unexplained (possibly a different `dump_tables.pl` revision or host at the
time of the original count) and doesn't change the conclusion — **a little
over half of ExifTool's binary tables are writable at all**, so synthesis was
never going to reach the whole 591-table gap.

## Classification: 613 emitted tables

| Class | Count | Meaning |
|---|---:|---|
| Already reachable | 21 | wired to a live `find_table()` call (see note below) |
| **Synthesizable** | **305** | writable, and the corpus has a carrier of the right make/container |
| Needs a real sample | 23 | writable, but no carrier exists anywhere in the corpus |
| Unwritable | 264 | ExifTool itself won't write it (no `WRITABLE`, no per-tag override) — synthesis cannot help regardless of sample availability |
| Unreachable total | **592** | 613 − 21 |

**Headline: of the 592 currently-dead tables, 305 (51.5%) are synthesizable** —
carrier and writability both check out. The other 48.5% split roughly evenly
between genuinely unwritable (264, 44.6% of the gap — mostly structural
container-format fields like `AIFF::FormatVers`, `LNK::Header`,
`RIFF::AVIHeader`, `QuickTime::*Atom` sizes, `EXE::MachO`, which ExifTool
reports but never writes) and writable-but-sample-starved (23, 3.9% of the
gap — see below).

### needs-real-sample (all 23)

Every one of these is a MakerNote table for a manufacturer the corpus simply
has no file from at all: **Casio** (2 tables), **FotoStation** (1),
**Kodak** (8), **Microsoft** (1), **Nintendo** (1), **Reconyx** (5),
**Ricoh** (4), **Sanyo** (1). This is a corpus gap, not a methodology
limit — a Casio/Kodak/Ricoh/Reconyx/Sanyo JPEG sample would very likely move
all 23 into "synthesizable" on the next run.

### synthesizable, by module (top entries)

| Module | Synthesizable tables | Carrier |
|---|---:|---|
| Nikon | 82 | vendor dir (307 JPEGs) |
| Canon | 81 | vendor dir (725 JPEGs) |
| Pentax | 38 | vendor dir (145 JPEGs) |
| Sony | 22 | vendor dir (761 JPEGs) |
| NikonCustom | 20 | vendor dir (low-confidence: model-dependent layout) |
| NikonCapture | 13 | vendor dir (low-confidence: capture-editing tags not guaranteed present) |
| Minolta | 7 | `Minolta.mrw` exemplar |
| CanonRaw | 6 | `CanonRaw.crw` exemplar |
| CanonVRD | 6 | `CanonVRD.vrd`/`.dr4` exemplar |
| Panasonic | 6 | vendor dir (477 JPEGs) |
| FujiFilm | 4 | vendor dir (411 JPEGs) |
| QuickTime | 4 | `QuickTime.mov` exemplar |
| MinoltaRaw, CanonCustom, Olympus, PanasonicRaw, Photoshop, Samsung | 2–3 each | — |
| Jpeg2000, PNG, Sigma | 1 each | — |

35 of the 305 synthesizable tables are flagged **low-confidence** in
`synth_carriers.py` — a plausible carrier dir/file exists, but whether *this
specific table's tags* actually populate on any given sample is unverified
until generation runs (CanonCustom's function tags, NikonCapture's
capture-editing tags, NikonCustom's per-body settings layouts). The subset run
below empirically resolves several of these.

Full per-table classification: [`corpus-synthesis-classification.json`](./corpus-synthesis-classification.json).

### Note on the 22 vs 21 discrepancy

Independently grepping every non-test `find_table(...)` call site in `src/`
found the same **22** call sites the task's starting point names — 18 static
plus 4 resolved via two model-dispatched branches (`Sony::amount.rs`'s
`ExtraInfo`/`ExtraInfo2`/`ExtraInfo3` selection, and
`raw/metadata.rs`'s `canon_crw_tag_key`'s `ShotInfo`/`AFInfo` selection). But
one of those 22 — `find_table("Canon", "AFInfo")` in
`raw/metadata.rs::canon_crw_tag_key` — targets a table that `codegen.py`
never actually emitted (it isn't among the 613), so that particular lookup
always returns `None` at runtime: a dead call site, not dead code exactly, but
not a live table either. Net reachable-and-live count: **21**, and unreachable
is **592**, not 591. This is flagged separately here rather than silently
reconciled to the task's number; the call site itself is a one-line latent
bug worth a follow-up (spawned separately, see below).

**Resolved.** `%Image::ExifTool::Canon::AFInfo` is a real table under exactly
that name (Canon.pm:6433) — it is not a rename, and the lookup is not a typo.
Its `PROCESS_PROC` is `\&ProcessSerialData` (Canon.pm:6434, sub at
Canon.pm:10518), so `is_binary_table` (codegen.py:177) rejects it and
`gen_table` (codegen.py:568) counts it under `table_not_binary`. That refusal
is correct and was not relaxed: in a serial record the keys are sequence
numbers, not byte offsets — key 8 `AFAreaXPositions` is `int16s[$val{0}]`, so
key 9 begins wherever key 8 ended, at a position that depends on the value of
key 0 in the file being read. A flat `BinaryTable` would place every field at
`index * 2` and report confident integers from meaningless offsets under real
ExifTool tag names.

So the call site is dead until the generator grows a serial-record kind, and
the fix makes that state *explicit* rather than silent:

* `src/exiftool_tables`'s `UNEMITTED_TABLES` registry records the module,
  table, `GROUPS => { 0 => ... }`, Perl citation, the generator's own refusal
  counter, and what would unblock it. `find_unemitted_table` is what
  `canon_crw_tag_key` now consults, so its `MakerNotes:` prefix is transcribed
  from `%Canon::AFInfo`'s own `GROUPS` (Canon.pm:6437) instead of borrowed from
  `ShotInfo` (Canon.pm:2778) — the two agree, which is why the dead lookup
  produced correct output for as long as it did, but agreement is a
  coincidence, not a derivation.
* `unemitted_tables_are_genuinely_absent` fails the day a regeneration starts
  emitting the table, so the entry retires itself.
* `synth_carriers.py` now splits `REACHABLE` (21) from `DEAD_LOOKUPS` (1), with
  `CALL_SITES` as their union (22), and `synth_classify.py` asserts both halves
  against the emitted set. The 22-vs-21 correction above is therefore derived
  from now on rather than reconciled by hand.

The fix is output-neutral, measured two ways:

* `oxidex -j` run under both the pre-fix and post-fix debug binaries over all
  **4238** corpus files — **0 files with differing output**, byte-for-byte.
* `tools/exiftool-tables/conformance.py --recursive --min-files 3875
  --min-tags 5000` against pinned ExifTool 13.59, run on each binary in turn:
  `TOTAL 4238 436960 match / 21 rename / 1671 value / 11700 missing / 10604
  extra` for both. The two reports differ only in the tie-break ordering of
  equal-count rows in the "top tags" listings; every numeric column is
  identical.

Note that this same-instrument baseline is itself 2 match / 2 value / 4 extra
away from the `436958 / 1673 / 10608` figure quoted when this follow-up was
handed over. The gap is present in the *unmodified* tree, so it predates this
change and is not explained by it; it is recorded here rather than reconciled,
since the earlier figure's build profile and tree state are not recoverable
from the number alone.

## Subset generation: 37 tables, empirically round-tripped

Selected across 15 modules (Canon, CanonCustom, CanonRaw, CanonVRD, FujiFilm,
Jpeg2000, Minolta, MinoltaRaw, Nikon, NikonCapture, NikonCustom, Olympus, PNG,
Panasonic, PanasonicRaw, Pentax, Photoshop, QuickTime, Samsung, Sigma, Sony),
spanning both high- and low-confidence carrier entries.

| Outcome | Tables |
|---|---:|
| Selected | 37 |
| **Tested** (write accepted, both round trips ran) | **24** |
| `WRITE_NOOP` (carrier body has no MakerNote block for this table) | 8 |
| `NO_ELIGIBLE_FIELDS` (every field depends on an unreproduced ValueConv/Condition/Hook, or is itself a SubDirectory pointer) | 3 |
| `WRITE_FAILED` (tag genuinely rejected by exiftool on this carrier) | 2 |

Of the 24 tested tables, **21 lit up** — at least one previously-dead tag read
correctly by oxidex.

| Metric | Count | % of attempted |
|---|---:|---:|
| Tags attempted | 375 | — |
| ExifTool round-trip OK (exiftool itself accepted the write) | 304 | 81.1% |
| **oxidex read correctly** | **284** | **75.7%** (93.4% of the 304 that round-tripped) |

Full per-tag results: [`corpus-synthesis-subset-report.json`](./corpus-synthesis-subset-report.json).

### What WRITE_NOOP actually measures

8 of 37 selected tables (22%) came back "0 image files updated" even on a
carrier from the right manufacturer — the body just doesn't have that
MakerNote sub-block (an older or newer camera than the table applies to). The
harness retries each vendor-dir table against a small fallback list of bodies
(old + new generation) before giving up; this cut the NOOP rate roughly in
half versus the first pass (16/37 → 8/37). **This is the single biggest
caveat on the 305-synthesizable classification number**: "carrier exists in
the corpus" is necessary but not sufficient — the *specific sample chosen*
also has to carry that table's block, and for tables gated to particular
camera generations, one corpus file per manufacturer will always miss some.

### Specific failure modes found

- **Genuine coverage gap, not a value bug**: `CanonRaw:ImageInfo` (3/3
  fields wrote and round-tripped through exiftool on a real `.crw` file; 0/3
  read by oxidex — `ImageWidth`/`PixelAspectRatio`/`Rotation` all `MISSING`).
  `Olympus:AFTargetInfo` and `Olympus:SubjectDetectInfo` similarly: every
  field round-tripped through exiftool cleanly, none read by oxidex. The
  table is transcribed and the carrier genuinely has the data; nothing wires
  it into the Olympus/CanonRaw MakerNote parsers.
- **Suspected byte-offset/decode bug, not a coverage gap**: `Minolta:CameraSettings`
  had 31/31 fields round-trip through exiftool but only 25/31 read correctly
  by oxidex — and the 6 mismatches aren't missing, they're *wrong but
  plausible* values (`WhiteBalance` written as raw 3 → oxidex reports "Auto"
  instead of "Tungsten"; `ISOSetting` written 0 → oxidex reports 65 instead of
  100; `MeteringMode`, `Sharpness`, `ExposureMode` all similarly off).
  Consistent wrong-but-plausible values across several fields in one table is
  the signature of a field-offset or endianness bug in oxidex's
  `Minolta::CameraSettings` decoder, not a missing-field gap — flagged
  separately for follow-up (see below), since fixing the classification
  script or this harness cannot resolve it.
- **Partial coverage**: `PanasonicRaw:WBInfo` (8/15 round-tripped through
  exiftool at all — several fields, e.g. `WBType4`–`WBType7`, aren't
  writable/readable on this file via the normal tag name at all, a
  registry/naming issue independent of oxidex) — of the 8 that did
  round-trip, 4 read correctly by oxidex.

## Does this carry Step 28's enablement gate?

**Partially, and unevenly across manufacturers.** The headline number — 305
of 592 unreachable tables (51.5%) are synthesizable, and the 24-table subset
that actually got tested lit up 21 tables (87.5%) with a 75.7% per-tag oxidex
read rate — says corpus synthesis is a real, substantial lever, not a token
one. But:

- It is structurally capped well under half of the total gap: 264 tables
  (44.6%) are unwritable in ExifTool regardless of sample availability, and
  synthesis cannot touch them at all. Those need real-world samples or stay
  permanently untested by any conformance gate keyed on writability.
- Coverage is lopsided by manufacturer: Nikon/Canon/Pentax/Sony account for
  243 of the 305 synthesizable tables (80%) because the corpus has deep
  vendor directories for them; Casio/Kodak/Ricoh/Reconyx/Sanyo/FotoStation/
  Microsoft/Nintendo (23 tables) are synthesizable in principle but blocked
  purely on missing corpus samples for those specific vendors.
- The 22%→~10% WRITE_NOOP rate (after fallback-body retries) means the
  classification's "carrier available" signal is optimistic per-table; a
  real Step 28 pipeline would need either multiple bodies per manufacturer or
  a per-table carrier-selection step, not one file per vendor.
- Where oxidex *is* wrong on a synthesized table, this harness cannot tell a
  coverage gap (nothing wired up, e.g. CanonRaw/Olympus above) from a decode
  bug (wrong field read, e.g. Minolta above) without per-table manual
  inspection — the gate would need that same triage step, not just a
  pass/fail count.

So: strong enough to justify building the Step 28 engine around it for the
manufacturer-rich half of the gap (Nikon/Canon/Pentax/Sony/Panasonic/Minolta/
Sigma/FujiFilm/Samsung/Olympus — most of the 305), but it cannot be the
*whole* gate. The 264 genuinely-unwritable tables need a different signal
entirely (real-world samples or accepting them as permanently untestable),
and the 23 sample-starved-but-writable tables are one small corpus addition
away rather than a synthesis problem.

## What was run vs. not run

- Ran: full 613-table classification against the pinned oracle's own
  `dump_tables.pl` output; a 37-table generation/round-trip subset across 15
  modules and both high- and low-confidence carrier entries.
- Not run: generation for all 305 synthesizable tables (out of scope per the
  task — "measure, don't over-build"); BITMASK-field and List/struct-field
  writing (skipped by design, see `synth_generate.py`'s field-eligibility
  filter — a `Mask`-bearing field is still attempted, but a field flagged
  `Omitted.value_conv`/`raw_conv`/`condition`/`hook`/`subdirectory` by the
  transcription itself is not, since writing a raw number under an
  unmodeled `ValueConv` and comparing it to a converted read would not be a
  meaningful measurement); the `MPF` "maybe embedded in vendor JPEGs"
  uncertainty was left unresolved (classified `none`, not probed).

## Instrument

Every number above: pinned exiftool `13.59` via
`/tmp/oxidex-exiftool-cache/exiftool-pinned.sh` (re-probed this run: `-ver` →
`13.59`, `OOXML.docx` → `FileType: DOCX`); oxidex release binary at commit
`cbc6618f`; corpus `/tmp/oxidex-exiftool-cache/combined-samples` (4,238
files, 13 manufacturer subdirectories + ExifTool's own single-exemplar test
files); `dump_tables.pl` run against the same pinned tree (153 modules
loaded, 0 failed).
