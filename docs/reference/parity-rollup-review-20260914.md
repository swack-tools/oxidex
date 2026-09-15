# Unified metadata parity PR

PR #779 consolidates the source inventory, generated readers, public writer foundation, numeric writes, Nikon generation and historical upgrade rehearsal.

Feature expansion is paused at the maintainer's request. The final merge state, integrated gate results and evidence replies are recorded on this PR. Historical tests retain their original commit and scope.

## Incorporated PR heads

| PR | Preserved head | Scope |
| --- | --- | --- |
| [#771](https://github.com/swack-tools/oxidex/pull/771) | `add1e97296e4121ca5ecc25be58ac9feb43ea3ec` | Checkpoint: generated public writer foundation |
| [#772](https://github.com/swack-tools/oxidex/pull/772) | `c1010a30815064cbba21cfa8afb817e3e0fa9b45` | Checkpoint: numeric writes and full native lookup identities |
| [#773](https://github.com/swack-tools/oxidex/pull/773) | `bef2571a75bf57c3221d0df6036de8c2eec763a6` | Checkpoint: recovered Nikon generation and decrypt operands |
| [#774](https://github.com/swack-tools/oxidex/pull/774) | `70981714be93077a6ba3bfd12f7da37acc9591d1` | Checkpoint: historical read/write upgrade rehearsal |

PRs #682 and #683 are excluded and remain independent.

## Landing validation

- Pinned canonical regeneration passed both selected tiers at `6f34cff4`; final regeneration at `980d5efe` also passed both tiers with zero net generated changes.
- Combined checkpoint `bc4f617e`: workspace/all-feature Rust 6,120 passed, zero failed, 126 ignored; formatting and workspace/all-feature Clippy passed; 105 focused Python tests passed. Later generator repairs require their own regeneration checks.
- Common-runtime observations at `be0ffe0c` are published in the [authenticated snapshot](catalog-hydrated-observed.md). Native results remain historical and do not validate later runtime changes.
- Linux executor/adapter teardown: 41 tests passed, zero skips at `be0ffe0c`; both recorded nested processes were absent after teardown. The source files are unchanged in the later integration.
- The local M5 writer checkpoint at `b4ca393d` passed all 1,530 cases and 1,353 authenticated positive write/readbacks across 38 Group1 names. The selected rehearsal report contract replay now passes all 1,530 rows, preserving the 876 baseline cases; adapter/executor tests passed all 45 cases. The separate 48-carrier comparison exposed a JPEG cleanup mismatch, repaired in `31e87191`; its fresh native rerun at agent commit `7b1cf056` now passes all 48 cases. The materialized historical rehearsal explicitly refused unsupported 11.78 `SetNewValue` caller control flow after passing its native probe; no historical read/write or 12.64 result is claimed. Source profiles for mandatory defaults are integrated, but they do not imply support for the entire historical caller.
- All fourteen original review items have matching validation evidence. The reverse-name regression remains in the full tag database suite; its test-only fixture also composes in the portable harness. The two JPEG carrier regressions pass in the 20-test JPEG module run. Original review threads retain the linked replies and resolution state.

Every original PR head remains an ancestor. PRs #682 and #683 are excluded.

## Carried-forward review items

These were unresolved on the original PRs when consolidation began. Closing the redundant PRs does not resolve them. Original locations and links remain the evidence trail.

- [x] [PR #771 review](https://github.com/swack-tools/oxidex/pull/771#discussion_r4006933113) — original `tools/exiftool-tables/fresh_jpeg_public_batch_matrix.py:443`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Keep the batch instrument version consistent**

  The required leading instrument header identifies this run as `fresh_jpeg_public_batch_matrix_v1`, while `run_matrix` writes `fresh_jpeg_public_batch_matrix_v2` into the JSON report. Consequently, any gate or baseline keyed by the first header attributes the expanded 207-case/JFIF-adjusted result schema to the older instrument version, making measurement comparisons ambiguous; use the same version identifier in both outputs.

- [x] [PR #771 review](https://github.com/swack-tools/oxidex/pull/771#discussion_r4006933126) — original `src/writers/jpeg_writer.rs:276`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Preserve whole-EXIF clearing through the mixed transaction**

  When `clear_all_metadata` or an empty-map write targets a JPEG containing both one of the migrated scalars and an ordinary EXIF tag, the planner creates a generated deletion plus legacy changes and reaches this TIFF-payload writer. Because `legacy_metadata` restores the migrated key to mask it from the legacy phase, the legacy map is no longer empty; `rewrite_tiff_payload_with_removals` then refuses removal of the ordinary entry because it cannot shrink that IFD. The previous JPEG planner recognized the empty EXIF-family request and removed the APP1 block, so these files now return `UnsupportedFormat` instead of being cleared; preserve the original whole-EXIF-clear signal before partitioning the transaction.

- [x] [PR #771 review](https://github.com/swack-tools/oxidex/pull/771#discussion_r4006933130) — original `tools/exiftool-tables/fresh_jpeg_public_batch_matrix.py:400`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Persist the batch report before launching the driver**

  The report is not constructed until after every native call, the Rust fixture subprocess, its return-code check, and result parsing. If the driver exits nonzero, times out, or produces missing/malformed results, the instrument raises before writing `--output`, losing the indexed native calls and selection probes that explain the failed experiment. Initialize and save the report before launching the driver, then record its failure state before propagating the error.

- [x] [PR #771 review](https://github.com/swack-tools/oxidex/pull/771#discussion_r4006933136) — original `src/writers/jpeg_writer.rs:465`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Honor the generated ExtendedEXIF creation barrier**

  For a JPEG without a standard `Exif\0\0` block but with an `ExtendedEXIF` directory, this fresh-insertion search considers only `creation_skip_markers` and never consumes the generated `creation_wait_for_directories` entry for `ExtendedEXIF`. It therefore treats the carrier as fresh and chooses the first non-APP0 segment rather than waiting for the existing extended directory as the native writer requires, which can create a separate ordinary EXIF block at the wrong boundary while leaving the extended metadata untouched. Recognize the generated directory barrier before selecting the insertion target, or refuse this carrier until it can be identified safely.

- [x] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910320) — original `tools/exiftool-tables/gen_nikon_encrypted_tables.py:347`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Reject nonempty dumped extra keys**

  When ExifTool introduces an unrecognized Nikon row property, `dump_tag_entry` records it only in `_extra_keys` (`dump_tables.pl:1285`), but this check permits `_extra_keys` and the renderer never inspects its contents. Regeneration can therefore succeed and overwrite the Rust table while silently discarding a new layout or executable control, defeating the generator's stated hard-error contract; require this list to be empty or explicitly validate every admitted key.

  AGENTS.md reference: [AGENTS.md:L36-L40](https://github.com/swack-tools/oxidex/blob/bef2571a75bf57c3221d0df6036de8c2eec763a6/AGENTS.md#L36-L40)

- [x] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910332) — original `tools/exiftool-tables/test_artifacts.py:19`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Preserve the manifest cardinality guard**

  When an artifact entry is accidentally removed, these replacement assertions still pass as long as each tier retains at least one item; the partition equality is tautological because `select(1)` and `select(2)` are derived from the same manifest. The previous exact counts caught that loss, so this addition should update the expectations to 45 total, 22 tier-1, and 23 tier-2 rather than removing the only inventory-size regression check.

- [x] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910346) — original `tools/exiftool-tables/gen_nikon_encrypted_tables.py:117`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Escape literal backslash sequences without rewriting them**

  When a native name, enum label, or regex contains the two literal characters `\n`, `\t`, or `\r`, `json.dumps` first escapes the backslash and these replacements then rewrite the second slash-plus-letter; for example `rs(r"a\nb")` emits the Rust literal `"a\\u{a}b"`, which evaluates to a different string. Escape the input character-by-character, as the Sony generator does, so actual control characters become Rust Unicode escapes while literal backslash sequences remain unchanged.

  AGENTS.md reference: [AGENTS.md:L36-L40](https://github.com/swack-tools/oxidex/blob/bef2571a75bf57c3221d0df6036de8c2eec763a6/AGENTS.md#L36-L40)

- [x] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910356) — original `tools/exiftool-tables/gen_nikon_encrypted_tables.py:193`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Validate Perl regexes against the Rust engine**

  When a recognized model, firmware, or root condition contains a Perl-only regex feature such as lookaround or a backreference, this grammar accepts the pattern verbatim, but `binary_data.rs` compiles it with Rust's `regex` crate and converts compilation failure into a permanently false condition. Regeneration therefore succeeds while silently making the affected table or tag unreachable; restrict the accepted regex grammar or validate compatibility before emitting it.

  AGENTS.md reference: [AGENTS.md:L36-L40](https://github.com/swack-tools/oxidex/blob/bef2571a75bf57c3221d0df6036de8c2eec763a6/AGENTS.md#L36-L40)

- [x] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910369) — original `tools/exiftool-tables/gen_nikon_encrypted_tables.py:279`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Keep the table namespace in graph identities**

  When the reachable graph contains both `Nikon::Foo` and `NikonCustom::Foo` and a `SubDirectory` explicitly targets the latter, this helper discards the module name; the later `nik.get(n) or custom.get(n)` lookup consequently selects `Nikon::Foo` and emits its rows under the custom edge without any error. Preserve `(module, table)` throughout `queue`, `seen`, and `idx` so identically named tables cannot be conflated.

  AGENTS.md reference: [AGENTS.md:L36-L40](https://github.com/swack-tools/oxidex/blob/bef2571a75bf57c3221d0df6036de8c2eec763a6/AGENTS.md#L36-L40)

- [x] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900449) — original `tools/exiftool-tables/version_rehearsal_executor.py:664`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Bound and record native-probe timeout cleanup**

  When a native probe reaches its timeout, this sends only `SIGTERM` and then waits with an unbounded `communicate()`; a child that ignores or delays termination can therefore hang the rehearsal indefinitely while holding the host lock. Even when the child exits, the re-raised `TimeoutExpired` is not caught by `_run_native`, leaving the journal in `running` rather than recording a failed stage. Use the bounded SIGTERM/SIGKILL escalation already implemented in `_run_record` and handle the timeout as a stage failure.

- [x] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900463) — original `tools/exiftool-tables/version_rehearsal_executor.py:481`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Reap descendants during timed-out stage teardown**

  Killing the entire process group simultaneously lets the adapter exit before it can reap its nested child. On Linux with a non-reaping PID 1, that child remains a defunct process indefinitely; `python3 -m unittest -v tools/exiftool-tables/test_version_rehearsal_stage_adapter.py` consequently fails `test_executor_timeout_kills_adapter_nested_child_and_releases_lock`, and repeated timeouts can accumulate zombies. Teardown should allow the adapter to reap its child or otherwise arrange for descendant reaping before returning.

- [x] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900465) — original `tools/exiftool-tables/version_rehearsal_stage_adapter.py:574`

  **<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  Invoke a matrix producer matching the v4 acceptance contract**

  Every configured write stage invokes `generated_tiff_write_matrix.py`, but that producer identifies its report as `generated_scalar_write_matrix_v2` and omits the v4 fields required here, including `wire_format`, `case_family`, `coverage_family`, and `target_directory`. Consequently `_matrix_report` always refuses the real producer's output—before or at this instrument check—even when every generated/native operation matches, so the persisted 11.78/12.64 write rehearsal cannot pass. The invoked producer and this validator need to use the same report contract.

- [x] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900471) — original `tools/exiftool-tables/version_rehearsal_executor.py:625`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Mark the release failed when a stage fails**

  When a generate, build, read, or write command fails or publishes an invalid result, this branch marks only the individual stage and top-level phase as failed, then returns `False`; `execute()` immediately returns that journal without updating `journal["releases"][release]["state"]`, which therefore remains `pending` despite its failure record. Consumers inspecting per-release outcomes receive a contradictory state, so this path should set the release state to `failed` as the checkout-exception path already does.

- [x] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900476) — original `src/tag_db/mod.rs:249`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Prefer a current reverse name over a retired identity**

  When an upgraded ExifTool release renames a migrated scalar while retaining its numeric ID and physical IFD, the migration ledger deliberately keeps the old name terminal and publishes the new name as current. Because `terminal_reverse` matches only the ID and physical group, the retired row triggers this early return and prevents the current `reverse_name` from ever being consulted; reads then expose `IFD0:0xNNNN` even though the newly named descriptor is writable. Check for a current generated reverse match first, or exclude IDs that have a current fact from the terminal reverse projection.

## Evidence and scope

The original comments above are preserved as review history; checked items have evidence replies on their original threads. Current completion state is recorded in the landing validation list, not in the original defect text.

- Canonical regeneration: both tiers passed against explicit Perl 5.38.2 and ExifTool 13.59 at `6f34cff4`. Expression verification matched all 16,789 evaluated comparisons over 607 translated expressions; 14 native-rejected probes were skipped.
- Python integration at `be0ffe0c`: 425 tests, six skipped, exit zero, unchanged source. A separate 29-test fresh-JPEG/Nikon run passed.
- Final workspace checkpoint at `d0c1b6ae`: formatting and Clippy passed; 6,121 Rust tests passed, zero failed, 126 ignored; 118 focused Python tests completed successfully, one skipped. The exact renamed-current/retired-address regression executed successfully.
- Linux teardown at `be0ffe0c`: 41 tests passed without skips. Native and adapter timeout tests confirmed descendant cleanup and lock release.
- Historical observations at `be0ffe0c`: 1,530 TIFF/JPEG cases matched, including 1,353 positive write/readbacks. The UserData reader matched all 126 comparisons across 63 fixtures. The authenticated catalog snapshot is published separately and retains its original runtime identity.
- Final compiler repairs: all eight replacement/deletion mutations of Intel/Motorola byte-pack C/c mappings refused; the public numeric compiler retains exactly its two supported formats while the private cleanup closure admits seven physical formats. Independent review found both blockers resolved.
- Actual pinned native survivor fixtures cover 48 combinations of TIFF/JPEG carrier, little/big endian, four integer storage types and counts zero/one/two. Native behavior removes matching count-one defaults and preserves count-zero/count-two fields. The fresh `fixedwidth_ifd1_public_write_v1` native/OxiDex comparison at `7b1cf056` passed all 48 cases.

The earlier `--all-targets` Clippy probe exposed pre-existing integration/forensic test lint failures. The required workspace/all-feature Clippy command passes; no all-targets pass is claimed.

The full catalog-wide parity goal remains incomplete. See [the checkpoint](goal-checkpoint-20260914.md) and [resume instructions](metadata-parity-resume.md).
