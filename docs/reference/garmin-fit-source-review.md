# Garmin FIT source review (ExifTool 13.59)

Status: source review for the first resumed parity milestone, 2026-09-15.
Everything below was read from the pinned `Image/ExifTool/Garmin.pm`
(`ProcessFIT`, lines 6293-6592) and the hydrated table dump. Line numbers
refer to that pinned file. Nothing here is an observation of OxiDex output;
see `tools/exiftool-tables/GARMIN_FIT_READER.md` for the generated
acceptance, and [Observed reading](#observed-reading) below for the native
comparison.

## What the source tables are, and are not

`Garmin::FIT` is a message map: 171 numeric message numbers, each a
`SubDirectory => { TagTable => ... }` edge with no other SubDirectory key, plus
the non-numeric keys `vers` (header `ProtocolVersion`) and `Common` (a hidden
edge to the three fields shared by every message). 160 of the 171 message
edges carry `Unknown => 1`; one of them, `Pad` (105), has no table at all.
The 170 message tables plus `Common` and `Dev` hold 1,725 field rows; with the
173 rows of the map itself the family has 1,898 source rows.

These tables have no `PROCESS_PROC`, so the IFD compiler emits them as
`IfdTable` schemas. That is an accident of the dump's processor
classification, not a FIT runtime. Its IFD edge semantics (`Start`, `Base`,
entry-declared formats) are not FIT semantics, and it sets
`omitted.subdirectory` on every message edge. The 927 "eligible" IFD schema
rows are therefore candidate inventory only. The FIT reader does not consume
the IFD schema.

The IFD compiler withholds almost every Garmin conversion as
`ifd_expr_domain_unknown`: FIT rows declare no `Format`, `Writable` or
`Count`, so no static input domain exists. In FIT the domain is fixed per
field at run time by the definition message's base type. The conversions
themselves are not the obstacle. Against the same pinned dump, the shared
expression compiler (`exprs.py`) compiles and the oracle PASS ledger verifies
722 numeric and 58 string PrintConv expressions, 533 ValueConv expressions and
56 RawConv expressions. One ValueConv (`GPS` field 7,
`my @a = join " ", map { $_ / 100} split " "`) does not compile.

## Protocol semantics and the capability each requires

| Semantic | Source | Required capability |
| --- | --- | --- |
| Header: `hdrLen = byte 0`, `dataLen = u32le at 4`, `ProtocolVersion = byte 1` (FIT table, group1 `File`); records end at `hdrLen + dataLen`; CRC not checked | 6307-6319 | Framing executor; `vers` bound as a protocol row |
| Normal header: local number = low 4 bits, bit 0x40 = definition, bit 0x20 = developer definitions follow | 6337-6340, 6404 | Framing executor |
| Compressed header: local = bits 5-6; a nonzero 5-bit offset rolls the running timestamp and overwrites that local definition's `TS` with `[253, ts]`, persisting for later normal-header records of the same local number; the update happens before the first-message gate | 6323-6336 | Timestamp state machine that reproduces the persistent overwrite |
| Definition: architecture byte selects byte order (any nonzero = big-endian); global number `u16` in that order | 6348-6352 | Framing executor |
| Message group: FIT table lookup by global number; an absent number becomes `Unknown<num>` with a synthesized table (group1 `Unknown<num>`, group2 `Unknown`) flagged Unknown | 6355-6375 | Generated message map plus explicit unknown-message handling |
| Field list is retained only for messages not flagged Unknown (default options) | 6381-6383 | Option-dependent extraction |
| A field whose base type is not in `%baseType` warns and is left out of the field list, but its size still counts toward the record size. Later fields' read offsets therefore shift. | 6386-6401 | Exact field-list construction, including this offset behavior |
| `TS` records field 253; `IsTimeStamp` can set `TS` only when no field 253 exists, and such a `TS` is never read (6460 requires `TS[0] == 253`). `IsTimeStamp` therefore has no output effect in 13.59. | 6389-6394, 6460 | Captured as a source fact; no runtime behavior |
| First-message gate: without ExtractEmbedded, only the first data record of each global number is processed | 6438-6445 | First-message handling |
| Timestamp: the first element of field 253, read without the invalid-value check; a change increments `DOC_NUM`. A message with no field list, or whose `TS` came from a compressed header, reports `Common:TimeStamp` under the message's group1 | 6460-6482 | Timestamp state machine; sub-document instance per `DOC_NUM` |
| Field lookup: message table, else `Common` (250 `PartIndex`, 253 `TimeStamp`, 254 `MessageIndex`) with group1 set to the message's, else a synthesized Unknown `<Msg>_<num>` tag | 6492-6502 | Common fallback with group override |
| Developer fields decode only from values captured from `DeveloperDataID` (207) and `FieldDescription` (206) records. Both messages are flagged Unknown, so under default options no values are captured, every developer field is skipped by size, and no warning is issued. | 6503-6546, 6581-6590 | Default mode: exact size skip. Decoding requires Unknown mode |
| Base types: `%baseType` gives the format, FIT name and invalid value; count = size / format size, and a non-integral count warns `Bad count ...`; a value whose text equals the invalid value is dropped (so multi-element arrays of sentinels are kept) | 25-43, 6549-6575 | Source-captured base-type table |
| `undef` (FIT `byte`) values reach HandleTag as scalar references | 6560 | Binary placeholder |
| Conversions: tag `RawConv`/`ValueConv`/`PrintConv` through ordinary `HandleTag` | 6560 | Runtime-typed conversion domains |
| Minor warning `Use ExtractEmbedded option to extract all timed metadata` without ExtractEmbedded; errors `Truncated ...`, `Missing definition ...` end the walk | 6313, 6588 | Warning emission |

Perl detail relevant to exact values: `Get64u`/`Get64s` (Writer.pl) compute
`$hi * 4294967296 + $lo`. Perl 5.38.2 with 64-bit integers keeps this exact
(a probe reads `0x0020000000000001` as `9007199254740993`), so 64-bit values
render as exact decimal integers. `GetFloat`/`GetDouble` produce Perl numbers
that print with 15 significant digits.

## Option dependence

OxiDex exposes neither `-u` (Unknown) nor `-ee` (ExtractEmbedded). The only
reachable mode is ExifTool's default, where:

- 11 messages have field lists: Session, Lap, Record, GPS, SegmentPoint,
  CoursePoint, ClimbPro, Jump, Split, WeatherConditions and SegmentLap;
- the other 160 messages contribute only a `TimeStamp` when it changes;
- developer fields are never decoded;
- only the first record of each message number is read.

Unknown-mode and ExtractEmbedded-mode behavior stay explicitly refused as
`option_not_exposed`; they are not reachable, and they are not claimed.

## Chosen first capability

A generated FIT message executor with runtime-typed conversion domains:

1. A sidecar, `capture_garmin_fit_fact.pl` (in the style of
   `capture_raw_jfif_fact.pl`), captures the protocol facts that are missing
   today: the `ProcessFIT` body, the live `%baseType` pad, format sizes, the
   `IsTimeStamp` values, integer width, and the bodies of the value readers
   it depends on, including the `Get64u`/`Get64s` bodies ReadValue autoloads
   from Writer.pl. The table dump itself is unchanged.
2. A generator builds the message map, every field row and its conversions
   from the dump. Expressions compile through the shared compiler and must
   carry a PASS in the oracle ledger. Each carries its compiled domain;
   refused rows stay present as withheld entries, each with its reason.
3. One handwritten executor applies the protocol above and applies a
   conversion only when the value's run-time domain equals the conversion's
   compiled domain. It never falls back to the raw value.

This replaces the handwritten Session matches while keeping their output, and
it avoids special cases for individual tags or field IDs. Rows needing Unknown
mode, ExtractEmbedded mode, list-domain conversions or the one uncompiled
ValueConv remain refused, each with its own reason.

## Observed reading

Instrument: `verify_garmin_fit_reader.py` (native and OxiDex `-j -a -G1`, in
print and `-n` modes, projected onto FIT groups), local M5, clean source
`ed5bf195`, OxiDex release binary built by the instrument, explicit Perl
5.38.2, pinned ExifTool 13.59 (DOCX capability probe passed), 18 fixtures
(17 synthetic behaviors plus `t/images/Garmin.fit`):

- 36 of 36 comparisons passed: no value difference, no OxiDex-only identity,
  and no missing identity except the declared list-domain refusal.
- 125 distinct group-qualified FIT identities matched (plus
  `ExifTool:Warning`), 220 matched occurrences in print mode.
- They resolve to 117 distinct catalog coordinates: Session 57, Lap 45,
  Record 10, GPS 2, Common 2 and the FIT header row. `Record:GPSAltitude` and
  `Record:GPSSpeed` each name two Record rows, so no coordinate is credited
  for them.
- The real `Garmin.fit`: all 121 projected native identities matched in both
  modes. The replaced handwritten reader reported 8 Session tags under the
  family-0 key with no family-1 group, so none matched under this instrument.

Corpus check, same host and source: `conformance.py` over the 194 files of
the pinned `t/images` (family-0 view; OxiDex default `-j`, oracle
`-G0:1:4 -a`). Base `b52e73ed` versus `ed5bf195`: FIT match 11 to 78, missing
114 to 47; totals match 10,081 to 10,148 and missing 1,467 to 1,400, with
VALUE (30), RENAME (8) and EXTRA (668) unchanged and every non-FIT format row
identical. The 47 FIT rows still missing in that view are second occurrences
of names reported by several messages (Session and Lap both report
`AvgHeartRate`): OxiDex's default JSON prints one value per family-0 key.
Under the group-qualified instrument above, every occurrence matches.

Evidence (durable, outside the repository):
`~/oxidex-ops/evidence/20260915-claude-parity-resume/` —
`native-read-ed5bf195/comparison.json`, `conformance-timages-{base,new}.json`,
`regen-2/` (canonical `regen-all.sh`, zero net delta) and `gate-1/`.
No observed writing is claimed.
