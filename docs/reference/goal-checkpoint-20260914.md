# Generated metadata parity checkpoint

Status: historical checkpoint of 2026-09-14. Work consolidated in PR #779 and resumed on 2026-09-15 ([resume instructions](metadata-parity-resume.md)); the first resumed milestone is the [generated Garmin FIT reader](garmin-fit-source-review.md). PRs #682 and #683 remain separate. The full source-derived read/write parity objective remains incomplete.

## What is now accounted for

The regenerated BuildTagLookup join classifies all 33,487 catalog entries against 35,886 hydrated source coordinates in 1,512 tables. The 21,373 distinct case-insensitive names appearing in those entries and the native legacy total of 21,437 have different definitions. Neither measures reading or writing success.

The current source ledger distinguishes:

| Classification | Catalog entries |
| --- | ---: |
| Generic QuickTime reader declarations | 179 |
| Explicit QuickTime reader refusals | 151 |
| Eligible IFD schema declarations | 3,815 |
| IFD schema rows with withheld semantics | 1,217 |
| Refused IFD schema rows | 9,563 |
| Not yet indexed by these compiler joins | 18,562 |

These rows conserve the catalog denominator. The writer axis separately contains 19 authenticated generated public-writer declarations. A row may have both reader and writer facts; those axes must not be added together.

QuickTime declarations comprise 92 ItemList, 70 Keys and 17 UserData rows. The three source tables contain 399 variants, including rows outside the public catalog. UserData now has a generic movie-level direct-format reader; language records, implicit formats and custom controls retain explicit refusal reasons.

The IFD compiler ledger covers 17,851 source rows: 6,117 emitted schema rows (4,313 eligible and 1,804 with omissions), plus 11,734 refused rows. Only catalog-matched rows enter the table above. Schema eligibility alone proves neither dispatch reachability nor observed output.

The machine-readable source ledger and per-table report are published at `docs/public/measurements/catalog-hydrated-join-13.59.json` and `docs/reference/catalog-hydrated-join.md`. Rows not indexed here may already have readers elsewhere; this is not a claim that they are unreadable.

## What has actually been measured

These are distinct immutable checkpoints, not a combined current coverage percentage:

- At `0c733800`, M4 native/public reader comparisons observed six ItemList and three Keys Group1 names. The generated TIFF/JPEG transaction matrix passed 1,530 cases. Authenticated native readback credits 1,353 positive write operations across 38 Group1 names: 906 operations match 19 catalog entries; 447 exercise alternate directory contexts. Deletions and no-ops are excluded from positive write credit.
- At `5862e046`, the authenticated UserData verifier passed all 126 mode comparisons across 63 fixtures, exercising 17 source identities and 15 Group1 names. It binds the real Cargo build, generated artifacts, native source, fixture bytes and full output transcripts.
- Canonical regeneration at `6f34cff4` passed both selected tiers and the declared write-set check using Perl 5.38.2 and ExifTool 13.59. All-feature Clippy passed. All-feature Rust reported 6,093 passed, zero failed and 126 ignored, aggregated from completed test-result lines.
- At `be0ffe0c`, the corrected Python gate completed 425 tests in 400.693 seconds, exit zero, with six skipped. The persisted controller verified unchanged HEAD and tracked files before and after the run. This clears the stale regeneration mock and capture failures; it does not validate subsequent writer changes.

The source-only snapshot deliberately has no observed credits. A separate [authenticated historical snapshot](catalog-hydrated-observed.md) is now published for the common runtime `be0ffe0c`. It contains observed reads for 26 catalog entries and observed writes for 19 catalog entries. All 1,530 native/public TIFF/JPEG cases matched; 1,353 positive write/readback operations span 38 Group1 names. UserData passed 126 comparisons across 63 fixtures. Those different denominators must not be conflated.

Snapshot import replayed the live receipt validators with the original clean runtime preserved. The reviewed publisher repair aligned non-ASCII receipt hashing with the join producer. Current-source applicability is reported separately from historical runtime evidence; the snapshot does not validate later writer changes.

The completed local checkpoint at `d0c1b6ae` passed formatting, workspace/all-feature Clippy, 6,121 Rust tests (zero failed, 126 ignored), and 118 focused Python tests (one skipped). This includes the actual current/retired reverse-name regression. Final generator repairs authenticate byte packing maps and keep the public two-format scalar ABI separate from seven private cleanup formats. Both repairs passed independent review; official regeneration at `980d5efe` passed both tiers with zero net generated changes.

Linux rehearsal teardown passed 41 executor/adapter tests, zero skips, at `be0ffe0c`. Recorded descendants were absent after teardown and the shared lock was released. The relevant source files are unchanged in the later integration. Exact historical mandatory-default source profiles for ExifTool 11.78 and 12.64 are integrated and independently reviewed. The fresh materialized rehearsal at `980d5efe` passed its 11.78 native probe (denominator two), then explicitly refused unsupported historical `SetNewValue` caller control flow during generation. Read/write stages and 12.64 did not run; promotion is forbidden. This is preserved remaining capability work, not a successful upgrade claim.

The writer checkpoint at `b4ca393d` executed locally on the Apple M5. It passed all 1,530 public TIFF/JPEG matrix cases (1,524 mutating and six native-idempotent cases). Authenticated native readback credited 1,353 positive write operations across 38 distinct Group1 names. This result is separate from the historical `be0ffe0c` published snapshot. The originally proposed remote M4 run did not occur: SSH was unavailable, and the first local run was mislabeled and aborted; its partial artifacts are preserved without credit.

The subsequent 48-carrier comparison found a JPEG mandatory-only IFD1 cleanup mismatch. The repair in `31e87191` applies the existing source-derived physical survivor matcher to JPEG, including JFIF default substitutions. Its focused regression, 20 JPEG module tests and Clippy pass; independent review found no actionable defect. The fresh `fixedwidth_ifd1_public_write_v1` comparison at clean commit `7b1cf056` passed all 48 native/OxiDex cases using Perl 5.38.2 and pinned ExifTool 13.59. The full public matrix at the same clean runtime also matched all 1,530 cases (1,524 mutating, six native-idempotent). Authenticated readback and final integrated gate receipts are recorded on PR #779; the published observed catalog snapshot remains bound to its original runtime.

The selected-release matrix adapter now derives its cohort from authenticated generated inputs rather than obsolete fixed totals. It accepted the actual 1,530-row selected report, including all 876 baseline cases, with zero mismatches. Its 45 adapter/executor tests passed. Historical 11.78 generation still refuses independently of this report-contract repair.

## Work remaining and how progress is measured

1. Before resuming, verify PR #779 is merged and inspect its final integrated gate and review receipts. The CI source-bound ledger replay and numeric composition harness repairs are included; do not infer their outcome from an older green commit.
2. On resumption, use the per-table omission ledger to select the next shared protocol or conversion capability. Each milestone must reduce an identified refusal block, regenerate affected rows and verify the relevant on-disk behaviors against the pin.
3. Grow observed read/write evidence separately from implementation declarations. Record catalog coordinates, Group1 names, fixture occurrences and positive write operations independently.
4. Continue toward full catalog-wide parity when the maintainer resumes the work. The 179 reader declarations, 19 writer declarations and IFD schema counts are intermediate evidence, not completion or an autogenerated-output percentage.

The original five-fixture pre-migration result remains in `quicktime-reading-baseline.json`. The migration objective is in [the source-family plan](source-family-migration-plan.md); review evidence is in [the consolidated review record](parity-rollup-review-20260914.md).
