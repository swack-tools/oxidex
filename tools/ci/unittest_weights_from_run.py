#!/usr/bin/env python3
"""Regenerate tools/ci/unittest_weights.json from one green GitHub Actions run.

    python3 tools/ci/unittest_weights_from_run.py <run-id> [--out PATH]

Reads the log of every `Verify Generated Tables / tools N/M` job of the run
through `gh run view --job <id> --log` and measures each test as the wall
seconds between consecutive `unittest -v` results: from the previous test's
result line (or the sharder's `shard N/M:` banner, for a shard's first test)
to this test's result line. Class/module fixtures therefore land on the first
test that pays for them, and nothing between two results is lost.

Output keeps the sharder's format: one whole-module total per module, plus a
full test id for any single test at or above EXPLICIT_ID_SECONDS so it can be
placed on its own. `source` records the run id and head SHA.

Every shard is cross-checked before anything is written: the parsed test count
must equal the shard's `Ran N tests`, the measured seconds must account for the
reported `in X s`, and the shards together must cover the run's full suite
exactly once. A log that fails any of these refuses -- a partial parse would
write confident, wrong weights.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from datetime import datetime
from pathlib import Path

EXPLICIT_ID_SECONDS = 30.0
JOB_NAME = re.compile(r"^Verify Generated Tables / tools (\d+)/(\d+)$")
# `gh run view --log` lines: "<job>\t<step>\t<ISO timestamp> <text>".
LOG_LINE = re.compile(r"^[^\t]*\t[^\t]*\t(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?)Z ?(.*)$")
BANNER = re.compile(r"^shard (\d+)/(\d+): (\d+) of (\d+) tests$")
# Python >= 3.11 verbose description: "<method> (<module>.<Class>.<method>)".
HEADER = re.compile(r"^(\w+) \(([\w.]+)\)")
RESULT = re.compile(
    r"(?:^|\.\.\. )(?:ok|FAIL|ERROR|expected failure|unexpected success|skipped .*)$")
RAN = re.compile(r"^Ran (\d+) tests? in ([\d.]+)s$")


def stamp(text):
    # Actions writes 7 fractional digits; fromisoformat wants at most 6.
    head, _, frac = text.partition(".")
    return datetime.fromisoformat(f"{head}.{(frac or '0')[:6]}").timestamp()


def parse_shard(lines):
    """-> (banner, {test_id: seconds}, ran_count, ran_seconds) for one job log."""
    events = []
    for raw in lines:
        match = LOG_LINE.match(raw)
        if match:
            events.append((stamp(match.group(1)), match.group(2).rstrip("\r")))
    starts = [i for i, (_, text) in enumerate(events) if BANNER.match(text)]
    if len(starts) != 1:
        raise ValueError(f"expected one `shard N/M:` banner, found {len(starts)}")
    start = starts[0]
    banner = BANNER.match(events[start][1])
    ran = [(i, RAN.match(text)) for i, (_, text) in enumerate(events) if RAN.match(text)]
    ran = [(i, m) for i, m in ran if i > start]
    if not ran:
        raise ValueError("no `Ran N tests` summary after the banner")
    end, ran_match = ran[-1]
    headers = []
    for i in range(start + 1, end):
        match = HEADER.match(events[i][1])
        if match and match.group(2).endswith("." + match.group(1)):
            headers.append((i, match.group(2)))
    seconds = {}
    previous = events[start][0]
    for k, (index, test_id) in enumerate(headers):
        stop = headers[k + 1][0] if k + 1 < len(headers) else end
        segment = range(index, stop)
        results = [i for i in segment if RESULT.search(events[i][1])]
        done = events[results[-1] if results else stop - 1][0]
        if test_id in seconds:
            raise ValueError(f"duplicate test id {test_id}")
        seconds[test_id] = max(done - previous, 0.0)
        previous = done
    return banner, seconds, int(ran_match.group(1)), float(ran_match.group(2))


def gh(*args):
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout


def collect(run_id):
    meta = json.loads(gh("run", "view", str(run_id), "--json", "jobs,headSha,conclusion"))
    jobs = []
    for job in meta["jobs"]:
        match = JOB_NAME.match(job["name"])
        if match:
            if job["conclusion"] != "success":
                raise SystemExit(f"{job['name']} concluded {job['conclusion']!r}; use a green run")
            jobs.append((int(match.group(1)), int(match.group(2)), job["databaseId"]))
    if not jobs:
        raise SystemExit(f"run {run_id} has no `Verify Generated Tables / tools N/M` jobs")
    totals = {total for _, total, _ in jobs}
    if len(totals) != 1 or sorted(n for n, _, _ in jobs) != list(range(1, totals.pop() + 1)):
        raise SystemExit(f"run {run_id} does not have a complete 1..M set of tools shards")
    per_test, suite_sizes = {}, set()
    for shard, total, job_id in sorted(jobs):
        log = gh("run", "view", "--job", str(job_id), "--log").splitlines()
        banner, seconds, ran, ran_seconds = parse_shard(log)
        if (int(banner.group(1)), int(banner.group(2))) != (shard, total):
            raise SystemExit(f"job {job_id}: banner {banner.group(0)!r} is not shard {shard}/{total}")
        if not len(seconds) == ran == int(banner.group(3)):
            raise SystemExit(f"shard {shard}: parsed {len(seconds)} results, Ran {ran}, "
                             f"banner {banner.group(3)}; the log format changed")
        measured = sum(seconds.values())
        if abs(measured - ran_seconds) > 2.0 + 0.02 * ran_seconds:
            raise SystemExit(f"shard {shard}: measured {measured:.1f}s vs Ran {ran_seconds:.1f}s")
        overlap = per_test.keys() & seconds.keys()
        if overlap:
            raise SystemExit(f"shard {shard}: ids already seen in another shard: {sorted(overlap)[:3]}")
        per_test.update(seconds)
        suite_sizes.add(int(banner.group(4)))
        print(f"shard {shard}/{total}: {ran} tests, {measured:.1f}s measured "
              f"(unittest reported {ran_seconds:.1f}s)", file=sys.stderr)
    if len(suite_sizes) != 1 or len(per_test) != suite_sizes.pop():
        raise SystemExit("shards do not add up to the suite size their banners report")
    return meta["headSha"], per_test


def weights(per_test):
    modules = {}
    for test_id, secs in per_test.items():
        module = test_id.split(".", 1)[0]
        modules[module] = modules.get(module, 0.0) + secs
    out = {module: max(round(secs, 1), 0.1) for module, secs in modules.items()}
    for test_id, secs in per_test.items():
        if secs >= EXPLICIT_ID_SECONDS:
            out[test_id] = round(secs, 1)
    return dict(sorted(out.items()))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("run_id", type=int)
    parser.add_argument("--out", type=Path, default=Path(__file__).with_name("unittest_weights.json"))
    args = parser.parse_args(argv)
    sha, per_test = collect(args.run_id)
    document = {
        "note": f"Module keys are whole-module seconds; full test ids (>={EXPLICIT_ID_SECONDS:g}s) "
                "override their share. Refresh: python3 tools/ci/unittest_weights_from_run.py <run-id>",
        "source": f"GitHub Actions run {args.run_id} at {sha[:12]} (ubuntu-latest shards): "
                  "per-test wall seconds between consecutive unittest -v results",
        "seconds": weights(per_test),
    }
    args.out.write_text(json.dumps(document, indent=1) + "\n")
    print(f"wrote {args.out}: {len(per_test)} tests, "
          f"{sum(per_test.values()):.0f}s total from run {args.run_id}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
