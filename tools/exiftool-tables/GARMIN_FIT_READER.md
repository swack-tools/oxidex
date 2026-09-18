# Garmin FIT generated reader

The FIT reader is one handwritten executor (`src/parsers/specialized/fit.rs`)
for ExifTool 13.59's `Garmin::ProcessFIT`, driven entirely by generated data
(`src/exiftool_tables/fit_tables.rs`). The source review behind it is
[garmin-fit-source-review.md](../../docs/reference/garmin-fit-source-review.md).

## Pipeline

```text
pinned Garmin.pm
  -> dump_tables.pl: Garmin tables (unchanged dump)
  -> capture_garmin_fit_fact.pl: ProcessFIT and value-reader bodies, live
     %baseType pad, format sizes, IsTimeStamp values, Perl integer width
  -> garmin_fit_specs.py (called by codegen.py before the ExprId enum freezes)
  -> fit_tables.rs + garmin_fit_ledger.json + fixtures/garmin_fit_source.json
  -> fit.rs (generic executor)
  -> verify_garmin_fit_reader.py (native comparison)
```

Conversions compile through the shared `exprs.py` grammar and must be in the
oracle PASS ledger (`expr_oracle_ledger.json`). A FIT field's format comes
from the file, so each conversion carries the domain it was compiled for
(`runtime::TypedConv`); `runtime::apply_typed`/`render_typed` run it only on
a value in that domain and otherwise report `DomainMismatch`, which the
executor turns into a withheld tag, never a raw fallback.

## Releases without the Garmin module

`Image::ExifTool::Garmin` first shipped in ExifTool 13.56 (13.59's `Changes`:
"Added read support for Garmin FIT files"). Older releases, such as 11.78 and
12.64 in the upgrade rehearsal, have no `Image/ExifTool/Garmin.pm` and route
the `.FIT` extension to FITS. For them the FIT reader has three states, never
two:

| State | Fact | Ledger | Rust `FIT_PROTOCOL.refusal` |
| --- | --- | --- | --- |
| present, admitted | `garmin_fit_reader_protocol_v1` | rows, `protocol.admitted: true` | `None` |
| present, refused | `garmin_fit_reader_protocol_v1` (changed bodies) | rows withheld, `protocol.reasons` | `Some(FitUnavailable::Refused(..))` |
| module absent | `garmin_fit_module_absent_v1` | `module_state: "absent"`, zero rows | `Some(FitUnavailable::ModuleAbsent { .. })` |

`capture_garmin_fit_fact.pl` records absence only from the selected tree
itself: `Image::ExifTool` loads from it, `stat` of
`Image/ExifTool/Garmin.pm` fails with `ENOENT`, no `.pm`/`.pl` in it declares
or references the Garmin package, `ProcessFIT`, `Garmin::FIT` or a
`=> 'Garmin'` module mapping, and the fact binds a sha256 over every file in
the tree plus `Image/ExifTool.pm`'s own digest. The release label is recorded
but never consulted. `garmin_fit_specs.py` admits the fact only for the dump's
own release and only when the dump has no Garmin module. Anything else is a
failure, not an absence: a crashed capture (empty or unparsable fact) raises
in `codegen.py` and `garmin_fit_specs.py`, and a Garmin reference without the
module file, or a module that fails to load, makes the capture itself die.
Tests: `test_garmin_fit_module_absent.py`.

## What is and is not claimed

Generated from the pinned 13.59 dump (`garmin_fit_ledger.json`, 1,898 source
rows across the FIT message map and 172 field tables):

| Runtime connection | Generated | Refused |
| --- | ---: | ---: |
| Default-mode message field lists (Session, Lap, Record, GPS, SegmentPoint, CoursePoint, ClimbPro, Jump, Split, WeatherConditions, SegmentLap) | 594 | 1 |
| `Common` fields (`PartIndex`, `TimeStamp`, `MessageIndex`) | 3 | 0 |
| Header `ProtocolVersion` | 1 | 0 |
| Message edges (171, including `Pad`, whose table ProcessFIT synthesizes) and the `Common` edge | 172 | 0 |
| Fields of the 160 `Unknown`-flagged messages (need `-u`, not exposed) | 1,123 | 4 |
| **Total** | **1,893** | **5** |

Refusals: four `TrainingSettings` keys above 255 (`field_key_outside_u8_protocol`:
ProcessFIT reads field numbers as one byte, so these rows cannot be reached)
and `GPS` field 7 (`value_conv_uncompiled`: `my @a = join " ", map ...` is
outside the closed expression grammar).

Of the 1,727 BuildTagLookup catalog entries in Garmin tables, 598 are now
generated and reachable in the default mode, 1,124 are generated but gated
on the unexposed Unknown option, and 5 are refused. Before this reader, the
same entries were IFD-schema declarations with no runtime (927 eligible,
799 withheld for lack of a static conversion domain, 1 refused).

Run-time refusals, not visible in the ledger: a conversion whose compiled
domain differs from the value's (notably a numeric conversion on a
multi-element value, which Perl interpolates as a joined list), an integer
of 16 or more digits under a numeric conversion, a hash PrintConv on a
string value, any conversion of a value exact only as Perl text (an
unsigned value above `i64::MAX`, a non-finite float; without a conversion
these print as that text), and a PrintConv returning `undef`. The walk stops,
keeping what it read, at a message edge the generator withheld, a compressed
header with no definition, and a timestamp that is not an `i64` integer.

Three different measurements, not to be combined:

1. **Generated declarations** (`garmin_fit_ledger.json`): source rows the
   generator accepted, with exact refusal reasons for the rest.
2. **Runtime connection**: whether ExifTool's default mode reaches the row.
   OxiDex exposes neither `-u` nor `-ee`, so rows in the 160 messages flagged
   `Unknown` are generated but connected only for their `TimeStamp`.
3. **Observed reading** (`verify_garmin_fit_reader.py`): group-qualified
   identities whose values matched pinned ExifTool on the fixtures below.

No writing is claimed; ExifTool does not write FIT.

## Regenerating

`tools/exiftool-tables/regen.sh` (or `regen-all.sh`) regenerates all three
outputs from a fresh dump. The bounded fixture lets the unit tests replay the
committed ledger and Rust without a full dump:

```sh
python3 -m unittest discover -s tools/exiftool-tables -p 'test_garmin_fit_specs.py'
OXIDEX_TABLES_JSON=<fresh full dump> python3 -m unittest discover -s tools/exiftool-tables -p 'test_garmin_fit_specs.py'
```

A new ordinary row in a Garmin table enters `fit_tables.rs` on regeneration
with no Rust edit (`RegenerationTests.test_ordinary_supported_row_appears_without_rust_edits`).
A changed `ProcessFIT` body, value reader or base-type capture refuses the
whole protocol until the executor is reviewed against it.

## Native comparison

```sh
python3 tools/exiftool-tables/verify_garmin_fit_reader.py \
  --exiftool-dir <pinned ExifTool tree> --out <new evidence directory>
```

One fixture per behavior, each compared in print and `-n` modes, projected
onto the FIT message groups, `File:ProtocolVersion` and `ExifTool:Warning`:

| Fixture | Behavior |
| --- | --- |
| `Garmin.fit` | The pinned tree's real activity file |
| `session-little.fit`, `session-big.fit` | Byte order; enum, unit, scaled, list and Common-fallback fields |
| `header-14.fit` | 14-byte header with CRC |
| `first-message-gate.fit` | Only the first record of a message is read |
| `unknown-message-timestamp.fit` | An Unknown-flagged message reports only its TimeStamp |
| `unlisted-and-synthesized.fit` | `Unknown<num>` messages and the table-less `Pad` edge |
| `compressed-timestamp.fit` | Compressed-header timestamp roll-over through Common |
| `unknown-base-type.fit` | Warning plus ProcessFIT's shifted reads after an unknown type |
| `invalid-values.fit` | Scalar sentinels dropped, sentinel lists kept, empty string and NaN |
| `bad-count.fit` | Non-integral count warning |
| `developer-fields.fit` | Developer fields sized and skipped in default mode |
| `float-string-int64.fit` | Float text, NUL-terminated string, exact 64-bit integer |
| `byte-field.fit` | `byte` values as binary placeholders |
| `position-conversions.fit` | RawConv, ValueConv and `ToDMS` PrintConv chain |
| `truncated.fit`, `missing-definition.fit` | Stream errors end the walk with the native warning |
| `developer-descriptions.fit` | `DeveloperDataID`/`FieldDescription` records are Unknown-flagged, so no developer tag appears |
| `text-only-values.fit` | Unsigned 64-bit above `i64::MAX`, `Inf`, and a list with `-Inf`, as Perl text |
| `negative-timestamp.fit` | Negative running timestamp through the compressed-header arithmetic |
| `list-domain-refusal.fit` | Declared refusal: a numeric conversion on a list value is withheld |

A MISSING identity fails the run unless it is declared for that fixture; a
value difference, an OxiDex-only identity (including one under an unexpected
group), a warning-multiset difference, or a fixture with no FIT identity
matched always fails. Warnings are compared as texts because ExifTool's JSON
writer keeps one entry per key (its text output lists every occurrence). Results for the
landed commit are in [the review record](../../docs/reference/garmin-fit-source-review.md#observed-reading).
