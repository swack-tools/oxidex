#!/usr/bin/env python3
"""The runner-local job journal (PLAN Stage 3 task 2, SPEC §5.3).

WHAT THIS IS FOR. `keel-runner` (SPEC §2 C7) starts, and must answer one
question before it does anything else: *what is already running on this
host that belongs to me?* Today `fleetd` answers it from the store alone
(`fleetd.adopt_workers`, L1407-1667), and when the store cannot be
reached it refuses to start at all -- `main` L2384-2394, exit code 5:

    an unreachable hub at startup is not a reason to run with an empty
    worker list -- that is the state that starts duplicate gates.

That reasoning is correct and this module does not weaken it. What it
does is remove the false dichotomy underneath it. "Run with an empty
worker list" was only ever the alternative because the store was the
*only* place the runner had ever written down what it launched. A
process this runner spawned, in a session it created, carrying a scope
token it derived, is not knowledge that has to make a network round trip
to be recoverable. This module writes it down locally first, so that a
runner whose store is away has evidence instead of a guess -- and so the
rc 5 path can go (Judge 2's "runner refuses to start with both routes
down", SPEC §12).

THE ONE RULE: THE JOURNAL IS EVIDENCE, NEVER AUTHORITY.

Stated as a property of the two actions a startup pass can take:

  * It can only ever SUPPRESS an action, never authorize one. An
    unreadable journal disarms the orphan sweep (SPEC §10 I6: "journal
    present/absent/unreadable ⇒ disarmed" -- the disarm attaches to the
    last of those three, since a host that has never run a job has no
    journal and must still be able to sweep). A readable journal adds
    nothing to the sweep's kill list: a process we journaled whose claim
    is gone from the store is *exactly* the unleased work M8's kill
    exists for, and letting a local file veto that would reintroduce the
    duplicate-gate hazard leases exist to prevent.
  * The single place it is DECISIVE is adoption with both routes down
    (`adopt_from_journal`), and even there every entry must still clear
    the same evidence bar `fleetd.adopt_workers` applies to a claim
    payload: the recorded process group is alive, and some live same-uid
    member of it carries a worker marker AND this runner's own scope
    token (`fleetd._scoped_worker_in_group`). A pgid is a recycled name
    otherwise. The journal says which pgid to look at; `ps` says whether
    it is ours.

The two dispositions the task statement fixes, both implemented below:
a journal entry whose pgid is gone is RELEASED, never adopted (the CAS
delete is deferred to `release_pending` when no route exists to make
it); a journal entry whose claim is held by ANOTHER host is refused --
never adopted, and never released either, because deleting another
host's live claim is the one thing `Claim.adopt`'s own docstring calls
"the single most important thing this method does NOT do".

FILE LAYOUT. One append-only JSONL file per job, under
`~/.keel/journal/<stem>.jsonl`, one JSON object per line, `flush()` +
`os.fsync()` on every record (and an `fsync` of the directory itself the
first time a file is created, so the directory entry survives the same
crash the record does). The task statement specifies "append-only JSONL
under ~/.keel/journal/"; SPEC §2 C7 and §5.3 spell the same store
`~/.keel/jobs/<claim_key>.json`. The split-per-job addressing is the
spec's, the append-only JSONL body and the directory name are the task's
-- so a job is still addressed by its claim key, and the history of that
job is a log rather than a file rewritten in place. Rewriting in place
is what a journal must not do: `os.replace` of a whole file loses the
previous state at exactly the moment (a crash mid-spawn) the journal is
being consulted for.

EVENTS, in the order a job produces them:

    offer    the runner has decided to run this and is about to claim.
             SPEC §5.3 puts the journal write BEFORE `Claim.acquire_or_reap`
             ("write journal entry → Claim.acquire_or_reap → spawn"), so
             this record exists to be the thing written first. It carries
             no pgid and no sha, and a job that never got past it is never
             adoptable -- nothing was spawned.
    claim    the lease is held: `claim_ref`, `claim_sha`, `started_at`
             (the ownership token's second half, recorded as the EXACT
             string in the payload -- see `started_at_of`), `expires_at`,
             and the `gate_version`/`rustc_id`/`platform_id` the payload
             carries, so a rebuilt claim restores them rather than
             re-measuring them under the wrong PATH (invariant I15).
    spawn    `pid`/`pgid` of the process group, plus the `scope_token`
             stamped into its argv. Written by the runner immediately
             after `Popen` returns and BEFORE the post-spawn `renew` that
             persists the pgid into the claim payload, so the crash
             window in which a process exists that nothing knows about is
             the width of one `write()+fsync`.
    verdict  what the gate stored (`outcome`, `tree`, `rc`), for `keel
             why` and for a human reading the file. Never consulted by
             adoption.
    exit     the job is over. This CLOSES the job: a closed job is never
             adopted, never released, and never contributes to anything.

Renewals are deliberately NOT journaled: `claim_sha` and `expires_at` go
stale the moment the renewer runs, and both are handled by being treated
as stale (see `rebuild_claim`), rather than by writing a record every
`renew_interval` for every job forever.

WHAT THIS MODULE DOES NOT DO. It never talks to the store on its own
(`release_pending` is handed a hub by its caller), never spawns or kills
anything, and never decides scheduling. `adopt_at_startup` is the entire
surface `keel/runner.py` needs -- one call, in place of `fleetd.main`'s
`adopt_workers(...)` / `except HubError: return 5` pair.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import sys
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Callable, Dict, List, NamedTuple, Optional, Sequence, Tuple

_KEEL_DIR = Path(__file__).resolve().parent
_FLEET_DIR = _KEEL_DIR.parent
for _p in (_FLEET_DIR, _KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

import claim as claim_mod  # noqa: E402
from claim import Claim  # noqa: E402
from fleetlib import HubError  # noqa: E402

# The SAME formatter/parser `claim._owns` compares with. `(holder_host,
# started_at)` is an ownership token made of the payload's literal TEXT;
# a second ISO implementation in this file that differed by a trailing
# "Z", a "+00:00", or six microsecond digits instead of three would make
# every journal-rebuilt claim fail to recognize its own renewals. See
# `Claim.adopt`'s `_iso(started) != raw_started` guard, which this module
# reproduces in `rebuild_claim` for the same reason.
_iso = claim_mod._iso
_parse_iso = claim_mod._parse_iso

DEFAULT_ROOT = Path.home() / ".keel" / "journal"

#: Bumped whenever the meaning of an existing field changes or a new
#: EVENT is added. Version 2 permits another run after an exit in the same
#: file; version 1 readers consider any exit final. Version 3 records a
#: kernel birth token with each new spawn so an exec cannot be mistaken for
#: recycled PGID. A record whose `v` is
#: greater than this makes its file unreadable rather than partially
#: understood -- an older runner rolled
#: back onto a host a newer one journaled must fail closed, not quietly
#: skip the records it does not recognize.
SCHEMA_VERSION = 3

OFFER = "offer"
CLAIM = "claim"
SPAWN = "spawn"
VERDICT = "verdict"
EXIT = "exit"

#: The closed set of events for `SCHEMA_VERSION`. Unknown ⇒ unreadable.
EVENTS = (OFFER, CLAIM, SPAWN, VERDICT, EXIT)

SUFFIX = ".jsonl"

#: How long after `exit` a closed job's file is kept for a human (and
#: `keel why`) before `prune` removes it. Seven days: long enough to
#: cover a weekend's worth of "what ran here on Friday", short enough
#: that the directory never becomes a thing anyone has to think about.
DEFAULT_RETENTION_S = 7 * 24 * 3600

_SAFE_STEM = re.compile(r"\A[A-Za-z0-9][A-Za-z0-9._-]{0,99}\Z")
_UNSAFE_RUN = re.compile(r"[^A-Za-z0-9._-]+")


class JournalError(Exception):
    """Base for this module. Never raised for an ABSENT journal."""


class JournalWriteError(JournalError):
    """A record could not be appended and fsynced.

    THE CALLER MUST NOT SPAWN. A process this runner cannot journal is a
    process it can never adopt: after the next restart it is a live
    process group with no local record and (until the post-spawn renew
    lands) no pgid in the claim payload either, which is precisely the
    shape the orphan sweep kills. Claim-before-launch already says the
    lease comes first; this says the same about the local record.
    """


@dataclass(frozen=True)
class JobState:
    """The latest run of one job, folded from its file's records.

    Every field belongs to this run. Earlier runs remain in the append-only
    file but cannot supply ownership, process, or result fields to this one.
    """

    job_key: str
    path: str
    kind: str = "gate"
    work_key: str = ""
    tag: str = ""
    workdir: Optional[str] = None
    claim_ref: Optional[str] = None
    claim_sha: Optional[str] = None
    holder_host: Optional[str] = None
    started_at: Optional[str] = None  # EXACT payload text, never reformatted
    expires_at: Optional[str] = None
    pid: Optional[int] = None
    pgid: Optional[int] = None
    scope_token: Optional[str] = None
    leader_birth: Optional[str] = None
    gate_version: str = ""
    # The toolchain ids the CLAIM was written with. Restored rather than
    # recomputed: they were measured under the GATE's PATH, not the
    # runner's (invariant I15), and recomputing shells out to rustc for
    # every adopted job at startup -- on the one code path whose whole
    # premise is that things are already going wrong.
    rustc_id: Optional[str] = None
    platform_id: Optional[str] = None
    outcome: Optional[str] = None
    rc: Optional[int] = None
    closed: bool = False
    events: Tuple[str, ...] = ()
    first_ts: Optional[str] = None
    last_ts: Optional[str] = None
    #: The file's final line was incomplete (no terminating newline) and
    #: was dropped. Tolerated, but it disarms the sweep -- see `JournalScan`.
    torn: bool = False
    prior_runs: int = 0

    @property
    def open(self) -> bool:
        return not self.closed

    @property
    def spawned(self) -> bool:
        """Did this job get as far as a real process group?"""
        return isinstance(self.pgid, int) and self.pgid > 1


@dataclass(frozen=True)
class JournalScan:
    """Everything one read of the journal directory established.

    `unreadable` is the fail-closed signal and the reason this is a
    result object rather than an exception: a caller has to be able to
    see BOTH the jobs it could read and the fact that the read was
    incomplete, because the correct response to the second is to disarm
    an action rather than to abort the pass.
    """

    root: str
    jobs: Tuple[JobState, ...] = ()
    #: (path, error) -- a file whose records could not be read or parsed,
    #: or a directory that could not be listed.
    unreadable: Tuple[Tuple[str, str], ...] = ()
    #: (path, dropped_bytes) -- a file whose final line was incomplete.
    torn: Tuple[Tuple[str, int], ...] = ()

    @property
    def readable(self) -> bool:
        """False ⇒ adopt nothing from the journal and sweep nothing."""
        return not self.unreadable

    @property
    def sweep_armed(self) -> bool:
        """May a startup pass kill anything at all this time?

        A torn tail disarms as well as an unreadable file. The dropped
        record is by construction the MOST RECENT one, so the tail is
        exactly where a `spawn` (the record that names a pgid) goes
        missing -- and a pgid we did not learn is the one input the kill
        decision cannot be taken without. Same argument, one level out,
        as `adopt_workers`' unreadable-claim disarm: sweeping anyway is
        the unrecoverable direction.
        """
        return self.readable and not self.torn

    @property
    def open_jobs(self) -> Tuple[JobState, ...]:
        return tuple(j for j in self.jobs if j.open)

    def job(self, job_key: str) -> Optional[JobState]:
        for j in self.jobs:
            if j.job_key == job_key:
                return j
        return None

    def why_not_readable(self) -> str:
        return "; ".join(f"{p}: {e}" for p, e in self.unreadable)

    def why_not_armed(self) -> str:
        parts = [f"{p}: {e}" for p, e in self.unreadable]
        parts += [f"{p}: truncated final record ({n} bytes dropped)" for p, n in self.torn]
        return "; ".join(parts)


def file_stem(job_key: str) -> str:
    """Filename stem for `job_key`, injective and always usable.

    Claim keys are already filesystem-shaped (`fleetd.start_gate` builds
    them as `branch.replace("/", "-")`), so the common case is the key
    verbatim and a human can `ls ~/.keel/journal` and read branch names.
    Anything else -- a key with a slash, a leading dot, a length past a
    filesystem's 255-byte limit -- is slugged and given a sha256 suffix
    rather than rejected: refusing to journal is refusing to spawn
    (`JournalWriteError`), and an exotic branch name must not cost the
    fleet a gate. The record itself carries `job_key` verbatim, so the
    true key never has to be recovered from the filename.

    The slugged form is prefixed `_`, which `_SAFE_STEM` forbids as a
    first character -- so the two forms live in disjoint namespaces and
    no verbatim key can ever collide with some other key's slug, however
    the digest lands.
    """
    if not isinstance(job_key, str) or not job_key.strip():
        raise ValueError(f"job_key must be a non-empty string, got {job_key!r}")
    if _SAFE_STEM.fullmatch(job_key) and job_key not in (".", ".."):
        return job_key
    digest = hashlib.sha256(job_key.encode("utf-8", "surrogateescape")).hexdigest()[:12]
    slug = _UNSAFE_RUN.sub("-", job_key).strip("-.") or "job"
    return f"_{slug[:80]}-{digest}"


def _utcnow() -> datetime:
    return datetime.now(timezone.utc)


class Journal:
    """Append-only per-job records under `root` (`$KEEL_HOME/journal` by default)."""

    def __init__(self, root: "str | os.PathLike[str] | None" = None):
        # Resolve at construction time: test fixtures set KEEL_HOME after
        # importing this module and must never touch the operator's journal.
        self.root = (Path(root) if root is not None else
                     Path(os.environ.get("KEEL_HOME", Path.home() / ".keel")) / "journal")

    # -- writing -------------------------------------------------------- #

    def path_for(self, job_key: str) -> Path:
        return self.root / (file_stem(job_key) + SUFFIX)

    def append(self, event: str, *, job_key: str, ts: Optional[str] = None,
               **fields) -> dict:
        """Append one record and fsync it. Raises `JournalWriteError`.

        `fields` are merged verbatim; `None` values are dropped so that a
        later record never un-sets what an earlier one established (the
        fold in `_fold` takes "the last record that MENTIONED it", and a
        `spawn` written without `claim_ref` must not erase the `claim`
        record's).
        """
        if event not in EVENTS:
            raise ValueError(f"unknown journal event {event!r}; expected one of {EVENTS}")
        record = {
            "v": SCHEMA_VERSION,
            "event": event,
            "job_key": job_key,
            "ts": ts or _iso(_utcnow()),
        }
        for k, v in fields.items():
            if v is not None:
                record[k] = v
        line = json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n"
        path = self.path_for(job_key)
        try:
            existed = path.exists()
            self.root.mkdir(parents=True, exist_ok=True)
            # One write() of a short line to an O_APPEND fd, then fsync.
            # The newline rides in the SAME write as the record, which is
            # what makes "ends without a newline" a reliable signal that
            # a crash interrupted this file rather than a signal that a
            # record is merely unusual.
            with open(path, "a", encoding="utf-8") as fh:
                fh.write(line)
                fh.flush()
                os.fsync(fh.fileno())
            if not existed:
                self._fsync_dir()
        except OSError as exc:
            raise JournalWriteError(
                f"could not journal {event} for {job_key!r} at {path}: {exc}"
            ) from exc
        return record

    def _fsync_dir(self) -> None:
        """fsync the directory so a newly created file's NAME is durable.

        Without this the record's bytes are on disk and its directory
        entry is not, which after a power loss is a journal that has
        forgotten the one job it most recently learned about. Best
        effort: some filesystems refuse `O_RDONLY` fsync on a directory,
        and that must not turn into a refusal to spawn.
        """
        try:
            fd = os.open(str(self.root), os.O_RDONLY)
        except OSError:
            return
        try:
            os.fsync(fd)
        except OSError:
            pass
        finally:
            os.close(fd)

    def offer(self, *, job_key: str, kind: str, work_key: str, tag: str,
              claim_ref: Optional[str] = None, workdir: Optional[str] = None,
              offer_id: Optional[str] = None, **extra) -> dict:
        """Written BEFORE `Claim.acquire_or_reap` (SPEC §5.3)."""
        return self.append(OFFER, job_key=job_key, kind=kind, work_key=work_key,
                           tag=tag, claim_ref=claim_ref, workdir=workdir,
                           offer_id=offer_id, **extra)

    def claim(self, *, job_key: str, claim_ref: str, claim_sha: Optional[str],
              holder_host: str, started_at: str, expires_at: Optional[str] = None,
              kind: Optional[str] = None, work_key: Optional[str] = None,
              gate_version: Optional[str] = None, rustc_id: Optional[str] = None,
              platform_id: Optional[str] = None, **extra) -> dict:
        """Written once the lease is held, BEFORE the spawn.

        `started_at` must be the EXACT string in the claim payload --
        `claim_from_payload`/`started_at_of` exist so no caller has to
        re-derive it. It is half the ownership token; a value this
        module cannot reproduce byte-for-byte makes the rebuilt claim
        unable to recognize its own renewals, and `rebuild_claim`
        refuses rather than adopt such a lease.
        """
        return self.append(CLAIM, job_key=job_key, claim_ref=claim_ref,
                           claim_sha=claim_sha, holder_host=holder_host,
                           started_at=started_at, expires_at=expires_at,
                           kind=kind, work_key=work_key,
                           gate_version=gate_version, rustc_id=rustc_id,
                           platform_id=platform_id, **extra)

    def spawn(self, *, job_key: str, pid: int, pgid: int,
              scope_token: Optional[str] = None, argv0: Optional[str] = None,
              **extra) -> dict:
        """Written immediately after `Popen` returns."""
        return self.append(SPAWN, job_key=job_key, pid=int(pid), pgid=int(pgid),
                           scope_token=scope_token, argv0=argv0, **extra)

    def verdict(self, *, job_key: str, outcome: Optional[str] = None,
                tree: Optional[str] = None, rc: Optional[int] = None,
                **extra) -> dict:
        return self.append(VERDICT, job_key=job_key, outcome=outcome, tree=tree,
                           rc=rc, **extra)

    def exit(self, *, job_key: str, rc: Optional[int] = None,
             outcome: Optional[str] = None, **extra) -> dict:
        """Closes the job: never adopted, released or swept again."""
        return self.append(EXIT, job_key=job_key, rc=rc, outcome=outcome, **extra)

    # -- reading -------------------------------------------------------- #

    def scan(self) -> JournalScan:
        """Read every job file. NEVER raises for a bad file -- see
        `JournalScan.unreadable`, which is the fail-closed signal.

        An ABSENT root is an empty, readable, sweep-armed scan: a host
        that has never run a job has no journal, and treating that as
        corruption would disarm every fresh host's first sweep forever.
        Absent is not unreadable -- the same distinction `fleetlib._read`
        draws between a missing ref and an unreachable one, and for the
        same reason.
        """
        root = self.root
        jobs: List[JobState] = []
        unreadable: List[Tuple[str, str]] = []
        torn: List[Tuple[str, int]] = []
        try:
            if root.is_dir():
                paths = sorted(root.glob("*" + SUFFIX))
            elif root.exists() or root.is_symlink():
                return JournalScan(root=str(root),
                                   unreadable=((str(root), "journal root is not a directory"),))
            else:
                paths = []
        except OSError as exc:
            return JournalScan(root=str(root), unreadable=((str(root), str(exc)),))
        for path in paths:
            try:
                raw = path.read_bytes()
            except OSError as exc:
                unreadable.append((str(path), str(exc)))
                continue
            dropped = 0
            if raw and not raw.endswith(b"\n"):
                cut = raw.rfind(b"\n")
                dropped = len(raw) - (cut + 1)
                raw = raw[: cut + 1]
                torn.append((str(path), dropped))
            try:
                records = _parse_records(raw)
            except _RecordError as exc:
                unreadable.append((str(path), str(exc)))
                continue
            if not records:
                # A file with no complete records is not corruption (the
                # very first record was torn); it just describes nothing.
                continue
            jobs.append(_fold(records, path=path, torn=bool(dropped)))
        return JournalScan(root=str(root), jobs=tuple(jobs),
                           unreadable=tuple(unreadable), torn=tuple(torn))

    def read_job(self, job_key: str) -> Optional[JobState]:
        """One job's folded state, or None if it has no file. Raises
        `JournalError` if the file exists but cannot be read -- a
        single-job read has no `unreadable` bucket to fail closed into,
        so it fails loudly instead."""
        path = self.path_for(job_key)
        try:
            if (self.root.exists() or self.root.is_symlink()) and not self.root.is_dir():
                raise JournalError(f"{self.root}: journal root is not a directory")
            if not path.is_file():
                if path.exists() or path.is_symlink():
                    raise JournalError(f"{path}: journal job is not a regular file")
                return None
        except OSError as exc:
            raise JournalError(f"{path}: {exc}") from exc
        try:
            raw = path.read_bytes()
        except OSError as exc:
            raise JournalError(f"{path}: {exc}") from exc
        dropped = 0
        if raw and not raw.endswith(b"\n"):
            cut = raw.rfind(b"\n")
            dropped = len(raw) - (cut + 1)
            raw = raw[: cut + 1]
        try:
            records = _parse_records(raw)
        except _RecordError as exc:
            raise JournalError(f"{path}: {exc}") from exc
        if not records:
            if dropped:
                raise JournalError(f"{path}: torn journal has no complete record")
            return None
        return _fold(records, path=path, torn=bool(dropped))

    def forget(self, job_key: str) -> bool:
        """Remove a job's file. True if one was there."""
        try:
            self.path_for(job_key).unlink()
            return True
        except FileNotFoundError:
            return False
        except OSError as exc:
            raise JournalError(f"could not remove journal for {job_key!r}: {exc}") from exc

    def prune(self, retention_s: float = DEFAULT_RETENTION_S,
              now: Optional[datetime] = None) -> List[str]:
        """Delete CLOSED jobs whose last record is older than
        `retention_s`. Open jobs are never touched, whatever their age:
        a job with no `exit` record may still be running, and a runner
        that has been offline for a week is exactly the one that needs
        its journal most."""
        now = now or _utcnow()
        removed: List[str] = []
        scan = self.scan()
        for job in scan.jobs:
            if not job.closed or not job.last_ts:
                continue
            try:
                last = _parse_iso(job.last_ts)
            except (ValueError, TypeError):
                continue
            if (now - last).total_seconds() < retention_s:
                continue
            try:
                Path(job.path).unlink()
                removed.append(job.job_key)
            except OSError:
                continue
        return removed


class _RecordError(ValueError):
    pass


def _parse_records(raw: bytes) -> List[dict]:
    """Every complete line of `raw` as a record, or `_RecordError`.

    Fails closed on: a line that is not JSON, a line that is not an
    object, a record with no known `event`, and a record whose `v` is
    from the future. The last two are version skew on this very host --
    a newer runner journaled here and an older one is now reading -- and
    "skip what I do not recognize" is the wrong answer to that: the
    record we skipped may be the `exit` that closed a job, or the
    `spawn` that named a pgid.
    """
    out: List[dict] = []
    for n, line in enumerate(raw.decode("utf-8", "replace").splitlines(), start=1):
        if not line.strip():
            continue
        try:
            rec = json.loads(line)
        except ValueError as exc:
            raise _RecordError(f"line {n} is not valid JSON: {exc}") from exc
        if not isinstance(rec, dict):
            raise _RecordError(f"line {n} is a {type(rec).__name__}, not an object")
        v = rec.get("v")
        if not isinstance(v, int) or v > SCHEMA_VERSION:
            raise _RecordError(
                f"line {n} has schema version {v!r}, and this keel understands "
                f"at most {SCHEMA_VERSION}"
            )
        if rec.get("event") not in EVENTS:
            raise _RecordError(f"line {n} has unknown event {rec.get('event')!r}")
        out.append(rec)
    return out


_FOLD_FIELDS = (
    "kind", "work_key", "tag", "workdir", "claim_ref", "claim_sha",
    "holder_host", "started_at", "expires_at", "pid", "pgid", "scope_token",
    "leader_birth",
    "gate_version", "rustc_id", "platform_id", "outcome", "rc",
)


def _runs(records: Sequence[dict]) -> List[List[dict]]:
    """Start a new run only when an offer follows a completed run."""
    runs: List[List[dict]] = []
    current: List[dict] = []
    closed = False
    for record in records:
        if closed and record.get("event") == OFFER:
            runs.append(current)
            current = []
            closed = False
        current.append(record)
        if record.get("event") == EXIT:
            closed = True
    if current:
        runs.append(current)
    return runs


def _fold(records: Sequence[dict], *, path: Path, torn: bool) -> JobState:
    runs = _runs(records)
    current = runs[-1]
    values: Dict[str, object] = {}
    events: List[str] = []
    job_key = current[-1].get("job_key") or path.stem
    for rec in current:
        events.append(str(rec.get("event")))
        for name in _FOLD_FIELDS:
            if name in rec and rec[name] is not None:
                values[name] = rec[name]
    for name in ("pid", "pgid", "rc"):
        if name in values and not isinstance(values[name], int):
            values.pop(name)
    if "leader_birth" in values and not isinstance(values["leader_birth"], str):
        values.pop("leader_birth")
    kwargs = {k: v for k, v in values.items() if k in _FOLD_FIELDS}
    return JobState(
        job_key=str(job_key),
        path=str(path),
        closed=any(rec.get("event") == EXIT for rec in current),
        events=tuple(events),
        first_ts=current[0].get("ts"),
        last_ts=current[-1].get("ts"),
        torn=torn,
        prior_runs=len(runs) - 1,
        **kwargs,  # type: ignore[arg-type]
    )


def started_at_of(payload: dict) -> Optional[str]:
    """The EXACT `started_at` text in a claim payload, for `Journal.claim`."""
    raw = (payload or {}).get("started_at")
    return raw if isinstance(raw, str) and raw else None


# ---------------------------------------------------------------------- #
# Rebuilding a live Claim from journal evidence (the offline path)
# ---------------------------------------------------------------------- #


class ClaimRebuild(NamedTuple):
    """`rebuild_claim`'s answer. `claim is None` ⇒ `why` says what was
    missing; `anchored_on_now` records which renewal anchor was used, so
    a caller reports the choice rather than re-deriving it from prose."""

    claim: Optional[Claim]
    why: str
    anchored_on_now: bool = False


def rebuild_claim(job: JobState, *, host: str, hub, ttl: Optional[float] = None,
                  renew_interval: Optional[float] = None,
                  now: Optional[datetime] = None,
                  start_renewer: bool = True) -> ClaimRebuild:
    """A renewing `Claim` for `job`, from journal fields alone.

    `Claim.adopt` cannot be used here: its very first act is
    `hub.read(ref)`, and the whole premise of this path is that no route
    to the store answers. So the payload's fields come from the journal
    instead -- which is safe for exactly one reason: every field
    `Claim.adopt` restores off the ref was written to the journal by
    THIS host at claim time, and the one field correctness rests on
    (`started_at`, the second half of `claim._owns`' ownership token) is
    recorded as the literal payload text and re-checked here for
    round-tripping, the same guard `Claim.adopt` applies at L568-573.

    THE RENEWAL ANCHOR, and the trade this makes. `Claim.adopt` anchors
    `_last_renew_ok` on the hub's own `expires_at` minus one TTL, and
    says why: anchoring on `now` would claim a fresh TTL of grace the
    hub never agreed to. That argument holds only when the hub's
    `expires_at` is in hand. The journal's copy is the value at CLAIM
    time and is not renewed (renewals are deliberately not journaled),
    so for any job older than one TTL it is guaranteed stale in the
    dangerous direction: anchoring on it makes `_note_renew_failure`
    declare the lease lost on the renewer's FIRST tick, and `lost` is
    sticky, so a store that comes back four seconds later still costs
    the fleet a healthy, correctly-leased gate. That is not a risk, it
    is a certainty for every gate that has been running longer than a
    lease.

    So: use the journaled `expires_at` while it is still in the FUTURE
    (recent claim, fast restart -- real evidence, still valid, and the
    conservative anchor `Claim.adopt` wants), and fall back to `now`
    only once it has passed, where it has stopped being evidence about
    anything. The cost of the fallback is bounded at one TTL of extra
    grace, in the narrow case where the store is reachable to other
    hosts but not to us; the alternative costs a gate on every offline
    restart. `JournalAdoption.anchored_on_now` records which jobs took
    the fallback, so the choice is visible rather than inferred.

    The lease is NOT renewed synchronously (`Claim.adopt` does, and
    refuses if that renewal proves the lease gone). Offline that
    renewal cannot succeed; its only other effect -- marking `lost` --
    arrives anyway on the renewer's first tick, and the runner's
    existing lost-lease kill (`fleetd.reconcile_once`'s `if w.claim.lost`
    branch -- L1965 in this tree; SPEC §10 I4 cites L1793-1832, from
    before the file shifted, so trust the name over the number) is
    then the single path that stops the work. One kill path, not two.
    """
    now = now or _utcnow()
    if not job.claim_ref:
        return ClaimRebuild(None, "journal entry has no claim ref")
    parsed = claim_mod.parse_claim_ref(job.claim_ref)
    if parsed is None:
        return ClaimRebuild(None, f"unparseable claim ref {job.claim_ref!r}")
    kind, key = parsed
    if job.holder_host != host:
        # Never ours to renew, release, or reason about. This is the
        # journal's version of `Claim.adopt`'s other-host refusal.
        return ClaimRebuild(
            None, f"journaled claim is held by {job.holder_host!r}, not {host!r}")
    if not job.started_at:
        return ClaimRebuild(None, "journal entry has no started_at (no ownership token)")
    try:
        started = _parse_iso(job.started_at)
    except (ValueError, TypeError):
        return ClaimRebuild(None, f"unparseable started_at {job.started_at!r}")
    if _iso(started) != job.started_at:
        return ClaimRebuild(
            None,
            f"started_at {job.started_at!r} does not round-trip; a claim whose "
            f"ownership token we cannot reproduce is unholdable",
        )

    c = Claim(
        hub,
        kind,
        key,
        work_kind=job.kind or kind,
        work_key=job.work_key or key,
        gate_version=job.gate_version or "",
        # Restored, never recomputed -- see `JobState.rustc_id`. `Claim`
        # resolves these lazily, so a `None` here (a journal written
        # before the runner knew them) still behaves exactly as today.
        rustc_id=job.rustc_id,
        platform_id=job.platform_id,
        workdir=job.workdir,
        holder_host=host,
        # `pid` defaults to `os.getpid()` inside `Claim`, which for a
        # rebuilt claim would stamp THIS runner's pid onto the running
        # worker's payload at the first renewal. The journal's pid is the
        # worker's; failing that, the pgid is (a session leader's pid IS
        # its pgid).
        pid=job.pid if job.pid is not None else job.pgid,
        pgid=job.pgid,
        leader_birth=job.leader_birth,
        ttl=ttl,
        renew_interval=renew_interval,
    )
    c._started_at = started
    c._sha = job.claim_sha  # expected STALE; `_owns` repairs it on renewal
    c._released = False
    anchor = None
    if job.expires_at:
        try:
            expires = _parse_iso(job.expires_at)
        except (ValueError, TypeError):
            expires = None
        if expires is not None and expires > now:
            anchor = expires - timedelta(seconds=c.ttl)
    on_now = anchor is None
    c._last_renew_ok = anchor if anchor is not None else now
    c._clear_lost()
    if start_renewer:
        c.start_renewer()
    return ClaimRebuild(
        c,
        ("anchored on now (the journaled expires_at had already passed, so it "
         "no longer bounds anything)" if on_now
         else "anchored on the journaled expires_at"),
        on_now,
    )


# ---------------------------------------------------------------------- #
# Adoption
# ---------------------------------------------------------------------- #


class OwedRelease(NamedTuple):
    """A claim of OURS whose work is provably gone, waiting for a route.

    `started_at` rides along because `holder_host` alone is not enough to
    make the deferred delete safe: this host may itself have taken the
    branch again in the interval (a reap by our own next runner, an
    `autonomous_when_serverless` gate), which produces a claim with our
    `holder_host` and a DIFFERENT acquisition. Deleting that one drops a
    live gate's lease. `(holder_host, started_at)` is `claim._owns`'
    ownership token for exactly this reason, and `release_pending`
    checks both.
    """

    job_key: str
    claim_ref: Optional[str]
    started_at: Optional[str]
    reason: str
    run_index: int = 0


@dataclass
class JournalAdoption:
    """What `adopt_from_journal` did, and what it refused to do."""

    #: (job_key, pgid) adopted; a `Worker` was appended for each.
    adopted: List[Tuple[str, int]] = field(default_factory=list)
    #: `OwedRelease` per job whose work is provably gone. The CAS delete
    #: is owed and deferred; `release_pending` performs it as soon as any
    #: route to the store answers.
    to_release: List[OwedRelease] = field(default_factory=list)
    #: (job_key, reason) -- not adopted, not released, not killed.
    refused: List[Tuple[str, str]] = field(default_factory=list)
    #: job_keys whose renewal deadline fell back to `now` (see `rebuild_claim`).
    anchored_on_now: List[str] = field(default_factory=list)
    #: Non-empty ⇒ nothing was adopted at all (fail closed).
    unreadable: List[Tuple[str, str]] = field(default_factory=list)
    #: Why no adoption happened; None when the journal was readable.
    refused_wholesale: Optional[str] = None
    #: Always set on this path: the offline runner sweeps nothing (SPEC §5.3).
    sweep_skipped: str = "journal adoption never sweeps"

    def summary(self) -> str:
        return (
            f"journal-adopted={[f'{k}#{p}' for k, p in self.adopted]} "
            f"to_release={[o.job_key for o in self.to_release]} "
            f"refused={len(self.refused)}"
            + (f" UNREADABLE({self.refused_wholesale})" if self.refused_wholesale else "")
        )


def _default_worker_factory(*, job: JobState, claim: Claim):
    """`fleetd.Worker` for an adopted journal entry.

    Imported lazily: `fleetd` is the runner's local half and pulls in the
    whole scheduler, while this module is also imported by tests and by
    `keel why` paths that have no business paying for that.
    """
    import fleetd  # noqa: PLC0415 -- deliberate, see docstring

    return fleetd.Worker(
        branch=job.work_key or claim.key,
        tag=f"journal-{job.kind or 'gate'}-{claim.key}",
        pgid=int(job.pgid),  # type: ignore[arg-type]
        claim=claim,
        popen=None,  # not our child: `alive()` falls back to the pgid listing
        kind=job.kind or "gate",
        job_key=job.job_key,
    )


def adopt_from_journal(
    journal: Journal,
    host: str,
    workers: list,
    *,
    hub,
    scope_token: Optional[str] = None,
    markers: Optional[Sequence[str]] = None,
    pgid_probe: Optional[Callable[[], set]] = None,
    identity_probe: Optional[Callable[..., Optional[str]]] = None,
    worker_factory: Callable[..., object] = _default_worker_factory,
    scan: Optional[JournalScan] = None,
    ttl: Optional[float] = None,
    renew_interval: Optional[float] = None,
    now: Optional[datetime] = None,
    renew_claims: bool = True,
) -> JournalAdoption:
    """Adopt this host's still-running work from LOCAL evidence only.

    The offline half of SPEC §5.3: "with *both* routes unreachable at
    start the runner no longer exits 5 -- it adopts journaled groups
    that are alive and identity-verified, starts their renewers (which
    will mark `lost` and kill per `claim.py` if the store stays away),
    sweeps nothing, spawns nothing, and retries the store every 30 s."

    Four dispositions, and the evidence each needs:

      ADOPT    an OPEN job with a pgid > 1 that is (a) in the `ps`
               listing and (b) identity-verified -- some live same-uid
               member of that group carries a worker marker AND this
               runner's own scope token. `fleetd.adopt_workers` demands
               exactly this of a claim payload's pgid and for exactly
               this reason: "a pgid is a name that gets recycled".
      RELEASE  our job whose pgid is kernel-proven gone, or whose pgid has
               a positively different live leader (recycled, or pre-scope).
               An omitted row, zombie leader, or unmarked child alone is
               inconclusive and cannot authorize release. The proven-dead
               work's claim should stop blocking the branch.
               There is no route to CAS-delete it right now, so it is
               recorded as owed and `release_pending` does it later.
      REFUSE   anything we cannot prove is ours to touch: a journaled
               `holder_host` that is not us, a job that never reached
               `spawn`, our own process group, a claim ref we cannot
               parse or whose ownership token will not round-trip.
               Never adopted, never released, never killed.
      NOTHING  is killed. Not one process. The sweep is the hub claim
               listing's prerogative (`fleetd.adopt_workers`), and this
               path runs precisely when that listing is unavailable.

    A journal that cannot be read adopts NOTHING (`res.unreadable`,
    `res.refused_wholesale`) -- the task statement's fail-closed rule.
    The runner then behaves as it would with an empty journal: no
    adoption, no sweep, no spawn, retry the store.
    """
    res = JournalAdoption()
    scan = journal.scan() if scan is None else scan
    if not scan.readable:
        res.unreadable = list(scan.unreadable)
        res.refused_wholesale = (
            f"journal at {scan.root} could not be read ({scan.why_not_readable()}); "
            f"adopting nothing and sweeping nothing"
        )
        return res

    if scope_token is None or pgid_probe is None or identity_probe is None:
        import fleetd  # noqa: PLC0415 -- see `_default_worker_factory`

        if scope_token is None:
            scope_token = fleetd.fleet_scope_token(hub.url)
        if pgid_probe is None:
            pgid_probe = fleetd.live_pgids
        if identity_probe is None:
            identity_probe = fleetd._scoped_worker_in_group

    from keel.runner import ProcessListingUnavailable  # local process evidence
    try:
        live = pgid_probe()
    except ProcessListingUnavailable as exc:
        res.refused_wholesale = (
            f"process listing unavailable ({exc}); adopting nothing and releasing nothing")
        return res
    try:
        own_pgid = os.getpgrp()
    except (AttributeError, OSError):
        own_pgid = None

    for job in sorted(scan.open_jobs, key=lambda j: j.job_key):
        if any(w.claim.ref == job.claim_ref and w.pgid == job.pgid and
               w.claim._started_at is not None and
               claim_mod._iso(w.claim._started_at) == job.started_at
               for w in workers):
            # A store pass may have adopted this job before a later hub
            # listing failed. Keep its live claim/renewer as the one owner.
            continue
        if job.holder_host is not None and job.holder_host != host:
            res.refused.append(
                (job.job_key,
                 f"journaled holder_host {job.holder_host!r} is not {host!r}; "
                 f"another host's claim is never ours to adopt or release")
            )
            continue
        if not job.spawned:
            # An acquired claim without a spawn record is ambiguous: the
            # daemon may have died before Popen, or the post-Popen append
            # may have failed. A missing local PGID cannot prove absence.
            res.refused.append((
                job.job_key,
                "claim has no durable spawn record; process absence unproved"
                if job.started_at else "offer never acquired a claim"))
            continue
        pgid = int(job.pgid)  # type: ignore[arg-type]
        if own_pgid is not None and pgid == own_pgid:
            res.refused.append((job.job_key, f"pgid {pgid} is this runner's own group"))
            continue
        missing_from_listing = pgid not in live
        if missing_from_listing:
            # A missing ps row is not proof that a live group disappeared.
            # Confirm kernel absence before recording a deferred CAS delete;
            # if the group exists, the identity check below still decides
            # whether this is the journaled worker or a recycled PGID.
            try:
                os.killpg(pgid, 0)
            except ProcessLookupError:
                res.to_release.append(
                    OwedRelease(job.job_key, job.claim_ref, job.started_at,
                                f"process group {pgid} is gone", job.prior_runs))
                continue
            except OSError:
                pass  # uncertain or EPERM: identity must decide
        if job.scope_token is not None and job.scope_token != scope_token:
            # The token is derived from the HUB URL (`fleet_scope_token`),
            # so a disagreement means this journal entry was written by a
            # runner pointed at a different fleet. `fleetd`'s sweep uses
            # the CURRENT daemon's token for exactly this reason -- a
            # marker match alone only proves gate-SHAPED, and provenance
            # that belongs to somebody else's hub is not ours to act on.
            res.refused.append(
                (job.job_key,
                 f"journaled under scope {job.scope_token!r}, but this runner's "
                 f"hub scope is {scope_token!r}")
            )
            continue
        # The CURRENT token, never the journaled one: adopting on the
        # strength of provenance recorded by a differently-configured
        # runner is how a fixture daemon comes to supervise the real
        # fleet (`fleet_scope_token`'s incident note).
        try:
            member = identity_probe(pgid, markers, scope_token)
        except ProcessListingUnavailable as exc:
            # Earlier entries may have been proved absent, but a partial
            # startup pass must never queue a release on a failed scan.
            res.to_release.clear()
            res.refused_wholesale = (
                f"process identity listing unavailable ({exc}); releasing nothing")
            return res
        if job.spawned:
            from keel.runner import _journal_group_identity
            try:
                group_identity = _journal_group_identity(
                    pgid, scope_token, markers, expected_birth=job.leader_birth,
                    require_birth=True)
            except ProcessListingUnavailable as exc:
                res.to_release.clear()
                res.refused_wholesale = (
                    f"process identity listing unavailable ({exc}); releasing nothing")
                return res
            if group_identity == "missing":
                # A missing ps row is inconclusive whichever listing omitted
                # it. Only a positively different group can retire this run.
                res.refused.append((job.job_key,
                                    f"pgid {pgid} exists but different identity is unproved"))
                continue
            if group_identity == "other":
                res.to_release.append(
                    OwedRelease(job.job_key, job.claim_ref, job.started_at,
                                f"recorded pgid {pgid} has different birth",
                                job.prior_runs))
                continue
            member = "verified by birth identity"
        rebuilt = rebuild_claim(job, host=host, hub=hub, ttl=ttl,
                                renew_interval=renew_interval, now=now,
                                start_renewer=renew_claims)
        if rebuilt.claim is None:
            res.refused.append((job.job_key, rebuilt.why))
            continue
        if rebuilt.anchored_on_now:
            res.anchored_on_now.append(job.job_key)
        workers.append(worker_factory(job=job, claim=rebuilt.claim))
        res.adopted.append((job.job_key, pgid))
    return res


def release_pending(hub, host: str, adoption: JournalAdoption,
                    journal: Optional[Journal] = None) -> List[Tuple[str, str]]:
    """CAS-delete the claims `adopt_from_journal` recorded as owed.

    Called by the runner once ANY route to the store answers again. Each
    delete re-verifies against the store first -- `holder_host` is us and
    `started_at` matches the journal's token -- because the journal is
    evidence and the store is authority, and between the crash and now
    the claim may have been legitimately reaped and re-taken by somebody
    else. A mismatch leaves the ref completely alone.

    Returns `(claim_ref, outcome)` per attempt; never raises for a store
    that is still away (`HubError` becomes an outcome string, and the
    entry stays owed for the next attempt).
    """
    done: List[Tuple[str, str]] = []
    still_owed: List[OwedRelease] = []
    for owed in adoption.to_release:
        job_key, ref, started_at, reason = (
            owed.job_key, owed.claim_ref, owed.started_at, owed.reason)
        if not ref:
            done.append((job_key, "no claim ref to release"))
            continue
        close_journal = journal
        if journal is not None:
            try:
                current = journal.read_job(job_key)
            except JournalError as exc:
                still_owed.append(owed)
                done.append((ref, f"journal unreadable: {exc}; left alone"))
                continue
            # A new offer has not acquired ownership yet. It must not erase
            # the previous run's cleanup debt; only the store can establish
            # whether that acquisition still owns the claim.
            pending_offer = (current is not None and
                             current.prior_runs > owed.run_index and
                             current.claim_ref == ref and
                             current.started_at is None)
            if current is not None and not pending_offer and (
                    current.prior_runs != owed.run_index or
                    current.claim_ref != ref or current.started_at != started_at):
                done.append((ref, "journal describes a newer run or different "
                                  "ownership; left alone"))
                continue
            # Local completion does not prove the remote claim was released.
            # Keep the ownership/CAS checks and retry debt, but do not append
            # another exit to this already completed run.
            if current is None or pending_offer or current.closed:
                # Retention can remove local evidence before a store outage
                # ends. The captured token and remote CAS still prove ownership.
                close_journal = None
        try:
            sha = hub.sha(ref)
            if sha is None:
                done.append((ref, "already gone"))
                if close_journal is not None:
                    _close(close_journal, job_key, reason)
                continue
            payload = hub.read(ref)
        except HubError as exc:
            still_owed.append(owed)
            done.append((ref, f"store unreachable: {exc}"))
            continue
        if not payload or payload.get("holder_host") != host:
            done.append((ref, f"held by {(payload or {}).get('holder_host')!r}; left alone"))
            if close_journal is not None:
                _close(close_journal, job_key, "claim is another host's now")
            continue
        if not started_at:
            done.append((ref, "no ownership token was journaled; left alone"))
            if close_journal is not None:
                _close(close_journal, job_key, "no ownership token to prove the claim is ours")
            continue
        if payload.get("started_at") != started_at:
            # OUR host, but not OUR acquisition -- this host took the
            # branch again in the interval (our own next runner reaping
            # it, an `autonomous_when_serverless` gate). Deleting it
            # drops a live gate's lease. `(holder_host, started_at)` is
            # `claim._owns`' token; half of it is not enough.
            done.append((ref, f"re-acquired at {payload.get('started_at')!r} "
                              f"(ours was {started_at!r}); left alone"))
            if close_journal is not None:
                _close(close_journal, job_key, "claim was re-acquired by this host")
            continue
        try:
            ok = hub.delete(ref, expect_sha=sha)
        except HubError as exc:
            still_owed.append(owed)
            done.append((ref, f"delete failed: {exc}"))
            continue
        done.append((ref, "released" if ok else "CAS lost; left alone"))
        if ok:
            if close_journal is not None:
                _close(close_journal, job_key, reason)
        else:
            still_owed.append(owed)
    adoption.to_release = still_owed
    return done


def _close(journal: Journal, job_key: str, reason: str) -> None:
    try:
        journal.exit(job_key=job_key, outcome="released", reason=reason)
    except JournalWriteError:
        # A journal we cannot write is not a reason to undo a release
        # that already landed on the store. The job stays open locally;
        # the next pass finds its pgid gone and owes the release again,
        # which `release_pending` answers with "already gone".
        pass


# ---------------------------------------------------------------------- #
# The one call `keel/runner.py` makes at startup
# ---------------------------------------------------------------------- #


@dataclass
class StartupAdoption:
    """The result of the whole startup adoption pass.

    `mode` is `"store"` when store adoption completed (hub claims are
    authoritative), or `"journal"` when it could not complete.
    """

    mode: str
    hub_result: object = None            # fleetd.AdoptionResult, or None
    store_workers: list = field(default_factory=list)  # includes partial adoption
    journal_result: Optional[JournalAdoption] = None
    scan: Optional[JournalScan] = None
    hub_error: Optional[BaseException] = None
    sweep_skipped: Optional[str] = None
    suppressed_kills: List[int] = field(default_factory=list)
    local_conflict_unresolved: bool = False
    unresolved_orphan_pgids: set = field(default_factory=set)
    unresolved_orphan_births: dict = field(default_factory=dict)

    @property
    def offline(self) -> bool:
        return self.mode == "journal"

    @property
    def local_work_unresolved(self) -> bool:
        return self.local_conflict_unresolved or bool(self.unresolved_orphan_pgids)

    @property
    def spawn_allowed(self) -> bool:
        """Require store authority and resolved local run conflicts."""
        return self.mode == "store" and not self.local_work_unresolved

    def summary(self) -> str:
        parts = [f"mode={self.mode}"]
        if self.hub_result is not None:
            parts.append(self.hub_result.summary())
        if self.journal_result is not None:
            parts.append(self.journal_result.summary())
        if self.sweep_skipped:
            parts.append(f"SWEEP-SKIPPED({self.sweep_skipped})")
        if self.local_work_unresolved:
            parts.append("LOCAL-WORK-UNRESOLVED(starts disabled)")
        return " ".join(parts)


#: SPEC §5.3: "retries the store every 30 s".
OFFLINE_RETRY_S = 30


def adopt_at_startup(
    hub,
    host: str,
    workers: list,
    *,
    journal: Optional[Journal] = None,
    markers: Optional[Sequence[str]] = None,
    scope_token: Optional[str] = None,
    hub_adopt: Optional[Callable[..., object]] = None,
    journal_adopt: Callable[..., JournalAdoption] = adopt_from_journal,
    ttl: Optional[float] = None,
    renew_interval: Optional[float] = None,
    allow_store_adoption: bool = True,
    log: Callable[[str], None] = lambda msg: print(msg, file=sys.stderr, flush=True),
) -> StartupAdoption:
    """Rebuild `workers` at runner start. THE replacement for `fleetd.main`'s

        try:    adoption = adopt_workers(hub, host, workers)
        except HubError: ...; return 5

    -- one call, and no exit-5 path. Call it exactly where fleetd calls
    `adopt_workers`: after the host singleton is held (which is what makes
    adoption safe -- no second runner on this host can be adopting the
    same work) and before the first reconcile.

    Two routes, in this order:

    1. The journal is SCANNED FIRST, before anything can be killed. If
       it is not `sweep_armed` (unreadable, or a torn final record) the
       hub pass runs with a killer that refuses -- disarmed, loudly,
       exactly as `adopt_workers` disarms itself on an unreadable claim.
       Scanning afterwards would be pointless: a suppression that
       arrives after the SIGKILL suppresses nothing.
    2. `fleetd.adopt_workers` runs against the store. With the store
       reachable the hub claim is truth EXACTLY as today (SPEC §5.3);
       the journal cannot out-vote a CAS'd lease. For a pending-spawn
       prefix, however, an exact store claim's kernel-absent PGID must be
       durably closed locally before its last remote evidence is deleted.
    3. If the store pass fails, or the caller has no host singleton yet,
       the journal supplies local observation only. Without host ownership,
       rebuilt worker claims do not start renewers; the caller applies the
       ordinary lost-lease deadline until ownership can be established.

    The caller must honour `spawn_allowed` (False while offline) and
    retry the store every `OFFLINE_RETRY_S`, calling `release_pending`
    with the returned `journal_result` once a route answers.
    """
    journal = journal if journal is not None else Journal()
    scan = journal.scan()
    if hub_adopt is None:
        import fleetd  # noqa: PLC0415 -- see `_default_worker_factory`

        hub_adopt = fleetd.adopt_workers

    res = StartupAdoption(mode="store", scan=scan)
    kwargs = {}
    recorded_births = {
        job.claim_ref: (job.started_at, job.pgid, job.leader_birth)
        for job in scan.open_jobs
        if (job.claim_ref and job.started_at and job.spawned and
            job.holder_host == host)
    }
    if recorded_births:
        kwargs["recorded_births"] = recorded_births
    if markers is not None:
        kwargs["markers"] = markers
    if scope_token is not None:
        kwargs["scope_token"] = scope_token
    # A failed post-Popen append leaves only offer/claim locally while the
    # exact store claim may already contain its PGID. If that group is now
    # kernel-absent, journal its proven-dead outcome BEFORE the store claim
    # (the only remaining PGID evidence) is deleted. A failed durable append
    # keeps the claim and lets the next startup retry the same proof.
    pending_by_ref: dict = {}
    for job in scan.open_jobs:
        if job.claim_ref and not job.spawned:
            pending_by_ref.setdefault(job.claim_ref, []).append(job)
    closed_pending: set = set()

    def _before_dead_release(ref: str, payload: dict) -> bool:
        pending = pending_by_ref.get(ref, ())
        if not pending:
            return True
        if not scan.sweep_armed:
            return False
        for job in pending:
            if (job.holder_host != host or not job.started_at or
                    job.started_at != payload.get("started_at")):
                return False
        for job in pending:
            try:
                journal.exit(
                    job_key=job.job_key, outcome="released",
                    reason="exact store claim's process group is kernel-absent",
                    pgid=payload["pgid"],
                    holder_host=host, started_at=job.started_at,
                    leader_birth=payload.get("leader_birth"),
                )
            except JournalError:
                return False
            closed_pending.add(job.job_key)
        return True

    if pending_by_ref:
        kwargs["before_dead_release"] = _before_dead_release
    if not scan.sweep_armed:
        res.sweep_skipped = (
            f"journal at {scan.root} is not trustworthy ({scan.why_not_armed()}); "
            f"nothing on this host is killed this pass"
        )
        log(f"keel-runner[{host}] ORPHAN SWEEP DISARMED -- {res.sweep_skipped}")

        def _refuse(pgid, **_kw):
            res.suppressed_kills.append(pgid)
            log(f"keel-runner[{host}] would have swept process group {pgid}; "
                f"not killed -- {res.sweep_skipped}")
            # The string `fleetd.adopt_workers` will print after its own
            # "killed:" label, so a log line composed by the caller still
            # says plainly that nothing died.
            return "NOT KILLED (journal not trustworthy)"

        kwargs["killer"] = _refuse

    try:
        if not allow_store_adoption:
            raise HubError("host singleton ownership is not established")
        res.hub_result = hub_adopt(hub, host, workers, **kwargs)
        res.store_workers = list(workers)
        if res.hub_result.unreadable:
            raise HubError("store adoption has unreadable claims or process identity")
        from keel.runner import _pgid_alive, fleet_scope_token, kill_process_group
        res.unresolved_orphan_pgids = {
            pgid for pgid, _outcome in res.hub_result.orphans_killed
            if _pgid_alive(pgid)
        }
        res.unresolved_orphan_births = {
            pgid: birth for pgid, birth in res.hub_result.orphan_births.items()
            if pgid in res.unresolved_orphan_pgids
        }
        if scan.open_jobs:
            # Every durable local run is an obligation, including one with
            # no current store claim. The same classifier runs at initial
            # startup, store recheck, and restart. Journal evidence may
            # trigger local stop-work but never confer claim ownership. A
            # good file beside a corrupt one still gives a known PGID to
            # gate on; the incomplete scan cannot authorize adoption/kill.
            suspects: list = []
            store_by_ref = {w.claim.ref: w for w in res.store_workers}
            claimed_by_ref = res.hub_result.claim_pgids_by_ref
            claimed_pgids = set(claimed_by_ref.values())
            local_scope = scope_token or fleet_scope_token(hub.url)
            for job in scan.open_jobs:
                if job.spawned or job.job_key in closed_pending or not job.started_at:
                    continue
                current = store_by_ref.get(job.claim_ref)
                if (current is None or current.claim._started_at is None or
                        claim_mod._iso(current.claim._started_at) != job.started_at or
                        not scan.sweep_armed):
                    res.local_conflict_unresolved = True
                    continue
                # A verified, exact store-adopted worker can restore the
                # missing post-Popen record. If that append still fails,
                # keep starts disabled rather than losing this obligation.
                try:
                    journal.spawn(job_key=job.job_key, pid=current.pgid,
                                  pgid=current.pgid, scope_token=local_scope,
                                  leader_birth=current.claim.leader_birth)
                except JournalError:
                    res.local_conflict_unresolved = True
            obligations = [
                job for job in scan.open_jobs
                if job.spawned and isinstance(job.pgid, int) and job.pgid > 1
                and job.holder_host in (None, host)
                and job.scope_token in (None, local_scope)
                and (job.claim_ref not in store_by_ref or
                     job.pgid != store_by_ref[job.claim_ref].pgid)
            ]
            attempted_orphans = {pgid for pgid, _ in res.hub_result.orphans_killed}
            if obligations and scan.sweep_armed:
                local = journal_adopt(
                    journal, host, suspects, hub=hub, scan=scan, markers=markers,
                    scope_token=scope_token, ttl=ttl, renew_interval=renew_interval,
                    renew_claims=False,
                )
                adopted = {(w.job_key, w.pgid): w for w in suspects}
                released = {owed.job_key for owed in local.to_release}
            else:
                adopted = {}
                released = set()
            for job in obligations:
                current = store_by_ref.get(job.claim_ref)
                if job.pgid in claimed_pgids:
                    # A readable claim still names this group, perhaps at
                    # another ref or under another host/token. A journal
                    # ref alone cannot authorize killing that worker.
                    if _pgid_alive(job.pgid):
                        res.local_conflict_unresolved = True
                    continue
                old = adopted.get((job.job_key, job.pgid))
                if old is not None:
                    if current is not None:
                        workers.append(old)
                    else:
                        # Fresh complete claim listing says this group has
                        # no lease. Do not re-signal an orphan already tried
                        # by the store sweep, but cover a missed worker row.
                        if job.pgid not in attempted_orphans:
                            kill_process_group(job.pgid)
                        if _pgid_alive(job.pgid):
                            res.unresolved_orphan_pgids.add(job.pgid)
                            if job.leader_birth:
                                res.unresolved_orphan_births.setdefault(
                                    job.pgid, job.leader_birth)
                elif job.job_key not in released and _pgid_alive(job.pgid):
                    if current is not None:
                        res.local_conflict_unresolved = True
                    else:
                        res.unresolved_orphan_pgids.add(job.pgid)
                        if job.leader_birth:
                            res.unresolved_orphan_births.setdefault(
                                job.pgid, job.leader_birth)
    except HubError as exc:
        res.store_workers = list(workers)
        # The store pass is unavailable or incomplete. Use local evidence
        # only until a complete store retry can arbitrate starts.
        res.mode = "journal"
        res.hub_error = exc
        source = "store adoption incomplete" if allow_store_adoption else "host lease unowned"
        log(f"keel-runner[{host}] {source} at startup ({exc}); adopting "
            f"from the local job journal instead of refusing to start")
        res.journal_result = journal_adopt(
            journal, host, workers, hub=hub, scan=scan, markers=markers,
            scope_token=scope_token, ttl=ttl, renew_interval=renew_interval,
            renew_claims=allow_store_adoption,
        )
        if res.journal_result.refused_wholesale:
            log(f"keel-runner[{host}] {res.journal_result.refused_wholesale}")
        res.sweep_skipped = res.sweep_skipped or res.journal_result.sweep_skipped
    return res
