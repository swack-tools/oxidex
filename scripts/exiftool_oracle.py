"""The single place that decides *which* ExifTool a parity run grades against.

Python mirror of ``src/exiftool_oracle.rs``; see that module for the full
rationale. The short version: OxiDex's tables and parsers are transcribed from a
specific ExifTool source tree, and every harness here scores OxiDex against an
``exiftool`` it shells out to. When those are not the same ExifTool, the score
silently measures ExifTool-against-ExifTool as much as it measures OxiDex.

Two independent ways that went wrong, both closed here:

**Wrong release.** Harnesses defaulted to a bare ``"exiftool"``, resolving off
``PATH`` to 13.55, while the transcriptions came from the pinned tree at 13.59.
13.59 selects ``Canon::ColorData11`` with
``($count == 3973 or $count == 3778) and $$valPt =~ /^[\\0-\\x40]/``; 13.55 uses
``$$valPt !~ /^\\x41\\0/``. A Canon R6 Mark III lands on a different sub-table
under each, so sixteen correctly-transcribed tags were reported as regressions.
The failure is symmetric -- it manufactures phantom *fixes* too.

**Wrong interpreter.** The pinned tree's ``exiftool`` starts
``#!/usr/bin/env perl``, which finds Homebrew perl 5.42, which has no
``Archive::Zip``. ExifTool then cannot look inside a ZIP container, so
``OOXML.docx`` reports ``FileType: ZIP`` and every container format degrades at
once -- while ``-ver`` still prints 13.59. A version check alone passes that
oracle, which is why :func:`resolve` also probes capability.

Resolution order:

1. ``$EXIFTOOL`` -- an explicit binary path, for callers who mean it.
2. ``$EXIFTOOL_CACHE_DIR/exiftool/exiftool`` (cache dir defaults to
   ``/tmp/oxidex-exiftool-cache``) run under an explicitly chosen perl.
3. ``exiftool`` off ``PATH`` -- last resort, reported as unverified.

Because the pinned form needs an interpreter and an ``-I`` flag, an oracle is an
**argv prefix**, not a path. Build invocations with :meth:`Oracle.command`.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
from pathlib import Path
from typing import Any, NamedTuple

BINARY_ENV = "EXIFTOOL"
CACHE_DIR_ENV = "EXIFTOOL_CACHE_DIR"
PERL_ENV = "EXIFTOOL_PERL"
ALLOW_SKEW_ENV = "OXIDEX_ALLOW_EXIFTOOL_SKEW"
DEFAULT_CACHE_DIR = "/tmp/oxidex-exiftool-cache"

# Ordering is a preference, not the decision: choose_perl() picks the first that
# actually loads REQUIRED_MODULES. Bare "perl" is last precisely because
# `#!/usr/bin/env perl` finding a module-less Homebrew perl is the bug being fixed.
PERL_CANDIDATES = ("/usr/bin/perl5.34", "/usr/bin/perl", "perl")

# Archive::Zip gates every OOXML/ZIP-container format. Without it ExifTool
# reports FileType: ZIP for a .docx and says so only in a Warning nothing reads.
REQUIRED_MODULES = ("Archive::Zip",)

_VERSION_RE = re.compile(r"""^\s*\$VERSION\s*=\s*['"]([^'"]+)['"]""", re.M)


class OracleError(RuntimeError):
    """The oracle could not be resolved, or is unfit to grade against."""


class SkewError(OracleError):
    """The oracle's version disagrees with the pinned source tree."""


class DegradedError(OracleError):
    """The oracle runs under an interpreter missing a required module."""


class Oracle(NamedTuple):
    """A resolved ExifTool oracle."""

    argv: list[str]
    version: str
    pinned_version: str | None
    source: str
    interpreter: str | None
    missing_modules: list[str]

    def command(self, extra: list[str] | None = None) -> list[str]:
        """Full argv for an invocation: this oracle's prefix plus ``extra``."""
        return [*self.argv, *(extra or [])]

    def display(self) -> str:
        """The invocation as one human-readable string, for messages."""
        return " ".join(self.argv)

    @property
    def version_matches(self) -> bool:
        return self.pinned_version is not None and self.pinned_version == self.version

    @property
    def verified(self) -> bool:
        """True only when version matches the pin *and* nothing is missing.

        Both halves matter: the wrong-interpreter oracle reports the right
        version, and a right-version oracle that cannot open a ZIP still
        produces a wrong number.
        """
        return self.version_matches and not self.missing_modules

    def provenance(self) -> str:
        """One line naming the oracle, fit for a report header."""
        s = f"ExifTool {self.version}"
        if self.pinned_version is None:
            s += " (UNVERIFIED -- no pinned source tree"
        elif self.pinned_version != self.version:
            s += f" (SKEWED -- pinned tree is {self.pinned_version}"
        else:
            s += " (pinned"
        if self.interpreter:
            s += f", perl {self.interpreter}"
        if self.missing_modules:
            s += f", DEGRADED -- missing {', '.join(self.missing_modules)}"
        return s + f", via {self.source})"

    def check_container_support(self, docx: str | Path) -> None:
        """Functional proof that container formats work.

        The module probe in :func:`resolve` is the cheap always-on check; this
        is the end-to-end one, asserting the property that actually broke rather
        than a proxy for it. Raises :class:`DegradedError` on failure.
        """
        out = subprocess.run(  # nosec B603 -- list-argv, no shell
            self.command(["-s", "-s", "-s", "-FileType", str(docx)]),
            capture_output=True,
            text=True,
            errors="replace",
        ).stdout.strip()
        if out != "DOCX":
            raise DegradedError(
                f"{self.display()} reports FileType {out!r} for {docx} -- expected 'DOCX'. "
                "The interpreter is probably missing Archive::Zip, which silently degrades "
                "every ZIP-container format while `-ver` still prints the right release."
            )


def cache_dir() -> Path:
    """Root of the cached ExifTool checkout."""
    return Path(os.environ.get(CACHE_DIR_ENV) or DEFAULT_CACHE_DIR)


def pinned_binary(cache: Path | None = None) -> Path:
    """The ``exiftool`` script inside a cached checkout."""
    return (cache or cache_dir()) / "exiftool" / "exiftool"


def pinned_lib(cache: Path | None = None) -> Path:
    """The ``lib/`` of a cached checkout, for ``perl -I``."""
    return (cache or cache_dir()) / "exiftool" / "lib"


def repo_pin() -> str | None:
    """The ExifTool release this checkout is transcribed from.

    Read from ``.exiftool-version`` at the repo root. The repo is the authority
    on what it was written against; the cache directory is just a copy that may
    or may not match. Trusting the cache instead means a re-download silently
    redefines what "correct" is, with no tracked file changing.
    """
    for parent in Path(__file__).resolve().parents:
        marker = parent / ".exiftool-version"
        if marker.is_file():
            return marker.read_text(encoding="utf-8").strip() or None
    return None


def tree_version(cache: Path | None = None) -> str | None:
    """The release a cached checkout actually contains.

    ``lib/Image/ExifTool.pm``'s ``$VERSION`` is what that tree really is, so it
    is read first; the ``.exiftool-version`` marker the justfile writes is a
    fallback for a tree whose ``lib/`` is absent or unreadable. This answers
    "what is in the cache", not "what should we grade against" -- that is
    :func:`repo_pin`, and the two disagreeing is itself a reportable fault.
    """
    cache = cache or cache_dir()
    pm = cache / "exiftool" / "lib" / "Image" / "ExifTool.pm"
    try:
        m = _VERSION_RE.search(pm.read_text(encoding="utf-8", errors="replace"))
        if m:
            return m.group(1).strip()
    except OSError:
        pass
    try:
        return (cache / ".exiftool-version").read_text(encoding="utf-8").strip() or None
    except OSError:
        return None


def _runs(argv: list[str]) -> bool:
    try:
        return subprocess.run(  # nosec B603 -- list-argv, no shell
            argv, capture_output=True, text=True
        ).returncode == 0
    except OSError:
        return False


def missing_modules(perl: str, modules: tuple[str, ...] = REQUIRED_MODULES) -> list[str]:
    """Which of ``modules`` this perl cannot load."""
    return [m for m in modules if not _runs([perl, f"-M{m}", "-e", "1"])]


def choose_perl() -> str | None:
    """The perl to run the pinned tree under.

    The first candidate that loads every required module, else the first that
    runs at all. Selecting *by capability* is the point: a nominally-correct
    interpreter that cannot open a ZIP is what produced the wrong numbers.
    """
    override = os.environ.get(PERL_ENV, "").strip()
    if override:
        return override
    fallback = None
    for cand in PERL_CANDIDATES:
        if not _runs([cand, "-e", "1"]):
            continue
        if not missing_modules(cand):
            return cand
        fallback = fallback or cand
    return fallback


def shebang_interpreter(binary: str | Path) -> str | None:
    """Read the interpreter out of a script's ``#!`` line, resolving ``env``."""
    try:
        with open(binary, encoding="utf-8", errors="replace") as fh:
            first = fh.readline().strip()
    except OSError:
        return None
    if not first.startswith("#!"):
        return None
    parts = first[2:].strip().split()
    if not parts:
        return None
    if Path(parts[0]).name == "env":
        return parts[1] if len(parts) > 1 else None
    return parts[0]


def _probe_version(argv: list[str]) -> str:
    out = subprocess.run(  # nosec B603 -- list-argv, no shell
        [*argv, "-ver"], capture_output=True, text=True, errors="replace"
    )
    if out.returncode != 0:
        raise OracleError(f"`{' '.join(argv)} -ver` exited {out.returncode}: {out.stderr.strip()}")
    version = out.stdout.strip()
    if not version:
        raise OracleError(f"`{' '.join(argv)} -ver` printed nothing")
    return version


def _skew_allowed() -> bool:
    return os.environ.get(ALLOW_SKEW_ENV, "").lower() in {"1", "true"}


def resolve(explicit: str | None = None) -> Oracle:
    """Resolve the oracle, refusing one that is skewed or degraded.

    Raises rather than warning, because the bug class this module exists to kill
    is a wrong number that looked right. Set ``$OXIDEX_ALLOW_EXIFTOOL_SKEW=1``
    when the mismatch is genuinely intended.
    """
    cache = cache_dir()

    # What we require comes from the repo, not from whatever is in the cache. A
    # cache that disagrees is a fault in its own right: re-downloading it would
    # otherwise silently redefine "correct" without touching a tracked file, and
    # every later run would grade against a release nobody chose.
    pin = repo_pin()
    actual_tree = tree_version(cache)
    if (
        pin is not None
        and actual_tree is not None
        and actual_tree != pin
        and not _skew_allowed()
    ):
        raise SkewError(
            f"Cached ExifTool tree is {actual_tree}, but this checkout declares {pin} "
            "(.exiftool-version).\n"
            f"The cache under {cache} was fetched for a different release than the tables "
            "were transcribed from, so grading against it would measure "
            "ExifTool-vs-ExifTool.\n"
            f"Re-fetch the cache at {pin}, or update .exiftool-version and regenerate the "
            "transcriptions -- do not simply grade against the newer tree."
        )
    # No repo pin (e.g. running the script outside a checkout): fall back to the
    # cache so the harness still knows what it ran, just with less authority.
    pin = pin or actual_tree

    named = None
    if explicit and explicit.strip():
        named = (explicit.strip(), "explicit argument")
    elif os.environ.get(BINARY_ENV, "").strip():
        named = (os.environ[BINARY_ENV].strip(), f"${BINARY_ENV}")

    # A named binary is invoked as-is, so its own shebang picks the interpreter
    # and that is what we probe. The pinned tree instead gets an interpreter we
    # choose, because its `#!/usr/bin/env perl` is exactly what cannot be
    # trusted to find a capable one.
    if named:
        path, source = named
        argv, interpreter = [path], shebang_interpreter(path)
    else:
        tree = pinned_binary(cache)
        if tree.is_file():
            perl = choose_perl()
            if not perl:
                raise OracleError("no usable perl found to run the pinned ExifTool")
            argv = [perl, f"-I{pinned_lib(cache)}", str(tree)]
            source, interpreter = "pinned source tree", perl
        else:
            found = shutil.which("exiftool")
            argv, source = ["exiftool"], "PATH lookup"
            interpreter = shebang_interpreter(found) if found else None

    oracle = Oracle(
        argv=argv,
        version=_probe_version(argv),
        pinned_version=pin,
        source=source,
        interpreter=interpreter,
        missing_modules=missing_modules(interpreter) if interpreter else [],
    )

    if not _skew_allowed():
        if pin is not None and pin != oracle.version:
            raise SkewError(
                f"ExifTool version skew: grading against {oracle.version} "
                f"({oracle.display()}, via {source}), but the transcriptions come from "
                f"{pin} ({cache}).\n"
                "Different releases select different sub-tables for the same bytes, so every "
                "number from this run would be part OxiDex and part ExifTool-vs-ExifTool -- "
                "including tags that look like regressions but are correct, and tags that look "
                "fixed but are not.\n"
                f"Fix by unsetting ${BINARY_ENV} so the pinned tree is used.\n"
                f"If the skew is deliberate, set {ALLOW_SKEW_ENV}=1 and say which version you "
                "graded against."
            )
        if oracle.missing_modules:
            raise DegradedError(
                f"Degraded ExifTool oracle: {oracle.display()} runs under perl "
                f"{oracle.interpreter}, which cannot load "
                f"{', '.join(oracle.missing_modules)}.\n"
                "ExifTool needs Archive::Zip to look inside ZIP containers; without it a .docx "
                f"reports `FileType: ZIP` and every OOXML-ish format degrades at once. `-ver` "
                f"still prints {oracle.version}, so a version check alone does not catch this.\n"
                f"Fix by pointing ${PERL_ENV} at a perl that has the module (macOS system perl "
                "/usr/bin/perl5.34 does; Homebrew perl does not).\n"
                f"If you really want the degraded oracle, set {ALLOW_SKEW_ENV}=1 -- and do not "
                "quote coverage numbers from that run."
            )
    return oracle


def resolve_tree(tree: str | Path) -> Oracle:
    """Build an oracle from an explicit ExifTool source tree.

    ``tree`` is a checkout root holding ``exiftool`` and ``lib/`` -- what a
    ``--exiftool-dir`` flag names. The interpreter is still chosen by
    capability, and the version is still reported, so pointing a harness at a
    tree by hand does not opt out of knowing what it graded against.
    """
    tree = Path(tree)
    script, lib = tree / "exiftool", tree / "lib"
    if not script.is_file():
        raise OracleError(f"{script} is not a file -- --exiftool-dir must name a checkout root")
    perl = choose_perl()
    if not perl:
        raise OracleError("no usable perl found to run the ExifTool tree")
    argv = [perl, f"-I{lib}", str(script)]
    return Oracle(
        argv=argv,
        version=_probe_version(argv),
        # The tree grades itself: its own $VERSION is the pin, so a tree named
        # explicitly is self-consistent by construction and only the capability
        # check can fail it.
        pinned_version=_VERSION_RE.search(
            (lib / "Image" / "ExifTool.pm").read_text(encoding="utf-8", errors="replace")
        ).group(1)
        if (lib / "Image" / "ExifTool.pm").is_file()
        else None,
        source=f"--exiftool-dir {tree}",
        interpreter=perl,
        missing_modules=missing_modules(perl),
    )


_SHARED: Oracle | None = None


def shared(explicit: str | None = None) -> Oracle:
    """The process-wide oracle, resolved once."""
    global _SHARED
    if _SHARED is None or explicit:
        _SHARED = resolve(explicit)
    return _SHARED


def resolve_or_exit(explicit: str | None = None) -> Oracle:
    """:func:`shared`, but print the failure and exit rather than raising.

    Intended for script entry points. A parity run that cannot say which
    ExifTool it graded against should not go on to report a number.
    """
    try:
        oracle = shared(explicit)
    except OracleError as exc:
        print(f"❌ {exc}", file=sys.stderr)
        raise SystemExit(2) from exc
    if not oracle.verified:
        print(f"⚠️  {oracle.provenance()}", file=sys.stderr)
        print(
            "⚠️  This run's numbers are not attributable to a known-good ExifTool.",
            file=sys.stderr,
        )
    return oracle


def run_json(oracle: Oracle, args: list[str], path: str | Path) -> Any:
    """Run the oracle and parse its JSON with floats left as strings.

    ``parse_float=str`` is not a nicety. Python's default float handling turns
    ExifTool's ``"1.80"`` into ``1.8``, and the harness then reports a value
    difference against an OxiDex output that was byte-identical. That has been
    rediscovered as a "bug" at least five separate times in this repo.
    """
    out = subprocess.run(  # nosec B603 -- list-argv, no shell
        oracle.command([*args, str(path)]),
        capture_output=True,
        text=True,
        errors="replace",
    ).stdout
    try:
        return json.loads(out, parse_float=str)
    except json.JSONDecodeError:
        return None


def main() -> int:
    """``python3 scripts/exiftool_oracle.py`` prints the resolved oracle."""
    oracle = resolve_or_exit()
    print(oracle.provenance())
    print(f"argv:    {oracle.display()}")
    print(f"version: {oracle.version}")
    print(f"pinned:  {oracle.pinned_version}")
    print(f"perl:    {oracle.interpreter}")

    docx = cache_dir() / "combined-samples" / "OOXML.docx"
    if docx.is_file():
        oracle.check_container_support(docx)
        print("docx:    DOCX (container support confirmed)")
    else:
        print("docx:    (no sample available to confirm container support)")
    return 0 if oracle.verified else 1


if __name__ == "__main__":
    raise SystemExit(main())
