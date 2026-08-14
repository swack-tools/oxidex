"""The shared "which oxidex, from what commit, against what corpus" header.

``exiftool_oracle.py`` already answers half of "name the instrument" -- which
ExifTool a run graded against, and whether it is even capable of grading
correctly. This module answers the other half: which OxiDex, built from what
source state, run over what corpus. Every measurement script in
``tools/exiftool-tables/`` (and the Rust ``jpeg-tag-matrix`` binary, which
mirrors this by hand since it cannot import Python) prints this header before
its first number, so a reader can attribute the number to a binary and a
commit instead of trusting a bare score.

This module exists because a session produced five wrong numbers in one
afternoon, each from a proxy standing in for the thing being measured:

1. A harness resolved oxidex from ``$repo/target/release/oxidex`` by
   convention while ``CARGO_TARGET_DIR`` pointed elsewhere. The binary did not
   exist; every comparison against it failed closed, and the run reported a
   confident, precisely-formatted 0% for nine consecutive gate runs before
   anyone looked at *which* binary ran. See :func:`resolve_binary`, which
   fails loudly the moment a resolved path does not exist, instead of letting
   every downstream subprocess call to a missing binary look like a parse
   failure.
2. A duplicate-loss scan graded a conveniently-already-built binary that
   turned out to be from an old commit, with the working tree already dirty
   again on top of that. Nothing about the run said so. See
   :func:`git_state` and :func:`staleness_note`, which compare the binary's
   mtime against the source it should have been built from and say so in the
   header when they disagree.
3. A dirty or otherwise ambiguous tree measures nothing attributable to any
   commit. See :func:`refuse_if_dirty`, which exits unless explicitly
   overridden, and notes the override in the header when one is used.

Keep this module dependency-light (stdlib only) -- it is imported by every
harness before argument parsing even happens, so a heavy or fragile import
here would take every one of them down with it.
"""

from __future__ import annotations

import datetime
import os
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
from dataclasses import dataclass, field
from pathlib import Path

DIRTY_OVERRIDE_ENV = "OXIDEX_ALLOW_DIRTY_TREE"

REPO_ROOT = Path(__file__).resolve().parent.parent


def _git(repo_root: Path, *args: str) -> str | None:
    """Run a git subcommand, returning stdout with only the trailing newline
    removed, or None on any failure.

    Trailing-only, not ``.strip()``: `git status --porcelain` prefixes every
    line with a fixed-width status column that starts with a literal space
    for an unstaged modification (`" M path"`). A full `.strip()` here eats
    that leading space off the FIRST line only (later lines are untouched,
    since `.strip()` only trims the ends of the whole string) and every
    caller that slices a fixed prefix off each line -- see
    :func:`git_state` -- then truncates that one file's name by a
    character. Caught by actually reading this module's own output rather
    than trusting the slice logic looked right.

    None (not an exception) on failure: a tool that cannot find `git`, or is
    not run from inside a checkout at all, should still be able to print a
    degraded header rather than crash before its first number.
    """
    try:
        out = subprocess.run(  # nosec B603 -- list-argv, no shell
            ["git", "-C", str(repo_root), *args],
            capture_output=True, text=True, errors="replace",
        )
    except OSError:
        return None
    if out.returncode != 0:
        return None
    return out.stdout.rstrip("\n")


@dataclass
class GitState:
    """The identity of the source tree a measurement is attributable to."""

    repo_root: Path
    commit: str | None
    describe: str | None
    dirty: bool
    dirty_files: list[str] = field(default_factory=list)
    head_time: float | None = None  # unix timestamp of HEAD's commit

    def short(self) -> str:
        commit = (self.commit or "unknown")[:12]
        describe = self.describe or commit
        state = f"DIRTY ({len(self.dirty_files)} file{'s' if len(self.dirty_files) != 1 else ''})" if self.dirty else "clean"
        return f"{describe} ({commit}, {state})"


def git_state(repo_root: Path | str | None = None) -> GitState:
    """The current identity of ``repo_root`` (default: this checkout)."""
    root = Path(repo_root) if repo_root else REPO_ROOT
    commit = _git(root, "rev-parse", "HEAD")
    describe = _git(root, "describe", "--always", "--tags", "--long", "--dirty")
    status = _git(root, "status", "--porcelain")
    dirty_files = [line[3:] for line in status.splitlines()] if status else []
    head_time_s = _git(root, "log", "-1", "--format=%ct")
    head_time = float(head_time_s) if head_time_s and head_time_s.lstrip("-").isdigit() else None
    return GitState(
        repo_root=root,
        commit=commit,
        describe=describe,
        dirty=bool(dirty_files),
        dirty_files=dirty_files,
        head_time=head_time,
    )


def refuse_if_dirty(git: GitState, tool: str) -> bool:
    """Refuse (exit) to measure against a dirty tree unless overridden.

    A dirty tree is ambiguous by construction: the binary under test may or
    may not reflect the uncommitted changes, and there is no commit a reader
    can check out later to reproduce the number. Returns True when the
    caller overrode the refusal (via $OXIDEX_ALLOW_DIRTY_TREE=1), so the
    header can say so; returns False when the tree was already clean.
    """
    if not git.dirty:
        return False
    if os.environ.get(DIRTY_OVERRIDE_ENV, "").strip().lower() in {"1", "true"}:
        return True
    shown = ", ".join(git.dirty_files[:8])
    more = f", +{len(git.dirty_files) - 8} more" if len(git.dirty_files) > 8 else ""
    sys.exit(
        f"❌ {tool}: refusing to measure against a dirty working tree "
        f"({len(git.dirty_files)} modified file(s) in {git.repo_root}): {shown}{more}\n"
        "   A number measured against an uncommitted, unreproducible tree state "
        "cannot be attributed to any commit -- see AGENTS.md 'Name the instrument'.\n"
        f"   Commit or stash first, or set {DIRTY_OVERRIDE_ENV}=1 to measure anyway "
        "(the header will record the override)."
    )


@dataclass
class BinaryIdentity:
    """A resolved, existing executable -- never a path that merely *should*
    exist. See :func:`resolve_binary`."""

    kind: str
    requested: str
    path: Path
    mtime: float
    size: int


def resolve_binary(requested: str, kind: str = "oxidex") -> BinaryIdentity:
    """Resolve ``requested`` to an absolute path, failing LOUDLY if it is not
    there.

    This is the fix for the specific incident this module documents: a
    harness that derives a binary path by convention and proceeds when it is
    absent does not fail -- every subprocess call to a nonexistent program
    fails closed, and the run reports every comparison as a parse failure,
    which is indistinguishable from a real regression until someone thinks to
    check whether the binary was ever there. Resolve explicitly, check
    existence explicitly, and exit with a clear message the moment it is not
    -- before a single tag is compared.
    """
    p = Path(requested)
    if not p.is_file():
        sys.exit(
            f"❌ {kind} binary not found at {p}\n"
            f"   (resolved from {requested!r}). Build it first (`cargo build "
            f"--bin {kind}`), or pass the correct path explicitly.\n"
            "   Refusing to proceed: every comparison against a missing binary "
            "fails closed and looks exactly like a real regression."
        )
    st = p.stat()
    return BinaryIdentity(kind=kind, requested=requested, path=p.resolve(), mtime=st.st_mtime, size=st.st_size)


def staleness_note(binary: BinaryIdentity, git: GitState) -> str | None:
    """A warning if ``binary`` looks older than the source it should reflect.

    Not proof -- mtimes can lie, and a from-scratch build after a `git pull`
    with no local edits legitimately postdates HEAD by however long the build
    took. But it is cheap, and it is exactly the check that would have caught
    a stale prebuilt binary being graded as though it were current: compare
    the binary's mtime against HEAD's commit time and against every dirty
    file's own mtime, and say so if the binary predates the newer of the two.
    """
    newest_source = git.head_time
    for f in git.dirty_files:
        try:
            newest_source = max(newest_source or 0.0, (git.repo_root / f).stat().st_mtime)
        except OSError:
            continue
    if newest_source is None or binary.mtime >= newest_source:
        return None
    bt = datetime.datetime.fromtimestamp(binary.mtime).isoformat(timespec="seconds")
    st = datetime.datetime.fromtimestamp(newest_source).isoformat(timespec="seconds")
    return (
        f"{binary.kind} binary at {binary.path} was built {bt}, which predates "
        f"the newest relevant source change ({st}). It may not reflect "
        f"{'the dirty working tree' if git.dirty else 'HEAD'} -- rebuild before "
        "trusting this run."
    )


def corpus_summary(corpus_paths: list, file_count: int) -> str:
    roots = ", ".join(str(p) for p in corpus_paths)
    return f"{roots} ({file_count} file{'s' if file_count != 1 else ''})"


def print_header(
    *,
    tool: str,
    git: GitState,
    binary: BinaryIdentity | None = None,
    dirty_overridden: bool = False,
    oracle=None,
    corpus_paths=None,
    file_count: int | None = None,
    extra: list[str] | None = None,
) -> None:
    """Print the standard instrument-identity header, before any numbers.

    Every parameter is optional except ``tool``/``git`` because not every
    harness touches an oxidex binary or a file corpus (e.g. reachability.py
    parses committed generated Rust and never shells out at all) -- print
    only what this particular tool's instrument actually consists of, rather
    than padding the header with placeholders for things it does not use.
    """
    print(f"=== instrument: {tool} ===")
    if binary is not None:
        print(f"oxidex:  {binary.path}")
        note = staleness_note(binary, git)
        if note:
            print(f"         ⚠️  {note}")
    tree_line = f"repo:    {git.short()}"
    if dirty_overridden:
        tree_line += "  [OXIDEX_ALLOW_DIRTY_TREE=1: measuring anyway]"
    print(tree_line)
    if git.dirty:
        shown = ", ".join(git.dirty_files[:8])
        more = f", +{len(git.dirty_files) - 8} more" if len(git.dirty_files) > 8 else ""
        print(f"         dirty: {shown}{more}")
    if oracle is not None:
        print(f"oracle:  {oracle.provenance()}")
        print(f"         {oracle.display()}")
    if corpus_paths is not None and file_count is not None:
        print(f"corpus:  {corpus_summary(corpus_paths, file_count)}")
    for line in extra or []:
        print(line)
    print()
