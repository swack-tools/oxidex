#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Break the fleet's 429s down by class -- the one question the harness
could not answer.

WHY THIS EXISTS
---------------
docs/AI_HARNESS.md records **27,662 rate-limit errors over 8 days**
(2026-07-22 -> 2026-07-30) as a single undifferentiated number. It is
undifferentiated because the harness that produced it never read the
gateway's ``theclawbayError`` discriminator, so nothing anywhere recorded
whether a given 429 meant "you are going too fast" or "your budget for the
week is spent". Those demand opposite responses -- one is worth retrying in
a few hundred milliseconds, the other cannot clear for days -- and for eight
days they were indistinguishable.

model_fix_loop.py now classifies every 429 before deciding anything and
appends the class to ``logs/model-calls.jsonl``. This tool is what turns
that into the number the model-layer change is judged by.

WHAT IT REFUSES TO DO
---------------------
It will not present a clean before/after delta, because there isn't one.
The baseline is a **total**; the successor is a **breakdown**. Comparing
them is legitimate for the total and meaningless per class, since the
baseline has no per-class value to compare against -- and no amount of
re-parsing the old logs can recover one. Every report says so in the output
rather than leaving the reader to assume like-for-like.

It also never divides a rate unless both terms were observed over the same
window, the invariant fleet_failrate.py exists to enforce. Where this tool
needs a delivery or patch-apply rate it defers to that script rather than
recomputing it differently.

USAGE
-----
    uv run scripts/rate_limit_report.py                    # last 24h
    uv run scripts/rate_limit_report.py --since 72h        # the P1 window
    uv run scripts/rate_limit_report.py --since 2026-08-03 # from a date
    uv run scripts/rate_limit_report.py --by role --by model
    uv run scripts/rate_limit_report.py --json             # for a dashboard
"""

import argparse
import collections
import datetime
import json
import os
import sys
from pathlib import Path

OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", Path.home() / ".oxidex"))

# docs/AI_HARNESS.md, measured 2026-07-22 -> 2026-07-30. A TOTAL, with no
# per-class breakdown available and none recoverable.
BASELINE_429_TOTAL = 27_662
BASELINE_DAYS = 8
BASELINE_WINDOW = "2026-07-22 -> 2026-07-30"

# The classes model_fix_loop.classify_429 emits. Kept as a literal rather
# than imported so this tool still parses a log written by a newer worker
# than itself -- an unknown class is reported, not dropped (see UNKNOWN).
RPM = "rpm"
WINDOW_CAP = "window_cap"
TERMINAL_CAP = "terminal_cap"
RATE_LIMIT_CLASSES = (RPM, WINDOW_CAP, TERMINAL_CAP)

CLASS_MEANING = {
    RPM: "going too fast -- retried locally with jitter",
    WINDOW_CAP: "5h cost budget spent -- not retried, endpoint parked",
    TERMINAL_CAP: "weekly budget spent or key rejected -- not retried, endpoint parked",
}

# Error classes that are not rate limits. Listed explicitly so that a class
# this build has genuinely never seen -- written by a worker newer than this
# script -- is reported as such rather than being lumped in with them.
KNOWN_OTHER_CLASSES = ("connection", "deadline", "empty_reply")


def is_known_class(name):
    return (name in RATE_LIMIT_CLASSES
            or name in KNOWN_OTHER_CLASSES
            or str(name).startswith("http_"))


def parse_since(spec, now=None):
    """Turn '24h' / '72h' / '7d' / an ISO date into a unix timestamp.

    None means no lower bound. Returns None for an unparseable spec rather
    than raising -- but main() treats that as an error, because silently
    reporting the wrong window is worse than refusing.
    """
    if spec is None:
        return None
    now = now if now is not None else datetime.datetime.now().timestamp()
    text = str(spec).strip()
    if not text:
        return None
    unit = text[-1].lower()
    if unit in ("h", "d", "m") and text[:-1].replace(".", "", 1).isdigit():
        scale = {"m": 60, "h": 3600, "d": 86400}[unit]
        return now - float(text[:-1]) * scale
    for fmt in ("%Y-%m-%dT%H:%M:%S", "%Y-%m-%d"):
        try:
            return datetime.datetime.strptime(text, fmt).timestamp()
        except ValueError:
            continue
    return None


def read_events(path, since=None):
    """Parse model-calls.jsonl into (events, malformed_count, exists).

    A malformed line is COUNTED, never skipped silently: a log the parser
    cannot read is a fact about the measurement, and hiding it is how a
    parser ends up reporting a confident number about half the data.
    """
    path = Path(path)
    events, malformed = [], 0
    try:
        text = path.read_text(errors="replace")
    except OSError:
        return [], 0, False
    for line in text.splitlines():
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except ValueError:
            malformed += 1
            continue
        if not isinstance(event, dict):
            malformed += 1
            continue
        ts = event.get("ts")
        if since is not None and isinstance(ts, (int, float)) and ts < since:
            continue
        events.append(event)
    return events, malformed, True


def count_legacy_429s(manifest_path, since_prefix=None):
    """Count 429s in the pre-classification manifest.log.

    This is the shape of the 27,662 baseline: a substring match, because a
    substring match is all the old format supports. Returns
    (count, total_error_lines, exists). No breakdown is attempted -- there
    is nothing in the line to break down.
    """
    path = Path(manifest_path)
    try:
        text = path.read_text(errors="replace")
    except OSError:
        return 0, 0, False
    hits = errors = 0
    for line in text.splitlines():
        if since_prefix and line[:len(since_prefix)] < since_prefix:
            continue
        if "ERROR=" not in line:
            continue
        errors += 1
        if "429" in line:
            hits += 1
    return hits, errors, True


def summarise(events):
    """Fold events into the counts the P1 exit criteria ask for."""
    by_class = collections.Counter()
    by_outcome = collections.Counter()
    rate_limited_by = {
        "role": collections.Counter(),
        "model": collections.Counter(),
        "endpoint": collections.Counter(),
        "day": collections.Counter(),
    }
    for event in events:
        outcome = event.get("outcome") or "unknown"
        by_outcome[outcome] += 1
        error_class = event.get("error_class")
        if error_class is None:
            continue
        by_class[error_class] += 1
        if error_class not in RATE_LIMIT_CLASSES:
            continue
        for field in ("role", "model", "endpoint"):
            rate_limited_by[field][f"{event.get(field) or '?'} [{error_class}]"] += 1
        ts = event.get("ts")
        if isinstance(ts, (int, float)):
            day = datetime.datetime.fromtimestamp(ts).strftime("%Y-%m-%d")
            rate_limited_by["day"][f"{day} [{error_class}]"] += 1
    return by_class, by_outcome, rate_limited_by


def observed_days(events):
    """Distinct local days the events span -- the denominator for a
    per-day comparison against an 8-day baseline."""
    days = {
        datetime.datetime.fromtimestamp(e["ts"]).strftime("%Y-%m-%d")
        for e in events
        if isinstance(e.get("ts"), (int, float))
    }
    return len(days)


def render(by_class, by_outcome, rate_limited_by, events, malformed,
           legacy, group_by=(), out=print):
    total_429 = sum(by_class[c] for c in RATE_LIMIT_CLASSES)
    other = {k: v for k, v in by_class.items()
             if k is not None and k not in RATE_LIMIT_CLASSES and is_known_class(k)}
    unknown = {k: v for k, v in by_class.items()
               if k is not None and not is_known_class(k)}

    out("")
    out("RATE LIMITS BY CLASS")
    out("=" * 68)
    if not events:
        out("  no classified events in this window.")
        if legacy[2] and legacy[0]:
            out(f"  ({legacy[0]} legacy 429s exist in manifest.log -- the worker "
                "writing model-calls.jsonl may not be deployed yet.)")
        out("")
        return

    days = observed_days(events)
    out(f"  {len(events)} call attempts over {days} day(s)")
    if malformed:
        out(f"  !! {malformed} unparseable line(s) -- this report covers the rest")
    out("")
    for name in RATE_LIMIT_CLASSES:
        count = by_class.get(name, 0)
        share = f"{100 * count / total_429:5.1f}%" if total_429 else "    --"
        out(f"  {name:<14} {count:>7}  {share}   {CLASS_MEANING[name]}")
    out(f"  {'TOTAL 429':<14} {total_429:>7}")
    if other:
        out("")
        out("  not rate limits (shown so the 429 total is not read as all errors):")
        for name, count in sorted(other.items()):
            out(f"    {name:<20} {count:>7}")
    if unknown:
        out("")
        out("  classes this build does not know (a newer worker wrote them):")
        for name, count in sorted(unknown.items()):
            out(f"    {name:<20} {count:>7}")

    out("")
    out("  outcomes: " + ", ".join(
        f"{k}={v}" for k, v in sorted(by_outcome.items())) or "  none")

    # The comparison, stated honestly.
    out("")
    out("AGAINST THE BASELINE")
    out("=" * 68)
    out(f"  baseline : {BASELINE_429_TOTAL:>7} 429s over {BASELINE_DAYS} days "
        f"({BASELINE_WINDOW})")
    out(f"           = {BASELINE_429_TOTAL / BASELINE_DAYS:>7.0f} / day, "
        f"UNCLASSIFIED -- no breakdown exists or can be recovered")
    if days:
        out(f"  observed : {total_429:>7} 429s over {days} day(s)")
        out(f"           = {total_429 / days:>7.0f} / day, "
            f"broken down above")
    out("")
    out("  The totals are comparable. The classes are NOT: the baseline has no")
    out("  per-class value to compare against, because the harness that produced")
    out("  it never read the discriminator. Do not report a per-class delta.")

    if legacy[2]:
        out("")
        out(f"  legacy manifest.log over the same window: {legacy[0]} 429s "
            f"of {legacy[1]} errors (unclassified, for cross-check)")

    for field in group_by:
        counter = rate_limited_by.get(field)
        if not counter:
            continue
        out("")
        out(f"BY {field.upper()}")
        out("=" * 68)
        for key, count in counter.most_common():
            out(f"  {key:<52} {count:>7}")

    # The two findings that change what someone should do next.
    out("")
    out("READING THIS")
    out("=" * 68)
    if total_429 and by_class.get(RPM, 0) / total_429 > 0.8:
        out("  rpm dominates. The fleet is asking for more than the account's")
        out("  per-minute ceiling allows. Retry tuning will not fix that --")
        out("  fleet concurrency is the lever, which is a P3 control.")
    if by_class.get(TERMINAL_CAP) or by_class.get(WINDOW_CAP):
        out("  Cost caps are present. Every one of these was NOT retried and")
        out("  parked its endpoint in one worker only. Check spend headroom")
        out("  before concluding anything about throughput.")
    if total_429 == 0:
        out("  No rate limits in this window.")
    out("")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--logs", type=Path, default=OXIDEX_HOME / "logs",
                        help="log directory (default: $OXIDEX_HOME/logs)")
    parser.add_argument("--since", default="24h",
                        help="window: 24h, 72h, 7d, or an ISO date. "
                             "'all' for no lower bound.")
    parser.add_argument("--by", action="append", default=[],
                        choices=["role", "model", "endpoint", "day"],
                        help="add a breakdown (repeatable)")
    parser.add_argument("--json", action="store_true",
                        help="emit machine-readable counts instead of a report")
    args = parser.parse_args(argv)

    since = None if args.since == "all" else parse_since(args.since)
    if since is None and args.since != "all":
        print(f"unparseable --since {args.since!r}: use 24h, 7d, or an ISO date",
              file=sys.stderr)
        return 2

    events_path = args.logs / "model-calls.jsonl"
    events, malformed, exists = read_events(events_path, since)
    if not exists:
        print(f"{events_path} not found.", file=sys.stderr)
        print("This is written by model_fix_loop.py once the classifying worker "
              "is deployed. Until then there is nothing classified to report.",
              file=sys.stderr)
        return 1

    since_prefix = (datetime.datetime.fromtimestamp(since).strftime("%Y-%m-%dT%H:%M:%S")
                    if since else None)
    legacy = count_legacy_429s(args.logs / "model-fix-requests" / "manifest.log",
                               since_prefix)
    by_class, by_outcome, grouped = summarise(events)

    if args.json:
        total = sum(by_class[c] for c in RATE_LIMIT_CLASSES)
        print(json.dumps({
            "window": args.since,
            "attempts": len(events),
            "days": observed_days(events),
            "malformed_lines": malformed,
            "rate_limits": {c: by_class.get(c, 0) for c in RATE_LIMIT_CLASSES},
            "rate_limits_total": total,
            "outcomes": dict(by_outcome),
            "baseline": {
                "total": BASELINE_429_TOTAL,
                "days": BASELINE_DAYS,
                "window": BASELINE_WINDOW,
                "classified": False,
            },
            "legacy_manifest_429s": legacy[0] if legacy[2] else None,
        }, indent=2))
        return 0

    render(by_class, by_outcome, grouped, events, malformed, legacy,
           group_by=args.by)
    return 0


if __name__ == "__main__":
    sys.exit(main())
