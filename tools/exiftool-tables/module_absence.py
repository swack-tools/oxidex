#!/usr/bin/env python3
"""Prove that an ExifTool module, or a named table, is absent from one release.

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

The same rules work one level down, for a named table inside a module that
exists (11.78's Canon.pm has no RFLensType; its Nikon.pm has no afPoints105).
``prove_table_absent`` requires the module file to be present, and then the
table's name plus the caller's tokens must occur nowhere in the tree, the
module itself included. A name that still occurs in the module means the
table is present, perhaps in a changed shape. A name that occurs only in
another file means the table moved. Both are refusals, never absences. The
record also binds the module file's sha256.

A present module or table is not this helper's business. Callers check for it
first and use their normal, strict loader when it is there, so something that
is present but broken or changed still refuses in that loader.

CLI (for Perl call sites)::

    module_absence.py --lib LIB --module InfiRay --token IJPEG --token HasIJPEG
    module_absence.py --lib LIB --module Canon --table RFLensType

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
TABLE_SCHEMA = "exiftool_table_absent_v1"
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


def _complete_tree(lib: Path) -> None:
    anchor = lib / "Image" / "ExifTool.pm"
    try:
        if not stat.S_ISREG(os.stat(anchor).st_mode):
            raise NotAbsent(f"{anchor} is not a regular file, so {lib} is not a release lib/ tree")
    except OSError as error:
        raise NotAbsent(f"{lib} is not a complete release lib/ tree: {anchor}: {error.strerror}") from error


def _module_file_present(lib: Path, module_file: str) -> bool:
    """True if present; False only when stat fails with ENOENT."""
    try:
        os.stat(lib / module_file)
    except OSError as error:
        if error.errno != errno.ENOENT:
            raise NotAbsent(f"cannot stat {module_file} in {lib}: {error.strerror}") from error
        return False
    return True


def _scan(lib: Path, names: list[str], what: str, module_file: str) -> dict:
    """Scan every regular file of ``lib``; refuse if any name occurs."""
    patterns = {name: _pattern(name) for name in names}
    occurrences = {name: 0 for name in names}
    hits: list[str] = []
    inventory = hashlib.sha256()
    digests: dict[str, str] = {}
    label = None
    files = _files(lib)
    for path in files:
        rel = path.relative_to(lib).as_posix()
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        inventory.update(f"{rel}\0{digest}\n".encode("utf-8", "surrogateescape"))
        if rel in ("Image/ExifTool.pm", module_file):
            digests[rel] = digest
        if rel == "Image/ExifTool.pm":
            found = _LABEL.search(data)
            label = found.group(1).decode("latin-1") if found else None
        for name, pattern in patterns.items():
            count = len(pattern.findall(data))
            if count:
                occurrences[name] += count
                hits.append(f"{rel}:{name}x{count}")
    if "Image/ExifTool.pm" not in digests:
        raise NotAbsent(f"{lib}/Image/ExifTool.pm was not scanned")
    if hits:
        listed = ", ".join(hits)
        if what == "module":
            raise NotAbsent(f"{module_file} is absent but referenced (broken, partial or moved): {listed}")
        if module_file in {h.split(":", 1)[0] for h in hits}:
            raise NotAbsent(f"table {names[0]} is present in {module_file} (possibly in a changed shape): {listed}")
        raise NotAbsent(f"table {names[0]} moved: named outside {module_file}: {listed}")
    return {
        "tokens": names,
        "occurrences_in_release": occurrences,
        "release_files_scanned": len(files),
        "release_pm_files_scanned": sum(1 for p in files if p.suffix == ".pm"),
        "release_inventory_sha256": inventory.hexdigest(),
        "exiftool_pm_sha256": digests["Image/ExifTool.pm"],
        "module_sha256": digests.get(module_file),
        "release_label_not_used_as_proof": label,
    }


def prove_module_absent(lib: os.PathLike | str, module: str, tokens: Iterable[str] = ()) -> dict:
    """Return the absence record for ``module`` in ``lib``, or raise NotAbsent."""
    names = _tokens(module, tokens)
    lib = Path(lib).resolve()
    _complete_tree(lib)
    module_file = f"Image/ExifTool/{module}.pm"
    if _module_file_present(lib, module_file):
        raise NotAbsent(f"present: {module_file} exists in {lib}")
    facts = _scan(lib, names, "module", module_file)
    facts.pop("module_sha256")
    return {"kind": SCHEMA, "module": module, "module_file": module_file,
            "module_file_present": False, **facts}


def prove_table_absent(lib: os.PathLike | str, module: str, table: str, tokens: Iterable[str] = ()) -> dict:
    """Return the absence record for ``table`` of a present ``module``, or raise NotAbsent."""
    _tokens(module, ())
    names = _tokens(table, tokens)
    lib = Path(lib).resolve()
    _complete_tree(lib)
    module_file = f"Image/ExifTool/{module}.pm"
    if not _module_file_present(lib, module_file):
        raise NotAbsent(f"module absent: {module_file} does not exist in {lib}; prove the module absent instead")
    facts = _scan(lib, names, "table", module_file)
    if facts["module_sha256"] is None:
        raise NotAbsent(f"{module_file} is not a regular file in {lib}")
    return {"kind": TABLE_SCHEMA, "module": module, "module_file": module_file,
            "module_file_present": True, "table": table, **facts}


def validate_absence_record(record: object, module: str, table: str | None = None) -> dict:
    """Refuse a record that does not carry a complete source-level proof.

    ``table`` None validates a module absence, otherwise a table absence.
    """
    subject = module if table is None else f"{module} table {table}"
    if not isinstance(record, dict):
        raise NotAbsent(f"absence record for {subject} is not an object")
    occurrences = record.get("occurrences_in_release")
    tokens = record.get("tokens")
    if table is None:
        shape = (record.get("kind") == SCHEMA and record.get("module_file_present") is False
                 and "table" not in record and "module_sha256" not in record)
    else:
        shape = (record.get("kind") == TABLE_SCHEMA and record.get("module_file_present") is True
                 and record.get("table") == table
                 and isinstance(record.get("module_sha256"), str) and bool(_SHA256.match(record["module_sha256"])))
    ok = (
        shape
        and record.get("module") == module
        and record.get("module_file") == f"Image/ExifTool/{module}.pm"
        and isinstance(tokens, list) and tokens and tokens[0] == (module if table is None else table)
        and isinstance(occurrences, dict) and set(occurrences) == set(tokens)
        and all(type(v) is int and v == 0 for v in occurrences.values())
        and type(record.get("release_files_scanned")) is int and record["release_files_scanned"] >= 1
        and type(record.get("release_pm_files_scanned")) is int and record["release_pm_files_scanned"] >= 1
        and all(isinstance(record.get(k), str) and _SHA256.match(record[k])
                for k in ("release_inventory_sha256", "exiftool_pm_sha256"))
    )
    if not ok:
        raise NotAbsent(f"absence record for {subject} does not carry a source-level proof: {record!r}")
    return record


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Prove an ExifTool module or table is absent from a release lib/.")
    parser.add_argument("--lib", required=True, type=Path)
    parser.add_argument("--module", required=True)
    parser.add_argument("--table", help="prove this named table absent from a present module")
    parser.add_argument("--token", action="append", default=[])
    args = parser.parse_args(argv)
    try:
        if args.table is None:
            record = prove_module_absent(args.lib, args.module, args.token)
        else:
            record = prove_table_absent(args.lib, args.module, args.table, args.token)
    except (NotAbsent, ValueError, OSError) as error:
        print(f"module_absence: {error}", file=sys.stderr)
        return 1
    print(json.dumps(record, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
