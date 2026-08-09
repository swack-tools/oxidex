#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Judgment-queue daemon: the tier that finally CONSUMES quarantine.jsonl.

WHY THIS EXISTS
---------------
The fleet has three working tiers and one dead end:

    32 workers  ->  7 squad mergers  ->  auto_publish_round  ->  origin/main
                          |
                          +--> ~/.oxidex/logs/quarantine.jsonl  (nothing reads it)

``squad_merge_loop.process_commit`` quarantines a commit on ANY validator
flag (its module docstring, step "c"). Nothing consumes the result, so
flagged work accumulates forever and silently. That is the mechanism
behind the fleet's signature failure -- measured historically as "4330
gaps but only 2 landed tags".

Measured on the live ledger 2026-07-26/27 (81 lines, 45 distinct
patch-ids), grouped per COMMIT by flag-class set rather than per flag:

    13  printconv-unverifiable
     9  printconv-mismatch + printconv-unverifiable
     7  cherry-pick-conflict
     6  missing-trailer + printconv-unverifiable
     5  missing-trailer
     4  printconv-mismatch
     1  targeted-test-failed

Note the reframing versus the raw flag histogram (378 missing-trailer,
64 printconv-mismatch, ...): 378 is a FLAG count. It is 42 commits each
missing EVERY required trailer at once -- a legacy-emitter artifact, not
42 partial-evidence commits. Spot-checked c115b64c and da6647e2: the
message body is a bare subject line with no trailer block at all. 41 of
the 42 also carry the pre-POLICY_VERSION-2 ``missing-trailer:Table``
flag, i.e. they were rejected by a rule that no longer exists. The
CURRENT worker emitter is fine (live model-fix-parallel-canon-99 head
carries a complete, correct 8-trailer block).

THE PROMOTION PATH IS NOT OBVIOUS -- USE THE TRACED ONE
-------------------------------------------------------
Re-admission is NOT "clear the ledger". There is no delete API, and
clearing it would be wrong anyway. Two gates block a quarantined commit,
both inside squad_merge_loop:

  Gate A ``candidate_commits``  drops a sha already in squad-status heads
  Gate B ``process_commit``     returns "skipped_quarantined" on patch-id

Both gates only ever see shas surfaced by ``candidate_commits``, which
walks exactly two branch families: ``model-fix-parallel-<format>``
(legacy) and ``refs/heads/model-fix-parallel-<squad>-<n>`` (squad slots).
Measured 2026-07-26: ZERO of the 44 quarantined patch-ids is reachable
from any branch in either family. 40 survive only on hand-made
``wip/preserve-*`` refs; 4 are already fully dangling, kept alive by
nothing but git's GC grace period. So the POLICY_VERSION retry lever is
correctly implemented and completely inert -- it clears gates that are
downstream of a discovery pass the commits never reach.

The mechanism that actually works, and the only one this daemon uses:

  1. PRESERVE the commit first (``refs/judgment-queue/<patch-id>``).
     Quarantining is what MAKES a commit discardable --
     ``parallel_model_fix_loop._squad_status_resolved`` treats
     "quarantined" as resolved, which is what unblocks the worker's
     ``checkout -B`` re-anchor and the janitor's worktree reset. The
     window is a GC grace period, not forever.
  2. Re-apply it onto current origin/main in this daemon's OWN worktree
     (``<home>/worktrees/judgment-queue/<squad>`` -- never under
     parallel-fix/ or squad-staging/, which a live fleet owns).
  3. Fix what was actually wrong (re-derive trailers / re-try the
     cherry-pick), re-verify, and fast-forward a RESERVED slot branch
     ``model-fix-parallel-<squad>-700`` that the squad's merger already
     globs. Slot 700 is far outside ``allocate_squad_slots``' range
     (live slots observed: 1..11 plus the special 99) and still matches
     ``parallel_model_fix_loop._SQUAD_BRANCH_RE``'s ``-<digits>``
     requirement, so ``squad_from_branch`` maps it to the right squad.

The merger then re-validates from scratch and publishes through the
normal green-stamp path. THIS DAEMON NEVER WRITES squad-status, NEVER
writes quarantine.jsonl, and NEVER touches squad/<squad>. It only mints
git refs and appends to its own decision ledger. The merger stays the
authority on what is admissible; a mistake here costs a re-quarantine,
not a bad merge.

WHY A NAIVE RE-ADMIT WOULD SHIP FABRICATIONS
--------------------------------------------
Two of three fleet fixes on 2026-07-26 contained fabricated constants
that their own "recheck-pass gaps=N->M" trailer could not see:

  TTF 5249a506  added Macintosh language 12 => 'es' and 4 => 'it'.
    ExifTool's %ttLang{Macintosh} says 12 => 'ar' and 4 => 'nl-NL';
    Spanish is 6 and Italian is 3. Font.ttf holds a REAL Dutch record at
    4, so FontSubfamily-it would have carried Dutch text.
  RAR a998b8fc  added RAR5 host-OS 2/3/4 => MacOS/BeOS/OS-2 plus a
    catch-all _ => "Unknown". ExifTool's RAR5 table is exactly
    {0: Win32, 1: Unix} and prints the RAW NUMBER when no PrintConv
    matches, so "Unknown" REPLACED data ExifTool would have shown.

Both passed ``validate_fix_commit.check_printconv``, whose entire test is
``if value.encode("utf-8") in source: continue`` (validate_fix_commit.py
:1224) -- the strings "es" and "it" both appear in Font.pm. What was
wrong was the numeric-KEY -> string-VALUE PAIRING, and no pairing check
exists anywhere in the validator. So every promotion here goes through
``verify_enum_maps.verify``, which parses ExifTool's lookup tables and
diffs the PAIRS. Nothing is promoted on any other verdict than "clean".

SAFETY PROPERTIES (each one load-bearing, each one tested)
----------------------------------------------------------
* Dry-run is the DEFAULT. ``--apply`` is required to mutate anything.
  A dry run reads git and the ExifTool checkout, decides, and prints --
  it creates no refs, no branches, no worktrees, no ledger lines.
* Idempotent structurally, not just by bookkeeping. Before promoting,
  the patch-id is checked for novelty against origin/main AND
  squad/<squad> AND the slot branch (``git cherry``, patch-id equality).
  A crash between "branch fast-forwarded" and "ledger appended" cannot
  double-promote: the next poll sees the patch already on the branch.
* Keyed on PATCH-ID everywhere, never on sha. Rebases and cherry-picks
  change shas; the ledger and both merger gates are patch-id keyed.
* "rejected-permanent" is permanent THROUGH a policy bump, deliberately
  unlike every other verdict. A fabricated key->value pair does not stop
  being fabricated because the trailer rules moved. Re-adjudicating it
  would be an infinite loop over the same 13 commits.
* Every commit touched gets a structured ledger line with a reason.
  Silent promotion is the exact failure mode this daemon exists to end,
  so there is no code path that promotes without appending -- including
  the code path where adjudication RAISES.
* One poll's failure never kills the loop (``_run_poll_safely``, the
  same discipline as
  ``parallel_model_fix_loop._run_auto_publish_safely``) and one raising
  COMMIT never stalls the other 44: a raise on entry 3 of 45 would
  otherwise abandon the rest every poll, forever.
* A singleton lock in --apply mode. The structural guards make a double
  PROMOTION impossible but not two concurrent daemons: both would drive
  the same per-squad worktree, and the second one's ``checkout
  --detach`` would land mid-cherry-pick. Not taken in dry-run mode --
  an operator's read-only look should never be blocked by the daemon.

TWO GUARDS THAT LIVE DATA DEMANDED
----------------------------------
Neither was anticipated; both came out of running the thing.

* DNG 786ea09b (measured 2026-07-27) is a tag-id -> tag-KEY map,
  ``33421 => "EXIF:CFARepeatPatternDim"``. Bound to a PrintConv table it
  produces a confident "fabricated" verdict on a commit that contains no
  PrintConv at all. ``rejected-permanent`` is terminal, so a conviction
  now additionally requires the flagged value to be something
  ``validate_fix_commit.extract_added_map_values`` -- which already
  excludes tag keys, identifiers and templates -- calls a display value.
* A DERIVED module binding can only ever return "clean", never convict.
  Only a Perl-Ref the commit cites ITSELF is an attestation strong
  enough to reject on; a module this daemon picked is a guess about
  which table applies.

The printconv-unverifiable class would otherwise be a permanent dead
end, since ``check_printconv`` flags it BECAUSE the cited Perl-Ref does
not resolve. So a module PROVED by pair verification is written back
into the trailer -- replacing rather than appending (the validator reads
the first occurrence) and without starting a second trailer paragraph
(``git interpret-trailers --parse`` reads only the last one).

Usage:
    uv run scripts/judgment_queue_daemon.py --once              # dry run
    uv run scripts/judgment_queue_daemon.py --once --apply
    uv run scripts/judgment_queue_daemon.py --infinite --apply
"""
import argparse
import functools
import json
import os
import re
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import time
from pathlib import Path

from find_tag_gaps import OXIDEX_HOME, REPO_ROOT
from distill_lessons import (
    STALE_HEARTBEAT_SECONDS,
    acquire_lock,
    compute_script_sha,
    release_lock,
    write_lock,
)
import squad_merge_loop
import validate_fix_commit
import verify_enum_maps

SCRIPTS_DIR = Path(__file__).resolve().parent
# Squad ownership/formats live in config.toml's [squads.*] tables (moved
# there so there is exactly one fleet config file); REPO_ROOT, not
# SCRIPTS_DIR, since config.toml sits at the repo root, not inside scripts/.
DEFAULT_CONFIG_PATH = REPO_ROOT / "config.toml"

ORIGIN_MAIN = "origin/main"

# Bump when a change here can turn a previously non-terminal decision
# into a different one -- new triage classes, changed derivation rules,
# a changed promotion gate. Stored on every ledger line alongside
# validate_fix_commit.POLICY_VERSION and verify_enum_maps.VERIFIER_VERSION
# so a queued entry is re-examined exactly when the rules that queued it
# have moved, and not otherwise.
#
# History:
#   1  initial: the four measured triage classes plus the enum-pair gate
DAEMON_VERSION = 1

# 5 minutes. Deliberately slower than the mergers' 120s: the ledger is
# append-only and grows by a handful of entries an hour at the fleet's
# observed rate, and a promotion costs two full comparison runs
# (cargo builds) for the bookkeeping class.
DEFAULT_POLL_SECONDS = 300

# Reserved slot for judgment-queue re-admissions. Must (a) match
# squad_merge_loop.squad_slot_branches' glob
# refs/heads/model-fix-parallel-<squad>-*, (b) match
# parallel_model_fix_loop._SQUAD_BRANCH_RE's `-<digits>$` so the janitor
# maps the branch to the right squad, and (c) never collide with a live
# worker slot. allocate_squad_slots hands out 1..total_slots (32 on this
# host; live branches observed 2026-07-27 top out at 11, plus the
# special -99 canary), so 700 is safely out of range.
DEFAULT_SLOT = 700

# Durable preservation refs. Under refs/ but outside refs/heads/ so they
# are invisible to every branch glob in the fleet (squad_slot_branches,
# candidate_worker_branches, the janitor's worktree scan) and cannot be
# mistaken for work to consume -- they exist only to hold the objects
# open against git gc.
PRESERVE_REF_PREFIX = "refs/judgment-queue"

# Verdicts that are never revisited, no matter how the rules move.
# "promoted" and "already-landed" because the work is downstream now;
# "rejected-permanent" because a fabricated key->value pair is a fact
# about ExifTool's tables, not about our policy (see module docstring).
TERMINAL_VERDICTS = frozenset({"promoted", "rejected-permanent", "already-landed"})

# Flag prefixes -> triage class, most-blocking first. Order is the
# precedence: a commit carrying both missing-trailer and
# printconv-mismatch is a printconv case, because rewriting its
# bookkeeping would not make a wrong pair right.
#
# The "semantic" class covers rejections that were reached by MEASURING
# the commit's effect on the corpus, not by reading its paperwork:
# duplicate-emission / new-oxidex-only came from a pre/post comparison,
# sweep-bisection came from overlord_sweep isolating the commit as the
# thing that broke a sweep, ff-refused is a should-never-happen anomaly.
# None of those is re-adjudicable from the diff, and re-offering a
# bisection-isolated commit is actively dangerous -- so this daemon
# leaves them queued with a reason and never promotes them.
_CLASS_RULES = (
    ("test-failed", ("targeted-test-failed",)),
    ("semantic", ("duplicate-emission", "new-oxidex-only", "sweep-bisection", "ff-refused")),
    ("printconv", ("printconv-mismatch", "printconv-wrong-perl-ref")),
    ("cherry-pick-conflict", ("cherry-pick-conflict",)),
    ("bookkeeping", ("missing-trailer",)),
    ("printconv-unverifiable", ("printconv-unverifiable",)),
)


# ---------------------------------------------------------------------------
# Paths / names
# ---------------------------------------------------------------------------

def decision_ledger_path(home):
    """This daemon's OWN append-only decision log. Deliberately a
    separate file from quarantine.jsonl: that ledger is the mergers'
    write surface and three other processes read it, so a consumer that
    also wrote to it would make "who said this" unanswerable."""
    return Path(home) / "logs" / "judgment-queue.jsonl"


def daemon_lock_path(home):
    """Singleton lock, alongside every other fleet daemon's under
    logs/knowledge/. Reuses distill_lessons' generically-named lock
    helpers rather than duplicating the takeover logic, exactly as
    squad_merge_loop.run_locked does."""
    return Path(home) / "logs" / "knowledge" / "judgment-queue.lock"


def judgment_worktree_dir(home, squad):
    """One worktree per squad, under a base dir the dispatcher's janitor
    does not scan. discover_worktree_candidates filters on
    --worktree-dir (parallel-fix/), so nothing here is ever a janitor
    reset candidate -- verified against parse_worktree_list's contract,
    not assumed."""
    return Path(home) / "worktrees" / "judgment-queue" / squad


def preserve_ref_name(patch_id):
    return f"{PRESERVE_REF_PREFIX}/{patch_id}"


def slot_branch_name(squad, slot=DEFAULT_SLOT):
    return f"model-fix-parallel-{squad}-{slot}"


def ruleset_id(daemon_version=None, policy_version=None, verifier_version=None):
    """The identity of the rules a decision was reached under. A
    non-terminal decision is re-examined exactly when this string
    changes -- which is the generalisation of the brief's "re-adjudicate
    on policy bump": the validator is not the only thing whose rules can
    move, and a verifier change is just as much a reason to look again."""
    d = DAEMON_VERSION if daemon_version is None else daemon_version
    p = validate_fix_commit.POLICY_VERSION if policy_version is None else policy_version
    v = verify_enum_maps.VERIFIER_VERSION if verifier_version is None else verifier_version
    return f"d{d}/p{p}/v{v}"


# ---------------------------------------------------------------------------
# Git (list-argv only, no shell=True; one choke point so tests can watch it)
# ---------------------------------------------------------------------------

def _git(args, cwd, check=True, input_text=None):
    return subprocess.run(  # nosec B603
        ["git", *args], cwd=cwd, capture_output=True, text=True,
        check=check, input=input_text,
    )


def commit_exists(repo_root, sha):
    """True when `sha` still names a commit object in this repo. Four of
    the 44 quarantined shas were measured DANGLING on 2026-07-26 -- no
    ref contains them -- so this is a real branch, not defensive noise."""
    if not sha:
        return False
    result = _git(["cat-file", "-e", f"{sha}^{{commit}}"], repo_root, check=False)
    return result.returncode == 0


def ref_exists(repo_root, ref):
    result = _git(["rev-parse", "--verify", "--quiet", ref], repo_root, check=False)
    return result.returncode == 0


def rev_parse(repo_root, ref):
    result = _git(["rev-parse", "--verify", "--quiet", ref], repo_root, check=False)
    return result.stdout.strip() if result.returncode == 0 else None


def preserve_commit(repo_root, patch_id, sha, *, apply=False):
    """Pin `sha` under refs/judgment-queue/<patch-id> so git gc cannot
    reclaim it while this daemon is deciding what to do with it.

    This is the FIRST thing done to any ledger entry, before triage,
    because quarantining is precisely what makes a commit discardable:
    parallel_model_fix_loop._squad_status_resolved counts "quarantined"
    as resolved, which is what permits the worker's `checkout -B`
    re-anchor and janitor_reset_stale_worktrees to drop the branch that
    was holding it. Measured 2026-07-26: 4 of 44 quarantined shas were
    already reachable from no ref at all.

    Returns one of "preserved", "already-preserved", "would-preserve"
    (dry run), "missing" (object is already gone), "failed".
    """
    if not commit_exists(repo_root, sha):
        return "missing"
    ref = preserve_ref_name(patch_id)
    existing = rev_parse(repo_root, ref)
    if existing == sha:
        return "already-preserved"
    if not apply:
        return "would-preserve"
    result = _git(["update-ref", ref, sha], repo_root, check=False)
    return "preserved" if result.returncode == 0 else "failed"


def create_or_fast_forward(repo_root, branch, new_sha, *, apply=False):
    """Publish `new_sha` on `branch`, creating the branch if absent and
    otherwise refusing anything that is not a genuine fast-forward.

    squad_merge_loop.fast_forward_branch is the model and is reused for
    the move; it cannot CREATE, because its `merge-base --is-ancestor
    <branch> <new>` guard fails outright on a branch that does not exist
    yet. The append-only property is what matters and is preserved in
    both arms: a slot branch this daemon owns only ever grows.

    Returns (ok, message).
    """
    if not apply:
        return True, "dry-run: would publish"
    if squad_merge_loop.branch_exists(repo_root, branch):
        return squad_merge_loop.fast_forward_branch(repo_root, branch, new_sha)
    result = _git(["update-ref", f"refs/heads/{branch}", new_sha], repo_root, check=False)
    if result.returncode != 0:
        return False, (result.stdout + result.stderr).strip()
    return True, "created"


def ensure_judgment_worktree(repo_root, path, *, origin_ref=ORIGIN_MAIN, apply=False, log_fn=print):
    """A dedicated worktree for this daemon's squad, detached at
    origin_ref. Never reuses a fleet worktree: parallel-fix/* holds 32
    live workers and squad-staging/* holds 7 live mergers, any of which
    may be mid-cherry-pick with no lock coordination available to us."""
    path = Path(path)
    if not apply:
        return False
    if (path / ".git").exists():
        return True
    path.parent.mkdir(parents=True, exist_ok=True)
    _git(["worktree", "add", "--detach", str(path), origin_ref], repo_root)
    log_fn(f"judgment-queue: created worktree {path} (detached at {origin_ref})")
    return True


# ---------------------------------------------------------------------------
# Decision ledger
# ---------------------------------------------------------------------------

def load_decisions(path):
    """{patch_id: newest decision} folded from the append-only ledger.

    "Newest" is by ts_epoch, with a stable fall back to file order for
    lines written before that field existed -- the same lenient-reader
    discipline as squad_merge_loop.load_quarantine, and for the same
    reason: 77 of the 80 live quarantine entries predate a schema
    addition, so a reader that assumes any field exists KeyErrors on
    100% of real rows.
    """
    decisions = {}
    path = Path(path)
    if not path.exists():
        return decisions
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return decisions
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
        patch_id = entry.get("patch_id")
        if not patch_id:
            continue
        prior = decisions.get(patch_id)
        if prior is None:
            decisions[patch_id] = entry
            continue
        # A TERMINAL verdict always wins over a later non-terminal one.
        # Without this, a stray "queued" line appended after a
        # "promoted" (a concurrent second daemon, a hand-edited ledger)
        # would silently re-open a patch-id that is already downstream.
        if prior.get("verdict") in TERMINAL_VERDICTS and entry.get("verdict") not in TERMINAL_VERDICTS:
            continue
        if entry.get("ts_epoch", 0) >= prior.get("ts_epoch", 0):
            decisions[patch_id] = entry
    return decisions


def append_decision(path, entry, *, apply=False):
    """One O_APPEND|O_CREAT|O_WRONLY open plus exactly one os.write of
    one line -- the fleet's standard atomic-JSONL append (K1 style, same
    as squad_merge_loop.append_quarantine). A dry run never writes."""
    if not apply:
        return entry
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    line = (json.dumps(entry, separators=(",", ":"), default=str) + "\n").encode("utf-8")
    fd = os.open(str(path), os.O_APPEND | os.O_CREAT | os.O_WRONLY, 0o644)
    try:
        os.write(fd, line)
    finally:
        os.close(fd)
    return entry


def make_decision(*, patch_id, sha, format_name, squad, klass, verdict, reason,
                  detail=None, attempt=0, promoted_branch=None, promoted_sha=None,
                  dry_run=True, now_fn=time.time, ruleset=None):
    """Build one structured decision record. Every commit this daemon
    looks at produces exactly one of these, including the ones it
    decides to leave alone -- "silent promotion is exactly the failure
    mode this daemon exists to end" cuts both ways: a silent SKIP is how
    the quarantine tier became invisible in the first place."""
    stamped_at = now_fn()
    return {
        # Same pair, same reasoning as squad_merge_loop.record_head's:
        # a human-readable naive-local `ts` plus an unambiguous instant,
        # from ONE clock read so an advancing test clock cannot make the
        # two disagree.
        "ts": time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(stamped_at)),
        "ts_epoch": float(stamped_at),
        "patch_id": patch_id,
        "sha": sha,
        "format": format_name,
        "squad": squad,
        "class": klass,
        "verdict": verdict,
        "reason": reason,
        "detail": detail or {},
        "attempt": attempt,
        "promoted_branch": promoted_branch,
        "promoted_sha": promoted_sha,
        "dry_run": bool(dry_run),
        "ruleset": ruleset or ruleset_id(),
        "daemon_version": DAEMON_VERSION,
        "policy_version": validate_fix_commit.POLICY_VERSION,
        "verifier_version": verify_enum_maps.VERIFIER_VERSION,
    }


def needs_adjudication(quarantine_entry, decision, ruleset=None):
    """Should this patch-id be looked at this poll?

    Never adjudicated before -> yes. A TERMINAL verdict -> never again
    (this is what stops the "same patch-id re-adjudicated in a loop" the
    brief forbids). A dry-run decision is advisory and never counts as
    having decided anything. Otherwise: only when the ruleset that
    produced the decision has moved, or the ledger has recorded a NEWER
    attempt than the one we judged.

    That last clause generalises the brief's "re-adjudicate on policy
    bump" correctly. squad_merge_loop.stale_policy keys purely on
    validate_fix_commit.POLICY_VERSION, but this daemon's verdicts also
    depend on verify_enum_maps' rules and its own triage, and all three
    must be able to trigger a fresh look.
    """
    if decision is None:
        return True
    if decision.get("dry_run"):
        return True
    if decision.get("verdict") in TERMINAL_VERDICTS:
        return False
    if decision.get("ruleset") != (ruleset or ruleset_id()):
        return True
    return (quarantine_entry or {}).get("attempt", 0) > decision.get("attempt", 0)


# ---------------------------------------------------------------------------
# Triage
# ---------------------------------------------------------------------------

def classify_flags(flags):
    """Triage class for one quarantine entry's flag list.

    Flags carry payloads after a colon (missing-trailer:Format,
    printconv-mismatch:<excerpt>), so matching is on the prefix before
    the first colon -- matching on the whole flag would silently classify
    every real printconv-mismatch as "unknown".
    """
    prefixes = {str(f).split(":", 1)[0] for f in (flags or [])}
    for klass, members in _CLASS_RULES:
        if prefixes & set(members):
            return klass
    return "unknown"


# ---------------------------------------------------------------------------
# Gap-report helpers (find_tag_gaps' ComparisonReport shape)
# ---------------------------------------------------------------------------

def tag_key_of(entry):
    """The "<family>:<name>" key for one gap entry.

    The two gap lists disagree on shape and always have:
    value_differences entries carry a combined "tag_key" ("EXIF:ISO"),
    missing_in_oxidex entries carry separate "family"/"name" fields
    (verified against a real /tmp/tagcmp-CR2-CR2.json). Reading only one
    spelling loses half the gaps."""
    if not isinstance(entry, dict):
        return None
    key = entry.get("tag_key")
    if key:
        return key
    name = entry.get("name")
    if not name:
        return None
    family = entry.get("family")
    return f"{family}:{name}" if family else name


def gap_index(report):
    """{tag_key: entry} across both gap lists of one per-format gap dict
    (squad_merge_loop.real_format_match's return value). A None report
    means the format has ZERO gaps -- find_tag_gaps.group_gaps_by_format
    drops formats with gap_count == 0 entirely -- so it indexes to {},
    which is exactly right and must not be confused with "no data"."""
    index = {}
    if not report:
        return index
    for key in ("missing_tags", "value_differences"):
        for entry in report.get(key) or []:
            tag = tag_key_of(entry)
            if tag and tag not in index:
                index[tag] = entry
    return index


def gap_count_of(report):
    if not report:
        return 0
    count = report.get("gap_count")
    if isinstance(count, int):
        return count
    return len(report.get("missing_tags") or []) + len(report.get("value_differences") or [])


def exiftool_value_of(entry):
    """ExifTool's value for one gap entry. missing_in_oxidex spells it
    "value"; value_differences spells it "exiftool_value"."""
    if not isinstance(entry, dict):
        return None
    value = entry.get("exiftool_value")
    if value is None:
        value = entry.get("value")
    return value


# ---------------------------------------------------------------------------
# Perl-Ref derivation
# ---------------------------------------------------------------------------

@functools.lru_cache(maxsize=512)
def _module_bytes(path_str):
    try:
        return Path(path_str).read_bytes()
    except OSError:
        return b""


def candidate_perl_modules(perl_lib):
    """Every real tag-table .pm under perl_lib, Lang/ excluded.

    Same corpus restriction validate_fix_commit._perl_lib_corpus applies
    and for the same measured reason: Image/ExifTool/Lang/*.pm is 16.3%
    of the tree and is nothing but translated UI strings, which hand out
    free substring matches to almost any plausible fabrication."""
    perl_lib = Path(perl_lib)
    root = perl_lib / "Image" / "ExifTool"
    base = root if root.is_dir() else perl_lib
    return [p for p in sorted(base.rglob("*.pm")) if "Lang" not in p.parts]


def find_perl_module_for_pairs(diff_text, perl_lib, table_hints=(), *,
                               verify_fn=None, modules_fn=candidate_perl_modules):
    """The Perl-Ref this commit's own diff can PROVE, or an abstention.

    Perl-Ref is the one required trailer that is NOT derivable from the
    diff plus a comparison run, and guessing it now BLOCKS rather than
    warns: printconv-wrong-perl-ref left WARN_ONLY_FLAG_PREFIXES at
    POLICY_VERSION 5, measured over 9,600 wrong-table trials at a 97.3%
    block rate. So this does not guess. It byte-prefilters the corpus to
    modules that literally contain every added display value, then asks
    the pair verifier, and accepts a module only if EXACTLY ONE comes
    back "clean" having actually checked pairs.

    Returns (Path|None, reason, verdict|None, hint|None). reason is
    "verified" on success, "not-required" when the diff adds no map-like
    values at all (the CONDITIONAL_TRAILERS case validate_commit already
    honours), "perl-ref-underivable" when nothing verifies, or
    "perl-ref-ambiguous:<n>" when several modules do.

    CRITICAL: this can only ever return a CLEAN verdict, never a
    fabricated one, and the clean verdict it returns is carried out
    together with the hint that produced it. A derived binding is not
    authoritative enough to convict on -- see the call site in
    adjudicate, and the DNG 786ea09b false positive measured 2026-07-27
    that made that rule necessary.
    """
    verify_fn = verify_fn or verify_enum_maps.verify
    pairs = [p for p in verify_enum_maps.extract_rust_pairs(diff_text) if p.kind == "pair"]
    if not pairs:
        return None, "not-required", None, None
    wanted = [p.value.encode("utf-8") for p in pairs if p.value]
    hits = []
    for module in modules_fn(perl_lib):
        source = _module_bytes(str(module))
        if not source or any(value not in source for value in wanted):
            continue
        for hint in [None, *table_hints]:
            verdict = verify_fn(diff_text, module, hint)
            if verdict.status == "clean" and verdict.pairs_checked:
                hits.append((module, verdict, hint))
                break
    if len(hits) == 1:
        module, verdict, hint = hits[0]
        return module, "verified", verdict, hint
    if not hits:
        return None, "perl-ref-underivable", None, None
    return None, f"perl-ref-ambiguous:{len(hits)}", None, None


# ---------------------------------------------------------------------------
# The anti-fabrication gate
# ---------------------------------------------------------------------------

_CANDIDATE_RE = re.compile(r"^(?:.*::)?(\w+)\[([^\]]+)\]$")


def candidate_hints(verdict):
    """Retry hints recovered from an "ambiguous-table" abstention.

    verify_enum_maps refuses a bare tag name that matches several tables
    and NAMES them: ZIP.pm holds three disagreeing OperatingSystem
    PrintConvs, reported as ('Image::ExifTool::ZIP::GZIP[9]',
    'Image::ExifTool::ZIP::RAR[8]',
    'Image::ExifTool::ZIP::RAR5[OperatingSystem]'). Those names convert
    directly into the "Table.Component" hint form the verifier accepts,
    which is how the RAR case gets its real evidence -- with the bare
    hint the verdict rests on the catch-all arm alone, and with
    RAR5.OperatingSystem it names keys 2/3/4 as absent from a table that
    is exactly {0: Win32, 1: Unix}.
    """
    out = []
    for name in getattr(verdict, "candidates", ()) or ():
        match = _CANDIDATE_RE.match(str(name))
        hint = f"{match.group(1)}.{match.group(2)}" if match else str(name)
        if hint not in out:
            out.append(hint)
    return out


def agreements(verdict):
    """Pairs that MATCHED. The best-supported table binding is the one
    the diff agrees with most -- ZIP.pm's three OperatingSystem tables
    all return "fabricated" for the RAR diff, but only RAR5 agrees on
    0 => Win32 and 1 => Unix, and only RAR5 is the table ExifTool
    actually uses there.

    Also the plausibility floor for a TERMINAL rejection: see
    pair_evidence_is_trustworthy.
    """
    return verdict.pairs_checked - len(verdict.mismatches)


def diff_block_count(diff_text):
    """How many distinct Rust match tables this diff adds.

    verify_enum_maps groups every extracted pair by its enclosing
    `match {}` (RustPair.block) precisely so two unrelated tables in one
    diff never collide. That count is what decides whether a FORCED
    table hint can be trusted -- see pair_evidence_is_trustworthy.

    Keyed on (file, block, block_key), not block alone: `block` is a
    line-ish ordinal within its own hunk, so two tables in two files can
    and do share one. `block_key` is None whenever the block has no
    enclosing arm to name it, which is most of them.
    """
    return len({(p.file, p.block, p.block_key)
                for p in verify_enum_maps.extract_rust_pairs(diff_text)})


def pair_evidence_is_trustworthy(verdict, hint, block_count):
    """May this "fabricated" verdict's PAIR mismatches be used as
    evidence for a TERMINAL rejection? Returns (ok, why_not).

    A string-valued catch-all is deliberately NOT considered here. It is
    the other, independent kind of evidence: it needs no table at all,
    because ExifTool prints the RAW NUMBER when no PrintConv matches, so
    `_ => "Unknown"` replaces data it would have shown whichever table is
    right. Keeping the two apart is what stops a commit being convicted
    on its catch-all while the log headlines pair mismatches that came
    from a misbinding -- measured on CR2 629f23a4 and 92e457f6, both of
    which did exactly that.

    Two refusals, both measured on the live ledger 2026-07-27 rather
    than imagined. An earlier, flatter rule convicted 13 of 51 entries
    and at least 7 of those were table misbindings, which a terminal
    verdict would have made permanent:

    1. Zero agreements is a misbinding, not N inventions. A real
       fabrication sits BESIDE correct values -- RAR a998b8fc agrees on
       0 => Win32 and 1 => Unix and disagrees on 2/3/4.

    2. A FORCED hint on a diff that adds more than one table cannot be
       trusted even when it shows agreements, and this is the subtle
       one. An explicit hint binds EVERY block in the diff to ONE table,
       so on a multi-table commit the correctly-bound block supplies the
       agreements while the misbound blocks supply the "fabrications" --
       the verdict looks corroborated and is not. DNG 786ea09b scored 9
       agreements out of 22 against Exif::Main[0xc617] (CFALayout)
       because the diff genuinely adds CFALayout AND a CFA colour map,
       and only the colour map was misbound. Same shape for a688591c,
       a04bc839 and c446aaf8. Per-block auto-binding (hint None) is
       exempt -- adjudicating table by table is precisely what it does
       -- and so is a genuinely single-table diff, which is the shape
       verify_enum_maps documents as needing a hint (RAR's
       `fn rar5_host_os(raw: u8)`, with no enclosing arm to bind to).

    Refusing does NOT promote the commit -- the outcome is "queued".
    The safety rule is "never promote anything not clean", not "always
    reject", so being permissive about queuing and severe about a
    terminal verdict is safe in both directions.
    """
    if agreements(verdict) <= 0:
        return False, "zero-agreement"
    if hint is not None and block_count > 1:
        return False, "forced-hint-on-multi-table-diff"
    return True, None


def _is_decisive(verdict):
    """Did this attempt actually settle the question?

    "fabricated" always did. "clean" did only if it checked something --
    verify_enum_maps' own warning is that a clean with pairs_checked == 0
    means nothing was verified, not that everything was right. The one
    honest exception is `no-enum-pairs-in-diff`, where zero is the true
    answer because there is nothing in the diff to check.
    """
    if verdict.status == "fabricated":
        return True
    if verdict.status != "clean":
        return False
    return verdict.pairs_checked > 0 or verdict.reason == "no-enum-pairs-in-diff"


def run_verifier(diff_text, pm_path, table_hints=(), *, verify_fn=None):
    """verify_enum_maps over one diff. Returns (Verdict, hint_used).

    Three tiers, and which one gets to decide the CLASS is the whole
    safety argument. Measured on the live ledger 2026-07-27 by running
    all three against the 12 commits an earlier, flatter rule convicted.

    Tier 1 -- PER-BLOCK AUTO-BINDING (hint=None) is authoritative
    whenever it reaches a verdict. This is the mode verify_enum_maps was
    designed around: it binds each Rust match block to the tag id of its
    own enclosing arm, so a diff adding several tables is adjudicated
    table by table instead of being flattened into one colliding key
    space. An explicit hint forces EVERY block in the diff onto ONE
    table, which on a multi-tag commit is guaranteed to misbind most of
    them. That is precisely what produced the bogus convictions: DNG
    786ea09b's `0 => "Red", 1 => "Green"` measured against a table
    reading `1 => 'Rectangular'` -- a CFA colour map compared with
    CFALayout. Under auto-binding those five commits come back
    cannot-verify and are queued, which is the honest answer.

    Tier 2 -- the commit's own Tag: trailers, tried ONLY when
    auto-binding abstains. That is the single-table shape the verifier
    documents as needing a hint (the RAR `fn rar5_host_os(raw: u8)`
    case, where there is no enclosing arm to bind to). Among these,
    fabricated beats clean and the best-supported binding wins: a wrong
    author-supplied hint can make a real fabrication look adjudicable
    against some other table, but it cannot invent a key->value
    disagreement out of nothing.

    Tier 3 -- EVIDENCE ONLY, never the class. When the answer is already
    "fabricated", the candidate tables an "ambiguous-table" abstention
    NAMED are retried and the best-supported one is reported. This is
    what turns the RAR verdict from "some catch-all is wrong" into
    "keys 2/3/4 are absent from a table that is exactly {0: Win32,
    1: Unix}". Confined to evidence because an expanded hint is not the
    commit's own attestation -- if it could vote on the class, a
    genuinely clean commit whose tag name happens to be ambiguous would
    be convicted by whichever sibling table disagrees with it.
    """
    verify_fn = verify_fn or verify_enum_maps.verify
    primary = (verify_fn(diff_text, pm_path, None), None)

    # A decisively CLEAN auto-binding ends it: there is no class left to
    # settle and no fabrication evidence to enrich. Skipping the hints
    # here is also the difference between one parse and one per Tag:
    # trailer, and a multi-tag commit carries a dozen.
    if _is_decisive(primary[0]) and primary[0].status == "clean":
        return primary

    hinted = [(verify_fn(diff_text, pm_path, hint), hint) for hint in table_hints]
    if not _is_decisive(primary[0]):
        fabricated = [a for a in hinted if a[0].status == "fabricated"]
        clean = [a for a in hinted if a[0].status == "clean"]
        if fabricated:
            primary = max(fabricated, key=lambda a: (agreements(a[0]), a[0].pairs_checked))
        elif clean:
            primary = max(clean, key=lambda a: a[0].pairs_checked)

    if primary[0].status != "fabricated":
        return primary

    attempts = [primary, *hinted]

    expanded = []
    for verdict, _ in attempts:
        for hint in candidate_hints(verdict):
            retry = verify_fn(diff_text, pm_path, hint)
            if retry.status == "fabricated":
                expanded.append((retry, hint))
    return max([primary, *expanded], key=lambda a: (agreements(a[0]), a[0].pairs_checked))


def bare_tag_names(trailers):
    """Tag: trailer values reduced to the bare tag name verify_enum_maps
    accepts as a hint ("EXIF:CustomRendered" -> "CustomRendered"), plus
    the full key, deduplicated in order."""
    hints = []
    for value in trailers.get("Tag", []) if trailers else []:
        if not value:
            continue
        for candidate in (value.partition(":")[2], value):
            if candidate and candidate not in hints:
                hints.append(candidate)
    return hints


# ---------------------------------------------------------------------------
# Trailer re-derivation (the dominant, bookkeeping-only class)
# ---------------------------------------------------------------------------

def derive_trailers(*, pre, post, format_name, worker, perl_ref, max_tags=8):
    """Re-derive the REQUIRED_TRAILERS block from measured evidence.

    Inputs are the two per-format gap reports taken around the re-applied
    commit -- `pre` at origin/main, `post` with the commit applied -- and
    that measurement is the whole point: every value here is observed,
    none is invented.

      Format         the merger already recorded it on the ledger entry.
      Tag            every tag_key present in `pre` and absent from
                     `post`, i.e. the gaps this commit actually closed.
                     Repeatable per spec M1.
      Sample         the closed gap entry's own "source_file". No samples
                     cache lookup needed -- the comparison report already
                     names the file the gap was observed on.
      Exiftool-Value the closed gap entry's ExifTool value.
      Oxidex-Value   the same string. This is a measurement, not a copy:
                     the gap appearing in `pre` and not in `post` IS the
                     statement that oxidex now emits what ExifTool emits
                     for that tag on that sample.
      Verified       "recheck-pass gaps=<before>-><after>". overlord_sweep
                     parses exactly this shape
                     (_VERIFIED_DELTA_RE = r"gaps=(\\d+)->(\\d+)") and sums
                     the deltas into its post-merge gate, so a synthesized
                     number that was not measured breaks the sweep. Both
                     numbers come straight from the two reports.
      Worker         identity only. Its single consumer, check_ownership,
                     emits exclusively warn-only flags -- the trailer is
                     BLOCKING when absent but its content never blocks --
                     so "<squad>-<slot>" is honest and sufficient. Shaped
                     so validate_fix_commit.squad_from_worker (strip a
                     trailing "-<digits>") recovers the right squad.
      Perl-Ref       supplied by the caller from find_perl_module_for_pairs,
                     which proves it rather than guessing. None here means
                     the diff adds no map values, and Perl-Ref is then not
                     required at all (CONDITIONAL_TRAILERS).

    Returns (trailers, problems, detail). A non-empty `problems` list
    means the block is NOT derivable and the caller must leave the commit
    quarantined and say so -- exactly the brief's "if a required trailer
    needs information only the original worker had".
    """
    problems = []
    before = gap_count_of(pre)
    after = gap_count_of(post)
    pre_index = gap_index(pre)
    post_index = gap_index(post)
    closed = [tag for tag in pre_index if tag not in post_index]
    detail = {"gaps_before": before, "gaps_after": after, "closed_tags": closed}

    if not format_name:
        problems.append("format-unknown")
    if after > before:
        # A regression, measured. Never dress this up in a "recheck-pass"
        # trailer -- that string is what overlord_sweep's post-merge gate
        # trusts.
        problems.append(f"gap-count-regressed:{before}->{after}")
    if not closed:
        # The blind spot the brief names, inverted: a green recheck never
        # validates a constant the sample does not exercise, so a commit
        # that closes NOTHING measurable has no evidence to write down.
        problems.append("no-gap-closed")
    if problems:
        return [], problems, detail

    closed.sort()
    primary = pre_index[closed[0]]
    sample = primary.get("source_file")
    exiftool_value = exiftool_value_of(primary)
    if not sample:
        problems.append("sample-unknown")
    if exiftool_value is None:
        problems.append("exiftool-value-unknown")
    if problems:
        return [], problems, detail

    trailers = [("Format", format_name)]
    trailers += [("Tag", tag) for tag in closed[:max_tags]]
    trailers += [
        ("Sample", str(sample)),
        ("Exiftool-Value", str(exiftool_value)),
        ("Oxidex-Value", str(exiftool_value)),
        ("Verified", f"recheck-pass gaps={before}->{after}"),
        ("Worker", worker),
    ]
    if perl_ref:
        trailers.append(("Perl-Ref", perl_ref))
    return trailers, [], detail


_TRAILER_LINE_RE = re.compile(r"^[A-Za-z][A-Za-z0-9-]*:(?:\s|$)")


def _ends_in_a_trailer(body):
    """True when the message's last non-empty line is itself a trailer,
    i.e. new trailers belong in THAT paragraph rather than a new one."""
    for line in reversed(body.splitlines()):
        if line.strip():
            return bool(_TRAILER_LINE_RE.match(line))
    return False


def message_with_trailers(message, trailers, *, note=None, drop_keys=()):
    """Append a trailer block to a commit message, skipping any key that
    already carries a non-empty value.

    The measured shape of the bookkeeping class makes the simple append
    correct: all 42 zero-trailer commits have a bare subject line and no
    trailer block at all. The skip-if-present guard is there so a
    PARTIALLY trailered commit is never given a second, contradicting
    value for a key -- `git interpret-trailers --parse` returns EVERY
    occurrence, and validate_fix_commit reads Perl-Ref with
    `next(iter(...))`, i.e. the FIRST one, so a second appended value
    would be silently ignored.

    drop_keys is that same fact used the other way round: to REPLACE a
    trailer rather than add one, its existing lines have to be removed
    first. That is the only way to correct an unresolvable Perl-Ref,
    which is what the printconv-unverifiable class needs.
    """
    body = (message or "").rstrip("\n")
    drop_keys = set(drop_keys)
    if drop_keys:
        body = "\n".join(
            line for line in body.splitlines()
            if line.partition(":")[0].strip() not in drop_keys or not line.partition(":")[1]
        ).rstrip("\n")
    present = set()
    for line in body.splitlines():
        key, sep, value = line.partition(":")
        if sep and value.strip():
            present.add(key.strip())
    lines = [f"{key}: {value}" for key, value in trailers if key not in present]
    if note:
        # Provenance, so a human reading `git log` months from now can
        # see that this evidence block was re-derived by a daemon rather
        # than attested by the worker that wrote the code. Not a
        # REQUIRED_TRAILERS key, so it is inert to every gate.
        lines.append(f"Judgment-Queue: {note}")
    if not lines:
        return body + "\n"
    # Separator, and it is load-bearing: `git interpret-trailers --parse`
    # reads only the LAST paragraph. Adding a blank line after a message
    # that already ENDS in trailers would start a second paragraph and
    # silently orphan every trailer above it -- which is exactly what
    # happens on the printconv-unverifiable path, where drop_keys removes
    # a Perl-Ref line from the middle of an otherwise complete block.
    separator = "\n" if _ends_in_a_trailer(body) else "\n\n"
    return body + separator + "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# Novelty (the structural double-promotion guard)
# ---------------------------------------------------------------------------

def already_present(repo_root, sha, refs):
    """The first ref in `refs` that already contains `sha`'s PATCH-ID, or
    None.

    Patch-id, not sha, on purpose: a promoted commit is a fresh
    cherry-pick with a fresh sha, and both merger gates plus the
    quarantine ledger are patch-id keyed. This is what makes promotion
    idempotent STRUCTURALLY -- a crash between the branch fast-forward
    and the ledger append cannot double-promote, because the next poll
    finds the patch already on the slot branch and stops.
    """
    for ref in refs:
        if not ref or not ref_exists(repo_root, ref):
            continue
        if not squad_merge_loop.is_patch_novel_against(repo_root, ref, sha):
            return ref
    return None


# ---------------------------------------------------------------------------
# Adjudication
# ---------------------------------------------------------------------------

def adjudicate(*, repo_root, home, entry, squad, klass, worktree_path, slot=DEFAULT_SLOT,
               perl_lib=None, cache_dir=None, config_path=DEFAULT_CONFIG_PATH,
               apply=False, verify_fn=None, validate_fn=None, recheck_fn=None,
               resolve_pm_fn=None, now_fn=time.time, log_fn=print):
    """Decide, and if the decision is "promote", carry it out.

    One commit in, one Decision out, always. Returns the decision dict;
    appending it to the ledger is the caller's job (so a caller that
    wants to batch, or a dry run that must not write, keeps control).

    The order below is deliberate and each step is a gate the next one
    depends on:

      0  preserve the object (before anything can gc it)
      1  is the patch already downstream? -> already-landed, terminal
      2  is the squad's merger currently blocked? -> defer, no work wasted
      3  re-apply onto CURRENT origin/main (this alone resolves the
         cherry-pick-conflict class -- "many are simply stale")
      4  verify_enum_maps on the diff -> the anti-fabrication gate
      5  bookkeeping class only: measure, re-derive trailers, amend
      6  validate_fix_commit on the rewritten commit -> pre-flight
      7  publish onto the reserved slot branch
    """
    verify_fn = verify_fn or verify_enum_maps.verify
    validate_fn = validate_fn or validate_fix_commit.validate_commit
    recheck_fn = recheck_fn or squad_merge_loop.real_format_match
    resolve_pm_fn = resolve_pm_fn or validate_fix_commit.resolve_perl_module

    patch_id = entry.get("patch_id")
    sha = entry.get("sha")
    fmt = entry.get("format")
    attempt = entry.get("attempt", 0)

    def decide(verdict, reason, **kwargs):
        detail = kwargs.pop("detail", None) or {}
        return make_decision(
            patch_id=patch_id, sha=sha, format_name=fmt, squad=squad, klass=klass,
            verdict=verdict, reason=reason, detail=detail, attempt=attempt,
            dry_run=not apply, now_fn=now_fn, **kwargs,
        )

    # --- classes this daemon deliberately does not touch ------------------
    if klass == "test-failed":
        # The brief is explicit: that is a real failure. A targeted
        # `cargo test --lib <fmt>` failing is the fix being wrong, not
        # the paperwork.
        return decide("queued", "targeted-test-failed is a real failure -- out of scope for re-admission")
    if klass == "semantic":
        # duplicate-emission / new-oxidex-only / sweep-bisection /
        # ff-refused were all reached by MEASURING the commit's effect,
        # not by reading its trailers. Re-offering a bisection-isolated
        # commit would hand the sweep back the thing it proved broke it.
        return decide("queued", "rejected on measured corpus effect, not paperwork -- not re-adjudicable here")

    # --- 0: preserve -------------------------------------------------------
    preservation = preserve_commit(repo_root, patch_id, sha, apply=apply)
    if preservation == "missing":
        return decide("queued", "commit object is gone from this repo -- nothing left to re-admit",
                      detail={"preservation": preservation})
    if preservation == "failed":
        return decide("error", "could not create the preservation ref",
                      detail={"preservation": preservation})

    # --- 1: already downstream? -------------------------------------------
    branch = slot_branch_name(squad, slot)
    landed_on = already_present(repo_root, sha, [ORIGIN_MAIN, f"squad/{squad}", branch])
    if landed_on:
        return decide("already-landed", f"patch-id already contained in {landed_on}",
                      detail={"ref": landed_on})

    # --- 2: is the squad's merger even accepting work? ---------------------
    batch_state = squad_merge_loop.load_batch_state(squad_merge_loop.batch_state_path(home, squad))
    if batch_state.get("blocked"):
        # poll_once halts ALL candidate processing for a blocked squad,
        # so a promotion into it gets zero uptake and no error. Defer
        # rather than burn two cargo builds on a commit nobody will look
        # at.
        return decide("queued", f"squad {squad!r} publication is blocked by a failed batch check",
                      detail={"blocked": True})

    # --- 3: re-apply onto current origin/main -----------------------------
    diff_text = _git(["show", "--format=", sha], repo_root).stdout
    message = _git(["show", "-s", "--format=%B", sha], repo_root).stdout

    def run_git_for_trailers(args, repo, input_text=None):
        result = _git(args, repo, check=False, input_text=input_text)
        return result.returncode, result.stdout, result.stderr

    trailers = validate_fix_commit.parse_trailers(message, repo_root, run_git_for_trailers)
    hints = bare_tag_names(trailers)

    # --- 4: the anti-fabrication gate -------------------------------------
    if not perl_lib:
        # No ExifTool checkout, no pair verification, no promotion. The
        # brief's hard rule ("NEVER promote anything verify_enum_maps did
        # not return clean for") has no safe reading in which an
        # unverifiable commit ships.
        return decide("queued", "no --perl-lib available -- the pair verifier cannot adjudicate")

    perl_ref = next((v for v in trailers.get("Perl-Ref", []) if v), "")
    cited_pm = resolve_pm_fn(perl_ref, Path(perl_lib)) if perl_ref else None
    derived_perl_ref = None

    # The values the VALIDATOR itself treats as real PrintConv display
    # strings. extract_added_map_values excludes tag keys, byte-string
    # identifiers, format! templates and test code -- shapes that read
    # like a lookup table but are not one. Measured live 2026-07-27: DNG
    # 786ea09b is a tag-id -> tag-KEY map (33421 =>
    # "EXIF:CFARepeatPatternDim"), and binding it to a PrintConv table
    # produced a confident "fabricated" verdict on a commit containing no
    # PrintConv at all. "rejected-permanent" is terminal, so it must
    # never rest on that.
    printconv_values = set(validate_fix_commit.extract_added_map_values(diff_text)[0])

    if cited_pm is not None:
        # The commit cites its own module. That citation is the author's
        # attestation and IS authoritative enough to convict on -- it is
        # the same binding check_printconv byte-checks against.
        pm_path = cited_pm
        verdict, hint_used = run_verifier(diff_text, pm_path, hints, verify_fn=verify_fn)
    else:
        # Either the trailer is absent (the whole bookkeeping class) or
        # it does not resolve (the printconv-unverifiable class). PROVE a
        # module rather than guess one -- and accept only a clean proof,
        # never a derived conviction.
        pm_path, why, verdict, hint_used = find_perl_module_for_pairs(
            diff_text, Path(perl_lib), hints, verify_fn=verify_fn,
        )
        if why == "not-required":
            # No map-like values in the diff at all: nothing for the pair
            # verifier to adjudicate and nothing for Perl-Ref to attest.
            # Recorded explicitly rather than treated as a silent pass.
            verdict = verify_enum_maps.Verdict("clean", [], [], [], reason="no-enum-pairs-in-diff",
                                               pairs_checked=0)
            hint_used = None
        elif pm_path is None:
            return decide("queued", f"cannot pin an ExifTool module to verify against ({why})",
                          detail={"perl_ref": perl_ref, "why": why})
        else:
            derived_perl_ref = pm_path.name

    verifier_detail = {
        "status": verdict.status,
        "pairs_checked": verdict.pairs_checked,
        "reason": verdict.reason,
        "table": verdict.table,
        "hint": hint_used,
        "pm": str(pm_path) if pm_path else None,
    }
    if verdict.status == "fabricated":
        # Two INDEPENDENT kinds of evidence, kept apart on purpose. A
        # pair mismatch only means anything if the table binding behind
        # it is trustworthy; a string-valued catch-all means something
        # regardless of the binding. Merging them convicted CR2 629f23a4
        # and 92e457f6 on their catch-alls while the log headlined pair
        # mismatches that came from a misbinding.
        #
        # Both kinds are additionally filtered through the VALIDATOR's
        # own notion of a display value: extract_added_map_values already
        # excludes tag keys, identifiers, format! templates and test
        # code. Without that, DNG 786ea09b -- a tag-id -> tag-KEY map --
        # convicts permanently on a table that was never appropriate.
        block_count = diff_block_count(diff_text)
        trustworthy, why_not = pair_evidence_is_trustworthy(verdict, hint_used, block_count)
        verifier_detail["agreements"] = agreements(verdict)
        verifier_detail["blocks"] = block_count

        convicting = [m for m in verdict.mismatches if m.rust_says in printconv_values]
        convicting_catch_alls = [c for c in verdict.catch_all_arms if c.value in printconv_values]
        if not trustworthy:
            verifier_detail["binding_refused"] = why_not
            verifier_detail["unreliable_mismatches"] = [
                {"key": m.key, "oxidex": m.rust_says, "exiftool": m.exiftool_says}
                for m in verdict.mismatches
            ]
            convicting = []

        if not convicting and not convicting_catch_alls:
            if not trustworthy:
                # Last guard on the terminal verdict, and the one live
                # data forced -- see pair_evidence_is_trustworthy for the
                # seven misbindings an earlier rule made permanent.
                reason = {
                    "zero-agreement":
                        f"every one of {verdict.pairs_checked} pairs disagrees with "
                        f"{verdict.table!r}, which reads as a wrong table binding rather than "
                        f"{len(verdict.mismatches)} independent fabrications",
                    "forced-hint-on-multi-table-diff":
                        f"the verdict rests on the forced hint {hint_used!r}, which binds all "
                        f"{block_count} match tables in this diff to {verdict.table!r} -- the "
                        "agreements come from the block that fits and the mismatches from the "
                        "ones that do not",
                }[why_not]
            else:
                reason = ("verifier flagged pairs the validator does not treat as PrintConv "
                          "display values (tag-key map, identifier or template)")
            return decide("queued", f"{reason}; not a safe basis for a permanent rejection",
                          detail={"verifier": verifier_detail})

        # PERMANENT, and deliberately immune to a policy bump. Whatever
        # evidence survived above is recorded in full so a human never
        # has to re-derive it, and so the same patch-id is never
        # re-adjudicated in a loop.
        if trustworthy:
            verifier_detail["mismatches"] = [
                {"key": m.key, "oxidex": m.rust_says, "exiftool": m.exiftool_says,
                 "exiftool_key_for_value": m.exiftool_key_for_value,
                 "file": m.file, "line": m.line}
                for m in verdict.mismatches
            ]
        verifier_detail["catch_alls"] = [
            {"value": c.value, "file": c.file, "line": c.line} for c in verdict.catch_all_arms
        ]
        summary = ", ".join(
            f"{m.key}={m.rust_says!r} but ExifTool says {m.exiftool_says!r}"
            + (f" ({m.rust_says!r} lives at {m.exiftool_key_for_value})"
               if m.exiftool_key_for_value else "")
            for m in convicting[:4]
        )
        if not convicting:
            # Catch-all evidence ALONE. The detection is sound -- ExifTool
            # prints the raw number for an unlisted key, so a string `_ =>`
            # arm really does replace data it would have shown -- but the
            # SEVERITY was wrong, and severity is the whole point of a
            # terminal verdict.
            #
            # Measured 2026-07-27, adjudicating 8 archived patches beside a
            # human pass over the same 8:
            #   elf-4b5a26e97cb8  pairs_checked=17, agreements=17,
            #                     mismatches=0 -- a PERFECT pair record
            #   pdf-a1a411f67e3f  measures a real -4 gap closure
            # Both were rejected-permanent on nothing but a `_ =>` arm, and
            # rejected-permanent is deliberately immune to a policy bump, so
            # both would have been discarded FOREVER over a one-line
            # divergence in a code path their own sample never exercises.
            #
            # A catch-all is a CORRECTABLE defect: the fix is to return None
            # (or the raw value) and it is mechanical. Discarding a commit
            # that also closes real gaps because of one is disproportionate.
            # A pair mismatch on a TRUSTWORTHY binding stays terminal -- that
            # is a fabricated fact, not a fixable slip.
            return decide(
                "queued",
                "string-valued catch-all replaces data ExifTool would print "
                f"({', '.join(repr(c.value) for c in convicting_catch_alls[:3])}); "
                "correctable -- return None or the raw value. Not terminal on its own.",
                detail={"verifier": verifier_detail},
            )
        return decide("rejected-permanent", f"fabricated enum pairs: {summary}",
                      detail={"verifier": verifier_detail})
    if verdict.status != "clean":
        return decide("queued", f"verify_enum_maps could not adjudicate ({verdict.reason})",
                      detail={"verifier": verifier_detail})

    # From here on the decision is "promote if the mechanics work". Every
    # remaining exit is a mechanical failure, never a judgment call.
    if not apply:
        return decide("queued",
                      "dry run: cleared the pair verifier; would re-apply onto "
                      f"{ORIGIN_MAIN} and promote onto {branch} if the cherry-pick, the "
                      "trailer derivation and the validator all succeed",
                      detail={"verifier": verifier_detail, "would_promote": True,
                              "branch": branch, "derived_perl_ref": derived_perl_ref})

    ensure_judgment_worktree(repo_root, worktree_path, apply=apply, log_fn=log_fn)
    base = rev_parse(repo_root, branch) or rev_parse(repo_root, ORIGIN_MAIN)
    if base is None:
        return decide("error", f"neither {branch} nor {ORIGIN_MAIN} resolves in {repo_root}")
    squad_merge_loop.checkout_detached(worktree_path, base)

    pre = None
    if klass == "bookkeeping":
        # Measured BEFORE the cherry-pick, on the exact tree the commit
        # is about to be applied to -- the "gaps=<before>" half of the
        # Verified trailer has to be the real starting point, not a
        # figure lifted from some other run.
        pre = recheck_fn(worktree_path, cache_dir, fmt, "judgment-queue")

    ok, cherry_message = squad_merge_loop.cherry_pick(worktree_path, sha)
    if not ok:
        # For the cherry-pick-conflict class this IS the answer: it was
        # not merely stale. Queued rather than permanent -- origin/main
        # moves, and the same commit may apply cleanly next week.
        return decide("queued", f"still conflicts against current {ORIGIN_MAIN}",
                      detail={"verifier": verifier_detail, "cherry_pick": cherry_message[:2000]})

    # --- 5: rewrite the evidence block ------------------------------------
    derivation = None
    amend_trailers = []
    drop_keys = []
    note = None
    if klass == "bookkeeping":
        post = recheck_fn(worktree_path, cache_dir, fmt, "judgment-queue")
        amend_trailers, problems, derivation = derive_trailers(
            pre=pre, post=post, format_name=fmt,
            worker=f"{squad}-{slot}",
            perl_ref=derived_perl_ref or perl_ref or None,
        )
        if problems:
            return decide("queued",
                          "required trailers are not derivable from the diff plus a comparison "
                          f"run ({', '.join(problems)}) -- leaving quarantined",
                          detail={"verifier": verifier_detail, "derivation": derivation,
                                  "problems": problems})
        drop_keys = ["Perl-Ref"] if derived_perl_ref else []
        note = f"trailers re-derived from a measured recheck (quarantined patch-id {patch_id})"
    elif derived_perl_ref:
        # The printconv-unverifiable class, and the reason it would
        # otherwise be a permanent dead end. check_printconv flags
        # "unverifiable" precisely because the cited Perl-Ref does not
        # resolve -- so proving a module and then NOT writing it down
        # means step 6 re-flags the commit forever. The trailer is
        # REPLACED, not appended: validate_fix_commit reads Perl-Ref with
        # next(iter(...)), i.e. the first occurrence, so a second value
        # would be ignored. This only ever writes a module the pair
        # verifier came back clean against -- it is recording a proof,
        # not making a guess.
        amend_trailers = [("Perl-Ref", derived_perl_ref)]
        drop_keys = ["Perl-Ref"]
        note = (f"Perl-Ref proved by pair verification against {derived_perl_ref} "
                f"(quarantined patch-id {patch_id})")

    if amend_trailers:
        amended = message_with_trailers(message, amend_trailers, note=note, drop_keys=drop_keys)
        # `--file -` reads the message from stdin, so no editor is ever
        # spawned. `--no-verify` because this repo's .githooks are
        # delegation shims (git-lfs, chroma indexing) that a headless
        # daemon must not depend on -- and an amend that changes only the
        # message cannot change anything a content hook would check.
        _git(["commit", "--amend", "--no-verify", "--file", "-"], worktree_path,
             input_text=amended)

    new_sha = squad_merge_loop.head_sha(worktree_path)

    # --- 6: pre-flight against the real validator -------------------------
    validation = validate_fn(new_sha, repo_root, perl_lib=perl_lib, config_path=str(config_path))
    if not validation.get("ok", False):
        return decide("queued",
                      f"still flagged after re-admission work: {', '.join(validation.get('flags') or [])}",
                      detail={"verifier": verifier_detail, "derivation": derivation,
                              "flags": validation.get("flags") or []})

    # --- 7: publish -------------------------------------------------------
    published, publish_message = create_or_fast_forward(repo_root, branch, new_sha, apply=apply)
    if not published:
        return decide("error", f"could not publish onto {branch}: {publish_message}",
                      detail={"verifier": verifier_detail, "derivation": derivation})
    log_fn(f"judgment-queue: {patch_id[:12]} ({fmt}/{squad}) {sha[:12]} -> {new_sha[:12]} "
           f"PROMOTED onto {branch}")
    return decide("promoted", f"re-admitted onto {branch} for the {squad} merger to validate",
                  detail={"verifier": verifier_detail, "derivation": derivation},
                  promoted_branch=branch, promoted_sha=new_sha)


# ---------------------------------------------------------------------------
# Poll cycle
# ---------------------------------------------------------------------------

def squad_of(entry, *, squads=()):
    """The squad that owns a quarantine entry.

    The ledger's own "squad" field is authoritative -- the merger that
    wrote the entry knew which squad it was. It can be absent only on a
    hand-edited line; overlord_sweep's bisection writer passes
    format=None but always names the squad.
    """
    squad = entry.get("squad")
    if squad and (not squads or squad in squads):
        return squad
    return None


def poll_once(*, repo_root=REPO_ROOT, home=OXIDEX_HOME, slot=DEFAULT_SLOT, perl_lib=None,
              cache_dir=None, config_path=DEFAULT_CONFIG_PATH, apply=False, limit=None,
              verify_fn=None, validate_fn=None, recheck_fn=None, resolve_pm_fn=None,
              adjudicate_fn=None, now_fn=time.time, log_fn=print, heartbeat_fn=None):
    """One full pass over the quarantine ledger.

    Returns {"considered", "adjudicated", "skipped", "decisions": [...]}.
    `decisions` holds the full record for everything decided this poll,
    dry run or not, so a caller (a test, an operator running --once) can
    read the reasoning without parsing the ledger back.

    heartbeat_fn, if given, is called after each decision -- a
    bookkeeping promotion pays for two full cargo-backed comparison runs,
    so the singleton lock must be refreshed mid-poll rather than only at
    acquire time (the same reason squad_merge_loop.poll_once takes one).
    """
    adjudicate_fn = adjudicate_fn or adjudicate
    heartbeat_fn = heartbeat_fn or (lambda: None)
    repo_root = Path(repo_root)
    home = Path(home)
    ledger = squad_merge_loop.quarantine_ledger_path(home)
    entries = squad_merge_loop.load_quarantine(ledger)
    decisions_path = decision_ledger_path(home)
    prior = load_decisions(decisions_path)
    rules = ruleset_id()

    considered = 0
    skipped = 0
    out = []
    # Oldest first, so a poll interrupted by --limit always makes
    # progress on the backlog's tail rather than re-chewing whatever the
    # newest quarantine happens to be.
    ordered = sorted(entries.items(), key=lambda kv: (kv[1].get("ts_epoch") or 0, kv[1].get("ts") or ""))
    for patch_id, entry in ordered:
        considered += 1
        if not needs_adjudication(entry, prior.get(patch_id), rules):
            skipped += 1
            continue
        squad = squad_of(entry)
        klass = classify_flags(entry.get("flags"))
        if squad is None:
            decision = make_decision(
                patch_id=patch_id, sha=entry.get("sha"), format_name=entry.get("format"),
                squad=None, klass=klass, verdict="queued",
                reason="ledger entry names no squad -- no merger to promote it to",
                attempt=entry.get("attempt", 0), dry_run=not apply, now_fn=now_fn,
            )
        else:
            try:
                decision = adjudicate_fn(
                    repo_root=repo_root, home=home, entry=entry, squad=squad, klass=klass,
                    worktree_path=judgment_worktree_dir(home, squad), slot=slot,
                    perl_lib=perl_lib, cache_dir=cache_dir, config_path=config_path,
                    apply=apply, verify_fn=verify_fn, validate_fn=validate_fn,
                    recheck_fn=recheck_fn, resolve_pm_fn=resolve_pm_fn,
                    now_fn=now_fn, log_fn=log_fn,
                )
            except Exception as e:  # noqa: BLE001 -- one bad commit must not stall a backlog
                # _run_poll_safely catches this one level up too, but that
                # is the WRONG granularity here: a raise on entry 3 of 45
                # would abandon the other 42 every poll, forever, and the
                # backlog this daemon exists to drain would never move.
                # An "error" verdict is non-terminal, so the entry is
                # retried next poll -- and it still gets a ledger line,
                # because a silent skip is how the quarantine tier became
                # invisible in the first place.
                decision = make_decision(
                    patch_id=patch_id, sha=entry.get("sha"), format_name=entry.get("format"),
                    squad=squad, klass=klass, verdict="error",
                    reason=f"adjudication raised {e!r}", attempt=entry.get("attempt", 0),
                    dry_run=not apply, now_fn=now_fn,
                )
        append_decision(decisions_path, decision, apply=apply)
        out.append(decision)
        log_fn(json.dumps(decision, separators=(",", ":"), default=str))
        heartbeat_fn()
        if limit is not None and len(out) >= limit:
            break
    return {"considered": considered, "adjudicated": len(out), "skipped": skipped,
            "decisions": out}


def _run_poll_safely(poll_fn=poll_once, poll_kwargs=None, log_fn=print):
    """poll_once, wrapped so one bad poll NEVER sinks the daemon -- the
    same discipline as parallel_model_fix_loop._run_auto_publish_safely,
    and for the same reason: an --infinite process lives for weeks and a
    transient git error, an unreadable ExifTool module or a cargo build
    killed by an operator's pkill must cost one poll, not the loop.

    Everything durable is already durable by the time a raise can happen:
    preservation refs and slot-branch fast-forwards are atomic git ref
    updates, and the decision ledger is a single append per decision. The
    return value is NOT swallowed -- a one-shot --once run needs
    something to read.
    """
    try:
        return poll_fn(**(poll_kwargs or {}))
    except Exception as e:  # noqa: BLE001 -- deliberately broad, see docstring
        log_fn(f"judgment-queue: poll raised {e!r} -- continuing the loop. Anything already "
               "promoted this poll STAYS promoted (a fast-forwarded slot branch is a durable "
               "ref update), and any decision already appended will not be re-adjudicated.")
        return {"considered": 0, "adjudicated": 0, "skipped": 0, "decisions": [],
                "status": "raised", "error": repr(e)}


# ---------------------------------------------------------------------------
# Singleton lock
# ---------------------------------------------------------------------------

def run_locked(home, fn, *, now_fn=time.time, kill_fn=None, script_sha=None, pid=None):
    """Run `fn(heartbeat)` under this daemon's singleton lock.

    The structural guards make a double-promotion impossible, but they do
    NOT make two concurrent daemons safe: both would drive the same
    per-squad worktree, and the second one's `checkout --detach` would
    land in the middle of the first one's cherry-pick. One process at a
    time, with the same stale-heartbeat / script-sha takeover discipline
    every other fleet daemon uses (distill_lessons' helpers, imported
    rather than re-implemented).

    Returns {"status": "already_running"} without calling fn() when a
    fresh same-sha holder exists; otherwise {"status": "ok", "result":
    fn(heartbeat)}.
    """
    kill_fn = kill_fn or os.kill
    lock_path = daemon_lock_path(home)
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    script_sha = script_sha or compute_script_sha()
    pid = os.getpid() if pid is None else pid
    if not acquire_lock(lock_path, pid, script_sha, now_fn, kill_fn, STALE_HEARTBEAT_SECONDS):
        return {"status": "already_running"}

    def heartbeat():
        write_lock(lock_path, pid, script_sha, now_fn())

    try:
        return {"status": "ok", "result": fn(heartbeat)}
    finally:
        release_lock(lock_path, pid)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def default_perl_lib():
    """The ExifTool Perl lib, resolved the way the rest of the fleet
    does. Imported lazily -- hermetic tests always pass --perl-lib and
    must never touch model_fix_loop or the real exiftool."""
    import attribute_gaps
    return attribute_gaps.default_perl_lib()


def main(argv=None, sleep_fn=time.sleep, now_fn=time.time, poll_fn=poll_once):
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--repo", default=str(REPO_ROOT))
    parser.add_argument("--home", default=str(OXIDEX_HOME))
    parser.add_argument("--config", default=str(DEFAULT_CONFIG_PATH),
                        help="config.toml, for its [squads.*] tables (see config.example.toml)")
    parser.add_argument("--perl-lib", default=None,
                        help="Image/ExifTool Perl lib root (default: resolved from the "
                             "exiftool on PATH, as the rest of the fleet does)")
    parser.add_argument("--cache-dir",
                        default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))  # nosec B108
    parser.add_argument("--slot", type=int, default=DEFAULT_SLOT,
                        help="reserved squad slot to publish re-admissions on "
                             f"(default {DEFAULT_SLOT}; must stay outside allocate_squad_slots' range)")
    parser.add_argument("--limit", type=int, default=None,
                        help="stop after this many adjudications in one poll")
    parser.add_argument("--poll-seconds", type=float, default=DEFAULT_POLL_SECONDS)
    parser.add_argument("--once", action="store_true", help="single pass then exit (default)")
    parser.add_argument("--infinite", action="store_true", help="poll forever until interrupted")
    parser.add_argument("--apply", action="store_true",
                        help="actually mutate: create preservation refs, re-apply commits, "
                             "publish slot branches and append to the decision ledger. "
                             "WITHOUT THIS THE DAEMON IS A COMPLETE BUT READ-ONLY DRY RUN.")
    parser.add_argument("--dry-run", action="store_true",
                        help="explicit no-op form of the default; overrides --apply if both given")
    args = parser.parse_args(argv)

    # Dry run is the DEFAULT and --dry-run WINS over --apply. This daemon
    # rewrites commit messages and mints branches a live 32-worker fleet
    # will consume; the safe mode must be the one you get by forgetting a
    # flag, and the explicit "don't touch anything" must be unoverridable
    # by an --apply left over in a shell-history line.
    apply = bool(args.apply) and not args.dry_run
    sys.stdout.reconfigure(line_buffering=True)

    perl_lib = args.perl_lib
    if perl_lib is None:
        try:
            perl_lib = str(default_perl_lib())
        except Exception as e:  # noqa: BLE001
            print(f"judgment-queue: could not resolve --perl-lib automatically ({e!r}) -- "
                  "the pair verifier will abstain on every commit until one is supplied")

    if not apply:
        print("judgment-queue: DRY RUN (pass --apply to mutate). No refs, branches, worktrees "
              "or ledger lines will be written.")

    kwargs = {
        "repo_root": Path(args.repo), "home": Path(args.home), "slot": args.slot,
        "perl_lib": perl_lib, "cache_dir": args.cache_dir, "config_path": args.config,
        "apply": apply, "limit": args.limit, "now_fn": now_fn,
    }
    while True:
        if apply:
            # The lock is taken ONLY in --apply mode, deliberately. A dry
            # run mutates nothing, so making an operator's read-only look
            # fail while the daemon is mid-poll would be pure friction.
            outcome = run_locked(
                Path(args.home),
                lambda heartbeat: _run_poll_safely(
                    poll_fn=poll_fn, poll_kwargs={**kwargs, "heartbeat_fn": heartbeat}),
                now_fn=now_fn,
            )
            if outcome["status"] == "already_running":
                print("judgment-queue: another daemon already holds the lock -- exiting quietly")
                return 0
            result = outcome["result"]
        else:
            result = _run_poll_safely(poll_fn=poll_fn, poll_kwargs=kwargs)
        print(f"judgment-queue: considered={result['considered']} "
              f"adjudicated={result['adjudicated']} skipped={result['skipped']}"
              + (f" status={result['status']}" if result.get("status") else ""))
        if not args.infinite:
            return 0
        sleep_fn(args.poll_seconds)


if __name__ == "__main__":
    sys.exit(main())
