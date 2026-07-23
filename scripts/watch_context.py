#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Live dashboard showing the full model context sent to, and received
from, each model_fix_loop.py worker.

model_fix_loop.py's main() (see make_logging_call_model) already writes
every fixer/reviewer call's full request (including the complete
messages list -- the whole prompt, plus any REQUEST:-protocol follow-up
turns) and full response to req_log_dir, and appends a one-line summary
to manifest.log. This dashboard reads only those files -- it never
touches worktrees, git, or the model API -- so it's safe to run
alongside an in-flight parallel run (see watch_parallel_fix.py, which
this shares conventions with but not code: that dashboard shows
build/test/review *outcomes* per round; this one shows the actual
prompt/response *content* per call).

Two modes:

  Overview (default): one row per worker, its most recent fixer and
  reviewer call (timestamp, model, prompt/reply size, elapsed time) --
  enough to see who's doing what without drowning in prompt text.

  Detail (--worker <ID>): the FULL latest prompt (every message in the
  conversation, including REQUEST:-protocol turns) and FULL latest
  response for one worker. This is what "full context sent and
  received" actually means for a single worker -- showing every
  worker's full text at once would be unreadable in any terminal.

Usage:
    uv run scripts/watch_context.py
    uv run scripts/watch_context.py --worker JPEG
    uv run scripts/watch_context.py --worker JPEG --phase reviewer
    uv run scripts/watch_context.py --interval 2
"""
import argparse
import json
import os
import re
import shutil
import sys
import time
from pathlib import Path

RESET = "\x1b[0m"
BOLD = "\x1b[1m"
DIM = "\x1b[2m"
GREEN = "\x1b[32m"
RED = "\x1b[31m"
YELLOW = "\x1b[33m"
CYAN = "\x1b[36m"
BRIGHT_CYAN = "\x1b[96m"
BRIGHT_WHITE = "\x1b[97m"

OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", str(Path.home() / ".oxidex")))
DEFAULT_REQ_LOG_DIR = OXIDEX_HOME / "logs" / "model-fix-requests"

# Matches both shapes manifest.log's make_logging_call_model writes:
#   "{ts} phase={phase} worker={worker} model={model} prompt_chars={n} elapsed={n}s reply_chars={n} OK"
#   "{ts} phase={phase} worker={worker} model={model} RETRY {msg}"
#   "{ts} phase={phase} worker={worker} model={model} prompt_chars={n} elapsed={n}s ERROR={msg}"
MANIFEST_LINE_RE = re.compile(
    r"^(?P<ts>\S+) phase=(?P<phase>\S+) worker=(?P<worker>\S+) model=(?P<model>\S+) "
    r"(?:(?P<status>RETRY) (?P<detail>.*)|"
    r"prompt_chars=(?P<prompt_chars>\d+) elapsed=(?P<elapsed>[\d.]+)s "
    r"(?:reply_chars=(?P<reply_chars>\d+) (?P<ok_status>OK)|(?P<err_status>ERROR)=(?P<err_detail>.*)))$"
)


def parse_manifest_line(line):
    """One manifest.log line -> dict, or None if it doesn't match (e.g.
    a stray blank line or a format this dashboard doesn't know about --
    never fatal, just skipped)."""
    match = MANIFEST_LINE_RE.match(line.strip())
    if not match:
        return None
    d = match.groupdict()
    if d["status"] == "RETRY":
        return {
            "ts": d["ts"], "phase": d["phase"], "worker": d["worker"], "model": d["model"],
            "status": "RETRY", "detail": d["detail"],
        }
    if d["ok_status"] == "OK":
        return {
            "ts": d["ts"], "phase": d["phase"], "worker": d["worker"], "model": d["model"],
            "status": "OK", "prompt_chars": int(d["prompt_chars"]), "elapsed": float(d["elapsed"]),
            "reply_chars": int(d["reply_chars"]),
        }
    return {
        "ts": d["ts"], "phase": d["phase"], "worker": d["worker"], "model": d["model"],
        "status": "ERROR", "prompt_chars": int(d["prompt_chars"]), "elapsed": float(d["elapsed"]),
        "detail": d["err_detail"],
    }


def load_manifest_entries(manifest_path):
    """All parseable lines from manifest.log, file order (oldest first).
    Missing file (no calls logged yet) returns []."""
    if not manifest_path.exists():
        return []
    entries = []
    try:
        with manifest_path.open() as f:
            for line in f:
                entry = parse_manifest_line(line)
                if entry:
                    entries.append(entry)
    except OSError:
        return []
    return entries


def latest_calls_per_worker(entries):
    """entries (file order) -> {worker: {"fixer": entry_or_None, "reviewer": entry_or_None}},
    each the most recent entry of that phase for that worker (any status --
    OK, RETRY, or ERROR, since an in-flight RETRY is itself useful "what's
    happening right now" context, not just completed calls)."""
    result = {}
    for entry in entries:
        worker = entry["worker"]
        phase = entry["phase"]
        result.setdefault(worker, {"fixer": None, "reviewer": None})
        result[worker][phase] = entry
    return result


def request_path_for(req_log_dir, entry):
    return req_log_dir / f"{entry['ts']}-{entry['phase']}-request.json"


def response_path_for(req_log_dir, entry):
    return req_log_dir / f"{entry['ts']}-{entry['phase']}-response.txt"


def load_request_messages(req_log_dir, entry):
    """The full messages list from this call's request JSON, or None if
    the file is missing/unreadable (e.g. log rotation, or the call is
    RETRY/ERROR-status with no successful request ever recorded under
    this exact timestamp -- shouldn't normally happen since the request
    is written before the call attempt, but never fatal either way)."""
    path = request_path_for(req_log_dir, entry)
    try:
        return json.loads(path.read_text()).get("messages", [])
    except (OSError, json.JSONDecodeError):
        return None


def load_response_text(req_log_dir, entry):
    """The full raw response text for this call, or None if this call
    never completed (RETRY/ERROR status -- no response file exists yet)."""
    path = response_path_for(req_log_dir, entry)
    try:
        return path.read_text()
    except OSError:
        return None


def format_elapsed_ago(ts_str, now_fn=time.time):
    """'\''3m ago'\'' style relative time from a manifest timestamp
    ("%Y-%m-%dT%H:%M:%S", local time, matching timestamped_log's own format)."""
    try:
        then = time.mktime(time.strptime(ts_str, "%Y-%m-%dT%H:%M:%S"))
    except ValueError:
        return "?"
    delta = max(0, now_fn() - then)
    if delta < 60:
        return f"{int(delta)}s ago"
    if delta < 3600:
        return f"{int(delta // 60)}m ago"
    return f"{int(delta // 3600)}h{int((delta % 3600) // 60)}m ago"


def status_color(status):
    return {"OK": GREEN, "RETRY": YELLOW, "ERROR": RED}.get(status, DIM)


def render_call_summary(entry, now_fn=time.time):
    if entry is None:
        return f"{DIM}(none yet){RESET}"
    color = status_color(entry["status"])
    ago = format_elapsed_ago(entry["ts"], now_fn=now_fn)
    if entry["status"] == "OK":
        return (
            f"{color}{entry['status']}{RESET} {entry['model']} "
            f"prompt={entry['prompt_chars']:,}c reply={entry['reply_chars']:,}c "
            f"{entry['elapsed']:.1f}s {DIM}({ago}){RESET}"
        )
    if entry["status"] == "RETRY":
        return f"{color}{entry['status']}{RESET} {entry['model']} {DIM}{entry['detail'][:60]} ({ago}){RESET}"
    return f"{color}{entry['status']}{RESET} {entry['model']} {DIM}{entry.get('detail', '')[:60]} ({ago}){RESET}"


def render_overview(latest_by_worker, term_width, now_fn=time.time):
    if not latest_by_worker:
        return f"{DIM}No model calls logged yet.{RESET}"
    lines = [f"{BOLD}{BRIGHT_WHITE}{'WORKER':<12} {'FIXER':<55} REVIEWER{RESET}"]
    lines.append(DIM + "-" * min(term_width, 110) + RESET)
    for worker in sorted(latest_by_worker):
        calls = latest_by_worker[worker]
        fixer_str = render_call_summary(calls.get("fixer"), now_fn=now_fn)
        reviewer_str = render_call_summary(calls.get("reviewer"), now_fn=now_fn)
        lines.append(f"{BRIGHT_CYAN}{worker:<12}{RESET} {fixer_str:<55} {reviewer_str}")
    lines.append("")
    lines.append(f"{DIM}Run with --worker <ID> to see that worker's full prompt + response.{RESET}")
    return "\n".join(lines)


def render_message(msg, index, term_width):
    role = msg.get("role", "?")
    content = msg.get("content", "")
    role_color = CYAN if role == "user" else GREEN
    header = f"{BOLD}{role_color}[{index}] {role.upper()} ({len(content):,} chars){RESET}"
    return f"{header}\n{DIM}{'-' * min(term_width, 80)}{RESET}\n{content}"


def render_worker_detail(worker, calls, req_log_dir, phase, term_width, now_fn=time.time):
    entry = calls.get(phase) if calls else None
    if entry is None:
        return f"{RED}No {phase} call logged yet for worker {worker!r}.{RESET}"

    lines = [
        f"{BOLD}{BRIGHT_WHITE}Worker {worker} -- {phase} -- {entry['status']} -- "
        f"{entry['model']} -- {format_elapsed_ago(entry['ts'], now_fn=now_fn)}{RESET}",
        "",
    ]

    messages = load_request_messages(req_log_dir, entry)
    if messages is None:
        lines.append(f"{RED}Request file missing or unreadable: {request_path_for(req_log_dir, entry)}{RESET}")
    else:
        lines.append(f"{BOLD}=== SENT ({len(messages)} message(s)) ==={RESET}")
        for i, msg in enumerate(messages):
            lines.append(render_message(msg, i, term_width))
            lines.append("")

    response = load_response_text(req_log_dir, entry)
    lines.append(f"{BOLD}=== RECEIVED ==={RESET}")
    if response is None:
        lines.append(f"{DIM}(no response yet -- last call status is {entry['status']}){RESET}")
    else:
        lines.append(response)

    return "\n".join(lines)


def main(argv=None, sleep_fn=time.sleep, stdout=sys.stdout, now_fn=time.time):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "--req-log-dir", default=str(DEFAULT_REQ_LOG_DIR),
        help=f"model_fix_loop.py's shared request/response log directory. Default: {DEFAULT_REQ_LOG_DIR}",
    )
    parser.add_argument(
        "--worker", default=None,
        help="Show this one worker's full latest prompt + response instead of the overview table "
             "(worker id matches --worker-id, typically the format name, e.g. JPEG, RW2).",
    )
    parser.add_argument(
        "--phase", default="fixer", choices=["fixer", "reviewer"],
        help="Which phase's latest call to show in --worker detail mode (default: fixer).",
    )
    parser.add_argument("--interval", type=float, default=1.0, help="Redraw interval in seconds")
    parser.add_argument("--once", action="store_true", help="Render once and exit, instead of looping")
    args = parser.parse_args(argv)

    req_log_dir = Path(args.req_log_dir)
    manifest_path = req_log_dir / "manifest.log"

    try:
        while True:
            entries = load_manifest_entries(manifest_path)
            latest_by_worker = latest_calls_per_worker(entries)
            term_width = shutil.get_terminal_size(fallback=(100, 24)).columns

            if args.worker:
                calls = latest_by_worker.get(args.worker)
                body = render_worker_detail(args.worker, calls, req_log_dir, args.phase, term_width, now_fn=now_fn)
            else:
                body = render_overview(latest_by_worker, term_width, now_fn=now_fn)

            stdout.write("\x1b[2J\x1b[H")  # clear screen, cursor home
            stdout.write(body + "\n")
            stdout.flush()

            if args.once:
                return 0
            sleep_fn(args.interval)
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    sys.exit(main())
