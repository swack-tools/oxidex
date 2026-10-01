#!/usr/bin/env python3
"""Model-call failure rate for the running fleet, from the one log that
records both outcomes.

WHY THIS EXISTS
---------------
The predecessor of this script grepped ``logs/fleet-up.log`` for
failure-shaped keywords ("timeout", "Traceback", "REJECTED", ...) and
divided by ``ok + fails``, where ``ok`` came from a pattern
(``model call ok|assistant reply received``) that matched **zero lines in
that file, ever**. With the success term structurally pinned at 0, the
arithmetic reduces to ``N/N`` and the monitor reported 100% failure
forever -- through seven healthy hours, indistinguishable from a real
outage. fleet-up.log is the dispatcher/merger supervisor log; individual
model calls are never written there at all.

So this tool enforces one invariant everywhere it reports a rate:

    A rate is only printed when the SUCCESS term and the FAILURE term
    were BOTH observed by the same parser over the same window.

If the success term is missing, the tool says so and exits non-zero
instead of dividing. "Everything failed" and "I cannot read the log" are
different facts and are never collapsed into the same number.

Discriminating those two cases is decidable, and this tool decides it:
zero OK records *in the window* while OK records exist *elsewhere in the
same file* is a real outage; zero OK records anywhere in a non-empty file
is a blind parser. See ``classify_ok_absence``.

THE AUTHORITATIVE SOURCE
------------------------
``logs/model-fix-requests/manifest.log``. model_fix_loop.py's
``make_logging_call_model`` writes the request JSON *before* the call,
appends ``ERROR={e}`` and re-raises when the call throws, and appends a
line ending in ``OK`` when it returns. That file -- and only that file --
carries both outcomes for the same population of calls.

Sources that were tried and are wrong, each failing in the same
direction because each counts "no evidence of success yet" as failure:

* Pairing request.json against response.txt. The request is written
  before the call and calls run long (1794s measured), so every in-flight
  request looks failed. In-flight is reported here as its own number and
  never as a failure.
* ``lessons.jsonl``. Records only outcomes worth learning from -- a
  numerator at best, never a denominator.
* ``pgrep -fc``. Returns a confident 0 for patterns that plainly match.
* Grepping fleet-up.log for failure keywords. No success term exists.

``RETRY`` lines are attempts absorbed inside ``call_model``'s own loop: a
call that retries twice and then returns is one success. They are
reported separately and never counted as failures.

Manifest timestamps are the *request* time, so a call started at 16:23:57
and finishing 903s later is logged under 16:23:57 -- the file is not in
completion order, but windowing by line prefix is still correct.

Usage:
    scripts/fleet_failrate.py                      # whole history
    scripts/fleet_failrate.py 2026-07-30T16:05     # since a cutoff
    scripts/fleet_failrate.py --since 2026-07-30T16:05 --json

Exit status:
    0  a rate was computed and printed
    2  the rate is UNKNOWN: log missing, empty, or unparseable
"""
import argparse
import json
import os
import re
import sys
import time
from pathlib import Path

# One regex, one authority where possible: watch_parallel_fix.py's dashboard
# parses this same file with this same pattern, so sharing it means a future
# manifest format change has one place to be fixed and the dashboard and this
# monitor can never disagree about what a line means.
#
# But the import is GUARDED. A monitor that raises ImportError reports
# nothing at all, which is strictly worse than the wrong number it was
# written to replace -- and this pattern is used only for the strict half of
# a two-tier read, so a vendored fallback costs accuracy in nothing but the
# drift warning. Availability of the measurement outranks provenance of the
# regex.
sys.path.insert(0, str(Path(__file__).resolve().parent))
try:
    from watch_parallel_fix import MANIFEST_ENTRY_RE
except Exception:  # ImportError, or anything that module drags in
    MANIFEST_ENTRY_RE = re.compile(
        r"^(?P<ts>\S+) phase=(?P<phase>fixer|reviewer|critique) worker=(?P<worker>\S+)"
        r"(?: tier=(?P<tier>\S+))? model=(?P<model>\S+) "
        r"prompt_chars=(?P<prompt_chars>\d+) elapsed=(?P<elapsed>[\d.]+)s "
        r"(?:reply_chars=\d+ )?(?P<rest>OK|ERROR=.*)$"
    )

DEFAULT_LOGS = Path(os.environ.get(
    "OXIDEX_HOME", str(Path.home() / ".oxidex"))) / "logs"

# A request whose outcome the manifest has not recorded is unsettled. It is
# in flight until it has been unsettled longer than any call could legally
# run, after which it is stale -- almost always a worker killed mid-call by
# a fleet restart. model_fix_loop's deadline is 600s; this leaves generous
# room for retry backoff on top, per the rule that a grace period must
# exceed deadline plus backoff rather than intuition about API latency.
# Neither in-flight nor stale is a failure, and neither enters the rate.
DEFAULT_STALE_AFTER_SECONDS = 1800

# Deliberately loose companion to MANIFEST_ENTRY_RE: an ISO timestamp and a
# terminal outcome token, with everything in between unconstrained. This is
# the drift detector. When a field is added mid-line (as `tier=` once was),
# the strict pattern stops matching while this one keeps working, so the
# success term survives and the gap between the two counts is reported as a
# loud warning -- rather than the success term silently collapsing to zero
# and the rate jumping to 100%.
TOLERANT_OK_RE = re.compile(r"^(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})\b.*\bOK$")
TOLERANT_ERR_RE = re.compile(r"^(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})\b.*\bERROR=(?P<msg>.*)$")
TOLERANT_RETRY_RE = re.compile(r"^(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})\b.* RETRY ")

# Loose identity of a call, for pairing request artifacts against outcomes.
# Kept independent of MANIFEST_ENTRY_RE so that format drift degrades the
# in-flight count gracefully instead of making every request look unsettled.
TOLERANT_KEY_RE = re.compile(
    r"^(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}) phase=(?P<phase>\S+) worker=(?P<worker>\S+)"
)

# logs/model-fix-diffs/manifest.log: one line per patch-apply attempt, with
# its own success term (`applied=True`) and failure term (`applied=False`).
# Same invariant, separately enforced, and the same two-tier read.
#
# `worker=` and `rung=` are OPTIONAL. 358 lines written before diffs were
# worker-tagged carry neither, and a first cut of this parser that required
# `worker=` silently dropped every one of them -- the identical
# over-strict-parser mistake this whole tool exists to catch, caught by
# cross-checking the parsed total against `wc -l`. Anything the loose read
# understands and the strict one does not is reported as drift.
DIFF_TOLERANT_RE = re.compile(
    r"^(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})\b.*\bapplied=(?P<applied>True|False)\b"
)
DIFF_ENTRY_RE = re.compile(
    r"^(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}) (?:worker=(?P<worker>\S+) )?"
    r"applied=(?P<applied>True|False)(?: rung=(?P<rung>\S+))? file=(?P<file>\S+) "
    r"apply_msg=(?P<msg>.*)$"
)

# {ts}-{worker}-{phase}-request.json, and the legacy {ts}-{phase}-request.json
# written before request artifacts were worker-tagged. Worker labels contain
# hyphens ("canon-1"), so the worker group must be greedy-tolerant.
REQUEST_FILE_RE = re.compile(
    r"^(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})"
    r"(?:-(?P<worker>.+))?-(?P<phase>fixer|reviewer|critique)-request\.json$"
)

TS_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$")
TS_FORMAT = "%Y-%m-%dT%H:%M:%S"


def fmt_age(seconds):
    seconds = max(0.0, seconds)
    if seconds < 90:
        return f"{seconds:.0f}s"
    if seconds < 5400:
        return f"{seconds / 60:.0f}m"
    return f"{seconds / 3600:.1f}h"


class Outcomes:
    """Counts for one population of attempts, with the success term and the
    failure term tracked separately so a rate can refuse to exist."""

    def __init__(self, name):
        self.name = name
        self.ok = 0
        self.err = 0
        self.retry = 0
        self.strict = 0          # lines the structured parser understood
        self.tolerant = 0        # lines the loose parser understood
        self.lines_in_window = 0
        self.ok_outside_window = 0
        self.errkinds = {}
        self.elapsed = []
        # The last timestamp the log itself carries, and the file's mtime.
        # Reported whenever a window comes up empty, because "no records
        # matched" has two very different causes and the reader must be able
        # to tell them apart at a glance: a log that stopped being written
        # (fleet down) versus a window computed wrongly (query broken).
        # Manifests are stamped in LOCAL time; a UTC-derived window silently
        # lands hours in the future and matches nothing while the fleet is
        # healthy, so all window arithmetic here stays in local time.
        self.last_ts = None
        self.last_write = None

    def note_line(self, line):
        ts = line[:19]
        if TS_RE.match(ts) and (self.last_ts is None or ts > self.last_ts):
            self.last_ts = ts

    def freshness(self, now=None):
        """Human phrase describing how current the underlying log is."""
        now = time.time() if now is None else now
        bits = []
        if self.last_ts:
            try:
                age = now - time.mktime(time.strptime(self.last_ts, TS_FORMAT))
                bits.append(f"last record {self.last_ts} ({fmt_age(age)} ago)")
            except ValueError:
                bits.append(f"last record {self.last_ts}")
        if self.last_write is not None:
            bits.append(f"file written {fmt_age(now - self.last_write)} ago")
        bits.append(f"now {time.strftime(TS_FORMAT, time.localtime(now))} local")
        return "; ".join(bits)

    @property
    def settled(self):
        return self.ok + self.err

    @property
    def rate(self):
        return (100.0 * self.err / self.settled) if self.settled else None

    @property
    def drift(self):
        """Lines the loose parser understood but the structured one did not.
        Non-zero means the manifest format moved and the structured readers
        (this tool, watch_parallel_fix's dashboard) are going blind."""
        return max(0, self.tolerant - self.strict)


def classify_ok_absence(counts, path_exists):
    """Why are there no successes? Returns (status, message).

    This is the whole point of the tool. ``ok == 0`` is ambiguous on its
    face, and the previous monitor resolved that ambiguity by assuming the
    worst and reporting 100%. It is not actually ambiguous:

    * no log at all, or no lines in the window   -> NO_DATA
    * lines in the window, none understood       -> BLIND (format changed)
    * no OK in the window, but OK exists in the
      same file outside it                       -> OUTAGE (really 100%)
    * no OK anywhere in a non-empty file         -> BLIND (format changed)
    """
    if not path_exists:
        return "NO_DATA", f"{counts.name}: manifest not found -- nothing to measure"
    # Freshness accompanies every empty/blind verdict. "No records matched"
    # is ambiguous between a stopped fleet and a mis-computed window, and the
    # last record's own timestamp next to the current local time resolves it
    # without a second command. (A UTC-derived cutoff against these
    # local-time manifests lands hours in the future and matches nothing --
    # the reader sees a last record newer than the cutoff and knows at once
    # that the query, not the fleet, is broken.)
    when = counts.freshness()
    if counts.lines_in_window == 0:
        return ("NO_DATA",
                f"{counts.name}: no log lines in this window -- nothing to "
                f"measure ({when})")
    if counts.tolerant == 0:
        return ("BLIND",
                f"{counts.name}: parser understood 0 of {counts.lines_in_window} "
                f"lines in this window -- log format may have changed ({when})")
    if counts.ok_outside_window > 0:
        return ("OUTAGE",
                f"{counts.name}: 0 successes in this window, but "
                f"{counts.ok_outside_window} elsewhere in the same file -- "
                f"the parser works, this is a real outage ({when})")
    return ("BLIND",
            f"{counts.name}: parser found no OK records anywhere in "
            f"{counts.lines_in_window} lines -- log format may have changed "
            f"({when})")


def read_model_calls(manifest_path, since):
    """Parse the model-call manifest into an Outcomes.

    Every line is read twice: once with the structured MANIFEST_ENTRY_RE
    shared with the dashboard, and once with the loose patterns. The rate is
    computed from the loose read, so a mid-line field addition cannot zero
    the success term; the gap between the two reads is surfaced as drift.
    """
    counts = Outcomes("model calls")
    path = Path(manifest_path)
    try:
        text = path.read_text(errors="replace")
    except OSError:
        return counts, False
    try:
        counts.last_write = path.stat().st_mtime
    except OSError:
        pass

    for line in text.splitlines():
        if not line:
            continue
        # Freshness is measured over the WHOLE file, not just the window:
        # when a window comes up empty, the last record anywhere in the log
        # is exactly the fact that separates "fleet stopped" from
        # "window computed wrong".
        counts.note_line(line)
        in_window = line[:len(since)] >= since if since else True
        if not in_window:
            # Still worth knowing whether successes exist OUTSIDE the window:
            # that is what distinguishes a real outage from a blind parser.
            if TOLERANT_OK_RE.match(line):
                counts.ok_outside_window += 1
            continue

        counts.lines_in_window += 1

        if TOLERANT_RETRY_RE.match(line):
            # Not a settled outcome, and MANIFEST_ENTRY_RE deliberately does
            # not match RETRY lines, so they are excluded from the strict-vs-
            # loose drift comparison too -- otherwise every retry would read
            # as format drift.
            counts.retry += 1
            continue

        m_err = TOLERANT_ERR_RE.match(line)
        if m_err:
            counts.err += 1
            counts.tolerant += 1
            kind = m_err.group("msg").strip()[:70]
            counts.errkinds[kind] = counts.errkinds.get(kind, 0) + 1
        elif TOLERANT_OK_RE.match(line):
            counts.ok += 1
            counts.tolerant += 1
        else:
            continue

        if MANIFEST_ENTRY_RE.match(line):
            counts.strict += 1
        for tok in line.split():
            if tok.startswith("elapsed=") and tok.endswith("s"):
                try:
                    counts.elapsed.append(float(tok[len("elapsed="):-1]))
                except ValueError:
                    pass

    return counts, True


def read_patch_applies(manifest_path, since):
    """Parse the diff manifest: applied=True is the success term."""
    counts = Outcomes("patch applies")
    path = Path(manifest_path)
    try:
        text = path.read_text(errors="replace")
    except OSError:
        return counts, False
    try:
        counts.last_write = path.stat().st_mtime
    except OSError:
        pass
    for line in text.splitlines():
        counts.note_line(line)

    for line in text.splitlines():
        if not line:
            continue
        loose = DIFF_TOLERANT_RE.match(line)
        in_window = line[:len(since)] >= since if since else True
        if not in_window:
            if loose and loose.group("applied") == "True":
                counts.ok_outside_window += 1
            continue
        counts.lines_in_window += 1
        if not loose:
            continue
        counts.tolerant += 1
        if DIFF_ENTRY_RE.match(line):
            counts.strict += 1
        if loose.group("applied") == "True":
            counts.ok += 1
        else:
            counts.err += 1
            _, sep, reason = line.partition("apply_msg=")
            reason = reason.strip().strip("'\"") if sep else "(no apply_msg)"
            counts.errkinds[reason[:70]] = counts.errkinds.get(reason[:70], 0) + 1
    return counts, True


def settled_keys(manifest_path, since):
    """(ts, worker, phase) for every call the manifest has an outcome for."""
    keys = set()
    try:
        text = Path(manifest_path).read_text(errors="replace")
    except OSError:
        return keys
    for line in text.splitlines():
        if since and line[:len(since)] < since:
            continue
        if TOLERANT_RETRY_RE.match(line):
            continue
        if not (TOLERANT_OK_RE.match(line) or TOLERANT_ERR_RE.match(line)):
            continue
        m = TOLERANT_KEY_RE.match(line)
        if m:
            keys.add((m.group("ts"), m.group("worker"), m.group("phase")))
    return keys


def count_unsettled(req_dir, manifest_path, since, stale_after, now=None):
    """(inflight, stale) requests -- attempts the manifest has no outcome for.

    Neither is a failure. An unsettled request younger than ``stale_after``
    is running; an older one was almost certainly orphaned by a fleet
    restart. Counting either as a failure is the exact mistake that produced
    a "15.4%" against a true 0%.
    """
    now = time.time() if now is None else now
    keys = settled_keys(manifest_path, since)
    inflight = stale = 0
    try:
        entries = os.scandir(req_dir)
    except OSError:
        return 0, 0
    with entries:
        for entry in entries:
            name = entry.name
            if not name.endswith("-request.json"):
                continue
            if since and name[:len(since)] < since:
                continue
            m = REQUEST_FILE_RE.match(name)
            if not m:
                continue
            ts, worker, phase = m.group("ts"), m.group("worker"), m.group("phase")
            if worker is not None and (ts, worker, phase) in keys:
                continue
            if worker is None:
                # Legacy untagged filename: the manifest line carries a
                # worker the filename does not, so fall back to the response
                # artifact, which only a successful call ever writes.
                if Path(str(entry.path).replace("-request.json", "-response.txt")).exists():
                    continue
            try:
                started = time.mktime(time.strptime(ts, "%Y-%m-%dT%H:%M:%S"))
            except ValueError:
                continue
            if now - started > stale_after:
                stale += 1
            else:
                inflight += 1
    return inflight, stale


DURATION_RE = re.compile(r"^(?P<n>\d+(?:\.\d+)?)(?P<unit>[smhd]?)$")
DURATION_UNITS = {"s": 1, "m": 60, "h": 3600, "d": 86400, "": 60}


def local_cutoff(duration, now=None):
    """ISO cutoff for "the last <duration>", in LOCAL time.

    The manifests are stamped with `time.strftime` -- local time, no zone
    suffix -- and the window filter is a plain string prefix comparison. A
    cutoff built from `date -u` is therefore offset by the UTC offset: west
    of UTC it lands in the future and matches nothing, so a fully healthy
    fleet reports "no calls". Deriving the cutoff here, from the same clock
    the writer used, removes the chance to get it wrong.
    """
    m = DURATION_RE.match(duration.strip())
    if not m:
        raise ValueError(f"cannot parse duration {duration!r}; use 30m, 2h, 900s")
    seconds = float(m.group("n")) * DURATION_UNITS[m.group("unit")]
    now = time.time() if now is None else now
    return time.strftime(TS_FORMAT, time.localtime(now - seconds))


def quantiles(values):
    if not values:
        return None
    e = sorted(values)

    def q(f):
        return e[min(len(e) - 1, int(len(e) * f))]

    return {"median": q(.5), "p90": q(.9), "max": e[-1], "n": len(e)}


def render(counts, extra=None, top_errors=5, show_retry=True):
    """Lines describing one population. Returns (lines, ok_to_report)."""
    lines = []
    if counts.settled and counts.ok > 0:
        parts = [f"ok={counts.ok}", f"err={counts.err}"]
        if show_retry:
            parts.append(f"retry={counts.retry}")
        if extra:
            parts += [f"{k}={v}" for k, v in extra.items()]
        suffix = "(" + " ".join(parts) + ")"
        lines.append(
            f"{counts.name}: FAILURE RATE {counts.err}/{counts.settled} = "
            f"{counts.rate:.1f}%   {suffix}"
        )
        ok_to_report = True
    else:
        ok_to_report = False

    if counts.drift:
        lines.append(
            f"  WARNING: {counts.drift} line(s) matched the loose parser but not "
            f"the structured one -- manifest format is drifting; update "
            f"MANIFEST_ENTRY_RE in scripts/watch_parallel_fix.py"
        )
    stats = quantiles(counts.elapsed)
    if stats:
        lines.append(
            f"  latency: median={stats['median']:.0f}s p90={stats['p90']:.0f}s "
            f"max={stats['max']:.0f}s  n={stats['n']}"
        )
    for kind, n in sorted(counts.errkinds.items(), key=lambda kv: -kv[1])[:top_errors]:
        lines.append(f"  ERROR x{n}: {kind}")
    return lines, ok_to_report


def main(argv=None):
    # `python -OO` strips docstrings, leaving __doc__ as None.
    p = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    p.add_argument("since_pos", nargs="?", default=None, metavar="ISO-CUTOFF",
                   help="only count events at or after this ISO prefix, "
                        "e.g. 2026-07-30T16:05")
    p.add_argument("--since", default=None, help="same as the positional cutoff")
    p.add_argument("--last", default=None, metavar="DURATION",
                   help="window covering the last DURATION (30m, 2h, 900s). "
                        "Computes the cutoff in LOCAL time, matching how the "
                        "manifests are stamped -- prefer this over piping in "
                        "`date -u`, which lands the window hours in the "
                        "future and silently matches nothing")
    p.add_argument("--logs", default=str(DEFAULT_LOGS), type=Path,
                   help=f"log root (default {DEFAULT_LOGS})")
    p.add_argument("--stale-after", type=float, default=DEFAULT_STALE_AFTER_SECONDS,
                   help="seconds after which an unsettled request is counted "
                        "stale rather than in flight")
    p.add_argument("--json", action="store_true", help="machine-readable output")
    args = p.parse_args(argv)

    since = args.since or args.since_pos or ""
    if args.last:
        try:
            since = local_cutoff(args.last)
        except ValueError as e:
            p.error(str(e))
    req_manifest = args.logs / "model-fix-requests" / "manifest.log"
    req_dir = args.logs / "model-fix-requests"
    diff_manifest = args.logs / "model-fix-diffs" / "manifest.log"

    calls, calls_exist = read_model_calls(req_manifest, since)
    patches, patches_exist = read_patch_applies(diff_manifest, since)
    inflight, stale = count_unsettled(req_dir, req_manifest, since, args.stale_after)

    call_lines, call_ok = render(calls, {"inflight": inflight, "stale": stale})
    patch_lines, patch_ok = render(patches, show_retry=False)

    problems = []
    if not call_ok:
        problems.append(classify_ok_absence(calls, calls_exist))
    if not patch_ok:
        problems.append(classify_ok_absence(patches, patches_exist))

    if args.json:
        out = {
            "since": since or None,
            "now_local": time.strftime(TS_FORMAT),
            "model_calls": {
                "ok": calls.ok, "err": calls.err, "retry": calls.retry,
                "settled": calls.settled, "rate_pct": calls.rate,
                "inflight": inflight, "stale": stale,
                "drift_lines": calls.drift,
                "lines_in_window": calls.lines_in_window,
                "last_record": calls.last_ts,
                "latency": quantiles(calls.elapsed),
                "errors": calls.errkinds,
            },
            "patch_applies": {
                "applied": patches.ok, "rejected": patches.err,
                "settled": patches.settled, "rate_pct": patches.rate,
                "drift_lines": patches.drift,
                "lines_in_window": patches.lines_in_window,
                "last_record": patches.last_ts,
            },
            "problems": [{"status": s, "message": m} for s, m in problems],
        }
        print(json.dumps(out, indent=2, sort_keys=True))
    else:
        header = f"window: {'since ' + since if since else 'all history'}"
        header += f"   (manifest {calls.freshness()})"
        print(header)
        for line in call_lines + patch_lines:
            print(line)
        for status, message in problems:
            # The headline the previous monitor could never print. A rate is
            # withheld rather than fabricated, and the reason is named.
            print(f"{status}: {message}")

    # Non-zero whenever any population's rate is unknown, so a cron/alert
    # wrapper distinguishes "measured and healthy" from "could not measure".
    # A blind monitor must be as loud as a broken fleet.
    return 2 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
