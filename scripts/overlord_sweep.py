#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Overlord sweep runbook -- spec M4 of
docs/plans/specs/2026-07-24-fleet-knowledge-and-scaling-design.md.

The human-driven session runs this every ~5 minutes (honest cadence: 5
min for non-JPEG-touching sweeps, up to ~15 min when JPEG rechecks and a
workspace test are in the window). One pass (``run_sweep``) does, in
order:

1. **Preflight** (``preflight``): report -- never acquire -- the health
   of every singleton lock the sweep depends on (the dispatcher, and
   every squad's merger), via the same lock-file shape
   distill_lessons.py's ``acquire_lock`` reads (``{pid, script_git_sha,
   heartbeat_ts}``). A stale heartbeat means that holder crashed without
   releasing; this is informational (logged loudly), not a hard stop.
2. **Collect green stamps** (``collect_green_stamps``): every squad's
   ``squad-status/<squad>.json`` (squad_merge_loop.py's own atomic-read/
   parse-failure-as-no-news reader, reused directly) newer than a
   persisted ``sweep-state.json`` cursor.
3. **Cut a fresh branch** (``next_sweep_branch_name`` /
   ``cut_fresh_sweep_branch``): ``sweep/tags-<date>-<n>`` from
   origin/main, NEVER reused -- ``n`` increments past every existing
   local/remote ref for today's date.
4. **Merge each green squad head at its stamped SHA**
   (``merge_squad_into_sweep``): fast-forward when the sweep tip still
   allows it (only ever the first squad processed against a freshly cut
   branch), a controlled ``--no-ff`` merge otherwise. A cross-squad
   conflict is a hard error for JUST that squad -- logged loudly, that
   squad contributes nothing this round, never silently skipped.
5. **Post-merge semantic recheck** (``run_post_merge_recheck`` /
   ``evaluate_post_merge``) for the union of touched formats: measured
   gap delta >= Sigma Verified trailers (over-delivery is bonus yield,
   never a failure), ``duplicate_emissions`` empty, ``new_oxidex_only``
   empty. A failure triggers mechanical bisection
   (``bisect_sweep_failure``): revert one squad's contribution at a
   time until the recheck clears; the isolated offender's commits are
   quarantined by patch-id in the SAME ``quarantine.jsonl`` squad
   mergers already consult (so it cannot re-enter -- no livelock).
6. One ``cargo test --workspace`` on the final sweep branch
   (``model_fix_loop.cargo_test_workspace``).
7. Auto-log ``machine_accepted`` sweep-review entries for every merged
   commit (``log_sweep_review.append_from_commits``).
7b. ``cargo fmt --all`` over the assembled branch, committed separately
   when (and only when) it changes something (``format_sweep_branch``,
   injectable -- ``fmt_fn``). Nothing upstream of here ever style-checks
   worker-authored Rust, and CI's "Lint & Audit" job runs ``cargo fmt
   --all -- --check``, so without this every sweep PR fails CI by
   construction -- measured on PR #124 (CI run 30186389305: Build & Test
   green, Lint & Audit red on exactly that step).
8. ``gh pr create`` (injectable -- ``create_pr_fn``), PR body carrying a
   per-tag evidence table (Tag / Exiftool-Value / Oxidex-Value / Sample
   count) parsed from the merged commits' trailers, plus a
   judgment-queue section (spec M4(e)): commits touching value-map/
   PrintConv-like tables, a new file or top-level ``fn parse_``,
   tests/fixtures, a reviewer ``UNVERIFIABLE`` outcome, or a commons
   file (``src/core/format_dispatch.rs``,
   ``src/parsers/tiff/makernotes/shared/``) are flagged for human
   review rather than silently auto-shipped.

Everything side-effectful is injectable (``comparison_fn``,
``checkout_fn``, ``cargo_test_workspace_fn``, ``create_pr_fn``,
``run_git``, ``now_fn``) -- hermetic tests exercise the git mechanics
(green-stamp collection, branch cutting, merge/revert/bisection) against
real tempdir git repos, matching test_squad_merge_loop.py's style, and
inject fakes for anything that would need a real cargo build, network,
or ``gh``.

Usage:
    uv run scripts/overlord_sweep.py
    uv run scripts/overlord_sweep.py --repo ~/.oxidex/worktrees/overlord-sweep
"""
import argparse
import json
import os
import re
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

from find_tag_gaps import OXIDEX_HOME, REPO_ROOT
from attribute_gaps import write_atomic as write_json_atomic
import squad_merge_loop
import validate_fix_commit
import log_sweep_review
from model_fix_loop import cargo_test_workspace as _real_cargo_test_workspace
from model_fix_loop import new_oxidex_only_keys
from distill_lessons import STALE_HEARTBEAT_SECONDS

SCRIPTS_DIR = Path(__file__).resolve().parent
DEFAULT_SQUADS_TOML = SCRIPTS_DIR / "squads.toml"
DEFAULT_SWEEP_STATE_PATH = OXIDEX_HOME / "logs" / "sweep-state.json"

ORIGIN_MAIN = "origin/main"

# spec M4(e) judgment-queue classification: files no commit may touch
# and still auto-ship.
COMMONS_FILES = {"src/core/format_dispatch.rs"}
COMMONS_PREFIXES = ("src/parsers/tiff/makernotes/shared/",)

_NEW_FILE_RE = re.compile(r"^new file mode")
_NEW_PARSE_FN_RE = re.compile(r"^\+\s*(?:pub(?:\(\w+\))?\s+)?fn\s+parse_\w+")
_VERIFIED_DELTA_RE = re.compile(r"gaps=(\d+)->(\d+)")


# ---------------------------------------------------------------------------
# Git plumbing (list-argv only, no shell=True; matches
# validate_fix_commit.run_git's exact (args, repo, input_text=None) ->
# (returncode, stdout, stderr) signature so validate_fix_commit's own
# helpers -- commit_message/parse_trailers/commit_diff/
# commit_changed_files/extract_added_map_values -- are directly reusable
# without a second parser)
# ---------------------------------------------------------------------------

def default_run_git(args, repo, input_text=None):
    result = subprocess.run(  # nosec B603
        ["git", *args], cwd=repo, input=input_text, capture_output=True, text=True,
    )
    return result.returncode, result.stdout, result.stderr


def head_sha(repo_root, run_git):
    rc, out, _err = run_git(["rev-parse", "HEAD"], repo_root)
    return out.strip() if rc == 0 else None


def commits_in_range(repo_root, base_ref, head_ref, run_git):
    """Commit shas reachable from head_ref but not base_ref, oldest
    first. Empty list (never raises) when git can't answer."""
    rc, out, _err = run_git(["log", f"{base_ref}..{head_ref}", "--format=%H", "--reverse"], repo_root)
    if rc != 0:
        return []
    return [line for line in out.splitlines() if line]


# ---------------------------------------------------------------------------
# Step 1: preflight (report lock health, never acquire)
# ---------------------------------------------------------------------------

def _pid_alive(pid):
    """Best-effort liveness probe for a bare-pid lock file, which
    carries no heartbeat concept at all: ``os.kill(pid, 0)`` sends no
    signal, it only asks whether `pid` is addressable. A
    ``PermissionError`` means the pid exists but belongs to a different
    user -- still alive, for this check's purposes."""
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except (PermissionError, OSError):
        return True
    return True


def _lock_health(lock_path, now_fn=time.time, stale_seconds=STALE_HEARTBEAT_SECONDS, alive_fn=None):
    """{"exists", "stale", "pid"} for one singleton lock file -- read
    only. Two on-disk shapes are handled: the {pid, script_git_sha,
    heartbeat_ts} JSON every squad merger writes (distill_lessons.
    acquire_lock/write_lock), staleness = heartbeat age; and the bare
    "<pid>\\n" text parallel_model_fix_loop.acquire_dispatcher_lock
    writes (an flock-backed singleton with no heartbeat_ts at all --
    ``json.loads("<pid>\\n")`` parses fine as a bare int, so this is
    detected by shape, not by which lock_path was passed). Staleness for
    the bare-pid shape means the recorded holder pid is no longer alive:
    a live flock holder would have refused a second acquirer, so a
    readable, still-alive pid here is by construction the current
    holder. A missing lock is not itself unhealthy (nothing currently
    running); an unreadable/corrupt lock is treated as stale (a live,
    healthy holder always writes valid JSON, so anything else is a
    crash-mid-write signal)."""
    alive_fn = alive_fn or _pid_alive
    lock_path = Path(lock_path)
    if not lock_path.exists():
        return {"exists": False, "stale": False, "pid": None}
    try:
        info = json.loads(lock_path.read_text())
    except (OSError, ValueError):
        return {"exists": True, "stale": True, "pid": None}
    if isinstance(info, bool):
        return {"exists": True, "stale": True, "pid": None}
    if isinstance(info, int):
        return {"exists": True, "stale": not alive_fn(info), "pid": info}
    if not isinstance(info, dict):
        return {"exists": True, "stale": True, "pid": None}
    heartbeat = info.get("heartbeat_ts")
    fresh = isinstance(heartbeat, (int, float)) and (now_fn() - heartbeat) < stale_seconds
    return {"exists": True, "stale": not fresh, "pid": info.get("pid")}


def preflight(home, squads, dispatcher_lock_path=None, now_fn=time.time,
             stale_seconds=STALE_HEARTBEAT_SECONDS, dispatcher_alive_fn=None):
    """spec M4 step 1: singleton-lock health for the dispatcher and
    every squad's merger -- reported, never acquired (this process
    never tries to take any of these locks). "ok" is False only when at
    least one PRESENT lock is stale (a holder that crashed without
    releasing); a squad with no merger currently running at all is
    normal, not a preflight failure."""
    if dispatcher_lock_path is None:
        from parallel_model_fix_loop import DISPATCHER_LOCK_PATH
        dispatcher_lock_path = DISPATCHER_LOCK_PATH
    dispatcher = _lock_health(dispatcher_lock_path, now_fn, stale_seconds, alive_fn=dispatcher_alive_fn)
    mergers = {
        squad: _lock_health(squad_merge_loop.merger_lock_path(home, squad), now_fn, stale_seconds)
        for squad in squads
    }
    stale = [squad for squad, health in mergers.items() if health["exists"] and health["stale"]]
    if dispatcher["exists"] and dispatcher["stale"]:
        stale = ["dispatcher"] + stale
    return {"dispatcher": dispatcher, "mergers": mergers, "ok": not stale, "stale": stale}


# ---------------------------------------------------------------------------
# Step 2: green-stamp collection (sweep-state.json cursor)
# ---------------------------------------------------------------------------

def squads_from_toml(squads_toml_path):
    with open(squads_toml_path, "rb") as f:
        data = tomllib.load(f)
    return list((data.get("squads") or {}).keys())


def load_sweep_state(path):
    """{"squads": {squad: {"last_ts", "last_squad_sha"}}} -- a missing
    file or a parse failure both read as "no news yet" (never raise),
    same discipline as squad_merge_loop.load_squad_status/
    load_batch_state."""
    path = Path(path)
    if not path.exists():
        return {"squads": {}}
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError):
        return {"squads": {}}
    if not isinstance(data, dict) or not isinstance(data.get("squads"), dict):
        return {"squads": {}}
    return data


def save_sweep_state(path, data):
    write_json_atomic(path, data)


def collect_green_stamps(home, squads, cursor):
    """spec M4 step 2: for every squad, the newest squad-status head
    entry (status="consumed", work_done, carrying a squad_sha) whose ts
    is newer than that squad's cursor entry. Reuses
    squad_merge_loop.load_squad_status directly (its own tempfile+
    os.replace-written file, atomic read, parse failure = "no news")
    rather than a second reader.

    The cursor advances for EVERY squad with news collected this round,
    independent of what later happens to it (a clean merge, a hard
    conflict, or a bisected-out offender) -- once a stamp is looked at,
    it never resurfaces as "news" again, which is what keeps a
    conflicting or quarantined squad from re-triggering the identical
    failure on every subsequent poll (the same "cannot re-enter -- no
    livelock" property spec M4 states for quarantined patch-ids).

    Returns (stamps, new_cursor): stamps is
    {squad: {"squad_sha", "ts", "formats": [...]}}.
    """
    squads_cursor = dict(cursor.get("squads") or {})
    new_cursor = {"squads": dict(squads_cursor)}
    stamps = {}
    for squad in squads:
        status = squad_merge_loop.load_squad_status(squad_merge_loop.squad_status_file(home, squad))
        heads = status.get("heads") or {}
        last_ts = (squads_cursor.get(squad) or {}).get("last_ts") or ""
        consumed = [
            (sha, entry) for sha, entry in heads.items()
            if entry.get("status") == "consumed" and entry.get("work_done") and entry.get("squad_sha")
            and (entry.get("ts") or "") > last_ts
        ]
        if not consumed:
            continue
        consumed.sort(key=lambda kv: kv[1].get("ts") or "")
        _newest_sha, newest_entry = consumed[-1]
        formats = sorted({entry.get("format") for _sha, entry in consumed if entry.get("format")})
        stamps[squad] = {
            "squad_sha": newest_entry["squad_sha"], "ts": newest_entry.get("ts"), "formats": formats,
        }
        new_cursor["squads"][squad] = {
            "last_ts": newest_entry.get("ts"), "last_squad_sha": newest_entry["squad_sha"],
        }
    return stamps, new_cursor


# ---------------------------------------------------------------------------
# Step 3: fresh sweep branch, never reused
# ---------------------------------------------------------------------------

def _existing_sweep_branch_numbers(repo_root, date_str, run_git):
    numbers = []
    for pattern in (
        f"refs/heads/sweep/tags-{date_str}-*",
        f"refs/remotes/origin/sweep/tags-{date_str}-*",
    ):
        rc, out, _err = run_git(["for-each-ref", "--format=%(refname)", pattern], repo_root)
        if rc != 0:
            continue
        for line in out.splitlines():
            suffix = line.strip().rsplit("-", 1)[-1]
            if suffix.isdigit():
                numbers.append(int(suffix))
    return numbers


def next_sweep_branch_name(repo_root, run_git, date_str=None, now_fn=None):
    """spec M4 step 3: ``sweep/tags-<date>-<n>``, NEVER reused -- scans
    both local and remote-tracking refs for the highest existing <n>
    for today's date and increments (starting at 1 if none exist yet).
    This is the ends-the-pr40-force-push-spiral rule: every sweep gets
    a brand new branch name, so nothing about a prior sweep's ref state
    is ever reused or force-pushed over."""
    if date_str is None:
        now_fn = now_fn or time.time
        date_str = datetime.fromtimestamp(now_fn(), tz=timezone.utc).strftime("%Y-%m-%d")
    numbers = _existing_sweep_branch_numbers(repo_root, date_str, run_git)
    n = (max(numbers) + 1) if numbers else 1
    return f"sweep/tags-{date_str}-{n}"


def cut_fresh_sweep_branch(repo_root, branch, run_git, origin_ref=ORIGIN_MAIN):
    """Create `branch` from origin_ref and check it out. Returns (ok, message)."""
    rc, _out, err = run_git(["branch", branch, origin_ref], repo_root)
    if rc != 0:
        return False, f"could not create {branch} from {origin_ref}: {err.strip()}"
    rc, _out, err = run_git(["checkout", branch], repo_root)
    if rc != 0:
        return False, f"could not check out {branch}: {err.strip()}"
    return True, "ok"


# ---------------------------------------------------------------------------
# Step 4: merge each green squad head at its stamped SHA
# ---------------------------------------------------------------------------

def merge_squad_into_sweep(repo_root, squad, squad_sha, run_git):
    """spec M4 step 4: merge one green squad head at its stamped SHA.

    Tries a plain fast-forward first (only ever possible for the FIRST
    squad merged onto a just-cut sweep branch, whose tip still equals
    origin_ref exactly); every subsequent squad's sha only descends
    from origin_ref, never from an already-merged sibling squad's
    commits, so its ff attempt necessarily falls through to a
    controlled ``--no-ff`` merge. A REAL content conflict (structurally
    near-impossible given one squad per shared emitter file, but not
    ruled out) is a hard error for JUST this squad: the merge is
    aborted and this squad contributes nothing to the sweep this round
    -- logged loudly, never silently skipped.

    Returns a dict: {"ok", "squad", "mode": "ff"|"merge"|None,
    "merge_sha": sha or None, "range_start", "range_end", "message"}.
    range_start/range_end always describe the commits this squad
    contributed (bisection's commits_contributed reads them back).
    """
    pre_tip = head_sha(repo_root, run_git)
    rc, _out, _err = run_git(["merge", "--ff-only", squad_sha], repo_root)
    if rc == 0:
        return {
            "ok": True, "squad": squad, "mode": "ff", "merge_sha": None,
            "range_start": pre_tip, "range_end": squad_sha, "message": "fast-forwarded",
        }

    rc, _out, err = run_git(
        ["merge", "--no-ff", squad_sha, "-m", f"sweep: merge squad/{squad} @ {squad_sha[:12]}"], repo_root,
    )
    if rc != 0:
        run_git(["merge", "--abort"], repo_root)
        return {
            "ok": False, "squad": squad, "mode": None, "merge_sha": None,
            "range_start": None, "range_end": None,
            "message": f"cross-squad conflict merging squad/{squad} @ {squad_sha[:12]}: {err.strip()}",
        }
    merge_sha = head_sha(repo_root, run_git)
    return {
        "ok": True, "squad": squad, "mode": "merge", "merge_sha": merge_sha,
        "range_start": pre_tip, "range_end": squad_sha, "message": "merged",
    }


def commits_contributed(repo_root, info, run_git):
    """The commit shas one merge_squad_into_sweep result actually
    brought into the sweep -- for a "merge" the squad's own commits
    (merge_sha^1..merge_sha^2, never the wrapper merge commit itself),
    for a "ff" the plain range_start..range_end."""
    if info["mode"] == "merge":
        return commits_in_range(repo_root, f"{info['merge_sha']}^1", f"{info['merge_sha']}^2", run_git)
    return commits_in_range(repo_root, info["range_start"], info["range_end"], run_git)


# ---------------------------------------------------------------------------
# spec M1 Verified trailer -> gap delta
# ---------------------------------------------------------------------------

def parse_verified_delta(value):
    """"recheck-pass gaps=<before>-><after>" -> before-after (spec M1's
    Verified trailer). None on anything that doesn't match (never
    raises -- a malformed trailer contributes 0 to the sum, same as a
    missing one)."""
    if not value:
        return None
    m = _VERIFIED_DELTA_RE.search(value)
    if not m:
        return None
    before, after = int(m.group(1)), int(m.group(2))
    return before - after


def sum_verified_deltas(repo_root, shas, run_git):
    """Sum of every commit's Verified-trailer delta -- reuses
    validate_fix_commit's own commit_message/parse_trailers (`git
    interpret-trailers --parse`), not a second hand-rolled trailer
    parser, per spec M4's "reuse validate_fix_commit.py machinery"."""
    total = 0
    for sha in shas:
        message = validate_fix_commit.commit_message(sha, repo_root, run_git)
        trailers = validate_fix_commit.parse_trailers(message, repo_root, run_git)
        for value in trailers.get("Verified", []):
            delta = parse_verified_delta(value)
            if delta is not None:
                total += delta
    return total


# ---------------------------------------------------------------------------
# Step 5: post-merge semantic recheck
# ---------------------------------------------------------------------------

def reattach_sweep_branch(repo_root, branch, run_git):
    """Point `branch` at the current HEAD and check it out ATTACHED.
    Returns (ok, message).

    Everything after step 5 -- bisection's `git revert` commits, step
    7b's cargo-fmt commit -- runs on a DETACHED HEAD, because
    run_post_merge_recheck ends with checkout_fn(repo_root, sweep_tip)
    and the production checkout_fn (real_checkout, also what
    parallel_model_fix_loop.default_sweep_fn wires in) is `git checkout
    --detach`. Commits made there are orphans: the sweep branch ref
    never moves, and the branch ref is the ONLY thing `git push origin
    <branch>` and `gh pr create --head <branch>` ever see.

    Measured 2026-07-26 by re-running RunSweepIntegrationTests'
    full-pass fixture with an fmt_fn that actually rewrites a .rs file
    (as `cargo fmt --all` does) instead of the no-op lambda that fixture
    used to inject: branch log = "fix JPEG:Foo | base", HEAD log =
    "style: cargo fmt --all (sweep publish) | fix JPEG:Foo | base", and
    `git show <branch>:src/parsers/jpeg/x.rs` still unformatted. The
    same fixture with a bisected sweep shipped the QUARANTINED squad's
    regression on the branch, because its revert commits were orphaned
    too. Net effect on the real fleet: every sweep PR pushed
    unformatted, CI's `cargo fmt --all -- --check` red (exactly the PR
    #124 failure step 7b exists to fix), auto_publish_round returns
    checks_red, nothing ever merges, and an --infinite dispatcher opens
    one red PR per round forever.

    `checkout -B` is safe here BECAUSE of the guard below and only
    because of it: HEAD is only ever an append to the branch tip (the
    merges run while attached; the recheck detaches AT that tip; the
    reverts append), so `branch` is always an ancestor of HEAD. If it
    somehow is not, this refuses rather than force-moving a ref --
    the fleet-wide no-discard invariant.
    """
    rc, head, err = run_git(["rev-parse", "HEAD"], repo_root)
    if rc != 0 or not head.strip():
        return False, f"could not resolve HEAD: {err.strip()}"
    rc, _out, err = run_git(["rev-parse", "--verify", "--quiet", f"refs/heads/{branch}"], repo_root)
    if rc != 0:
        return False, f"sweep branch {branch} no longer exists: {err.strip()}"
    rc, _out, _err = run_git(["merge-base", "--is-ancestor", branch, "HEAD"], repo_root)
    if rc != 0:
        return False, (f"refusing to move {branch} to HEAD: {branch} is not an ancestor of "
                       f"{head.strip()[:12]} (it carries commits HEAD does not)")
    rc, _out, err = run_git(["checkout", "-B", branch, head.strip()], repo_root)
    if rc != 0:
        return False, f"could not re-attach to {branch}: {err.strip()}"
    return True, f"re-attached HEAD to {branch}"


def run_post_merge_recheck(*, repo_root, formats, cache_dir, comparison_fn, checkout_fn, base_ref, sweep_tip):
    """spec M4 step 5: pre (origin/main-base) vs post (merged sweep
    branch tip) per-format comparison for the union of touched formats
    -- same intra-worktree lineage discipline as everywhere else in
    this fleet (worker recheck, merger batch check): checks out
    base_ref, runs comparison_fn per format, checks out sweep_tip, runs
    it again, and leaves the worktree on sweep_tip either way.
    """
    checkout_fn(repo_root, base_ref)
    pre = {fmt: comparison_fn(repo_root, cache_dir, fmt, "sweep-pre") for fmt in formats}
    checkout_fn(repo_root, sweep_tip)
    post = {fmt: comparison_fn(repo_root, cache_dir, fmt, "sweep-post") for fmt in formats}
    return pre, post


def evaluate_post_merge(pre, post, verified_delta_sum):
    """spec M4 step 5's mechanical assertion: measured gap delta >=
    Sigma Verified trailers (over-delivery -- an Exif.pm fix closing
    gaps in nine formats, a table port closing unenumerated siblings --
    is logged as bonus yield, never a failure), duplicate_emissions
    empty, new_oxidex_only empty. Returns (ok, measured_delta, problems)."""
    problems = []
    measured_delta = 0
    has_dup_or_new = False
    for fmt, post_report in post.items():
        pre_report = pre.get(fmt)
        pre_count = (pre_report or {}).get("gap_count", 0)
        post_count = (post_report or {}).get("gap_count", 0)
        measured_delta += pre_count - post_count
        dup = (post_report or {}).get("duplicate_emissions") or []
        if dup:
            problems.append(f"{fmt}: duplicate_emissions {dup}")
            has_dup_or_new = True
        introduced = new_oxidex_only_keys(pre_report, post_report)
        if introduced:
            problems.append(f"{fmt}: unexplained new_oxidex_only {introduced}")
            has_dup_or_new = True
    delta_ok = measured_delta >= verified_delta_sum
    if not delta_ok:
        problems.append(f"measured gap delta {measured_delta} < sum(Verified)={verified_delta_sum}")
    return (delta_ok and not has_dup_or_new), measured_delta, problems


# ---------------------------------------------------------------------------
# Mechanical bisection (spec M4 step 5's failure path)
# ---------------------------------------------------------------------------

def revert_squad_contribution(repo_root, info, run_git):
    """Undo one squad's contribution to the sweep branch: a controlled
    merge reverts via `git revert -m 1 <merge_sha>` (mainline=1, the
    sweep branch's own history before this squad's merge) -- always
    exactly one revert commit. A fast-forwarded squad (no wrapper commit
    exists) must ALSO end up as exactly one revert commit even when it
    contributed several: `undo_last_revert` (the isolation probe's
    restore step) only ever reverts the single most recent commit on
    HEAD, so `git revert <range>` -- which creates one revert commit PER
    original commit in the range -- would leave `undo_last_revert` able
    to restore only the last of them, silently dropping the rest of an
    innocent squad's already-verified work. `git revert --no-commit
    <range>` stages every commit's revert without committing between
    them, so one final `git commit` always folds the whole range into a
    single revert commit, symmetric with the merge-mode case. Aborts
    cleanly (never leaves a conflicted revert sitting in the index) on
    failure. Returns (ok, message)."""
    if info["mode"] == "merge":
        rc, _out, err = run_git(["revert", "--no-edit", "-m", "1", info["merge_sha"]], repo_root)
        if rc != 0:
            run_git(["revert", "--abort"], repo_root)
            return False, err.strip()
        return True, "reverted"

    commits = commits_in_range(repo_root, info["range_start"], info["range_end"], run_git)
    if not commits:
        return True, "nothing to revert"
    rc, _out, err = run_git(
        ["revert", "--no-commit", f"{info['range_start']}..{info['range_end']}"], repo_root,
    )
    if rc != 0:
        run_git(["revert", "--abort"], repo_root)
        return False, err.strip()
    rc, _out, err = run_git(
        ["commit", "-m", f"Revert squad/{info['squad']} contribution {info['range_start'][:12]}..{info['range_end'][:12]}"],
        repo_root,
    )
    if rc != 0:
        # git revert --no-commit itself finished cleanly (nothing to
        # abort there); a plain commit failing is an operational anomaly
        # (e.g. disk full) -- reset the staged revert away so no
        # half-reverted state is left on the sweep branch.
        run_git(["reset", "--hard", "HEAD"], repo_root)
        return False, err.strip()
    return True, "reverted"


def undo_last_revert(repo_root, run_git):
    """Restore a squad's contribution after a failed isolation probe by
    reverting the revert (pure append, never a reset -- consistent with
    the fleet-wide no-discard invariant). Returns (ok, message)."""
    head = head_sha(repo_root, run_git)
    if head is None:
        return False, "could not resolve HEAD"
    rc, _out, err = run_git(["revert", "--no-edit", head], repo_root)
    if rc != 0:
        run_git(["revert", "--abort"], repo_root)
        return False, err.strip()
    return True, "restored"


def bisect_sweep_failure(*, repo_root, merge_infos, formats, cache_dir, comparison_fn, checkout_fn,
                         base_ref, verified_deltas, quarantine_path, run_git, log_fn=print,
                         now_fn=time.time):
    """spec M4 step 5's mechanical bisection: try reverting ONE squad's
    contribution at a time (deterministic, sorted order); the first
    revert whose removal makes the post-merge recheck pass is declared
    the offender -- its commits are quarantined by patch-id in the SAME
    quarantine.jsonl ledger squad_merge_loop.py's mergers already
    consult (so a quarantined patch-id cannot re-enter through a
    squad's next recut either), and its contribution STAYS reverted. A
    squad whose removal does NOT fix the check is restored
    (undo_last_revert) and the next candidate is tried.

    If no single squad's removal clears the check (a genuine multi-
    squad combination effect, or the squads.toml-scale search space this
    single pass does not attempt), every remaining squad is quarantined
    and reverted -- a fully-aborted sweep round rather than a silently
    shipped bad merge.

    Returns {"offenders": [...], "surviving_squads": [...], "sweep_tip": sha}.
    """
    surviving = dict(merge_infos)
    offenders = []
    quarantine_entries = squad_merge_loop.load_quarantine(quarantine_path)

    def quarantine_squad(squad, reason):
        info = merge_infos[squad]
        for sha in commits_contributed(repo_root, info, run_git):
            patch_id = squad_merge_loop.compute_patch_id_for_sha(repo_root, sha)
            entry = squad_merge_loop.append_quarantine(
                quarantine_path, patch_id=patch_id, sha=sha, format_name=None, squad=squad,
                reason=reason, flags=["sweep-bisection"], quarantine_entries=quarantine_entries,
                now_fn=now_fn,
            )
            quarantine_entries[patch_id] = entry
        offenders.append(squad)
        surviving.pop(squad, None)

    def recheck(excluded_squad):
        delta_sum = sum(v for squad, v in verified_deltas.items() if squad in surviving and squad != excluded_squad)
        sweep_tip = head_sha(repo_root, run_git)
        pre, post = run_post_merge_recheck(
            repo_root=repo_root, formats=formats, cache_dir=cache_dir, comparison_fn=comparison_fn,
            checkout_fn=checkout_fn, base_ref=base_ref, sweep_tip=sweep_tip,
        )
        ok, _measured, _problems = evaluate_post_merge(pre, post, delta_sum)
        return ok

    found = False
    for squad in sorted(merge_infos):
        info = merge_infos[squad]
        reverted, message = revert_squad_contribution(repo_root, info, run_git)
        if not reverted:
            log_fn(f"bisection: could not revert squad/{squad}'s contribution ({message}) -- leaving it in place")
            continue
        if recheck(squad):
            log_fn(f"bisection: reverting squad/{squad} clears the sweep -- quarantining its commits")
            quarantine_squad(squad, "overlord sweep bisection: isolated as the offending squad")
            found = True
            break
        restored, r_message = undo_last_revert(repo_root, run_git)
        if not restored:
            log_fn(f"bisection: could not restore squad/{squad} after a failed isolation probe "
                   f"({r_message}) -- leaving it reverted; treating it as an offender this round")
            quarantine_squad(squad, "overlord sweep bisection: could not restore after isolation probe")
            found = True
            break

    if not found:
        log_fn("bisection: no single squad's removal clears the sweep -- quarantining and reverting "
               "every remaining squad this round (full sweep abort)")
        for squad in sorted(surviving):
            info = merge_infos[squad]
            reverted, message = revert_squad_contribution(repo_root, info, run_git)
            if reverted:
                quarantine_squad(squad, "overlord sweep bisection: still failing after isolating every candidate")
            else:
                log_fn(f"bisection: could not revert squad/{squad} during full abort ({message})")

    return {
        "offenders": offenders, "surviving_squads": sorted(surviving),
        "sweep_tip": head_sha(repo_root, run_git),
    }


# ---------------------------------------------------------------------------
# Step 7/8: judgment queue classification + PR evidence table
# ---------------------------------------------------------------------------

def classify_for_judgment_queue(sha, repo, run_git):
    """spec M4(e): reasons this commit must NOT auto-ship (empty list =
    ships mechanically as machine_accepted). Reuses
    validate_fix_commit's own diff extraction
    (extract_added_map_values) for the value-map/PrintConv check rather
    than a second pattern."""
    message = validate_fix_commit.commit_message(sha, repo, run_git)
    trailers = validate_fix_commit.parse_trailers(message, repo, run_git)
    diff_text = validate_fix_commit.commit_diff(sha, repo, run_git)
    changed = validate_fix_commit.commit_changed_files(sha, repo, run_git)

    reasons = []
    values, unverifiable = validate_fix_commit.extract_added_map_values(diff_text)
    if values or unverifiable:
        reasons.append("touches a value-map/PrintConv-like table")
    lines = diff_text.splitlines()
    if any(_NEW_FILE_RE.match(line) for line in lines):
        reasons.append("adds a new file")
    if any(_NEW_PARSE_FN_RE.match(line) for line in lines):
        reasons.append("adds a new top-level parse_ function")
    if any("/tests/" in f or f.startswith("tests/") or "fixtures" in f.lower() for f in changed):
        reasons.append("touches tests/fixtures")
    if trailers.get("Review-Unverifiable"):
        reasons.append("reviewer returned UNVERIFIABLE")
    if any(f in COMMONS_FILES or f.startswith(COMMONS_PREFIXES) for f in changed):
        reasons.append("touches a commons file")
    return reasons


def build_evidence_rows(shas, repo, run_git):
    """{tag: {"exiftool_value", "oxidex_value", "sample_count"}} parsed
    from every merged commit's Tag:/Exiftool-Value:/Oxidex-Value:/
    Sample: trailers (spec M1) -- one commit carries at most one
    Exiftool-Value/Oxidex-Value pair (for tag_keys[0] of its cluster,
    see model_fix_loop._build_fix_gap_trailers) but potentially several
    Tag: entries; every Tag: on that commit gets the same evidence
    pair, and sample_count aggregates across every merged commit that
    mentions the tag."""
    rows = {}
    for sha in shas:
        message = validate_fix_commit.commit_message(sha, repo, run_git)
        trailers = validate_fix_commit.parse_trailers(message, repo, run_git)
        tags = [t for t in trailers.get("Tag", []) if t]
        if not tags:
            continue
        et_val = next(iter(trailers.get("Exiftool-Value", [])), None)
        ox_val = next(iter(trailers.get("Oxidex-Value", [])), None)
        sample = next(iter(trailers.get("Sample", [])), None)
        for tag in tags:
            row = rows.setdefault(tag, {"exiftool_value": None, "oxidex_value": None, "sample_count": 0})
            if row["exiftool_value"] is None and et_val is not None:
                row["exiftool_value"] = et_val
            if row["oxidex_value"] is None and ox_val is not None:
                row["oxidex_value"] = ox_val
            if sample:
                row["sample_count"] += 1
    return rows


def render_evidence_table(rows):
    lines = ["| Tag | Exiftool-Value | Oxidex-Value | Sample count |", "|---|---|---|---|"]
    for tag in sorted(rows):
        row = rows[tag]
        lines.append(
            f"| {tag} | {row.get('exiftool_value') or ''} | {row.get('oxidex_value') or ''} | "
            f"{row.get('sample_count', 0)} |"
        )
    return "\n".join(lines)


def render_judgment_queue_section(judgment_entries):
    if not judgment_entries:
        return "No commits in this sweep require judgment-queue review -- everything ships mechanically."
    lines = ["The following commits are flagged for human judgment-queue review before merge:", ""]
    for sha, reasons in judgment_entries:
        lines.append(f"- `{sha[:12]}`: {', '.join(reasons)}")
    return "\n".join(lines)


def build_pr_body(*, evidence_rows, judgment_entries, branch):
    return "\n".join([
        f"## Sweep {branch}",
        "",
        "### Evidence table",
        "",
        render_evidence_table(evidence_rows),
        "",
        "### Judgment queue",
        "",
        render_judgment_queue_section(judgment_entries),
    ])


# ---------------------------------------------------------------------------
# Step 7b: cargo fmt the sweep branch before it is ever pushed
# ---------------------------------------------------------------------------

# Deliberately its own commit, and deliberately labelled: a reviewer
# scrolling the sweep PR's diff must be able to tell "this is rustfmt
# moving whitespace" from "this is a worker changing tag extraction"
# without reading either.
FMT_COMMIT_MESSAGE = """style: cargo fmt --all (sweep publish)

Worker-authored Rust reaches this branch semantically validated but never
style-checked: validate_fix_commit.py, the per-commit merger check and the
post-merge recheck all assert behaviour (gap deltas, no duplicate
emissions, no unexplained oxidex-only keys) and none of them look at
formatting. CI's "Lint & Audit" job runs `cargo fmt --all -- --check`, so
an unformatted sweep branch fails CI by construction.

Measured on PR #124 (branch sweep/tags-2026-07-26-1, the first sweep PR
ever opened): CI run 30186389305 -- "Build & Test" success, "Lint &
Audit" failure, and the failing step is literally `Run cargo fmt --all --
--check`. Kept as a separate commit so the tag-fix diffs stay readable.
"""


def real_cargo_fmt(repo_root):
    """`cargo fmt --all` in repo_root -> (ok, output). Injectable
    (``fmt_fn``) so hermetic tests never shell out to a real cargo."""
    result = subprocess.run(  # nosec B603
        ["cargo", "fmt", "--all"], cwd=repo_root, capture_output=True, text=True,
    )
    return result.returncode == 0, (result.stdout + result.stderr).strip()


def format_sweep_branch(repo_root, run_git, fmt_fn=None, log_fn=print):
    """Run cargo fmt over the assembled sweep branch and commit the
    result, if and only if it changed something. Returns
    {"ok", "committed", "message"}.

    Only tracked *.rs files are staged (`git add -u -- '*.rs'`): rustfmt
    never creates a file and never touches anything else, so scoping the
    stage this way makes it impossible for a stray artifact left in the
    sweep worktree (a comparison report, an editor swapfile) to ride
    along inside a commit labelled "cargo fmt".

    A cargo fmt that FAILS outright (no rustfmt component installed, a
    parse error) is logged loudly and reported as ok=False, but is not
    fatal to the caller: the worst case is the same red "Lint & Audit"
    the sweep has always had, and a PR left open for a human beats
    throwing away a branch full of validated fixes.
    """
    fmt_fn = fmt_fn or real_cargo_fmt
    ok, output = fmt_fn(repo_root)
    if not ok:
        log_fn(f"cargo fmt --all FAILED on the sweep branch: {output} -- pushing unformatted "
               "(expect CI's Lint & Audit job to go red on this PR)")
        return {"ok": False, "committed": False, "message": f"cargo fmt failed: {output}"}

    rc, _out, err = run_git(["add", "-u", "--", "*.rs"], repo_root)
    if rc != 0:
        return {"ok": False, "committed": False, "message": f"git add failed: {err.strip()}"}
    rc, staged, _err = run_git(["diff", "--cached", "--name-only"], repo_root)
    if rc != 0 or not staged.strip():
        return {"ok": True, "committed": False, "message": "already cargo-fmt clean"}

    rc, _out, err = run_git(["commit", "-m", FMT_COMMIT_MESSAGE], repo_root)
    if rc != 0:
        return {"ok": False, "committed": False, "message": f"git commit failed: {err.strip()}"}
    files = [line for line in staged.splitlines() if line]
    log_fn(f"cargo fmt --all reformatted {len(files)} file(s) -- committed separately before push")
    return {"ok": True, "committed": True, "message": f"committed cargo fmt over {len(files)} file(s)"}


# ---------------------------------------------------------------------------
# Step 8: PR creation (injectable -- never shells out in a way that
# fails hermetic tests)
# ---------------------------------------------------------------------------

def real_push_branch(repo_root, branch):
    """Push the freshly cut sweep branch to origin BEFORE ever calling
    ``gh pr create`` -- ``cut_fresh_sweep_branch`` only ever creates/
    checks out `branch` locally, so without this, ``gh pr create --head
    branch`` runs against a ref origin has never heard of: it either
    fails outright or blocks on an interactive "push it now?" prompt
    neither this script nor its human-driven ~5-minute cadence can ever
    answer. Injectable (``push_branch_fn``) so hermetic tests never
    touch a real "origin" remote. Returns (ok, message)."""
    result = subprocess.run(  # nosec B603
        ["git", "push", "-u", "origin", branch], cwd=repo_root, capture_output=True, text=True,
    )
    return result.returncode == 0, (result.stdout + result.stderr).strip()


def real_create_pr(title, body, branch, base="main", repo_root=REPO_ROOT):
    result = subprocess.run(  # nosec B603
        ["gh", "pr", "create", "--title", title, "--body", body, "--head", branch, "--base", base],
        cwd=repo_root, capture_output=True, text=True,
    )
    return {"ok": result.returncode == 0, "stdout": result.stdout.strip(), "stderr": result.stderr.strip()}


# ---------------------------------------------------------------------------
# Orchestrator
# ---------------------------------------------------------------------------

def run_sweep(*, repo_root, home, cache_dir, comparison_fn, checkout_fn,
             squads_toml_path=DEFAULT_SQUADS_TOML, cargo_test_workspace_fn=None, create_pr_fn=None,
             push_branch_fn=None, fmt_fn=None, run_git=None, now_fn=time.time, log_fn=print,
             sweep_state_path=None, quarantine_path=None, sweep_review_log_path=None,
             origin_ref=ORIGIN_MAIN, dispatcher_lock_path=None):
    """One full overlord sweep pass (spec M4). See module docstring for
    the step-by-step breakdown. Returns a summary dict whose "status"
    is one of: "no_news", "branch_cut_failed", "nothing_merged",
    "sweep_aborted" (bisection quarantined every candidate),
    "reattach_failed", "workspace_tests_failed", "push_failed",
    "pr_create_failed", "ok".
    """
    run_git = run_git or default_run_git
    cargo_test_workspace_fn = cargo_test_workspace_fn or _real_cargo_test_workspace
    create_pr_fn = create_pr_fn or real_create_pr
    push_branch_fn = push_branch_fn or real_push_branch
    sweep_state_path = Path(sweep_state_path) if sweep_state_path else DEFAULT_SWEEP_STATE_PATH
    quarantine_path = Path(quarantine_path) if quarantine_path else squad_merge_loop.quarantine_ledger_path(home)
    sweep_review_log_path = (
        Path(sweep_review_log_path) if sweep_review_log_path
        else Path(home) / "logs" / "sweep-review-history.jsonl"
    )

    squads = squads_from_toml(squads_toml_path)

    health = preflight(home, squads, dispatcher_lock_path=dispatcher_lock_path, now_fn=now_fn)
    if not health["ok"]:
        log_fn(f"preflight: stale lock(s) {health['stale']} -- informational, not a hard stop")

    cursor = load_sweep_state(sweep_state_path)
    stamps, new_cursor = collect_green_stamps(home, squads, cursor)
    if not stamps:
        log_fn("no news since last sweep -- nothing to do")
        return {"status": "no_news", "preflight": health}

    def persist_cursor(durable_squads):
        """Advance the cursor ONLY for squads whose stamp reached a
        durable outcome this round -- quarantined by bisection (a
        permanent quarantine.jsonl record, and the one case eager
        advancement is actually required: re-surfacing a quarantined
        patch-id would just re-trigger the identical failure forever,
        see collect_green_stamps's own docstring), or carried all the
        way to a cut, semantically-rechecked, workspace-tested sweep
        branch. Every squad NOT in durable_squads keeps its OLD cursor
        entry (untouched, from the `cursor` this round started with) --
        a branch-cut failure, a hard cross-squad merge conflict, or a
        workspace-test failure unrelated to any one squad must never
        make that squad's already-landed, never-PR'd commits invisible
        to every future sweep with no durable record to show for it."""
        if not durable_squads:
            return
        merged = {"squads": dict(cursor.get("squads") or {})}
        for squad in durable_squads:
            if squad in new_cursor["squads"]:
                merged["squads"][squad] = new_cursor["squads"][squad]
        save_sweep_state(sweep_state_path, merged)

    branch = next_sweep_branch_name(repo_root, run_git, now_fn=now_fn)
    ok, message = cut_fresh_sweep_branch(repo_root, branch, run_git, origin_ref=origin_ref)
    if not ok:
        log_fn(f"could not cut {branch}: {message}")
        # Nothing durable happened yet -- cursor stays exactly as loaded
        # so every squad's stamp is retried whole next sweep.
        return {"status": "branch_cut_failed", "branch": branch, "message": message, "preflight": health}

    merge_infos = {}
    failed_squads = {}
    for squad in sorted(stamps):
        info = merge_squad_into_sweep(repo_root, squad, stamps[squad]["squad_sha"], run_git)
        if info["ok"]:
            merge_infos[squad] = info
        else:
            failed_squads[squad] = info["message"]
            log_fn(f"HARD ERROR: {info['message']}")

    if not merge_infos:
        # Every squad hard-conflicted -- none quarantined, none durable;
        # cursor stays exactly as loaded so next sweep retries all of them.
        return {"status": "nothing_merged", "branch": branch, "failed_squads": failed_squads, "preflight": health}

    touched_formats = sorted({fmt for squad in merge_infos for fmt in stamps[squad]["formats"]})
    verified_deltas = {
        squad: sum_verified_deltas(repo_root, commits_contributed(repo_root, info, run_git), run_git)
        for squad, info in merge_infos.items()
    }

    sweep_tip = head_sha(repo_root, run_git)
    pre, post = run_post_merge_recheck(
        repo_root=repo_root, formats=touched_formats, cache_dir=cache_dir, comparison_fn=comparison_fn,
        checkout_fn=checkout_fn, base_ref=origin_ref, sweep_tip=sweep_tip,
    )
    ok, measured_delta, problems = evaluate_post_merge(pre, post, sum(verified_deltas.values()))

    bisection_result = None
    durable_squads = set()
    if not ok:
        log_fn(f"post-merge semantic recheck FAILED: {problems} -- bisecting")
        bisection_result = bisect_sweep_failure(
            repo_root=repo_root, merge_infos=merge_infos, formats=touched_formats, cache_dir=cache_dir,
            comparison_fn=comparison_fn, checkout_fn=checkout_fn, base_ref=origin_ref,
            verified_deltas=verified_deltas, quarantine_path=quarantine_path, run_git=run_git,
            log_fn=log_fn, now_fn=now_fn,
        )
        # Offenders are quarantined by patch-id -- durable and safe to
        # advance regardless of what happens to the surviving squads below.
        durable_squads.update(bisection_result["offenders"])
        merge_infos = {
            squad: info for squad, info in merge_infos.items()
            if squad in bisection_result["surviving_squads"]
        }
        if not merge_infos:
            persist_cursor(durable_squads)
            return {
                "status": "sweep_aborted", "branch": branch, "bisection": bisection_result,
                "failed_squads": failed_squads, "preflight": health,
            }

    # Step 5 left the worktree DETACHED at sweep_tip (see
    # run_post_merge_recheck), and so did every bisection probe. From
    # here on the round makes commits again -- bisection's reverts are
    # already on this HEAD, and step 7b's fmt commit is still to come --
    # and all of them have to be reachable from the branch ref that gets
    # pushed. Re-attaching once, here, is the single point that
    # guarantees it for everything downstream.
    attached_ok, attach_message = reattach_sweep_branch(repo_root, branch, run_git)
    if not attached_ok:
        log_fn(f"could not re-attach HEAD to {branch}: {attach_message} -- refusing to push a branch "
               "that does not contain this round's work (bisection reverts and/or the cargo fmt "
               "commit); leaving everything for manual inspection")
        # Same durability reasoning as workspace_tests_failed below: only
        # bisection-quarantined offenders are durable, the surviving
        # squads never reached a PR and must be retried by a later sweep.
        persist_cursor(durable_squads)
        return {
            "status": "reattach_failed", "branch": branch, "message": attach_message,
            "bisection": bisection_result, "failed_squads": failed_squads, "preflight": health,
        }

    all_shas = []
    for squad, info in merge_infos.items():
        all_shas.extend(commits_contributed(repo_root, info, run_git))

    workspace_ok, workspace_output = cargo_test_workspace_fn(repo_root)
    if not workspace_ok:
        log_fn("cargo test --workspace FAILED on the final sweep branch -- leaving it for manual "
               "inspection, no PR created")
        # Only the bisection-quarantined offenders (if any) are durable --
        # the surviving squads' recheck passed but never made it to a PR,
        # so their cursor entry must NOT advance: a later sweep needs to
        # find their stamp again to retry them.
        persist_cursor(durable_squads)
        return {
            "status": "workspace_tests_failed", "branch": branch, "bisection": bisection_result,
            "failed_squads": failed_squads, "workspace_output": workspace_output, "preflight": health,
        }

    # Full success from here: every surviving squad's contribution sits on
    # a cut, semantically-rechecked, workspace-tested sweep branch -- durable
    # regardless of what happens to the push/PR-creation calls below.
    durable_squads.update(merge_infos.keys())
    persist_cursor(durable_squads)

    git_runner = log_sweep_review.make_git_runner(repo_root)
    written, _skipped = log_sweep_review.append_from_commits(
        sweep_review_log_path, all_shas, git_runner, verdict="machine_accepted", now_fn=now_fn,
    )

    judgment_entries = [
        (sha, reasons) for sha in all_shas
        if (reasons := classify_for_judgment_queue(sha, repo_root, run_git))
    ]
    evidence_rows = build_evidence_rows(all_shas, repo_root, run_git)
    body = build_pr_body(evidence_rows=evidence_rows, judgment_entries=judgment_entries, branch=branch)
    title = f"sweep {branch}: {len(all_shas)} tag fix(es) across {len(merge_infos)} squad(s)"

    # Formatting is deliberately the LAST thing to touch the branch.
    # all_shas / the evidence table / the judgment queue above are the
    # TAG-FIX commits, and the fmt commit is not one of them: it carries
    # no trailers, closes no gap, and must not show up as a row in the
    # PR's evidence table or as an entry in the judgment queue. Running
    # it after cargo_test_workspace_fn is also deliberate -- rustfmt only
    # moves whitespace, so re-running a multi-minute workspace suite
    # afterwards would double the sweep's wall clock for no semantic gain.
    fmt_result = format_sweep_branch(repo_root, run_git, fmt_fn=fmt_fn, log_fn=log_fn)

    push_ok, push_message = push_branch_fn(repo_root, branch)
    if not push_ok:
        log_fn(f"could not push {branch} to origin: {push_message} -- skipping PR creation "
               "(the sweep branch and its commits are unaffected; retry the push/PR by hand)")
        return {
            "status": "push_failed", "branch": branch, "message": push_message,
            "merged_squads": sorted(merge_infos), "failed_squads": failed_squads,
            "measured_delta": measured_delta, "verified_delta_sum": sum(verified_deltas.values()),
            "bisection": bisection_result, "judgment_entries": judgment_entries, "preflight": health,
            "sweep_review_written": len(written), "fmt": fmt_result,
        }

    pr_result = create_pr_fn(title, body, branch, "main")
    # `gh pr create` fails routinely and quietly: an expired token, a
    # secondary rate limit, an org rule forbidding PRs from this actor,
    # no network. Until 2026-07-26 this result was stuffed into the
    # summary and never looked at, so the round still reported "ok" --
    # and auto_publish_round then treated the failure as a live PR:
    # pr_ref_from_result finds no http line in the empty stdout, falls
    # back to the BRANCH NAME, and wait_for_pr_checks polls `gh pr
    # checks <branch>` against a branch with no PR (three "unknown"
    # answers, two 30s sleeps). Net result was an orphan branch on
    # origin with no PR, the cursor already advanced past its stamps,
    # 60s burned, and `overlord_sweep.py` exiting 0.
    #
    # Only an EXPLICIT {"ok": False} counts as a failure: create_pr_fn is
    # injectable and several callers return None or a bare URL string,
    # and inventing a failure for those would be a worse bug than the
    # one being fixed.
    if isinstance(pr_result, dict) and "ok" in pr_result and not pr_result["ok"]:
        detail = (pr_result.get("stderr") or pr_result.get("stdout") or "").strip()
        log_fn(f"gh pr create FAILED for {branch}: {detail} -- the branch IS pushed and its "
               "commits are safe; open the PR by hand (`gh pr create --head "
               f"{branch} --base main`)")
        return {
            "status": "pr_create_failed", "branch": branch, "pr": pr_result, "message": detail,
            "fmt": fmt_result, "merged_squads": sorted(merge_infos), "failed_squads": failed_squads,
            "measured_delta": measured_delta, "verified_delta_sum": sum(verified_deltas.values()),
            "bisection": bisection_result, "judgment_entries": judgment_entries, "preflight": health,
            "sweep_review_written": len(written),
        }

    return {
        "status": "ok",
        "branch": branch,
        "fmt": fmt_result,
        "merged_squads": sorted(merge_infos),
        "failed_squads": failed_squads,
        "measured_delta": measured_delta,
        "verified_delta_sum": sum(verified_deltas.values()),
        "bisection": bisection_result,
        "judgment_entries": judgment_entries,
        "pr": pr_result,
        "preflight": health,
        "sweep_review_written": len(written),
    }


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def real_checkout(repo_root, ref):
    """The real ``checkout_fn`` for the pre/post recheck's detach dance.
    Public (not _-prefixed) because the dispatcher's own auto-publish
    step reuses it verbatim when it calls run_sweep in-process -- see
    parallel_model_fix_loop.default_sweep_fn."""
    subprocess.run(["git", "checkout", "--detach", ref], cwd=repo_root, check=True)  # nosec B603


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", default=str(REPO_ROOT))
    parser.add_argument("--home", default=str(OXIDEX_HOME))
    parser.add_argument("--squads-toml", default=str(DEFAULT_SQUADS_TOML))
    parser.add_argument("--cache-dir", default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))  # nosec B108
    args = parser.parse_args(argv)

    home = Path(args.home)
    repo_root = Path(args.repo)

    def comparison_fn(repo, cache_dir, fmt, suffix):
        return squad_merge_loop.real_format_match(repo, cache_dir, fmt, suffix)

    result = run_sweep(
        repo_root=repo_root, home=home, cache_dir=args.cache_dir, comparison_fn=comparison_fn,
        checkout_fn=real_checkout, squads_toml_path=args.squads_toml,
    )
    printable = {k: v for k, v in result.items() if k != "pr"}
    print(json.dumps(printable, indent=2, default=str))
    if result.get("status") == "pr_create_failed":
        # Loud and unmissable: the branch is on origin but NO PR exists,
        # and a `PR: {'ok': False, ...}` dict buried under a JSON blob is
        # not something anyone spots in a cron log.
        print(f"PR CREATION FAILED for {result.get('branch')} -- the branch is pushed but no PR "
              f"exists: {result.get('message')}")
        print(f"  retry with: gh pr create --head {result.get('branch')} --base main")
    elif result.get("pr") is not None:
        print(f"PR: {result['pr']}")
    return 0 if result.get("status") in ("ok", "no_news") else 1


if __name__ == "__main__":
    sys.exit(main())
