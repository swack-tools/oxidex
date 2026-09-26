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
4. An implicit toolchain resolution. ``rust-toolchain.toml`` pins the
   compiler, but only rustup's proxies read that file: a Mac with Homebrew's
   ``/opt/homebrew/bin`` ahead of ``~/.cargo/bin`` on PATH resolves ``rustc``
   (and ``cargo``) to Homebrew's release and silently builds with it. Even
   rustup's own ``cargo`` then invokes the Homebrew ``rustc`` it finds on
   PATH. Every local binary and corpus measurement on 2026-09-23 was built
   that way (1.98.1) while CI used the pin (1.97.1). See
   :func:`toolchain_report`, which identifies the compiler that built the
   *binary under test* from the ``/rustc/<commit-hash>/`` paths std embeds in
   every Rust executable -- the compiler on PATH today is not necessarily the
   one that built a prebuilt binary -- and warns loudly when it is not the
   pinned one.

Keep this module dependency-light (stdlib only) -- it is imported by every
harness before argument parsing even happens, so a heavy or fragile import
here would take every one of them down with it.
"""

from __future__ import annotations

import datetime
import os
import re
import shutil
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


def _repo_relative_path(root: Path, path: Path | str) -> str:
    candidate = Path(path)
    if not candidate.is_absolute():
        candidate = root / candidate
    try:
        return candidate.resolve().relative_to(root.resolve()).as_posix()
    except ValueError as error:
        raise ValueError(f"allowed dirty path escapes repository: {path}") from error


def refuse_if_dirty(
    git: GitState,
    tool: str,
    *,
    allowed_dirty_paths: list[Path | str] | tuple[Path | str, ...] = (),
) -> bool:
    """Refuse (exit) to measure against a dirty tree unless overridden.

    A dirty tree is ambiguous by construction: the binary under test may or
    may not reflect the uncommitted changes, and there is no commit a reader
    can check out later to reproduce the number. Returns True when the
    caller overrode the refusal (via $OXIDEX_ALLOW_DIRTY_TREE=1), so the
    header can say so; returns False when the tree was already clean.
    """
    if not git.dirty:
        return False
    allowed = {_repo_relative_path(git.repo_root, path) for path in allowed_dirty_paths}
    unexpected = [path for path in git.dirty_files if path not in allowed]
    if allowed and not unexpected:
        return False
    if os.environ.get(DIRTY_OVERRIDE_ENV, "").strip().lower() in {"1", "true"}:
        return True
    refused = unexpected if allowed else git.dirty_files
    shown = ", ".join(refused[:8])
    more = f", +{len(refused) - 8} more" if len(refused) > 8 else ""
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


# --- which compiler -------------------------------------------------------

# std's panic locations embed the toolchain's source path, remapped to
# `/rustc/<full commit hash>/library/...`, in every Rust executable, stripped
# or not. The hash is the one `rustc -vV` prints as `commit-hash:`, so it
# names the toolchain whose std was linked -- that is, the compiler that
# built the binary -- without trusting whatever `rustc` resolves to now.
_EMBEDDED_RUSTC = re.compile(rb"/rustc/([0-9a-f]{40})/")
_CHANNEL_LINE = re.compile(r'^\s*channel\s*=\s*"([^"]+)"\s*(?:#.*)?$', re.MULTILINE)
_FULL_RELEASE = re.compile(r"^\d+\.\d+\.\d+$")
_MINOR_RELEASE = re.compile(r"^\d+\.\d+$")


def rust_toolchain_file(repo_root: Path | str) -> Path | None:
    """The checkout's toolchain pin file (``rust-toolchain.toml``, or the
    legacy extensionless ``rust-toolchain``), or None when it has none."""
    for name in ("rust-toolchain.toml", "rust-toolchain"):
        candidate = Path(repo_root) / name
        if candidate.is_file():
            return candidate
    return None


def pinned_rust_channel(repo_root: Path | str) -> str | None:
    """``[toolchain] channel`` from the checkout's own pin file, or None.

    A legacy ``rust-toolchain`` file may hold just the channel on one line.
    """
    path = rust_toolchain_file(repo_root)
    if path is None:
        return None
    text = path.read_text(encoding="utf-8", errors="replace")
    match = _CHANNEL_LINE.search(text)
    if match:
        return match.group(1).strip()
    if path.name == "rust-toolchain":
        first = text.strip().splitlines()[0].strip() if text.strip() else ""
        return first or None
    return None


def channel_matches(channel: str, release: str | None) -> bool | None:
    """Does a rustc/cargo ``release`` satisfy ``channel``?

    True/False for a numeric pin (``1.97.1`` exactly, or any ``1.97.x`` for a
    ``1.97`` pin); None when the channel is symbolic (``stable``, a nightly
    date) and a release number alone cannot say.
    """
    if not release:
        return False
    if _FULL_RELEASE.match(channel):
        return release == channel
    if _MINOR_RELEASE.match(channel):
        return release.startswith(channel + ".")
    return None


def parse_rustc_verbose(text: str) -> dict[str, str]:
    """``rustc -vV`` -> {"version": first line, "release": ..., "commit-hash": ...}."""
    lines = text.strip().splitlines()
    fields: dict[str, str] = {"version": lines[0].strip() if lines else ""}
    for line in lines[1:]:
        key, sep, value = line.partition(":")
        if sep:
            fields[key.strip()] = value.strip()
    return fields


def cargo_release(text: str) -> str | None:
    """``cargo -V`` (``cargo 1.97.1 (c980f4866 2026-06-30)``) -> ``1.97.1``."""
    parts = text.strip().split()
    return parts[1] if len(parts) >= 2 and parts[0] == "cargo" else None


@dataclass
class RustcIdentity:
    """One resolved rustc: the command asked for, the file it resolved to,
    and what ``-vV`` says about it."""

    command: str
    path: str | None
    version: str
    release: str | None
    commit_hash: str | None
    sysroot: str | None = None

    def describe(self) -> str:
        where = self.path or self.command
        return f"{self.version or 'rustc ?'} at {where}"


def _run_text(argv: list[str], *, cwd: Path | str | None = None, env: dict | None = None,
              timeout: float = 30) -> str | None:
    try:
        out = subprocess.run(  # nosec B603 -- list-argv, no shell
            argv, cwd=cwd, env=env, capture_output=True, text=True, errors="replace", timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return out.stdout if out.returncode == 0 else None


def rustc_identity(rustc: str | None = None, *, cwd: Path | str | None = None,
                   env: dict | None = None) -> RustcIdentity | None:
    """What ``rustc`` means in ``cwd`` under ``env`` -- the resolution cargo
    itself uses (``$RUSTC`` when set, else ``rustc`` on PATH). Run in the
    checkout so a rustup proxy honours its ``rust-toolchain.toml``."""
    environ = os.environ if env is None else env
    command = rustc or environ.get("RUSTC") or "rustc"
    text = _run_text([command, "-vV"], cwd=cwd, env=env)
    if text is None:
        return None
    fields = parse_rustc_verbose(text)
    sysroot = _run_text([command, "--print", "sysroot"], cwd=cwd, env=env)
    return RustcIdentity(
        command=command,
        path=shutil.which(command, path=environ.get("PATH")),
        version=fields.get("version", ""),
        release=fields.get("release"),
        commit_hash=fields.get("commit-hash"),
        sysroot=sysroot.strip() if sysroot else None,
    )


def _rustup_executable(env: dict | None = None) -> str | None:
    environ = os.environ if env is None else env
    found = shutil.which("rustup", path=environ.get("PATH"))
    if found:
        return found
    cargo_home = environ.get("CARGO_HOME") or str(Path(environ.get("HOME", str(Path.home()))) / ".cargo")
    candidate = Path(cargo_home) / "bin" / "rustup"
    return str(candidate) if candidate.is_file() else None


def pinned_rustc_identity(channel: str, *, cwd: Path | str | None = None,
                          env: dict | None = None) -> RustcIdentity | None:
    """The pinned toolchain's own rustc, asked of rustup -- never of PATH.

    First ``rustup which --toolchain <channel> rustc`` (the toolchain's real
    executable, run directly), then ``rustup run <channel> rustc -vV``. None
    when rustup or the toolchain is absent, or when what rustup returns does
    not report the channel's release: a PATH compiler that merely reports the
    same release is never adopted as the pin. ``RUSTUP_AUTO_INSTALL=0``:
    identifying the pin must never download it."""
    rustup = _rustup_executable(env)
    if rustup is None:
        return None
    run_env = dict(os.environ if env is None else env)
    run_env["RUSTUP_AUTO_INSTALL"] = "0"
    run_env.pop("RUSTUP_TOOLCHAIN", None)
    which = _run_text([rustup, "which", "--toolchain", channel, "rustc"], cwd=cwd, env=run_env)
    path = which.strip() if which else None
    text = _run_text([path, "-vV"], cwd=cwd, env=run_env) if path and Path(path).is_file() else None
    command = path or ""
    if text is None:
        text = _run_text([rustup, "run", channel, "rustc", "-vV"], cwd=cwd, env=run_env)
        command = f"rustup run {channel} rustc"
    if text is None:
        return None
    fields = parse_rustc_verbose(text)
    if channel_matches(channel, fields.get("release")) is False or not fields.get("commit-hash"):
        return None
    return RustcIdentity(
        command=command,
        path=path,
        version=fields.get("version", ""),
        release=fields.get("release"),
        commit_hash=fields.get("commit-hash"),
    )


def embedded_rustc_commits(binary: Path | str) -> list[str]:
    """Every ``/rustc/<commit-hash>/`` std path embedded in ``binary``, sorted.

    One hash means one toolchain built it; none means the fingerprint is
    absent (not a Rust binary, or a heavily post-processed one)."""
    try:
        data = Path(binary).read_bytes()
    except OSError:
        return []
    return sorted({match.group(1).decode() for match in _EMBEDDED_RUSTC.finditer(data)})


@dataclass
class ToolchainReport:
    """What the header says about compilers: the lines to print, and whether
    anything is known to differ from the pin (``mismatch``) or could not be
    confirmed at all (``unverified``)."""

    lines: list[str]
    mismatch: bool
    unverified: bool
    channel: str | None = None
    binary_commits: list[str] = field(default_factory=list)


def assess_toolchain(
    *,
    channel: str | None,
    binary_commits: list[str] | None,
    binary_label: str | None,
    pinned: RustcIdentity | None,
    current: RustcIdentity | None,
) -> ToolchainReport:
    """Pure verdict over already-gathered facts (see :func:`toolchain_report`).

    ``binary_commits`` is None when no binary is being measured; then only
    the current PATH resolution is judged.
    """
    warn = "         ⚠️  "
    if channel is None:
        return ToolchainReport(["rustc:   no rust-toolchain.toml in this checkout -- compiler not checked"],
                               mismatch=False, unverified=True)
    lines: list[str] = []
    mismatch = unverified = False
    # Only the pinned toolchain itself (resolved through rustup) names the
    # pin's commit. A PATH compiler reporting the same release is NOT
    # adopted: it would let an unresolved pin read as a confirmed one.
    if pinned is not None and channel_matches(channel, pinned.release) is False:
        pinned = None
    pinned_hash = pinned.commit_hash if pinned else None
    known = {}
    for ident in (pinned, current):
        if ident is not None and ident.commit_hash:
            known.setdefault(ident.commit_hash, ident)

    def name(commit: str) -> str:
        ident = known.get(commit)
        return f"{ident.version} ({ident.path or ident.command})" if ident else f"an unidentified rustc (commit {commit[:12]})"

    pin_text = f"pin {channel} (rust-toolchain.toml{', commit ' + pinned_hash[:12] if pinned_hash else ''})"
    if binary_commits is not None:
        label = binary_label or "binary"
        if not binary_commits:
            lines.append(f"rustc:   compiler that built the {label} is UNKNOWN; {pin_text}")
            lines.append(warn + f"no /rustc/<commit> fingerprint in the {label}: cannot confirm it was built "
                                f"with the pinned toolchain.")
            unverified = True
        elif pinned_hash is None:
            lines.append(f"rustc:   {label} built by {', '.join(name(c) for c in binary_commits)}; {pin_text}")
            lines.append(warn + f"pinned toolchain {channel} is not resolvable through rustup here "
                                f"(`rustup which --toolchain {channel} rustc` / `rustup run {channel} rustc -vV` "
                                "failed): cannot confirm the binary's compiler. PATH is never taken as the pin.")
            unverified = True
        elif binary_commits == [pinned_hash]:
            lines.append(f"rustc:   {label} built by the pinned toolchain -- {name(pinned_hash)}")
        else:
            mismatch = True
            lines.append(f"rustc:   {label} built by {', '.join(name(c) for c in binary_commits)}")
            lines.append(warn + f"TOOLCHAIN MISMATCH: the {label} was NOT compiled by the {pin_text}. "
                                "Its numbers are not comparable with CI's. Rebuild with the pin: put "
                                "~/.cargo/bin ahead of /opt/homebrew/bin on PATH (see AGENTS.md 'Rust toolchain pin').")
    lead = "         " if lines else "rustc:   "
    if current is None:
        lines.append(f"{lead}rustc on PATH now: none resolvable")
        unverified = True
    else:
        verdict = channel_matches(channel, current.release)
        suffix = "" if verdict else ("  [!= pin " + channel + "]" if verdict is False else "  [symbolic pin: unchecked]")
        lines.append(f"{lead}rustc on PATH now: {current.describe()}{suffix}")
        if verdict is False and binary_commits is None:
            mismatch = True
            lines.append(warn + f"TOOLCHAIN MISMATCH: a build here would use rustc {current.release}, not the "
                                f"{pin_text}.")
    return ToolchainReport(lines, mismatch=mismatch, unverified=unverified, channel=channel,
                           binary_commits=list(binary_commits or []))


def toolchain_report(repo_root: Path | str, binary: BinaryIdentity | Path | str | None = None,
                     *, env: dict | None = None) -> ToolchainReport:
    """Which compiler built ``binary`` (by its embedded fingerprint), and is
    it ``repo_root``'s pinned toolchain? Also records what ``rustc`` on PATH
    resolves to now, which is what the *next* build would use -- a different
    question from what built the binary under test."""
    channel = pinned_rust_channel(repo_root)
    current = rustc_identity(cwd=repo_root, env=env)
    pinned = pinned_rustc_identity(channel, cwd=repo_root, env=env) if channel else None
    commits = label = None
    if binary is not None:
        path = binary.path if isinstance(binary, BinaryIdentity) else Path(binary)
        commits = embedded_rustc_commits(path)
        label = f"{binary.kind} binary" if isinstance(binary, BinaryIdentity) else "binary"
    return assess_toolchain(channel=channel, binary_commits=commits, binary_label=label,
                            pinned=pinned, current=current)


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
        # The compiler that built THIS binary (by its embedded fingerprint),
        # not merely the one PATH resolves now -- see toolchain_report.
        for line in toolchain_report(git.repo_root, binary).lines:
            print(line)
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
