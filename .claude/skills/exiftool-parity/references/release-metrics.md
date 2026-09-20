# Release parity metrics and receipt contract

The receipt template is a summary index, not a replacement for instrument
artifacts or an attestation by itself. Fill it from retained
JSON, verified receipts and command logs; never manufacture counts or copy
console totals without their structured evidence. Run
`tools/ci/validate_release_receipt.py --kind parity` with the exact requested
version and candidate SHA before handoff; validation failure blocks release
use. Paths must remain available to the documentation/release reviewer;
include SHA-256 for every artifact.

## Four independent measurement families

| Family | Unit and denominator | What may be claimed |
| --- | --- | --- |
| Conformance | Public-output tag occurrences in selected files after the named instrument's exclusions and normalization | Agreement on that corpus/options, with MISSING/VALUE/RENAME/EXTRA and per-format/per-file detail |
| Authenticated reads | Exact Group1 identities per file, their value occurrences in both print/raw modes, and traced source coordinates | Observed matches and credited source rows under the authenticated receipt rules |
| Generated catalog | Declared/accepted/refused rows and joins in pinned source/catalog artifacts | Static knowledge and supported declarations, not observed parsing or writing |
| JPEG matrix | Manifest-selected writable JPEG tags actually attempted on the recorded fixture and options | JPEG read/write and round-trip outcomes within that manifest; no inference about other formats |

Never report one blended overall parity percentage. Distinct identities,
file-identity pairs, value occurrences, source coordinates and catalog rows
are different units. Give every percentage a numerator, denominator, scope
and instrument. Do not add independently measured roots together unless their
file populations and duplicate policy have been explicitly reconciled.

## Conformance formulas and identity

Use the actual `conformance.py` fields, preserving its comparator semantics:

- `matched` = paired occurrences agreeing under this instrument's matching
  and normalization rules, not necessarily byte-exact raw values.
- `missing` (MISSING) = native occurrences lacking an OxiDex counterpart.
- `value_diff` (VALUE) = paired names whose values disagree. Preserve the
  severity histogram (identity/structural/numeric/date_time/binary/display_only).
- `renames` (RENAME) = heuristic votes from `infer_renames`: a normalized value
  has a unique candidate in each direction among missing/extra occurrences,
  and either the normalized name matches or the value is sufficiently
  distinctive under `distinctive()`. This function does not consult a source
  table. Retain votes for review; require a pinned-table investigation before
  promoting a suggested rename to a confirmed mapping or implementing it.
- `extra` (EXTRA) = unmatched OxiDex-only occurrences, reported separately.
- Native scored denominator `N = matched + value_diff + missing + renames`.
  Score = `matched / N`; rename ceiling = `(matched + renames) / N`, a
  provisional estimate from heuristic votes, not confirmed or earned parity.
  EXTRA never enters this recall denominator.
- This instrument's precision = `matched / (matched + extra)`, not a generic
  all-output correctness rate. Its empty-denominator convention is 1.0;
  preserve that convention but label the empty population, not “100% parity”.
  An empty native denominator is unmeasured and cannot pass release floors.

Aggregate counts from `per_format` with the exact counted file population,
and retain `per_file`, rename votes, missing/extra identities and severity.
The oracle uses family `-G0:1:4` to preserve duplicate occurrences; the
instrument reports family-0/name identities and uses family-1/4 structure
while pairing. OxiDex names containing colons must remain intact. Repeated
keys are not deduplicated into a set: two missing occurrences count twice.
Cross-group matching is governed by the instrument's value/uniqueness rules,
not arbitrary bare-name equality. State these semantics alongside results.

Keep ignored filesystem/tool facts and detected-only identity tags visible in
the scope description. If the comparator includes FileType/FileTypeExtension/
MIMEType, do not silently subtract them from its published result; separately
report detected-only formats and payload extraction. Identity detection alone
does not establish that a format's metadata was parsed.

## Authenticated reads and source-coordinate credit

`corpus_read_receipt.py verify` replays exact parsed JSON value multisets with
numeric literal text preserved. It folds native CopyN and public duplicate
suffixes into Group1 identities; duplicate JSON keys are refused. An identity
matches only if print and raw modes both match. Print-only matches remain a
separate counter and cannot earn full credit.

Source coordinates are `(table, tag ID, variant index)`. A coordinate earns
credit only if every file where the oracle observes it matches the relevant
identity and occurrence attribution succeeds. Preserve unattributable,
withheld, missing, mismatched and failed-file-mode counts; generated rows
never fill these gaps. Distinct matched identities and credited coordinates
are not interchangeable percentages. Published read-gate losses must be zero
for a passing gate; retain the gate's actual verdict and snapshot identity.

## Catalog and write scope

Report accepted, refused, generated and joined row counts with their source
pin, generator/tool commit, source-coordinate model and denominators. Run the
catalog ratchet against its named floors and verify source snapshot currency.
A successful ratchet over stale JSON cannot support a current-release claim.
If regeneration is required, follow the existing producer and verifier; never
hand-edit generated declarations, snapshots or ratchet floors to create green.

For writes record manifest size, writable/attempted/successful/no-op/failed/
skipped populations and the exact success criterion from the matrix report.
Preserve read support separately. “ExifTool declares writable” is not proof
OxiDex wrote successfully; “command exited 0” is not a successful round trip.
Any option-gated behavior must name the option. Filtering or `--skip-write`
narrows evidence and cannot be described as complete write parity.

## Provenance, regressions and status

Each corpus entry records absolute roots, recursive selection, filters,
manifest path/hash, file count, justified file/native-tag floors, measured
native occurrences and the instrument's file exclusion rules. Each run
records argv, tool commit/hash, timestamp, environment, target directory,
logs/status-file paths and exit. Capture base/head SHA/tree, binary/build proof
and oracle hashes. Matching version text alone is not matching provenance.

Rebuild and freshly observe base/head on identical corpus/options/oracle.
Record exact new losses, changed values, gained matches and EXTRA changes per
file/identity, plus the separate published-read gate. A stale supplied
baseline, changed denominator, unproven binary or nonzero/partial run blocks
the relevant comparison. If a legitimate pin/corpus change is required,
report it as a measurement-scope change, not code improvement.

`verified` means the scoped evidence was produced and validated; it does not
mean perfect parity. Known MISSING/VALUE/EXTRA outcomes remain visible and
must be described truthfully. Required regressions or failed prerequisites
make overall status `blocked`. Missing required measurements are `unverified`.
All four families are required by default for a complete release receipt;
narrower requests may explicitly scope out a family with rationale, but a
consumer must accept that scope and may not make claims from the omitted data.
Do not silently promote a partial receipt to release-ready.

On failed canonical Perl probes, emit a blocked receipt with null metrics,
probe command/exit/stderr, and recovery prerequisite. Do not publish stale
numbers as current and do not fall back to another oracle. Restore the
canonical Perl installation and its matching standard library/modules, pass
the version/module/DOCX checks, then start a new evidence directory and fresh
measurements. Keep the refusal record for the handoff.
