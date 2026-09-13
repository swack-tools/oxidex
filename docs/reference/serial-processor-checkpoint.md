# Native serial-processor checkpoint

Updated September 13, 2026. Implementation checkpoint `f313e38a` is published
on `codex/serial-native-probe-20260913`. The same two source files were
integrated onto merged #754 (`1138a880`) at `c5bc4c9f` and independently
revalidated. The integration branch is `codex/serial-native-integration-20260913`.

## Purpose and current result

The remaining CanonRaw child processor reads sequential fields whose lengths
can depend on earlier raw values. Before implementing that shared capability,
we need an independent record of what the actual native processor reads and
reports. `probe_serial_processor.pl` executes the selected table's native
`PROCESS_PROC`; it contains no Canon tag IDs or table-specific dispatch.

This checkpoint adds a validation instrument and its tests. It does not
generate a serial descriptor, change a Rust reader, enable a production route,
or retire manual tag rules. **One unsupported child processor and four omitted
parent rows remain.**

## What the instrument observes

Requests use JSONL protocol `oxidex.serial_processor.v1` and select a module,
table, bytes, bounded directory span, byte order, saved members and options.
Each request receives a fresh native object. Results record:

- The selected processor and its package-local numeric-reader binding, with
  relative source identity, source hash and deparsed body hash.
- Actual scalar `ReadValue` arguments/results and native tag selection.
- Raw `FoundTag` observations, return value, warnings and verbose callbacks.
- Byte-order and temporary unknown-tag state restoration.

The helper wraps the actual reader binding and proves its direct scalar call
position using Perl's operation tree. A quoted string, object method call,
foreign binding, coderef operand or nested-call shape cannot stand in for that
call. The operation-tree contract is scoped to canonical Perl 5.38.2.

`FoundTag` is an observer returning success; these records do not establish
final metadata keys, conversions or full output behavior. Nested directory
execution and dynamically evaluated calls need separate support before the
instrument is used to claim coverage of those behaviors.

## Validation

Independent review accepted `f313e38a` after reader-binding and call-position
corrections. Canonical Perl syntax checking and **eight native tests pass** on
the byte-identical integration files at `c5bc4c9f`. No test was skipped. Inputs
use pinned ExifTool 13.59 and Perl 5.38.2.

Fixtures cover both byte orders, missing and differing model state, zero and
large counts, truncation, unknown-tag options and restoration. Copied native
source mutations verify that rebinding the reader changes observed values,
while unsupported callback bypasses and deceptive call shapes refuse.
This is a finite validation scope, not equivalence for arbitrary native Perl.

The first hosted run at `7c9dce65` failed: 706 Python tests ran, with ten
failing subcases in these eight new tests. The local Perl build was
non-threaded; the threaded build represents the same direct scalar call with
different operation nodes. An isolated threaded Perl 5.38.2 reproduced all ten
failures on the unchanged checkpoint. The repair, reviewed at `9ff5c749` and
integrated at `f214accd`, resolves the callee through the selected processor's
own pad and checks the exact scalar assignment structure. All eight tests now
pass on both Perl builds, including the misleading-call mutation controls.
The corrected hosted run remains required before merge. The earlier failure
and both build configurations are retained in the evidence.

Run the focused checks from the repository root with explicit selected inputs:

```bash
OXIDEX_PINNED_EXIFTOOL=../exiftool-13.59 \
EXIFTOOL_PERL=perl5.38.2 \
  python3 -m unittest discover -s tools/exiftool-tables \
    -p 'test_probe_serial_processor.py' -v
```

Use the actual pinned source location and canonical Perl executable. Explicit
invalid inputs fail; omitted optional inputs skip these tests and do not count
as validation. Hosted CI already supplies both variables to Python discovery.
The preceding merged batch passed 698 tests; that count excludes these eight.

Evidence is in the task's `shared-pilot/serial-native-20260913/validation.json`,
`perl-syntax.log` and `canonical-native-tests.log`. Publication proof is in
`shared-pilot/word-directory-20260913/serial-probe-publication.json`. The owned
checkout's untracked `HANDOFF.md` locates the task evidence.
The platform correction adds `hosted-native-failure.log`,
`threaded-before-fix.json` and `dual-perl-validation.json` under the serial
evidence directory.

## Next measurable results

1. Derive sequential layouts, prior-value count expressions, conditions and
   conversions from the native source. Unsupported semantics remain explicit.
   Do not hard-code the AFInfo layout into a generator.
2. Implement the corresponding shared reader and compare its raw effects with
   this native instrument. Preserve selection-before-read, raw saved values,
   bounded reads, reporting order and state restoration. Count the child
   processor as resolved only when its complete required behavior is verified.
3. Resolve the four separate parent byte-handling omissions, then verify real
   CRW and JPEG carriers and retire the duplicate manual readers. Native JPEG
   AFInfo setup must not be borrowed silently into the CRW carrier.

The progress measure is fewer unsupported source rules followed by verified
runtime migration and manual-rule removal. The amount of probe code is not an
autogeneration percentage.
