"""fleet -- operator CLI over the hub's fleet refs (FLEET.md M2 + M9).

    fleet up <host> --gates N [--agents N]     raise a host's targets
    fleet down <host> --reason "..."           disable a host (drain, not kill)
    fleet drain <host>                         finish current work, start nothing
    fleet status                               the whole fleet from hub refs only

Every subcommand talks ONLY to hub refs -- no ssh fan-out, no per-host
config. `up/down/drain` are read-modify-CAS-write on `refs/fleet/desired`
with retry: a conflict means another operator's edit landed first, so we
re-read and reapply ours on top (both edits survive).

`status` renders from `refs/fleet/hosts/*` heartbeats. A heartbeat older
than HEARTBEAT_STALE renders DOWN -- the ryzen's cron was dead for a full
day with nothing noticing; this line is why that cannot recur silently.
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import workqueue
from fleetlib import Hub, HubError

DESIRED_REF = "refs/fleet/desired"
HOSTS_PREFIX = "refs/fleet/hosts/"
CLAIMS_PREFIX = "refs/fleet/claims/"
HEARTBEAT_STALE = 180
CAS_RETRIES = 5
CAS_BACKOFF_S = 3

DEFAULT_LIMITS = {"min_free_gb": 14, "min_free_mem_gb": 8}


def _hub(args) -> Hub:
    url = args.hub or os.environ.get("FLEET_HUB_URL")
    if not url:
        print("fleet: no hub URL (--hub or FLEET_HUB_URL)", file=sys.stderr)
        raise SystemExit(2)
    return Hub(url, workdir=Path.home() / ".fleetd" / "clicache")


def _edit_desired(hub: Hub, mutate) -> dict:
    """Read-modify-CAS-write with retry. `mutate(doc) -> doc` must be
    idempotent: on conflict it is re-applied to the fresh document, so
    both racing operators' edits land (the CAS-conflict test in
    tools/fleet/tests proves this composition)."""
    last_err = "unknown"
    for attempt in range(CAS_RETRIES):
        cur_sha = hub.sha(DESIRED_REF)
        doc = hub.read(DESIRED_REF) or {"generation": 0, "hosts": {}, "limits": dict(DEFAULT_LIMITS)}
        doc = mutate(doc)
        doc["generation"] = int(doc.get("generation") or 0) + 1
        try:
            ok = hub.create(DESIRED_REF, doc) if cur_sha is None else hub.update(DESIRED_REF, doc, cur_sha)
        except HubError as e:
            ok, last_err = False, str(e)
        if ok:
            return doc
        time.sleep(CAS_BACKOFF_S * (attempt + 1))
    raise SystemExit(f"fleet: could not update {DESIRED_REF} after {CAS_RETRIES} attempts ({last_err})")


def cmd_up(args) -> int:
    hub = _hub(args)

    def mutate(doc):
        entry = doc.setdefault("hosts", {}).setdefault(args.host, {})
        entry["gates"] = args.gates
        if args.agents is not None:
            entry["agents"] = args.agents
        entry["enabled"] = True
        entry.pop("reason", None)
        return doc

    doc = _edit_desired(hub, mutate)
    print(f"desired gen {doc['generation']}: {args.host} -> {doc['hosts'][args.host]}")
    return 0


def cmd_down(args) -> int:
    hub = _hub(args)

    def mutate(doc):
        entry = doc.setdefault("hosts", {}).setdefault(args.host, {})
        entry["enabled"] = False
        entry["reason"] = args.reason
        return doc

    doc = _edit_desired(hub, mutate)
    print(f"desired gen {doc['generation']}: {args.host} disabled ({args.reason})")
    return 0


def cmd_drain(args) -> int:
    hub = _hub(args)

    def mutate(doc):
        entry = doc.setdefault("hosts", {}).setdefault(args.host, {})
        entry["gates"] = 0
        entry["agents"] = 0
        # enabled stays True: drain = converge to zero without the hard
        # disabled stop, so a later `fleet up` needs no re-enable.
        entry.setdefault("enabled", True)
        return doc

    doc = _edit_desired(hub, mutate)
    print(f"desired gen {doc['generation']}: {args.host} draining to 0")
    return 0


def _age_seconds(ts: str) -> float:
    try:
        then = datetime.strptime(ts, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
        return (datetime.now(timezone.utc) - then).total_seconds()
    except (ValueError, TypeError):
        return float("inf")


def cmd_status(args) -> int:
    hub = _hub(args)
    desired = hub.read(DESIRED_REF) or {}
    want = desired.get("hosts") or {}

    rows = []
    for ref in sorted(hub.list(HOSTS_PREFIX)):
        host = ref[len(HOSTS_PREFIX):]
        hb = hub.read(ref) or {}
        age = _age_seconds(hb.get("ts") or "")
        w = want.get(host) or {}
        if w.get("enabled") is False:
            state = "QUAR"
        elif age > HEARTBEAT_STALE:
            state = "DOWN"
        else:
            state = "up"
        oracle = hb.get("oracle_ok")
        rows.append((
            host,
            state,
            f"{hb.get('gates_running', '?')}/{w.get('gates', '?')}",
            f"{hb.get('agents_running', '?')}/{w.get('agents', '?')}",
            f"{hb.get('free_gb', '?')}G",
            "✓" if oracle else ("?" if oracle is None else "✗"),
            hb.get("owning_user", "?"),
            f"{int(age)}s" if age != float("inf") else "never",
            (w.get("reason") or "")[:40],
        ))

    hdr = ("HOST", "STATE", "GATES", "AGENTS", "FREE", "ORACLE", "USER", "HEARTBEAT", "NOTE")
    widths = [max(len(str(r[i])) for r in rows + [hdr]) for i in range(len(hdr))]
    for r in [hdr] + rows:
        print("  ".join(str(c).ljust(w) for c, w in zip(r, widths)).rstrip())
    if not rows:
        print("(no host heartbeats yet -- has fleetd been installed anywhere?)")

    try:
        qn = len(workqueue.Queue(hub).compute())
    except Exception as e:  # noqa: BLE001 -- status must render even if queue fails
        qn = f"error: {e}"
    claims = len(hub.list(CLAIMS_PREFIX))
    gen = desired.get("generation", "-")
    print(f"\nQUEUE {qn}   CLAIMS {claims}   DESIRED gen {gen}")
    return 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="fleet", description=__doc__.splitlines()[0])
    ap.add_argument("--hub", default=None, help="hub git URL (or FLEET_HUB_URL)")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("up", help="raise a host's worker targets")
    p.add_argument("host")
    p.add_argument("--gates", type=int, required=True)
    p.add_argument("--agents", type=int, default=None)
    p.set_defaults(fn=cmd_up)

    p = sub.add_parser("down", help="disable a host (drain, not kill)")
    p.add_argument("host")
    p.add_argument("--reason", required=True)
    p.set_defaults(fn=cmd_down)

    p = sub.add_parser("drain", help="converge a host to zero without disabling")
    p.add_argument("host")
    p.set_defaults(fn=cmd_drain)

    p = sub.add_parser("status", help="render the fleet from hub refs")
    p.set_defaults(fn=cmd_status)

    args = ap.parse_args(argv)
    return args.fn(args)


if __name__ == "__main__":
    raise SystemExit(main())
