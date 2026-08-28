#!/usr/bin/env python3
"""keel -- the Stage 2 operator CLI over FallbackHub(ServerHub, GitHubHub).

    keel status [--json]                 the fleet status -- computed the
                                          same way whether read via the
                                          server or --direct
    keel events [--follow] [--since N]   the server's event stream (SSE) --
                                          server-only, never on the hub
    keel desired show [--json]           refs/fleet/desired and its sha
    keel desired set --host H ...        read-modify-CAS one host's targets;
                                          generation++ server-side over
                                          PUT /v1/desired, client-side
                                          under --direct
    keel server status [--json]          server health + FallbackHub route
                                          + the server lease
    keel server rehost                   CAS-acquire the server lease if it
                                          is not currently live

docs/AGENT-SERVER-SPEC.md component C10: "`keel` CLI (ex-`cli.py`) with
`--direct`... `--direct` works with the server down." `--direct` bypasses
`ServerHub`/`FallbackHub` ENTIRELY and reads/writes the state repo hub
directly (the `GitHubHub` half) -- it is not merely "prefer this route",
it is "never construct a ServerHub or need KEEL_SERVER_URL at all".
`keel status` and `keel status --direct` compute the identical payload
off the identical Hub-shaped interface (`compute_status()` below never
branches on which kind of hub it was handed); only the `server` key is
expected to differ (SPEC SS3.4's own re-host acceptance instrument:
`diff <(keel status --json --direct) <(keel status --json) | jq
'del(.ts,.server)'` is empty), which is why every command deletes it
before comparing rather than special-casing route information out of the
payload.

Config is env-only, no `~/.keel/config.toml` (PLAN Stage 2 task 6):
    --hub / FLEET_HUB_URL          the state repo (refs/fleet/*)
    --code / FLEET_CODE_URL        the code repo (refs/heads/*); defaults
                                    to --hub, mirroring fleetd.py/cli.py's
                                    own --hub/--code pair (SPEC SS4.4)
    --server / KEEL_SERVER_URL     http(s)://host:port of keel-server
    --token-file / KEEL_TOKEN_FILE file holding the raw bearer token

THE SERVERHUB GAP. FallbackHub(ServerHub, GitHubHub) needs a `ServerHub`
that speaks `keel/server.py`'s wire protocol; PLAN Stage 2 lists it as
its own task, but no sibling branch (`staging/keel-2-serverhub`) exists to
supply one, so `keel/serverhub.py` was written alongside this file to
close that gap -- see its module docstring for the full account and the
note to reconcile/dedupe against a dedicated ServerHub task if one lands
separately.

STAGE 2's ACTUAL SCOPE, honestly kept. `keel status`'s server route reads
`refs/fleet/*` through the server's `/v1/refs` CAS facade -- the only
thing `keel/server.py` serves at this stage (no `/v1/status` route
exists yet; that is Stage 4's scheduler). `keel server rehost` performs
only SPEC SS3.4 step 1 (the CAS election on `refs/fleet/claims/server/
singleton`) and says so in its own output -- forking `keel-server`,
entering settle, and re-sweeping (steps 2-6) needs `election.py`/
`runner.py`, neither of which exists yet (PLAN Stage 2 task 5 / Stage 3).
"""

from __future__ import annotations

import argparse
import copy
import json
import os
import socket
import sys
import time
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Callable, Optional, Tuple

_KEEL_DIR = Path(__file__).resolve().parent
_FLEET_DIR = _KEEL_DIR.parent
for _p in (_FLEET_DIR, _KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

import workqueue  # noqa: E402
from claim import CLAIMS_PREFIX, claim_ref, is_expired  # noqa: E402
from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402

# Qualified `keel.<name>` imports, not bare ones -- see serverhub.py's
# module docstring for why a bare `import fallbackhub`/`import serverhub`
# here would risk a second, distinct module object (and therefore a
# second, distinct exception class) alongside whatever any other file in
# the process imported qualified.
from keel.fallbackhub import FallbackHub  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402

KEEL_VERSION = "keel-cli/0.1-stage2"

DESIRED_REF = "refs/fleet/desired"
HOSTS_PREFIX = "refs/fleet/hosts/"
HEARTBEAT_STALE_S = 180

# SPEC SS3.1/SS3.3 rule 5: the server's own singleton lease.
SERVER_CLAIM_KIND = "server"
SERVER_CLAIM_KEY = "singleton"
SERVER_CLAIM_TTL_S = 120

# `cli._edit_desired`'s retry semantics, which SPEC SS5.1 names as the
# ones `PUT /v1/desired` must preserve -- same counts as
# `tools/fleet/cli.py` L58-59 (CAS_RETRIES / CAS_BACKOFF_S), because a
# retry that re-applies the caller's own mutation to the fresh document
# is what lets two racing operators' edits BOTH survive, and the number
# of attempts is part of that promise.
CAS_RETRIES = 5
CAS_BACKOFF_S = 3

# `tools/fleet/cli.py` L62's DEFAULT_LIMITS, verbatim. Deliberately the
# other writer's defaults rather than SPEC SS3.1's fuller list: the only
# time this matters is a create-from-nothing, and `fleet up` and `keel
# desired set` creating two DIFFERENT initial documents would be worse
# than either one being short a key the operator can add.
DEFAULT_LIMITS = {"min_free_gb": 14, "min_free_mem_gb": 8}

DEFAULT_CLICACHE = Path.home() / ".keel" / "clicache"


# ------------------------------------------------------------------------ #
# Config resolution
# ------------------------------------------------------------------------ #


def _resolve(args: argparse.Namespace, attr: str, env: str) -> Optional[str]:
    return getattr(args, attr, None) or os.environ.get(env)


def _read_token_file(path: Optional[str]) -> Optional[str]:
    if not path:
        return None
    try:
        text = Path(path).expanduser().read_text().strip()
    except OSError as exc:
        print(f"keel: warning: could not read token file {path}: {exc}", file=sys.stderr)
        return None
    return text or None


def build_github_hub(args: argparse.Namespace) -> Hub:
    url = _resolve(args, "hub", "FLEET_HUB_URL")
    if not url:
        print("keel: no hub URL (--hub or FLEET_HUB_URL)", file=sys.stderr)
        raise SystemExit(2)
    code = _resolve(args, "code", "FLEET_CODE_URL")
    return Hub(url, workdir=DEFAULT_CLICACHE, code_url=code)


def build_server_hub(args: argparse.Namespace) -> Optional[ServerHub]:
    """`None` when no server is configured at all -- distinct from
    `--direct`, which skips this even when one is configured."""
    server_url = _resolve(args, "server", "KEEL_SERVER_URL")
    if not server_url:
        return None
    token = _read_token_file(_resolve(args, "token_file", "KEEL_TOKEN_FILE"))
    return ServerHub(server_url, token=token)


def build_hub(args: argparse.Namespace) -> Tuple[object, Optional[ServerHub]]:
    """`(hub, primary)`. `hub` is what every command reads/writes
    through; `primary` is the `ServerHub` half when the server route is
    in play (so callers can separately ask it for `/v1/health`), or
    `None` under `--direct` or with no server configured.

    `--direct` short-circuits before `build_server_hub` is even called --
    not just "prefer GitHub", but "never construct a ServerHub, never
    require KEEL_SERVER_URL" (SPEC C10).
    """
    github = build_github_hub(args)
    if getattr(args, "direct", False):
        return github, None
    primary = build_server_hub(args)
    if primary is None:
        print(
            "keel: no server URL (--server or KEEL_SERVER_URL); "
            "pass --direct to talk to the hub directly",
            file=sys.stderr,
        )
        raise SystemExit(2)
    return FallbackHub(primary, github), primary


# ------------------------------------------------------------------------ #
# Status: computed identically over any Hub-shaped object
# ------------------------------------------------------------------------ #


def _age_seconds(ts: str) -> float:
    try:
        then = datetime.strptime(ts, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
        return (datetime.now(timezone.utc) - then).total_seconds()
    except (ValueError, TypeError):
        return float("inf")


def _pairs(hb: dict, key: str) -> list:
    """`[{"reason":..., "detail":...}, ...]` from a heartbeat's
    `refused`/`warnings` field. Same wire tolerance as
    `tools/fleet/cli.py`'s `_refused_list`/`_warnings_list`: each entry
    may be a 2-element array (the JSON round-trip of a tuple), a dict, or
    a bare string; an absent key yields `[]`, never an exception."""
    out = []
    for entry in hb.get(key) or ():
        if isinstance(entry, dict):
            reason, detail = entry.get("reason"), entry.get("detail")
        elif isinstance(entry, (list, tuple)) and len(entry) == 2:
            reason, detail = entry
        else:
            reason, detail = entry, None
        out.append({"reason": str(reason), "detail": "" if detail in (None, "") else str(detail)})
    return out


def _host_row(host: str, hb: dict, want: dict) -> dict:
    age = _age_seconds(hb.get("ts") or "")
    if want.get("enabled") is False:
        state = "quarantined"
    elif age > HEARTBEAT_STALE_S:
        state = "down"
    else:
        state = "up"
    return {
        "host": host,
        "state": state,
        "heartbeat_age_s": None if age == float("inf") else round(age, 1),
        "gates_running": hb.get("gates_running"),
        "gates_wanted": want.get("gates"),
        "agents_running": hb.get("agents_running"),
        "agents_wanted": want.get("agents"),
        "awaiting_train": len(hb.get("awaiting_train") or []),
        "needs_author": len(hb.get("needs_author") or []),
        "killed_this_loop": hb.get("killed_this_loop"),
        "free_gb": hb.get("free_gb"),
        "oracle_ok": hb.get("oracle_ok"),
        "owning_user": hb.get("owning_user"),
        "reason": want.get("reason"),
        "refused": _pairs(hb, "refused"),
        "warnings": _pairs(hb, "warnings"),
    }


def compute_status(hub) -> dict:
    """The fleet status as one JSON-shaped dict, built ONLY from
    Hub-shaped calls (`hub.read`/`hub.list`, `workqueue.Queue`) -- so it
    is exactly as valid over a `FallbackHub` routed through the server as
    over a bare `GitHubHub` (`--direct`). That equivalence is the Stage 2
    claim under test, not an incidental property of this function.
    """
    desired = hub.read(DESIRED_REF) or {}
    want_by_host = desired.get("hosts") or {}
    hosts = [
        _host_row(ref[len(HOSTS_PREFIX):], hub.read(ref) or {}, want_by_host.get(ref[len(HOSTS_PREFIX):]) or {})
        for ref in sorted(hub.list(HOSTS_PREFIX))
    ]
    claims = len(hub.list(CLAIMS_PREFIX))
    queue, refusal = workqueue.Queue(hub).compute_or_refusal()
    queue_payload: dict = {"count": len(queue), "slugs": sorted(queue.keys())}
    if refusal is not None:
        reason, detail = refusal
        queue_payload["refused"] = {"reason": reason, "detail": detail}
    return {
        "hosts": hosts,
        "desired": {"generation": desired.get("generation"), "hosts": want_by_host},
        "claims": claims,
        "queue": queue_payload,
    }


def _route_meta(args: argparse.Namespace, primary: Optional[ServerHub]) -> dict:
    if getattr(args, "direct", False) or primary is None:
        return {"route": "direct"}
    meta: dict = {"route": "server", "url": primary.base_url}
    try:
        meta["health"] = primary.health()
    except HubUnreachableError as exc:
        meta["health_error"] = str(exc)
    return meta


def _print_status_table(payload: dict) -> None:
    hosts = payload.get("hosts") or []
    hdr = ("HOST", "STATE", "GATES", "AGENTS", "AWAITING", "NEEDS_AUTH", "FREE", "ORACLE", "HEARTBEAT")
    rows = []
    for h in hosts:
        age = h.get("heartbeat_age_s")
        oracle = h.get("oracle_ok")
        rows.append((
            h["host"], h["state"],
            f"{h.get('gates_running', '?')}/{h.get('gates_wanted', '?')}",
            f"{h.get('agents_running', '?')}/{h.get('agents_wanted', '?')}",
            str(h.get("awaiting_train", 0)), str(h.get("needs_author", 0)),
            f"{h.get('free_gb', '?')}G",
            "y" if oracle else ("?" if oracle is None else "n"),
            f"{int(age)}s" if age is not None else "never",
        ))
    widths = [max(len(str(r[i])) for r in rows + [hdr]) for i in range(len(hdr))]
    for r in [hdr] + rows:
        print("  ".join(str(c).ljust(w) for c, w in zip(r, widths)).rstrip())
    if not rows:
        print("(no host heartbeats yet)")
    q = payload.get("queue") or {}
    desired_gen = (payload.get("desired") or {}).get("generation", "-")
    route = (payload.get("server") or {}).get("route", "?")
    print(f"\nQUEUE {q.get('count')}   CLAIMS {payload.get('claims')}   DESIRED gen {desired_gen}   ROUTE {route}")


def cmd_status(args: argparse.Namespace) -> int:
    hub, primary = build_hub(args)
    try:
        payload = compute_status(hub)
    except HubUnreachableError as exc:
        print(f"keel: status: hub unreachable: {exc}", file=sys.stderr)
        return 1
    payload["ts"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    payload["server"] = _route_meta(args, primary)
    if getattr(args, "json", False):
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        _print_status_table(payload)
    return 0


# ------------------------------------------------------------------------ #
# Events: server-only (SPEC SS3.2 -- the ring is lossy-by-design and never
# replicated to the hub, so there is no `--direct` for this command)
# ------------------------------------------------------------------------ #


def cmd_events(args: argparse.Namespace) -> int:
    server_url = _resolve(args, "server", "KEEL_SERVER_URL")
    if not server_url:
        print(
            "keel: events needs a server (--server or KEEL_SERVER_URL) -- "
            "the event ring is server-only and is never replicated to the "
            "hub (SPEC SS3.2: lossy by design)",
            file=sys.stderr,
        )
        return 2
    token = _read_token_file(_resolve(args, "token_file", "KEEL_TOKEN_FILE"))
    primary = ServerHub(server_url, token=token)
    since = args.since or 0
    as_json = getattr(args, "json", False)
    try:
        for seq, kind, payload in primary.events(since=since, follow=args.follow, timeout=args.timeout):
            if as_json:
                print(json.dumps({"seq": seq, "kind": kind, "payload": payload}, sort_keys=True))
            else:
                print(f"{seq}\t{kind}\t{json.dumps(payload, sort_keys=True)}")
            sys.stdout.flush()
    except KeyboardInterrupt:
        return 0
    except HubUnreachableError as exc:
        print(f"keel: events: {exc}", file=sys.stderr)
        return 1
    return 0


# ------------------------------------------------------------------------ #
# desired: read-modify-CAS over PUT /v1/desired (SPEC SS5.1), or over the
# hub CAS under --direct
# ------------------------------------------------------------------------ #
#
# ONE retry loop, TWO places the generation++ can happen, and the
# difference is the whole point of the route. `edit_desired` below is
# `tools/fleet/cli.py`'s `_edit_desired` (L84-100) with the arithmetic
# lifted out into the client object:
#
#   * `_ServerDesired` PUTs the mutated document at `/v1/desired` and the
#     SERVER computes `generation` from the pre-image at the witnessed
#     version (SPEC SS5.1 "generation++ server-side"). Two operators
#     racing with the same witness produce one landed edit and exactly
#     one increment, decided by the one participant that can see both
#     images of the same CAS.
#   * `_DirectDesired` (`--direct`, no server) does the arithmetic here,
#     byte-identical to `cli._edit_desired`, because with no server there
#     is nowhere else to do it.
#
# The RETRY stays client-side on both routes: it re-applies the caller's
# own `mutate` to the freshly-read document, so a conflict means both
# operators' edits survive. A server cannot do that on the client's
# behalf -- it never sees `mutate`, only its result -- so it answers 409
# and this loop re-reads. Anything that merged server-side would be
# inventing a desired state neither operator asked for.


def _blank_desired() -> dict:
    return {"generation": 0, "hosts": {}, "limits": dict(DEFAULT_LIMITS)}


def _next_generation(current: Optional[dict]) -> int:
    """`cli._edit_desired` L94's arithmetic, tolerant of a document that
    has never carried a generation. `keel/server.py` carries the same
    three lines for the server route; they are the same rule stated at
    both ends on purpose -- the direct route has no server to ask."""
    raw = (current or {}).get("generation")
    try:
        return int(raw) + 1
    except (TypeError, ValueError):
        return 1


class _DirectDesired:
    """The hub CAS, generation bumped client-side."""

    route = "direct"

    def __init__(self, hub):
        self.hub = hub

    def read(self) -> Tuple[Optional[str], Optional[dict]]:
        return self.hub.read_with_sha(DESIRED_REF)

    def write(self, doc: dict, expect_sha: Optional[str], pre_image: Optional[dict]) -> Optional[dict]:
        doc = dict(doc)
        doc["generation"] = _next_generation(pre_image)
        ok = self.hub.create(DESIRED_REF, doc) if expect_sha is None else self.hub.update(DESIRED_REF, doc, expect_sha)
        return doc if ok else None


class _ServerDesired:
    """`PUT /v1/desired`. `pre_image` is deliberately unused: the server
    reads it for itself at the witnessed version, which is the only place
    the pre-image and the post-image of one CAS are both visible."""

    route = "server"

    def __init__(self, primary: ServerHub):
        self.primary = primary

    def read(self) -> Tuple[Optional[str], Optional[dict]]:
        return self.primary.read_desired()

    def write(self, doc: dict, expect_sha: Optional[str], pre_image: Optional[dict]) -> Optional[dict]:
        del pre_image
        return self.primary.put_desired(doc, expect_sha)


def edit_desired(client, mutate: Callable[[dict], dict], *, retries: int = CAS_RETRIES, backoff_s: float = CAS_BACKOFF_S) -> dict:
    """Read-modify-CAS-write with retry. `mutate(doc) -> doc` must be
    idempotent: on conflict it is re-applied to the FRESH document, so
    both racing operators' edits land. Returns the document the store
    now holds (generation included, whichever end computed it)."""
    last = "unknown"
    for attempt in range(retries):
        cur_sha, cur_doc = client.read()
        doc = mutate(copy.deepcopy(cur_doc) if cur_doc is not None else _blank_desired())
        # A `HubError` from `write` is a ROUTE failure, not a lost race,
        # and is deliberately NOT caught here: for an AMBIGUOUS write
        # (SPEC SS4.3 r2) re-issuing is the one thing that must not
        # happen, and a retry loop that swallowed the raise would do
        # exactly that on the next pass. It propagates to the caller,
        # which reports it and exits non-zero.
        landed = client.write(doc, cur_sha, cur_doc)
        if landed is not None:
            return landed
        last = "lost the CAS race"
        if attempt + 1 < retries:
            time.sleep(backoff_s * (attempt + 1))
    raise SystemExit(f"keel desired: could not update {DESIRED_REF} after {retries} attempts ({last})")


def _build_desired_client(args: argparse.Namespace):
    """`--direct` -> the hub CAS; otherwise the server route.

    There is no fallback for the WRITE. `FallbackHub` can route a read
    around an absent server safely, but a `PUT /v1/desired` that failed
    ambiguously must not be re-issued anywhere (SPEC SS4.3 r2), and a PUT
    that never reached the server would silently move the generation++
    from the server back to this process -- the same last-writer-wins
    hazard the route exists to remove. With no server configured, say so
    and name `--direct`, which is a DELIBERATE choice of the other
    semantics rather than an accident of reachability."""
    if getattr(args, "direct", False):
        return _DirectDesired(build_github_hub(args))
    primary = build_server_hub(args)
    if primary is None:
        print(
            "keel: desired needs a server (--server or KEEL_SERVER_URL) so the "
            "generation is bumped server-side (SPEC SS5.1); pass --direct to "
            "read-modify-CAS the hub yourself instead",
            file=sys.stderr,
        )
        raise SystemExit(2)
    return _ServerDesired(primary)


def cmd_desired_show(args: argparse.Namespace) -> int:
    hub, _primary = build_hub(args)
    try:
        sha, doc = hub.read_with_sha(DESIRED_REF)
    except HubUnreachableError as exc:
        print(f"keel desired show: hub unreachable: {exc}", file=sys.stderr)
        return 1
    if getattr(args, "json", False):
        print(json.dumps({"ref": DESIRED_REF, "sha": sha, "desired": doc}, indent=2, sort_keys=True))
        return 0
    if doc is None:
        print(f"{DESIRED_REF}: absent")
        return 0
    print(f"{DESIRED_REF} @ {sha}  generation {doc.get('generation')}")
    for host, entry in sorted((doc.get("hosts") or {}).items()):
        print(f"  {host}: {json.dumps(entry, sort_keys=True)}")
    limits = doc.get("limits")
    if limits:
        print(f"  limits: {json.dumps(limits, sort_keys=True)}")
    return 0


def cmd_desired_set(args: argparse.Namespace) -> int:
    if args.gates is None and args.agents is None and args.enabled is None and args.reason is None:
        print(
            "keel desired set: nothing to change -- pass at least one of "
            "--gates/--agents/--enable/--disable/--reason",
            file=sys.stderr,
        )
        return 2

    def mutate(doc: dict) -> dict:
        entry = doc.setdefault("hosts", {}).setdefault(args.host, {})
        if args.gates is not None:
            entry["gates"] = args.gates
        if args.agents is not None:
            entry["agents"] = args.agents
        if args.enabled is not None:
            entry["enabled"] = args.enabled
        if args.reason is not None:
            entry["reason"] = args.reason
        elif args.enabled is True:
            # `fleet up`'s own behaviour: re-enabling clears the stale
            # "why was this host down" note rather than leaving it to be
            # read as current.
            entry.pop("reason", None)
        return doc

    client = _build_desired_client(args)
    try:
        landed = edit_desired(client, mutate)
    except HubError as exc:
        print(
            f"keel desired set: {exc}\n"
            f"(the write was NOT re-issued on another route -- SPEC SS4.3 r2; "
            f"re-run once the server answers, or use --direct)",
            file=sys.stderr,
        )
        return 1
    if getattr(args, "json", False):
        print(json.dumps({"route": client.route, "desired": landed}, indent=2, sort_keys=True))
    else:
        print(
            f"desired gen {landed.get('generation')} via {client.route}: "
            f"{args.host} -> {json.dumps((landed.get('hosts') or {}).get(args.host), sort_keys=True)}"
        )
    return 0


# ------------------------------------------------------------------------ #
# Server: health/lease status, and the (partial, honestly-scoped) rehost
# ------------------------------------------------------------------------ #


def cmd_server_status(args: argparse.Namespace) -> int:
    hub, primary = build_hub(args)
    payload: dict = {"route": "direct" if primary is None else "server"}
    if primary is not None:
        try:
            payload["health"] = primary.health()
        except HubUnreachableError as exc:
            payload["health_error"] = str(exc)
    if isinstance(hub, FallbackHub):
        payload["fallback"] = hub.status()
    lease_ref = claim_ref(SERVER_CLAIM_KIND, SERVER_CLAIM_KEY)
    try:
        lease = hub.read(lease_ref)
    except HubUnreachableError as exc:
        print(f"keel: server status: hub unreachable: {exc}", file=sys.stderr)
        return 1
    payload["lease"] = lease
    payload["lease_live"] = bool(lease) and not is_expired(lease)
    if getattr(args, "json", False):
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        print(f"route: {payload['route']}")
        if "health" in payload:
            print(f"health: {payload['health']}")
        elif "health_error" in payload:
            print(f"health: unreachable ({payload['health_error']})")
        if "fallback" in payload:
            print(f"fallback: {payload['fallback']}")
        holder = f" (holder={lease.get('holder_host')})" if lease else ""
        print(f"server lease: {'live' if payload['lease_live'] else 'absent/expired'}{holder}")
    return 0


def cmd_server_rehost(args: argparse.Namespace) -> int:
    hub, _primary = build_hub(args)
    lease_ref = claim_ref(SERVER_CLAIM_KIND, SERVER_CLAIM_KEY)
    now = datetime.now(timezone.utc)
    try:
        cur_sha, cur_payload = hub.read_with_sha(lease_ref)
    except HubUnreachableError as exc:
        print(f"keel server rehost: hub unreachable: {exc}", file=sys.stderr)
        return 1
    if cur_payload is not None and not is_expired(cur_payload, now=now):
        print(
            f"keel server rehost: refusing -- the server lease is live "
            f"(holder={cur_payload.get('holder_host')}, expires_at={cur_payload.get('expires_at')}); "
            f"SPEC SS3.4: 'a live lease makes it refuse'",
            file=sys.stderr,
        )
        return 3
    payload = {
        "holder_host": socket.gethostname(),
        "pid": os.getpid(),
        "started_at": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "expires_at": (now + timedelta(seconds=SERVER_CLAIM_TTL_S)).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "advertise_urls": [],
        "boot_id": uuid.uuid4().hex,
        "keel_version": KEEL_VERSION,
    }
    try:
        won = hub.create(lease_ref, payload) if cur_sha is None else hub.update(lease_ref, payload, cur_sha)
    except HubError as exc:
        print(f"keel server rehost: hub unreachable: {exc}", file=sys.stderr)
        return 1
    if not won:
        print(
            "keel server rehost: lost the race for the server lease -- another host won it first",
            file=sys.stderr,
        )
        return 3
    print(
        f"keel server rehost: acquired the server lease as {payload['holder_host']} "
        f"(boot_id={payload['boot_id']}, ttl={SERVER_CLAIM_TTL_S}s).\n"
        f"NOTE: this performs SPEC SS3.4 step 1 only (the CAS election). Forking "
        f"keel-server, entering settle, and re-sweeping (steps 2-6) is election.py's "
        f"job (PLAN Stage 2 task 5 / Stage 3), not yet wired up -- start keel-server "
        f"on this host yourself now."
    )
    return 0


# ------------------------------------------------------------------------ #
# argparse wiring
# ------------------------------------------------------------------------ #


def _add_hub_args(p: argparse.ArgumentParser) -> None:
    p.add_argument("--hub", default=None, help="state repo git URL (or FLEET_HUB_URL)")
    p.add_argument("--code", default=None, help="code repo git URL (or FLEET_CODE_URL; default: same as --hub)")
    p.add_argument("--server", default=None, help="keel-server base URL, e.g. http://127.0.0.1:8470 (or KEEL_SERVER_URL)")
    p.add_argument("--token-file", dest="token_file", default=None, help="file holding the bearer token (or KEEL_TOKEN_FILE)")
    p.add_argument("--direct", action="store_true", help="bypass the server entirely; talk to the hub directly (SPEC C10)")
    p.add_argument("--json", action="store_true", help="JSON output")


def _add_server_args(p: argparse.ArgumentParser) -> None:
    p.add_argument("--server", default=None, help="keel-server base URL, e.g. http://127.0.0.1:8470 (or KEEL_SERVER_URL)")
    p.add_argument("--token-file", dest="token_file", default=None, help="file holding the bearer token (or KEEL_TOKEN_FILE)")
    p.add_argument("--json", action="store_true", help="JSON output")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="keel", description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_status = sub.add_parser("status", help="the fleet status (SPEC SS5.1-shaped, over the coordination hub)")
    _add_hub_args(p_status)
    p_status.set_defaults(func=cmd_status)

    p_events = sub.add_parser("events", help="the server's event stream (SPEC SS5.2)")
    _add_server_args(p_events)
    p_events.add_argument("--follow", action="store_true", help="keep streaming new events (SSE) instead of stopping once caught up")
    p_events.add_argument("--since", type=int, default=0, help="only events with seq > SINCE")
    p_events.add_argument(
        "--timeout", type=float, default=None,
        help="idle read timeout in seconds (default: 1.5s to catch up without --follow, "
        "the configured read timeout with --follow)",
    )
    p_events.set_defaults(func=cmd_events)

    p_desired = sub.add_parser("desired", help="read/edit refs/fleet/desired (SPEC SS3.1, SS5.1)")
    desired_sub = p_desired.add_subparsers(dest="desired_cmd", required=True)

    p_desired_show = desired_sub.add_parser("show", help="print refs/fleet/desired and its sha")
    _add_hub_args(p_desired_show)
    p_desired_show.set_defaults(func=cmd_desired_show)

    p_desired_set = desired_sub.add_parser(
        "set",
        help="read-modify-CAS one host's targets (generation++ server-side unless --direct)",
    )
    _add_hub_args(p_desired_set)
    p_desired_set.add_argument("--host", required=True, help="the host whose entry to edit")
    p_desired_set.add_argument("--gates", type=int, default=None, help="gate slots wanted on this host")
    p_desired_set.add_argument("--agents", type=int, default=None, help="agent slots wanted on this host")
    enable_group = p_desired_set.add_mutually_exclusive_group()
    enable_group.add_argument("--enable", dest="enabled", action="store_const", const=True, default=None)
    enable_group.add_argument("--disable", dest="enabled", action="store_const", const=False, default=None)
    p_desired_set.add_argument("--reason", default=None, help="why (recorded on the host's entry)")
    p_desired_set.set_defaults(func=cmd_desired_set)

    p_server = sub.add_parser("server", help="server lease/health (SPEC SS3.4)")
    server_sub = p_server.add_subparsers(dest="server_cmd", required=True)

    p_server_status = server_sub.add_parser("status", help="server health + FallbackHub route + the server lease")
    _add_hub_args(p_server_status)
    p_server_status.set_defaults(func=cmd_server_status)

    p_server_rehost = server_sub.add_parser(
        "rehost", help="CAS-acquire the server lease if it is not live (SPEC SS3.4 step 1 only -- see module docstring)",
    )
    _add_hub_args(p_server_rehost)
    p_server_rehost.set_defaults(func=cmd_server_rehost)

    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
