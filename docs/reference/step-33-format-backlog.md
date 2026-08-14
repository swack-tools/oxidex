# Step 33 format backlog (measurement, 2026-08-14)

This is the ranking half of Step 33 only.  It deliberately does not add a
parser or enable a table.  The MakerNote data-half migration belongs to Step
31 and is out of scope.

## Instruments and reproducibility

All file-level comparison numbers below are from
`python3 tools/exiftool-tables/conformance.py` against the single named
carrier in `/tmp/oxidex-exiftool-cache/combined-samples`, with
`--exiftool-dir /tmp/oxidex-exiftool-cache/exiftool` and
`--oxidex /home/allen/git/s33-codex/target/debug/oxidex`.  The oracle was
capability-probed immediately before the corpus run: the pinned wrapper
`/tmp/oxidex-exiftool-cache/exiftool-pinned.sh -ver` printed `13.59`, and
`... -s -FileType .../combined-samples/OOXML.docx` printed `DOCX`.

The measured binary was identified immediately before those runs by:

```text
38144a2c feat(tables): one BinaryData engine, with static and measured enablement gates
# git status --short: (empty)
```

The dispatch audit was `src/core/format_dispatch.rs`: an unsupported dispatch
falls through `read_metadata_report_with_detector_and_options()` to
`add_identity_tags` and is reported in JSON as `Status: IdentifiedOnly`.
The census ran that status check over every regular file in both supplied
carrier roots (realpath-deduplicated).  It found the 29 formats in the table
below.  This is evidence about supplied carriers, not a claim that a filetype
magic entry without a carrier has been exercised.

`R/M/V/E` means `RENAME/MISSING/VALUE/EXTRA`.  Each number in that column and
the `missing tags` column is from the named file-level conformance instrument;
the score and ceiling are its recall columns.  The identity tags explain the
usual one EXTRA: conformance intentionally treats precision separately from
recall.

For cost evidence, `tables` is the number of relevant
`src::exiftool_tables::find_table(module, table)` lookups that resolve over the
number of BinaryData tables dumped from the pinned Perl module; `fields` is
the corresponding pinned `%Image::ExifTool::<Module>::<Table>` field total
versus generated-field total.  Both came from
`perl -I/tmp/oxidex-exiftool-cache/exiftool/lib tools/exiftool-tables/dump_tables.pl ...`
and `src/exiftool_tables/binary_tables.rs`, not from `-listx` tag knowledge.
`n/a` means the handler is custom/IFD-oriented rather than a dumped
BinaryData table, so it would be wrong to invent a table denominator.

## Ranked IdentifiedOnly backlog

| Rank | detected format and carrier | missing tags | R/M/V/E; score -> ceiling | transcribed layout evidence | value / cost reasoning |
| ---: | --- | ---: | --- | --- | --- |
| 1 | MRC — `MRC.mrc` | 88 | 0/88/0/1; 3.3% -> 3.3% | `MRC`: 2/2 tables; 134/132 fields. `FEI12` is 98/96; `Main` 36/36. | Largest observed gap and virtually all layout is already transcribed; two omitted FEI12 fields must be accounted for, not guessed. |
| 2 | JP2 — `Jpeg2000.jp2` | 55 | 0/55/0/1; 5.2% -> 5.2% | `Jpeg2000`: 5/7; 82/14 fields. | Shares one implementation with J2C and already has box-table data, but `Main` (57 fields) and `JUMD` (5) are absent, so it is not a table-enable-only task. |
| 2 | J2C — `Jpeg2000.j2c` | 5 | 0/5/0/1; 37.5% -> 37.5% | Same `Jpeg2000` 5/7, 82/14 evidence as JP2. | Incremental value of the shared JP2/J2C reader; rank is shared rather than double-counting parser cost. |
| 3 | WTV — `WTV.wtv` | 69 | 0/69/0/1; 4.2% -> 4.2% | `WTV`: 0/2; 74/0 fields. | Very large observed gap, but only two source tables; hand-port cost is bounded and measurable. |
| 4 | PFM — `Font.pfm` | 26 | 0/26/0/1; 10.3% -> 10.3% | `Font::PFM`: 1/1; 24/24 fields. | Small, completely transcribed fixed record; good low-risk follow-up once a font route is established. |
| 5 | PMP — `Sony.pmp` | 23 | 0/23/0/1; 11.5% -> 11.5% | `Sony::PMP`: 1/1; 14/14 fields. | Complete PMP header table exists; remaining work is faithful PMP/JPEG container traversal. |
| 6 | PGF — `PGF.pgf` | 24 | 0/24/0/1; 11.1% -> 11.1% | `PGF`: 1/1; 9/9 fields. | Complete small table, but the carrier emits more than its nine table fields, so parser framing still needs proof. |
| 7 | CZI — `ZISRAW.czi` | 31 | 0/31/0/1; 8.8% -> 8.8% | `ZISRAW`: 1/1; 3/3 fields. | Table coverage is complete but tiny relative to the observed result; container traversal dominates. |
| 8 | PCX — `PCX.pcx` | 15 | 0/15/0/1; 16.7% -> 16.7% | `PCX`: 1/1; 15/15 fields. | Complete fixed header and a modest gap make this a bounded implementation. |
| 9 | RAW (Kyocera) — `KyoceraRaw.raw` | 17 | 0/17/0/1; 15.0% -> 15.0% | `KyoceraRaw`: 1/1; 11/11 fields. | Complete table plus existing raw infrastructure; verify routing before treating it as an engine enablement. |
| 10 | MOI — `MOI.moi` | 7 | 0/7/0/1; 30.0% -> 30.0% | `MOI`: 1/1; 7/7 fields. | Lowest observed gap among complete direct tables; cheap but lower user value. |
| 11 | MOBI — `Palm.mobi` | 21 | 0/21/0/1; 12.5% -> 12.5% | `Palm`: 2/3; 58/14 fields. | Some binary data exists, but most fields and one table remain untranscribed. |
| 12 | RM — `Real.rm` | 49 | 0/49/0/1; 5.8% -> 5.8% | `Real`: 0/11; 129/0 fields. | High value but a genuine hand parser: no usable BinaryData transcription. |
| 12 | RA — `Real.ra` | 8 | 0/8/0/1; 27.3% -> 27.3% | Same `Real`: 0/11; 129/0 evidence. | Share RM/RA framing and cost; do not count a second parser job. |
| 13 | R3D — `Red.r3d` | 34 | 0/34/0/1; 8.1% -> 8.1% | `Red`: 2/3; 51/8 fields. | Good observed value, but the large `Main` table is not transcribed. |
| 14 | TNEF — `TNEF.tnef` | 34 | 0/34/0/1; 8.1% -> 8.1% | `TNEF`: 0/3; 83/0 fields. | High gap with no table head start. |
| 15 | AA — `Audible.aa` | 29 | 0/29/0/1; 9.4% -> 9.4% | `Audible`: 0/5; 22/0 fields. | Moderate gap, no transcription; defer behind high-value pretranscribed work. |
| 16 | PSP — `PSP.psp` | 23 | 0/23/0/1; 11.5% -> 11.5% | `PSP`: 1/4; 21/8 fields. | Partial table head start, but three tables and most fields are absent. |
| 17 | Torrent — `Torrent.torrent` | 21 | 0/21/0/1; 12.5% -> 12.5% | `Torrent`: 0/4; 27/0 fields. | No BinaryData table path; bencode parser work. |
| 18 | XISF — `XISF.xisf` | 22 | 0/22/0/1; 12.0% -> 12.0% | `XISF`: 0/1; 37/0 fields. | One untranscribed table but XML/container work remains. |
| 19 | SWF — `Flash.swf` | 11 | 0/11/0/1; 21.4% -> 21.4% | `Flash`: 0/7; 62/0 fields. | Lower observed value and no table head start. |
| 20 | BTF — `BigTIFF.btf` | 10 | 0/10/0/1; 23.1% -> 23.1% | n/a — TIFF IFD parsing is not a `find_table` BinaryData layout. | Existing TIFF parser is relevant evidence, but BigTIFF routing/offset semantics must be verified before assigning a low cost. |
| 21 | PFB — `Font.pfb` | 16 | 0/16/0/1; 15.8% -> 15.8% | n/a — PostScript/font extraction is custom; `Font::PFM` is not evidence for PFB. | Do not falsely credit an unrelated transcribed font table. |
| 22 | DV — `DV.dv` | 15 | 0/15/0/1; 16.7% -> 16.7% | `DV`: 0/1; 13/0 fields. | Small but wholly untranscribed format handler. |
| 23 | ITC — `ITC.itc` | 10 | 0/10/0/1; 23.1% -> 23.1% | n/a — no BinaryData table in the pinned dump. | Lower value, custom plist-like/container work. |
| 24 | INDD — `InDesign.indd` | 9 | 0/9/0/1; 25.0% -> 25.0% | n/a — no BinaryData table in the pinned dump. | Lower observed tag return and custom container work. |
| 25 | MacOS — `MacOS.macos` | 8 | 0/8/0/1; 27.3% -> 27.3% | `MacOS`: 0/3; 144/0 fields. | Low carrier value and no layout coverage. |
| 26 | PPM — `PPM.ppm` | 6 | 0/6/0/1; 33.3% -> 33.3% | n/a — no BinaryData table in the pinned dump. | Smallest non-shared gap; custom text/raster parsing is not justified ahead of the rows above. |
| 27 | PICT — `PICT.pict` | 6 | 0/6/0/1; 33.3% -> 33.3% | `PICT`: 0/1; 145/0 fields. | Low observed value despite a large untranscribed source table. |

The source-module field totals intentionally expose short tables rather than
turning them into a promise.  In particular, MRC's generated `FEI12` has 96
of the pinned table's 98 fields, and JPEG 2000 has only 14 generated fields
across 5 of 7 tables (the source has 82).  Those absences mean
"not transcribed", never "tag does not exist".  This is the same rule behind
the known AIFF `Common` example: its `SampleRate` is an 80-bit `extended`
value omitted by the generator, even though ExifTool reads it.

## Plan-list correction: CRW, DICOM, and FIT are not IdentifiedOnly

The Step 33 plan named seven formats.  Current branch evidence does not put
all seven in this backlog.  The command above on `CanonRaw.crw`, `DICOM.dcm`,
and `Garmin.fit` returned normal parsed JSON (no `Status: IdentifiedOnly`) and
real parser tags; they must not be represented as zero-parser formats merely
because their conformance gaps remain.

| format/carrier | classifier R/M/V/E; score -> ceiling | BinaryData lookup evidence | Step 33 ranking decision |
| --- | --- | --- | --- |
| CRW — `CanonRaw.crw` | 0/150/0/1; 5.7% -> 5.7% | `CanonRaw`: 8/10 tables; 30/87 fields (its `Main` is not generated). | Parsed; exclude from this no-parser ranking. Its MISSING debt is separate parser conformance work. |
| DICOM — `DICOM.dcm` | 0/92/0/0; 8.9% -> 8.9% | `DICOM`: 0/1; 0/5,669 fields. | Parsed; exclude. The existing DICOM parser, not a missing dispatch, is the correct scope for its gap. |
| FIT — `Garmin.fit` | 0/67/0/0; 14.1% -> 14.1% | `Garmin`: 0/173; 0/1,898 fields. | Parsed; exclude. The existing FIT parser is the correct scope for its gap. |

## Renames are filler: result

The claim survives this measurement for the no-parser backlog: every one of
the 29 IdentifiedOnly carrier rows has `RENAME 0`, so its score-to-ceiling
spread is exactly 0 percentage points.  No format in this backlog can be
advanced by a rename-only change.  The full-corpus classifier result below is
the broader check; it is reported separately because global renames may occur
in formats that already have parsers.

## Unmeasurable formats

None of the 29 formats in the actual IdentifiedOnly carrier census is
unmeasurable: each has the named real carrier above.  This does **not** assert
that all filetype magic entries have a carrier.  A filetype with no supplied
carrier is not included or assigned an invented gap/cost number; it needs a
carrier before it can enter this ranked, measured backlog.

## Full-corpus conformance run

The required full-corpus command (the same pinned oracle, binary, and revision
identified above) was:

```sh
python3 tools/exiftool-tables/conformance.py /tmp/oxidex-exiftool-cache/combined-samples \
  --exiftool-dir /tmp/oxidex-exiftool-cache/exiftool \
  --oxidex /home/allen/git/s33-codex/target/debug/oxidex \
  --recursive --min-files 3875 --min-tags 5000
```

It scored **4,238 files** and reported **TOTAL 437,050 match / 21 RENAME /
1,671 VALUE / 11,610 MISSING / 10,602 EXTRA; score 97.0%, ceiling 97.1%,
precision 97.6%**.  These are measurements from that exact command, not a
baseline inherited from a previous revision.

The global score-to-ceiling gain is only 0.1 percentage points (21 renames),
while the corpus has 11,610 MISSING and 1,671 VALUE differences.  The
format-specific figures in the ranked table are also the corresponding rows
from this full run.  Thus the plan's "renames are filler" conclusion survives
both the backlog-only and full-corpus views: rename work is useful hygiene but
not the extraction opportunity.
