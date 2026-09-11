# Historical capture path notation

The saved benchmark and fleet/test captures use relative paths for presentation.
Their original bytes remain in commit
`e72f1992e2c14a52acc58c28a9fa4aecc9dce7bb`. Path normalization does not rerun a
measurement or change its numbers, tag values, outcomes, timestamps, or sample
filename suffixes. It does not assert that an external sample or temporary
file still exists.

| Captures | Path notation |
| --- | --- |
| `benches/benchmark_results.json` and its Markdown report | Repository files are relative to the historical checkout root; `benchmark-work/` represents its temporary benchmark directory. |
| `tests/data_lfs_{errors,success}{,_before_fix}.log` | `../examples/data.lfs/` is the historical external sibling corpus, not a checked-in fixture directory. |
| `tools/fleet/tests/fullsuite-cli.txt` | `../keel-2-cli/` identifies the historical sibling worktree. |
| `tools/fleet/tests/fullsuite-election.txt` | `../keel-2-election/` identifies the historical sibling worktree. |
| Both fleet captures | `test-work/` represents the host temporary root; individual temporary-directory names and suffixes are retained. |

The saved JSON changes only file locations inside command strings. All numeric
values, array structure, and other JSON values remain unchanged. The seven
capture files retain their line counts; log content outside the declared path
prefixes is unchanged. These are normalized records, not byte-identical raw
command output or directly executable reproduction commands.

Two explanatory Python docstrings use logical locations instead of personal
paths: the main checkout in `scripts/parallel_model_fix_loop.py`, and the
`external-repos/afx-local.git` presentation alias in `tools/fleet/tests/_env.py`.
Executable Python syntax is unchanged.

Runtime defaults, service installation paths, pinned oracle/cache paths, and
intentional absolute-path test sentinels are separate contracts. In particular,
launchd does not expand shell variables or tilde notation; its installed paths
must remain concrete. This presentation cleanup does not reconfigure services
or weaken those tests.
