#!/usr/bin/env python3
"""`keel-server`'s transport layer: stdlib `ThreadingHTTPServer` with HTTP/1.1
keep-alive, chunked SSE for `/v1/events`, `/v1/wait` long-poll plumbing,
bearer auth, a connection cap, and a listener watchdog.

SCOPE (docs/AGENT-SERVER-SPEC.md SS2 C6, SS5, SS8; PLAN Stage 2 task 4). This
file owns TRANSPORT ONLY:

  * the HTTP request-routing skeleton (`Router`/`Route`, `KeelRequestHandler`),
  * bearer auth over sha256-hashed tokens (`TokenStore`) with a 401-vs-403
    distinction and a hard rule that no code path here ever logs a raw
    token or `Authorization` header,
  * chunked Server-Sent Events for `/v1/events`, replaying an sqlite ring
    (`EventLog`) from `?since=<seq>`/`Last-Event-ID`, with a `: keepalive`
    comment on an idle stream,
  * `/v1/wait?ref=&since=` generic long-poll plumbing (poll a `Store`'s
    cheap `sha()` probe until it changes or a deadline passes),
  * the connection cap (503 past N concurrent connections), the LISTEN
    BACKLOG that has to be at least as deep as that cap, and the listener
    watchdog thread (self-probes the bound address; rebinds if nothing has
    answered in `watchdog_timeout` seconds),
  * the bind-address restriction (loopback or Tailscale CGNAT only, unless
    overridden),
  * `/v1/health`, and
  * `GET|PUT /v1/desired` (SPEC SS5.1), the one route in this file whose
    body it does more than forward: the generation counter is bumped
    SERVER-SIDE, which is the whole reason the route exists rather than
    letting every operator PUT `refs/fleet/desired` through the CAS
    facade with its own idea of what the next generation is.

It does NOT own ref CAS semantics, index freshness, the write-through to
the state repo, or the "fresh claims" rule -- that is `keel/cachedhub.py`'s
`CachedHub`, built to satisfy the `Store` protocol in `keel/store_api.py`.
`server.py` imports nothing from `cachedhub.py`; every route that touches a
ref calls `self.server.store.<method>()` and maps the result per
`store_api`'s contract (`False`/`None`/raise), never inspecting what is on
the other side of that call. See `store_api.py`'s module docstring for the
full contract and for `InMemoryStore`, the reference double this module's
own tests and its standalone `main()` use in place of a real `CachedHub`.

Standard library only.
"""

from __future__ import annotations

import argparse
import hashlib
import hmac
import ipaddress
import json
import logging
import os
import re
import socket
import sqlite3
import sys
import threading
import time
import urllib.parse
import uuid
from dataclasses import dataclass
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Callable, Dict, FrozenSet, List, Optional, Sequence, Tuple

_KEEL_DIR = Path(__file__).resolve().parent
_FLEET_DIR = _KEEL_DIR.parent
for _p in (_KEEL_DIR, _FLEET_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

import store_api  # noqa: E402
from store_api import Payload, StoreUnreachableError  # noqa: E402

# --------------------------------------------------------------------- #
# Defaults
# --------------------------------------------------------------------- #

DEFAULT_PORT = 8470
DEFAULT_MAX_CONNECTIONS = 64
DEFAULT_LONG_POLL_TIMEOUT_S = 25.0
DEFAULT_LONG_POLL_INTERVAL_S = 0.2
DEFAULT_SSE_KEEPALIVE_INTERVAL_S = 15.0
DEFAULT_WATCHDOG_TIMEOUT_S = 30.0
DEFAULT_WATCHDOG_CHECK_INTERVAL_S = 5.0
DEFAULT_EVENT_RING_CAPACITY = 50_000

# SPEC SS8: "the server refuses to bind a non-tailnet, non-loopback address
# unless KEEL_ALLOW_PUBLIC_BIND=1". Tailscale's CGNAT range.
_TAILSCALE_CGNAT = ipaddress.ip_network("100.64.0.0/10")


def _keel_home() -> Path:
    override = os.environ.get("KEEL_HOME")
    if override:
        return Path(override)
    return Path.home() / ".keel"


def default_tokens_file_path() -> Path:
    return _keel_home() / "auth.json"


def default_events_db_path() -> Path:
    return _keel_home() / "events.db"


def validate_bind_host(host: str, allow_any_bind: bool) -> None:
    """Raise `ValueError` unless `host` is loopback or in Tailscale's CGNAT
    range `100.64.0.0/10` -- unless `allow_any_bind` or
    `KEEL_ALLOW_PUBLIC_BIND=1` says to skip the check entirely (SPEC SS8).
    """
    if allow_any_bind or os.environ.get("KEEL_ALLOW_PUBLIC_BIND") == "1":
        return
    if host == "localhost":
        return
    try:
        addr = ipaddress.ip_address(host)
    except ValueError as exc:
        raise ValueError(
            f"refusing to bind {host!r}: not a literal loopback/Tailscale IP "
            "and --allow-any-bind was not given (SPEC SS8)"
        ) from exc
    if addr.is_loopback:
        return
    if addr in _TAILSCALE_CGNAT:
        return
    raise ValueError(
        f"refusing to bind {host}: neither loopback nor a Tailscale address "
        f"in {_TAILSCALE_CGNAT} -- pass --allow-any-bind or set "
        "KEEL_ALLOW_PUBLIC_BIND=1 to override (SPEC SS8)"
    )


# --------------------------------------------------------------------- #
# Auth
# --------------------------------------------------------------------- #


@dataclass(frozen=True)
class Principal:
    id: str
    role: str


class AuthError(Exception):
    """Carries the HTTP status the dispatcher should answer with: 401 for
    "not authenticated at all" (no header, malformed header, unrecognized
    token), 403 for "authenticated, but this role may not call this
    route". Never constructed with the raw token in `code` or anywhere
    else that could reach a log line.
    """

    def __init__(self, status: int, code: str):
        super().__init__(code)
        self.status = status
        self.code = code


class TokenStore:
    """sha256(token) -> `Principal`. Holds no raw tokens after construction
    (SPEC SS5/SS8: "Server stores only sha256 hashes ... compares with
    hmac.compare_digest"). `authenticate()` takes a raw token only long
    enough to hash it -- it is never written to a log, an exception
    message, or a repr anywhere in this module.
    """

    def __init__(self, entries: Sequence[Dict[str, str]]):
        by_hash: Dict[str, Principal] = {}
        for entry in entries:
            digest = entry["sha256"].strip().lower()
            by_hash[digest] = Principal(id=entry["id"], role=entry["role"])
        self._by_hash = by_hash

    @classmethod
    def from_file(cls, path: "os.PathLike[str] | str") -> "TokenStore":
        data = json.loads(Path(path).read_text())
        entries = data["tokens"] if isinstance(data, dict) else data
        return cls(entries)

    @classmethod
    def empty(cls) -> "TokenStore":
        return cls([])

    def authenticate(self, token: str) -> Optional[Principal]:
        digest = hashlib.sha256(token.encode("utf-8")).hexdigest()
        for known_digest, principal in self._by_hash.items():
            if hmac.compare_digest(digest, known_digest):
                return principal
        return None


# --------------------------------------------------------------------- #
# Event ring (SPEC SS3.2, SS5.1 GET /v1/events)
# --------------------------------------------------------------------- #


@dataclass(frozen=True)
class Event:
    seq: int
    ts: float
    kind: str
    payload: Payload


class EventLog:
    """An sqlite-backed, capacity-bounded, append-only ring: the source
    the SSE handler replays from `since=<seq>` and blocks on for new rows.

    Deliberately NOT a source of truth (SS3.2: "lossy by design") -- it
    lets a reconnecting dashboard/CLI/runner catch up on recent history
    without re-deriving it from claims/verdicts/attempts; it is not where
    a decision gets reconstructed from once the ring has rotated past it.

    One `sqlite3` connection, `check_same_thread=False`, all access
    serialized through `self._cond`'s lock -- simpler and plenty fast for
    an in-process event stream; correctness does not depend on SQLite's
    own cross-connection locking here; it is used purely for durability
    across a restart.
    """

    def __init__(self, db_path: "os.PathLike[str] | str", ring_capacity: int = DEFAULT_EVENT_RING_CAPACITY):
        self._db_path = Path(db_path)
        if str(self._db_path) != ":memory:":
            self._db_path.parent.mkdir(parents=True, exist_ok=True)
        self._ring_capacity = ring_capacity
        self._cond = threading.Condition()
        self._conn = sqlite3.connect(str(self._db_path), check_same_thread=False)
        self._conn.execute(
            "CREATE TABLE IF NOT EXISTS events ("
            "seq INTEGER PRIMARY KEY AUTOINCREMENT, "
            "ts REAL NOT NULL, "
            "kind TEXT NOT NULL, "
            "payload TEXT NOT NULL)"
        )
        self._conn.commit()

    def append(self, kind: str, payload: Payload) -> int:
        with self._cond:
            cur = self._conn.execute(
                "INSERT INTO events (ts, kind, payload) VALUES (?, ?, ?)",
                (time.time(), kind, json.dumps(payload, sort_keys=True)),
            )
            seq = cur.lastrowid
            # Ring: drop everything more than `ring_capacity` rows behind
            # the current max. A no-op until the table actually exceeds
            # capacity (SPEC SS3.2: 50k rows).
            self._conn.execute(
                "DELETE FROM events WHERE seq <= (SELECT COALESCE(MAX(seq), 0) FROM events) - ?",
                (self._ring_capacity,),
            )
            self._conn.commit()
            self._cond.notify_all()
            return int(seq)

    def since(self, seq: int) -> List[Event]:
        with self._cond:
            rows = self._conn.execute(
                "SELECT seq, ts, kind, payload FROM events WHERE seq > ? ORDER BY seq ASC",
                (seq,),
            ).fetchall()
        return [Event(seq=r[0], ts=r[1], kind=r[2], payload=json.loads(r[3])) for r in rows]

    def latest_seq(self) -> int:
        with self._cond:
            return self._latest_seq_locked()

    def _latest_seq_locked(self) -> int:
        row = self._conn.execute("SELECT COALESCE(MAX(seq), 0) FROM events").fetchone()
        return int(row[0]) if row else 0

    def wait_for_new(self, after_seq: int, timeout: float) -> bool:
        """Block until `latest_seq() > after_seq` or `timeout` elapses.
        Returns whether new events became available."""
        deadline = time.monotonic() + max(0.0, timeout)
        with self._cond:
            while self._latest_seq_locked() <= after_seq:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                self._cond.wait(remaining)
            return True

    def close(self) -> None:
        with self._cond:
            self._conn.close()


def _format_sse(seq: int, kind: str, payload: Payload) -> bytes:
    lines = [f"id: {seq}", f"event: {kind}", f"data: {json.dumps(payload, sort_keys=True)}", "", ""]
    return "\n".join(lines).encode("utf-8")


_SSE_KEEPALIVE_LINE = b": keepalive\n\n"


# --------------------------------------------------------------------- #
# Routing skeleton
# --------------------------------------------------------------------- #

HandlerFn = Callable[["KeelRequestHandler", Dict[str, str], Dict[str, List[str]], Optional[Principal]], None]

_PARAM_RE = re.compile(r"\{([a-zA-Z_][a-zA-Z0-9_]*)(:path)?\}")


def compile_path(template: str) -> "re.Pattern[str]":
    """`"/v1/refs/{ref:path}"` -> a compiled regex with a named group per
    `{name}` (matches one path segment) or `{name:path}` (matches the
    rest of the path, slashes included -- refs are slash-shaped)."""
    parts = []
    last = 0
    for m in _PARAM_RE.finditer(template):
        parts.append(re.escape(template[last : m.start()]))
        name = m.group(1)
        is_path = m.group(2) == ":path"
        parts.append(f"(?P<{name}>.+)" if is_path else f"(?P<{name}>[^/]+)")
        last = m.end()
    parts.append(re.escape(template[last:]))
    return re.compile("^" + "".join(parts) + "$")


@dataclass(frozen=True)
class Route:
    method: str
    path_pattern: "re.Pattern[str]"
    handler: HandlerFn
    # None = no auth at all (/v1/health); frozenset() = any authenticated
    # principal regardless of role; a non-empty frozenset = only those
    # roles (everyone else authenticates fine and gets 403).
    auth: Optional[FrozenSet[str]]
    name: str


class Router:
    """A tiny, generic HTTP route table. Deliberately not tied to any one
    stage's route list: later stages (scheduler, agents, OPERATOR) add
    routes by calling `.add()`, never by editing the dispatch mechanism
    itself."""

    def __init__(self) -> None:
        self._routes: List[Route] = []

    def add(
        self,
        method: str,
        template: str,
        handler: HandlerFn,
        auth: Optional[FrozenSet[str]],
        name: Optional[str] = None,
    ) -> None:
        self._routes.append(
            Route(
                method=method.upper(),
                path_pattern=compile_path(template),
                handler=handler,
                auth=auth,
                name=name or template,
            )
        )

    def match(self, method: str, path: str) -> Tuple[Optional[Route], Dict[str, str], bool]:
        """Returns `(route, params, path_exists)`. `path_exists` is True
        when some route's path matched but not this method -- lets the
        dispatcher answer 405 instead of a misleading 404."""
        method = method.upper()
        path_exists = False
        for route in self._routes:
            m = route.path_pattern.match(path)
            if not m:
                continue
            path_exists = True
            if route.method == method:
                return route, m.groupdict(), True
        return None, {}, path_exists


# --------------------------------------------------------------------- #
# Route handlers
# --------------------------------------------------------------------- #


def _decode_ref(raw: str) -> str:
    return urllib.parse.unquote(raw)


# SPEC SS4.3 rule 1 (r1, "fresh claims"): any read under
# `refs/fleet/claims/` is served LIVE from the store, never from an
# index -- a stale sha on a runner's own claim makes `claim.renew()`
# adopt it, the CAS rejects the renewal, and `_mark_lost` kills a
# healthy gate. `CachedHub` applies the same policy internally (it is
# the one place that knows what an index is), but the refs handlers
# force `fresh` here as well so the property holds against ANY store a
# server is wired to, and for any client that forgot its `?fresh=1`.
# Trailing slash so `refs/fleet/claimsX` can never match by accident.
_CLAIMS_LIVE_PREFIX = "refs/fleet/claims/"


def handle_health(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    server = handler.server
    body: Dict[str, object] = {
        "boot_id": server.boot_id,
        "uptime_s": round(time.monotonic() - server.boot_monotonic, 3),
        "connections": {"active": server.active_connection_count(), "max": server.config.max_connections},
        "watchdog": {
            "restart_count": server.restart_count,
            "last_healthy_probe_age_s": round(time.monotonic() - server.last_healthy_probe_at, 3),
        },
    }
    try:
        extra = server.health_provider() or {}
    except Exception:
        logging.exception("keel-server: health_provider raised")
        extra = {"health_provider_error": True}
    body.update(extra)
    # Election hook (SPEC SS5.1: health carries `lease_expires_at` and
    # `settle_until`): duck-typed so this module never imports election.py.
    if server.election is not None:
        body.update(server.election.health_fields())
    handler._send_json(200, body)


def handle_refs_list(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    prefix = query.get("prefix", [None])[0]
    if not prefix:
        handler._send_json(400, {"error": "missing-prefix"})
        return
    entries = handler.server.store.list(prefix)
    body = {ref: {"sha": e.sha, "observed_at": e.observed_at} for ref, e in entries.items()}
    handler._send_json(200, body)


def handle_refs_get(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    ref = _decode_ref(params["ref"])
    fresh = query.get("fresh", ["0"])[0] == "1" or ref.startswith(_CLAIMS_LIVE_PREFIX)
    sha, payload = handler.server.store.read_with_sha(ref, fresh=fresh)
    if sha is None:
        handler._send_json(404, {"error": "not-found"})
        return
    handler._send_json(200, {"sha": sha, "payload": payload})


def _read_json_body(handler: "KeelRequestHandler") -> Optional[Payload]:
    length = int(handler.headers.get("Content-Length", "0") or "0")
    raw = handler.rfile.read(length) if length else b""
    if not raw:
        return {}
    try:
        return json.loads(raw.decode("utf-8"))
    except (json.JSONDecodeError, UnicodeDecodeError):
        return None


def _answer_create(handler: "KeelRequestHandler", ref: str, payload: Payload) -> None:
    """The one create path both spellings share: `POST /v1/refs/{ref}`
    (the ServerHub client's spelling, PLAN Stage 2 task 3) and
    `PUT /v1/refs/{ref}` + `If-None-Match: *` (SPEC SS4.2's). 201 with
    the sha the store produced, or 409 on a lost race."""
    result = handler.server.store.create(ref, payload)
    if result is False:
        handler._send_json(409, {"error": "conflict"})
    else:
        handler._send_json(201, {"sha": result})


def handle_refs_post(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    ref = _decode_ref(params["ref"])
    payload = _read_json_body(handler)
    if payload is None:
        handler._send_json(400, {"error": "invalid-json"})
        return
    _answer_create(handler, ref, payload)


def handle_refs_put(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    ref = _decode_ref(params["ref"])
    payload = _read_json_body(handler)
    if payload is None:
        handler._send_json(400, {"error": "invalid-json"})
        return
    if_none_match = handler.headers.get("If-None-Match")
    if_match = handler.headers.get("If-Match")
    if if_none_match == "*":
        _answer_create(handler, ref, payload)
        return
    if if_match:
        result = handler.server.store.update(ref, payload, expect_sha=if_match)
        if result is False:
            handler._send_json(409, {"error": "conflict"})
        else:
            handler._send_json(200, {"sha": result})
        return
    handler._send_json(400, {"error": "missing-precondition-header"})


def handle_refs_delete(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    ref = _decode_ref(params["ref"])
    length = int(handler.headers.get("Content-Length", "0") or "0")
    if length:
        handler.rfile.read(length)  # drain -- DELETE bodies are unexpected but must not corrupt keep-alive framing
    if_match = handler.headers.get("If-Match")
    if not if_match:
        handler._send_json(400, {"error": "missing-precondition-header"})
        return
    ok = handler.server.store.delete(ref, expect_sha=if_match)
    if not ok:
        handler._send_json(409, {"error": "conflict"})
        return
    handler._send_status_only(204)


# -- GET|PUT /v1/desired (SPEC SS3.1, SS5.1) ---------------------------- #
#
# SPEC SS5.1: "`GET|PUT /v1/desired` (`If-Match`; generation++ server-side;
# `cli._edit_desired` retry semantics)."
#
# WHY THIS IS NOT JUST `PUT /v1/refs/refs%2Ffleet%2Fdesired`. The CAS
# facade would carry the document perfectly well; what it cannot carry is
# the ONE invariant `refs/fleet/desired` has that no other ref does --
# `generation` is a monotonic counter of edits, and every writer bumping
# it for itself means every writer can get it wrong. `tools/fleet/cli.py`'s
# `_edit_desired` (L84-100) does the arithmetic client-side today because
# there was nowhere else to do it; with a server there is, and the server
# is the only participant that sees the pre-image and the post-image of
# the same CAS. So the body a client PUTs here is its DESIRED STATE, never
# its idea of the next generation: whatever `generation` the body carries
# is discarded and replaced with `<generation at the witnessed version> +
# 1`. Two operators racing with the same witness therefore produce exactly
# one landed edit and exactly one increment -- the lost-update that a
# last-writer-wins PUT would hide.
#
# `cli._edit_desired`'s RETRY semantics stay CLIENT-side, unchanged and on
# purpose: the retry re-applies the caller's own `mutate` to the fresh
# document, so both racing operators' edits survive. The server cannot do
# that for them -- it never sees `mutate`, only its result -- so it answers
# 409 and lets the client re-read, re-mutate and re-PUT. A server that
# silently merged instead would be inventing a desired state neither
# operator asked for.
#
# Preconditions, and what each refusal means:
#   * `If-Match: <sha>`   -> read-modify-CAS at that version. 200 + the
#                            stored document on success, 409 on a lost
#                            race (the ref moved, or is absent).
#   * `If-None-Match: *`  -> create the ref iff absent (generation 1).
#                            201, or 409 if someone created it first.
#   * neither              -> 412. A PUT with no witness is the
#                            lost-update this route exists to refuse; it
#                            is never treated as "clobber".
#   * a malformed witness  -> 400. Includes `If-Match: *` (a wildcard is
#                            not a version, and honouring it would be
#                            exactly the clobber above) and any weak
#                            validator `W/"..."` (weak comparison cannot
#                            arbitrate a CAS).

DESIRED_REF = "refs/fleet/desired"

# A witness is opaque to this module -- a real store returns git object
# ids, `store_api.InMemoryStore` returns `mem0000...`-shaped tokens -- so
# "malformed" is checked structurally (non-empty, one token, no control
# characters, bounded) rather than against a hex pattern this file has no
# business asserting.
_MAX_WITNESS_LEN = 200


class _BadWitness(Exception):
    """`(status, code)` for a witness the CAS cannot use."""

    def __init__(self, status: int, code: str):
        super().__init__(code)
        self.status = status
        self.code = code


def _normalize_witness(raw: Optional[str]) -> str:
    """An `If-Match` header value -> the bare sha `Store.update` wants.

    Strips one layer of HTTP entity-tag quoting (the route answers with an
    `ETag`, so a well-behaved client may echo it back quoted) and refuses
    everything a CAS cannot arbitrate."""
    if raw is None:
        raise _BadWitness(412, "precondition-required")
    value = raw.strip()
    if value.startswith("W/"):
        raise _BadWitness(400, "weak-witness")
    if len(value) >= 2 and value.startswith('"') and value.endswith('"'):
        value = value[1:-1].strip()
    if not value:
        raise _BadWitness(400, "malformed-witness")
    if value == "*":
        raise _BadWitness(400, "wildcard-witness")
    if len(value) > _MAX_WITNESS_LEN or any(c.isspace() for c in value) or not value.isprintable():
        raise _BadWitness(400, "malformed-witness")
    return value


def _next_generation(current: Optional[Payload]) -> int:
    """`cli._edit_desired` L94, moved server-side: the generation at the
    witnessed version plus one, tolerating a document that has never
    carried one (absent, null) or carries something that is not a whole
    number (a hand-edited ref) rather than 500-ing on it."""
    raw = (current or {}).get("generation")
    try:
        return int(raw) + 1
    except (TypeError, ValueError):
        return 1


def handle_desired_get(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    # Always fresh: `desired` is what a PUT is about to CAS against, and a
    # witness handed out from a stale index is a 409 the client cannot
    # explain (or, worse, a CAS against a version the store never had).
    sha, payload = handler.server.store.read_with_sha(DESIRED_REF, fresh=True)
    if sha is None:
        handler._send_json(404, {"error": "not-found", "ref": DESIRED_REF})
        return
    handler._send_json(200, {"sha": sha, "ref": DESIRED_REF, "payload": payload}, extra_headers={"ETag": f'"{sha}"'})


def handle_desired_put(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    # A zero-length body is refused BEFORE parsing, because
    # `_read_json_body` answers `{}` for both "no body at all" and an
    # explicit `{}` -- and the first is a client bug that would otherwise
    # wipe every host out of `desired` and bump the generation while
    # doing it. An operator who really means "empty document" sends the
    # two bytes.
    if int(handler.headers.get("Content-Length", "0") or "0") <= 0:
        handler._send_json(400, {"error": "empty-body", "hint": "PUT /v1/desired takes the whole desired document; send {} to mean an empty one"})
        return
    body = _read_json_body(handler)
    if body is None:
        handler._send_json(400, {"error": "invalid-json"})
        return
    if not isinstance(body, dict):
        handler._send_json(400, {"error": "desired-must-be-an-object"})
        return
    store = handler.server.store
    if_match = handler.headers.get("If-Match")
    if_none_match = handler.headers.get("If-None-Match")

    if if_none_match is not None and if_none_match.strip() == "*":
        if if_match is not None:
            handler._send_json(400, {"error": "conflicting-preconditions"})
            return
        doc = dict(body)
        doc["generation"] = _next_generation(None)
        result = store.create(DESIRED_REF, doc)
        if result is False:
            handler._send_json(409, {"error": "conflict", "ref": DESIRED_REF})
            return
        handler._send_json(201, {"sha": result, "ref": DESIRED_REF, "payload": doc}, extra_headers={"ETag": f'"{result}"'})
        return
    if if_none_match is not None:
        handler._send_json(400, {"error": "unsupported-if-none-match"})
        return

    try:
        witness = _normalize_witness(if_match)
    except _BadWitness as bad:
        handler._send_json(bad.status, {"error": bad.code, "hint": "PUT /v1/desired requires If-Match: <sha>, or If-None-Match: * to create"})
        return

    # Read the PRE-IMAGE at the witnessed version. The short-circuit below
    # is an optimisation and a better error, never the CAS itself: the
    # store's `update(expect_sha=...)` is what actually arbitrates, so a
    # writer that lands between this read and that call still loses here
    # (409), not silently wins.
    cur_sha, cur_payload = store.read_with_sha(DESIRED_REF, fresh=True)
    if cur_sha != witness:
        handler._send_json(
            409,
            {"error": "conflict", "ref": DESIRED_REF, "sha": cur_sha,
             "detail": "the witness is stale; re-read, re-apply your edit, and PUT again"},
        )
        return
    doc = dict(body)
    doc["generation"] = _next_generation(cur_payload)
    result = store.update(DESIRED_REF, doc, expect_sha=witness)
    if result is False:
        handler._send_json(
            409,
            {"error": "conflict", "ref": DESIRED_REF,
             "detail": "lost the CAS; re-read, re-apply your edit, and PUT again"},
        )
        return
    handler._send_json(200, {"sha": result, "ref": DESIRED_REF, "payload": doc}, extra_headers={"ETag": f'"{result}"'})


def _parse_since_events(query: Dict[str, List[str]], headers) -> int:
    if query.get("since") and query["since"][0] != "":
        try:
            return max(0, int(query["since"][0]))
        except ValueError:
            return 0
    last_event_id = headers.get("Last-Event-ID")
    if last_event_id:
        try:
            return max(0, int(last_event_id))
        except ValueError:
            return 0
    return 0


def handle_events(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    server = handler.server
    last_seq = _parse_since_events(query, handler.headers)
    handler._begin_sse()
    last_keepalive = time.monotonic()
    keepalive_interval = server.config.sse_keepalive_interval
    # Registered for the duration of the stream so `server.stop()` can wait
    # for this thread to actually exit instead of racing it against
    # whatever the caller closes right after `stop()` returns (see
    # `register_stream_thread`'s docstring).
    current_thread = threading.current_thread()
    server.register_stream_thread(current_thread)
    try:
        while not server._stop_event.is_set():
            events = server.events.since(last_seq)
            for ev in events:
                handler._write_chunk(_format_sse(ev.seq, ev.kind, ev.payload))
                last_seq = ev.seq
            if events:
                continue
            due_in = keepalive_interval - (time.monotonic() - last_keepalive)
            if due_in <= 0:
                handler._write_chunk(_SSE_KEEPALIVE_LINE)
                last_keepalive = time.monotonic()
                continue
            server.events.wait_for_new(last_seq, timeout=min(due_in, 1.0))
    except (BrokenPipeError, ConnectionResetError, OSError):
        pass
    finally:
        handler.close_connection = True
        server.unregister_stream_thread(current_thread)


# -- election hooks (SPEC SS3.3 rule 8, SS3.4; PLAN Stage 2 task 5) ------ #
#
# These two handlers and the `election` attribute they read are the
# server-lease/settle/demotion surface `keel/election.py` plugs into.
# `server.election` is duck-typed (an `ElectionManager`, attached by
# `attach_election`) so the transport keeps importing nothing but
# `store_api` and its tests keep needing neither git nor a lease.


def handle_status(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    """`GET /v1/status`: the fleet as this server sees it, plus a `server`
    block. EVERYTHING about this server process -- its lease included --
    lives under the `server` key, and the server's own claim is excluded
    from the `claims` map, so the SS3.4 acceptance instrument (`status
    --json` diff with `del(.ts, .server)` across a kill + re-host) is
    empty exactly when two servers agree about the fleet."""
    server = handler.server
    desired = server.store.read("refs/fleet/desired")
    claims = {
        ref: entry.sha
        for ref, entry in sorted(server.store.list("refs/fleet/claims").items())
        if not ref.startswith("refs/fleet/claims/server/")
    }
    server_block: Dict[str, object] = {"boot_id": server.boot_id, "settling": False, "lease": None}
    if server.election is not None:
        server_block = {"boot_id": server.boot_id}
        server_block.update(server.election.status_fields())
    handler._send_json(
        200,
        {"ts": time.time(), "server": server_block, "desired": desired, "claims": claims},
    )


def handle_runner_register(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    """`POST /v1/runners/{id}/register` -- the Stage-2 skeleton of SS5.3
    step 1. What exists of it here is exactly what the unreachable-leader
    demotion timer needs: the registration is COUNTED (a runner reached
    our advertised address, so we are not unreachable) and answered with
    `{boot_id, settle_until, lease_expires_at}`. The capabilities and
    `live_workers[]` in the body are accepted and ignored until the
    Stage-3 runner protocol lands."""
    body = _read_json_body(handler)
    if body is None:
        handler._send_json(400, {"error": "invalid-json"})
        return
    server = handler.server
    reply: Dict[str, object] = {"boot_id": server.boot_id, "settle_until": None, "lease_expires_at": None}
    if server.election is not None:
        server.election.note_registration(params["id"])
        health = server.election.health_fields()
        reply["settle_until"] = health.get("settle_until")
        reply["lease_expires_at"] = health.get("lease_expires_at")
    handler._send_json(200, reply)


def handle_wait(handler: "KeelRequestHandler", params: Dict[str, str], query: Dict[str, List[str]], principal: Optional[Principal]) -> None:
    server = handler.server
    ref = query.get("ref", [None])[0]
    if not ref:
        handler._send_json(400, {"error": "missing-ref"})
        return
    since_values = query.get("since")
    since_val = since_values[0] if since_values and since_values[0] != "" else None
    deadline = time.monotonic() + server.config.long_poll_timeout
    poll_interval = server.config.long_poll_interval
    while True:
        # Always fresh: a long-poll answered from a stale index could
        # never observe the change it exists to detect (store_api.py).
        current = server.store.sha(ref, fresh=True)
        if current != since_val:
            handler._send_json(200, {"ref": ref, "sha": current})
            return
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            handler._send_status_only(204)
            return
        time.sleep(min(poll_interval, remaining))


def build_router() -> Router:
    router = Router()
    router.add("GET", "/v1/health", handle_health, auth=None)
    router.add("GET", "/v1/refs", handle_refs_list, auth=frozenset())
    router.add("GET", "/v1/refs/{ref:path}", handle_refs_get, auth=frozenset())
    router.add("POST", "/v1/refs/{ref:path}", handle_refs_post, auth=frozenset({"runner", "operator"}))
    router.add("PUT", "/v1/refs/{ref:path}", handle_refs_put, auth=frozenset({"runner", "operator"}))
    router.add("DELETE", "/v1/refs/{ref:path}", handle_refs_delete, auth=frozenset({"runner", "operator"}))
    router.add("GET", "/v1/events", handle_events, auth=frozenset())
    router.add("GET", "/v1/wait", handle_wait, auth=frozenset())
    # `desired` is READ by everyone with a token (a runner needs its own
    # targets) and WRITTEN only by the operator token class: SPEC SS3.1
    # names the writers `keel up/down/drain` + OPERATOR, and SS5 puts both
    # in the operator class. A runner token authenticates fine here and
    # gets 403 -- the distinction AuthError exists to make.
    router.add("GET", "/v1/desired", handle_desired_get, auth=frozenset())
    router.add("PUT", "/v1/desired", handle_desired_put, auth=frozenset({"operator"}))
    # Election hooks (see the comment block above `handle_status`).
    router.add("GET", "/v1/status", handle_status, auth=frozenset())
    router.add("POST", "/v1/runners/{id}/register", handle_runner_register, auth=frozenset({"runner", "operator"}))
    return router


# --------------------------------------------------------------------- #
# Server config
# --------------------------------------------------------------------- #


@dataclass(frozen=True)
class ServerConfig:
    bind_host: str = "127.0.0.1"
    port: int = DEFAULT_PORT
    allow_any_bind: bool = False
    max_connections: int = DEFAULT_MAX_CONNECTIONS
    long_poll_timeout: float = DEFAULT_LONG_POLL_TIMEOUT_S
    long_poll_interval: float = DEFAULT_LONG_POLL_INTERVAL_S
    sse_keepalive_interval: float = DEFAULT_SSE_KEEPALIVE_INTERVAL_S
    watchdog_timeout: float = DEFAULT_WATCHDOG_TIMEOUT_S
    watchdog_check_interval: float = DEFAULT_WATCHDOG_CHECK_INTERVAL_S


# --------------------------------------------------------------------- #
# The server
# --------------------------------------------------------------------- #


class KeelHTTPServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    # THE LISTEN BACKLOG MUST BE AT LEAST THE CONNECTION CAP.
    # `socketserver.TCPServer.request_queue_size` is 5, and it is the
    # argument `server_activate()` passes to `listen(2)`: the depth of the
    # kernel's completed-connection queue, i.e. how many connections may
    # sit fully established but not yet `accept()`ed. It is a completely
    # different quantity from `config.max_connections` (64, SPEC SS2 C6),
    # which bounds how many this process will SERVE at once -- and leaving
    # it at 5 caps the server far below its own cap in the one situation
    # that matters. Every `ServerHub` call opens a fresh connection (that
    # module's docstring: one connection per request buys phase certainty
    # at `connect()`), so a fleet-wide moment -- a settle ending, a
    # re-host, a burst of renewals coming due together -- arrives as N
    # simultaneous SYNs, not N requests on N idle keep-alive sockets.
    # Past the backlog the kernel drops or RSTs them, and the client sees
    # a connect-time failure, which is precisely the BEFORE-SEND phase
    # `FallbackHub` is entitled to route around (SPEC SS4.3 r2): a healthy,
    # reachable server silently stops being used. `test_serverhub.py`'s
    # own fixture had to subclass this class to raise the backlog for an
    # eight-process race (7 red runs in 8 at the default); that override
    # was evidence about production, not about the test.
    #
    # `server_activate` below raises this further when `max_connections`
    # is configured above it; the class attribute is the floor and keeps
    # `restart_accept_loop()`'s re-bind (which calls `server_activate()`
    # again) at the same depth.
    request_queue_size = DEFAULT_MAX_CONNECTIONS

    def server_activate(self) -> None:
        # Called by `TCPServer.__init__` (after `self.config` is set) and
        # again by `restart_accept_loop()`. The kernel silently clamps the
        # backlog to its own `somaxconn`; asking for the cap is still the
        # right ask.
        self.request_queue_size = max(type(self).request_queue_size, int(self.config.max_connections))
        super().server_activate()

    def __init__(
        self,
        server_address: Tuple[str, int],
        config: ServerConfig,
        store: "store_api.Store",
        tokens: TokenStore,
        events: EventLog,
        health_provider: Optional[Callable[[], Dict[str, object]]] = None,
        router: Optional[Router] = None,
    ):
        validate_bind_host(server_address[0], config.allow_any_bind)
        self.config = config
        self.store = store
        self.tokens = tokens
        self.events = events
        self.health_provider = health_provider or (lambda: {})
        self.router = router or build_router()
        self.boot_id = uuid.uuid4().hex
        self.boot_monotonic = time.monotonic()
        self.restart_count = 0
        self.last_healthy_probe_at = time.monotonic()
        self._conn_lock = threading.Lock()
        self._conn_count = 0
        self.total_connections_accepted = 0
        self._restart_lock = threading.Lock()
        self._stop_event = threading.Event()
        self._serve_thread: Optional[threading.Thread] = None
        self._watchdog_thread: Optional[threading.Thread] = None
        self._stream_lock = threading.Lock()
        self._stream_threads: set = set()
        # Election hook: an `ElectionManager` (keel/election.py), attached
        # by `attach_election` after construction. Duck-typed on purpose --
        # the transport never imports election.py (and vice-versa there is
        # no cycle); None means "no lease is wired in" (transport tests,
        # standalone smoke runs) and every reader must tolerate it.
        self.election = None
        super().__init__(server_address, KeelRequestHandler)

    def attach_election(self, manager) -> None:
        """Wire an `ElectionManager` in (keel/election.py). `/v1/health`
        gains its `lease_expires_at`/`settle_until` rows, `/v1/status` its
        `server` block, and `POST /v1/runners/{id}/register` starts
        feeding the unreachable-demotion timer."""
        self.election = manager

    # -- connection cap (SPEC SS2 C6: "caps the server at 64 connections") --

    def try_acquire_connection(self) -> bool:
        with self._conn_lock:
            if self._conn_count >= self.config.max_connections:
                return False
            self._conn_count += 1
            self.total_connections_accepted += 1
            return True

    def release_connection(self) -> None:
        with self._conn_lock:
            self._conn_count = max(0, self._conn_count - 1)

    def active_connection_count(self) -> int:
        with self._conn_lock:
            return self._conn_count

    # -- long-lived streaming handlers (SSE) --
    #
    # A `/v1/events` handler runs for as long as its client stays
    # connected, on its own per-connection thread -- entirely separate
    # from the accept-loop thread `shutdown()`/`server_close()` stop.
    # `stop()` sets `_stop_event`, which the SSE loop polls, but it can
    # still be blocked inside `EventLog.wait_for_new()` for up to a second
    # when that happens. Registering here lets `stop()` actually WAIT for
    # every such thread to notice and exit before returning, instead of
    # returning immediately and racing whatever the caller closes next
    # (e.g. the `EventLog` itself, as `test_server_transport.py` caught
    # with `ResourceWarning`s promoted to errors: a thread that woke up
    # after `EventLog.close()` had already run raised
    # `sqlite3.ProgrammingError: Cannot operate on a closed database`).

    def register_stream_thread(self, thread: threading.Thread) -> None:
        with self._stream_lock:
            self._stream_threads.add(thread)

    def unregister_stream_thread(self, thread: threading.Thread) -> None:
        with self._stream_lock:
            self._stream_threads.discard(thread)

    def handle_error(self, request, client_address) -> None:
        # A keep-alive client going away between requests (closing its
        # connection, or timing out an idle probe) makes the accept
        # thread's next `readline()` raise -- an expected, constant
        # occurrence for any long-lived HTTP/1.1 server, not a bug.
        # `socketserver`'s default `handle_error` prints a full traceback
        # to stderr for every one of these, which drowns out a real
        # handler defect in the noise. Anything else still gets the
        # default (loud) treatment.
        exc = sys.exc_info()[1]
        if isinstance(exc, (ConnectionResetError, BrokenPipeError, TimeoutError)):
            logging.debug("keel-server: benign connection error from %s: %r", client_address, exc)
            return
        super().handle_error(request, client_address)

    # -- lifecycle --

    def start(self) -> None:
        self._serve_thread = threading.Thread(target=self.serve_forever, name="keel-accept-loop", daemon=True)
        self._serve_thread.start()
        self._watchdog_thread = threading.Thread(target=self._watchdog_loop, name="keel-watchdog", daemon=True)
        self._watchdog_thread.start()

    def stop(self) -> None:
        self._stop_event.set()
        try:
            self.shutdown()
        except Exception:
            pass
        if self._serve_thread is not None:
            self._serve_thread.join(timeout=5)
        if self._watchdog_thread is not None:
            self._watchdog_thread.join(timeout=5)
        with self._stream_lock:
            stream_threads = list(self._stream_threads)
        for thread in stream_threads:
            thread.join(timeout=3)
            if thread.is_alive():
                logging.warning("keel-server: streaming handler thread %s did not exit within 3s of stop()", thread.name)
        try:
            self.server_close()
        except Exception:
            pass

    # -- listener watchdog (SPEC SS2 C6: "restarts the accept loop if it
    # stops accepting for 30 s") --

    def _probe_alive(self, timeout: float) -> bool:
        host, port = self.server_address[0], self.server_address[1]
        probe_host = host if host not in ("", "0.0.0.0") else "127.0.0.1"
        try:
            with socket.create_connection((probe_host, port), timeout=timeout):
                return True
        except OSError:
            return False

    def _watchdog_loop(self) -> None:
        self.last_healthy_probe_at = time.monotonic()
        interval = self.config.watchdog_check_interval
        probe_timeout = min(2.0, interval)
        while not self._stop_event.wait(interval):
            self._watchdog_tick(self._probe_alive(timeout=probe_timeout))

    def _watchdog_tick(self, alive: bool) -> None:
        """One watchdog decision, given whether the self-probe just
        succeeded. Split out from `_watchdog_loop` so the decision (probe
        failed for longer than `watchdog_timeout` -> restart) can be
        exercised directly, with a fake clock/probe/restart, instead of
        racing a real background thread against wall-clock sleeps."""
        now = time.monotonic()
        if alive:
            self.last_healthy_probe_at = now
            return
        if now - self.last_healthy_probe_at > self.config.watchdog_timeout:
            logging.warning(
                "keel-server watchdog: no successful accept probe in %.1fs; restarting accept loop",
                now - self.last_healthy_probe_at,
            )
            self.restart_accept_loop()
            self.last_healthy_probe_at = time.monotonic()

    def restart_accept_loop(self) -> None:
        """Stop the current `serve_forever()` loop, close and rebind the
        listening socket at the same address, and start a fresh loop.
        Safe to call whether or not the old accept-loop thread is still
        alive: `BaseServer.serve_forever()` sets its `_is_shut_down` event
        in a `finally`, so `shutdown()` returns promptly either way."""
        with self._restart_lock:
            old_thread, self._serve_thread = self._serve_thread, None
            try:
                self.shutdown()
            except Exception:
                logging.exception("keel-server: shutdown() during restart raised")
            if old_thread is not None:
                old_thread.join(timeout=5)
            try:
                self.server_close()
            except Exception:
                logging.exception("keel-server: server_close() during restart raised")
            self.socket = socket.socket(self.address_family, self.socket_type)
            self.server_bind()
            self.server_activate()
            self._serve_thread = threading.Thread(target=self.serve_forever, name="keel-accept-loop", daemon=True)
            self._serve_thread.start()
            self.restart_count += 1


class KeelRequestHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "keel-server/0.1"
    server: KeelHTTPServer  # narrows the inherited Any-typed attribute

    # -- connection admission (cap) --

    def setup(self) -> None:
        super().setup()
        self._connection_admitted = self.server.try_acquire_connection()

    def finish(self) -> None:
        try:
            super().finish()
        finally:
            if getattr(self, "_connection_admitted", False):
                self.server.release_connection()

    def handle(self) -> None:
        if not getattr(self, "_connection_admitted", False):
            self._reject_over_capacity()
            return
        super().handle()

    def _reject_over_capacity(self) -> None:
        # Read exactly one request off the wire before responding so the
        # client's own bytes are drained and the connection can be closed
        # cleanly (no RST race against unread input).
        self.close_connection = True
        try:
            self.raw_requestline = self.rfile.readline(65537)
            if self.raw_requestline and self.parse_request():
                self.send_response(HTTPStatus.SERVICE_UNAVAILABLE, "not-ready")
                self.send_header("Retry-After", "1")
                self.send_header("Content-Length", "0")
                self.send_header("Connection", "close")
                self.end_headers()
        except Exception:
            pass

    def log_message(self, fmt: str, *args) -> None:
        # Overridden only to route through `logging`. `fmt`/`args` here are
        # exactly what BaseHTTPRequestHandler already builds for its access
        # log -- the request line, status, and byte count -- never headers,
        # so an Authorization bearer token can never reach this line. Grep
        # this file for other `logging.*` calls to confirm none of them are
        # handed a token or a raw Authorization header either.
        logging.info("%s - %s", self.address_string(), fmt % args)

    # -- HTTP verbs --

    def do_GET(self) -> None:
        self._dispatch("GET")

    def do_PUT(self) -> None:
        self._dispatch("PUT")

    def do_DELETE(self) -> None:
        self._dispatch("DELETE")

    def do_POST(self) -> None:
        self._dispatch("POST")

    def _dispatch(self, method: str) -> None:
        parsed = urllib.parse.urlsplit(self.path)
        query = urllib.parse.parse_qs(parsed.query, keep_blank_values=True)
        route, params, path_exists = self.server.router.match(method, parsed.path)
        if route is None:
            status = 405 if path_exists else 404
            self._send_json(status, {"error": "method-not-allowed" if path_exists else "not-found"})
            return
        try:
            principal = self._authenticate(route.auth)
            route.handler(self, params, query, principal)
        except AuthError as exc:
            self._send_auth_error(exc)
        except StoreUnreachableError:
            self._send_json(503, {"error": "store-unreachable"})
        except (BrokenPipeError, ConnectionResetError):
            self.close_connection = True
        except Exception:
            logging.exception("keel-server: unhandled error handling %s %s", method, parsed.path)
            try:
                self._send_json(500, {"error": "internal-error"})
            except Exception:
                pass

    def _authenticate(self, required_roles: Optional[FrozenSet[str]]) -> Optional[Principal]:
        if required_roles is None:
            return None
        header = self.headers.get("Authorization", "")
        if not header:
            raise AuthError(401, "missing-authorization")
        scheme, _, token = header.partition(" ")
        if scheme.lower() != "bearer" or not token:
            raise AuthError(401, "malformed-authorization")
        principal = self.server.tokens.authenticate(token)
        if principal is None:
            raise AuthError(401, "invalid-token")
        if required_roles and principal.role not in required_roles:
            raise AuthError(403, "forbidden-role")
        return principal

    def _send_auth_error(self, exc: AuthError) -> None:
        extra = {"WWW-Authenticate": "Bearer"} if exc.status == 401 else None
        self._send_json(exc.status, {"error": exc.code}, extra_headers=extra)

    # -- response helpers --

    def _send_json(self, status: int, body: Dict[str, object], extra_headers: Optional[Dict[str, str]] = None) -> None:
        data = json.dumps(body).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        if extra_headers:
            for key, value in extra_headers.items():
                self.send_header(key, value)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(data)

    def _send_status_only(self, status: int) -> None:
        self.send_response(status)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def _begin_sse(self) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Transfer-Encoding", "chunked")
        self.send_header("Connection", "keep-alive")
        self.end_headers()

    def _write_chunk(self, data: bytes) -> None:
        self.wfile.write(f"{len(data):X}\r\n".encode("ascii"))
        self.wfile.write(data)
        self.wfile.write(b"\r\n")
        self.wfile.flush()


def build_server(
    config: ServerConfig,
    store: "store_api.Store",
    tokens: Optional[TokenStore] = None,
    events: Optional[EventLog] = None,
    health_provider: Optional[Callable[[], Dict[str, object]]] = None,
) -> KeelHTTPServer:
    # Validate the bind address BEFORE constructing anything with a real
    # side effect. `KeelHTTPServer.__init__` re-checks this too (so
    # constructing one directly is never unsafe), but by then a default
    # `events=None` here would already have opened a real sqlite file at
    # `default_events_db_path()` (typically `~/.keel/events.db`) for a
    # server that was never going to bind at all -- exactly the kind of
    # untracked disk write a test (or a caller) touching a real home
    # directory should never see as a side effect of a rejected config.
    validate_bind_host(config.bind_host, config.allow_any_bind)
    tokens = tokens if tokens is not None else TokenStore.empty()
    events = events if events is not None else EventLog(default_events_db_path())
    return KeelHTTPServer(
        (config.bind_host, config.port),
        config,
        store,
        tokens,
        events,
        health_provider=health_provider,
    )


# --------------------------------------------------------------------- #
# Standalone entrypoint -- smoke-testing / manual use only. Production
# wiring (once cachedhub.py/election.py exist) injects a real Store and
# health_provider instead of the InMemoryStore default here.
# --------------------------------------------------------------------- #


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(prog="keel-server", description="Keel server transport (SPEC SS2 C6).")
    parser.add_argument("--bind-host", default=os.environ.get("KEEL_BIND", "127.0.0.1"))
    parser.add_argument("--port", type=int, default=int(os.environ.get("KEEL_PORT", str(DEFAULT_PORT))))
    parser.add_argument(
        "--allow-any-bind",
        action="store_true",
        default=False,
        help="skip the loopback/Tailscale bind restriction (SPEC SS8); KEEL_ALLOW_PUBLIC_BIND=1 does the same",
    )
    parser.add_argument("--max-connections", type=int, default=DEFAULT_MAX_CONNECTIONS)
    parser.add_argument("--tokens-file", default=str(default_tokens_file_path()))
    parser.add_argument("--events-db", default=str(default_events_db_path()))
    parser.add_argument("--long-poll-timeout", type=float, default=DEFAULT_LONG_POLL_TIMEOUT_S)
    parser.add_argument("--sse-keepalive-interval", type=float, default=DEFAULT_SSE_KEEPALIVE_INTERVAL_S)
    parser.add_argument("--watchdog-timeout", type=float, default=DEFAULT_WATCHDOG_TIMEOUT_S)
    parser.add_argument("--log-level", default="INFO")
    # The spine. With --state-url the Store is a CachedHub over fleetlib.Hub
    # (keel/hubstore.py, the one adapter between the two surfaces); without
    # it the server runs on InMemoryStore and is a smoke test of the
    # transport only. Same env names fleetd/cli resolve (FLEET_HUB_URL is the
    # state repo, FLEET_CODE_URL the code repo); KEEL_STATE_URL wins.
    parser.add_argument(
        "--state-url",
        default=os.environ.get("KEEL_STATE_URL") or os.environ.get("FLEET_HUB_URL"),
        help="state repo remote answering refs/fleet/* (or KEEL_STATE_URL / FLEET_HUB_URL); "
        "absent = InMemoryStore, smoke-test only",
    )
    parser.add_argument(
        "--state-workdir",
        default=os.environ.get("KEEL_STATE_WORKDIR"),
        help="disposable local object cache for the state repo (default: $KEEL_HOME/state.git)",
    )
    parser.add_argument(
        "--code-url",
        default=os.environ.get("FLEET_CODE_URL"),
        help="code repo remote (or FLEET_CODE_URL; default: same as --state-url)",
    )
    parser.add_argument(
        "--sweep-interval",
        type=float,
        default=float(os.environ.get("KEEL_SWEEP_INTERVAL", "30")),
        help="seconds between whole-namespace index sweeps (0 = never; boot still sweeps once)",
    )
    args = parser.parse_args(argv)

    logging.basicConfig(level=getattr(logging, args.log_level.upper(), logging.INFO), format="%(asctime)s %(levelname)s %(message)s")

    config = ServerConfig(
        bind_host=args.bind_host,
        port=args.port,
        allow_any_bind=args.allow_any_bind,
        max_connections=args.max_connections,
        long_poll_timeout=args.long_poll_timeout,
        sse_keepalive_interval=args.sse_keepalive_interval,
        watchdog_timeout=args.watchdog_timeout,
    )

    tokens_path = Path(args.tokens_file)
    if tokens_path.exists():
        tokens = TokenStore.from_file(tokens_path)
    else:
        tokens = TokenStore.empty()
        logging.warning("keel-server: no tokens file at %s -- every authenticated route will 401", tokens_path)

    # Validate the bind BEFORE anything with a side effect: building the
    # index creates the state workdir on disk and costs a round trip to the
    # spine, and a rejected bind should leave no trace of either.
    try:
        validate_bind_host(config.bind_host, config.allow_any_bind)
    except ValueError as exc:
        logging.error("keel-server: %s", exc)
        return 2

    store: "store_api.Store"
    health_provider: Optional[Callable[[], Dict[str, object]]] = None
    close_store: Callable[[], None] = lambda: None
    if args.state_url:
        # Imported here, not at module top: server.py's import graph stays
        # free of cachedhub/fleetlib so the transport tests need neither
        # (store_api.py's docstring); only the production wiring pays for it.
        import hubstore  # noqa: E402

        workdir = Path(args.state_workdir) if args.state_workdir else _keel_home() / "state.git"
        try:
            hub_store = hubstore.build_store(
                args.state_url, workdir, code_url=args.code_url, sweep_interval=args.sweep_interval
            )
        except hubstore.HubError as exc:
            logging.error("keel-server: cannot build the index from %s: %s", args.state_url, exc)
            return 3
        store = hub_store
        health_provider = hub_store.health
        close_store = hub_store.close
        logging.info(
            "keel-server: CachedHub over %s (workdir %s, %d refs indexed, sweep every %ss)",
            args.state_url, workdir, hub_store.health()["index_refs"], args.sweep_interval,
        )
    else:
        store = store_api.InMemoryStore()
        logging.warning(
            "keel-server: running with InMemoryStore -- no real spine is wired in "
            "(no --state-url / KEEL_STATE_URL / FLEET_HUB_URL). Smoke-test path only."
        )

    events = EventLog(Path(args.events_db))
    try:
        server = build_server(config, store=store, tokens=tokens, events=events, health_provider=health_provider)
    except ValueError as exc:
        logging.error("keel-server: %s", exc)
        close_store()
        return 2

    server.start()
    logging.info("keel-server listening on %s:%s (boot_id=%s)", config.bind_host, config.port, server.boot_id)
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    finally:
        server.stop()
        close_store()
    return 0


if __name__ == "__main__":
    sys.exit(main())
