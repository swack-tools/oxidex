#!/usr/bin/env python3
"""The capability ledger -- T1.4 (`docs/FLEET.md` P6, M5 "the strong check").

This is the fix for the `solo-ryzen5` incident: an agent spent hours writing
`src/parsers/specialized/legacy.rs` to route SWF/PICT/PPM/RA/KyoceraRAW, for
formats the tip had *already* routed as `5cef5b3d`, measured at 97.3%.
Nothing detected the duplication until a human diffed it.

`intent.py`'s other two checks (open-intent overlap, commit-message history)
are cheap text matches -- useful dedup, but exactly the kind of check a
determined-but-wrong agent talks itself past ("that commit was about
something else"). This module is the one meant to be un-arguable: it does
not read what a commit *says* it did, it asks the binary built from the tip
what it *actually does*, against the real pinned ExifTool oracle, on a real
corpus sample.

Two traps this module exists to not fall into (both named in AGENTS.md):

**"Detected is not parsed."** A format can produce a perfectly correct
`File:FileType`, `FileTypeExtension` and `MIMEType` while extracting zero of
its real tags -- `read_metadata` falls back to `add_identity_tags` for the
~40 formats with no parser, and that fallback alone is enough to make
`oxidex -j` on such a file *look* healthy. So this module never grades
"is the format detected"; it always runs a real tag-by-tag diff against the
oracle and grades the MISSING count. A source grep for the format name
(`grep_dispatch_evidence` below) is kept only as a diagnostic breadcrumb in
the refusal message -- it is never sufered alone to decide anything, which
matters concretely for KyoceraRAW: it has no `FileFormat::KyoceraRAW` token
anywhere in `format_dispatch.rs` at all (it is reached through
`detection/mod.rs`'s magic-byte sniff into `FileFormat::CameraRaw(_)`), so a
grep-only implementation of this check would confidently say "not routed"
about a format that is, measurably, 100% covered. Caught by hand while
building this module (see the T1.4 report).

**"A matching -ver is not a working oracle."** `probe_capability` below runs
the *exact* two-part probe the gate uses (`gate.sh`'s "Hard precondition"
comment, mirrored in `doctor.py`): `-ver` must print `13.59` *and*
`-s3 -FileType OOXML.docx` must print `DOCX`. The pinned tree's
`#!/usr/bin/env perl` can resolve a Homebrew perl with no `Archive::Zip`,
which prints the right `-ver` while silently reporting `FileType: ZIP` for
every container format. A ledger graded against that degraded oracle would
confidently report every container format as "already covered" (zero real
tags, but also zero of them show as MISSING because the oracle itself
returns nothing to compare against) -- the single most dangerous failure
mode this component has, since it would block all future work on those
formats. `check_scope` therefore treats a failed probe as an infrastructure
fault (`LedgerError`), never as evidence either way, and `intent.register`
fails the whole registration closed rather than guessing.

Standard library only. Every subprocess call decodes with
``errors="replace"`` rather than ``text=True`` -- a plain ``text=True``
decode crashes outright on at least one real sample in this corpus
(`Real.ra`'s ID3 tag carries a non-UTF-8 byte), which is its own small
instance of the same doctrine: a broken instrument must fail loudly, not
produce a stack trace that looks like the file was never measured.
"""

from __future__ import annotations

import os
import re
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

REPO_ROOT_FOR_IMPORT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT_FOR_IMPORT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT_FOR_IMPORT))

from scripts import instrument  # noqa: E402  (path must be set up first)

# --------------------------------------------------------------------- #
# Constants
# --------------------------------------------------------------------- #

# Ground rule 7 (T1.4 task brief) / AGENTS.md: never invoke bare `exiftool`.
# Same cache-dir convention as doctor.py (T0.1) and gate.sh (T0.3).
CACHE_DIR = Path(os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))
ORACLE_SCRIPT = CACHE_DIR / "exiftool-pinned.sh"
DOCX_SAMPLE = CACHE_DIR / "exiftool" / "t" / "images" / "OOXML.docx"
CORPUS_DIR = CACHE_DIR / "combined-samples"
EXPECTED_ORACLE_VERSION = "13.59"  # .exiftool-version at the repo root is the authority

# Same candidate list `scripts/compare_file.py` uses, RELEASE first rather
# than debug: this module answers "is this already true in what ships",
# which is the gate's `cargo build --release` artifact
# (`tools/fleet/gate.sh`), not whichever binary a dev loop happened to
# leave newest. compare_file.py orders debug-first because it serves fast
# local iteration -- a different question.
BINARY_CANDIDATES = ("target/release/oxidex", "target/debug/oxidex", "target/fixloop/oxidex")

# Filesystem facts and the banner: not extraction results, differ by
# machine/second, would drown the real signal. Identical set to
# scripts/compare_file.py's SKIP.
SKIP = frozenset(
    {
        "ExifToolVersion",
        "FileName",
        "Directory",
        "FileSize",
        "FileModifyDate",
        "FileAccessDate",
        "FileInodeChangeDate",
        "FilePermissions",
    }
)

# ExifTool's `-a -G1 -s` prints `[Group]  TagName : value`; oxidex prints a
# bare `TagName: value` under the same flags, so the group is OPTIONAL here
# -- see compare_file.py's own comment on this same regex for the incident
# that made the group non-mandatory.
TAG_LINE = re.compile(r"^(?:\[(?P<group>\S+)\]\s+)?(?P<name>[A-Za-z0-9_:]+)\s*:\s?(?P<value>.*)$")

# A format counts as "already covered" when MISSING is exactly zero (the bar
# 5cef5b3d's own commit message uses for all five of its formats -- PICT/PPM/
# SWF at 100%, RA/RAW at 90.9%/90.0% but MISSING 0 either way, the gap being
# WRONG/value-conversion, not extraction). The ratio tolerance below exists
# only for "low MISSING count" per FLEET_SPEC.md's own looser phrasing of
# this check; it is deliberately small, and every verdict states the exact
# counts so a caller can re-derive a stricter bar without re-measuring.
MAX_MISSING_RATIO_FOR_COVERED = 0.10
MIN_COMPARED_FOR_VERDICT = 1

# Diagnostic-only breadcrumb sources for grep_dispatch_evidence -- see the
# module docstring for why these are never authoritative on their own.
DISPATCH_SURFACE_FILES = (
    "src/core/format_dispatch.rs",
    "src/core/file_format.rs",
    "src/parsers/detection/mod.rs",
    "src/parsers/raw/metadata.rs",
)

# Corpus filenames that don't literally share the format token as their
# extension or stem. Everything else is found by extension/stem match in
# `find_sample_for_format` without needing an entry here.
FORMAT_SAMPLE_OVERRIDES = {
    # Other.pm's ProcessPPM reports one shared `FileType: PPM` for all three
    # NetPBM ASCII/binary variants (see src/parsers/detection/mod.rs's own
    # comment on this) -- there is no PGM.pgm/PBM.pbm sample in the corpus.
    "PGM": "PPM.ppm",
    "PBM": "PPM.ppm",
}

MAX_TAG_SCAN_FILES_DEFAULT = 60


class LedgerError(Exception):
    """The ledger could not produce a trustworthy verdict at all.

    Raised instead of ever guessing: an unusable oracle, a missing binary, or
    a corpus that cannot be read all mean "coverage is unknown", which is
    NOT the same thing as "not covered" -- see the module docstring on why
    treating an infrastructure fault as "not covered" would silently
    reintroduce the exact bug this module exists to close (a duplicate
    registers successfully because the instrument that should have caught
    it was quietly broken).
    """


# --------------------------------------------------------------------- #
# Capability probe -- must match gate.sh's "Hard precondition" exactly
# --------------------------------------------------------------------- #


@dataclass
class CapabilityProbe:
    ok: bool
    oracle_version: str
    docx_filetype: str
    detail: str


def probe_capability(
    oracle_script: Path = ORACLE_SCRIPT,
    docx_sample: Path = DOCX_SAMPLE,
    expected_version: str = EXPECTED_ORACLE_VERSION,
    timeout: int = 30,
) -> CapabilityProbe:
    """The exact two-part probe `gate.sh` runs as its hard precondition and
    `doctor.py` runs as its oracle check: `-ver` must equal
    `expected_version` AND `-s3 -FileType <docx_sample>` must equal `DOCX`.

    A matching `-ver` alone is not sufficient (AGENTS.md, "A matching -ver
    is not a working oracle") -- the pinned tree's `#!/usr/bin/env perl` can
    resolve a Homebrew perl with no `Archive::Zip`, silently degrading every
    ZIP-container format while `-ver` still prints the right release.
    """
    if not oracle_script.is_file():
        return CapabilityProbe(False, "", "", f"pinned oracle script missing: {oracle_script}")
    if not os.access(oracle_script, os.X_OK):
        return CapabilityProbe(False, "", "", f"pinned oracle script not executable: {oracle_script}")

    try:
        ver_out = subprocess.run(
            [str(oracle_script), "-ver"], capture_output=True, timeout=timeout
        )  # nosec B603
    except (OSError, subprocess.TimeoutExpired) as exc:
        return CapabilityProbe(False, "", "", f"could not run `{oracle_script} -ver`: {exc}")
    version = ver_out.stdout.decode("utf-8", "replace").strip()
    if version != expected_version:
        return CapabilityProbe(
            False,
            version,
            "",
            f"`{oracle_script} -ver` -> {version!r}, expected {expected_version!r} "
            f"(stderr: {ver_out.stderr.decode('utf-8', 'replace').strip()!r})",
        )

    if not docx_sample.is_file():
        return CapabilityProbe(
            False,
            version,
            "",
            f"-ver matched ({version}) but capability sample missing: {docx_sample}. "
            "A matching -ver alone is not a working oracle.",
        )
    try:
        docx_out = subprocess.run(
            [str(oracle_script), "-s3", "-FileType", str(docx_sample)],
            capture_output=True,
            timeout=timeout,
        )  # nosec B603
    except (OSError, subprocess.TimeoutExpired) as exc:
        return CapabilityProbe(False, version, "", f"could not run the capability probe: {exc}")
    filetype = docx_out.stdout.decode("utf-8", "replace").strip()
    if filetype != "DOCX":
        return CapabilityProbe(
            False,
            version,
            filetype,
            f"-ver reports {version} but `-s3 -FileType {docx_sample.name}` -> {filetype!r}, "
            "expected 'DOCX'. This is the degraded-interpreter failure mode: the pinned "
            "tree's #!/usr/bin/env perl found a perl with no Archive::Zip, so every "
            "ZIP-container format silently degrades while -ver still lies.",
        )
    return CapabilityProbe(True, version, filetype, f"-ver={version}, OOXML.docx -> DOCX (via {oracle_script})")


# --------------------------------------------------------------------- #
# Binary resolution -- "ask the binary at the tip", so know which binary
# --------------------------------------------------------------------- #


@dataclass
class BinaryResolution:
    path: Path
    candidate: str
    note: Optional[str]  # staleness warning, or None


def resolve_oxidex_binary(repo_root: Path, override: Optional[str] = None) -> BinaryResolution:
    """The oxidex binary this ledger will interrogate, resolved LOUDLY.

    Mirrors `scripts/instrument.py`'s `resolve_binary` doctrine (a harness
    that derives a binary path by convention and proceeds when it is absent
    fails closed on every later subprocess call, indistinguishable from a
    real regression) but returns rather than `sys.exit`s, since this is a
    library a caller (`intent.py`, tests) needs to catch cleanly. Also
    carries a staleness note -- built from the same source `staleness_note`
    compares against (HEAD's commit time and every dirty file's mtime) --
    because a ledger answering "is this already true" from a binary built
    before the change in question would be exactly the dangerous failure
    mode this module exists to avoid.
    """
    repo_root = Path(repo_root)
    if override:
        p = Path(override)
        if not p.is_file():
            raise LedgerError(f"oxidex binary not found at {p} (--binary override)")
        candidate = override
    else:
        p = None
        for rel in BINARY_CANDIDATES:
            cand = repo_root / rel
            if cand.is_file():
                p = cand
                candidate = rel
                break
        if p is None:
            raise LedgerError(
                f"no oxidex binary found under {repo_root} (tried {', '.join(BINARY_CANDIDATES)}) "
                "-- build one first (`cargo build --release --bin oxidex`); refusing to grade "
                "against a missing binary, which fails closed and looks exactly like a real gap"
            )

    git = instrument.git_state(repo_root)
    identity = instrument.BinaryIdentity(
        kind="oxidex", requested=str(p), path=p.resolve(), mtime=p.stat().st_mtime, size=p.stat().st_size
    )
    note = instrument.staleness_note(identity, git)
    return BinaryResolution(path=p.resolve(), candidate=candidate, note=note)


# --------------------------------------------------------------------- #
# Tag extraction
# --------------------------------------------------------------------- #


def _parse_tags(text: str) -> dict:
    values: dict = {}
    for line in text.splitlines():
        m = TAG_LINE.match(line)
        if not m:
            continue
        name = m["name"].split(":")[-1]
        values.setdefault(name, m["value"].strip())
    return values


def _read_tags(argv: list, timeout: int = 60) -> dict:
    """Run `argv`, decode leniently, parse `Group:Name : value` lines.

    `errors="replace"` rather than `text=True` deliberately -- see the
    module docstring. A non-zero exit is not itself fatal here: oxidex and
    ExifTool both still print whatever tags they found before an error on
    a partially-unsupported file, and that partial output is real evidence.
    """
    try:
        proc = subprocess.run(argv, capture_output=True, timeout=timeout)  # nosec B603
    except FileNotFoundError as exc:
        raise LedgerError(f"could not execute {argv[0]!r}: {exc}") from exc
    except subprocess.TimeoutExpired as exc:
        raise LedgerError(f"`{' '.join(argv)}` timed out after {timeout}s") from exc
    return _parse_tags(proc.stdout.decode("utf-8", "replace"))


# --------------------------------------------------------------------- #
# Sample discovery
# --------------------------------------------------------------------- #


def normalize_token(s: str) -> str:
    return re.sub(r"[^A-Za-z0-9]", "", s).upper()


def find_sample_for_format(format_name: str, corpus_dir: Path = CORPUS_DIR) -> Optional[Path]:
    """A corpus file to measure `format_name` against, or None.

    Data-driven off the corpus's own naming, not a hand-maintained table for
    every format oxidex might ever be asked about: the combined-samples
    corpus conventionally names each file `<Display>.<ext>`, and for every
    format this was checked against, either the extension or the stem
    normalizes to the format token (`Flash.swf` -> ext `swf` == `SWF`;
    `KyoceraRaw.raw` -> ext `raw` != `KYOCERARAW`, but stem `KyoceraRaw` ==
    `KYOCERARAW`). `FORMAT_SAMPLE_OVERRIDES` only covers the cases where
    neither matches (NetPBM's shared FileType).
    """
    if not corpus_dir.is_dir():
        return None
    key = normalize_token(format_name)

    override = FORMAT_SAMPLE_OVERRIDES.get(key)
    if override:
        p = corpus_dir / override
        if p.is_file():
            return p

    files = sorted(p for p in corpus_dir.iterdir() if p.is_file())
    for p in files:
        if normalize_token(p.suffix) == key:
            return p
    for p in files:
        if normalize_token(p.stem) == key:
            return p
    return None


# --------------------------------------------------------------------- #
# Diagnostic-only source evidence (never authoritative alone)
# --------------------------------------------------------------------- #


def grep_dispatch_evidence(repo_root: Path, format_name: str) -> dict:
    """{file: [matching lines]} for `format_name` across the dispatch
    surface -- a breadcrumb for the refusal message, NEVER the basis for a
    covered/not-covered verdict. See the module docstring: KyoceraRAW has no
    `FileFormat::KyoceraRAW` token in `format_dispatch.rs` at all, so a
    grep-only check would wrongly call a 100%-measured format unrouted.
    """
    repo_root = Path(repo_root)
    needle = format_name.strip()
    hits: dict = {}
    if not needle:
        return hits
    pattern = re.compile(re.escape(needle), re.IGNORECASE)
    for rel in DISPATCH_SURFACE_FILES:
        path = repo_root / rel
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        matches = [line.strip() for line in text.splitlines() if pattern.search(line)]
        if matches:
            hits[rel] = matches[:5]
    return hits


# --------------------------------------------------------------------- #
# The measurement -- the load-bearing function
# --------------------------------------------------------------------- #


@dataclass
class FormatCapability:
    format_name: str
    sample: Optional[Path]
    compared: int
    missing: int
    wrong: int
    missing_tags: list = field(default_factory=list)
    wrong_tags: list = field(default_factory=list)
    covered: bool = False
    reason: str = ""
    dispatch_hits: dict = field(default_factory=dict)


def _hits_summary(hits: dict) -> str:
    if not hits:
        return "none"
    return "; ".join(f"{f} ({len(lines)} line(s))" for f, lines in hits.items())


def measure_format(
    repo_root: Path,
    binary: Path,
    oracle_script: Path,
    format_name: str,
    corpus_dir: Path = CORPUS_DIR,
    binary_note: Optional[str] = None,
) -> FormatCapability:
    """Ask the binary at the tip whether `format_name` is already covered.

    Runs the pinned oracle and `binary` over the same corpus sample with
    identical flags, diffs tag-by-tag, and grades MISSING -- the exact
    "detected is not parsed" gap: a format can report a perfect
    File:FileType/MIMEType while extracting none of its real tags, and only
    a real diff catches that.
    """
    hits = grep_dispatch_evidence(repo_root, format_name)
    sample = find_sample_for_format(format_name, corpus_dir)
    if sample is None:
        return FormatCapability(
            format_name=format_name,
            sample=None,
            compared=0,
            missing=0,
            wrong=0,
            covered=False,
            reason=(
                f"{format_name}: no corpus sample found under {corpus_dir} to measure against -- "
                "refusing to claim coverage without evidence (doctrine: never approximate). "
                f"dispatch grep hits (diagnostic only): {_hits_summary(hits)}"
            ),
            dispatch_hits=hits,
        )

    et = _read_tags([str(oracle_script), "-G1", "-s", str(sample)])
    ox = _read_tags([str(binary), "-G1", "-s", str(sample)])

    compared = {k: v for k, v in et.items() if k not in SKIP}
    missing = sorted(k for k in compared if k not in ox)
    wrong = sorted(k for k in compared if k in ox and ox[k] != compared[k])

    ratio = (len(missing) / len(compared)) if compared else 1.0
    covered = len(compared) >= MIN_COMPARED_FOR_VERDICT and (
        len(missing) == 0 or ratio <= MAX_MISSING_RATIO_FOR_COVERED
    )

    reason = (
        f"{format_name}: measured {sample.name} via {oracle_script} (pinned {EXPECTED_ORACLE_VERSION}) "
        f"vs {binary} -- compared {len(compared)} tags, MISSING {len(missing)}, WRONG {len(wrong)}"
        f" ({ratio * 100:.1f}% missing). dispatch grep hits (diagnostic only, not the basis for this "
        f"verdict): {_hits_summary(hits)}"
    )
    if missing:
        reason += f". Missing: {', '.join(missing[:10])}" + (", ..." if len(missing) > 10 else "")
    if binary_note:
        # A stale binary measured here reported `readable 2702 -> 0` once and
        # `MISSING 11` for five already-routed formats once -- both looked
        # exactly like real gaps. The note must ride in the verdict itself,
        # not a side channel nobody prints.
        reason += f". WARNING: {binary_note}"

    return FormatCapability(
        format_name=format_name,
        sample=sample,
        compared=len(compared),
        missing=len(missing),
        wrong=len(wrong),
        missing_tags=missing,
        wrong_tags=wrong,
        covered=covered,
        reason=reason,
        dispatch_hits=hits,
    )


@dataclass
class TagCapability:
    tag: str
    sample: Optional[Path]
    scanned: int
    covered: bool
    reason: str


def measure_tag(
    repo_root: Path,
    binary: Path,
    oracle_script: Path,
    tag: str,
    corpus_dir: Path = CORPUS_DIR,
    max_scan: int = MAX_TAG_SCAN_FILES_DEFAULT,
) -> TagCapability:
    """Ask the binary at the tip whether `tag` (bare name or `Group:Name`) is
    already covered.

    "Is it in the enabled allowlist" (FLEET_SPEC.md's phrasing) is not
    implemented as a text lookup into `src/exiftool_tables/enabled.rs`
    because that allowlist only gates the *generic* ProcessBinaryData engine
    (Gate B, `(module, table)` pairs) -- a great many tags are emitted by
    dedicated hand-written parsers (`kyocera.rs`, `pict.rs`, ...) that never
    touch it, and a check tied to `enabled.rs` alone would wrongly call
    those tags uncovered. So this uses the same behavioural technique as
    `measure_format`: scan the corpus (bounded by `max_scan`, since there is
    no tag->file index) for a sample the oracle reports the tag on, then
    check whether `binary` also emits it there.
    """
    if not corpus_dir.is_dir():
        raise LedgerError(f"corpus directory missing: {corpus_dir}")
    bare = tag.split(":")[-1]
    files = sorted(p for p in corpus_dir.iterdir() if p.is_file())
    scanned = 0
    for sample in files:
        if scanned >= max_scan:
            break
        scanned += 1
        et = _read_tags([str(oracle_script), "-G1", "-s", str(sample)])
        compared_names = {k for k in et if k not in SKIP}
        if bare not in compared_names:
            continue
        ox = _read_tags([str(binary), "-G1", "-s", str(sample)])
        covered = bare in ox
        return TagCapability(
            tag=tag,
            sample=sample,
            scanned=scanned,
            covered=covered,
            reason=(
                f"tag {tag}: observed in oracle output for {sample.name} "
                f"(sample {scanned}/{max_scan} scanned) -- oxidex "
                f"{'also emits' if covered else 'does NOT emit'} it"
            ),
        )
    return TagCapability(
        tag=tag,
        sample=None,
        scanned=scanned,
        covered=False,
        reason=(
            f"tag {tag}: not observed in any of {scanned} corpus sample(s) scanned under "
            f"{corpus_dir} -- cannot claim coverage without evidence (doctrine: never approximate)"
        ),
    )


# --------------------------------------------------------------------- #
# Whole-scope report
# --------------------------------------------------------------------- #


@dataclass
class LedgerReport:
    probe: CapabilityProbe
    binary: BinaryResolution
    formats: list = field(default_factory=list)  # list[FormatCapability]
    tags: list = field(default_factory=list)  # list[TagCapability]

    @property
    def already_covered(self) -> bool:
        return any(f.covered for f in self.formats) or any(t.covered for t in self.tags)

    def covered_reasons(self) -> list:
        """Reasons for every already-covered item -- what a refusal cites."""
        out = [f.reason for f in self.formats if f.covered]
        out.extend(t.reason for t in self.tags if t.covered)
        return out

    def all_reasons(self) -> list:
        out = [f.reason for f in self.formats]
        out.extend(t.reason for t in self.tags)
        return out


def check_scope(
    repo_root: Path,
    scope: dict,
    oracle_script: Path = ORACLE_SCRIPT,
    binary_override: Optional[str] = None,
    corpus_dir: Path = CORPUS_DIR,
) -> LedgerReport:
    """The capability ledger's entry point: for every format/tag named in an
    intent's `scope`, ask the binary at the tip whether it is already
    covered. Raises `LedgerError` -- never returns a guess -- when the
    instrument itself (oracle or binary) cannot be trusted.
    """
    probe = probe_capability(oracle_script=oracle_script)
    if not probe.ok:
        raise LedgerError(
            f"capability probe failed: {probe.detail} -- refusing to grade against a possibly "
            "degraded oracle. A matching -ver alone is not a working oracle (AGENTS.md); grading "
            "here anyway risks the most dangerous failure mode this module has: confidently "
            "reporting every capability as already covered because the oracle silently returned "
            "nothing to compare against."
        )
    binary = resolve_oxidex_binary(repo_root, override=binary_override)

    formats = [
        measure_format(
            repo_root, binary.path, oracle_script, fmt, corpus_dir=corpus_dir, binary_note=binary.note
        )
        for fmt in scope.get("formats", [])
    ]
    tags = [
        measure_tag(repo_root, binary.path, oracle_script, tag, corpus_dir=corpus_dir)
        for tag in scope.get("tags", [])
    ]
    return LedgerReport(probe=probe, binary=binary, formats=formats, tags=tags)


def _main(argv=None) -> int:
    import argparse
    import json

    parser = argparse.ArgumentParser(description="Query the fleet capability ledger for a scope.")
    parser.add_argument("--repo", default=".", help="repo root to measure against (default: cwd)")
    parser.add_argument("--format", action="append", default=[], dest="formats")
    parser.add_argument("--tag", action="append", default=[], dest="tags")
    parser.add_argument("--binary", default=None, help="override the resolved oxidex binary")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)

    repo_root = Path(args.repo).resolve()
    scope = {"formats": args.formats, "tags": args.tags}
    try:
        report = check_scope(repo_root, scope, binary_override=args.binary)
    except LedgerError as exc:
        print(f"LEDGER ERROR: {exc}", file=sys.stderr)
        return 2

    if args.json:
        print(
            json.dumps(
                {
                    "probe": vars(report.probe),
                    "binary": {"path": str(report.binary.path), "note": report.binary.note},
                    "already_covered": report.already_covered,
                    "formats": [
                        {**{k: v for k, v in vars(f).items() if k != "sample"}, "sample": str(f.sample) if f.sample else None}
                        for f in report.formats
                    ],
                    "tags": [
                        {**{k: v for k, v in vars(t).items() if k != "sample"}, "sample": str(t.sample) if t.sample else None}
                        for t in report.tags
                    ],
                },
                indent=2,
            )
        )
    else:
        print(f"probe: {report.probe.detail}")
        print(f"binary: {report.binary.path}" + (f"  ({report.binary.note})" if report.binary.note else ""))
        for f in report.formats:
            print(f"  [{'COVERED' if f.covered else 'not covered'}] {f.reason}")
        for t in report.tags:
            print(f"  [{'COVERED' if t.covered else 'not covered'}] {t.reason}")
        print(f"already_covered: {report.already_covered}")
    return 1 if report.already_covered else 0


if __name__ == "__main__":
    sys.exit(_main())
