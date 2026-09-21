#!/usr/bin/env python3
"""Probe the exact Perl/ExifTool oracle used for release evidence.

Release measurements must fail closed when the configured interpreter, pinned
source tree, modules, version, or container-format support is unavailable.  No
PATH lookup or alternate interpreter is permitted here.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys
from collections.abc import Sequence
from typing import Any


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2]))
from scripts.ops_paths import ops_root

DEFAULT_PERL = ops_root() / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2"
DEFAULT_CACHE_ROOT = ops_root() / "cache/exiftool"


class OracleProbeError(RuntimeError):
    """The configured release oracle is absent, skewed, or degraded."""


def _pin(repo: pathlib.Path) -> str:
    pin_path = repo / ".exiftool-version"
    if not pin_path.is_file():
        raise OracleProbeError(f"pin_path: missing file: {pin_path}")
    pin = pin_path.read_text(encoding="utf-8").strip()
    if not pin:
        raise OracleProbeError(f"pin_path: empty file: {pin_path}")
    return pin


def default_paths(repo: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    """Return explicit release-oracle paths, honoring only named overrides."""

    pin = _pin(repo.resolve())
    perl = pathlib.Path(os.environ.get("EXIFTOOL_PERL", str(DEFAULT_PERL)))
    cache = pathlib.Path(
        os.environ.get("EXIFTOOL_CACHE_DIR", str(DEFAULT_CACHE_ROOT / pin))
    )
    return perl, cache / "exiftool"


def _run(argv: Sequence[str], *, label: str) -> str:
    try:
        result = subprocess.run(
            argv,
            check=False,
            capture_output=True,
            text=True,
            env={
                **os.environ,
                "EXIFTOOL_HOME": os.devnull,
                "PERL5LIB": "",
                "PERLLIB": "",
                "PERL5OPT": "",
            },
        )
    except OSError as exc:
        raise OracleProbeError(f"{label}: could not execute: {exc}") from exc
    if result.returncode != 0:
        stderr = result.stderr.strip()
        raise OracleProbeError(
            f"{label}: exit {result.returncode}"
            + (f": {stderr}" if stderr else "")
        )
    return result.stdout.strip()


def _sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _library_fingerprint(library: pathlib.Path) -> tuple[str, int]:
    """Hash every regular library file by relative path and content identity."""

    digest = hashlib.sha256()
    files = sorted(path for path in library.rglob("*") if path.is_file())
    if not files:
        raise OracleProbeError(f"library_path: contains no files: {library}")
    for path in files:
        relative = path.relative_to(library).as_posix().encode("utf-8")
        digest.update(relative)
        digest.update(b"\0")
        digest.update(_sha256(path).encode("ascii"))
        digest.update(b"\n")
    return digest.hexdigest(), len(files)


def probe_oracle(
    repo: pathlib.Path, perl_path: pathlib.Path, tree_path: pathlib.Path
) -> dict[str, Any]:
    """Verify and describe one explicit pinned release oracle."""

    repo = repo.resolve()
    perl_path = perl_path.resolve()
    tree_path = tree_path.resolve()
    if not perl_path.is_file():
        raise OracleProbeError(f"perl_path: missing file: {perl_path}")
    if not os.access(perl_path, os.X_OK):
        raise OracleProbeError(f"perl_path: not executable: {perl_path}")
    if not tree_path.is_dir():
        raise OracleProbeError(f"tree_path: missing directory: {tree_path}")

    script = tree_path / "exiftool"
    library = tree_path / "lib"
    docx = tree_path / "t/images/OOXML.docx"
    for label, path, kind in (
        ("script_path", script, "file"),
        ("library_path", library, "directory"),
        ("docx_path", docx, "file"),
    ):
        exists = path.is_dir() if kind == "directory" else path.is_file()
        if not exists:
            raise OracleProbeError(f"{label}: missing {kind}: {path}")
    if (tree_path / ".ExifTool_config").exists():
        raise OracleProbeError(
            f"tree_path: unexpected .ExifTool_config: {tree_path / '.ExifTool_config'}"
        )

    expected = _pin(repo)
    perl_version = _run([str(perl_path), "-e", "print $^V"], label="Perl version")
    if perl_version != "v5.38.2":
        raise OracleProbeError(
            f"Perl version: expected 'v5.38.2', got {perl_version!r}"
        )
    _run(
        [
            str(perl_path),
            "-Mstrict",
            "-Mwarnings",
            "-MArchive::Zip",
            "-MCompress::Zlib",
            "-e",
            "1",
        ],
        label="Perl modules",
    )

    oracle = [str(perl_path), f"-I{library}", str(script), "-config", ""]
    version = _run([*oracle, "-ver"], label="ExifTool version")
    if version != expected:
        raise OracleProbeError(
            f"ExifTool version: expected {expected!r}, got {version!r}"
        )
    docx_type = _run(
        [*oracle, "-s3", "-FileType", str(docx)], label="DOCX capability"
    )
    if docx_type != "DOCX":
        raise OracleProbeError(
            f"DOCX capability: expected 'DOCX', got {docx_type!r}"
        )

    library_fingerprint, library_file_count = _library_fingerprint(library)
    return {
        "schema_version": 1,
        "status": "verified",
        "pin": expected,
        "perl_path": str(perl_path),
        "perl_sha256": _sha256(perl_path),
        "perl_version": perl_version,
        "tree_path": str(tree_path),
        "script_path": str(script),
        "script_sha256": _sha256(script),
        "library_fingerprint_path": str(library),
        "library_fingerprint_sha256": library_fingerprint,
        "library_file_count": library_file_count,
        "exiftool_version": version,
        "docx_path": str(docx),
        "docx_sha256": _sha256(docx),
        "docx_file_type": docx_type,
        "probes": ["perl-version", "required-modules", "exiftool-version", "docx"],
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=pathlib.Path, default=pathlib.Path.cwd())
    parser.add_argument("--perl", type=pathlib.Path)
    parser.add_argument("--exiftool-dir", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        default_perl, default_tree = default_paths(args.repo)
        receipt = probe_oracle(
            args.repo,
            args.perl or default_perl,
            args.exiftool_dir or default_tree,
        )
    except (OSError, OracleProbeError, ValueError) as exc:
        print(f"release oracle refused: {exc}", file=sys.stderr)
        return 1
    rendered = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.output is None:
        sys.stdout.write(rendered)
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
        print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
