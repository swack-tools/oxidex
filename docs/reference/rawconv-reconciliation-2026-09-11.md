# Shared EXIF conversion reconciliation, 2026-09-11

## Implementation and evidence boundary

This change reconciles preserved RawConv commits `ad9fdc32` and `0bf02cd9`
onto verified integration `afd3a628cbcacb893c2b1d4dc54053eaf64565a4`.
The final measured implementation is `0de2c15416188d773eb59aba4c84e9b1c38d54f5`;
subsequent report updates change documentation only. ExifTool stays pinned to
**13.59**. Generated tables, generator policy and activation gates are unchanged.
This is a runtime consolidation, not Exif::Main engine activation or an ExifTool
release upgrade.

PNG, PSD, WebP, JXL, FLIF and HEIF now use the shared embedded EXIF converter.
BPG and MIFF retain their shared route with an explicit container base. The two
private PNG conversion modules are removed. The public two-argument
`parse_embedded_exif` and `png::chunk_parser::parse_exif_chunk` helpers remain
available through shared implementations; containers needing a reported offset
base use `parse_embedded_exif_at`. PNG text keys used by the writer remain intact.

The consolidation required the following native-backed repairs:

- Read BYTE/SBYTE as numeric scalars or lists. Read the 35 generated, simple
  UNDEFINED string declarations as strings, retaining the existing refusal
  boundaries; this census does not establish 35 CLI-exposed names. Within that selected
  declaration population, count-one UNDEFINED follows the native byte rule;
  other unknown binary data stays binary.
- Apply the implemented RawConv whitespace rule before UTF-8 repair. Preserve
  NUL bytes during decoding; JSON tests boolean/number typing before deleting
  NULs from its quoted-string fallback. Unicode whitespace survives. The Option
  wrapper implements the two existing Panasonic omission cases, not arbitrary
  Perl RawConv.
- Use the physical IFD entry count for the next-directory pointer, even when an
  entry was skipped. Replay ordinary and thumbnail IFD1 occurrences in physical
  order, including omitted duplicate controls. PDF's Photoshop resource path
  preserves each selected winner's order, value form and priority; its existing
  one-value-per-key projection remains until cross-resource directory tracking
  can safely support additional duplicates.
- Preserve native numeric forms at 25 existing Canon enum decoder sites and
  retain winning/losing occurrence identity. LensID keeps its original EF value
  during RF display selection; ShootingMode uses the forward generated map and
  its native unknown-code form. No inverse display-label lookup is introduced.
- Preserve raw forms through default filtering and JSON/plain/short/CSV output.
  Apply APEX ValueConv consistently and format typed raw floats with the existing
  native numeric formatter. Rational JSON retains numeric typing and native
  quotient precision. Arbitrary numeric strings are not reinterpreted.
- Honor embedded IFD0 dimension priority and physical-order full-resolution
  promotion within one directory. Correct VP8X's three-byte canvas height read.
  Keep each QuickTime mdat size as its own occurrence; only AvgBitrate sums those
  sizes, with checked arithmetic.

## Final acceptance

The native oracle is the pinned 13.59 source under `/usr/bin/perl` 5.34.1,
with a successful DOCX capability probe. Both debug executables were freshly
built from clean commits and copied to immutable paths. These are correctness
measurements, not performance comparisons.

- Control binary SHA256: `be2e9708e391d66f23e81be28a139f4df6dff5d8691e8905f4533250c19ba568`.
- Candidate binary SHA256: `f2090fdc188af0b292e6bff52524e0b4285a6304dd10cc07ed9776983b024175`.

| Instrument and population | Result |
| --- | --- |
| `cargo clippy --all-features -- -D warnings` | Passed; final production patch was checked before commit |
| `cargo fmt --all -- --check`, CI corpus guards and tag-stat synchronization | Passed on final implementation |
| `cargo test --workspace --all-features --no-fail-fast -- --test-threads=4` | 5,605 runtime tests passed, 50 ignored; 223 doctests passed, 63 ignored; 173.683 seconds, source stable |
| `conformance.py`, all 4,238 combined samples | MATCH 450,021→450,036; MISSING 30,196→30,193; VALUE 524→512; RENAME 28 unchanged; EXTRA 1,565→1,564; candidate 395.142 seconds |
| Complete per-file occurrence comparison | Zero previously correct facts lost, zero new missing facts; 57 changed previously wrong strings individually classified below |
| Strict CLI projection, 71 files × 12 modes | 852 commands per role, zero execution/parse errors, zero lost correct facts, +2,963 matches; reviewed 39 VALUE and 216 EXTRA warnings |
| PNG dimension-priority carriers, 12 files × 12 modes | 144 commands per role; zero lost correct facts/new missing/new VALUE; 24 EXTRA warnings classified as correct values in existing slots or changed pairing |
| Cross-name IFD1 checks | PSD's six-field sequence matches native printed/raw output; PDF's shared six-field sequence matches with `-a`, with its late legacy Compression occurrence and default winner remaining explicit exceptions |
| Final scoped PDF CLI replay | 12/12 bounded contracts passed; ordinary/shuffled IFD1 layouts and repeated-resource containment; standing native-absent winner remains |
| Additional native controls on unchanged implementation | Canon 48/48 case/modes; APEX/typed-float 30/30 case/modes (90 field observations); HEIC/MOV 6/6 projections; source and evidence identities retained |

The final full-corpus fact comparison is identical to the independently reviewed
c18d result. The final main and PNG projection facts are also identical to those
reviewed before the bounded PDF change. Fresh IFD1/PDF checks establish its order
and selection behavior separately. Final reports and plans add no runtime change.

The full census includes all 4,238 files (132,547,495 bytes), with no exclusion
or symlink deduplication. It requires at least 4,238 files and 400,000 native
raw keys; the control has 518,919. Captures call the unchanged conformance
extractors and matcher and retain per-file inputs, values, identities and
checkpoints. The stronger occurrence comparison refuses compensating gains as
proof that a previously correct fact survived. The existing FujiFilmISPro
native return-code-1 case is retained on both sides, not counted as a new
execution failure or silently removed from the population.

The projection instrument executes 71 files in 12 modes: 852 commands per
executable across JSON and targeted plain output, with default, grouped,
duplicate-preserving, printed and raw variants. OxiDex raw mode is
`--no-print-conv`; its `-n` means dry-run. Native plain text is compared as
bytes. Negative controls cover empty populations, artifact identity, command
coverage, scalar types, ordering and parse failures. Per-name projection order
and cross-name physical IFD1 order are separate checks.

The strict instruments retain `REVIEW_REQUIRED`; their warnings have explicit
independent dispositions. No warning is silently reclassified as complete
ExifTool parity. The full census's 50 changed VALUE and seven changed EXTRA
facts are exactly JSON NUL deletion on previously wrong strings. All 57 source
values, identities and before/after projections were independently checked;
underlying ASCII termination debt remains.

The main projection's 39 VALUE warnings consist of 12 correct raw forms exposed
through existing multi-group/winner behavior, 17 numeric-string values already
present in the original control's grouped raw output, and ten Latin1 plain-text
cases already wrong before the change. Those 17 include 11 precision cases,
two existing Canon arithmetic/dependency last-digit cases and four existing
Pentax/APP12 reciprocal-input cases. These are separately scoped output debts.

Its 216 EXTRA warnings include 184 correct VP8X height rows: 174 grouped rows
with WebP versus native RIFF identity, plus ten default duplicate-height slots.
The remainder are 19 corrected raw forms on prior extras, six unchanged values
whose pairing changed, six comparison rows for three newly covered correct PDF
IFD1 fields appearing as additional default-projection duplicates in printed/raw
modes, and one existing Canon FNumber precision/group-form case. The separate 24 PNG warnings retain the same
two occurrence slots: 18 correctly decoded EXIF byte values and six unchanged
container values with different pairing. Exact row-level ledgers remain with
the evidence; these counts do not mean 216 newly invented parser values.

## What happened when

Times are America/Chicago on September 11. Commit times locate milestones;
measured command durations come from their own logs and do not measure authoring
hours. Initial and rejected candidates are retained as diagnostic evidence.

| Time | Milestone and interpretation |
| --- | --- |
| 11:58 | Started from freshly verified integration `afd3a628`, reusing the owned checkout. |
| 12:24 | Initial port `d2db5d27`: workspace passed and the full default corpus improved, but the stricter projection lost 50 previously correct raw JSON facts. This was not retirement acceptance. |
| 12:45–13:06 | Physical IFD1 order, Rational JSON, dimension priority and Canon raw-form repairs. Candidate `4cf55d71` still lost 55 correct projection facts, primarily exposing the VP8X height bug. |
| 13:24 | Candidate `c18d153d` repaired VP8X, HEIC and APEX/raw formatting. Its full 4,238-file census preserved every previously correct fact; projection residuals were individually reviewed. |
| 13:29–13:36 | Full workspace found two old assertions expecting PrintConv during raw output. Pinned ExifTool confirmed both new raw values; tests now assert printed and raw forms separately. All 24 writer tests passed. |
| 13:43 | Candidate `d666744f`: PDF occurrence boundaries and original public helper APIs preserved; 5,604 runtime tests and 223 doctests passed. A repeated-resource native probe still exposed one additional wrong duplicate. |
| 13:48 | Candidate `0de2c154`: retained PDF resource winner selection while preserving selected forms, priorities and physical order. Cross-resource directory tracking stays explicitly deferred. |

The d2db full-corpus result alone was insufficient: it improved MATCH by 15
while concealing raw-mode regressions. Its 233.139-second workspace result and
the later c18d 441.104-second census remain historical measurements of those
commits. The final gates above establish the final implementation separately.
An exploratory `clippy --tests` invocation also encountered existing PE fixture
identity-operation warnings; the required strict Clippy command is reported
separately, without changing those unrelated fixtures or weakening CI.

## Work still useful

1. Reconcile IFD1 prerequisite `7a69d2fa` onto the newly verified integration.
   Preserve the named verified-key/input-domain CODE-reference gate, then run a
   fresh pinned dump, its own hash-matching verification ledger and regeneration.
   The old committed table diff only added `unwalked: None`; improved eligibility
   existed in a scratch preview. The old whole-dump eligibility assertion needs
   the matching ledger; unwalked-edge accounting and the shared converter's
   35-declaration population also need rechecking after regeneration.
   Reproduce eligibility before considering any
   separate named-IFD activation. IFD4/Olympus and broader embedded-directory
   state remain separate tasks.
2. Repair classifier/producer accounting from the release rehearsal: recognize
   the existing Garmin parser, separate standing debt from release changes and
   group repeated field rows by cause. Keep emitted, eligible, activated and
   observed coverage distinct.
3. Address the explicitly measured output debts below using fresh native
   carriers and preserved occurrences. Do not approximate conversions merely
   to make a comparison score rise.
4. Promote useful external projection/occurrence controls into maintained
   repository tooling. The acceptance harnesses here are durable artifacts, not
   newly installed CI gates. Four missing Sony/Nikon producers and catalog
   carry-forward policy remain in the broader automation backlog.

Repeated PDF EXIFInfo resources can alias earlier IFD or sub-IFD addresses.
Native ExifTool tracks all visited directory addresses; skipping merely the first
resource or repeated IFD0 offsets would be incorrect. The adapter retains its
existing one-winner-per-key output boundary while preserving the winner's forms,
priority and order. The repeated-offset control still retains one Canon LensType
winner (raw 136) that native omits. Containment removes the newly exposed second
occurrence, not that standing wrong winner. Full cross-resource duplicate
preservation remains deferred.
The legacy PDF rescanner still lacks source identity and can add a late
Compression occurrence after the shared six-field IFD1 sequence. Complete PDF
raw/printed identity is not established. Cross-block full-resolution priority
and native repeated-directory omission are outside the single-IFD0 proof.
SEMInfo/DNGPrivateData registry exposure, source strings after their first NUL,
Latin1 plain-text byte output, numeric-string precision and default group/winner
semantics remain bounded, explicit debt. Existing corpus VALUE/MISSING counts
are not zero.

Evidence directory on the validation host:
`/Users/allen/Documents/Codex/2026-09-10/oxidex-worktree-cleanup-audit/handoff-continuation/rawconv-ro9m336v`.
It retains source/binary hashes, native fixtures, exact commands, original failed
controls, immutable captures, reviews and the PR/merge record. The continuation
reuses `/Users/allen/git/oxidex-upgrade-triage`; all 13 registered worktrees and
the preserved source branches remain. The protected checkout is unchanged.
