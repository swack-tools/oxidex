#!/usr/bin/env python3
"""Compare runner benchmark results: the median of each metric per runner.

Reads the <runner>-<run>.json files written by the Runner benchmark workflow
and prints a markdown report for the run summary.

    python3 summarize.py <results dir>
"""

import glob
import json
import statistics
import sys

GH, SELF = "github-hosted", "self-hosted"

# (key, label) in the order the workflow runs them. Seconds, lower is better.
PHASES = [
    ("nextest", "nextest (Build & Test)"),
    ("cargo_test", "cargo test (Build & Test)"),
    ("clippy", "clippy (Lint & Audit)"),
    ("fixloop_build", "fixloop build (Release Build)"),
]

# (key, label). Higher is better for all of these.
SYNTHETIC = [
    ("cpu_1t", "CPU, 1 thread (events/s)"),
    ("cpu_2t", "CPU, 2 threads (events/s)"),
    ("cpu_3t", "CPU, 3 threads (events/s)"),
    ("cpu_4t", "CPU, 4 threads (events/s)"),
    ("cpu_8t", "CPU, 8 threads (events/s)"),
    ("mem_read_1t", "Memory read, 1 thread (MiB/s)"),
    ("mem_write_1t", "Memory write, 1 thread (MiB/s)"),
    ("mem_read_4t", "Memory read, 4 threads (MiB/s)"),
    ("mem_write_4t", "Memory write, 4 threads (MiB/s)"),
    ("disk_seq_read", "Disk sequential read, 1M (MiB/s)"),
    ("disk_seq_write", "Disk sequential write, 1M (MiB/s)"),
    ("disk_rand_read", "Disk random read, 4k QD32 (IOPS)"),
    ("disk_rand_write", "Disk random write, 4k QD32 (IOPS)"),
]


def load(results_dir):
    runs = {}
    for path in sorted(glob.glob(f"{results_dir}/*.json")):
        with open(path) as f:
            result = json.load(f)
        runs.setdefault(result.get("runner", "?"), []).append(result)
    return runs


def median(values):
    values = [v for v in values if v is not None]
    return statistics.median(values) if values else None


def fmt(value):
    if value is None:
        return "–"
    return f"{value:,.1f}" if value < 100 else f"{value:,.0f}"


def relative(gh, self_hosted, higher_is_better):
    """Self-hosted relative to GitHub-hosted; above 1 means self-hosted did better."""
    if not gh or not self_hosted:
        return ""
    ratio = self_hosted / gh if higher_is_better else gh / self_hosted
    return f"{ratio:.2f}×"


def phase(result, key):
    return result.get("phases", {}).get(key, {})


def phase_cell(results, key):
    seconds = median(phase(r, key).get("seconds") for r in results)
    failed = sum(1 for r in results if phase(r, key).get("exit", 0) != 0)
    note = f" ({failed} failed)" if failed else ""
    return seconds, f"{fmt(seconds)}{note}"


def total_seconds(result):
    """All phases of one run added up, or None if any phase is missing."""
    seconds = [phase(result, key).get("seconds") for key, _ in PHASES]
    return None if None in seconds else sum(seconds)


def main():
    runs = load(sys.argv[1] if len(sys.argv) > 1 else "results")
    gh_runs, self_runs = runs.get(GH, []), runs.get(SELF, [])

    print("## Runner benchmark\n")
    print(
        f"Medians of {len(gh_runs)} GitHub-hosted and {len(self_runs)} self-hosted "
        "runs. **Self-hosted relative** above 1.00× means the self-hosted runner "
        "did better.\n"
    )

    print("### ci.yml phases, cold, in seconds (lower is better)\n")
    print("| Phase | GitHub-hosted | Self-hosted | Self-hosted relative |")
    print("|---|---:|---:|---:|")
    for key, label in PHASES:
        g, g_cell = phase_cell(gh_runs, key)
        s, s_cell = phase_cell(self_runs, key)
        print(f"| {label} | {g_cell} | {s_cell} | {relative(g, s, False)} |")
    g = median(total_seconds(r) for r in gh_runs)
    s = median(total_seconds(r) for r in self_runs)
    cells = [fmt(g), fmt(s), relative(g, s, False)]
    print("| **All phases** | " + " | ".join(f"**{c}**" if c else "" for c in cells) + " |")

    print("\n### Synthetic (higher is better)\n")
    print("| Test | GitHub-hosted | Self-hosted | Self-hosted relative |")
    print("|---|---:|---:|---:|")
    for key, label in SYNTHETIC:
        g = median(r.get(key) for r in gh_runs)
        s = median(r.get(key) for r in self_runs)
        print(f"| {label} | {fmt(g)} | {fmt(s)} | {relative(g, s, True)} |")

    print("\n### Environment\n")
    for runner, results in sorted(runs.items()):
        models = ", ".join(sorted({r.get("cpu_model") or "?" for r in results}))
        systems = ", ".join(sorted({r.get("os") or "?" for r in results}))
        nprocs = ", ".join(sorted({str(r.get("nproc")) for r in results}))
        modes = ", ".join(sorted({r.get("disk_mode") or "?" for r in results}))
        peak = median(r.get("phases_peak_mem_gib") for r in results)
        print(
            f"- **{runner}**: {len(results)} runs on {systems}; CPU {models}; "
            f"nproc {nprocs}; disk I/O {modes}; peak memory during phases "
            f"{fmt(peak)} GiB"
        )


if __name__ == "__main__":
    main()
