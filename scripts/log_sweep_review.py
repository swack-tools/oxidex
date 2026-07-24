#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Append one verdict to the shared sweep-review-history log.

The tag-fix loop's own per-tag attempt history (run_tag_loop's persisted
state, see model_fix_loop.py's format_previous_attempts) only ever
captures a build/test *failure* on the exact same tag being retried. It
has no way to represent "this diff built, tested, and got merged into the
PR sweep -- but a human reviewer later found it wrong" (wrong tag name,
duplicate of existing code, formatting regression) or "this was correctly
accepted, here's why" -- both of which are exactly the kind of judgment
calls a fixer repeats blindly without ever seeing.

This script is the write side of that gap: whoever is doing sweep review
(a human, or an agent doing the review) calls this once per commit
reviewed, and model_fix_loop.py's build_prompt reads recent entries back
in (see load_recent_sweep_reviews/format_sweep_review_history) so the
next round targeting the same format sees actual, specific outcomes --
not just generic static advice.

Usage:
    uv run scripts/log_sweep_review.py --format RW2 --tag IFD0:BlackLevelBlue \\
        --verdict accepted --reason "Matches this codebase's IFD0: convention"

    uv run scripts/log_sweep_review.py --format XMP --tag XMP:AboutCvTermCvId \\
        --verdict rejected --reason "Guessed [1,2] JSON-array syntax; real \\
        exiftool text output is comma-space-separated: 1, 2" --commit e77bffe
"""
import argparse
import json
import os
import sys
import time
from pathlib import Path

OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", str(Path.home() / ".oxidex")))
DEFAULT_LOG_PATH = OXIDEX_HOME / "logs" / "sweep-review-history.jsonl"
DEFAULT_LANDED_LOG_PATH = OXIDEX_HOME / "logs" / "landed-tags.log"


def append_sweep_review(log_path, format_name, tag, verdict, reason, commit=None, now_fn=time.time,
                        landed_log_path=None):
    """Append one JSON line. Appends are small single lines (well under
    PIPE_BUF), so this is safe without extra locking even with concurrent
    writers -- same reasoning as model_fix_loop.py's log_tag_found.

    An accepted verdict additionally appends "<iso-ts> <format>:<tag>" to
    landed_log_path (when given) -- the landed-tags set model_fix_loop.py's
    run_tag_loop reads back so workers stop re-deriving already-merged
    fixes."""
    if verdict not in ("accepted", "rejected"):
        raise ValueError(f"verdict must be 'accepted' or 'rejected', got {verdict!r}")
    entry = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(now_fn())),
        "format": format_name,
        "tag": tag,
        "verdict": verdict,
        "reason": reason,
        "commit": commit,
    }
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("a") as f:
        f.write(json.dumps(entry) + "\n")
    if verdict == "accepted" and landed_log_path:
        landed_log_path = Path(landed_log_path)
        landed_log_path.parent.mkdir(parents=True, exist_ok=True)
        with landed_log_path.open("a") as f:
            f.write(f"{entry['timestamp']} {format_name}:{tag}\n")
    return entry


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--format", required=True, help="Format name, e.g. RW2, JPEG")
    parser.add_argument("--tag", required=True, help="Full tag key, e.g. IFD0:BlackLevelBlue")
    parser.add_argument("--verdict", required=True, choices=["accepted", "rejected"])
    parser.add_argument("--reason", required=True, help="Why -- this is what future rounds will read")
    parser.add_argument("--commit", default=None, help="Short SHA, if applicable")
    parser.add_argument("--log-path", default=str(DEFAULT_LOG_PATH))
    parser.add_argument(
        "--landed-log", default=str(DEFAULT_LANDED_LOG_PATH),
        help="Landed-tags log appended to on accepted verdicts -- the skip "
             "set model_fix_loop.py workers re-read every round",
    )
    args = parser.parse_args(argv)

    entry = append_sweep_review(
        Path(args.log_path), args.format, args.tag, args.verdict, args.reason, args.commit,
        landed_log_path=Path(args.landed_log),
    )
    print(f"logged: {entry['format']} {entry['tag']} {entry['verdict']} -> {args.log_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
