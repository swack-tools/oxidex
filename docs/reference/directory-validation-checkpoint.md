# Directory validation implementation checkpoint

Integration base: `634e5616` on `refactor/tag-machinery`, including the inactive
reader merged in PR #752. Work branch:
`codex/source-backed-directory-validation-20260913`. This source checkpoint is
locally validated; canonical generated artifacts and ledgers remain pending.
It does not activate Canon routing or remove manual code.

## What changed

`dump_tables.pl` records each loaded validation helper's body, source file,
hash and nested function dependencies after module loading. A bounded isolated
native probe captures the actual unsigned reader, unpacking implementation,
byte-order setters/getters and observed unpacking state. The loaded parent must
agree with that snapshot. Aliases, overrides, inconsistent state, incomplete
facts and timeouts cause refusal.

The compiler recognizes complete supported source bodies and takes offsets and
expected sizes from the native call. It does not dispatch by camera, tag or
validation-helper name. The independent oracle checks source facts, call
operands and numeric-reader provenance without importing compiler recognition.
Unknown behavior remains explicitly unwalked.

The shared reader applies validation to the complete bounded child value with
its declared size and inherited byte order. A rejected child preserves parent
state and processing of later entries. Native short reads at or before the
buffer end coerce to zero; starting beyond the buffer throws in Perl and must
reject the child. This distinction was found by native probing and corrected.

## Verified locally

| Instrument | Result | Limit |
| --- | --- | --- |
| Focused Python test modules listed below | 56 tests pass | Not the complete tool suite or merge gate. |
| Rust keyed-reader tests | 17 pass | The production Canon route remains inactive. |
| Rust size-check primitive tests | Three pass | Shared operation checks, not carrier coverage. |
| Exact repository CI lint command | Pass | Broader earlier integration-test lint had 23 existing warnings. |
| Actual ExifTool 13.59 dump, code generation and independent oracle replay, explicit Perl 5.34.1 | 61 source rows = 57 represented + four explicit omissions; zero missing, stale or mismatched native facts | Local source replay; canonical Perl 5.38.2 regeneration remains required. |
| Native unsigned-reader probe | 262,144 numeric cases across both byte orders and two offsets, plus ten boundary cases; zero failures | Complemented by complete supported-body recognition; not every possible caller or offset. |
| Copied native source: change only `Get16u` from unsigned 16-bit to unsigned 32-bit | All seven fresh checks retain the blocker; all seven stale checks fail independent verification | One real native mutation, plus focused synthetic source/state mutations. |

Local source replay now gives reader proof to all seven validation calls.
Three have no remaining child-edge blockers; four retain `target_processor`.
Including one other unsupported edge, the parent has five unwalked child edges,
down from eight. Undefined parent format and three opaque-value keys also
remain unresolved. These are compiler milestones, not live output gains.

```sh
PYTHONPATH=tools/exiftool-tables python3 -m unittest test_native_reader_contract test_directory_validation test_keyed_directory test_dump_validate_functions test_dump_binary_reader_contract
cargo test --lib --all-features exiftool_tables::keyed_engine::tests
cargo test --lib --all-features exiftool_tables::validation::tests
cargo clippy --all-features -- -D warnings
```

Evidence is under `shared-pilot/directory-validation-20260913/reader-contract/`
within the session evidence root; `final/native-replay.json` and
`final/core-width-mutation.json` retain the source replay and mutation results.
The system temporary directory's `oxidex-sony-plain-current.txt` points to that
root. The full native artifact also survives Rust formatting and independent
verification. Review corrected multiline digest parsing and requires all ten
distinct boundary records with their inputs and outcomes. Earlier report-format
failures and the corrected reader-test fixture failure are preserved; failed attempts are not counted as validation.

## What remains

1. Complete the full Python tool suite (running at publication) and add the
   inline eight-byte child validation regression identified by review.
2. Regenerate official artifacts and ledgers through
   `tools/exiftool-tables/regen.sh` with the repository pin and canonical Perl
   5.38.2 environment, holding the i7 heavy-job lock across every phase. The
   new source facts deliberately change the dump hash; an old ledger cannot
   authenticate them. The i7 accepts authentication but currently cannot open
   a command session. Its live lock, processes and native paths are unverified.
3. Resolve remaining parent/child processing rules and prove both CRW and JPEG
   carriers establish the native byte order immediately before reading. A
   consistent initial byte-order switch is not itself part of the fingerprint.
4. Run required current-head checks and carrier/corpus comparisons before
   production activation, then retire the replaced manual Make/Model code.

This checkpoint establishes no whole-project automation percentage.
