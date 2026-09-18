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


# ---------------------------------------------------------------------------
# Entry level: one tag id absent from a named table of a present module.
#
# The third granularity. 11.78's Sony::Main has no 0x2032..0x2039, 0x204a or
# 0x205c; 12.64's has no 0x204a or 0x205c. The module and the table both exist,
# so neither proof above applies, and the id is a number, not a name, so the
# token scan cannot express it. ``prove_entry_absent`` proves it two ways, and
# both must agree:
#
# * Loaded: the release's own perl loads the release's own lib/ (config file
#   disabled, ``%INC`` checked to resolve inside ``lib``), the table hash is
#   defined and has at least one tag id, and the id is not a key of it. This is
#   the hash ExifTool dispatches on; source text alone is not.
# * Source: the module file declares ``%Image::ExifTool::<Module>::<Table> = (``
#   and the id occurs nowhere in the module file, in any spelling (hex with any
#   case and leading zeros, or decimal), not even in a comment. An id still
#   named in the module is present, perhaps in a changed shape: refuse.
#
# A missing module or a missing table is a refusal naming the coarser proof,
# never an entry absence. An id present in the loaded table is a refusal: the
# caller's strict path owns present entries, changed or not. The record binds
# the ExifTool.pm and module sha256s, the tree inventory, and a sha256 over the
# loaded table's sorted tag ids, so a consumer can check its own dump is the
# same table. The release label is recorded and never used.
# ---------------------------------------------------------------------------
ENTRY_SCHEMA = "exiftool_entry_absent_v1"

_ENTRY_PERL = r"""
use strict; use warnings;
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
my ($lib, $module, $table, $id) = @ARGV;
unshift @INC, $lib;
require Image::ExifTool;
my $file = "Image/ExifTool/$module.pm";
require $file;
no strict 'refs';
my $defined = defined(*{"Image::ExifTool::${module}::${table}"}{HASH}) ? 1 : 0;
my $hash = $defined ? \%{"Image::ExifTool::${module}::${table}"} : {};
my @ids;
my $present = 0;
for my $key (keys %$hash) {
    my $num;
    if ($key =~ /^[0-9]+\z/) { $num = $key + 0 }
    elsif ($key =~ /^0x([0-9a-fA-F]+)\z/) { $num = hex $1 }
    else { next }
    push @ids, $num;
    $present = 1 if $num == $id;
}
@ids = sort { $a <=> $b } @ids;
my $out = join "\n",
    "perl_version\t$^V",
    "core_inc\t$INC{'Image/ExifTool.pm'}",
    "module_inc\t$INC{$file}",
    "table_defined\t$defined",
    "id_present\t$present",
    "tag_ids\t" . join(',', @ids);
print "$out\n";
"""


def _id_patterns(tag_id: int) -> list[re.Pattern[bytes]]:
    hexits = f"{tag_id:x}".encode("ascii")
    return [
        re.compile(rb"(?<![A-Za-z0-9_])0[xX]0*" + hexits + rb"(?![A-Za-z0-9_])", re.I),
        re.compile(rb"(?<![A-Za-z0-9_.])" + str(tag_id).encode("ascii") + rb"(?![A-Za-z0-9_.])"),
    ]


def _loaded_table(perl: str, lib: Path, module: str, table: str, tag_id: int) -> dict:
    import subprocess

    env = {k: v for k, v in os.environ.items() if k not in ("PERL5LIB", "PERLLIB", "PERL5OPT")}
    try:
        done = subprocess.run([perl, "-e", _ENTRY_PERL, str(lib), module, table, str(tag_id)],
                              capture_output=True, env=env, timeout=300, check=False)
    except OSError as error:
        raise NotAbsent(f"cannot run {perl} to load {module}::{table}: {error.strerror}") from error
    if done.returncode != 0:
        raise NotAbsent(f"{perl} could not load {module} from {lib} (exit {done.returncode}): "
                        f"{done.stderr.decode('utf-8', 'replace').strip()[:400]}")
    facts = dict(line.split("\t", 1) for line in done.stdout.decode("utf-8").splitlines() if "\t" in line)
    if set(facts) != {"perl_version", "core_inc", "module_inc", "table_defined", "id_present", "tag_ids"}:
        raise NotAbsent(f"{perl} printed an incomplete loaded-table report: {done.stdout[:400]!r}")
    for key, rel in (("core_inc", "Image/ExifTool.pm"), ("module_inc", f"Image/ExifTool/{module}.pm")):
        if Path(facts[key]).resolve() != (lib / rel).resolve():
            raise NotAbsent(f"{rel} loaded from {facts[key]}, not from {lib}")
    ids = [int(x) for x in facts["tag_ids"].split(",") if x]
    return {"perl_version": facts["perl_version"], "table_defined": facts["table_defined"] == "1",
            "id_present": facts["id_present"] == "1", "tag_ids": ids}


def loaded_tag_ids_sha256(tag_ids: Iterable[int]) -> str:
    """The digest an entry record binds: sorted distinct decimal ids, one per line."""
    return hashlib.sha256("".join(f"{i}\n" for i in sorted(set(tag_ids))).encode("ascii")).hexdigest()


def prove_entry_absent(lib: os.PathLike | str, module: str, table: str, tag_id: int, perl: str) -> dict:
    """Return the absence record for ``tag_id`` in ``module``'s present ``table``, or raise NotAbsent."""
    _tokens(module, ())
    _tokens(table, ())
    if type(tag_id) is not int or not 0 <= tag_id <= 0xFFFFFFFF:
        raise ValueError(f"tag id must be a non-negative int, got {tag_id!r}")
    lib = Path(lib).resolve()
    _complete_tree(lib)
    module_file = f"Image/ExifTool/{module}.pm"
    if not _module_file_present(lib, module_file):
        raise NotAbsent(f"module absent: {module_file} does not exist in {lib}; prove the module absent instead")
    if not stat.S_ISREG(os.stat(lib / module_file).st_mode):
        raise NotAbsent(f"{module_file} is not a regular file in {lib}")
    source = (lib / module_file).read_bytes()
    declaration = re.compile(rb"^%Image::ExifTool::" + re.escape(module.encode()) + rb"::"
                             + re.escape(table.encode()) + rb"\s*=\s*\(", re.M)
    declarations = len(declaration.findall(source))
    if declarations != 1:
        raise NotAbsent(f"table absent or ambiguous: {module_file} declares %Image::ExifTool::{module}::{table} "
                        f"{declarations} times; prove the table absent instead")
    hits = sum(len(p.findall(source)) for p in _id_patterns(tag_id))
    if hits:
        raise NotAbsent(f"id 0x{tag_id:x} is present in {module_file} ({hits} occurrences, possibly in a "
                        f"changed shape); an entry absence needs it named nowhere")
    loaded = _loaded_table(perl, lib, module, table, tag_id)
    if not loaded["table_defined"] or not loaded["tag_ids"]:
        raise NotAbsent(f"table absent: %Image::ExifTool::{module}::{table} is not defined or has no tag ids "
                        f"once loaded from {lib}; prove the table absent instead")
    if loaded["id_present"]:
        raise NotAbsent(f"id 0x{tag_id:x} is present in the loaded %Image::ExifTool::{module}::{table} "
                        f"(the source text does not name it, so it is built at load time)")
    files = _files(lib)
    inventory = hashlib.sha256()
    label = None
    exiftool_pm_sha256 = None
    for path in files:
        rel = path.relative_to(lib).as_posix()
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        inventory.update(f"{rel}\0{digest}\n".encode("utf-8", "surrogateescape"))
        if rel == "Image/ExifTool.pm":
            exiftool_pm_sha256 = digest
            found = _LABEL.search(data)
            label = found.group(1).decode("latin-1") if found else None
    if exiftool_pm_sha256 is None:
        raise NotAbsent(f"{lib}/Image/ExifTool.pm was not scanned")
    return {
        "kind": ENTRY_SCHEMA, "module": module, "module_file": module_file, "module_file_present": True,
        "table": table, "table_declarations_in_module": declarations,
        "tag_id": tag_id, "tag_id_hex": f"0x{tag_id:x}",
        "id_occurrences_in_module": 0,
        "id_in_loaded_table": False,
        "loaded_table_tag_ids": len(set(loaded["tag_ids"])),
        "loaded_table_tag_ids_sha256": loaded_tag_ids_sha256(loaded["tag_ids"]),
        "perl": perl, "perl_version": loaded["perl_version"],
        "release_files_scanned": len(files),
        "release_pm_files_scanned": sum(1 for p in files if p.suffix == ".pm"),
        "release_inventory_sha256": inventory.hexdigest(),
        "exiftool_pm_sha256": exiftool_pm_sha256,
        "module_sha256": hashlib.sha256(source).hexdigest(),
        "release_label_not_used_as_proof": label,
    }


def validate_entry_absence_record(record: object, module: str, table: str, tag_id: int) -> dict:
    """Refuse an entry record that does not carry both halves of the proof."""
    subject = f"{module} table {table} id {tag_id!r}"
    ok = (
        isinstance(record, dict)
        and record.get("kind") == ENTRY_SCHEMA
        and record.get("module") == module
        and record.get("module_file") == f"Image/ExifTool/{module}.pm"
        and record.get("module_file_present") is True
        and record.get("table") == table
        and record.get("table_declarations_in_module") == 1
        and type(tag_id) is int and type(record.get("tag_id")) is int and record["tag_id"] == tag_id
        and record.get("tag_id_hex") == f"0x{tag_id:x}"
        and type(record.get("id_occurrences_in_module")) is int and record["id_occurrences_in_module"] == 0
        and record.get("id_in_loaded_table") is False
        and type(record.get("loaded_table_tag_ids")) is int and record["loaded_table_tag_ids"] >= 1
        and type(record.get("release_files_scanned")) is int and record["release_files_scanned"] >= 1
        and type(record.get("release_pm_files_scanned")) is int and record["release_pm_files_scanned"] >= 1
        and all(isinstance(record.get(k), str) and _SHA256.match(record[k])
                for k in ("release_inventory_sha256", "exiftool_pm_sha256", "module_sha256",
                          "loaded_table_tag_ids_sha256"))
    )
    if not ok:
        raise NotAbsent(f"absence record for {subject} does not carry a loaded and source-level proof: {record!r}")
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
