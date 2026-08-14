---
outline: 2
---

# The BinaryData engine and its two enablement gates (Step 28)

::: info Instrument
Everything measured here used the pinned **exiftool 13.59**
(`.exiftool-version`) via `/tmp/oxidex-exiftool-cache/exiftool-pinned.sh`,
re-probed for this run: `-ver` → `13.59`, and `OOXML.docx` →
`FileType: DOCX` (which is the probe that catches a `-ver`-correct tree whose
`Archive::Zip` is missing — see `AGENTS.md`). Corpus:
`/tmp/oxidex-exiftool-cache/combined-samples`, 4,238 files, `--recursive`.
oxidex: **debug** build (a release build is gated centrally and was not run),
control at commit `c3a7508e`.
:::

## What was folded

Before this step the repository carried three independent ports of
`Image::ExifTool::ProcessBinaryData` (ExifTool.pm:9877). Each implemented a
different subset of the same function, and the union of their gaps was the
gap:

| port | drove | had | lacked |
|---|---|---|---|
| `exiftool_tables::runtime::decode_binary_table` | the 613 generated tables | `Mask`, fractional keys, the five `Omitted` refusals | `varSize`, negative indices, `ReadValue`'s count shortening, sub-directory recursion |
| `makernotes::shared::binary_subdir` | the per-vendor `codegen_subdirs.py` tables | `Condition` groups, `DataMember` set/gate, count shortening, `PRIORITY => 0` | `varSize`/`Hook`, negative indices, recursion |
| `makernotes::canon::camera_info` | Canon `CameraInfo` | `varSize` + `Hook`, negative indices, `PRIORITY => 0` | count shortening, `DataMember` gates |

`src/exiftool_tables/engine.rs` is now the single port of the arithmetic and
the reader, and all three drive it:

* **`Cursor`** — ExifTool.pm:9957-9964. `int($index) * $increment + $varSize`,
  the negative-index wrap (`$entry += $size; next if $entry < 0`), and
  `last if $more <= 0`. `Step::Skip` and `Step::Stop` are kept distinct
  because ExifTool's `next` and `last` are not interchangeable — a `Hook`
  that adds `0x10000` is ExifTool's own way of ending a walk.
* **`read_value`** — ExifTool.pm:6286-6332. Count shortening
  (ExifTool.pm:6301-6303: `$count = int($size/$len); $count < 1 and return
  undef`), and `string`/`undef` as ONE value spanning every byte
  (ExifTool.pm:6307-6311) with `string` truncated at the first NUL and not
  trimmed. ExifTool's `string[8]` is `format => 'string', count => 8, len =>
  1`, so the shortening rule is per byte; the generated schema folds the `[8]`
  into `Fmt::Str`'s payload, and `read_value` un-folds it.
* **`process_binary_data`** — the full walk, including ExifTool's key order
  (ExifTool.pm:9917: negatives sort last), `_variants` first-match-wins
  interleaved into that one key space, `Mask`/`BitShift` (ExifTool.pm:10079),
  `PRIORITY => 0` (ExifTool.pm:9471), and `SubDirectory` recursion
  (ExifTool.pm:10102-10151) over Step 27's compiled `SubdirEdge`, guarded by
  ExifTool's own `$$self{PROCESSED}` cycle check (ExifTool.pm:9065-9072).

The conversion layers were **not** folded. `binary_subdir` and `camera_info`
carry hand-written `ValueConv`/`PrintConv` ports that the mechanical
transcription deliberately refuses to reproduce; merging them onto the
generated schema's conversions would not merge three engines, it would delete
tags.

`var_*` formats (ExifTool.pm:9986-10047) are still refused, not implemented:
their width is data-dependent, and the table records the resulting hazard as
`offsets_sound_until` instead. `Hook` bodies are Perl closures; the engine
provides the `varSize` seam a Hook moves (`Cursor::shift`) and never invents a
Hook's arithmetic.

## The two gates

A table is walked by the engine only if **both** pass. Opt-in: off until
proven (design D1).

**Gate A — static soundness**, computed by `codegen.py` and emitted into
`binary_tables.rs` as `GateA { blocked_by }`. A table passes when every field
ExifTool declares was either fully transcribed or emitted with an explicit
`Omitted` flag, every `PrintConv` was reproduced exactly, every
`SubDirectory` edge compiled, and no refused `var_*` field left a live
`offsets_sound_until` hazard.

The distinction that matters: a field carrying a refusal flag is *fine* — it
is withheld, loudly and countably. A field the generator **dropped** is not:
it is absent with nothing marking its place, so nothing downstream can tell
"ExifTool has no tag here" from "we could not transcribe the tag here". A
**dropped conversion** is worse than either, because the field is still
emitted and reports a raw number where ExifTool prints a string — a plausible
wrong VALUE under a real tag name.

**Gate B — dynamic conformance**, measured per table with
`tools/exiftool-tables/conformance.py` against the pinned oracle: enabling the
table must yield only MISSING→matched, with zero new group-qualified VALUE and
zero new EXTRA. The allowlist is `src/exiftool_tables/enabled.rs`, one line
per table with its evidence; per-table revert is that one line.

## Measured: 613 tables

Run `just reachability` (or `just reachability docs/reference/step28-reachability.json`
for the per-table JSON, committed alongside this page) — the census is **generated** from the committed
artifacts (gate A out of `binary_tables.rs`, gate B out of `enabled.rs`), so
it cannot disagree with what it describes. `cargo test
every_table_lands_in_exactly_one_enablement_class` pins the same split from
the Rust side.

| class | count | meaning |
|---|---:|---|
| enabled | **5** | both gates; the engine walks it |
| eligible | **350** | gate A passes, no gate B measurement possible yet |
| refused | **258** | gate A blocks it |

Gate A's refusal reasons, by tables affected (a table can trip several):

| reason | tables |
|---|---:|
| `expr_unsupported` | 141 |
| `tag_fmt_unsupported` | 86 |
| `enum_int_partial` | 50 |
| `enum_str_partial` | 16 |
| `conv_dropped` | 14 |
| `tag_variant_skipped` | 9 |
| `tag_variant_cond_unsupported` | 8 |
| `tag_var_format` | 7 |
| `offsets_sound_until` | 4 |
| `tag_bad_index` | 4 |
| `subdir_refused_processproc` | 1 |
| `tag_variant_field_unsupported` | 1 |

This is design D3 read literally: a construct we cannot parse counts against
us. 258 refusals is the scoreboard, and `expr_unsupported` alone — 141 tables
blocked because at least one `PrintConv` expression did not translate — says
where the next unit of work buys the most enablement.

## Why only 5 are enabled

Enablement needs a table to be *reached* — something has to route bytes into
it. Two routes exist, and both were measured:

1. **Hand-wired `find_table` call sites.** 21 distinct tables, independently
   reproducing the count in
   [corpus-synthesis](/reference/corpus-synthesis). Of those, 6 pass gate A,
   and 5 of the 6 have a live call site (the sixth, `Ricoh::ImageInfo`, is
   named only inside a comment explaining that the module does *not* call it
   — `reachability.py` now strips comments before counting, because the first
   version of that script did not and reported it as reachable).
2. **`SubdirEdge` recursion**, which Step 27 compiled the edges for and this
   step built the walk for. In the pinned 13.59 tree there are 64 edges from
   43 tables, but **exactly one** hangs off a hand-wired table:
   `CanonVRD::Ver2 → CanonVRD::DLOInfo`. `DLOInfo` trips
   `tag_fmt_unsupported=1`, so gate A blocks it. **Subdirectory recursion
   therefore enables nothing yet.** That is a measurement, not an omission —
   and it is the concrete thing to fix if the next step wants the edges to pay.

The remaining 350 eligible tables have no live call site at all. Enabling one
would produce no tags and no measurement — enablement on no evidence, which is
what opt-in exists to prevent.

## Conformance A/B

Both runs: same corpus, same pinned oracle, same `--min-files 3875
--min-tags 5000` floors.

```
control  (c3a7508e, pre-Step-28)
TOTAL  4238  437050 match  21 rename  1671 value  11610 missing  10602 extra   97.0% / 97.1% / 97.6%

Step 28 (5 tables enabled, all three engines folded)
TOTAL  4238  437050 match  21 rename  1671 value  11610 missing  10602 extra   97.0% / 97.1% / 97.6%
```

**Every delta is zero.** Concretely:

* **0 new VALUE, 0 new EXTRA** — gate B's pass condition, met by all 5.
* **0 MISSING→matched** — the enabled tables gained nothing either. The
  shared `ReadValue`'s count shortening only differs from the old strict read
  on a record truncated mid-field, and no file in this corpus truncates one of
  those five tables.
* The two engine folds (`camera_info.rs` and `binary_subdir.rs` onto
  `Cursor`/`read_value`) are on the hot path for every Canon, Pentax,
  Panasonic and Sony file in the corpus, and are **behaviour-preserving across
  all 4,238 files** — which is the result those folds needed and the only
  claim a null delta can carry.

The one non-zero difference between the two reports is tie-break ordering
inside equal-count listings (`VCARD Email`/`Sound`, four 41-file `MPImage3:*`
rows). No count moved.

### A note on the starting baseline

The task's stated starting point was `TOTAL 4238 436945 / 21 / 1686 / 11700 /
10608`. The control measured here is `437050 / 21 / 1671 / 11610 / 10602`
(+105 match, −15 value, −90 missing, −6 extra). That gap is **not** an effect
of this step: it is the same pre-Step-28 tree, measured on this host, and the
difference is attributable to commits already on the branch
(`baccab9e` routed MPC and standalone MIE; `ab628bbb` landed composite
gating) plus a debug rather than release build. Both numbers are reported
rather than reconciled, per `AGENTS.md`: the comparison that means anything is
control-vs-change on one instrument, and that comparison is the zero above.

## What was run, and what was not

* **Ran:** full-corpus conformance twice (control and shipped configuration,
  4,238 files each); `just verify-tables` → **PASS** (68 subdirectory edges,
  6,895 hook/subdirectory flags, 1,563 variant enum entries, 0 mismatches);
  `cargo test --workspace` → 4,202 lib tests + all integration suites green;
  `cargo fmt --all`; `cargo clippy --all-targets` (no new warnings from this
  change); `just reachability`; a byte-identical regeneration check on
  `binary_tables.rs` before touching `codegen.py`.
* **Not run:** `just ci-standard` and any release build (gated centrally);
  gate B for the 350 eligible tables (they have no call site, so there is
  nothing to measure); the corpus-synthesis harness
  (`tools/exiftool-tables/synth_*.py`) as an enablement gate — it measures
  whether a table's tags can be *written and read back*, which is a different
  question from whether enabling it regresses conformance, and the 5 tables
  actually enableable here all had real corpus coverage already.
