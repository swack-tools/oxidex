# Shared EXIF conversion reconciliation, 2026-09-11

## Scope and source

Control: `afd3a628cbcacb893c2b1d4dc54053eaf64565a4` on
`refactor/tag-machinery`. Implementation: `d2db5d27b54b46bff8de34d65b49efde610f97cd`.
The initial measurement used that implementation; the later repairs and final
acceptance are distinguished below. The preserved RawConv commits `ad9fdc32` and `0bf02cd9` were reconciled onto
that fresh control; the current Canon occurrence/raw-ID implementation was
retained. ExifTool remains pinned to **13.59**. No generated output or pin changed.

The six duplicated conversion paths in PNG, PSD, WebP, JXL, FLIF and HEIF now
use the common embedded EXIF reader. BPG and MIFF retain their existing shared
route and supply their container base explicitly. The old PNG EXIF helper
modules are removed; PNG text chunk keys consumed by the writer remain intact.
PDF and PSD share the thumbnail and ordinary IFD1 reader, preserving physical
occurrence order across both kinds of fields.
This removes duplicated runtime implementations; it does not activate the
generated Exif::Main IFD engine or establish all-container IFD1 coverage.

## Repairs required before retirement

Fresh native/control/initial-port probes found defects outside the real corpus:

- BYTE and SBYTE went through a printable-byte/length heuristic in the shared
  converter. They now use unsigned/signed numeric scalar and list reads.
- Plain UNDEFINED strings became binary. The repair reads existing generated
  Exif::Main declarations, accepting 35 simple string declarations without a
  Format override, reporting flags, unsupported conversion, condition or
  subdirectory. The four already implemented trim RawConv tags are the only
  exception to raw-conversion refusal. Writable limits this population; it is
  not used as a read-format override. Unknown/binary declarations remain binary.
- NUL and whitespace processing occurred in the wrong order. UNDEFINED strings
  retain their NULs during decoding. The known RawConv trims terminal ASCII
  whitespace bytes before UTF-8 repair; Unicode whitespace survives. JSON tests
  boolean/number typing before deleting NULs from the quoted-string fallback.
  The existing plain renderer and public compatibility helper keep their behavior.
- A skipped IFD0 entry shortened the decoded vector, moving the inferred next
  pointer backwards. The common helper now reads the physical entry count from
  the directory. PSD keeps valid IFD1 tags after type-zero and bad-offset entries;
  existing bounds and cycle checks remain in place.

Full WebP tests exercise Canon MakerNote parsing through LensID computation:
EF129/136 label collisions, real RF substitution, RF-zero fallback, and winning
and losing duplicate occurrences. They distinguish native full-file expectations
from internal occurrence-arbitration controls.

A stricter projection check then found retirement blockers that default corpus
scores could not expose:

- Rational JSON rendering quoted whole numbers. It now uses the existing native
  ten-significant-digit quotient renderer and JSON typing, including inf/undef.
- The separate thumbnail and ordinary IFD1 passes changed cross-name order.
  They now replay complete occurrences in physical directory order; dropped
  RawConv entries do not consume a later duplicate's position.
- Raw projection and Canon numeric/printed forms required additional repairs;
  newly exposed labels are not acceptable substitutes for numeric raw values.
  Existing decoder sites now attach 25 exact Canon enum values before occurrence
  arbitration. LensID retains the original EF value during RF display selection,
  and ShootingMode uses the forward generated display map. Default raw filtering
  retains the winning occurrence's value form. JSON, short, plain and CSV writers
  respect raw mode instead of applying display enums again.
- Embedded IFD0 dimensions need ExifTool's declared priority and physical-order
  full-resolution promotion so container dimensions and composites agree.
  The repair is scoped to one embedded IFD0. Cross-block priority state and native
  repeated-directory omission remain outside this claim; no state is inferred
  from previously printed metadata.

These repairs are implemented and independently reviewed. Final workspace and
fresh binary acceptance are pending; this report does not yet establish safe
retirement or a landed PR.

## Fresh validation

The explicit native oracle is ExifTool 13.59 under `/usr/bin/perl` 5.34.1,
including the DOCX capability probe. Both executables were freshly built from
clean source and copied to immutable evidence paths. Control SHA256:
`be2e9708e391d66f23e81be28a139f4df6dff5d8691e8905f4533250c19ba568`.
Candidate SHA256:
`91443833bb855a9e37a7cb79a049afeebd5271436872b0f92d05e8fbc945d3e0`.
These are correctness measurements of debug executables, not performance results.

| Instrument and population | Result |
| --- | --- |
| Original-port converter tests | Two intended failures; 60 existing converter tests passed |
| Strict `cargo clippy --all-features -- -D warnings` | Passed |
| `cargo test --all-features --lib` | 4,624 passed; one ignored |
| `cargo test --workspace --all-features --no-fail-fast` | 5,584 runtime tests passed, 50 ignored; 223 doctests passed, 63 ignored; 233.139 seconds |
| PNG writer and Step 20 real CLI controls | Executed and passed within the workspace suite, with pinned fixtures available |
| `conformance.py`, nine real container files | MATCH 431→434; MISSING 60→57; VALUE 1 unchanged; RENAME 5 unchanged; EXTRA 46→45 |
| `conformance.py`, complete combined corpus at d2db5d27 | 4,238 files; MATCH 450,021→450,036, MISSING 30,196→30,193, VALUE 524→512, RENAME 28 unchanged, EXTRA 1,565→1,564 |
| Strict CLI projection, 71 files × 12 modes at d2db5d27 | Blocked retirement: 50 formerly correct raw JSON facts lost; additional newly emitted raw values required repair |

The nine-file changes are exactly three PDF IFD1 resolution matches and removal
of PSD's spurious ExifOffset. The full control contains 4,238 files, 518,919 raw
oracle tags, 450,021 MATCH, 30,196 MISSING, 524 VALUE, 28 RENAME and 1,565 EXTRA;
it completed in 458.069 seconds. The full comparison uses the same bytes and
requires 4,238 files and at least 400,000 raw oracle tags. Capture wrappers call
the unchanged `conformance.py` extractor and matcher and preserve every parsed
input plus per-file results. The stronger retirement audit checks occurrence
multiplicity and refuses a compensating gain elsewhere as evidence of no loss.
The d2db5d27 default full-corpus capture completed in 491.298 seconds. No formerly
correct default-projection facts were lost. Fifty-seven already-wrong facts
changed only by the newly correct JSON deletion of NULs; each before/after value
and its source path were independently reviewed. The underlying ASCII reader's
retention of bytes after the first NUL remains separate debt.

Those default results did not certify raw mode. The 71-file matrix executes
852 commands per executable across JSON and targeted plain output, with default,
grouped, duplicate-preserving, printed and raw variants. It exposed the blockers
above. OxiDex raw mode uses `--no-print-conv`; its `-n` is dry-run. Earlier probes
using the wrong flag are preserved and explicitly invalidated. Native plain text
is compared as bytes, without assuming every value is UTF-8. The external
instruments have negative population, identity, typing, order and parsing controls.

## Remaining work and useful limits

The next runtime task is IFD1 prerequisite `7a69d2fa`, reconciled onto the fresh
integration while preserving the verified-key/input-domain CODE-reference gate,
then regenerated using a fresh pinned dump and its own verified ledger. Its old
committed table diff left `unwalked: None`; eligibility improvements existed only
in a scratch preview. Demonstrate eligibility separately from any later named-IFD
activation. IFD4/Olympus retirement and embedded-IFD0/PNG follow-ups remain separate.

The RawConv Option wrapper models the two existing Panasonic omission cases,
not arbitrary Perl RawConv. The PDF Photoshop boundary still flattens metadata
occurrences, so the WebP Canon test does not certify complete PDF raw identity.
Generated declarations and the 35-tag converter census do not prove that every
name is exposed by the CLI registry. Corpus VALUE/MISSING debt remains explicit.
The external occurrence and projection harnesses are acceptance artifacts;
promoting useful controls into maintained repository tooling remains separate work.

Evidence directory on the validation host:
`/Users/allen/Documents/Codex/2026-09-10/oxidex-worktree-cleanup-audit/handoff-continuation/rawconv-ro9m336v`.
It retains native fixtures, exact commands, hashes, failed original-port controls,
build/test results, corpus checkpoints and independent reviews. The continuation
reused `/Users/allen/git/oxidex-upgrade-triage`; no registered worktree or preserved
RawConv/IFD1 branch was removed.
