#!/usr/bin/env python3
"""Diff OxiDex against ExifTool across a corpus, and classify the differences.

The comparison report says which formats score badly. It does not say *why*,
and the why matters enormously for what you should do about it:

  RENAME    OxiDex read the value correctly and called it something ExifTool
            does not call it. Zero parsing work; fix the name and the tag
            counts as matched. BMP scoring 0% is entirely this.
  MISSING   ExifTool emits a tag OxiDex does not. Real extraction work.
  VALUE     Both emit the tag, values disagree. Usually a PrintConv gap.
  EXTRA     OxiDex-only, with no plausible ExifTool counterpart.

A tag-at-a-time fix loop cannot tell these apart, so it pays full price for
renames -- the cheapest possible class -- as if they were parsing work. This
script separates them mechanically, for every format at once.

Rename detection uses the generated tables as the name universe: if OxiDex
emits `BMP:Width`, ExifTool does not, and ExifTool's BMP table contains
`ImageWidth` which OxiDex is missing *and whose value matches*, that is a
rename, not two independent defects. The value check is what makes the
inference safe -- name similarity alone would guess, and guessing is how you
get a confident wrong mapping.

Matching is group-qualified: a same-named tag from two different groups is
only paired across groups when the value matches (a harmless group alias) or
the name was unique on both sides to begin with. Bare-name comparison is
group-blind, and group-blind comparison is not free of cost -- on a single-
file APE.mpc corpus, comparing every OxiDex `MPC:*`/`APE:*`/`ID3:*`/`ID3v1:*`
tag against every ExifTool tag sharing its bare name manufactured 10 false
VALUE diffs and one false cross-group MATCH out of tags that, group-
qualified, are 11 `MPC:*` MISSING, 11 `APE:*` MISSING, `ID3v1:*` EXTRA (Oxi-
Dex reads the trailer ExifTool's JSON writer drops when ID3v2 outranks it),
and zero VALUE. See test_conformance.py for the pinned regression.

EXTRA is a precision axis, not a recall penalty: it is reported (with a
`precision` column and an EXTRA vote table) but never enters the score/
ceiling denominator, so a format cannot buy a better score by inventing tags
and is not punished on recall for genuinely extra ones -- later stages
budget it explicitly (Step 21's default-mode EXTRA-budget gate reads this).

Real VALUE differences are further classed by severity -- identity,
structural, numeric, date_time, binary, display_only -- so a PrintConv
rounding nit doesn't read the same as a wrong decode.

The two sides' keys are split by two different rules, on purpose. The
oracle is asked for `-G0:1:4` keys ('EXIF:IFD0:Make'), and split_oracle_key
takes the FIRST segment as the reporting group and the LAST as the name --
ExifTool tag names never contain ':', so the middle segments are structure
only. OxiDex keys go through split_oxidex_key: first segment is the group,
EVERYTHING after it is the name, exactly the rule the plain -G era used. The
distinction matters because OxiDex emits keys whose middle segment is part
of the name ('OOXML:Custom:Division', 'PNG:tEXt:comment'); one shared
last-segment rule silently rewrote those to 'Division'/'comment' and
matched them, masking a name defect that is OxiDex's to fix.

Unmatched occurrences are reported one row each. Under -G0:1:4 two leftover
oracle rows from different family-1 groups ('EXIF:IFD0:Compression',
'EXIF:IFD1:Compression') spell the same family-0 report key, so the 2nd,
3rd... occurrence of a key carries ' (2)', ' (3)' rather than overwriting
the first -- see place_occurrence. MISSING/EXTRA are counts of occurrences,
not of distinct keys.
"""

import argparse
from contextlib import contextmanager
import hashlib
import json
import math
import os
import re
import shutil
import stat
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

# The single resolution point for "which ExifTool are we grading against".
# Kept in scripts/ so the Rust harnesses, the fleet scripts and this tool all
# answer that question the same way. instrument answers the other half --
# which oxidex, from what commit, over what corpus -- see its module doc.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import exiftool_oracle  # noqa: E402
import instrument  # noqa: E402

# Tags that describe the file on disk rather than its metadata. They differ by
# construction (paths, timestamps, the tool's own version) and would swamp the
# signal without saying anything about parser conformance.
IGNORE = {
    "SourceFile", "ExifToolVersion", "FileName", "Directory",
    "FileModifyDate", "FileAccessDate", "FileInodeChangeDate",
    "FilePermissions", "FileSize", "Now", "ProcessingTime",
}


class ReceiptError(ValueError):
    """A conformance receipt cannot be trusted as measured."""


PERL_STARTUP_ENV = ("PERL5OPT", "PERL5LIB", "PERLLIB")
ORACLE_SELECTOR_ENV = ("EXIFTOOL", "EXIFTOOL_CACHE_DIR", "EXIFTOOL_PERL")
ORACLE_LOCALE_ENV = {"LC_ALL": "C", "LANG": "C"}


def expected_exiftool_version():
    """Read the release pin from this checkout, never from a cache path."""
    marker = Path(__file__).resolve().parents[2] / ".exiftool-version"
    try:
        version = marker.read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise exiftool_oracle.OracleError(
            f"cannot read ExifTool release pin {marker}: {exc}") from exc
    if not version or not re.fullmatch(r"[0-9]+(?:\.[0-9]+)*", version):
        raise exiftool_oracle.OracleError(
            f"invalid ExifTool release pin in {marker}: {version!r}")
    return version


def _scrubbed_perl_env():
    env = os.environ.copy()
    for key in PERL_STARTUP_ENV:
        env.pop(key, None)
    for key in ORACLE_SELECTOR_ENV:
        env.pop(key, None)
    env.update(ORACLE_LOCALE_ENV)
    return env


@contextmanager
def scrubbed_perl_environment():
    """Prevent inherited Perl startup hooks from entering the oracle."""
    saved = {key: os.environ[key] for key in PERL_STARTUP_ENV if key in os.environ}
    try:
        for key in PERL_STARTUP_ENV:
            os.environ.pop(key, None)
        yield
    finally:
        for key in PERL_STARTUP_ENV:
            os.environ.pop(key, None)
        os.environ.update(saved)


def _reject_ambient_oracle_selectors():
    present = [f"${key}" for key in ORACLE_SELECTOR_ENV if key in os.environ]
    if present:
        raise exiftool_oracle.OracleError(
            "receipt oracle environment must not be set with ambient selectors: "
            + ", ".join(present)
        )


def _oracle_script_path_from_argv(argv):
    for raw in reversed(list(argv)):
        path = Path(raw)
        if path.name == "exiftool" and path.is_file():
            return path.resolve()
    raise ReceiptError("oracle command is missing its ExifTool executable")


def _oracle_script_path(oracle):
    return _oracle_script_path_from_argv(oracle.argv)


def _has_empty_config(argv):
    return any(argv[index:index + 2] == ["-config", ""]
               for index in range(len(argv) - 1))


def _insert_empty_config(argv):
    argv = list(argv)
    if _has_empty_config(argv):
        return argv
    script_index = None
    for index in range(len(argv) - 1, -1, -1):
        if Path(argv[index]).name == "exiftool":
            script_index = index
            break
    if script_index is None:
        script_index = 0
    return [*argv[:script_index + 1], "-config", "", *argv[script_index + 1:]]


def _oracle_probe(argv, extra):
    return subprocess.run(
        _insert_empty_config([*argv, *extra]),
        capture_output=True, text=True, errors="replace", env=_scrubbed_perl_env(),
    )


def _resolve_executable(raw, label):
    candidate = shutil.which(str(raw)) if not Path(str(raw)).is_absolute() else str(raw)
    if not candidate:
        raise exiftool_oracle.OracleError(f"{label} is not on PATH: {raw}")
    path = Path(candidate).resolve()
    if not path.is_file() or not os.access(path, os.X_OK):
        raise exiftool_oracle.OracleError(f"{label} is not executable: {path}")
    return path


def _oracle_inputs(exiftool_dir=None, perl=None):
    """Resolve a source tree and interpreter from explicit portable inputs."""
    if exiftool_dir:
        root = Path(exiftool_dir).expanduser().resolve()
        script = root / "exiftool"
    else:
        selected = os.environ.get("EXIFTOOL", "").strip()
        if selected:
            script = _resolve_executable(selected, "$EXIFTOOL")
            root = script.parent
        else:
            found = shutil.which("exiftool")
            if not found:
                raise exiftool_oracle.OracleError(
                    "no ExifTool source tree supplied; pass --exiftool-dir or set $EXIFTOOL")
            script = Path(found).resolve()
            root = script.parent
    lib = root / "lib"
    if not script.is_file() or not lib.is_dir():
        raise exiftool_oracle.OracleError(
            f"ExifTool source root must contain executable {script} and lib/: {root}")
    selected_perl = perl or os.environ.get("EXIFTOOL_PERL", "").strip() or "perl"
    return root, script, lib, _resolve_executable(selected_perl, "ExifTool Perl")


def _portable_oracle(exiftool_dir=None, perl=None):
    expected = expected_exiftool_version()
    root, script, lib, interpreter = _oracle_inputs(exiftool_dir, perl)
    module_probe = subprocess.run(
        [str(interpreter), "-MArchive::Zip", "-e", "1"],
        capture_output=True, text=True, errors="replace", env=_scrubbed_perl_env(),
    )
    if module_probe.returncode != 0:
        raise exiftool_oracle.DegradedError(
            f"ExifTool Perl {interpreter} cannot load Archive::Zip: "
            f"{module_probe.stderr.strip()}"
        )
    argv = [str(interpreter), f"-I{lib}", str(script), "-config", ""]
    version = _oracle_probe(argv, ["-ver"])
    if version.returncode != 0 or version.stdout.strip() != expected:
        raise exiftool_oracle.SkewError(
            f"ExifTool probe reported {version.stdout.strip()!r}; expected {expected} "
            f"from .exiftool-version ({version.stderr.strip()})"
        )
    return exiftool_oracle.Oracle(
        argv=argv, version=expected, pinned_version=expected,
        source=f"pinned source tree {root}", interpreter=str(interpreter),
        missing_modules=[],
    )


def resolve_oracle(exiftool_dir=None, *, perl=None, strict=False):
    """Resolve a portable oracle; strict mode requires explicit clean authority.

    Normal runs may use the CI-provided source tree and ``EXIFTOOL_PERL``.
    Strict release verification rejects ambient selectors and requires both the
    explicitly supplied source tree and interpreter before any subprocess can
    run.  There is deliberately no implicit resolver fallback in this path:
    its probes would execute inherited Perl startup/configuration controls
    before this module could isolate them.
    """
    if strict:
        _reject_ambient_oracle_selectors()
        if not exiftool_dir or not perl:
            raise exiftool_oracle.OracleError(
                "strict release oracle requires explicit ExifTool source and "
                "explicit Perl interpreter inputs"
            )
        return _portable_oracle(exiftool_dir, perl)
    return _portable_oracle(exiftool_dir, perl)


def check_oracle_capability(oracle, docx):
    out = _oracle_probe(
        oracle.argv,
        ["-s", "-s", "-s", "-FileType", str(docx)],
    )
    if out.returncode != 0 or out.stdout.strip() != "DOCX":
        raise exiftool_oracle.DegradedError(
            f"{oracle.display()} reports FileType {out.stdout.strip()!r} for {docx}; "
            "expected 'DOCX'"
        )


def binary_sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _file_identity(path):
    path = Path(path).resolve()
    info = path.stat()
    return {
        "sha256": binary_sha256(path),
        "size": info.st_size,
        "mode": stat.S_IMODE(info.st_mode),
    }


def _manifest_digest(files):
    payload = [
        [name, identity["sha256"], identity["size"], identity["mode"]]
        for name, identity in sorted(files.items())
    ]
    encoded = json.dumps(payload, separators=(",", ":"), ensure_ascii=True).encode()
    return hashlib.sha256(encoded).hexdigest()


def _manifest_for_paths(paths, root):
    root = Path(root).resolve()
    files = {}
    for raw_path in sorted({str(Path(path).resolve()) for path in paths}):
        path = Path(raw_path)
        try:
            name = str(path.relative_to(root))
        except ValueError as exc:
            raise ReceiptError(f"manifest path escapes root: {path}") from exc
        files[name] = _file_identity(path)
    return {"root": str(root), "file_count": len(files),
            "sha256": _manifest_digest(files), "files": files}


def corpus_manifest(paths):
    """Hash every selected corpus input so mutable samples cannot drift."""
    files = {}
    for raw_path in sorted({str(Path(path).resolve()) for path in paths}):
        files[raw_path] = _file_identity(raw_path)
    return {"file_count": len(files), "sha256": _manifest_digest(files),
            "files": files}


def _canonical_digest(value):
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"),
                         ensure_ascii=True).encode()
    return hashlib.sha256(encoded).hexdigest()


def _oracle_source_paths(source_root):
    source_root = Path(source_root).resolve()
    script = source_root / "exiftool"
    lib_root = source_root / "lib"
    if not script.is_file() or not lib_root.is_dir():
        raise ReceiptError("pinned oracle source is missing its executable or lib tree")
    return [script] + sorted(path for path in lib_root.rglob("*") if path.is_file())


def extension_set(spec):
    if not spec:
        return None
    return {e.strip().lower().lstrip(".") for e in spec.split(",") if e.strip()}


def select_corpus_files(corpus_paths, recursive, only, exts, excluded):
    """Re-enumerate the exact deterministic selection used by a receipt."""
    missing_roots = [path for path in corpus_paths if not os.path.isdir(path)]
    if missing_roots:
        raise ReceiptError("corpus root(s) not found: " + ", ".join(missing_roots))

    def walk(root):
        if recursive:
            return (os.path.join(directory, name)
                    for directory, _dirs, names in os.walk(root) for name in names)
        return (os.path.join(root, name) for name in os.listdir(root))

    def keep(path):
        if not os.path.isfile(path):
            return False
        base = os.path.basename(path)
        if only and only.lower() not in base.lower():
            return False
        ext = os.path.splitext(base)[1].lstrip(".").lower()
        if exts is not None and ext not in exts:
            return False
        return ext not in excluded

    seen_real = set()
    files = []
    for path in sorted(path for root in corpus_paths for path in walk(root) if keep(path)):
        real = os.path.realpath(path)
        if real in seen_real:
            continue
        seen_real.add(real)
        files.append(path)
    if not files:
        raise ReceiptError("corpus selection is empty")
    return files


def corpus_selection(corpus_paths, recursive, only, exts, excluded):
    return {
        "roots": [str(Path(path).resolve()) for path in corpus_paths],
        "recursive": bool(recursive),
        "only": only,
        "extensions": sorted(exts) if exts is not None else None,
        "excluded_extensions": sorted(excluded),
    }


def measurement_contract(corpus_paths, recursive, only, exts, excluded,
                         min_files, min_tags):
    """Return the invocation contract kept outside the JSON receipt."""
    return {
        "selection": corpus_selection(corpus_paths, recursive, only, exts, excluded),
        "floors": {"min_files": min_files, "min_tags": min_tags},
    }


def transcript_row(path, oracle_tags, candidate_tags=None, result=None):
    row = {
        "path": str(Path(path).resolve()),
        "scored": bool(oracle_tags),
        "oracle_sha256": _canonical_digest(oracle_tags),
        "oracle_occurrences": occurrence_count(oracle_tags, split_oracle_key),
    }
    if not oracle_tags:
        row.update({
            "candidate_sha256": None,
            "candidate_occurrences": 0,
            "matched_occurrences": 0,
            "missing_occurrences": 0,
            "extra_occurrences": 0,
            "value_occurrences": 0,
            "rename_source_occurrences": 0,
            "rename_target_occurrences": 0,
        })
        return row
    row.update({
        "candidate_sha256": _canonical_digest(candidate_tags),
        "candidate_occurrences": occurrence_count(candidate_tags, split_oxidex_key),
        "matched_occurrences": len(result["matched"]),
        "missing_occurrences": len(result["missing"]),
        "extra_occurrences": len(result["extra"]),
        "value_occurrences": len(result["value_diff"]),
        "rename_source_occurrences": len(result["renames"]),
        "rename_target_occurrences": len(result["renames"]),
    })
    return row


def measurement_transcript(rows):
    return {
        "schema": 1,
        "row_count": len(rows),
        "sha256": _canonical_digest(rows),
        "rows": rows,
    }


def _git_tree(repo_root):
    result = subprocess.run(
        ["git", "-C", str(repo_root), "rev-parse", "HEAD^{tree}"],
        capture_output=True, text=True, errors="replace",
    )
    if result.returncode != 0:
        raise ReceiptError(f"cannot resolve source tree: {result.stderr.strip()}")
    return result.stdout.strip()


def repo_identity(git, dirty_overridden=False):
    """Capture the source commit, tree, and clean-state identity."""
    return {
        "root": str(Path(git.repo_root).resolve()),
        "commit": git.commit,
        "tree": _git_tree(git.repo_root),
        "dirty": bool(git.dirty),
        "dirty_files": sorted(git.dirty_files),
        "dirty_overridden": bool(dirty_overridden),
    }


def oracle_identity(oracle):
    """Capture and hash every mutable input used by the pinned oracle."""
    if not oracle.interpreter or not oracle.argv:
        raise ReceiptError("oracle provenance is incomplete")
    script = _oracle_script_path(oracle)
    if not _has_empty_config(oracle.argv):
        raise ReceiptError("oracle command is missing the canonical empty ExifTool config")
    source_root = script.parent
    source_paths = _oracle_source_paths(source_root)
    capability_path = source_root / "t" / "images" / "OOXML.docx"
    runtime = Path(oracle.interpreter).resolve()
    return {
        "version": oracle.version,
        "pinned_version": oracle.pinned_version,
        "source": oracle.source,
        "source_root": str(source_root),
        "source_manifest": _manifest_for_paths(source_paths, source_root),
        "runtime": str(runtime),
        "runtime_sha256": binary_sha256(runtime),
        "argv": list(oracle.argv),
        "missing_modules": list(oracle.missing_modules),
        "verified": oracle.verified,
        "command": oracle.display(),
        "provenance": oracle.provenance(),
        "locale": dict(ORACLE_LOCALE_ENV),
        "capability": {
            "path": str(capability_path),
            "file_type": "DOCX",
            "sha256": binary_sha256(capability_path),
        },
    }


def _oracle_command(oracle, extra):
    if isinstance(oracle, dict):
        return _insert_empty_config([*oracle["argv"], *extra])
    return _insert_empty_config(
        oracle.command(extra) if hasattr(oracle, "command") else [*oracle.argv, *extra]
    )


def run_exiftool(oracle, path):
    out = subprocess.run(
        # No -n: ExifTool must apply PrintConv, because OxiDex applies its
        # own. Comparing converted output against raw values would report
        # every correctly-read tag as a value mismatch.
        #
        # -G0:1:4, not -G: ExifTool's JSON writer keeps ONE entry per key,
        # and under family-0 keys every MPF sub-image (MPImage1/2/3), every
        # IFD0/IFD1 pair and every repeated XMP block share a key, so all but
        # one occurrence vanish before this script ever sees them -- in both
        # directions: OxiDex's correct rows score EXTRA against a partner
        # that was dropped, and oracle rows OxiDex lacks are never counted
        # MISSING. Measured on combined-samples/Apple/Apple_iPhone11.jpg
        # (three embedded MPF images) with the pinned 13.59: `-G -s -j -a`
        # prints 7 MPImage keys, `-G0:1 -s -j -a` prints 23, and the text
        # form `-G0:1 -s -a` (one row per occurrence) prints 23 MPImage rows;
        # `oxidex -j` prints 23. The family-1 segment carries the structure
        # OxiDex's own -j output already prints, so the keys stop colliding.
        #
        # Family 4 on top of that: -G0:1 still collapsed a repeat inside ONE
        # family-1 group. Measured per file as text-mode `-G0:1 -s -a` rows
        # (one per occurrence, minus IGNORE) against parsed JSON keys over
        # the author's 200-file subset (residual.py, pinned 13.59): 393 of
        # 25,269 occurrences lost on 50 files -- MakerNotes:ImageName 53,
        # ImageData 53, TextStamp 43, JUMBF:JUMDType 25, JUMDLabel 25,
        # MakerNotes:BabyAge 15 ... Family 4 is ExifTool's per-instance
        # 'Copy N' group (lib/Image/ExifTool.pm:3856), so `-G0:1:4` spells
        # every repeat distinctly ('EXIF:IFD1:Copy2:XResolution'): residual
        # 0 over the same 200 files. split_oracle_key reads the first and
        # last segments only, so neither the family-1 nor the 'Copy N'
        # segment reaches a report or a --json-out consumer -- they see the
        # family-0 group strings they always saw.
        _oracle_command(oracle, ["-G0:1:4", "-s", "-j", "-a", path]),
        capture_output=True, text=True, errors="replace",
        env=_scrubbed_perl_env(),
    ).stdout
    try:
        # parse_float=str: the default turns ExifTool's "1.80" into 1.8, and
        # the harness then reports a value difference against byte-identical
        # OxiDex output. Rediscovered as a "bug" five separate times here.
        return json.loads(out, parse_float=str)[0]
    except (json.JSONDecodeError, IndexError, KeyError):
        return {}


def run_oxidex(binary, path):
    out = subprocess.run(
        [binary, "-j", path], capture_output=True, text=True, errors="replace",
    ).stdout
    try:
        # Same parse_float reasoning as run_exiftool: both sides must round-trip
        # their numbers identically or the comparison invents differences.
        d = json.loads(out, parse_float=str)
    except json.JSONDecodeError:
        return {}
    if isinstance(d, list):
        d = d[0] if d else {}
    return d if isinstance(d, dict) else {}


def split_oracle_key(k):
    """-> (group, name) for an ExifTool JSON key. ORACLE side only.

    ExifTool -G0:1 gives 'EXIF:IFD0:Make' (family 0, family 1, name) or --
    when the two families coincide -- just 'File:FileType'; with family 4
    asked for as well, 'EXIF:IFD0:Copy1:Make'. Bare names have no group.
    The reporting GROUP is the first ':'-segment (family 0), so every report
    row and every --json-out consumer sees the group strings it saw under
    plain -G; the NAME is the last segment. The middle segments exist only
    to keep the oracle's JSON keys distinct (its writer keeps one entry per
    key). ExifTool tag names never contain ':' (audited over the 4,238-file
    census at 25a2109e: zero oracle keys with a colon inside the name), so
    last-segment is exact on THIS side. It is wrong for OxiDex keys, which
    can carry ':' inside the name -- those go through split_oxidex_key.
    """
    if ":" not in k:
        return ("", k)
    segs = k.split(":")
    return (segs[0], segs[-1])


def split_oxidex_key(k):
    """-> (group, name) for an `oxidex -j` key: first segment, then the rest.

    OxiDex prints 'IFD0:Make' (its group, then the name) -- but it also
    prints keys whose middle segment is part of the NAME, not a group:
    ooxml.rs emits 'OOXML:Custom:<property>' (26 keys on t/images/OOXML.docx
    under the 25a2109e binary), and the PNG reader used to emit
    'PNG:<chunk>:<keyword>' ('PNG:tEXt:comment' on PNG.png, until
    staging/png-text-names). ExifTool spells those 'XML:Division' and
    'PNG:Comment', so the colon-in-name is a defect the census must keep
    exposing: with the pre-f3b5f5e6 first-segment rule, kept here verbatim,
    'Custom:Division' stays a distinct name and lands in EXTRA (or RENAME)
    beside the oracle's MISSING 'Division'. Applying the oracle's last-
    segment rule to this side -- which f3b5f5e6 did -- silently rewrote the
    name to 'Division' and reported a match.
    """
    return tuple(k.split(":", 1)) if ":" in k else ("", k)


def file_type(et):
    """ExifTool's File:FileType, whatever key shape the group flags gave it.

    Under -G0:1 the key is 'File:FileType' (family 1 == family 0, so ExifTool
    prints one segment), not 'File:File:FileType' -- but per_format must not
    depend on that quirk, so the lookup goes through split_oracle_key.
    """
    for k, v in et.items():
        if split_oracle_key(k) == ("File", "FileType"):
            return v
    return et.get("FileType")


def norm_value(v):
    """Loose value comparison: we are checking identity, not formatting."""
    if v is None:
        return ""
    if isinstance(v, (list, tuple)):
        return " ".join(norm_value(x) for x in v)
    s = str(v).strip()
    # Compare numbers numerically so 2 == 2.0 == "2".
    try:
        f = float(s)
        return f"{f:.6g}"
    except ValueError:
        return s.casefold()


def name_key(n):
    """Normalised name, for spotting pure spelling/case renames."""
    return "".join(ch for ch in n.lower() if ch.isalnum())


def distinctive(v):
    """Is this value strong enough to identify a tag on its own?

    Booleans, small integers and short strings recur across unrelated tags, so
    matching on them pairs things at random. The check exists because the naive
    version produced crossed nonsense -- `Blue -> RedTRC` *and* `Red -> BlueTRC`
    in the same file, because all three ICC curves hold identical data, and
    `Height -> Aperture` because both happened to be 8.
    """
    s = str(v).strip()
    if len(s) < 4:
        return False
    if s.casefold() in {"true", "false", "yes", "no", "none", "n/a", "inf"}:
        return False
    try:
        f = float(s)
        # Infinities parse ("float('-inf')" succeeds for -inf, Infinity,
        # +inf...) but int(f) below raises OverflowError, and an infinite
        # reading identifies nothing anyway -- same verdict as "inf" above.
        # NaN (int() -> ValueError) matches nothing including itself.
        if not math.isfinite(f):
            return False
        # Small round numbers are the worst offenders.
        return abs(f) >= 1000 and f != int(f) or abs(f) >= 10000
    except ValueError:
        return True


def infer_renames(missing, extra):
    """Pair OxiDex-only tags with missing ExifTool tags, conservatively.

    A pair is accepted only when it is unambiguous in BOTH directions -- the
    value identifies exactly one candidate on each side -- and additionally
    either the names normalise to the same string (BITPIX -> Bitpix) or the
    value is distinctive enough to stand alone (a timestamp, a long string).

    Ambiguous groups are left alone and reported as missing/extra. Under-
    claiming here is deliberate: a wrong rename would send someone to "fix" a
    correctly-named tag, which is worse than saying nothing.
    """
    by_val_missing = defaultdict(list)
    by_val_extra = defaultdict(list)
    for en, (_g, ev) in missing.items():
        by_val_missing[norm_value(ev)].append(en)
    for on, (_g, ov) in extra.items():
        by_val_extra[norm_value(ov)].append(on)

    renames = []
    for val, ens in by_val_missing.items():
        ons = by_val_extra.get(val)
        if not ons:
            continue
        if len(ens) != 1 or len(ons) != 1:
            continue  # ambiguous: several tags share this value
        en, on = ens[0], ons[0]
        if name_key(en) == name_key(on) or distinctive(val):
            renames.append((on, en, missing[en][1]))
    return renames


def tags_by_name(tags, split):
    """Collect every occurrence of a tag name, retaining its group and value.

    `split` is the side's own key rule -- split_oracle_key for ExifTool
    output, split_oxidex_key for `oxidex -j` -- and is deliberately not
    defaulted: the two rules differ on exactly the keys where it matters
    (see split_oxidex_key), so a caller must say which side it is holding.

    Group names normally should not affect a comparison: OxiDex deliberately
    normalises a number of ExifTool groups.  But names such as ``CreateDate``
    and ``XResolution`` can occur in more than one group in one file.  Keeping
    every occurrence lets ``compare`` match their values before deciding that
    a pair is a value difference.
    """
    by_name = defaultdict(list)
    for k, v in tags.items():
        g, n = split(k)
        if n in IGNORE:
            continue
        by_name[n].append((g, v))
    return by_name


def occurrence_count(tags, split):
    """Count comparable tag occurrences without collapsing duplicate names."""
    return sum(len(rows) for rows in tags_by_name(tags, split).values())


def receipt_instrument(repo, binary, oracle, corpus_paths, file_count,
                       scored_file_count, min_files, min_tags, binary_digest,
                       corpus, selection, transcript):
    """Return the complete identity needed to reproduce a receipt."""
    binary_path = Path(binary.path).resolve()
    binary_info = _file_identity(binary_path)
    return {
        "tool": "conformance.py",
        "repo": repo,
        "binary": {
            "kind": binary.kind,
            "requested": binary.requested,
            "path": str(binary_path),
            "sha256": binary_digest,
            "size": binary_info["size"],
            "mode": binary_info["mode"],
            "mtime": binary.mtime,
        },
        "oracle": oracle,
        "corpus_roots": [str(Path(path).resolve()) for path in corpus_paths],
        "file_count": file_count,
        "selected_file_count": file_count,
        "scored_file_count": scored_file_count,
        "corpus_manifest": corpus,
        "selection": selection,
        "measurement_transcript": transcript,
        "floors": {"min_files": min_files, "min_tags": min_tags},
    }


def _require_hex(value, length, label):
    if not isinstance(value, str) or len(value) != length \
            or any(ch not in "0123456789abcdef" for ch in value.lower()):
        raise ReceiptError(f"receipt {label} is not a valid hexadecimal identity")


def _validate_repo_identity(receipt_repo, current_git=None):
    required = {"root", "commit", "tree", "dirty", "dirty_files", "dirty_overridden"}
    if not isinstance(receipt_repo, dict) or not required <= receipt_repo.keys():
        raise ReceiptError("receipt provenance is missing repository identity")
    _require_hex(receipt_repo["commit"], 40, "repository commit")
    _require_hex(receipt_repo["tree"], 40, "repository tree")
    if not Path(receipt_repo["root"]).is_absolute() \
            or receipt_repo["dirty"] is not False \
            or receipt_repo["dirty_files"] != [] \
            or receipt_repo["dirty_overridden"] is not False:
        raise ReceiptError("receipt provenance is not a clean source identity")
    if current_git is None:
        current_git = instrument.git_state()
    current = repo_identity(current_git, False)
    if receipt_repo != current:
        raise ReceiptError("receipt provenance does not match the current source identity")


def _validate_binary_identity(binary, verify_binary):
    required = {"kind", "requested", "path", "sha256", "size", "mode", "mtime"}
    if not isinstance(binary, dict) or not required <= binary.keys():
        raise ReceiptError("receipt is missing full binary identity")
    if not Path(binary["path"]).is_absolute():
        raise ReceiptError("receipt binary identity must use an absolute path")
    _require_hex(binary["sha256"], 64, "binary SHA-256")
    if not isinstance(binary["size"], int) or binary["size"] <= 0 \
            or not isinstance(binary["mode"], int):
        raise ReceiptError("receipt binary identity has invalid size or mode")
    try:
        current = _file_identity(binary["path"])
    except OSError as exc:
        raise ReceiptError(f"candidate binary is unreadable: {exc}") from exc
    if verify_binary and (
        current["sha256"] != binary["sha256"]
        or current["size"] != binary["size"]
        or current["mode"] != binary["mode"]
    ):
        raise ReceiptError("candidate binary changed after measurement")


def _validate_corpus_identity(instrument_data, expected_selection):
    roots = instrument_data.get("corpus_roots")
    selected = instrument_data.get("selected_file_count")
    scored = instrument_data.get("scored_file_count")
    manifest = instrument_data.get("corpus_manifest")
    if not isinstance(roots, list) or not roots or any(
            not isinstance(root, str) or not Path(root).is_absolute() or not Path(root).is_dir()
            for root in roots):
        raise ReceiptError("receipt provenance is missing corpus roots")
    selection = instrument_data.get("selection")
    required_selection = {"roots", "recursive", "only", "extensions", "excluded_extensions"}
    if not isinstance(selection, dict) or not required_selection <= selection.keys():
        raise ReceiptError("receipt provenance is missing the corpus selection specification")
    if selection != expected_selection or selection["roots"] != roots \
            or not isinstance(selection["recursive"], bool) \
            or (selection["only"] is not None and not isinstance(selection["only"], str)):
        raise ReceiptError("receipt corpus selection does not match the external contract")
    for key in ("extensions", "excluded_extensions"):
        values = selection[key]
        if values is not None and (
                not isinstance(values, list)
                or values != sorted(set(values))
                or any(not isinstance(value, str) or not value for value in values)):
            raise ReceiptError("receipt corpus selection has invalid extension filters")
    if not isinstance(selected, int) or selected <= 0 \
            or not isinstance(scored, int) or scored <= 0 or scored > selected:
        raise ReceiptError("receipt provenance has invalid selected/scored file counts")
    if instrument_data.get("file_count") != selected:
        raise ReceiptError("receipt provenance has inconsistent selected file counts")
    if not isinstance(manifest, dict) or not isinstance(manifest.get("files"), dict):
        raise ReceiptError("receipt provenance is missing the corpus input manifest")
    if manifest.get("file_count") != selected or len(manifest["files"]) != selected:
        raise ReceiptError("receipt corpus manifest count does not match selection")
    paths = list(manifest["files"])
    for raw_path in paths:
        path = Path(raw_path)
        if not path.is_absolute() or not any(
                _is_below(path, Path(root)) for root in roots):
            raise ReceiptError("receipt corpus manifest contains an out-of-root path")
    try:
        selected_paths = select_corpus_files(
            roots, selection["recursive"], selection["only"],
            set(selection["extensions"]) if selection["extensions"] is not None else None,
            set(selection["excluded_extensions"]),
        )
    except ReceiptError as exc:
        raise ReceiptError(f"cannot re-enumerate corpus selection: {exc}") from exc
    selected_paths = sorted(str(Path(path).resolve()) for path in selected_paths)
    if selected_paths != sorted(paths):
        raise ReceiptError("receipt corpus manifest is not the deterministic selection")
    current = corpus_manifest(paths)
    if current != manifest:
        raise ReceiptError("corpus input changed after measurement")


def _is_below(path, root):
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


def _validate_oracle_identity(oracle, expected_oracle=None):
    required = {
        "version", "pinned_version", "source", "source_root", "source_manifest",
        "runtime", "runtime_sha256", "argv", "missing_modules", "verified",
        "command", "provenance", "locale", "capability",
    }
    if not isinstance(oracle, dict) or not required <= oracle.keys():
        raise ReceiptError("receipt provenance is missing complete oracle identity")
    try:
        trusted = expected_oracle or resolve_oracle(strict=True)
        trusted_identity = (trusted if isinstance(trusted, dict)
                            else oracle_identity(trusted))
    except (OSError, ReceiptError, exiftool_oracle.OracleError) as exc:
        raise ReceiptError(f"external oracle contract cannot be resolved: {exc}") from exc
    for key in required:
        if oracle.get(key) != trusted_identity.get(key):
            raise ReceiptError(f"receipt oracle identity does not match external {key}")
    expected_version = expected_exiftool_version()
    if oracle["version"] != expected_version \
            or oracle["pinned_version"] != expected_version \
            or oracle["verified"] is not True or oracle["missing_modules"] != []:
        raise ReceiptError("receipt provenance has an unverified oracle")
    if not all(isinstance(value, str) and value for value in (
            oracle["source"], oracle["command"], oracle["provenance"])):
        raise ReceiptError("receipt provenance has incomplete oracle source identity")
    source_root = Path(oracle["source_root"])
    runtime = Path(oracle["runtime"])
    argv = oracle["argv"]
    if not source_root.is_absolute() or not runtime.is_absolute() \
            or not isinstance(argv, list) or len(argv) < 2 \
            or not all(
                isinstance(arg, str) and (arg or (index > 0 and argv[index - 1] == "-config"))
                for index, arg in enumerate(argv)
            ):
        raise ReceiptError("receipt provenance has invalid oracle paths")
    _require_hex(oracle["runtime_sha256"], 64, "oracle runtime SHA-256")
    if Path(argv[0]).resolve() != runtime.resolve() \
            or _oracle_script_path_from_argv(argv) != (source_root / "exiftool").resolve():
        raise ReceiptError("receipt provenance has inconsistent oracle command identity")
    if not _has_empty_config(argv):
        raise ReceiptError("receipt provenance is missing the canonical empty ExifTool config")
    if oracle["locale"] != ORACLE_LOCALE_ENV:
        raise ReceiptError("receipt provenance has a non-deterministic oracle locale")
    source_manifest = oracle["source_manifest"]
    if not isinstance(source_manifest, dict) or source_manifest.get("root") != str(source_root):
        raise ReceiptError("receipt provenance is missing the oracle source manifest")
    expected_source_manifest = _manifest_for_paths(
        _oracle_source_paths(source_root), source_root)
    if source_manifest != expected_source_manifest:
        raise ReceiptError("oracle source manifest is incomplete or changed")
    if binary_sha256(runtime) != oracle["runtime_sha256"]:
        raise ReceiptError("oracle runtime changed after measurement")
    capability = oracle["capability"]
    if not isinstance(capability, dict) or set(("path", "file_type", "sha256")) - capability.keys():
        raise ReceiptError("receipt provenance is missing the oracle capability identity")
    capability_path = Path(capability["path"])
    if capability["file_type"] != "DOCX" or not capability_path.is_absolute() \
            or capability_path != source_root / "t" / "images" / "OOXML.docx":
        raise ReceiptError("receipt provenance has invalid oracle capability identity")
    _require_hex(capability["sha256"], 64, "oracle capability SHA-256")
    if binary_sha256(capability_path) != capability["sha256"]:
        raise ReceiptError("oracle capability input changed after measurement")
    version = subprocess.run(
        argv + ["-ver"], capture_output=True, text=True, errors="replace",
        env=_scrubbed_perl_env(),
    )
    docx = subprocess.run(
        argv + ["-s3", "-FileType", str(capability_path)],
        capture_output=True, text=True, errors="replace",
        env=_scrubbed_perl_env(),
    )
    if version.returncode != 0 or version.stdout.strip() != expected_version \
            or docx.returncode != 0 or docx.stdout.strip() != "DOCX":
        raise ReceiptError("oracle capability or version boundary no longer verifies")


def _validate_measurement_transcript(receipt):
    instrument_data = receipt["instrument"]
    transcript = instrument_data.get("measurement_transcript")
    if not isinstance(transcript, dict) \
            or transcript.get("schema") != 1 \
            or not isinstance(transcript.get("rows"), list) \
            or transcript.get("row_count") != len(transcript["rows"]):
        raise ReceiptError("receipt is missing a complete measurement transcript")
    if transcript.get("sha256") != _canonical_digest(transcript["rows"]):
        raise ReceiptError("measurement transcript commitment does not verify")

    manifest = instrument_data["corpus_manifest"]
    expected_paths = sorted(manifest["files"])
    rows = transcript["rows"]
    row_paths = [row.get("path") if isinstance(row, dict) else None for row in rows]
    if row_paths != expected_paths:
        raise ReceiptError("measurement transcript does not cover the exact corpus selection")

    binary_path = instrument_data["binary"]["path"]
    oracle = instrument_data["oracle"]
    totals = Counter()
    per_ext = defaultdict(Counter)
    rename_votes = defaultdict(Counter)
    missing_votes = Counter()
    extra_votes = Counter()
    severity_votes = Counter()
    per_file = {}
    for path, recorded in zip(expected_paths, rows):
        oracle_tags = run_exiftool(oracle, path)
        if oracle_tags:
            candidate_tags = run_oxidex(binary_path, path)
            result = compare(oracle_tags, candidate_tags)
            expected = transcript_row(path, oracle_tags, candidate_tags, result)
            ext = (file_type(oracle_tags) or os.path.splitext(path)[1].lstrip(".")).upper()
            counts = per_ext[ext]
            counts["files"] += 1
            counts["matched"] += len(result["matched"])
            counts["value_diff"] += len(result["value_diff"])
            counts["missing"] += len(result["missing"])
            counts["renames"] += len(result["renames"])
            counts["extra"] += len(result["extra"])
            for on, en, _value in result["renames"]:
                rename_votes[ext][f"{on}->{en}"] += 1
            for name in result["missing"]:
                missing_votes[f"{ext}:{name}"] += 1
            for name in result["extra"]:
                extra_votes[f"{ext}:{name}"] += 1
            for _name, _expected, _actual, severity in result["value_diff"]:
                severity_votes[severity] += 1
            per_file[path] = {
                "format": ext,
                "value_diff": [[n, ev, ov, severity]
                               for n, ev, ov, severity in result["value_diff"]],
                "extra": {key: list(value) for key, value in result["extra"].items()},
                "missing": {key: list(value) for key, value in result["missing"].items()},
                "parser_status": parser_status(path, candidate_tags),
                "family_views": family_views(oracle_tags, candidate_tags),
            }
        else:
            expected = transcript_row(path, oracle_tags)
        if recorded != expected:
            raise ReceiptError(f"measurement transcript does not verify for {path}")
        for key in (
                "oracle_occurrences", "candidate_occurrences", "matched_occurrences",
                "missing_occurrences", "extra_occurrences", "value_occurrences",
                "rename_source_occurrences", "rename_target_occurrences"):
            totals[key] += expected[key]
    if sum(row["scored"] for row in rows) != instrument_data["scored_file_count"]:
        raise ReceiptError("measurement transcript scored-file count does not verify")
    if any(receipt[key] != totals[key] for key in totals):
        raise ReceiptError("measurement transcript counters do not verify")
    if receipt["oracle_tag_count"] != totals["oracle_occurrences"]:
        raise ReceiptError("measurement transcript oracle floor does not verify")
    return {
        "per_format": {key: dict(value) for key, value in per_ext.items()},
        "renames": {key: dict(value) for key, value in rename_votes.items()},
        "missing": dict(missing_votes),
        "extra": dict(extra_votes),
        "severity": dict(severity_votes),
        "per_file": per_file,
        "parser_status": {},
        "family_views": {},
    }


def validate_receipt(receipt, verify_binary=True, current_git=None,
                     expected_contract=None, expected_oracle=None):
    """Reject incomplete, vacuous, tampered, or inconsistent receipts."""
    required = (
        "schema", "oracle_occurrences", "candidate_occurrences",
        "matched_occurrences", "missing_occurrences", "extra_occurrences",
        "value_occurrences", "rename_source_occurrences",
        "rename_target_occurrences", "oracle_tag_count", "instrument",
    )
    missing = [key for key in required if key not in receipt]
    if missing:
        raise ReceiptError("receipt missing required fields: " + ", ".join(missing))
    if receipt["schema"] != 1:
        raise ReceiptError(f"unsupported receipt schema: {receipt['schema']!r}")

    counters = (
        "oracle_occurrences", "candidate_occurrences", "matched_occurrences",
        "missing_occurrences", "extra_occurrences", "value_occurrences",
        "rename_source_occurrences", "rename_target_occurrences",
        "oracle_tag_count",
    )
    if any(not isinstance(receipt[key], int) or receipt[key] < 0 for key in counters):
        raise ReceiptError("receipt occurrence totals must be non-negative integers")

    instrument = receipt["instrument"]
    if not isinstance(instrument, dict):
        raise ReceiptError("receipt instrument identity must be an object")
    floors = instrument.get("floors")
    if not isinstance(floors, dict) or not isinstance(floors.get("min_files"), int) \
            or not isinstance(floors.get("min_tags"), int) \
            or floors["min_files"] <= 0 or floors["min_tags"] <= 0:
        raise ReceiptError("receipt is missing positive integer measurement floors")
    scored_file_count = instrument.get("scored_file_count", instrument.get("file_count", 0))
    if not isinstance(scored_file_count, int) or scored_file_count < floors["min_files"]:
        raise ReceiptError(
            f"receipt file count {scored_file_count} is below floor "
            f"{floors['min_files']}"
        )
    if receipt["oracle_tag_count"] != receipt["oracle_occurrences"]:
        raise ReceiptError("oracle tag floor is not bound to the occurrence denominator")
    if receipt["oracle_occurrences"] <= 0:
        raise ReceiptError("vacuous receipt has zero oracle occurrences")
    if receipt["oracle_occurrences"] < floors["min_tags"]:
        raise ReceiptError(
            f"receipt oracle occurrence count {receipt['oracle_occurrences']} is below floor "
            f"{floors['min_tags']}"
        )

    oracle_total = receipt["matched_occurrences"] + receipt["missing_occurrences"] \
        + receipt["value_occurrences"] + receipt["rename_source_occurrences"]
    candidate_total = receipt["matched_occurrences"] + receipt["extra_occurrences"] \
        + receipt["value_occurrences"] + receipt["rename_target_occurrences"]
    if receipt["oracle_occurrences"] != oracle_total:
        raise ReceiptError("oracle occurrence totals do not reconcile")
    if receipt["candidate_occurrences"] != candidate_total:
        raise ReceiptError("candidate occurrence totals do not reconcile")

    if not isinstance(expected_contract, dict) \
            or set(("selection", "floors")) - expected_contract.keys():
        raise ReceiptError("external measurement provenance contract is required")
    expected_floors = expected_contract["floors"]
    if floors != expected_floors:
        raise ReceiptError("receipt floors do not match the external contract")
    expected_selection = expected_contract["selection"]
    if not isinstance(expected_selection, dict):
        raise ReceiptError("external measurement selection contract is invalid")

    _validate_repo_identity(instrument.get("repo"), current_git)
    _validate_binary_identity(instrument.get("binary"), verify_binary)
    _validate_corpus_identity(instrument, expected_selection)
    _validate_oracle_identity(instrument.get("oracle"), expected_oracle)
    claims = _validate_measurement_transcript(receipt)
    for key, expected in claims.items():
        if receipt.get(key) != expected:
            raise ReceiptError(f"receipt {key} claims do not match replay")

    # Replay is another potentially long read of every input. Re-authenticate
    # all mutable identities after it, immediately before accepting the receipt.
    # The first checks protect the replay itself; these checks protect the
    # publication boundary from a persistent mid-validation mutation.
    _validate_repo_identity(instrument.get("repo"), current_git=None)
    _validate_binary_identity(instrument.get("binary"), verify_binary)
    _validate_corpus_identity(instrument, expected_selection)
    _validate_oracle_identity(instrument.get("oracle"), expected_oracle)
    return receipt


def occurrence_name(group, name, duplicate):
    """Report key for one unmatched occurrence.

    'Group:Name' when the name occurs more than once on either side of this
    file, bare 'Name' when it is unique. `group` is the reporting group --
    family 0 on the oracle side -- so under -G0:1:4 two leftover oracle rows
    from different family-1 groups ('EXIF:IFD0:Compression' and
    'EXIF:IFD1:Compression') spell the SAME key here; place_occurrence keeps
    them apart.
    """
    return f"{group}:{name}" if duplicate else name


def place_occurrence(rows, key, group, value):
    """Add one leftover occurrence to `rows` without overwriting an earlier one.

    The 2nd, 3rd... occurrence of a report key is stored as 'key (2)',
    'key (3)'. Before this, the dict write was last-writer-wins, so N
    unmatched oracle occurrences sharing a family-0 key survived as ONE
    MISSING row (DNG.dng: 'EXIF:Compression' kept IFD1's 'JPEG' and lost
    IFD0's 'Uncompressed'; 187 occurrences lost over a 200-file format-
    breadth subset, 969 leftovers -> 968 rows on a JPEG subset), so MISSING
    was a lower bound. Suffixing -- rather than carrying the family-1 group
    in the key -- keeps every key that existed before spelled exactly as it
    was, so per_file.missing/.extra keys stay comparable across runs and the
    'FMT:key' vote tables keep their shape; only a formerly-lost row gets a
    new key. The first occurrence in oracle key order (ExifTool's extraction
    order: IFD0 before IFD1) keeps the bare key. Applied to both sides for
    symmetry; an `oxidex -j` dict cannot actually collide, since its key IS
    'group:name'.
    """
    if key not in rows:
        rows[key] = (group, value)
        return key
    n = 2
    while f"{key} ({n})" in rows:
        n += 1
    rows[f"{key} ({n})"] = (group, value)
    return f"{key} ({n})"


_DATE_RE = re.compile(r"^\d{4}[:\-]\d{2}[:\-]\d{2}([ T]\d{2}:\d{2}:\d{2})?")


def classify_severity(expected, actual):
    """Bucket a genuine VALUE difference by what kind of gap it represents.

    This runs only after matching/missing/extra are already decided -- it
    never changes a count, it labels the value_diff pairs so a reviewer (or
    a later CI gate, see Stage 4's output-mode matrix) can tell a PrintConv
    rounding nit from a wrong decode without opening every file by hand.

    Classes: identity (same string modulo case/whitespace -- a formatting
    nit), date_time, binary (a "(Binary data N bytes...)" placeholder on
    either side), numeric (both sides parse as numbers but disagree),
    display_only (one side is a substring of the other -- usually a
    PrintConv applied on one side only, e.g. "5" vs "5 (Standard)"), and
    structural (the fallback: genuinely different data).
    """
    e, a = str(expected), str(actual)
    if _DATE_RE.match(e) or _DATE_RE.match(a):
        return "date_time"
    if "Binary data" in e or "Binary data" in a:
        return "binary"
    if e.casefold().strip() == a.casefold().strip():
        return "identity"
    try:
        float(e)
        float(a)
        return "numeric"
    except ValueError:
        pass
    e_norm, a_norm = e.casefold(), a.casefold()
    if e_norm and a_norm and (e_norm in a_norm or a_norm in e_norm):
        return "display_only"
    return "structural"


def _match_bucket(_name, expected, actual):
    """Resolve one tag-name bucket (all ET and OxiDex occurrences of `name`
    in one file) into matched / value_diff / leftover-missing / leftover-
    extra, group-qualified.

    Four tiers, most specific evidence first, each consuming what it pairs
    so a later tier never re-considers an already-decided occurrence:

      1. Exact group AND exact value.  Unambiguous under any circumstance.
      2. Exact value, any group.  A harmless group alias (OxiDex normalises
         a number of ExifTool's group names) still identifies the same
         underlying data, so this stays group-blind -- but only as a value-
         confirmed pairing, never a blind position grab.
      3. Exact group, differing value.  The strongest evidence of a real
         PrintConv/value discrepancy once (1)/(2) found nothing.
      4. Last resort: pair whatever is left, but ONLY when the tag name was
         unique on both sides to begin with (exactly one ET occurrence, one
         OxiDex occurrence). This is the fix for the APE.mpc cascade: the
         old code punted here whenever *anything* remained on the OxiDex
         side, regardless of how many candidates were competing, and that
         is what let an unrelated MPC:*/APE:*/ID3v1:* value get pulled in
         and reported as a false VALUE diff (or, when the value happened to
         coincide, a false MATCH) against a completely different real tag.
         When the name occurs more than once on either side, an unresolved
         leftover is reported as MISSING + EXTRA instead of a guessed pair
         -- under-claiming a defect classification is deliberate here, the
         same principle infer_renames() below already applies to renames.
    """
    original_expected_n = len(expected)
    original_actual_n = len(actual)
    duplicate = original_expected_n > 1 or original_actual_n > 1

    remaining_actual = list(actual)
    matched_pairs = []   # (group, value) from the ET side, for `matched`
    diff_pairs = []      # (e_group, e_value, a_group, a_value)

    # Tier 1: exact group + exact value.
    still = []
    for g, v in expected:
        idx = next(
            (i for i, (og, ov) in enumerate(remaining_actual)
             if og == g and norm_value(ov) == norm_value(v)),
            None,
        )
        if idx is not None:
            remaining_actual.pop(idx)
            matched_pairs.append((g, v))
        else:
            still.append((g, v))
    expected = still

    # Tier 2: exact value, any group (harmless alias tolerance).
    still = []
    for g, v in expected:
        idx = next(
            (i for i, (_og, ov) in enumerate(remaining_actual)
             if norm_value(ov) == norm_value(v)),
            None,
        )
        if idx is not None:
            remaining_actual.pop(idx)
            matched_pairs.append((g, v))
        else:
            still.append((g, v))
    expected = still

    # Tier 3: exact group, value differs.
    still = []
    for g, v in expected:
        idx = next(
            (i for i, (og, _ov) in enumerate(remaining_actual) if og == g),
            None,
        )
        if idx is not None:
            og, ov = remaining_actual.pop(idx)
            diff_pairs.append((g, v, og, ov))
        else:
            still.append((g, v))
    expected = still

    # Tier 4: cross-group punt, gated on name-uniqueness (see docstring).
    if (original_expected_n == 1 and original_actual_n == 1
            and expected and remaining_actual):
        g, v = expected.pop(0)
        og, ov = remaining_actual.pop(0)
        diff_pairs.append((g, v, og, ov))

    return matched_pairs, diff_pairs, expected, remaining_actual, duplicate


def compare(et, ox):
    et_by_name = tags_by_name(et, split_oracle_key)
    ox_by_name = tags_by_name(ox, split_oxidex_key)

    matched, value_diff = [], []
    missing, extra = {}, {}

    # Sorted, never the bare set: a set of str iterates in hash order, which
    # PYTHONHASHSEED changes per interpreter, and everything below inherits
    # this order -- value_diff and matched directly, missing/extra through
    # their insertion order (which infer_renames and place_occurrence read).
    # Unsorted, two censuses of identical output differed in --json-out's
    # per_file value_diff lists (fujim vs e1, 2026-09-12: 51 of 4,238 files
    # dict-unequal, 16 real). Within one name, oracle key order still rules.
    for n in sorted(et_by_name.keys() | ox_by_name.keys()):
        expected = et_by_name.get(n, [])
        actual = list(ox_by_name.get(n, []))

        matched_pairs, diff_pairs, leftover_expected, leftover_actual, duplicate = (
            _match_bucket(n, expected, actual)
        )

        matched.extend(n for _g, _v in matched_pairs)
        for _g, v, _og, ov in diff_pairs:
            value_diff.append((n, v, ov, classify_severity(v, ov)))
        for g, v in leftover_expected:
            place_occurrence(missing, occurrence_name(g, n, duplicate), g, v)
        for g, v in leftover_actual:
            place_occurrence(extra, occurrence_name(g, n, duplicate), g, v)

    renames = infer_renames(missing, extra)
    for on, en, _v in renames:
        missing.pop(en, None)
        extra.pop(on, None)

    return {
        "matched": matched,
        "value_diff": value_diff,
        "missing": missing,
        "extra": extra,
        "renames": renames,
    }


# --- Step 13 seam ---------------------------------------------------------
# Step 13 adds ReadReport (Parsed|Partial|IdentifiedOnly|Unsupported) to
# OxiDex's own output -- a machine-readable parse-status distinct from "did
# it emit any tags at all" (see AGENTS.md: "detected is not parsed"). Once
# that lands, this hook stops returning None and starts feeding a genuine
# IdentifiedOnly-per-format count into the report and into --json-out, so a
# format that only ever emits identity tags stops being invisible in the
# conformance table. Deliberately unimplemented here -- Step 13 owns it.
def parser_status(_path, _ox):
    return None


# --- Stage 4 seam ----------------------------------------------------------
# Stage 4 gives OxiDex a TagOccurrence store carrying family-0/1 group
# identity per occurrence. Once that exists, this hook can return a
# family-0 "ExifTool-compatible" view and a family-1 "OxiDex-structural"
# view of one file's comparison, so this instrument reports both without
# another rewrite of compare(). Deliberately unimplemented here -- Stage 4
# owns it.
def family_views(_et, _ox):
    return None


def positive_int(value):
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return parsed


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus", nargs="+",
                    help="one or more directories of sample files. Multiple "
                         "roots are scored as a single corpus, because no one "
                         "tree covers everything: ExifTool's own t/images is "
                         "the format-breadth corpus (~126 formats, pinned and "
                         "cloned in CI), while tests/fixtures carries the "
                         "OxiDex-specific samples. Duplicate paths across "
                         "roots are scored once.")
    ap.add_argument("--exiftool-dir",
                    help="ExifTool checkout root; its version must match "
                         ".exiftool-version")
    ap.add_argument("--oracle-perl",
                    help="Perl executable for the selected ExifTool source tree")
    ap.add_argument("--strict-release", action="store_true",
                    help="require an explicit external release oracle contract")
    ap.add_argument("--oxidex", default="./target/debug/oxidex")
    ap.add_argument("--only", help="substring filter on filename")
    ap.add_argument("--ext",
                    help="comma-separated extension allow-list, e.g. "
                         "'jpg,tif,png'. For narrowing a run to one format "
                         "while debugging. Default is no filter -- prefer "
                         "--exclude-ext for corpus-wide runs, so that a newly "
                         "added format is scored without editing anything.")
    ap.add_argument("--exclude-ext",
                    help="comma-separated extension deny-list, e.g. "
                         "'sh,md,py,json'. Corpora that double as test-fixture "
                         "trees carry harness scaffolding -- mock .sh scripts, "
                         ".json baselines, .md notes -- and ExifTool happily "
                         "scores those as ENV SCRIPT/JSON, whose 'tags' are "
                         "just object keys. tests/fixtures alone scores 83.8%% "
                         "with them and 96%%+ without.\n"
                         "Prefer this over --ext for corpus-wide runs: a deny-"
                         "list of things that are never metadata keeps scoring "
                         "every real format, including ones added later, "
                         "whereas an allow-list silently omits each new format "
                         "until someone remembers to extend it.")
    ap.add_argument("--recursive", action="store_true",
                    help="walk the corpus recursively (most sample corpora are "
                         "nested one directory per manufacturer)")
    ap.add_argument("--min-files", type=positive_int, default=1,
                    help="fail if fewer files than this were scored")
    ap.add_argument("--min-tags", type=positive_int, default=1,
                    help="fail if fewer ExifTool tags than this were seen")
    ap.add_argument("--show", type=int, default=0,
                    help="print per-file detail for the N worst files")
    ap.add_argument("--json-out")
    args = ap.parse_args()

    # Resolve every instrument before reading a single file. A run that
    # cannot say which ExifTool, which oxidex, and from what commit it
    # graded should not go on to report a number.
    try:
        oracle = resolve_oracle(args.exiftool_dir, perl=args.oracle_perl,
                                strict=args.strict_release)
    except exiftool_oracle.OracleError as exc:
        sys.exit(f"❌ {exc}")
    expected_version = expected_exiftool_version()
    if oracle.version != expected_version:
        sys.exit(
            f"❌ conformance.py requires ExifTool {expected_version} from "
            ".exiftool-version, "
            f"got {oracle.version}"
        )
    try:
        check_oracle_capability(
            oracle, _oracle_script_path(oracle).parent / "t" / "images" / "OOXML.docx")
    except (AttributeError, OSError, exiftool_oracle.OracleError) as exc:
        sys.exit(f"❌ ExifTool capability probe failed: {exc}")

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "conformance.py")
    if dirty_overridden:
        sys.exit("❌ conformance receipts require a clean source tree")
    try:
        repo_before = repo_identity(git)
        oracle_record = oracle_identity(oracle)
    except (OSError, ReceiptError) as exc:
        sys.exit(f"❌ cannot authenticate measurement inputs: {exc}")
    binary = instrument.resolve_binary(args.oxidex, kind="oxidex")
    try:
        measured_binary_sha256 = binary_sha256(binary.path)
    except OSError as exc:
        sys.exit(f"❌ cannot hash candidate binary {binary.path}: {exc}")

    # A missing root is fatal rather than skipped. Corpora are optional by
    # configuration, not by accident: silently dropping one that was asked for
    # would shrink the denominator and report a score for a corpus nobody
    # chose -- the same class of quiet wrongness the floors below exist to catch.
    exts = extension_set(args.ext)
    excluded = extension_set(args.exclude_ext) or set()
    try:
        files = select_corpus_files(args.corpus, args.recursive, args.only,
                                    exts, excluded)
    except ReceiptError as exc:
        if "root(s) not found" in str(exc):
            sys.exit(
                "❌ " + str(exc) + "\n"
                "   Refusing to score a partial corpus. Drop the root from the "
                "command line if it is genuinely not expected to be present."
            )
        sys.exit(
            f"no files in {', '.join(args.corpus)}"
            + (f" matching --ext {args.ext}" if exts else "")
        )
    selection = corpus_selection(args.corpus, args.recursive, args.only,
                                 exts, excluded)
    contract = measurement_contract(args.corpus, args.recursive, args.only,
                                    exts, excluded, args.min_files, args.min_tags)
    try:
        corpus_before = corpus_manifest(files)
    except OSError as exc:
        sys.exit(f"❌ cannot authenticate corpus inputs: {exc}")

    instrument.print_header(
        tool="conformance.py",
        git=git,
        binary=binary,
        dirty_overridden=dirty_overridden,
        oracle=oracle,
        corpus_paths=args.corpus,
        file_count=len(files),
    )

    per_ext = defaultdict(Counter)
    rename_votes = defaultdict(Counter)
    missing_votes = Counter()
    extra_votes = Counter()
    severity_votes = Counter()
    detail = []
    per_file = {}

    scored_files = 0
    et_tags_seen = 0
    oracle_occurrences = 0
    candidate_occurrences = 0
    matched_occurrences = 0
    missing_occurrences = 0
    extra_occurrences = 0
    value_occurrences = 0
    rename_source_occurrences = 0
    rename_target_occurrences = 0
    transcript_rows = []
    for path in files:
        et = run_exiftool(oracle, path)
        if not et:
            transcript_rows.append(transcript_row(path, et))
            continue
        scored_files += 1
        et_occurrences = occurrence_count(et, split_oracle_key)
        et_tags_seen += et_occurrences
        ox = run_oxidex(str(binary.path), path)
        r = compare(et, ox)
        transcript_rows.append(transcript_row(path, et, ox, r))
        oracle_occurrences += et_occurrences
        candidate_occurrences += occurrence_count(ox, split_oxidex_key)
        matched_occurrences += len(r["matched"])
        missing_occurrences += len(r["missing"])
        extra_occurrences += len(r["extra"])
        value_occurrences += len(r["value_diff"])
        rename_source_occurrences += len(r["renames"])
        rename_target_occurrences += len(r["renames"])
        ext = (file_type(et) or os.path.splitext(path)[1].lstrip(".")).upper()

        c = per_ext[ext]
        c["files"] += 1
        c["matched"] += len(r["matched"])
        c["value_diff"] += len(r["value_diff"])
        c["missing"] += len(r["missing"])
        c["renames"] += len(r["renames"])
        c["extra"] += len(r["extra"])

        for on, en, _v in r["renames"]:
            rename_votes[ext][(on, en)] += 1
        for n in r["missing"]:
            missing_votes[(ext, n)] += 1
        for n in r["extra"]:
            extra_votes[(ext, n)] += 1
        for _n, _ev, _ov, sev in r["value_diff"]:
            severity_votes[sev] += 1

        detail.append((len(r["missing"]) + len(r["renames"]), path, r))

        # (c) --json-out per-file VALUE/EXTRA identities, so a reviewer (or
        # a later stage's CI gate) can see exactly which tags disagreed on
        # which file without re-running the corpus. parser_status/
        # family_views are Step 13 / Stage 4 seams -- always None today.
        per_file[str(Path(path).resolve())] = {
            "format": ext,
            "value_diff": [[n, ev, ov, sev] for n, ev, ov, sev in r["value_diff"]],
            "extra": {k: list(v) for k, v in r["extra"].items()},
            "missing": {k: list(v) for k, v in r["missing"].items()},
            "parser_status": parser_status(path, ox),
            "family_views": family_views(et, ox),
        }

    # Refuse to print a number from a run that plainly did not happen. A
    # degraded oracle does not crash: it reads a fraction of the corpus and
    # reports a confident, precisely-formatted, completely wrong percentage.
    # Measured once at 109,261 tags over 832 files where a working oracle got
    # 507,295 over 4,230 -- nothing about the output looked wrong.
    if scored_files < args.min_files or et_tags_seen < args.min_tags:
        sys.exit(
            f"❌ vacuous run: scored {scored_files} file(s) / {et_tags_seen} ExifTool tag(s), "
            f"below the floor of {args.min_files}/{args.min_tags}.\n"
            f"   {len(files)} file(s) were found in {', '.join(args.corpus)}"
            f"{'' if args.recursive else ' (non-recursive; pass --recursive for a nested corpus)'}.\n"
            f"   oracle: {oracle.provenance()}\n"
            "   Check the oracle can actually read this corpus before trusting any score."
        )

    try:
        final_binary_sha256 = binary_sha256(binary.path)
    except OSError as exc:
        sys.exit(f"❌ candidate binary disappeared after measurement: {exc}")
    if final_binary_sha256 != measured_binary_sha256:
        sys.exit("❌ candidate binary changed after measurement")
    try:
        git_after = instrument.git_state()
        repo_after = repo_identity(git_after)
        corpus_after = corpus_manifest(files)
    except (OSError, ReceiptError) as exc:
        sys.exit(f"❌ cannot recheck measurement inputs: {exc}")
    if repo_after != repo_before:
        sys.exit("❌ source repository changed during measurement")
    if corpus_after != corpus_before:
        sys.exit("❌ corpus input changed during measurement")

    # score/ceiling (recall) are computed over matched+value_diff+missing+
    # renames only -- extra never enters that denominator, by design (see
    # AGENTS.md "bare-name comparison is group-blind" / project memory).
    # precision is the separate axis this step adds: how much of what
    # OxiDex emitted for a matched name was real vs. spurious. It is
    # reported, not folded into score, so a format cannot buy a better
    # score by emitting extra noise, and cannot be penalized on recall for
    # emitting it either -- extras are budgeted by later stages (Step 21's
    # "default-mode EXTRA budget ~= 0" gate reads this column).
    print(f"{'format':<10}{'files':>6}{'match':>7}{'rename':>8}"
          f"{'value':>7}{'missing':>9}{'extra':>7}{'score':>8}"
          f"{'ceiling':>9}{'precision':>11}")
    print("-" * 86)
    grand = Counter()
    for ext in sorted(per_ext):
        c = per_ext[ext]
        tot = c["matched"] + c["value_diff"] + c["missing"] + c["renames"]
        if not tot:
            continue
        grand.update(c)
        score = c["matched"] / tot
        # What the score becomes if every rename is corrected -- free coverage.
        ceiling = (c["matched"] + c["renames"]) / tot
        denom = c["matched"] + c["extra"]
        precision = (c["matched"] / denom) if denom else 1.0
        print(f"{ext:<10}{c['files']:>6}{c['matched']:>7}{c['renames']:>8}"
              f"{c['value_diff']:>7}{c['missing']:>9}{c['extra']:>7}"
              f"{score:>7.1%}{ceiling:>9.1%}{precision:>10.1%}")

    tot = grand["matched"] + grand["value_diff"] + grand["missing"] + grand["renames"]
    print("-" * 86)
    if tot:
        g_denom = grand["matched"] + grand["extra"]
        g_precision = (grand["matched"] / g_denom) if g_denom else 1.0
        print(f"{'TOTAL':<10}{grand['files']:>6}{grand['matched']:>7}"
              f"{grand['renames']:>8}{grand['value_diff']:>7}{grand['missing']:>9}"
              f"{grand['extra']:>7}"
              f"{grand['matched']/tot:>7.1%}"
              f"{(grand['matched']+grand['renames'])/tot:>9.1%}"
              f"{g_precision:>10.1%}")

    if rename_votes:
        print("\nrenames -- OxiDex reads these correctly under the wrong name.")
        print("value-confirmed, so these are name fixes, not parsing work:\n")
        for ext in sorted(rename_votes):
            for (on, en), n in rename_votes[ext].most_common():
                print(f"  {ext:<8} {on:<26} -> {en:<26} ({n} file{'s'*(n>1)})")

    if severity_votes:
        print("\nvalue differences by severity (precision debt, not recall):")
        for sev, c in severity_votes.most_common():
            print(f"  {sev:<14} {c}")

    print("\ntop genuinely missing tags (real extraction work):")
    for (ext, n), c in missing_votes.most_common(25):
        print(f"  {ext:<8} {n:<34} {c} file{'s'*(c>1)}")

    if extra_votes:
        print("\ntop OxiDex-only tags (precision axis -- budgeted, not scored):")
        for (ext, n), c in extra_votes.most_common(25):
            print(f"  {ext:<8} {n:<34} {c} file{'s'*(c>1)}")

    if args.show:
        for _k, path, r in sorted(detail, reverse=True)[:args.show]:
            print(f"\n--- {os.path.basename(path)} ---")
            print("  missing:", ", ".join(sorted(r["missing"])) or "-")
            print("  extra:  ", ", ".join(sorted(r["extra"])) or "-")
            if r["value_diff"]:
                print("  value:  ", ", ".join(
                    f"{n} [{sev}]" for n, _ev, _ov, sev in r["value_diff"]))

    if args.json_out:
        receipt = {
                 "schema": 1,
                 "oracle_occurrences": oracle_occurrences,
                 "candidate_occurrences": candidate_occurrences,
                 "matched_occurrences": matched_occurrences,
                 "missing_occurrences": missing_occurrences,
                 "extra_occurrences": extra_occurrences,
                 "value_occurrences": value_occurrences,
                 "rename_source_occurrences": rename_source_occurrences,
                 "rename_target_occurrences": rename_target_occurrences,
                 "oracle_tag_count": et_tags_seen,
                 "instrument": receipt_instrument(
                     repo_before, binary, oracle_record, args.corpus, len(files),
                     scored_files, args.min_files, args.min_tags,
                     measured_binary_sha256, corpus_before, selection,
                     measurement_transcript(transcript_rows)),
                 "per_format": {k: dict(v) for k, v in per_ext.items()},
                 "renames": {k: {f"{a}->{b}": n for (a, b), n in v.items()}
                             for k, v in rename_votes.items()},
                 "missing": {f"{e}:{n}": c for (e, n), c in missing_votes.items()},
                 # (b) extras as a precision axis, symmetric with `missing`.
                 "extra": {f"{e}:{n}": c for (e, n), c in extra_votes.items()},
                 # (d) severity histogram of real VALUE differences.
                 "severity": dict(severity_votes),
                 # (c) per-file VALUE/EXTRA/MISSING identities.
                 "per_file": per_file,
                 # (e)/(f) seams -- always empty until Step 13 / Stage 4 land.
                 "parser_status": {},
                 "family_views": {}}
        try:
            validate_receipt(receipt, current_git=git_after,
                             expected_contract=contract, expected_oracle=oracle)
        except (ReceiptError, OSError) as exc:
            sys.exit(f"❌ invalid conformance receipt: {exc}")
        with open(args.json_out, "w", encoding="utf-8") as fh:
            json.dump(receipt, fh, indent=2, sort_keys=True)
        print(f"\nwrote {args.json_out}")


if __name__ == "__main__":
    main()
