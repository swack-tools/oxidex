#!/usr/bin/env python3
"""`ServerHub` -- the CLI/runner-side HTTP client for `keel-server`'s
`/v1/refs*` CAS facade (docs/AGENT-SERVER-SPEC.md SS4.2, component C4;
routes in `keel/server.py`; contract in `keel/store_api.py`).

WHY THIS FILE EXISTS. PLAN Stage 2's task table lists "ServerHub" as its
own row, separate from the CLI task this module was written for -- but no
sibling branch delivering `keel/serverhub.py` exists yet (checked:
`git branch -a` has no `staging/keel-2-serverhub`, and neither
`keel-2-cachedhub`, `keel-2-fallbackhub` nor `keel-2-http` touches this
file). `FallbackHub(primary, github)` is meaningless with no `primary`
that speaks the wire protocol `keel/server.py` actually serves, and the
CLI's own acceptance test ("status via server and --direct agree") has
no way to exist without one. This is the minimal client that satisfies
exactly what `FallbackHub` calls on its `primary` (SPEC SS4.3's own
"WHAT ROUTES WHERE": `sha, read, read_with_sha, list, fetch_namespace,
create, update, delete` -- never `code_sha`/`code_list`/`push_ref`/etc,
which FallbackHub always sends to the GitHub half). A fuller
`test_serverhub.py` (409/404/503 mapping matrix, ProcessPool
one-winner-through-the-server, PLAN Stage 2 task row) is out of this
task's file ownership (`tools/fleet/keel/cli.py` +
`tests/test_keel_cli.py` only) and should be reconciled/deduped against
this file if a dedicated ServerHub task lands separately later.

Implements exactly the `fleetlib.Hub` surface `FallbackHub` touches on a
primary. Everything else (`code_sha`, `code_list`, `push_code_ref`, ...)
is GitHubHub-only by design and is simply absent here -- `FallbackHub`
never calls it on `primary`, so there is nothing to stub.

SPEC SS4.2's HTTP mapping, mechanical:
  * `GET /v1/refs/{ref}` 200 -> `(sha, payload)`; 404 -> `(None, None)`.
  * `PUT` + `If-None-Match: *` (create) 201 -> True; 409 -> False.
  * `PUT` + `If-Match: <sha>` (update) 200 -> True; 409 -> False.
  * `DELETE` + `If-Match: <sha>` 204 -> True; 409 -> False.
  * `GET /v1/refs?prefix=` -> `{ref: sha}` (the wire shape is the richer
    `{ref:{sha,observed_at}}`, SPEC SS5.1; reduced here to match
    `fleetlib.Hub.list`/`fetch_namespace`'s `{ref: sha}` contract so this
    is a structural drop-in for either -- every existing consumer of
    `hub.list(...)` in this tree only ever iterates the keys).
  * 401/403/5xx/connect-error/timeout -> raised, NEVER read as 404
    (absent) or 409 (lost race): SPEC SS4.3's whole point is that a
    transport failure must never be conflated with either.
`create`/`update` return a plain bool, matching `fleetlib.Hub.create`/
`.update`'s own `-> bool` contract (not the sha the server's wire
response also carries) -- every `Hub`-shaped caller in this tree
(`claim.py`'s CAS loop, `cli.py`'s `_edit_desired`) only ever checks
truthiness.

Every failure is raised as `fallbackhub.PrimaryFailure` with
`request_sent` set per the vocabulary `fallbackhub.classify_primary_failure`
documents: `False` when this module KNOWS the store operation cannot have
run (a `_connect()` failure, before any bytes were even sent; or the
server's own `503 not-ready`, which SPEC SS4.3 rule 2 and
`classify_primary_failure`'s docstring both name as the one HTTP status
that means "answered, executed nothing"), `True` for everything else,
including every other 5xx, a 401/403, and any failure once `conn.request()`
has started sending (a dropped connection or a read timeout cannot say
whether the CAS ran). This is plumbing, not policy: `FallbackHub` is what
decides whether a write may be retried against GitHub (SPEC SS4.3 rule
2); this module only tells it, precisely, which side of that line a
given failure fell on, rather than leaving `FallbackHub` to guess from a
bare `HubUnreachableError`.

Connect timeout 5 s, read timeout 20 s (SPEC SS4.2), tracked
independently because `http.client.HTTPConnection`'s one `timeout=`
constructor argument only ever governs `connect()`; a longer read
timeout is set on the live socket afterwards, once, and used for every
read on that connection (a fresh one is opened per call -- no
keep-alive reuse across calls, since none of `sha`/`read`/`create`/... is
called back-to-back often enough on the CLI/runner side to be worth the
lifecycle complexity `server.py`'s own keep-alive server SIDE already
provides).

`events()` is not part of the `Hub` contract -- it is this module's
answer to `GET /v1/events` (SPEC SS5.1), the one route `keel events`
needs that no bare `fleetlib.Hub` has an equivalent for (the event ring
is server-only, SPEC SS3.2: "lossy by design", never replicated to the
state repo). It yields `(seq, kind, payload)` tuples read off the
chunked SSE stream via `http.client.HTTPResponse.readline()`, which
already de-chunks (`server.py`'s `_write_chunk` writes standard HTTP
chunked framing, and `http.client` decodes it transparently regardless
of which method reads it -- verified against a live `KeelHTTPServer`
before this file was written the other way). `follow=False` (the
default, "catch me up") treats an idle-timeout as "nothing more is
queued right now" and simply stops iterating; `follow=True` treats the
same idle-timeout as a real problem (the server's own 15 s keepalive,
SPEC SS5.1, should have arrived long before a `follow`-sized timeout
elapses) and raises.

Standard library only, like everything under `tools/fleet/keel/`.
"""

from __future__ import annotations

import http.client
import json
import socket
import sys
import urllib.parse
from pathlib import Path
from typing import Dict, Iterator, List, Optional, Sequence, Tuple, Union

_KEEL_DIR = Path(__file__).resolve().parent
_FLEET_DIR = _KEEL_DIR.parent
for _p in (_FLEET_DIR, _KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

# Qualified, not `from fallbackhub import ...`: `fallbackhub.py` lives
# inside `keel/`, and `hubstore.py` already established the convention
# that a `keel/`-internal file importing a `keel/` sibling does so via
# `keel.<name>` -- never the bare name. The bare form would still resolve
# (both directories are on sys.path), but it would do so by creating a
# SECOND, distinct module object under `sys.modules["fallbackhub"]`
# alongside `sys.modules["keel.fallbackhub"]` (whichever some other file
# imports it as) -- two classes named `PrimaryFailure`, and an
# `isinstance`/`except` check written against one is silently false
# against an exception raised as the other. Matching the one path every
# other `keel/`-internal cross-import already uses is what keeps there
# being only one `PrimaryFailure` class in the process.
from keel.fallbackhub import PrimaryFailure  # noqa: E402

__all__ = [
    "ServerHub",
    "ServerHubHTTPError",
    "DEFAULT_CONNECT_TIMEOUT_S",
    "DEFAULT_READ_TIMEOUT_S",
]

# SPEC SS4.2: "Connect 5 s, read 20 s."
DEFAULT_CONNECT_TIMEOUT_S = 5.0
DEFAULT_READ_TIMEOUT_S = 20.0

# The one HTTP status classify_primary_failure's docstring (fallbackhub.py)
# names as BEFORE_SEND: "the server answered 'not ready' and executed
# nothing" (SPEC SS4.3 rule 2). Every other status this module raises for
# is AMBIGUOUS (request_sent=True) -- deliberately not extended to 401/403
# on this module's own judgment; that is `classify_primary_failure`'s
# vocabulary to extend, not this client's to assume.
_NOT_READY_STATUS = 503


class ServerHubHTTPError(PrimaryFailure):
    """A `PrimaryFailure` raised by `ServerHub` specifically -- carries
    nothing `PrimaryFailure` does not already (`status`, `request_sent`);
    exists only so a `except ServerHubHTTPError` can tell "this came from
    the HTTP client" apart from a `PrimaryFailure` some other primary
    raised, without inspecting `type(exc).__module__`.
    """


def _quote_ref(ref: str) -> str:
    # Refs are slash-shaped ("refs/fleet/claims/gate/x"); server.py's
    # `{ref:path}` route pattern matches the slashes literally, so encode
    # everything but them.
    return urllib.parse.quote(ref, safe="/")


def _parse_sse_frame(lines: List[str]) -> Optional[Tuple[int, str, dict]]:
    """One SSE frame's lines (blank-line-terminated, blank line itself
    excluded) -> `(seq, kind, payload)`, or `None` for a frame with no
    `id`/`event` (a bare `: keepalive` comment, `server.py`'s
    `_SSE_KEEPALIVE_LINE`)."""
    seq: Optional[int] = None
    kind: Optional[str] = None
    data_raw: Optional[str] = None
    for line in lines:
        if not line or line.startswith(":"):
            continue  # SSE comment -- keepalive
        field, _, value = line.partition(":")
        if value.startswith(" "):
            value = value[1:]
        if field == "id":
            try:
                seq = int(value)
            except ValueError:
                pass
        elif field == "event":
            kind = value
        elif field == "data":
            data_raw = value
    if seq is None or kind is None:
        return None
    try:
        payload = json.loads(data_raw) if data_raw else {}
    except json.JSONDecodeError:
        payload = {"raw": data_raw}
    return (seq, kind, payload)


class ServerHub:
    """The HTTP half of `FallbackHub(primary=ServerHub(...), github=...)`.

    `base_url` is `http://host:port` (a trailing slash, if any, is
    stripped). `token` is the raw bearer token -- never logged, never put
    in an exception message anywhere in this module; `None` sends no
    `Authorization` header at all, which is exactly what `/v1/health`
    wants (SPEC SS5.1: no auth) and everything else here does not use.
    """

    def __init__(
        self,
        base_url: str,
        token: Optional[str] = None,
        *,
        connect_timeout: float = DEFAULT_CONNECT_TIMEOUT_S,
        read_timeout: float = DEFAULT_READ_TIMEOUT_S,
    ):
        parsed = urllib.parse.urlsplit(base_url)
        if parsed.scheme not in ("http", "https") or not parsed.netloc:
            raise ValueError(f"ServerHub needs an absolute http(s) base_url, got {base_url!r}")
        self.base_url = f"{parsed.scheme}://{parsed.netloc}"
        self._scheme = parsed.scheme
        self._host = parsed.hostname or "127.0.0.1"
        self._port = parsed.port or (443 if parsed.scheme == "https" else 80)
        self._token = token
        self.connect_timeout = float(connect_timeout)
        self.read_timeout = float(read_timeout)

    # ------------------------------------------------------------------ #
    # Hub-shaped reads (SPEC SS4.3: always what FallbackHub tries first)
    # ------------------------------------------------------------------ #

    def sha(self, ref: str) -> Optional[str]:
        return self.read_with_sha(ref)[0]

    def read(self, ref: str) -> Optional[dict]:
        return self.read_with_sha(ref)[1]

    def read_with_sha(self, ref: str) -> Tuple[Optional[str], Optional[dict]]:
        status, body = self._request("GET", f"/v1/refs/{_quote_ref(ref)}")
        if status == 404:
            return (None, None)
        if status == 200:
            return (body.get("sha"), body.get("payload"))
        raise self._http_error("GET", ref, status, body)

    def list(self, prefix: str) -> Dict[str, str]:
        status, body = self._request("GET", "/v1/refs", query={"prefix": prefix})
        if status == 200:
            return {ref: entry.get("sha") for ref, entry in (body or {}).items()}
        raise self._http_error("GET", prefix, status, body)

    def fetch_namespace(self, prefix: str) -> Dict[str, str]:
        # store_api.py: no dedicated route exposes a raw namespace fetch;
        # `list()`'s `?prefix=` already answers "the ref AT prefix and
        # every ref UNDER it" (server.py's `handle_refs_list` forwards
        # straight to `Store.list`), which is exactly what
        # `fetch_namespace` promises.
        return self.list(prefix)

    # ------------------------------------------------------------------ #
    # Hub-shaped writes (SPEC SS4.3 rule 2 governs retry, not this class)
    # ------------------------------------------------------------------ #

    def create(self, ref: str, payload: dict, push_options: Optional[Sequence[str]] = None) -> bool:
        del push_options  # GitHubHub-only concept (git push -o); the server has none.
        status, body = self._request(
            "PUT", f"/v1/refs/{_quote_ref(ref)}", body=payload, headers={"If-None-Match": "*"},
        )
        if status == 409:
            return False
        if status == 201:
            return True
        raise self._http_error("PUT", ref, status, body)

    def update(
        self, ref: str, payload: dict, expect_sha: str, push_options: Optional[Sequence[str]] = None,
    ) -> bool:
        del push_options
        status, body = self._request(
            "PUT", f"/v1/refs/{_quote_ref(ref)}", body=payload, headers={"If-Match": expect_sha},
        )
        if status == 409:
            return False
        if status == 200:
            return True
        raise self._http_error("PUT", ref, status, body)

    def delete(self, ref: str, expect_sha: str, push_options: Optional[Sequence[str]] = None) -> bool:
        del push_options
        status, body = self._request(
            "DELETE", f"/v1/refs/{_quote_ref(ref)}", headers={"If-Match": expect_sha},
        )
        if status == 409:
            return False
        if status == 204:
            return True
        raise self._http_error("DELETE", ref, status, body)

    # ------------------------------------------------------------------ #
    # Not the server's job (SPEC SS4.2: "branch pushes never go through
    # the server")
    # ------------------------------------------------------------------ #

    def push_ref(self, *_args, **_kwargs):
        raise NotImplementedError(
            "ServerHub.push_ref: branch pushes never go through the server (SPEC SS4.2)"
        )

    # ------------------------------------------------------------------ #
    # Extras beyond the bare Hub contract: health, events
    # ------------------------------------------------------------------ #

    def health(self) -> dict:
        """`GET /v1/health` -- no auth (SPEC SS5.1)."""
        status, body = self._request("GET", "/v1/health", authed=False)
        if status == 200:
            return body
        raise self._http_error("GET", "/v1/health", status, body)

    def events(
        self, since: int = 0, *, follow: bool = False, timeout: Optional[float] = None,
    ) -> Iterator[Tuple[int, str, dict]]:
        """`GET /v1/events?since=` as an iterator of `(seq, kind,
        payload)`. `follow=False` (default) stops the first time no new
        line arrives within the idle timeout ("caught up"); `follow=True`
        keeps blocking for the next event and treats the same timeout as
        a real failure (SPEC SS5.1's 15 s keepalive should have arrived
        long before it). `timeout` overrides the computed idle timeout
        either way -- mainly for tests that want a fast bound in
        `follow=True` mode without waiting out the real default.
        """
        idle_timeout = timeout if timeout is not None else (self.read_timeout if follow else 1.5)
        conn = self._connect()
        try:
            try:
                conn.request("GET", f"/v1/events?since={int(since)}", headers=self._headers(authed=True))
                resp = conn.getresponse()
            except (OSError, http.client.HTTPException) as exc:
                raise self._wrap_transport_error("GET", "/v1/events", exc, before_send=False) from exc
            if resp.status != 200:
                raw = resp.read()
                raise self._http_error("GET", "/v1/events", resp.status, self._maybe_json(raw))
            conn.sock.settimeout(idle_timeout)
            frame: List[str] = []
            while True:
                try:
                    raw_line = resp.readline()
                except socket.timeout as exc:
                    if follow:
                        raise self._wrap_transport_error(
                            "GET", "/v1/events", exc, before_send=False
                        ) from exc
                    return
                if raw_line == b"":
                    return  # server closed the stream
                line = raw_line.decode("utf-8", "replace").rstrip("\r\n")
                if line == "":
                    parsed = _parse_sse_frame(frame)
                    frame = []
                    if parsed is not None:
                        yield parsed
                    continue
                frame.append(line)
        finally:
            conn.close()

    # ------------------------------------------------------------------ #
    # Transport
    # ------------------------------------------------------------------ #

    def _connect(self) -> http.client.HTTPConnection:
        cls = http.client.HTTPSConnection if self._scheme == "https" else http.client.HTTPConnection
        try:
            conn = cls(self._host, self._port, timeout=self.connect_timeout)
            conn.connect()
        except (OSError, http.client.HTTPException) as exc:
            raise self._wrap_transport_error("CONNECT", "-", exc, before_send=True) from exc
        return conn

    def _headers(self, *, authed: bool) -> Dict[str, str]:
        headers = {"Accept": "application/json"}
        if authed and self._token:
            headers["Authorization"] = f"Bearer {self._token}"
        return headers

    def _request(
        self,
        method: str,
        path: str,
        *,
        query: Optional[Dict[str, str]] = None,
        body: Optional[dict] = None,
        headers: Optional[Dict[str, str]] = None,
        authed: bool = True,
    ) -> Tuple[int, Union[dict, list]]:
        url_path = path
        if query:
            url_path = f"{path}?{urllib.parse.urlencode(query)}"
        hdrs = self._headers(authed=authed)
        if headers:
            hdrs.update(headers)
        data: Optional[bytes] = None
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            hdrs["Content-Type"] = "application/json"
            hdrs["Content-Length"] = str(len(data))
        conn = self._connect()
        try:
            conn.sock.settimeout(self.read_timeout)
            try:
                conn.request(method, url_path, body=data, headers=hdrs)
                resp = conn.getresponse()
                raw = resp.read()
            except (OSError, http.client.HTTPException) as exc:
                raise self._wrap_transport_error(method, url_path, exc, before_send=False) from exc
            return resp.status, self._maybe_json(raw)
        finally:
            conn.close()

    def _wrap_transport_error(
        self, method: str, path: str, exc: BaseException, *, before_send: bool,
    ) -> PrimaryFailure:
        return ServerHubHTTPError(
            f"{method} {path} against {self.base_url}: {type(exc).__name__}: {exc}",
            request_sent=not before_send,
            status=None,
        )

    def _http_error(self, method: str, path: str, status: int, body) -> PrimaryFailure:
        detail = ""
        if isinstance(body, dict) and body.get("error"):
            detail = f" ({body['error']})"
        return ServerHubHTTPError(
            f"{method} {path} against {self.base_url} -> HTTP {status}{detail}",
            request_sent=(status != _NOT_READY_STATUS),
            status=status,
        )

    @staticmethod
    def _maybe_json(raw: bytes) -> Union[dict, list]:
        if not raw:
            return {}
        try:
            return json.loads(raw.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            return {"raw": raw[:200].decode("utf-8", "replace")}
