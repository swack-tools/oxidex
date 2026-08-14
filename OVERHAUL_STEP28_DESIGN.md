# Step 28 design checkpoint — one BinaryData engine, instrument-gated enablement

**Status: DECIDED by the maintainer 2026-08-13. Implementation authorized, with one
strategy change that is larger than the questions I asked.**

- **D4 — FOLD ALL THREE ENGINES AT ONCE.** Overrides my incremental recommendation.
- **D1/D2 — the maintainer redirected the question**: rather than choosing which corpus
  gates enablement, *manufacture the corpus*. Use ExifTool itself to WRITE the tables onto
  sample files, so a table with no natural sample gets a synthetic one.
- **D3 — the denominator is ALL of ExifTool's source.** Anything ExifTool can parse is a
  target, and a construct we cannot parse COUNTS AGAINST US. Refusals are not neutral
  bookkeeping; they are the gap.

**The synthesis idea is verified working.** Writing into a real binary table and reading it
back through the pinned oracle:

    $ exiftool -overwrite_original -Canon:SelfTimer=5 Canon.jpg
    $ exiftool -a -G1 -s -Canon:SelfTimer Canon.jpg
    [Canon]  SelfTimer : 5 s
    $ exiftool -v3 Canon.jpg | grep CameraSettings
      | | | 0)  CanonCameraSettings (SubDirectory) -->

There is already precedent in-repo: `jpeg-tag-matrix` writes a value with ExifTool into a
clean JPEG and requires oxidex to read it back — 4,819 tags are already tested this way.
Step 28's enablement gate becomes an extension of that harness rather than a new mechanism.

REACH, measured — synthesis is powerful but NOT universal:
- ExifTool declares **7,235 writable tags** in total.
- **424 of 726 ProcessBinaryData tables (58%)** carry table-level `WRITABLE`/`WRITE_PROC`.
- The remaining ~42% cannot be synthesised: encrypted Nikon tables, computed/derived
  values, and read-only records. Those still need real samples or stay refused-and-counted.
- Synthesis also needs a CARRIER of the right make: a Canon table needs a Canon file,
  because the MakerNote must be present for ExifTool to write into it.

(Measurement caveat recorded so nobody repeats it: `exiftool -listw -Canon:all` does NOT
filter by group — it returns the global 7,235 for every group. Per-group writable counts
need a different approach.)

Orchestrator-authored (plan rule 7 — never delegated). Committed to the branch so step
worktrees carry it; the Step 10 and Step 15 artifacts were untracked and invisible to their
implementing agents.

Measured on the integration tip with Steps 18–23 merged, pinned ExifTool 13.59.

---

## 1. The seam, measured

`codegen.py` emits **613 tables / 6,670 tags**. At runtime, `find_table(...)` names
**22 distinct tables**, across 14 files calling `decode_binary_table`.

**So 591 of 613 transcribed tables are dead data.** They are verified against the oracle by
`verify-tables`, they cost bytes in the binary, and nothing ever reads them. That is the
"22-of-612 reachability seam" the plan names, re-measured at 22-of-613 after Step 23.

This is why bump economics have not moved. Step 17's real 13.58→13.59 triage put the AUTO
share at **36.5%** — the machinery absorbs a third of a release unaided. Most of the
remainder is not *untranscribed*; it is transcribed-and-unreachable.

## 2. What the schema already carries

From the current codegen report — this is what an engine would have to work with:

| Recorded and APPLIED | |
|---|---|
| tables / tags | 613 / 6,670 |
| variant tags compiled (Step 23) | 100 |
| exprs translated | 842 (407 exact-match + 435 grammar-compiled) |
| int / string enums | 2,327 / 83 |
| masked fields · bit fields | 1,106 · 1,026 |

| Recorded but NOT APPLIED — the engine's actual work list | |
|---|---|
| ValueConv | 1,226 |
| RawConv | 718 |
| Condition | 744 |
| SubDirectory | 68 |
| Hook | 35 |
| bit fields with no Mask | 11 |

| Refused outright, counted with reasons | |
|---|---|
| exprs unsupported | 308 |
| other PrintConv | 65 |
| variant tags | 12 (11 conditions outside the closed grammar) |
| unsupported format | 154 (15 `var_*`) |
| offsets unsound past a refused `var_*` | 4 tables / 84 fields |

The top untranslated expressions are concentrated and nameable:
`$self->ConvertDateTime($val)` 65 · `sprintf("%.2g",$val)` 30 ·
`Exif::PrintFraction($val)` 24 · `ConvertDuration($val)` 21 · `ConvertBitrate($val)` 14.

## 3. The design

Grow `decode_binary_table` into the full ProcessBinaryData port — folding in
`binary_subdir.rs`'s variants/members, `camera_info.rs`'s varSize + Hook + `PRIORITY => 0`,
negative indices, subdirectory recursion, and ReadValue's string rules — then retire the
three separate engines.

**Enablement is per table, via two generated gates. A table is enabled only if it passes both.**

**Gate A — static soundness.** Every field in the table is either fully transcribed or
carries an explicit refusal flag, and the table has no `offsets_sound_until` hazard. Purely
a property of the generated data; computable at codegen time, no corpus required.

**Gate B — dynamic conformance.** Enabling table T on the corpus yields *only*
MISSING→matched transitions: zero new group-qualified VALUE, zero new EXTRA. Measured with
Step 12's instrument against the pinned oracle.

The allowlist diff per enabled table is the review artifact. Per-table revert is one line.

## 4. Why gate B has to be per-table and adversarial

This session produced the argument. Step 22 replaced a hand-tuned rank table with real
arbitration and moved **2,038 rows** match→value before root-causing down to 41 — none of
which was visible from the diff looking correct. Step 21's `TargetPrinter` and Step 15's
DJI regression both passed their authors' own checks and were caught only by a gate the
author did not run.

An engine that enables 591 tables at once would produce a delta nobody can attribute. Enabling
one table at a time, with a measured before/after, is what makes a regression traceable to
its cause.

## 5. Decisions I need

**D1 — Enablement default.** Opt-in (a table is off until it passes both gates and is
explicitly listed) or opt-out (on by default, listed off when it fails)? **Recommendation:
opt-in.** 591 tables have never executed; assuming they work inverts the burden of proof,
and the project's doctrine is that absence beats a confident wrong value.

**D2 — Gate B's corpus.** `t/images` (194 files, fast, the pinned oracle's own suite) or
`combined-samples` (4,238 files, ~10 min/run)? **Recommendation: combined-samples for the
enablement decision, t/images for iteration.** A table that only appears in manufacturer
samples would otherwise be enabled on no evidence at all.

**D3 — What happens to a table that passes A but has no corpus coverage.** Many of the 591
have zero sample files, so gate B is vacuous for them — it cannot fail, and it cannot
confirm. Options: (a) enable on gate A alone and count them separately as
"enabled-unverified"; (b) keep them off until a sample exists. **Recommendation: (a) with a
distinct counter**, because keeping them off makes coverage permanently hostage to corpus
acquisition — but the count must be published per bump so "enabled" is never confused with
"verified". This is the decision most likely to change the shape of Stage 6.

**D4 — Scope of the engine fold in one step.** Step 28 is rated L and is the highest-risk
item on the board. Fold all three engines at once, or land the engine first with the three
existing call paths untouched and migrate them one at a time? **Recommendation: land the
engine and migrate incrementally.** Step 22's flattening-bug recurrence — the same bare
`iter()`-then-`insert()` shape found three separate times — is evidence that big-bang
replacements in this codebase leave residue.
