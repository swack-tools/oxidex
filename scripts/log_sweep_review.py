#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Append sweep-review verdicts to the shared sweep-review-history log.

The tag-fix loop's own per-tag attempt history (run_tag_loop's persisted
state, see model_fix_loop.py's format_previous_attempts) only ever
captures a build/test *failure* on the exact same tag being retried. It
has no way to represent "this diff built, tested, and got merged into the
PR sweep -- but a human reviewer later found it wrong" (wrong tag name,
duplicate of existing code, formatting regression) or "this was correctly
accepted, here's why" -- both of which are exactly the kind of judgment
calls a fixer repeats blindly without ever seeing.

This script is the write side of that gap: whoever is doing sweep review
(a human, an agent doing the review, or the squad merger / overlord sweep
machinery) calls this once per commit reviewed, and model_fix_loop.py's
build_prompt reads recent entries back in (see load_recent_sweep_reviews/
format_sweep_review_history) so the next round targeting the same format
sees actual, specific outcomes -- not just generic static advice.

Spec K4 verdict classes: entries carry a ``verdict_class`` drawn from
``human_accepted | human_rejected | machine_accepted | machine_rejected |
reverted``. The legacy CLI spellings "accepted"/"rejected" are still taken
(and alias to human_accepted/human_rejected), and every entry keeps the
legacy binary ``verdict`` field so pre-K4 readers keep working unchanged.

Spec K1 lesson mirroring: every HUMAN verdict (accepted/rejected spellings,
human_accepted/human_rejected, and --revert) is unconditionally mirrored as
one event line in ``<home>/logs/lessons.jsonl`` (the fleet-wide append-only
ledger) so the human signal always reaches the distiller, not just the
per-format review window. ``--lesson "free text"`` is optional and merely
supplies the generalizable takeaway; without it the --reason is the lesson
text. Machine verdicts entered through this CLI mirror only when --lesson
is explicit (the merger/sweep log their own machine events). See
append_lesson_line for the atomicity contract (single os.write, 2000-byte
clamp).

Spec M5 tombstones: ``--revert <sha>`` appends a
``<iso-ts> REVERTED <FORMAT>:<tag>`` tombstone to landed-tags.log so
load_landed_tags lets the reverted tag re-enter the worker pool, and logs
a verdict_class=reverted review entry.

Spec M6 auto-generated entries: ``--from-commit <sha> --repo <path>`` /
``--from-range <a>..<b> --repo <path>`` parse each commit's evidence
trailers (git interpret-trailers --parse; Format:/Tag: per spec M1) and
write one entry per Tag: trailer without human typing, recording the
stable patch-id and deduping by (patch_id, reason) so a re-polled commit
writes nothing.

Usage:
    uv run scripts/log_sweep_review.py --format RW2 --tag IFD0:BlackLevelBlue \\
        --verdict accepted --reason "Matches this codebase's IFD0: convention"

    uv run scripts/log_sweep_review.py --format XMP --tag XMP:AboutCvTermCvId \\
        --verdict rejected --reason "Guessed [1,2] JSON-array syntax; real \\
        exiftool text output is comma-space-separated: 1, 2" --commit e77bffe \\
        --lesson "Verify list formatting against exiftool text output, not -j"

    uv run scripts/log_sweep_review.py --revert 2433c79 --format JPEG \\
        --tag MakerNotes:AELButton --reason "landed value wrong on Canon bodies"

    uv run scripts/log_sweep_review.py --from-range origin/main..squad/tiff \\
        --repo ~/.oxidex/worktrees/squad-staging/tiff --verdict machine_accepted
"""
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

# The ONE canonical K1 normalize+fingerprint implementation lives in
# distill_lessons.py (its norm_reason folds digit/hex runs and date
# stamps so "index 42" and "index 43" cluster). Every ledger writer must
# produce byte-identical fingerprints or the distiller accumulates
# fingerprint dialects and the K2/K3 promotion rule (>=3 occurrences
# across >=2 modules) silently undercounts -- so this script delegates
# instead of keeping its own copy.
from distill_lessons import (
    fingerprint_generic as _fingerprint_generic,
    fingerprint_scoped as _fingerprint_scoped,
    norm_reason as _norm_reason,
)

OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", str(Path.home() / ".oxidex")))
DEFAULT_LOG_PATH = OXIDEX_HOME / "logs" / "sweep-review-history.jsonl"
DEFAULT_LANDED_LOG_PATH = OXIDEX_HOME / "logs" / "landed-tags.log"

# --- Spec K4: verdict classes -------------------------------------------------

# Legacy CLI spellings, still accepted everywhere a verdict is taken.
VERDICT_ALIASES = {"accepted": "human_accepted", "rejected": "human_rejected"}

# The K4 vocabulary stored as verdict_class.
VERDICT_CLASSES = (
    "human_accepted", "human_rejected", "machine_accepted", "machine_rejected",
    "reverted",
)

# What pre-K4 readers see in the legacy "verdict" field. Old readers only
# understand the accepted/rejected binary, so each class degrades to the
# nearer pole; a revert means the landed fix was wrong, so it reads as
# rejected to them.
LEGACY_VERDICT = {
    "human_accepted": "accepted",
    "machine_accepted": "accepted",
    "human_rejected": "rejected",
    "machine_rejected": "rejected",
    "reverted": "rejected",
}

# --- Spec K1: lessons.jsonl event ledger --------------------------------------

# The full K1 event enum -- kept here (not just the subset this script
# emits) so append_lesson_line can validate any caller's event.
LESSON_EVENTS = frozenset({
    "build_failed", "gap_not_closed", "wrong_value", "test_regressed",
    "duplicate", "review_rejected", "critique", "fixed", "machine_accepted",
    "human_accepted", "human_rejected", "structural", "infra",
})

# --lesson mirrors the verdict into the closest K1 event. The enum has no
# machine_rejected or reverted member: a machine rejection is by definition
# a reviewer/merger rejection (review_rejected), and a revert is a human
# saying the landed fix was wrong (human_rejected).
LESSON_EVENT_FOR_VERDICT = {
    "human_accepted": "human_accepted",
    "human_rejected": "human_rejected",
    "machine_accepted": "machine_accepted",
    "machine_rejected": "review_rejected",
    "reverted": "human_rejected",
}

# K1 atomicity contract: one os.write per line, clamped to this many bytes
# (including the trailing newline). Readers skip malformed lines, so even
# a pathological hard clamp only ever loses that one event.
LESSON_LINE_MAX_BYTES = 2000


def resolve_verdict_class(verdict):
    """Map any accepted CLI/API spelling to its K4 verdict_class.

    Raises ValueError on anything outside the vocabulary -- same contract
    the pre-K4 append_sweep_review had for its accepted/rejected binary,
    just over the wider set."""
    verdict_class = VERDICT_ALIASES.get(verdict, verdict)
    if verdict_class not in VERDICT_CLASSES:
        raise ValueError(
            f"verdict must be one of {sorted(VERDICT_ALIASES)} or "
            f"{sorted(VERDICT_CLASSES)}, got {verdict!r}"
        )
    return verdict_class


def normalize_reason(reason):
    """The canonical K1 reason normalization -- a thin alias for
    distill_lessons.norm_reason (lowercase, leading [YYYY-MM-DD] stamp
    dropped, hex/digit runs folded to "#", whitespace collapsed, 200-char
    truncate). Two reviewers typing the same lesson with different
    spacing/casing/indices should cluster, not fork -- and this writer
    must normalize EXACTLY like the distiller or their fingerprints
    diverge into dialects that never share a cluster."""
    return _norm_reason(reason)


def compute_fingerprints(event, module, checklist_id, reason):
    """The two K1 fingerprints:

        fingerprint_scoped  = sha1(event, module, checklist_id or norm-reason)
        fingerprint_generic = sha1(event,         checklist_id or norm-reason)

    The generic one deliberately drops the module so "same mistake in
    Canon.pm and Nikon.pm" is clusterable by the distiller; the scoped one
    keeps per-module counts honest. Both are computed by the shared
    distill_lessons implementation (sha1 over unit-separator-joined
    fields), so events written here are byte-identical to events written
    by the distiller's migration or any other K1 writer."""
    return (
        _fingerprint_scoped(event, module, checklist_id, reason),
        _fingerprint_generic(event, checklist_id, reason),
    )


def build_lesson_event(event, reason, format_name=None, tag_key=None, worker=None,
                       module=None, table=None, evidence=None, checklist_id=None,
                       now_fn=time.time):
    """Assemble one complete K1 schema dict, fingerprints included.

    Every schema key is always present (None when unknown) so downstream
    consumers -- the distiller's grouping, the tail-window readers -- never
    have to .get() defensively across writers."""
    if event not in LESSON_EVENTS:
        raise ValueError(f"event must be one of {sorted(LESSON_EVENTS)}, got {event!r}")
    scoped, generic = compute_fingerprints(event, module, checklist_id, reason)
    return {
        "ts": time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(now_fn())),
        "worker": worker,
        "format": format_name,
        "module": module,
        "table": table,
        "tag_key": tag_key,
        "event": event,
        "reason": reason,
        "evidence": evidence,
        "checklist_id": checklist_id,
        "fingerprint_scoped": scoped,
        "fingerprint_generic": generic,
    }


def _clamp_lesson_line(event_dict):
    """Serialize event_dict to one newline-terminated JSON line of at most
    LESSON_LINE_MAX_BYTES bytes.

    Best effort first: an oversized line almost always means an oversized
    free-text reason, so the reason is truncated (on a UTF-8-safe boundary)
    until the line fits and stays valid JSON. Only if that still cannot fit
    (enormous evidence blob, say) does the hard byte clamp kick in -- which
    may leave the line malformed, and that is fine by contract: K1 readers
    skip malformed lines rather than degrading to {}."""
    raw = (json.dumps(event_dict, separators=(",", ":")) + "\n").encode("utf-8")
    reason = event_dict.get("reason")
    if len(raw) > LESSON_LINE_MAX_BYTES and isinstance(reason, str):
        trimmed = dict(event_dict)
        reason_bytes = reason.encode("utf-8")
        keep = max(0, len(reason_bytes) - (len(raw) - LESSON_LINE_MAX_BYTES))
        while True:
            trimmed["reason"] = reason_bytes[:keep].decode("utf-8", "ignore")
            raw = (json.dumps(trimmed, separators=(",", ":")) + "\n").encode("utf-8")
            if len(raw) <= LESSON_LINE_MAX_BYTES or keep == 0:
                break
            # json escaping (\uXXXX etc.) can inflate past the byte estimate;
            # shave the remaining overflow and re-dump. keep strictly
            # decreases, so this terminates.
            keep = max(0, keep - (len(raw) - LESSON_LINE_MAX_BYTES))
    if len(raw) > LESSON_LINE_MAX_BYTES:
        raw = raw[: LESSON_LINE_MAX_BYTES - 1] + b"\n"
    return raw


def append_lesson_line(home, event_dict):
    """Append one event to <home>/logs/lessons.jsonl per the K1 atomicity
    contract: open with os.open(O_APPEND|O_CREAT|O_WRONLY), then exactly ONE
    os.write of one newline-terminated line clamped to 2000 bytes.

    O_APPEND makes each single write land atomically at the tail even with
    the whole fleet appending concurrently (this is stricter than the
    PIPE_BUF hand-wave: one syscall, one line, bounded size). The file is
    never rotated or rewritten. Returns the exact bytes written."""
    path = Path(home) / "logs" / "lessons.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    raw = _clamp_lesson_line(event_dict)
    fd = os.open(path, os.O_APPEND | os.O_CREAT | os.O_WRONLY, 0o644)
    try:
        os.write(fd, raw)
    finally:
        os.close(fd)
    return raw


def append_sweep_review(log_path, format_name, tag, verdict, reason, commit=None, now_fn=time.time,
                        landed_log_path=None, patch_id=None, worker=None, table=None):
    """Append one JSON line. Appends are small single lines (well under
    PIPE_BUF), so this is safe without extra locking even with concurrent
    writers -- same reasoning as model_fix_loop.py's log_tag_found.

    verdict takes the legacy "accepted"/"rejected" spellings or any K4
    verdict_class (see resolve_verdict_class); the stored entry carries
    both the K4 ``verdict_class`` and the legacy binary ``verdict`` field
    so pre-K4 readers keep working (see LEGACY_VERDICT for the mapping).

    A human_accepted verdict (i.e. legacy "accepted") additionally appends
    "<iso-ts> <format>:<tag>" to landed_log_path (when given) -- the
    landed-tags set model_fix_loop.py's run_tag_loop reads back so workers
    stop re-deriving already-merged fixes. Machine acceptance deliberately
    does NOT write landed-tags: the squad merger's squad-status file is
    authoritative for machine state, and landed-tags stays the
    human-verified skip set (spec K4: machine_accepted is never presented
    as human-equivalent signal).

    patch_id/worker/table are the M6 auto-entry extras (patch_id is the
    (patch_id, reason) dedup key -- always stored, None for hand-typed
    entries; worker/table come from the M1 commit trailers)."""
    verdict_class = resolve_verdict_class(verdict)
    entry = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(now_fn())),
        "format": format_name,
        "tag": tag,
        "verdict": LEGACY_VERDICT[verdict_class],
        "verdict_class": verdict_class,
        "reason": reason,
        "commit": commit,
        "patch_id": patch_id,
    }
    if worker is not None:
        entry["worker"] = worker
    if table is not None:
        entry["table"] = table
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("a") as f:
        f.write(json.dumps(entry) + "\n")
    if verdict_class == "human_accepted" and landed_log_path:
        landed_log_path = Path(landed_log_path)
        landed_log_path.parent.mkdir(parents=True, exist_ok=True)
        with landed_log_path.open("a") as f:
            f.write(f"{entry['timestamp']} {format_name}:{tag}\n")
    return entry


def append_revert(log_path, landed_log_path, format_name, tag, sha, reason,
                  now_fn=time.time):
    """Spec M5 tombstone: record that an already-landed fix was reverted.

    Appends "<iso-ts> REVERTED <FORMAT>:<tag>" to landed-tags.log --
    load_landed_tags honors these tombstones so the reverted tag re-enters
    the worker pool instead of being suppressed forever (the
    permanent-suppression hazard from the 00:57:41 backfill) -- and logs a
    verdict_class=reverted sweep-review entry pointing at the reverted sha
    so future rounds see *why* it came back."""
    entry = append_sweep_review(
        log_path, format_name, tag, "reverted", reason, commit=sha,
        now_fn=now_fn, landed_log_path=None,
    )
    landed_log_path = Path(landed_log_path)
    landed_log_path.parent.mkdir(parents=True, exist_ok=True)
    with landed_log_path.open("a") as f:
        f.write(f"{entry['timestamp']} REVERTED {format_name}:{tag}\n")
    return entry


# --- Spec M6: auto-generated entries from commit trailers ---------------------


def make_git_runner(repo):
    """Real git runner: run_git(args, input_text=None) -> stdout, executed
    in repo via `git -C`. Tests inject a fake with the same signature
    instead (hermetic: no repo, no subprocess)."""
    def run_git(args, input_text=None):
        proc = subprocess.run(
            ["git", "-C", str(repo)] + list(args),
            input=input_text, capture_output=True, text=True, check=True,
        )
        return proc.stdout
    return run_git


def parse_commit_trailers(run_git, sha):
    """Parse one commit's trailers via `git interpret-trailers --parse`
    (the M1 evidence-trailer contract: Format:, repeatable Tag:, Sample:,
    Exiftool-Value:, Oxidex-Value:, Perl-Ref:, Verified:, Worker:, Table:).

    Returns an ordered list of (key, value) pairs; keys keep their
    canonical spelling, values are stripped. Repeated keys (Tag:) repeat
    in the list. interpret-trailers reads the message on stdin, which is
    why the runner takes input_text."""
    body = run_git(["log", "-1", "--format=%B", sha])
    parsed = run_git(["interpret-trailers", "--parse"], input_text=body)
    trailers = []
    for line in parsed.splitlines():
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        key, value = key.strip(), value.strip()
        if key:
            trailers.append((key, value))
    return trailers


def commit_patch_id(run_git, sha):
    """Stable patch-id for one commit (`git show <sha> | git patch-id
    --stable`). Recorded because cherry-pick and rebase-merge rewrite SHAs
    (spec M5): the patch-id is the only identity that survives, so it is
    the dedup key here and in quarantine/squad-status. Returns None when
    the diff is empty (patch-id emits nothing for empty commits)."""
    diff = run_git(["show", sha])
    out = run_git(["patch-id", "--stable"], input_text=diff)
    fields = out.split()
    return fields[0] if fields else None


def dedup_identity(patch_id, sha):
    """The commit-identity half of the M6 (patch_id, reason) dedup key.

    Patch-id when the commit has one; "sha:<sha>" for empty-diff commits
    (`git patch-id --stable` emits nothing for them, and empty commits
    with Tag: trailers arise naturally from the M2/M5 cherry-pick
    pipeline). Without the fallback, distinct empty commits would collide
    in-batch on (None, reason) and None-patch_id entries could never be
    reloaded, so every re-poll would re-import them."""
    return patch_id if patch_id else f"sha:{sha}"


def load_dedup_keys(log_path):
    """(identity, reason) pairs already in the review log -- the M6 dedup
    set (identity per dedup_identity: patch-id, or the recorded commit
    sha for patch-id-less entries). A re-polled merger failure or a
    re-run import must not flood the prompt window and evict human
    verdicts (the K4 eviction critique), so a key already present writes
    nothing. Malformed lines are skipped, matching every other reader of
    this log."""
    keys = set()
    log_path = Path(log_path)
    if not log_path.exists():
        return keys
    try:
        lines = log_path.read_text().splitlines()
    except OSError:
        return keys
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(entry, dict):
            continue
        if entry.get("patch_id"):
            keys.add((entry["patch_id"], entry.get("reason")))
        elif entry.get("commit"):
            # Stored patch_id=None (empty diff or hand-typed): the sha
            # fallback keeps re-polls of the same commit idempotent.
            keys.add((f"sha:{entry['commit']}", entry.get("reason")))
    return keys


def _trailer_value(trailers, key):
    """First value for key (case-insensitive), or None."""
    for k, v in trailers:
        if k.lower() == key.lower():
            return v
    return None


def append_from_commits(log_path, shas, run_git, verdict="machine_accepted",
                        reason=None, now_fn=time.time):
    """Spec M6: derive review entries from commit trailers, no human typing.

    Per commit: Format:/Tag: trailers give format and one entry per Tag,
    Worker:/Table: are carried along, `git patch-id --stable` gives the
    dedup identity (sha fallback for empty diffs, see dedup_identity).
    reason defaults to "auto: <subject>" -- deterministic across re-runs,
    which is what makes the (patch_id, reason) dedup actually catch a
    re-poll. A commit whose key is already in the log (or already seen
    earlier in this same batch -- same patch cherry-picked twice) writes
    nothing; a commit with no Tag: trailers has nothing to attribute and
    is skipped.

    Never touches landed-tags.log: machine imports are not the
    human-verified skip set (see append_sweep_review's rationale).

    Returns (written_entries, skipped_duplicate_shas)."""
    log_path = Path(log_path)
    seen = load_dedup_keys(log_path)
    written, skipped = [], []
    for sha in shas:
        trailers = parse_commit_trailers(run_git, sha)
        tags = [v for k, v in trailers if k.lower() == "tag" and v]
        if not tags:
            continue
        patch_id = commit_patch_id(run_git, sha)
        commit_reason = reason or f"auto: {run_git(['log', '-1', '--format=%s', sha]).strip()}"
        key = (dedup_identity(patch_id, sha), commit_reason)
        if key in seen:
            skipped.append(sha)
            continue
        seen.add(key)
        format_name = _trailer_value(trailers, "Format")
        for tag in tags:
            written.append(append_sweep_review(
                log_path, format_name, tag, verdict, commit_reason,
                commit=sha, now_fn=now_fn, patch_id=patch_id,
                worker=_trailer_value(trailers, "Worker"),
                table=_trailer_value(trailers, "Table"),
            ))
    return written, skipped


def resolve_range_shas(run_git, commit_range):
    """Expand --from-range's <a>..<b> into shas, oldest first (so log
    order matches merge order, like the rest of the append-only logs)."""
    out = run_git(["rev-list", "--reverse", commit_range])
    return [line.strip() for line in out.splitlines() if line.strip()]


def main(argv=None, run_git=None, now_fn=time.time):
    """CLI entry point. run_git/now_fn are injectable for hermetic tests
    (fake git, fixed clock); both default to the real thing."""
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--format", help="Format name, e.g. RW2, JPEG")
    parser.add_argument("--tag", help="Full tag key, e.g. IFD0:BlackLevelBlue")
    parser.add_argument(
        "--verdict",
        choices=sorted(VERDICT_ALIASES) + sorted(VERDICT_CLASSES),
        help="accepted/rejected (legacy, = human_*) or any K4 verdict class; "
             "defaults to machine_accepted in --from-commit/--from-range mode",
    )
    parser.add_argument("--reason", help="Why -- this is what future rounds will read")
    parser.add_argument("--commit", default=None, help="Short SHA, if applicable")
    parser.add_argument(
        "--lesson", default=None,
        help="Generalizable takeaway used as the mirrored K1 lesson event's "
             "reason (human verdicts always mirror to <home>/logs/"
             "lessons.jsonl, falling back to --reason; machine verdicts "
             "mirror only when this flag is given)",
    )
    parser.add_argument(
        "--revert", default=None, metavar="SHA",
        help="Log a revert of an already-landed fix: writes a REVERTED "
             "tombstone to the landed-tags log (spec M5) plus a "
             "verdict_class=reverted review entry; needs --format/--tag",
    )
    parser.add_argument(
        "--from-commit", default=None, metavar="SHA",
        help="Derive entries from this commit's evidence trailers (spec M6); "
             "needs --repo",
    )
    parser.add_argument(
        "--from-range", default=None, metavar="A..B",
        help="Derive entries from every commit in the range (spec M6); "
             "needs --repo",
    )
    parser.add_argument("--repo", default=None, help="Git repo the --from-* shas live in")
    parser.add_argument(
        "--home", default=None,
        help="OXIDEX_HOME override; default log paths derive from it "
             "(falls back to $OXIDEX_HOME, then ~/.oxidex)",
    )
    parser.add_argument("--log-path", default=None,
                        help="Review log; default <home>/logs/sweep-review-history.jsonl")
    parser.add_argument(
        "--landed-log", default=None,
        help="Landed-tags log appended to on accepted verdicts and REVERTED "
             "tombstones -- the skip set model_fix_loop.py workers re-read "
             "every round; default <home>/logs/landed-tags.log",
    )
    args = parser.parse_args(argv)

    home = Path(args.home) if args.home else OXIDEX_HOME
    log_path = Path(args.log_path) if args.log_path else home / "logs" / "sweep-review-history.jsonl"
    landed_log_path = Path(args.landed_log) if args.landed_log else home / "logs" / "landed-tags.log"

    from_mode = args.from_commit or args.from_range
    if sum(bool(x) for x in (args.revert, args.from_commit, args.from_range)) > 1:
        parser.error("--revert, --from-commit and --from-range are mutually exclusive")

    if from_mode:
        if not args.repo and run_git is None:
            parser.error("--from-commit/--from-range require --repo")
        if args.format or args.tag or args.lesson:
            parser.error("--format/--tag/--lesson don't apply in --from-* mode "
                         "(format and tags come from the commit trailers)")
        if run_git is None:
            run_git = make_git_runner(args.repo)
        shas = ([args.from_commit] if args.from_commit
                else resolve_range_shas(run_git, args.from_range))
        written, skipped = append_from_commits(
            log_path, shas, run_git,
            verdict=args.verdict or "machine_accepted",
            reason=args.reason, now_fn=now_fn,
        )
        for entry in written:
            print(f"logged: {entry['format']} {entry['tag']} {entry['verdict_class']} -> {log_path}")
        if skipped:
            print(f"skipped {len(skipped)} duplicate commit(s) by (patch_id, reason)")
        return 0

    if args.revert:
        if not (args.format and args.tag):
            parser.error("--revert requires --format and --tag")
        if args.verdict not in (None, "reverted"):
            parser.error("--revert implies --verdict reverted")
        reason = args.reason or f"reverted {args.revert}"
        entry = append_revert(log_path, landed_log_path, args.format, args.tag,
                              args.revert, reason, now_fn=now_fn)
        print(f"logged: {entry['format']} {entry['tag']} reverted -> {log_path} "
              f"(tombstone -> {landed_log_path})")
    else:
        if not (args.format and args.tag and args.verdict and args.reason):
            parser.error("--format, --tag, --verdict and --reason are required")
        entry = append_sweep_review(
            log_path, args.format, args.tag, args.verdict, args.reason, args.commit,
            now_fn=now_fn, landed_log_path=landed_log_path,
        )
        print(f"logged: {entry['format']} {entry['tag']} {entry['verdict']} -> {log_path}")

    # Spec K1: log_sweep_review mirrors EVERY human verdict into the
    # lessons ledger -- --lesson only swaps in the generalizable free
    # text; without it the --reason is the lesson. Gating the mirror on
    # an optional flag would starve the distiller of exactly the human
    # signal K1/K4 prioritize. Machine verdicts typed through this CLI
    # mirror only on an explicit --lesson (the merger/sweep log their own
    # machine events, so unconditional mirroring here would double-count).
    human_verdict = entry["verdict_class"] in (
        "human_accepted", "human_rejected", "reverted")
    if args.lesson or human_verdict:
        event = build_lesson_event(
            LESSON_EVENT_FOR_VERDICT[entry["verdict_class"]],
            args.lesson or entry["reason"],
            format_name=entry["format"], tag_key=entry["tag"],
            evidence={"commit": entry["commit"]} if entry["commit"] else None,
            now_fn=now_fn,
        )
        append_lesson_line(home, event)
        print(f"lesson: {event['event']} -> {home / 'logs' / 'lessons.jsonl'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
