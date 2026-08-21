"""fleet -- operator CLI over the hub's fleet refs (FLEET.md M2 + M9).

    fleet up <host> --gates N [--agents N]     raise a host's targets
    fleet down <host> --reason "..."           disable a host (drain, not kill)
    fleet drain <host>                         finish current work, start nothing
    fleet status                               the whole fleet from hub refs only
    fleet status --why                         + per-host refused reasons, heartbeat
                                                age, desired gates/agents

Every subcommand talks ONLY to hub refs -- no ssh fan-out, no per-host
config. `up/down/drain` are read-modify-CAS-write on `refs/fleet/desired`
with retry: a conflict means another operator's edit landed first, so we
re-read and reapply ours on top (both edits survive).

`status` renders from `refs/fleet/hosts/*` heartbeats. A heartbeat older
than HEARTBEAT_STALE renders DOWN -- the ryzen's cron was dead for a full
day with nothing noticing; this line is why that cannot recur silently.
`--why` answers "why is nothing starting" from the same heartbeats' durable
`refused` field (PLAN Stage 1 task 5) -- no ssh, no re-derivation.

`--code`/`FLEET_CODE_URL` names the CODE repo (docs/AGENT-SERVER-SPEC.md
§4.4). `status`'s QUEUE line asks `workqueue.Queue` for the live queue,
and on a split spine that means a `hub.code_sha(TIP_REF)` against the CODE
repo, not the state repo `--hub`/`FLEET_HUB_URL` names. Leaving `_hub()`
with no way to learn that URL was not "coordination-only" (§4.4(c) listed
cli.py that way) -- it made the QUEUE line a permanent
`error: tip ref ... does not exist on the code repo '<state url>'` on every
split-spine invocation, `--why` included, because `fleetlib.Hub`'s
`code_url` defaults to `.url` when unset. `--code`/`FLEET_CODE_URL` mirror
`fleetd.py`'s own `--hub`/`--code` pair exactly, including the same
default (unset means "same repo as --hub", the single-repo topology this
tool predates)."""

from __future__ import annotations

import argparse
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fleetd
import workqueue
from claim import is_expired
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
    # R1: `code_url` is `fleetlib.Hub`'s own constructor argument (the same
    # one `fleetd.py --code`/`FLEET_CODE_URL` resolves, PLAN Stage 1 task
    # 4) -- left `None` here it defaults to `url`, which is exactly the
    # single-repo topology this tool predates and every existing (non-split)
    # invocation still gets unchanged. On a split spine, giving `status` a
    # way to be told the code repo is what turns its QUEUE line from a
    # permanent `queue-unavailable`-shaped error into the real count.
    code = getattr(args, "code", None) or os.environ.get("FLEET_CODE_URL")
    return Hub(url, workdir=Path.home() / ".fleetd" / "clicache", code_url=code)


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


def _claims_by_host(hub: Hub) -> dict:
    """{holder_host: [work_key, ...]} for every LIVE (unexpired) claim on
    the hub -- ARCH-FIX R4's WORK column. Reads the same `holder_host` /
    `work_key` fields claim.py writes and workqueue.py's exclusion check
    matches against, so this renders exactly what a second host's queue
    computation would see as "already spoken for", not an independent
    guess at it."""
    out: dict = {}
    now = datetime.now(timezone.utc)
    for ref in hub.list(CLAIMS_PREFIX):
        payload = hub.read(ref)
        if payload is None or is_expired(payload, now=now):
            continue
        holder = payload.get("holder_host")
        work_key = payload.get("work_key")
        if holder and work_key:
            out.setdefault(holder, []).append(work_key)
    return out


def _work_column(work_keys: list, limit: int = 2, max_len: int = 40) -> str:
    """Truncated summary of a host's live work_keys for the WORK column:
    the first `limit` keys, a "+N" tail for the rest, then a hard
    character cap so one very long branch name can't blow out the table.
    """
    if not work_keys:
        return "-"
    shown = list(work_keys[:limit])
    text = ",".join(shown)
    extra = len(work_keys) - len(shown)
    if extra > 0:
        text += f"+{extra}"
    if len(text) > max_len:
        text = text[: max_len - 1] + "…"
    return text


def _parked_rows(host: str, hb: dict) -> list:
    """`(host, state, branch, sha, source)` for every branch this host's
    last reconcile parked -- ARCH-FIX R4's shared-cache clause.

    The AWAITING/NEEDS_AUTH columns are counts, and a count cannot tell an
    operator whether a parked branch is still the branch that was judged.
    A verdict is about the SHA it was measured at, so fleetd records that
    sha alongside the name; this renders it. A `needs_author` row whose sha
    no longer matches `git ls-remote` is a branch whose author has already
    acted -- it should clear on the next loop, and if it does not, that is
    the bug to chase. Heartbeats written by an older fleetd carry bare
    names with no sha; those render "-" rather than being dropped.
    """
    out = []
    for state, key in (("AWAITING", "awaiting_train"), ("NEEDS_AUTH", "needs_author")):
        for entry in hb.get(key) or ():
            if isinstance(entry, dict):
                name, sha, source = entry.get("branch"), entry.get("sha"), entry.get("source")
            else:
                name, sha, source = str(entry), None, None
            if not name:
                continue
            out.append((host, state, name, (sha or "-")[:12], source or "-"))
    return out


def _refused_list(hb: dict) -> list:
    """[(reason, detail)] from a heartbeat's `refused` field (PLAN Stage 1
    task 5 / SPEC L121, L278: `ReconcileResult.refused` carried verbatim
    into `fleetd.write_heartbeat`'s payload).

    `write_heartbeat` JSON-round-trips each `(reason, detail)` tuple as a
    2-element array, so that is the shape read back here; a dict shape is
    also accepted for forward compatibility, and a heartbeat written by an
    older fleetd with no `refused` key at all yields `[]` rather than
    raising -- absence means "not yet reported", not "nothing refused".
    """
    out = []
    for entry in hb.get("refused") or ():
        if isinstance(entry, dict):
            reason, detail = entry.get("reason"), entry.get("detail")
        elif isinstance(entry, (list, tuple)) and len(entry) == 2:
            reason, detail = entry
        else:
            reason, detail = entry, None
        out.append((str(reason), "" if detail in (None, "") else str(detail)))
    return out


def cmd_status(args) -> int:
    hub = _hub(args)
    desired = hub.read(DESIRED_REF) or {}
    want = desired.get("hosts") or {}
    claims_by_host = _claims_by_host(hub)

    rows = []
    parked = []
    why_rows = []  # (host, state, age_s, want_gates, want_agents, [(reason, detail)])
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
        # ARCH-FIX R4: branch states this host's last reconcile surfaced,
        # and T1's lost-lease kill count for that same loop -- both come
        # straight from the heartbeat fleetd already writes (fleetd.py's
        # reconcile_once), nothing re-derived here.
        awaiting_train = len(fleetd.branch_names(hb.get("awaiting_train")))
        needs_author = len(fleetd.branch_names(hb.get("needs_author")))
        killed = hb.get("killed_this_loop")
        parked.extend(_parked_rows(host, hb))
        why_rows.append((host, state, age, w.get("gates"), w.get("agents"),
                          _refused_list(hb)))
        rows.append((
            host,
            state,
            f"{hb.get('gates_running', '?')}/{w.get('gates', '?')}",
            f"{hb.get('agents_running', '?')}/{w.get('agents', '?')}",
            _work_column(claims_by_host.get(host) or []),
            str(awaiting_train),
            str(needs_author),
            str(killed) if killed is not None else "-",
            f"{hb.get('free_gb', '?')}G",
            "✓" if oracle else ("?" if oracle is None else "✗"),
            hb.get("owning_user", "?"),
            f"{int(age)}s" if age != float("inf") else "never",
            (w.get("reason") or "")[:40],
        ))

    hdr = (
        "HOST", "STATE", "GATES", "AGENTS", "WORK", "AWAITING", "NEEDS_AUTH", "KILLED",
        "FREE", "ORACLE", "USER", "HEARTBEAT", "NOTE",
    )
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

    # Printed AFTER the QUEUE line on purpose: everything above it is the
    # per-host table, and this is a per-BRANCH one. Keeping the two apart
    # is also what lets a table parser stop at "QUEUE" and still be right.
    if parked:
        phdr = ("HOST", "STATE", "BRANCH", "DECIDED_AT", "SOURCE")
        pwidths = [max(len(str(r[i])) for r in parked + [phdr]) for i in range(len(phdr))]
        print("\nPARKED (verdict already known; not offered as gate work)")
        for r in [phdr] + parked:
            print("  " + "  ".join(str(c).ljust(w) for c, w in zip(r, pwidths)).rstrip())

    # PLAN Stage 1 task 5: "the human's question 'why is nothing starting'
    # has an answer from the laptop" -- per host, the last reconcile's
    # refused reasons plus the two numbers an operator checks first
    # (heartbeat freshness, desired targets), all from the durable
    # heartbeat + `desired` refs already read above. Behind a flag so the
    # default table (and `_parse_table`'s "stop at QUEUE" convention)
    # stay exactly as they were.
    if getattr(args, "why", False):
        print("\nWHY (last reconcile's refused[] from refs/fleet/hosts/*)")
        for host, state, age, want_gates, want_agents, refused in why_rows:
            age_s = f"{int(age)}s" if age != float("inf") else "never"
            gates = want_gates if want_gates is not None else "-"
            agents = want_agents if want_agents is not None else "-"
            print(f"  {host}  {state}  heartbeat age {age_s}  "
                  f"desired gates={gates} agents={agents}")
            if refused:
                for reason, detail in refused:
                    print(f"      refused: {reason}" + (f" ({detail})" if detail else ""))
            else:
                print("      (no refused reasons on file)")
        if not why_rows:
            print("  (no host heartbeats yet -- has fleetd been installed anywhere?)")
    return 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="fleet", description=__doc__.splitlines()[0])
    ap.add_argument("--hub", default=None, help="hub git URL (or FLEET_HUB_URL)")
    # R1: mirrors fleetd.py's own --code/FLEET_CODE_URL (PLAN Stage 1 task
    # 4) -- unset means "same repo as --hub", so a single-repo fleet is
    # unaffected. `status`'s QUEUE line is the one thing in this file that
    # needs it (workqueue.Queue asks `hub.code_sha(TIP_REF)`).
    ap.add_argument("--code", default=None,
                    help="code repo git URL (or FLEET_CODE_URL; default: same as --hub)")
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
    p.add_argument("--why", action="store_true",
                    help="per host: last reconcile's refused reasons, "
                         "heartbeat age, and desired gates/agents")
    p.set_defaults(fn=cmd_status)

    args = ap.parse_args(argv)
    return args.fn(args)


if __name__ == "__main__":
    raise SystemExit(main())
