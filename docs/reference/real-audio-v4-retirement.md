# Real AudioV4 manual-sequence retirement

The shared serial capability merged in #757 at `58849bc7`. This next change
uses it in the Real AudioV4 carrier and removes the manually copied 31-slot
field sequence. It adds no new tag-specific generator or interpreter.

## What changes

`real_audio.rs` retains signature/version selection and the native 512-byte
maximum body read. Generated `Real::AudioV4` supplies names, formats, field
order, dynamic lengths, visibility and groups to the common serial reader.
The carrier projects emitted rows through the existing metadata insertion API.

Fresh native comparisons also identify corrections to the old parser:

- Invalid UTF-8 uses native question-mark repair. This fixes Copyright on the
  real corpus sample, not only synthetic input.
- Strings stop at NUL while consuming their full declared byte span.
- An incomplete Artist span stops the serial reader. The old cursor stayed in
  place after the failed read and could invent Copyright and Comment from the
  same bytes. Cases with 511, 512 and 600 body bytes prove the correction.

AudioV3/V5 activation, native warnings and complete occurrence groups remain
outside this migration. The current API derives stored group 0 from the visible
Real-RA4 key and cannot store group 2; this pre-existing limitation remains
explicit. The descriptor retains the native group facts. No new project
percentage or Canon retirement is claimed.

## Evidence before the full acceptance gate

Control is `58849bc7`; candidate runtime is `4f01db97`. `d3f51325` adds only
regression assertions. The six carrier unit tests, full all-feature Clippy and
formatting pass. The source builds reuse one coordinator-owned local cache:
control 1.486 seconds, candidate 7.443 seconds; the final six-test build/run is
61.502 seconds. Workers perform source/evidence work without Cargo.

The native carrier contract covers 19 fixtures against pinned ExifTool 13.59
and Perl 5.38.2. All 15 scored candidate projections match native Real-* output.
Ten fixtures improve legacy output; five are unchanged. Four pre-existing
scope/diagnostic cases remain recorded. Control/candidate exit codes agree.
Original failed assumptions that five control projections were already correct
are preserved alongside raw runs and the independently corrected contract.
These counts are bounded fixture evidence, not a full-corpus result.

A separate copied-source proof changes exactly one supported native property:
`Real::AudioV4[24].Name` from `Title` to `UpgradeTitle`. Fresh dump, generation,
independent verification and formatting change one generated Rust name. A
source archive compiled with that artifact and the unchanged carrier changes
exactly `Real-RA4:Title` to `Real-RA4:UpgradeTitle` on the real sample, retaining
its value and matching the modified native source. Compilation takes 15.461
seconds. Tag-specific Python/Rust edits: zero. This proves one supported source
change, not compatibility with an entire new release.

## Acceptance and retirement accounting

Before landing, require the complete paired 4,238-file census and final hosted
checks. The paired runner reuses `conformance.py` scoring, authenticates both
binary/build manifests and unchanged scoring/oracle helpers, probes DOCX oracle
capability, and preserves file hashes, raw outputs, exits and per-file results.
It caps concurrency at two and persists progress; interrupted or vacuous runs
cannot be reported as a pass. Record any pre-existing diagnostic exits apart
from new failures. Exact per-file changes, rather than equal totals alone,
determine acceptance.

After acceptance and squash merge, count one manual serial reader and its
31-slot sequence as retired. This is not 31 newly emitted tags: hidden fields
still advance the cursor, and visible output is separately measured. The
remaining Canon child processor and four parent omissions are separate work.

Evidence is under `$OXIDEX_WORK_EVIDENCE/shared-pilot/real-v4-integration-20260913/`:
`native-carrier-comparison.json`, `native-carrier-accepted.json`, build/test
records, `source-upgrade-proof/runtime-replay.json` and `full-pair-20260913/`.
Native fixture generation/contracts are in sibling `real-audio-retirement-20260913/`.
