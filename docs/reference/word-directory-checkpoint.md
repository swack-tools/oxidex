# Shared word-directory checkpoint

Updated September 13, 2026. Work branch:
`codex/word-directory-verification-20260913`. Integration base: `72eae8a5`,
which follows merged directory-validation PR #753 (`1a47cfa3`).

## Goal and result so far

Generate tag-specific rules from the pinned ExifTool source and run them through
one shared reader. The source compiler recognizes the complete supported
processor body and its final numeric-reader binding; it does not select tables
by camera model, table name or tag number.

The full source capture identifies nine tables using the supported processor:

| Native table | Native rows |
| --- | ---: |
| FuncsUnknown | 0 |
| Functions10D | 17 |
| Functions1D | 22 |
| Functions20D | 18 |
| Functions30D | 19 |
| Functions350D | 9 |
| Functions400D | 11 |
| Functions5D | 21 |
| FunctionsD30 | 15 |
| **Total** | **132** |

The independent processor inventory contains 1,016 table identities and 16,430
row records, including 32 empty tables. Those are source facts, not a count of
supported or generated runtime tables.

## Verified, pending and remaining

| Measure | Result | Limit |
| --- | --- | --- |
| Word definitions | 9 native tables / 9 generated; 132 native rows / 132 generated; zero row or source-binding discrepancies | `verify.py` checks definitions and provenance; it does not prove processor execution. |
| Parent CIFF definitions | 61 native rows = 57 represented + 4 explicit omissions; zero native fact discrepancies | Parent still has one unsupported child processor and four omitted rows. |
| Child-processing refusals | Five before, one after generation | This does not enable a carrier or retire manual code. |
| Translated expressions | 607 passed; 16,789 matching probes; 14 inapplicable probe inputs | Expression validation is distinct from word-directory execution. |
| Rust reader and native replay | 24 keyed-reader tests pass; the explicitly selected native/Rust test passes all six cases using generated tables | Both byte orders, invalid headers, short reads, missing model state and parent dispatch are covered. Verbose-directory reporting remains a review finding under correction. |
| Rust lint | Exact CI Clippy command passed | Must remain green after the verbose-reporting correction. |
| Official two-tier regeneration | Partial; fourth attempt next | Earlier attempts stopped on a missing Rust import, an unreported processor counter, then a legacy Sony selector rejecting enriched processor facts. All three causes are corrected; a complete pass is still required. |
| Runtime migration and retirement | Not complete | No production Canon route is enabled; no duplicate Canon reader is removed. |

Regeneration uses isolated Perl 5.38.2, Archive::Zip 1.68 and repository-pinned
ExifTool 13.59. The changed outputs are the keyed definitions and their native
dump ledger; the other table artifacts are byte-identical after formatting.

The definition gate explicitly selects native
`Image::ExifTool::CanonCustom::ProcessCanonCustom` as its completeness scope.
This selector is verification input in regeneration, CI and `just verify-tables`;
it does not route production metadata. Expected tables come from fresh native
facts, including empty tables, rather than from whatever Rust was emitted.

Source identities and final package-local reader bindings are checked
independently. The native replay helper executes the actual selected processor
and records its return, warnings and handler arguments. Matching a source
digest alone is deliberately not treated as matching execution.

Evidence is under the task's `shared-pilot/word-directory-20260913/` directory:
`canonical-r1/`, `canonical-r2/`, `canonical-r3/`, `rust-native-r1/`,
`definition-verification.json`,
`definition-verification.log`, `clippy-initial.json` and publication records.
The owned checkout's local `HANDOFF.md` locates this task evidence.

Published checkpoints: definitions and documentation `ff1237ea`, runtime
validation `90034b1c`, shared-processor compatibility `88e07a47`, combined
branch `a4dd6af1`. These are pushed work-branch commits, not merged runtime
activation.

## Next measurable steps

1. Finish the official two-tier regeneration and publish its exact verdict.
2. Complete verbose-directory reporting and extend native/Rust replay to
   compare that effect. The quiet-mode replay and real generated parent-edge
   dispatch checks already pass. CI explicitly runs the native test and
   rejects a successful command that selects zero tests.
3. Implement the remaining dynamic-length processor and resolve the four
   omitted parent rows, recording each reduction in unsupported rules.
4. Verify both real Canon carriers, enable the validated path and remove the
   duplicate manual readers. Only then claim runtime migration or retirement.
