# Shared serial runtime checkpoint

This checkpoint introduced one shared reader for native serial layouts, with
no production caller at that stage. Its first production caller subsequently
merged in #758, replacing Real AudioV4's manual field sequence after carrier,
source-change and full output checks passed.

## Source and publication

The base is merged PR #756 (`eb700430`), pinned to ExifTool 13.59. Runtime,
emitter and independent verifier are integrated at `9693ef9e` on
`codex/shared-serial-integration-20260913`. The earlier compiled checkpoint
`ff383b8e` is published. Its Rust source is unchanged by the later verifier and
pipeline additions. Final head `72fbebb1` passed all five hosted checks and
merged as `58849bc7` in PR #757 at 12:47 UTC on September 13. Canonical Python
passed 757 tests with zero failures/skips in 641.913 seconds; nextest passed
5,756 tests with 59 intentionally skipped. Full Cargo test/doc and explicit
serial native replay also passed.

Native inventory selects all tables whose processor is `ProcessSerialData`
before examining generated output. Eight tables contain 130 entries and 132
alternatives. The source descriptor has 115 clear alternatives and 17 named
refusals. Rust emission adds unsupported-format and missing-member-condition
refusals, producing 106 alternatives and 26 row omission records. No table or
alternative disappears from the inventory.

| Table | Emitted alternatives | Definition gate |
| --- | ---: | --- |
| Canon AFInfo | 9 | Blocked |
| Canon AFInfo2 | 6 | Blocked |
| Real AudioV3 | 12 | Clear |
| Real AudioV4 | 31 | Clear |
| Real AudioV5 | 18 | Clear |
| Real ContentDescr | 8 | Clear |
| Real MediaProps | 16 | Blocked, including native priority policy |
| Real Properties | 6 | Blocked |

A clear definition gate does not enable a parser or prove complete execution.
Native/Rust execution currently covers AudioV3 and AudioV4. Caller enablement
is false by default.

## Independent checks

`verify_serial_directory.py` reads the emitted Rust independently of the
compiler and compares it with a fresh captured native population. It checks
source bindings, group defaults, fields, counts, conditions, flags, gate
reasons and omission records. The recorded result is eight tables / 132
alternatives / 106 emitted / 26 omitted, with zero mismatches.

Review found and closed two false-success cases: claiming every supported
table was descriptor-refused, and appending an omission for a nonexistent
table. Both now fail. Removed rows, changed counts/groups/source hashes,
stale native source and ambiguous single-alternative variant identity also
fail. Nested module group defaults are checked against actual native
`GetTagTable` behavior. This verifier proves source facts; it does not replace
runtime comparison.

The Rust reader's 13 unit tests pass. The explicit
`real_audio_serial_tables_replay_pinned_native_callbacks` test runs the actual
generated AudioV3/V4 tables against `probe_serial_processor.pl`: ten cases
cover both byte orders, Unknown policy, zero/NUL/truncated strings and a
nonzero bounded directory offset. It passes on both threaded and non-threaded
Perl 5.38.2. It compares callback scalar/string values and native source group
facts, not final metadata grouping or production activation. CI invokes this
ignored test explicitly and requires one passed test, preventing an empty
selector from passing.

The combined focused Python suite passes all 64 tests, zero failures and zero
skips, in 65.023 seconds. The full `cargo clippy --all-features -- -D warnings`
check passes in 10.125 seconds, and `cargo fmt --all -- --check` passes. These
local checks precede the required full hosted acceptance run.

The first combined compilation found an invalid byte-order enum spelling in
a test; review also caught a fixture using ASCII 52 instead of length 4. Both
were corrected before the passing run. These were test setup errors, not
claimed output improvements.

## Reproduction and build cost

`regen-all.sh` now owns 32 declared artifacts: 10 primary outputs and 22
secondary outputs. `serial_tables.rs` is generated from the same fresh dump,
formatted, and checked by the independent verifier. The manifest, write guard,
`just verify-tables`, CI and failure-propagation tests include it.

The official two-tier run at `9693ef9e` passes in 190.956 seconds. All generated
Rust remains unchanged. Only the expression ledger's invocation paths change
when absolute local tool locations are supplied. Re-running that producer with
the portable library path and interpreter command takes 19.028 seconds and
reproduces the committed ledger exactly. No generated artifact was edited by
hand. The initial 22.583-second attempt stopped on the untracked handoff's
dirty-tree guard; the successful run explicitly records the permitted override.
All 607 expressions pass 16,789 applicable native comparisons; 14 probe inputs
are inapplicable.

The first corrected reader build/test takes 74.437 seconds; the native test
build and run takes 48.276 seconds. A second native replay reuses the exact
compiled binary and finishes in 0.837 seconds. Terra workers perform source
work in separate checkouts, while the coordinator owns the shared build cache.
No i7 job or daemon change is part of this checkpoint.

## What remains

- AudioV4 retirement is merged after bounded and full corpus checks, hosted
  acceptance and supported source-change execution proof, as described in the
  [V4 retirement record](real-audio-v4-retirement.md). Continue the
  [Canon autofocus definition plan](serial-afinfo-plan.md).
- Preserve the carrier's known output-model gaps: native group 0/group 2 are
  not stored by the current occurrence API; header-only native warning output
  is absent; AudioV3/V5 are not activated. Do not call these complete parity.
- Prove or explicitly refuse zero-count numeric prior reuse, verbose output
  and other unexercised conversion/binary behavior before broader enablement.
- Resolve the remaining Canon child processor and four parent omissions,
  prove both Canon carriers, then retire their duplicate readers.

The runtime checkpoint itself removed no manual reader. Its subsequent V4
retirement is recorded separately; neither establishes a new project
autogenerated percentage.
