#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Answer one question the fleet could not answer about itself: IS EVERY
FORMAT STILL BEING PUBLISHED?

WHY THIS EXISTS
---------------
Before #209, squad->format routing was redundant: 13 of 14 squads listed
JPEG, so 13 mergers consumed the same worker branch. That was wasteful --
it produced 22 commits carrying 7 distinct patch-ids -- but it was also
accidental redundancy. A dead merger was masked by twelve others doing the
same job, so "a merger died" and "work stopped flowing" were different
events, and only the second one hurt.

#209 replaced that with `squad_merge_loop.format_owner_map`: exactly ONE
squad consumes each format's worker branch. That is correct, and it makes a
dead merger a TOTAL LOSS for the formats it owns -- there is no longer a
second consumer. #209's author flagged this as the follow-on risk.

It arrived four hours later. On 2026-07-30 at 23:40:41 the disk hit 100%;
eleven of fourteen mergers died on ENOSPC inside 32 seconds and the
supervisor died with them (an unguarded `mv` in its own state write, under
`set -e`). Nothing noticed, and nothing could have: the check an operator
would reach for is `pgrep -f squad_merge_loop`, and the fleet has burnt
itself on that before -- `pgrep -f` matches the asking shell's own argv, and
`~/.oxidex/logs/fleet-up.state` went on reporting all fourteen mergers
`running` for the next hour because the process that maintains it was dead.

So liveness here is NOT pgrep and the state file is never trusted as a
verdict. During a poll, the primary evidence is the merger's own singleton
lock (`<home>/logs/knowledge/merger-<squad>.lock`), which is:

  * written only by the merger itself, at acquire and at every heartbeat;
  * released in `run_locked`'s `finally` after EVERY poll, before the normal
    60-second sleep -- so a missing lock alone is ambiguous, not proof of
    death;
  * carrying a pid and a heartbeat timestamp, so a lock left by a
    SIGKILLed process is still detectable as stale rather than believed.

Between polls, `fleet-up.state` supplies only a candidate PID. That PID must
both exist and still have the exact squad merger command. The stale state
from the 2026-07-30 outage therefore remains harmless (its dead/recycled
PIDs fail validation), while a normally sleeping merger no longer looks
dead. Looking up one recorded PID is also structurally incapable of the
`pgrep -f` self-match that caused the earlier false healthy verdict.

A squad also counts as not-owning when it is BLOCKED. A merger whose batch
full-corpus check fails holds publication and retries on a cadence -- which
is right, and deliberately not terminal (quarantine being terminal is a
documented defect in this repo). But held publication is indistinguishable
from a dead merger where it matters: nothing reaches origin/main.
squad/panasonic-leica sat blocked from 19:04 to 23:15 on a duplicate-symbol
error (`the name parse_icc is defined multiple times`), logging the same
line every ~60s, with no escalation anywhere. That is a stall that looks
alive, and it belongs in the same alarm as a death.

Usage:
    fleet_health.py                        # report; exit 1 if any format is unowned
    fleet_health.py --json                 # same, machine-readable
    fleet_health.py --formats-for nikon    # formats that squad exclusively owns
    fleet_health.py --quiet                # exit status only
"""
import argparse
import json
import os
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from squad_merge_loop import (  # noqa: E402
    DEFAULT_CONFIG_PATH,
    OXIDEX_HOME,
    all_squad_names,
    batch_state_path,
    format_owner_map,
    merger_lock_path,
)

#: Heartbeat age past which a merger is reported as POSSIBLY STALLED. It is
#: an advisory, never an outage -- see merger_state for why the pid, not the
#: heartbeat, decides aliveness.
#:
#: Measured 2026-07-31 while writing this: with the machine at load ~397,
#: squad/olympus and squad/thermal were provably alive (logging `Compiling
#: oxidex` seconds earlier) yet 980s past their last heartbeat, because
#: `heartbeat_fn` only fires between commits and a cargo build under that
#: load holds the poll far longer. A threshold-only check called both of
#: them dead. An alarm that cries wolf about live mergers is worse than no
#: alarm, because it is the one people learn to ignore.
DEFAULT_STALE_SECONDS = 1800.0

#: How long a squad may hold publication before it is reported as an outage
#: rather than a hiccup. A failing batch check retries on the batch cadence,
#: so a couple of cycles is normal; four hours is what panasonic-leica did.
DEFAULT_BLOCKED_SECONDS = 3600.0


def fleet_state_merger_pid(home, squad):
    """Candidate merger PID from fleet-up.state, never a liveness verdict.

    The supervisor writes tab-separated rows as
    ``merger:<squad>  <pid>  running  <argv-pattern>``. A stale file is
    expected after a supervisor crash, so callers MUST validate both PID
    existence and argv before trusting the result.
    """
    path = Path(home) / "logs" / "fleet-up.state"
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return None
    wanted = f"merger:{squad}"
    for line in lines:
        fields = line.split("\t", 3)
        if len(fields) < 3 or fields[0] != wanted or fields[2] != "running":
            continue
        try:
            pid = int(fields[1])
        except ValueError:
            return None
        return pid if pid > 0 else None
    return None


def pid_alive(pid, kill_fn=os.kill):
    """True if `pid` exists. Deliberately NOT a pattern search: this is only
    ever asked about a pid a merger wrote into its own lock file, so there is
    nothing to self-match (the `pgrep -f` failure mode that made a dead
    merger tier look healthy on 2026-07-26)."""
    if not isinstance(pid, int) or pid <= 0:
        return False
    try:
        kill_fn(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True          # exists, owned by someone else
    except OSError:
        return False
    return True


def pid_is_merger_for(pid, squad, argv_fn=None):
    """PID-REUSE GUARD. `pid_alive` alone would believe any process that
    happened to inherit a dead merger's pid; this repo has already been bitten
    by exactly that (2fbf051c, "one recycled pgid must not kill the whole
    dispatcher"). Looking up ONE known pid rather than searching keeps this
    structurally incapable of matching the asking process.

    Unknown (argv unavailable) is treated as a match: a merger owned by
    another uid, or a `ps` that fails under load, must not be reported as an
    outage on that evidence alone.
    """
    if argv_fn is None:
        def argv_fn(p):
            try:
                out = subprocess.run(  # nosec B603 B607 -- list argv, no shell
                    ["ps", "-o", "command=", "-p", str(p)],
                    capture_output=True, text=True, timeout=10, check=False,
                )
            except (OSError, subprocess.SubprocessError):
                return None
            return out.stdout.strip() or None

    argv = argv_fn(pid)
    if not argv:
        return True
    return "squad_merge_loop.py" in argv and f"--squad {squad}" in f"{argv} "


def merger_state(home, squad, *, now=None, stale_seconds=DEFAULT_STALE_SECONDS,
                 kill_fn=os.kill, argv_fn=None):
    """``{"squad", "alive", "stalled", "reason", "pid", "age"}`` for one
    squad's merger.

    THE PID DECIDES, NOT THE HEARTBEAT. The states that matter:

      no lock file            -> normal between polls. Read the supervisor's
                                 candidate PID, then validate pid + exact argv.
                                 No candidate or failed validation is DOWN.
      lock + pid gone/recycled-> DOWN. The SIGKILL corpse: nothing ran the
                                 `finally`, so the file outlived its writer.
      lock + pid still ours   -> ALIVE, whatever the heartbeat says. A stale
                                 heartbeat on a live pid means BUSY (a long
                                 cargo build), not dead, and is reported as
                                 `stalled` for a human to look at rather than
                                 as an outage.

    `reason` is always populated, including when alive, so the report states
    what it observed instead of just asserting a verdict.
    """
    now = time.time() if now is None else now

    def down(reason, pid=None, age=None):
        return {"squad": squad, "alive": False, "stalled": False,
                "pid": pid, "age": age, "reason": reason}

    path = merger_lock_path(home, squad)
    try:
        raw = path.read_text()
    except FileNotFoundError:
        pid = fleet_state_merger_pid(home, squad)
        if pid is None:
            return down("no lock file and no running supervisor-state entry")
        if not pid_alive(pid, kill_fn=kill_fn):
            return down(f"supervisor state names pid {pid}, which is gone", pid=pid)
        if not pid_is_merger_for(pid, squad, argv_fn=argv_fn):
            return down(
                f"supervisor state names pid {pid}, but that pid is now a different "
                "program (recycled) -- merger not running",
                pid=pid,
            )
        return {
            "squad": squad,
            "alive": True,
            "stalled": False,
            "pid": pid,
            "age": None,
            "reason": f"pid {pid} alive between polls (validated from supervisor state)",
        }
    except OSError as exc:
        return down(f"lock unreadable: {exc}")
    try:
        info = json.loads(raw)
    except ValueError:
        return down("lock file corrupt")
    if not isinstance(info, dict):
        return down("lock file corrupt")

    pid = info.get("pid")
    heartbeat = info.get("heartbeat_ts")
    age = None if not isinstance(heartbeat, (int, float)) else max(0.0, now - heartbeat)

    if not pid_alive(pid, kill_fn=kill_fn):
        return down(f"lock held by pid {pid}, which is gone (crashed without cleanup)",
                    pid=pid, age=age)
    if not pid_is_merger_for(pid, squad, argv_fn=argv_fn):
        return down(f"lock names pid {pid}, but that pid is now a different program "
                    "(recycled) -- merger not running", pid=pid, age=age)

    stalled = age is not None and age >= stale_seconds
    if age is None:
        reason = f"pid {pid} alive; lock has no heartbeat"
    elif stalled:
        reason = (f"pid {pid} alive but no heartbeat for {age:.0f}s "
                  f"(> {stale_seconds:.0f}s) -- long build, or wedged")
    else:
        reason = f"pid {pid} alive, heartbeat {age:.0f}s ago"
    return {"squad": squad, "alive": True, "stalled": stalled,
            "pid": pid, "age": age, "reason": reason}


def blocked_state(home, squad, *, now=None, blocked_seconds=DEFAULT_BLOCKED_SECONDS):
    """``{"blocked", "since", "reason"}`` -- is this squad holding publication,
    and for how long? A squad blocked longer than `blocked_seconds` publishes
    nothing, which for an exclusively-owned format is the same outage as a
    dead merger."""
    now = time.time() if now is None else now
    try:
        data = json.loads(batch_state_path(home, squad).read_text())
    except (FileNotFoundError, ValueError, OSError):
        return {"blocked": False, "since": None, "reason": ""}
    if not isinstance(data, dict) or not data.get("blocked"):
        return {"blocked": False, "since": None, "reason": ""}
    ts = data.get("last_batch_ts")
    since = None if not isinstance(ts, (int, float)) else max(0.0, now - ts)
    if since is not None and since < blocked_seconds:
        return {"blocked": False, "since": since,
                "reason": f"batch check failing for {since / 60:.0f}m (under threshold)"}
    span = "unknown" if since is None else f"{since / 3600:.1f}h"
    return {"blocked": True, "since": since,
            "reason": f"publication held since the last batch check failed ({span} ago)"}


def assess(home, config_path=DEFAULT_CONFIG_PATH, *, now=None,
           stale_seconds=DEFAULT_STALE_SECONDS,
           blocked_seconds=DEFAULT_BLOCKED_SECONDS, kill_fn=os.kill, argv_fn=None):
    """Full health picture. `unowned` is the alarm: formats whose one
    exclusive owning squad is not publishing, whether because its merger is
    gone or because it is chronically blocked."""
    now = time.time() if now is None else now
    owners = format_owner_map(config_path)
    squads = all_squad_names(config_path)

    mergers = {s: merger_state(home, s, now=now, stale_seconds=stale_seconds,
                               kill_fn=kill_fn, argv_fn=argv_fn)
               for s in squads}
    blocks = {s: blocked_state(home, s, now=now, blocked_seconds=blocked_seconds)
              for s in squads}

    unowned = []
    for fmt in sorted(owners):
        squad = owners[fmt]
        merger = mergers.get(squad) or {"alive": False, "reason": f"unknown squad {squad!r}"}
        block = blocks.get(squad) or {"blocked": False, "reason": ""}
        if not merger["alive"]:
            unowned.append({"format": fmt.upper(), "squad": squad,
                            "cause": "merger-down", "detail": merger["reason"]})
        elif block["blocked"]:
            unowned.append({"format": fmt.upper(), "squad": squad,
                            "cause": "publication-blocked", "detail": block["reason"]})

    return {
        "checked_at": now,
        "owners": {f.upper(): s for f, s in owners.items()},
        "mergers": mergers,
        "blocked": {s: b for s, b in blocks.items() if b["blocked"]},
        "stalled": sorted(s for s, m in mergers.items() if m.get("stalled")),
        "unowned": unowned,
        "healthy": not unowned,
    }


def formats_owned_by(squad, config_path=DEFAULT_CONFIG_PATH):
    """Formats `squad` EXCLUSIVELY owns -- i.e. what stops flowing entirely
    if its merger dies. Used by fleet_up.sh to name the blast radius in the
    same log line that reports the death."""
    owners = format_owner_map(config_path)
    return sorted(f.upper() for f, s in owners.items() if s == squad)


def render(report, config_path=DEFAULT_CONFIG_PATH):
    """Operator-facing text. Leads with the alarm, because the alarm is the
    reason to run this.

    config_path is threaded through from the caller (main() passes
    args.config) rather than re-derived here, because DEFAULT_CONFIG_PATH
    is a gitignored, per-installation file -- unlike the git-tracked
    scripts/squads.toml this used to fall back to, it is not guaranteed to
    exist, so a caller that already resolved a real path must not have that
    choice silently overridden by this function reaching for its own
    default.
    """
    lines = []
    unowned = report["unowned"]
    if not unowned:
        live = sum(1 for m in report["mergers"].values() if m["alive"])
        lines.append(f"OK: all {len(report['owners'])} owned formats have a live, "
                     f"unblocked merger ({live}/{len(report['mergers'])} mergers up)")
    else:
        lines.append(f"ALARM: {len(unowned)} format(s) have NO live owner -- "
                     "work for these is stranded, nothing will publish them")
        width = max(len(u["format"]) for u in unowned)
        for u in unowned:
            lines.append(f"  {u['format']:<{width}}  owner={u['squad']}  "
                         f"{u['cause']}: {u['detail']}")

    down = [m for m in report["mergers"].values() if not m["alive"]]
    if down:
        lines.append("")
        lines.append(f"mergers down ({len(down)}):")
        for m in sorted(down, key=lambda x: x["squad"]):
            owned = ", ".join(formats_owned_by(m["squad"], config_path)) or "no exclusive format"
            lines.append(f"  {m['squad']:<16} {m['reason']}  [owns: {owned}]")

    if report.get("stalled"):
        lines.append("")
        lines.append("possibly stalled (alive, but no heartbeat for a long time -- "
                     "usually a long cargo build under load):")
        for squad in report["stalled"]:
            lines.append(f"  {squad:<16} {report['mergers'][squad]['reason']}")
    return "\n".join(lines)


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--home", default=str(OXIDEX_HOME))
    p.add_argument("--config", default=str(DEFAULT_CONFIG_PATH),
                   help="config.toml, for its [squads.*] tables (see config.example.toml)")
    p.add_argument("--stale-seconds", type=float, default=DEFAULT_STALE_SECONDS)
    p.add_argument("--blocked-seconds", type=float, default=DEFAULT_BLOCKED_SECONDS)
    p.add_argument("--formats-for", metavar="SQUAD",
                   help="print the formats this squad exclusively owns, one per "
                        "line, and exit 0")
    p.add_argument("--json", action="store_true")
    p.add_argument("--quiet", action="store_true", help="exit status only")
    args = p.parse_args(argv)

    if args.formats_for:
        for fmt in formats_owned_by(args.formats_for, args.config):
            print(fmt)
        return 0

    report = assess(Path(args.home), args.config,
                    stale_seconds=args.stale_seconds,
                    blocked_seconds=args.blocked_seconds)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    elif not args.quiet:
        print(render(report, args.config))
    # Non-zero is the point: this is meant to be usable from a supervisor,
    # a cron line, or `watch`, without parsing anything.
    return 0 if report["healthy"] else 1


if __name__ == "__main__":
    sys.exit(main())
