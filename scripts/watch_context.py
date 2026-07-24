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

ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


def strip_ansi(text):
    """curses draws plain characters, not ANSI escape sequences -- the
    interactive dashboard reuses render_worker_detail's already-colored
    output (rather than a separate uncolored render path) and strips the
    color codes back out, so overview/--once mode keeps its color and
    the interactive mode doesn't show literal escape-code garbage."""
    return ANSI_RE.sub("", text)

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
    """entries (file order) -> {worker: {"fixer": entry_or_None, "reviewer": entry_or_None,
    "fixer_content": ..., "reviewer_content": ...}}.

    "{phase}" is the most recent entry of that phase for that worker (any
    status -- OK, RETRY, or ERROR, since an in-flight RETRY is itself
    useful "what's happening right now" context, not just completed
    calls). "{phase}_content" is the most recent NON-RETRY entry: RETRY
    manifest lines carry their own retry-event timestamp, which never
    matches a request/response file on disk (those are named for the
    original call's timestamp), so during a long retry storm the latest
    entry has status but no viewable content -- the _content entry is
    what render_worker_detail falls back to so the dashboard keeps
    showing the conversation instead of "request file missing"."""
    result = {}
    for entry in entries:
        worker = entry["worker"]
        phase = entry["phase"]
        result.setdefault(worker, {
            "fixer": None, "reviewer": None,
            "fixer_content": None, "reviewer_content": None,
        })
        result[worker][phase] = entry
        if entry["status"] != "RETRY":
            result[worker][f"{phase}_content"] = entry
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

    content_entry = entry
    messages = load_request_messages(req_log_dir, entry)
    if messages is None:
        # A RETRY entry's timestamp is the retry event, not the original
        # call -- no file exists under that name. Fall back to the last
        # completed call so a retry storm doesn't blank the whole view.
        fallback = calls.get(f"{phase}_content") if calls else None
        if fallback is not None and fallback["ts"] != entry["ts"]:
            fallback_messages = load_request_messages(req_log_dir, fallback)
            if fallback_messages is not None:
                content_entry = fallback
                messages = fallback_messages
                lines.append(
                    f"{YELLOW}Current call is {entry['status']} (no content under its own "
                    f"timestamp) -- showing the last completed {phase} call, "
                    f"{format_elapsed_ago(content_entry['ts'], now_fn=now_fn)}:{RESET}"
                )
                lines.append("")
    if messages is None:
        lines.append(f"{RED}Request file missing or unreadable: {request_path_for(req_log_dir, content_entry)}{RESET}")
    else:
        lines.append(f"{BOLD}=== SENT ({len(messages)} message(s)) ==={RESET}")
        for i, msg in enumerate(messages):
            lines.append(render_message(msg, i, term_width))
            lines.append("")

    response = load_response_text(req_log_dir, content_entry)
    lines.append(f"{BOLD}=== RECEIVED ==={RESET}")
    if response is None:
        lines.append(f"{DIM}(no response yet -- last call status is {entry['status']}){RESET}")
    else:
        lines.append(response)

    return "\n".join(lines)


def clamp_index(index, count):
    """Keep a worker-list index in [0, count-1] -- 0 if the list is empty
    (nothing to select, but never a negative or out-of-range index for
    the next render to index into)."""
    if count == 0:
        return 0
    return max(0, min(index, count - 1))


def compute_max_scroll(num_lines, viewport_height):
    """How far the view can scroll down before the last line reaches the
    top of the viewport -- never negative (a short body just doesn't
    scroll at all)."""
    return max(0, num_lines - viewport_height)


# Default key-code mapping for handle_navigation_key -- real curses
# constants (only resolvable once curses.initscr() has run) are passed
# in by run_interactive_dashboard; tests pass plain-int fakes instead so
# this logic is exercisable without a real terminal.
def default_curses_key_codes():
    import curses
    return {
        "left": curses.KEY_LEFT, "right": curses.KEY_RIGHT,
        "up": curses.KEY_UP, "down": curses.KEY_DOWN,
        "page_up": curses.KEY_PPAGE, "page_down": curses.KEY_NPAGE,
        "quit": ord("q"), "toggle_phase": ord("p"),
        "vim_up": ord("k"), "vim_down": ord("j"),
    }


def handle_navigation_key(key, state, key_codes, worker_count, page_size):
    """Pure state transition: given a keypress and the current UI state
    dict ({"worker_index", "scroll_offset", "phase"}), return the new
    state dict (scroll/worker-index re-clamped by the caller after
    re-rendering, since the new worker's content length isn't known
    here) and whether to quit. No I/O, no curses import required at
    call time -- key_codes lets tests exercise every branch with plain
    integers instead of needing a real curses session.
    """
    worker_index = state["worker_index"]
    scroll_offset = state["scroll_offset"]
    phase = state["phase"]

    if key == key_codes["quit"]:
        return state, True
    if key == key_codes["left"]:
        worker_index = clamp_index(worker_index - 1, worker_count)
        scroll_offset = 0
    elif key == key_codes["right"]:
        worker_index = clamp_index(worker_index + 1, worker_count)
        scroll_offset = 0
    elif key in (key_codes["up"], key_codes["vim_up"]):
        scroll_offset = max(0, scroll_offset - 1)
    elif key in (key_codes["down"], key_codes["vim_down"]):
        scroll_offset += 1
    elif key == key_codes["page_up"]:
        scroll_offset = max(0, scroll_offset - page_size)
    elif key == key_codes["page_down"]:
        scroll_offset += page_size
    elif key == key_codes["toggle_phase"]:
        phase = "reviewer" if phase == "fixer" else "fixer"
        scroll_offset = 0

    return {"worker_index": worker_index, "scroll_offset": scroll_offset, "phase": phase}, False


def render_interactive_frame(state, latest_by_worker, req_log_dir, width, height, now_fn=time.time):
    """Build the full plain-text frame (header + visible slice of the
    selected worker's detail view) for one redraw. Pure given its
    inputs -- no curses calls -- so it's directly testable;
    run_interactive_dashboard is the thin curses-I/O wrapper around it.
    Returns (lines_to_draw, max_scroll).
    """
    workers = sorted(latest_by_worker)
    if not workers:
        return ["No model calls logged yet."], 0

    worker_index = clamp_index(state["worker_index"], len(workers))
    worker = workers[worker_index]
    calls = latest_by_worker[worker]
    body = strip_ansi(render_worker_detail(worker, calls, req_log_dir, state["phase"], width, now_fn=now_fn))
    body_lines = body.split("\n")
    viewport_height = max(1, height - 1)  # row 0 reserved for the header
    max_scroll = compute_max_scroll(len(body_lines), viewport_height)
    scroll_offset = max(0, min(state["scroll_offset"], max_scroll))

    header = (
        f"[{worker_index + 1}/{len(workers)}] {worker} ({state['phase']})  "
        "←/→ switch worker  ↑/↓ scroll  PgUp/PgDn  p toggle phase  q quit"
    )
    visible = body_lines[scroll_offset:scroll_offset + viewport_height]
    return [header] + visible, max_scroll


def run_interactive_dashboard(stdscr, req_log_dir, refresh_interval=1.0, now_fn=time.time,
                               initial_worker=None, initial_phase="fixer"):
    """curses main loop: redraws on a timer (so the view stays live even
    with no keypress) and reacts instantly to arrow/page/phase/quit keys
    in between. All actual state transitions and rendering happen in
    handle_navigation_key/render_interactive_frame -- this function is
    just the curses glue (screen setup, the input/redraw loop, catching
    the harmless "wrote to the bottom-right cell" error curses raises
    on some terminals).
    """
    import curses

    curses.curs_set(0)
    stdscr.nodelay(True)
    stdscr.timeout(int(refresh_interval * 1000))
    key_codes = default_curses_key_codes()

    manifest_path = req_log_dir / "manifest.log"
    state = {"worker_index": 0, "scroll_offset": 0, "phase": initial_phase}
    if initial_worker:
        entries = load_manifest_entries(manifest_path)
        workers = sorted(latest_calls_per_worker(entries))
        if initial_worker in workers:
            state["worker_index"] = workers.index(initial_worker)

    while True:
        entries = load_manifest_entries(manifest_path)
        latest_by_worker = latest_calls_per_worker(entries)
        height, width = stdscr.getmaxyx()

        lines, max_scroll = render_interactive_frame(state, latest_by_worker, req_log_dir, width, height, now_fn=now_fn)
        state["scroll_offset"] = max(0, min(state["scroll_offset"], max_scroll))

        stdscr.erase()
        for row, line in enumerate(lines[:height]):
            try:
                stdscr.addstr(row, 0, line[:max(0, width - 1)])
            except curses.error:
                pass  # bottom-right-cell write; curses quirk, not a real error
        stdscr.refresh()

        key = stdscr.getch()
        if key == -1:
            continue  # no key pressed within the timeout -- just redraw (picks up new log data)
        worker_count = len(latest_by_worker)
        page_size = max(1, height - 1)
        state, should_quit = handle_navigation_key(key, state, key_codes, worker_count, page_size)
        if should_quit:
            return


def main(argv=None, stdout=sys.stdout, now_fn=time.time):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "--req-log-dir", default=str(DEFAULT_REQ_LOG_DIR),
        help=f"model_fix_loop.py's shared request/response log directory. Default: {DEFAULT_REQ_LOG_DIR}",
    )
    parser.add_argument(
        "--worker", default=None,
        help="Start pre-selected on this worker (worker id matches --worker-id, typically the "
             "format name, e.g. JPEG, RW2) -- left/right arrow keys still switch to any other "
             "worker from there. With --once, shows only this worker's detail and exits.",
    )
    parser.add_argument(
        "--phase", default="fixer", choices=["fixer", "reviewer"],
        help="Which phase's latest call to show initially (default: fixer; 'p' toggles it live).",
    )
    parser.add_argument("--interval", type=float, default=1.0, help="Redraw interval in seconds")
    parser.add_argument(
        "--once", action="store_true",
        help="Render one plain-text frame and exit, instead of the interactive dashboard -- for "
             "scripting/piping output rather than an interactive terminal session.",
    )
    args = parser.parse_args(argv)

    req_log_dir = Path(args.req_log_dir)
    manifest_path = req_log_dir / "manifest.log"

    if args.once:
        entries = load_manifest_entries(manifest_path)
        latest_by_worker = latest_calls_per_worker(entries)
        term_width = shutil.get_terminal_size(fallback=(100, 24)).columns
        if args.worker:
            calls = latest_by_worker.get(args.worker)
            body = render_worker_detail(args.worker, calls, req_log_dir, args.phase, term_width, now_fn=now_fn)
        else:
            body = render_overview(latest_by_worker, term_width, now_fn=now_fn)
        stdout.write("\x1b[2J\x1b[H")
        stdout.write(body + "\n")
        stdout.flush()
        return 0

    import curses
    try:
        curses.wrapper(
            lambda stdscr: run_interactive_dashboard(
                stdscr, req_log_dir, refresh_interval=args.interval, now_fn=now_fn,
                initial_worker=args.worker, initial_phase=args.phase,
            )
        )
    except KeyboardInterrupt:
        pass  # Ctrl-C is the normal way to leave the live dashboard, not an error
    return 0


if __name__ == "__main__":
    sys.exit(main())
