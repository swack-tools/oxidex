#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Deterministic distiller: lessons.jsonl -> module playbooks + GLOBAL-PITFALLS.

Spec: docs/plans/specs/2026-07-24-fleet-knowledge-and-scaling-design.md,
sections K1 (ledger contract), K2 (GLOBAL-PITFALLS.md curation) and K3
(module playbooks + this distiller). The distiller is deliberately dumb:
**no model calls**, ever. It is a pure fold over the append-only event
ledger `<home>/logs/lessons.jsonl`:

  1. Read newline-terminated lines from the persisted byte cursor
     (`knowledge/distiller.cursor`) to EOF. A trailing partial line (a
     writer mid-append) is never consumed -- the cursor only ever advances
     past complete lines, and only after every output file has been
     atomically replaced, so a crash at any point replays events instead
     of losing them. Replay is idempotent: every applied ledger line is
     remembered by its sha1 in the state file, so reprocessing the same
     bytes is a no-op (K3 "dedupe by sha1 of the event line").
  2. Skip malformed lines (readers of lessons.jsonl never degrade to `{}`
     -- K1) and skip `event=infra` entirely: 429/timeout noise is excluded
     from every knowledge query at the source.
  3. Cluster surviving events by `fingerprint_generic` -- sha1 of
     (event, checklist_id-or-normalized-reason) -- which is exactly what
     makes "same mistake in Canon.pm and Nikon.pm" clusterable across
     modules. Per cluster we track total count, the set of modules it was
     seen in, the latest representative reason/tag/date.
  4. Render `knowledge/modules/<Module>.md` (workers read these at
     build_prompt; only this script ever writes them), newest-first,
     capped at 4000 chars per file, written via tempfile + os.replace:

         - wrong_value x7 (Canon.pm, Minolta.pm): PrintConv strings must
           match Perl byte-for-byte - last: JPEG:MakerNotes:AELButton 2026-07-24

  5. Promote any cluster with >=3 occurrences across >=2 distinct modules
     as a candidate bullet in `knowledge/GLOBAL-PITFALLS.md` (K2): hard
     cap 3000 chars / 12 bullets, oldest *candidates* evicted first, and
     human-seeded bullets (first line contains "[seed]") are never
     dropped. The file is rewritten only when its content actually
     changes, and the previous version is copied to
     `knowledge/history/GLOBAL-PITFALLS-<ts>.md` first.

Singleton discipline (K3): `knowledge/distiller.lock` holds JSON
`{pid, script_git_sha, heartbeat_ts}`. A launcher that finds a fresh
heartbeat (<10 min) under the same script sha exits 0 quietly; a stale
heartbeat or a sha mismatch means the holder is dead or outdated -- it is
SIGTERMed (kill fn injectable for tests) and the lock taken over. The
heartbeat is refreshed after each output file written.

One-time migration (`--migrate-format-memory`): the 22 legacy
`<home>/logs/format-memory/*.md` rolling notes are converted into
synthetic `structural` lesson events (module = the format name, the
documented fallback key), with 429/timeout/rate-limit noise lines
dropped, appended to lessons.jsonl through the standard K1 append
contract, distilled, and the originals moved to `format-memory/archived/`.
`summarize_format_memory` and `append_format_memory_note` are retired by
this migration -- worker notes become ledger events.

Usage:
    uv run scripts/distill_lessons.py --once
    uv run scripts/distill_lessons.py --home /tmp/oxidex-test --once
    uv run scripts/distill_lessons.py --migrate-format-memory
"""
import argparse
import hashlib
import json
import os
import re
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# --- K1 shared conventions (identical across all Phase-1 writers) ------------

#: The full event vocabulary (K1). The distiller does not reject unknown
#: events (forward compatible), but `infra` is excluded from every
#: knowledge output.
EVENT_ENUM = (
    "build_failed", "gap_not_closed", "wrong_value", "test_regressed",
    "duplicate", "review_rejected", "critique", "fixed", "machine_accepted",
    "human_accepted", "human_rejected", "structural", "infra",
)

#: One ledger line is clamped to this many bytes (K1 atomicity contract:
#: one os.write of one line, O_APPEND).
LESSON_LINE_MAX_BYTES = 2000

#: Lock heartbeat older than this is stale and the holder gets SIGTERMed.
STALE_HEARTBEAT_SECONDS = 600

#: knowledge/modules/<Module>.md hard size cap (K3).
MODULE_FILE_CHAR_CAP = 4000

#: GLOBAL-PITFALLS.md hard caps (K2).
PITFALLS_CHAR_CAP = 3000
PITFALLS_BULLET_CAP = 12

#: Promotion rule (K3): a generic cluster is a GLOBAL-PITFALLS candidate
#: once it has this many occurrences across this many distinct modules.
PROMOTE_MIN_COUNT = 3
PROMOTE_MIN_MODULES = 2

#: Representative reasons are clamped to keep bullets one-screen readable.
REASON_DISPLAY_CHARS = 240

#: Lines in legacy format-memory files matching this are 429/timeout noise
#: and are dropped by the migration instead of becoming lesson events.
NOISE_RE = re.compile(
    r"(?i)\b429\b|too many requests|rate[ -]?limit|tim(?:e|ed|ing)[ -]?out|timeout"
)

#: Bullet shape this script itself renders -- parsed back out of
#: GLOBAL-PITFALLS.md so a promoted cluster whose count grows updates its
#: existing bullet in place instead of appending a duplicate.
CANDIDATE_BULLET_RE = re.compile(
    r"^- (?P<event>[A-Za-z_]+) x(?P<count>\d+)"
    r"(?: \[(?P<cid>[^\]]+)\])? \((?P<mods>[^)]*)\): (?P<rest>.*)$"
)

DEFAULT_PITFALLS_PREAMBLE = (
    "# Global pitfalls — distilled cross-module lessons\n"
    "\n"
    "Written by scripts/distill_lessons.py (spec K2/K3): a bullet is promoted\n"
    "when the same generic fingerprint recurs >=3 times across >=2 modules.\n"
    "Cap 3000 chars / 12 bullets; \"[seed]\" bullets are human-curated and are\n"
    "never evicted by the distiller.\n"
    "\n"
)


def home_dir(cli_home=None):
    """Resolve OXIDEX_HOME: --home flag beats env beats ~/.oxidex."""
    if cli_home:
        return Path(cli_home)
    return Path(os.environ.get("OXIDEX_HOME", str(Path.home() / ".oxidex")))


def iso_ts(epoch):
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(epoch))


def clamp_text(text, limit=REASON_DISPLAY_CHARS):
    text = str(text)
    return text if len(text) <= limit else text[: limit - 1] + "…"


def flatten_ws(text):
    """Collapse ALL whitespace runs -- newlines included -- to single
    spaces. Rendered bullets must stay exactly ONE "- " line: a reason
    containing "\\n- ..." (trivially produced by a multi-line --lesson
    shell argument flowing through lessons.jsonl) would otherwise render
    a block that split_bullets re-parses as several bullets next pass,
    the in-place identity match would miss, and the same cluster would be
    APPENDED again on every distiller pass, forever."""
    return " ".join(str(text).split())


def norm_reason(reason):
    """Normalize free-text reasons so near-identical failures cluster.

    Lowercase, drop a leading "[YYYY-MM-DD]" stamp, fold hex/decimal runs
    to "#" (so "index 42" and "index 43" are the same mistake), collapse
    whitespace, truncate. This is the "normalized reason" half of the K1
    fingerprint convention shared by every ledger writer -- change it in
    lockstep everywhere or fingerprints stop matching across scripts.
    """
    s = str(reason or "").strip().lower()
    s = re.sub(r"^\[\d{4}-\d{2}-\d{2}\]\s*", "", s)
    s = re.sub(r"0x[0-9a-f]+", "#", s)
    s = re.sub(r"\d+", "#", s)
    s = re.sub(r"\s+", " ", s)
    return s[:200]


def sha1_fields(*parts):
    return hashlib.sha1("\x1f".join(str(p) for p in parts).encode("utf-8", "replace")).hexdigest()


def fingerprint_key(checklist_id, reason):
    """checklist_id when present, else the normalized reason (K1)."""
    cid = str(checklist_id or "").strip()
    return cid if cid else norm_reason(reason)


def fingerprint_scoped(event, module, checklist_id, reason):
    return sha1_fields(event, module, fingerprint_key(checklist_id, reason))


def fingerprint_generic(event, checklist_id, reason):
    return sha1_fields(event, fingerprint_key(checklist_id, reason))


def make_lesson(*, ts, worker, format_name, module, event, reason,
                evidence="", table="", tag_key="", checklist_id=""):
    """Build one schema-complete K1 event dict (fingerprints included)."""
    return {
        "ts": iso_ts(ts) if isinstance(ts, (int, float)) else str(ts),
        "worker": worker,
        "format": format_name,
        "module": module,
        "table": table,
        "tag_key": tag_key,
        "event": event,
        "reason": reason,
        "evidence": evidence,
        "checklist_id": checklist_id,
        "fingerprint_scoped": fingerprint_scoped(event, module, checklist_id, reason),
        "fingerprint_generic": fingerprint_generic(event, checklist_id, reason),
    }


def encode_lesson_line(event, max_bytes=LESSON_LINE_MAX_BYTES):
    """Serialize one event to a single newline-terminated line <= max_bytes.

    Oversized events shed `evidence` then `reason` content until they fit;
    as a last resort the encoded bytes are hard-clamped (producing a
    malformed line that readers skip -- the K1 contract explicitly allows
    that failure mode, it never allows a torn multi-write).
    """
    def enc(e):
        return (json.dumps(e, separators=(",", ":"), ensure_ascii=False) + "\n").encode("utf-8")

    raw = enc(event)
    if len(raw) <= max_bytes:
        return raw
    slim = dict(event)
    for field in ("evidence", "reason"):
        val = str(slim.get(field) or "")
        while val and len(raw) > max_bytes:
            val = val[: max(0, len(val) - max(len(raw) - max_bytes, 16))]
            slim[field] = val
            raw = enc(slim)
        if len(raw) <= max_bytes:
            return raw
    return raw[: max_bytes - 1] + b"\n"


def append_lesson(lessons_path, event):
    """K1 append contract: O_APPEND|O_CREAT|O_WRONLY + exactly ONE os.write
    of one clamped line. O_APPEND makes concurrent single writes from many
    workers interleave at line granularity (same-filesystem guarantee; the
    spec forbids pointing OXIDEX_HOME at NFS for exactly this reason)."""
    lessons_path = Path(lessons_path)
    lessons_path.parent.mkdir(parents=True, exist_ok=True)
    line = encode_lesson_line(event)
    fd = os.open(str(lessons_path), os.O_APPEND | os.O_CREAT | os.O_WRONLY, 0o644)
    try:
        os.write(fd, line)
    finally:
        os.close(fd)
    return line


# --- ledger reading -----------------------------------------------------------

def parse_lesson_line(raw):
    """Decode one ledger line; None for anything malformed (K1: readers
    skip malformed lines, they never degrade them to `{}`)."""
    try:
        ev = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, ValueError):
        return None
    if not isinstance(ev, dict):
        return None
    event = ev.get("event")
    if not isinstance(event, str) or not event:
        return None
    return ev


def read_complete_lines(lessons_path, offset):
    """Read newline-terminated lines from byte `offset` to EOF.

    Returns (lines, new_offset) where new_offset covers ONLY the complete
    lines returned -- a trailing partial line (a concurrent writer caught
    mid-append) stays un-consumed for the next pass. The ledger is
    append-only and never rotated (K1), so a persisted offset is always
    valid.
    """
    lessons_path = Path(lessons_path)
    if not lessons_path.exists():
        return [], offset
    with lessons_path.open("rb") as f:
        f.seek(offset)
        data = f.read()
    end = data.rfind(b"\n")
    if end == -1:
        return [], offset
    complete = data[: end + 1]
    return complete.split(b"\n")[:-1], offset + len(complete)


# --- aggregation state --------------------------------------------------------

def fresh_state():
    return {"seq": 0, "seen": [], "clusters": {}}


def load_state(state_path):
    """Persisted aggregate (clusters + applied-line hashes). A corrupt or
    missing state file just means we start folding from scratch for lines
    not yet passed by the cursor -- outputs are recomputed, never torn."""
    try:
        state = json.loads(Path(state_path).read_text())
    except (OSError, ValueError):
        return fresh_state()
    if not isinstance(state, dict) or not isinstance(state.get("clusters"), dict):
        return fresh_state()
    state.setdefault("seq", 0)
    state.setdefault("seen", [])
    return state


def apply_events(state, lines):
    """Fold complete ledger lines into the cluster state.

    Dedupe by sha1 of the raw line (idempotent replay after a crash
    between output-replace and cursor-advance); skip malformed lines and
    `event=infra`. Returns the number of events actually applied.
    """
    seen = set(state["seen"])
    applied = 0
    for raw in lines:
        line_hash = hashlib.sha1(raw).hexdigest()
        if line_hash in seen:
            continue
        ev = parse_lesson_line(raw)
        if ev is None or ev["event"] == "infra":
            continue
        _apply_one(state, ev)
        state["seen"].append(line_hash)
        seen.add(line_hash)
        applied += 1
    # The replay window is only ever the last un-cursored batch, so a
    # bounded recent-hash set is plenty.
    state["seen"] = state["seen"][-50000:]
    return applied


def _apply_one(state, ev):
    """Merge one event into its generic-fingerprint cluster. The event's
    own fingerprint_generic is honored when present (all writers share one
    formula); computed from (event, checklist_id|norm-reason) otherwise.
    Module attribution falls back to the format name (K3) then Unknown."""
    state["seq"] += 1
    seq = state["seq"]
    event = ev["event"]
    module = str(ev.get("module") or ev.get("format") or "Unknown")
    cid = str(ev.get("checklist_id") or "").strip()
    reason = str(ev.get("reason") or "").strip()
    fp = str(ev.get("fingerprint_generic") or "").strip()
    if not fp:
        fp = fingerprint_generic(event, cid, reason)
    cluster = state["clusters"].get(fp)
    if cluster is None:
        cluster = {
            "event": event,
            "checklist_id": cid,
            "key": fingerprint_key(cid, reason),
            "count": 0,
            "modules": [],
            "reason": reason,
            "last_tag_key": "",
            "last_ts": "",
            "first_seq": seq,
            "last_seq": seq,
        }
        state["clusters"][fp] = cluster
    cluster["count"] += 1
    if module not in cluster["modules"]:
        cluster["modules"].append(module)
    cluster["last_seq"] = seq
    if reason:
        cluster["reason"] = reason
    if cid and not cluster.get("checklist_id"):
        cluster["checklist_id"] = cid
    tag_key = str(ev.get("tag_key") or "")
    if tag_key:
        cluster["last_tag_key"] = tag_key
    ts = ev.get("ts")
    if ts is not None and str(ts):
        cluster["last_ts"] = str(ts)


# --- rendering ----------------------------------------------------------------

def date_of(ts):
    """Best-effort YYYY-MM-DD from either an ISO string or an epoch."""
    s = str(ts or "")
    if re.match(r"^\d{4}-\d{2}-\d{2}", s):
        return s[:10]
    if re.match(r"^\d{9,}(\.\d+)?$", s):
        return time.strftime("%Y-%m-%d", time.localtime(float(s)))
    return ""


def display_reason(cluster):
    """The exact (flattened, clamped) reason text render_bullet embeds.
    Shared with cluster_identity so a bullet re-parsed from disk always
    has the same identity as the live cluster it was rendered from --
    any drift between the two re-appends duplicates instead of updating
    in place."""
    return clamp_text(flatten_ws(
        cluster.get("reason") or cluster.get("key") or "(no reason recorded)"))


def render_bullet(cluster):
    """One bullet in the shared K3 shape:

        - wrong_value x7 (Canon.pm, Minolta.pm): <reason> - last: <tag> <date>

    A checklist id, when the cluster has one, rides as "[C2]" after the
    count so the bullet stays identifiable even as its representative
    reason drifts (checklist-id clusters can carry varying reasons). The
    reason is whitespace-flattened (see flatten_ws) so the bullet is
    guaranteed to be a single "- " line."""
    mods = ", ".join(sorted(cluster["modules"]))
    cid = cluster.get("checklist_id") or ""
    cid_part = f" [{cid}]" if cid else ""
    reason = display_reason(cluster)
    tail_bits = [b for b in (cluster.get("last_tag_key"), date_of(cluster.get("last_ts"))) if b]
    tail = f" - last: {' '.join(tail_bits)}" if tail_bits else ""
    return f"- {cluster['event']} x{cluster['count']}{cid_part} ({mods}): {reason}{tail}"


def module_filename(module):
    """"Canon.pm" -> "Canon.md"; path bits and shell-hostile chars dropped."""
    name = str(module or "Unknown").strip().split("/")[-1]
    if name.endswith((".pm", ".pl")):
        name = name[:-3]
    name = re.sub(r"[^A-Za-z0-9._-]", "_", name).strip("._") or "Unknown"
    return name + ".md"


def render_module_files(clusters, modules_dir):
    """Yield (path, text) for every module playbook, newest-first bullets,
    each file capped at MODULE_FILE_CHAR_CAP chars. Pure function of the
    cluster state -- rendering twice from the same state is byte-identical,
    which is what lets the caller skip unchanged files entirely."""
    by_module = {}
    for cluster in clusters.values():
        for module in cluster["modules"]:
            by_module.setdefault(module, []).append(cluster)
    out = []
    for module in sorted(by_module):
        ordered = sorted(by_module[module], key=lambda c: c["last_seq"], reverse=True)
        header = (
            f"# {module} — distilled lessons\n\n"
            "Generated by scripts/distill_lessons.py from lessons.jsonl. Do not\n"
            "edit: rewritten on every distiller pass (workers read, never write).\n\n"
        )
        text = header
        for cluster in ordered:
            bullet = render_bullet(cluster) + "\n"
            if len(text) + len(bullet) > MODULE_FILE_CHAR_CAP:
                break
            text += bullet
        out.append((Path(modules_dir) / module_filename(module), text))
    return out


# --- GLOBAL-PITFALLS.md -------------------------------------------------------

def split_bullets(text):
    """Split a pitfalls file into (preamble, [bullet blocks]).

    A bullet starts at a column-0 "- " line; its indented/blank
    continuation lines stay attached (the seeded human bullets wrap)."""
    pre, blocks, cur = [], [], None
    for line in text.splitlines():
        if line.startswith("- "):
            if cur is not None:
                blocks.append("\n".join(cur).rstrip())
            cur = [line]
        elif cur is not None:
            cur.append(line)
        else:
            pre.append(line)
    if cur is not None:
        blocks.append("\n".join(cur).rstrip())
    preamble = "\n".join(pre) + ("\n" if pre else "")
    return preamble, blocks


def is_seed_bullet(block):
    return "[seed]" in block.splitlines()[0]


def bullet_identity(block):
    """(event, checklist_id-or-normalized-reason) for a distiller-rendered
    candidate bullet, or None for anything else (seeds, hand-written)."""
    m = CANDIDATE_BULLET_RE.match(block.splitlines()[0])
    if not m:
        return None
    reason = m.group("rest").rsplit(" - last: ", 1)[0]
    return (m.group("event"), m.group("cid") or norm_reason(reason))


def cluster_identity(cluster):
    """Mirror of bullet_identity for a live cluster (uses the same
    flattened+clamped display reason the bullet was rendered from, so
    the round trip through the file cannot change the identity)."""
    cid = cluster.get("checklist_id") or ""
    if cid:
        return (cluster["event"], cid)
    return (cluster["event"], norm_reason(display_reason(cluster)))


def update_global_pitfalls(clusters, pitfalls_path, history_dir, now, replace_fn=os.replace):
    """Promote qualifying clusters into GLOBAL-PITFALLS.md (K2 discipline).

    - Candidates: >=PROMOTE_MIN_COUNT occurrences across >=PROMOTE_MIN_MODULES
      distinct modules. An already-present candidate (same identity) is
      updated in place; new ones append at the end (newest last).
    - Eviction: while over 12 bullets or 3000 chars, drop the OLDEST
      non-seed bullet; "[seed]" bullets are never dropped.
    - Written only if the content hash actually changes; the previous
      version is copied into history/ first.

    Returns True when the file was rewritten.
    """
    pitfalls_path = Path(pitfalls_path)
    promoted = sorted(
        (c for c in clusters.values()
         if c["count"] >= PROMOTE_MIN_COUNT and len(c["modules"]) >= PROMOTE_MIN_MODULES),
        key=lambda c: c["first_seq"],
    )
    if not promoted and not pitfalls_path.exists():
        return False
    if not promoted:
        # Nothing to add or refresh; leave the (possibly hand-edited) file
        # byte-for-byte alone rather than re-normalizing it.
        return False
    old_text = pitfalls_path.read_text() if pitfalls_path.exists() else DEFAULT_PITFALLS_PREAMBLE
    preamble, blocks = split_bullets(old_text)
    for cluster in promoted:
        rendered = render_bullet(cluster)
        ident = cluster_identity(cluster)
        for i, block in enumerate(blocks):
            if not is_seed_bullet(block) and bullet_identity(block) == ident:
                blocks[i] = rendered
                break
        else:
            blocks.append(rendered)

    def assemble(bs):
        text = preamble
        if text and not text.endswith("\n"):
            text += "\n"
        return text + ("\n".join(bs) + "\n" if bs else "")

    while True:
        text = assemble(blocks)
        if len(blocks) <= PITFALLS_BULLET_CAP and len(text) <= PITFALLS_CHAR_CAP:
            break
        victim = next((i for i, b in enumerate(blocks) if not is_seed_bullet(b)), None)
        if victim is None:
            break  # only seeds left -- never dropped, even over cap
        del blocks[victim]

    if pitfalls_path.exists() and text == old_text:
        return False
    if pitfalls_path.exists():
        history_dir = Path(history_dir)
        stamp = time.strftime("%Y%m%dT%H%M%S", time.localtime(now))
        hist = history_dir / f"GLOBAL-PITFALLS-{stamp}.md"
        n = 1
        while hist.exists():
            hist = history_dir / f"GLOBAL-PITFALLS-{stamp}-{n}.md"
            n += 1
        atomic_write(hist, old_text, replace_fn)
    atomic_write(pitfalls_path, text, replace_fn)
    return True


# --- atomic files, lock, cursor ----------------------------------------------

def atomic_write(path, text, replace_fn=os.replace):
    """tempfile-in-same-dir + replace: readers only ever see a complete old
    or complete new file. replace_fn is injectable so tests can prove the
    cursor never advances past a failed output replace."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(text)
        replace_fn(tmp, str(path))
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def write_lock(lock_path, pid, script_sha, heartbeat_ts):
    """Lock writes always use the real os.replace: the lock is our own
    liveness signal and must not be entangled with injected output faults."""
    lock_path = Path(lock_path)
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps({"pid": pid, "script_git_sha": script_sha, "heartbeat_ts": heartbeat_ts})
    fd, tmp = tempfile.mkstemp(dir=str(lock_path.parent), prefix=".distiller.lock.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(payload)
        os.replace(tmp, str(lock_path))
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def acquire_lock(lock_path, pid, script_sha, now_fn=time.time, kill_fn=os.kill,
                 stale_seconds=STALE_HEARTBEAT_SECONDS):
    """Singleton discipline (K3). Returns False (caller exits 0 quietly)
    only when another live holder with a FRESH heartbeat runs the SAME
    script sha. A stale heartbeat means the holder died without cleanup; a
    sha mismatch means it runs outdated code -- either way it is SIGTERMed
    (best-effort: an already-dead pid is fine) and the lock taken over."""
    lock_path = Path(lock_path)
    if lock_path.exists():
        try:
            info = json.loads(lock_path.read_text())
        except (OSError, ValueError):
            info = None
        if isinstance(info, dict):
            holder = info.get("pid")
            heartbeat = info.get("heartbeat_ts")
            fresh = isinstance(heartbeat, (int, float)) and (now_fn() - heartbeat) < stale_seconds
            if fresh and info.get("script_git_sha") == script_sha and holder != pid:
                return False
            if isinstance(holder, int) and holder != pid:
                try:
                    kill_fn(holder, signal.SIGTERM)
                except (ProcessLookupError, PermissionError, OSError):
                    pass
    write_lock(lock_path, pid, script_sha, now_fn())
    return True


def release_lock(lock_path, pid):
    """Remove the lock only if it is still ours (a takeover may already
    have replaced it with the new holder's record)."""
    lock_path = Path(lock_path)
    try:
        if json.loads(lock_path.read_text()).get("pid") == pid:
            lock_path.unlink()
    except (OSError, ValueError):
        pass


def load_cursor(cursor_path):
    try:
        return int(Path(cursor_path).read_text().strip())
    except (OSError, ValueError):
        return 0


def compute_script_sha():
    """git HEAD of the checkout this script lives in (what the lock's
    sha-mismatch takeover compares); content hash as a fallback so the
    mechanism still works from an exported tree. Tests always inject
    --script-sha and never reach this."""
    script = Path(__file__).resolve()
    try:
        proc = subprocess.run(
            ["git", "-C", str(script.parent), "rev-parse", "HEAD"],
            capture_output=True, text=True, timeout=10,
        )
        if proc.returncode == 0 and proc.stdout.strip():
            return proc.stdout.strip()
    except OSError:
        pass
    return hashlib.sha1(script.read_bytes()).hexdigest()


# --- orchestration ------------------------------------------------------------

def distill_once(home, *, now_fn=time.time, kill_fn=os.kill, script_sha=None,
                 replace_fn=os.replace, pid=None):
    """One full distiller pass. Ordering is the crash-safety contract:

        read cursor -> fold new complete lines -> replace module files ->
        replace GLOBAL-PITFALLS (if changed) -> replace state -> replace cursor

    The cursor is written LAST, so any failure replays the batch next run;
    replay is idempotent via the state's applied-line hashes. The lock
    heartbeat is refreshed after each file written.
    """
    home = Path(home)
    logs_dir = home / "logs"
    knowledge_dir = logs_dir / "knowledge"
    lock_path = knowledge_dir / "distiller.lock"
    knowledge_dir.mkdir(parents=True, exist_ok=True)
    script_sha = script_sha or compute_script_sha()
    pid = os.getpid() if pid is None else pid
    if not acquire_lock(lock_path, pid, script_sha, now_fn, kill_fn):
        return {"status": "already_running"}

    def heartbeat():
        write_lock(lock_path, pid, script_sha, now_fn())

    try:
        state_path = knowledge_dir / "distiller.state.json"
        cursor_path = knowledge_dir / "distiller.cursor"
        state = load_state(state_path)
        offset = load_cursor(cursor_path)
        lines, new_offset = read_complete_lines(logs_dir / "lessons.jsonl", offset)
        applied = apply_events(state, lines)

        files_written = []
        for path, text in render_module_files(state["clusters"], knowledge_dir / "modules"):
            if path.exists() and path.read_text() == text:
                continue
            atomic_write(path, text, replace_fn)
            heartbeat()
            files_written.append(str(path))
        pitfalls_path = knowledge_dir / "GLOBAL-PITFALLS.md"
        if update_global_pitfalls(state["clusters"], pitfalls_path,
                                  knowledge_dir / "history", now_fn(), replace_fn):
            heartbeat()
            files_written.append(str(pitfalls_path))

        atomic_write(state_path, json.dumps(state, separators=(",", ":")), replace_fn)
        heartbeat()
        atomic_write(cursor_path, str(new_offset), replace_fn)
        return {
            "status": "ok",
            "events_applied": applied,
            "cursor": new_offset,
            "files_written": files_written,
        }
    finally:
        release_lock(lock_path, pid)


def parse_memory_bullets(text):
    """Extract "- ..." bullets (indented continuations folded in) from a
    legacy format-memory markdown file."""
    bullets, cur = [], None
    for line in text.splitlines():
        if line.startswith("- "):
            if cur:
                bullets.append(cur)
            cur = line[2:].strip()
        elif cur is not None and (line.startswith((" ", "\t"))) and line.strip():
            cur += " " + line.strip()
        else:
            if cur:
                bullets.append(cur)
            cur = None
    if cur:
        bullets.append(cur)
    return bullets


def migrate_format_memory(home, *, now_fn=time.time, kill_fn=os.kill,
                          script_sha=None, replace_fn=os.replace, pid=None):
    """One-time K3 migration of `<home>/logs/format-memory/*.md`.

    Surviving bullets (429/timeout/rate-limit noise dropped) become
    synthetic `structural` lesson events with module = the format name
    (the documented fallback key when module attribution is ambiguous),
    appended through the standard K1 contract so the normal distill pass
    absorbs them. Originals are then moved to `format-memory/archived/`
    -- after the events are safely in the ledger, so a crash in between
    at worst leaves the source files for a harmless re-run to notice.
    """
    home = Path(home)
    memory_dir = home / "logs" / "format-memory"
    lessons_path = home / "logs" / "lessons.jsonl"
    files = sorted(memory_dir.glob("*.md")) if memory_dir.is_dir() else []
    migrated = 0
    for path in files:
        format_name = path.stem
        for bullet in parse_memory_bullets(path.read_text()):
            if NOISE_RE.search(bullet):
                continue
            reason = re.sub(r"^\[\d{4}-\d{2}-\d{2}\]\s*", "", bullet).strip()
            if not reason:
                continue
            append_lesson(lessons_path, make_lesson(
                ts=now_fn(), worker="format-memory-migration",
                format_name=format_name, module=format_name,
                event="structural", reason=reason,
                evidence=f"format-memory/{path.name}",
            ))
            migrated += 1

    result = distill_once(home, now_fn=now_fn, kill_fn=kill_fn,
                          script_sha=script_sha, replace_fn=replace_fn, pid=pid)

    archived = []
    if files:
        archive_dir = memory_dir / "archived"
        archive_dir.mkdir(parents=True, exist_ok=True)
        for path in files:
            dest = archive_dir / path.name
            if dest.exists():
                dest = archive_dir / f"{path.stem}-{int(now_fn())}{path.suffix}"
            os.replace(str(path), str(dest))
            archived.append(dest.name)
    return {"status": result.get("status"), "migrated_events": migrated,
            "archived": archived, "distill": result}


def main(argv=None):
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--home", default=None,
                        help="OXIDEX_HOME override (default: $OXIDEX_HOME or ~/.oxidex)")
    parser.add_argument("--once", action="store_true",
                        help="single pass then exit -- this is also the default; the "
                             "dispatcher invokes the distiller periodically, the script "
                             "itself never loops")
    parser.add_argument("--migrate-format-memory", action="store_true",
                        help="one-time: distill legacy logs/format-memory/*.md into "
                             "structural lesson events (dropping 429/timeout noise) "
                             "and archive the originals")
    parser.add_argument("--script-sha", default=None,
                        help="override the script sha recorded in the lock (tests)")
    args = parser.parse_args(argv)

    home = home_dir(args.home)
    if args.migrate_format_memory:
        result = migrate_format_memory(home, script_sha=args.script_sha)
    else:
        result = distill_once(home, script_sha=args.script_sha)
    if result.get("status") == "already_running":
        return 0  # fresh same-sha holder: exit quietly (K3)
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
