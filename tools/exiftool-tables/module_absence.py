#!/usr/bin/env python3
"""Prove that an ExifTool module is absent from one release's own ``lib/``.

Older ExifTool releases legitimately lack modules later releases ship
(11.78 has no ``InfiRay.pm`` and no ``NikonSettings.pm``). A generator that
reads such a module must then record "absent in this release" as an explicit
state instead of crashing -- but only when the absence is proven from the
selected tree itself, and never from a version label. This helper is that
proof, shared by every call site that needs it:

* ``lib`` must be a complete release tree: ``Image/ExifTool.pm`` is a regular
  file inside it.
* ``Image/ExifTool/<Module>.pm`` must be absent: ``stat`` fails with ENOENT.
  Any other ``stat`` failure is a refusal, not an absence.
* Every token -- the module name plus the caller's distinctive names (its
  process procedure, its dispatch marker) -- occurs nowhere in any regular
  file of the tree: not declared, not referenced, not in a comment. Matching
  is case-sensitive on identifier boundaries, the same rule
  ``dump_af_points.pl`` applies. A reference without the module file is a
  broken or partial tree, or a moved module, never an absence.

The record binds the sha256 of ``Image/ExifTool.pm`` and a digest over every
file's relative path and sha256, with scan counts. The release label is
recorded for the reader's convenience only; nothing here or in any consumer
decides anything from it.

A present module is not this helper's business: callers stat for the module
first and use their normal, strict loader when it is there, so a module that
is present but broken or changed still refuses in that loader.

CLI (for Perl call sites)::

    module_absence.py --lib LIB --module InfiRay --token IJPEG --token HasIJPEG

prints the JSON record and exits 0 only when absence is proven; otherwise it
prints the refusal to stderr and exits 1.
"""
from __future__ import annotations

import argparse
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
from typing import Iterable

SCHEMA = "exiftool_module_absent_v1"
_IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*\Z")
_SHA256 = re.compile(r"[0-9a-f]{64}\Z")
_LABEL = re.compile(rb"^\s*\$VERSION\s*=\s*['\"]([^'\"]*)['\"]", re.M)


class NotAbsent(RuntimeError):
    """Absence could not be proven; the caller must refuse."""


def _pattern(token: str) -> re.Pattern[bytes]:
    return re.compile(rb"(?<![A-Za-z0-9_])" + re.escape(token.encode("ascii")) + rb"(?![A-Za-z0-9_])")


def _tokens(module: str, tokens: Iterable[str]) -> list[str]:
    if not isinstance(module, str) or not _IDENT.match(module):
        raise ValueError(f"module must be a bare ExifTool module name, got {module!r}")
    names = [module]
    for token in tokens:
        if not isinstance(token, str) or not _IDENT.match(token):
            raise ValueError(f"token must be an identifier, got {token!r}")
        if token not in names:
            names.append(token)
    return names


def _files(lib: Path) -> list[Path]:
    found = []
    for root, dirs, names in os.walk(lib):
        dirs.sort()
        for name in names:
            path = Path(root, name)
            mode = os.stat(path).st_mode
            if stat.S_ISREG(mode):
                found.append(path)
    return sorted(found, key=lambda p: p.relative_to(lib).as_posix())


def prove_module_absent(lib: os.PathLike | str, module: str, tokens: Iterable[str] = ()) -> dict:
    """Return the absence record for ``module`` in ``lib``, or raise NotAbsent."""
    names = _tokens(module, tokens)
    lib = Path(lib).resolve()
    anchor = lib / "Image" / "ExifTool.pm"
    try:
        if not stat.S_ISREG(os.stat(anchor).st_mode):
            raise NotAbsent(f"{anchor} is not a regular file, so {lib} is not a release lib/ tree")
    except OSError as error:
        raise NotAbsent(f"{lib} is not a complete release lib/ tree: {anchor}: {error.strerror}") from error

    module_file = f"Image/ExifTool/{module}.pm"
    try:
        os.stat(lib / module_file)
    except OSError as error:
        if error.errno != errno.ENOENT:
            raise NotAbsent(f"cannot stat {module_file} in {lib}: {error.strerror}") from error
    else:
        raise NotAbsent(f"present: {module_file} exists in {lib}")

    patterns = {name: _pattern(name) for name in names}
    occurrences = {name: 0 for name in names}
    hits: list[str] = []
    inventory = hashlib.sha256()
    anchor_sha256 = None
    label = None
    files = _files(lib)
    for path in files:
        rel = path.relative_to(lib).as_posix()
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        inventory.update(f"{rel}\0{digest}\n".encode("utf-8", "surrogateescape"))
        if rel == "Image/ExifTool.pm":
            anchor_sha256 = digest
            found = _LABEL.search(data)
            label = found.group(1).decode("latin-1") if found else None
        for name, pattern in patterns.items():
            count = len(pattern.findall(data))
            if count:
                occurrences[name] += count
                hits.append(f"{rel}:{name}x{count}")
    if anchor_sha256 is None:
        raise NotAbsent(f"{anchor} was not scanned")
    if hits:
        raise NotAbsent(f"{module_file} is absent but referenced (broken, partial or moved): " + ", ".join(hits))
    return {
        "kind": SCHEMA,
        "module": module,
        "module_file": module_file,
        "module_file_present": False,
        "tokens": names,
        "occurrences_in_release": occurrences,
        "release_files_scanned": len(files),
        "release_pm_files_scanned": sum(1 for p in files if p.suffix == ".pm"),
        "release_inventory_sha256": inventory.hexdigest(),
        "exiftool_pm_sha256": anchor_sha256,
        "release_label_not_used_as_proof": label,
    }


def validate_absence_record(record: object, module: str) -> dict:
    """Refuse a record that does not carry a complete source-level proof."""
    if not isinstance(record, dict):
        raise NotAbsent(f"absence record for {module} is not an object")
    occurrences = record.get("occurrences_in_release")
    tokens = record.get("tokens")
    ok = (
        record.get("kind") == SCHEMA
        and record.get("module") == module
        and record.get("module_file") == f"Image/ExifTool/{module}.pm"
        and record.get("module_file_present") is False
        and isinstance(tokens, list) and tokens and tokens[0] == module
        and isinstance(occurrences, dict) and set(occurrences) == set(tokens)
        and all(type(v) is int and v == 0 for v in occurrences.values())
        and type(record.get("release_files_scanned")) is int and record["release_files_scanned"] >= 1
        and type(record.get("release_pm_files_scanned")) is int and record["release_pm_files_scanned"] >= 1
        and all(isinstance(record.get(k), str) and _SHA256.match(record[k])
                for k in ("release_inventory_sha256", "exiftool_pm_sha256"))
    )
    if not ok:
        raise NotAbsent(f"absence record for {module} does not carry a source-level proof: {record!r}")
    return record


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Prove an ExifTool module is absent from a release lib/.")
    parser.add_argument("--lib", required=True, type=Path)
    parser.add_argument("--module", required=True)
    parser.add_argument("--token", action="append", default=[])
    args = parser.parse_args(argv)
    try:
        record = prove_module_absent(args.lib, args.module, args.token)
    except (NotAbsent, ValueError, OSError) as error:
        print(f"module_absence: {error}", file=sys.stderr)
        return 1
    print(json.dumps(record, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
