# Unified metadata parity PR

PR #779 consolidates the source inventory, generated readers, public writer foundation, numeric writes, Nikon generation and historical upgrade rehearsal.

The combined branch is not yet validated for landing. Consolidation preserves work and review obligations; it does not turn historical test results into a passing combined gate.

## Incorporated PR heads

| PR | Preserved head | Scope |
| --- | --- | --- |
| [#771](https://github.com/swack-tools/oxidex/pull/771) | `add1e97296e4121ca5ecc25be58ac9feb43ea3ec` | Checkpoint: generated public writer foundation |
| [#772](https://github.com/swack-tools/oxidex/pull/772) | `c1010a30815064cbba21cfa8afb817e3e0fa9b45` | Checkpoint: numeric writes and full native lookup identities |
| [#773](https://github.com/swack-tools/oxidex/pull/773) | `bef2571a75bf57c3221d0df6036de8c2eec763a6` | Checkpoint: recovered Nikon generation and decrypt operands |
| [#774](https://github.com/swack-tools/oxidex/pull/774) | `70981714be93077a6ba3bfd12f7da37acc9591d1` | Checkpoint: historical read/write upgrade rehearsal |

PRs #682 and #683 are excluded and remain independent.

## Validation remaining

- Regenerate the hydrated capture, source-bound QuickTime snapshot and all combined reader/writer ledgers from one pinned canonical source capture. Older numeric artifacts must not replace the later authenticated source closure.
- Run the full combined Rust and Python gates and pinned read/write matrices. Historical passes apply only to their recorded commits and scopes.
- Review and resolve every carried-forward item below before merging #779.
- Validate the integrated writer, Nikon, and rehearsal corrections against the complete regenerated artifacts. The native/public matrix remains unrun on the combined tree.

Integration checks so far: every original PR head is an ancestor; no unresolved Git conflicts; Perl syntax, Python syntax and the 14 merged matrix unit tests pass. These checks do not prove combined runtime parity.

## Carried-forward review items

These were unresolved on the original PRs when consolidation began. Closing the redundant PRs does not resolve them. Original locations and links remain the evidence trail.

- [ ] [PR #771 review](https://github.com/swack-tools/oxidex/pull/771#discussion_r4006933113) — original `tools/exiftool-tables/fresh_jpeg_public_batch_matrix.py:443`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Keep the batch instrument version consistent**

  The required leading instrument header identifies this run as `fresh_jpeg_public_batch_matrix_v1`, while `run_matrix` writes `fresh_jpeg_public_batch_matrix_v2` into the JSON report. Consequently, any gate or baseline keyed by the first header attributes the expanded 207-case/JFIF-adjusted result schema to the older instrument version, making measurement comparisons ambiguous; use the same version identifier in both outputs.

- [ ] [PR #771 review](https://github.com/swack-tools/oxidex/pull/771#discussion_r4006933126) — original `src/writers/jpeg_writer.rs:276`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Preserve whole-EXIF clearing through the mixed transaction**

  When `clear_all_metadata` or an empty-map write targets a JPEG containing both one of the migrated scalars and an ordinary EXIF tag, the planner creates a generated deletion plus legacy changes and reaches this TIFF-payload writer. Because `legacy_metadata` restores the migrated key to mask it from the legacy phase, the legacy map is no longer empty; `rewrite_tiff_payload_with_removals` then refuses removal of the ordinary entry because it cannot shrink that IFD. The previous JPEG planner recognized the empty EXIF-family request and removed the APP1 block, so these files now return `UnsupportedFormat` instead of being cleared; preserve the original whole-EXIF-clear signal before partitioning the transaction.

- [ ] [PR #771 review](https://github.com/swack-tools/oxidex/pull/771#discussion_r4006933130) — original `tools/exiftool-tables/fresh_jpeg_public_batch_matrix.py:400`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Persist the batch report before launching the driver**

  The report is not constructed until after every native call, the Rust fixture subprocess, its return-code check, and result parsing. If the driver exits nonzero, times out, or produces missing/malformed results, the instrument raises before writing `--output`, losing the indexed native calls and selection probes that explain the failed experiment. Initialize and save the report before launching the driver, then record its failure state before propagating the error.

- [ ] [PR #771 review](https://github.com/swack-tools/oxidex/pull/771#discussion_r4006933136) — original `src/writers/jpeg_writer.rs:465`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Honor the generated ExtendedEXIF creation barrier**

  For a JPEG without a standard `Exif\0\0` block but with an `ExtendedEXIF` directory, this fresh-insertion search considers only `creation_skip_markers` and never consumes the generated `creation_wait_for_directories` entry for `ExtendedEXIF`. It therefore treats the carrier as fresh and chooses the first non-APP0 segment rather than waiting for the existing extended directory as the native writer requires, which can create a separate ordinary EXIF block at the wrong boundary while leaving the extended metadata untouched. Recognize the generated directory barrier before selecting the insertion target, or refuse this carrier until it can be identified safely.

- [ ] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910320) — original `tools/exiftool-tables/gen_nikon_encrypted_tables.py:347`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Reject nonempty dumped extra keys**

  When ExifTool introduces an unrecognized Nikon row property, `dump_tag_entry` records it only in `_extra_keys` (`dump_tables.pl:1285`), but this check permits `_extra_keys` and the renderer never inspects its contents. Regeneration can therefore succeed and overwrite the Rust table while silently discarding a new layout or executable control, defeating the generator's stated hard-error contract; require this list to be empty or explicitly validate every admitted key.

  AGENTS.md reference: [AGENTS.md:L36-L40](https://github.com/swack-tools/oxidex/blob/bef2571a75bf57c3221d0df6036de8c2eec763a6/AGENTS.md#L36-L40)

- [ ] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910332) — original `tools/exiftool-tables/test_artifacts.py:19`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Preserve the manifest cardinality guard**

  When an artifact entry is accidentally removed, these replacement assertions still pass as long as each tier retains at least one item; the partition equality is tautological because `select(1)` and `select(2)` are derived from the same manifest. The previous exact counts caught that loss, so this addition should update the expectations to 45 total, 22 tier-1, and 23 tier-2 rather than removing the only inventory-size regression check.

- [ ] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910346) — original `tools/exiftool-tables/gen_nikon_encrypted_tables.py:117`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Escape literal backslash sequences without rewriting them**

  When a native name, enum label, or regex contains the two literal characters `\n`, `\t`, or `\r`, `json.dumps` first escapes the backslash and these replacements then rewrite the second slash-plus-letter; for example `rs(r"a\nb")` emits the Rust literal `"a\\u{a}b"`, which evaluates to a different string. Escape the input character-by-character, as the Sony generator does, so actual control characters become Rust Unicode escapes while literal backslash sequences remain unchanged.

  AGENTS.md reference: [AGENTS.md:L36-L40](https://github.com/swack-tools/oxidex/blob/bef2571a75bf57c3221d0df6036de8c2eec763a6/AGENTS.md#L36-L40)

- [ ] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910356) — original `tools/exiftool-tables/gen_nikon_encrypted_tables.py:193`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Validate Perl regexes against the Rust engine**

  When a recognized model, firmware, or root condition contains a Perl-only regex feature such as lookaround or a backreference, this grammar accepts the pattern verbatim, but `binary_data.rs` compiles it with Rust's `regex` crate and converts compilation failure into a permanently false condition. Regeneration therefore succeeds while silently making the affected table or tag unreachable; restrict the accepted regex grammar or validate compatibility before emitting it.

  AGENTS.md reference: [AGENTS.md:L36-L40](https://github.com/swack-tools/oxidex/blob/bef2571a75bf57c3221d0df6036de8c2eec763a6/AGENTS.md#L36-L40)

- [ ] [PR #773 review](https://github.com/swack-tools/oxidex/pull/773#discussion_r4006910369) — original `tools/exiftool-tables/gen_nikon_encrypted_tables.py:279`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Keep the table namespace in graph identities**

  When the reachable graph contains both `Nikon::Foo` and `NikonCustom::Foo` and a `SubDirectory` explicitly targets the latter, this helper discards the module name; the later `nik.get(n) or custom.get(n)` lookup consequently selects `Nikon::Foo` and emits its rows under the custom edge without any error. Preserve `(module, table)` throughout `queue`, `seen`, and `idx` so identically named tables cannot be conflated.

  AGENTS.md reference: [AGENTS.md:L36-L40](https://github.com/swack-tools/oxidex/blob/bef2571a75bf57c3221d0df6036de8c2eec763a6/AGENTS.md#L36-L40)

- [ ] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900449) — original `tools/exiftool-tables/version_rehearsal_executor.py:664`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Bound and record native-probe timeout cleanup**

  When a native probe reaches its timeout, this sends only `SIGTERM` and then waits with an unbounded `communicate()`; a child that ignores or delays termination can therefore hang the rehearsal indefinitely while holding the host lock. Even when the child exits, the re-raised `TimeoutExpired` is not caught by `_run_native`, leaving the journal in `running` rather than recording a failed stage. Use the bounded SIGTERM/SIGKILL escalation already implemented in `_run_record` and handle the timeout as a stage failure.

- [ ] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900463) — original `tools/exiftool-tables/version_rehearsal_executor.py:481`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Reap descendants during timed-out stage teardown**

  Killing the entire process group simultaneously lets the adapter exit before it can reap its nested child. On Linux with a non-reaping PID 1, that child remains a defunct process indefinitely; `python3 -m unittest -v tools/exiftool-tables/test_version_rehearsal_stage_adapter.py` consequently fails `test_executor_timeout_kills_adapter_nested_child_and_releases_lock`, and repeated timeouts can accumulate zombies. Teardown should allow the adapter to reap its child or otherwise arrange for descendant reaping before returning.

- [ ] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900465) — original `tools/exiftool-tables/version_rehearsal_stage_adapter.py:574`

  **<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  Invoke a matrix producer matching the v4 acceptance contract**

  Every configured write stage invokes `generated_tiff_write_matrix.py`, but that producer identifies its report as `generated_scalar_write_matrix_v2` and omits the v4 fields required here, including `wire_format`, `case_family`, `coverage_family`, and `target_directory`. Consequently `_matrix_report` always refuses the real producer's output—before or at this instrument check—even when every generated/native operation matches, so the persisted 11.78/12.64 write rehearsal cannot pass. The invoked producer and this validator need to use the same report contract.

- [ ] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900471) — original `tools/exiftool-tables/version_rehearsal_executor.py:625`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Mark the release failed when a stage fails**

  When a generate, build, read, or write command fails or publishes an invalid result, this branch marks only the individual stage and top-level phase as failed, then returns `False`; `execute()` immediately returns that journal without updating `journal["releases"][release]["state"]`, which therefore remains `pending` despite its failure record. Consumers inspecting per-release outcomes receive a contradictory state, so this path should set the release state to `failed` as the checkout-exception path already does.

- [ ] [PR #774 review](https://github.com/swack-tools/oxidex/pull/774#discussion_r4006900476) — original `src/tag_db/mod.rs:249`

  **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Prefer a current reverse name over a retired identity**

  When an upgraded ExifTool release renames a migrated scalar while retaining its numeric ID and physical IFD, the migration ledger deliberately keeps the old name terminal and publishes the new name as current. Because `terminal_reverse` matches only the ID and physical group, the retired row triggers this early return and prevents the current `reverse_name` from ever being consulted; reads then expose `IFD0:0xNNNN` even though the newly named descriptor is writable. Check for a current generated reverse match first, or exclude IDs that have a current fact from the terminal reverse projection.

## Integration checkpoint after review corrections

Writer corrections, Nikon generator hardening, and rehearsal timeout/journal fixes were integrated and pushed in `2168208a`. Focused macOS validation: 12 batch-report tests, 33 Nikon/artifact tests, and 39 rehearsal tests passed. The writer checkpoint passed `cargo clippy --lib -- -D warnings`; its writer suite reported 214 passed, 2 ignored, and 2 Artist/IFD1 address refusals. These are incomplete combined-gate results.

The first canonical regeneration used Perl 5.38.2 and pinned ExifTool 13.59. `verify_exprs.py` passed all 607 translated expressions: 16,789 matching comparisons, zero disagreements, and 14 probes skipped because Perl rejected the input. Regeneration then stopped because the merged mandatory-default compiler lacked the cleanup field expected by its generator. `3c175a53` restores that source-authenticated grammar while retaining the later numeric omission handling; the native cleanup mutation test and generation against the fresh full dump passed. A complete regeneration is still required.

The Artist/IFD1 native-table filter is integrated in `8ff6d4ef`. Pinned `SetNewValue` can queue the same name from multiple source tables; `WriteExif` selects the concrete directory table when applying the queued value. The filter preserves full table identity and refuses unknown or unmapped same-table candidates. Rust validation awaits the freshly generated candidate fields.

The separate fresh hydrated capture passes the independent catalog audit: 1,512 tables, 35,886 variants, zero unresolved references, and all 33,487 catalog/source records retain identical semantic joins. Only the dump digest/size and dump-producer digest changed in the audit profile. These are inventory results, not additional observed read/write support.

Remaining carried review checkboxes are intentionally open pending complete regeneration, combined gates, and native evidence. Linux timeout/zombie behavior remains unverified. The registry distribution regression is being updated to count authenticated generated replacements alongside the manual registry, without using YAML fallback or duplicate directory aliases to inflate the count.

### Address regeneration and focused runtime verification

The native probe now authenticates and replays the captured loaded module closure
before comparing exact helper identities. This fixes the dump/probe XMP deparse
context mismatch without weakening body or source authentication. Against the
fresh canonical dump, address codegen emits 191 rows and 1,261 candidates. The
public planner now preserves a proven foreign source-table namespace and refuses
an unmapped same-table candidate.

`cargo clippy --lib -- -D warnings` passes; `cargo test --lib writers::` reports
219 passed, 0 failed, 2 ignored. The exact registry distribution test passes, as
do 18 native mandatory-default tests and six native address-probe tests using
Perl 5.38.2 and ExifTool 13.59. Full regeneration still stops at the Nikon
encrypted-callback contract; these focused checks do not replace the combined
workspace/native matrix gate or resolve its pending review evidence.

### Authenticated implementation accounting

The fresh full capture and catalog join conserve all 33,487 entries. Complete
QuickTime compiler/selector replay binds the bounded source, declaration ledger,
capability ledger and emitted Rust. The joined ledger classifies 92 catalog entries
as generated reader declarations, 211 as explicit reader refusals and 33,184 as
not yet consumed by this implementation join. All observed read/write fields remain
unobserved. Fourteen join tests and 33 QuickTime baseline/generator tests pass.
The generated QuickTime Rust is byte-identical to the previous artifact.

The Nikon repair is integrated and generates 91 tables/2,317 rows from an actual
canonical native reader capture; 17 focused tests pass. Full regeneration is the
next check. No broader parity or completed review gate is claimed here.


## Latest integration checkpoint

At `be0ffe0c`, canonical regeneration, all-feature Rust (6,093 passed, zero
failed, 126 ignored), and all-feature Clippy passed for the recorded scope.
The corrected Python gate completed 425 tests in 400.693 seconds, exit zero,
with six skipped. Its controller verified unchanged HEAD and tracked files.
A separate clean-checkout run at that same commit passed all 29 fresh-JPEG
batch and Nikon generator tests.

| Carried finding | Direct completed evidence | Remaining evidence |
| --- | --- | --- |
| 1: batch instrument version | `test_instrument_identifier_is_single_sourced` | Combined landing gate |
| 3: persist failed batch reports | `test_predriver_failure_persists_terminal_report`, `test_driver_timeout_persists_terminal_report` | Combined landing gate |
| 5: Nikon extra keys | `test_dumped_extra_keys_must_be_empty`; canonical regeneration | Combined landing gate |
| 6: artifact cardinality | `ManifestTests.test_unique_valid_partition_and_selectors` checks 63 total, 40 tier-1, 23 tier-2 | Combined landing gate |
| 7: literal backslashes | `test_rust_string_escaping_preserves_literal_backslash_sequences`; canonical regeneration | Combined landing gate |
| 8: regex compatibility | `test_perl_only_regexes_refuse_before_emission`; canonical regeneration | Combined landing gate |
| 9: table namespaces | `test_graph_identity_keeps_nikon_and_nikoncustom_name_collisions_distinct`; canonical regeneration | Combined landing gate |
| 10, 13: native timeout and release state | Native-timeout, cleanup-error and failed-execution regression tests passed | Actual version rehearsal remains separate |
| 11: descendant reaping | Nested-child and late-child timeout tests passed on macOS | Same tests on Linux |
| 12: v4 matrix contract | Adapter report-contract tests passed | Actual materialized version rehearsal |
| 2, 4: mixed EXIF clearing and ExtendedEXIF | Writer corrections are in the tested Rust tree | Native carrier/mixed-transaction checks |
| 14: current reverse identity | Current reverse lookup precedes the retired fallback in the tested Rust tree | Focused reverse-name and actual upgrade evidence |

The source ledger now joins 179 QuickTime catalog reader declarations, 151
QuickTime refusals and the full IFD schema ledger. See
`goal-checkpoint-20260914.md` for the separate source and observation counts.
A fresh M4 reader/write/readback run at `be0ffe0c` is collecting compatible
receipts for the observation publisher. These results must not be credited
to the subsequent writer cleanup before that change's own gates finish.

The writer cleanup passed its 18 focused source tests at `8f49942c`; its
combined Rust/native validation is in progress. All review threads remain
visible until their corresponding evidence is recorded and checked.
