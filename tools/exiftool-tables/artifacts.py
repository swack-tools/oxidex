#!/usr/bin/env python3
"""The output inventory for the two currently wired regeneration tiers.

This is not a catalog of every generated-looking file or a generator runner.
Producer order stays in regen*.sh. A snapshot/check pair observes final net
repository changes, including ignored source files, except Git metadata,
Cargo target directories, Python bytecode caches, and validated cache roots.
It cannot see transient writes, writes outside the repository, or writes in
those caches. It detects unexpected changes; it does not roll them back.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import subprocess
import sys


@dataclass(frozen=True)
class Artifact:
    key: str
    tier: int
    producer: str
    path: str
    mode: str = "whole"


ARTIFACTS = (
    Artifact("binary", 1, "codegen", "src/exiftool_tables/binary_tables.rs"),
    Artifact("ifd", 1, "codegen", "src/exiftool_tables/ifd_tables.rs"),
    Artifact("expr-ledger", 1, "verify_exprs", "tools/exiftool-tables/expr_oracle_ledger.json"),
    Artifact("value-ledger", 1, "codegen", "tools/exiftool-tables/value_conv_ledger.json"),
    Artifact("filetypes", 1, "codegen_filetypes", "src/filetype/tables.rs"),
    Artifact("composite", 1, "codegen_composite", "src/composite/tables.rs"),
    Artifact("composite-compute", 1, "codegen_composite", "src/composite/generated_compute.rs"),
    Artifact("fits", 1, "codegen_fits", "src/parsers/specialized/fits/tables.rs"),
    Artifact("fujifilm", 2, "codegen_subdirs", "src/parsers/tiff/makernotes/fujifilm/settings_tables.rs"),
    Artifact("panasonic", 2, "codegen_subdirs", "src/parsers/tiff/makernotes/panasonic/face_tables.rs"),
    Artifact("pentax", 2, "codegen_subdirs", "src/parsers/tiff/makernotes/pentax/subdir_tables.rs"),
    Artifact("af-points-json", 2, "dump_af_points", "tools/exiftool-tables/af_points.json"),
    Artifact("af-points", 2, "codegen_af_points", "src/parsers/tiff/makernotes/nikon/af_points.rs", "mixed"),
    Artifact("canon-custom", 2, "gen_canon_custom_functions2", "src/parsers/tiff/makernotes/canon/custom_functions2_tables.rs"),
    Artifact("infiray", 2, "gen_infiray_tables", "src/parsers/jpeg/app_segments/infiray_tables.rs"),
    Artifact("qualcomm", 2, "gen_qualcomm_tables", "src/parsers/jpeg/app_segments/qualcomm_tables.rs"),
    Artifact("samsung", 2, "gen_samsung_lookups", "src/parsers/tiff/makernotes/samsung/lookups.rs"),
    Artifact("olympus", 2, "gen_olympus_lookups", "src/parsers/tiff/makernotes/olympus/lookups.rs"),
    Artifact("leica", 2, "splice_leica", "src/parsers/tiff/makernotes/lens_data.rs", "mixed"),
    Artifact("sony-main", 2, "gen_sony_main_extra_tables", "src/parsers/tiff/makernotes/sony/main_extra_tables.rs"),
    Artifact("minolta-a100", 2, "gen_minolta_a100_tables", "src/parsers/tiff/makernotes/minolta_a100_tables.rs"),
    Artifact("nikon-settings", 2, "gen_nikon_settings_tables", "src/parsers/tiff/makernotes/nikon/settings_tables.rs"),
    Artifact("mac-japanese", 2, "generate_charsets", "src/parsers/font/mac_charset/mac_japanese.rs"),
    Artifact("mac-chinese-tw", 2, "generate_charsets", "src/parsers/font/mac_charset/mac_chinese_tw.rs"),
    Artifact("mac-korean", 2, "generate_charsets", "src/parsers/font/mac_charset/mac_korean.rs"),
    Artifact("mac-chinese-cn", 2, "generate_charsets", "src/parsers/font/mac_charset/mac_chinese_cn.rs"),
    Artifact("geotiff", 2, "gen_geotiff_printconv", "src/parsers/tiff/geotiff_printconv.rs"),
    Artifact("dicom", 2, "gen_dicom_dict", "src/parsers/specialized/dicom_dict.rs"),
    Artifact("lens-alternatives", 2, "dump_lens_alternatives", "src/composite/lens_alternatives.rs"),
)


def validate(artifacts=ARTIFACTS):
    keys, paths = set(), set()
    for item in artifacts:
        path = PurePosixPath(item.path)
        if (not item.key or item.key in keys or item.path in paths
                or path.is_absolute() or path.as_posix() != item.path
                or any(p in ("", ".", "..", ".git", "target", "__pycache__") for p in path.parts)
                or any(c.isspace() or ord(c) < 32 for c in item.path) or "\\" in item.path
                or item.tier not in (1, 2) or item.mode not in ("whole", "mixed")
                or not item.producer):
            raise ValueError(f"invalid or duplicate artifact: {item!r}")
        keys.add(item.key)
        paths.add(item.path)


def select(tier="all", kind="all", producer=None):
    validate()
    if str(tier) not in ("all", "1", "2") or kind not in ("all", "rust"):
        raise ValueError("invalid artifact selector")
    selected = [a for a in ARTIFACTS if (str(tier) == "all" or a.tier == int(tier))
                and (kind == "all" or a.path.endswith(".rs"))
                and (producer is None or a.producer == producer)]
    if not selected:
        raise ValueError("artifact selector matched no outputs")
    return selected


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def repository(root):
    root = Path(root).resolve()
    actual = Path(os.fsdecode(git(root, "rev-parse", "--show-toplevel")).strip()).resolve()
    if actual != root:
        raise ValueError(f"expected repository root, got {root}")
    return root


def digest(data):
    return hashlib.sha256(data).hexdigest()


def manifest_digest():
    return digest(json.dumps([asdict(a) for a in ARTIFACTS], sort_keys=True).encode())


def checked_caches(root, caches):
    """Never let a cache exclusion hide tracked files or declared outputs."""
    tracked = [root / os.fsdecode(p) for p in git(root, "ls-files", "-z").split(b"\0") if p]
    protected = tracked + [root / a.path for a in ARTIFACTS]
    result = []
    for raw in caches:
        path = Path(raw)
        path = (path if path.is_absolute() else root / path).resolve()
        if path == root or root.is_relative_to(path):
            raise ValueError(f"cache exclusion contains repository: {path}")
        if not path.is_relative_to(root):
            continue
        if any(p.is_relative_to(path) for p in protected):
            raise ValueError(f"cache exclusion overlaps tracked files or outputs: {path}")
        result.append(path.relative_to(root).as_posix())
    return sorted(set(result))


def state(root, caches):
    files = {}
    excluded = {root / p for p in checked_caches(root, caches)}
    # Read-only Git commands can refresh index stat data. Hash logical entries.
    identity = {"head": git(root, "rev-parse", "HEAD").decode().strip(),
                "index": digest(git(root, "ls-files", "--stage", "-z"))}
    for directory, dirs, names in os.walk(root, followlinks=False):
        directory = Path(directory)
        dirs[:] = sorted(d for d in dirs if d not in (".git", "target", "__pycache__")
                         and directory / d not in excluded)
        # Directory symlinks are entries, never traversal roots.
        links = [d for d in dirs if (directory / d).is_symlink()]
        dirs[:] = [d for d in dirs if d not in links]
        for name in sorted(names + links):
            path = directory / name
            if path == root / ".git" or path in excluded:
                continue
            info = path.lstat()
            mode = stat.S_IMODE(info.st_mode)
            if stat.S_ISLNK(info.st_mode):
                entry = ["symlink", mode, os.readlink(path)]
            elif stat.S_ISREG(info.st_mode):
                hasher = hashlib.sha256()
                with path.open("rb") as stream:
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                        hasher.update(chunk)
                entry = ["file", mode, hasher.hexdigest()]
            else:
                raise ValueError(f"unsupported repository entry: {path}")
            files[path.relative_to(root).as_posix()] = entry
    # Conventional cache names must not exempt a tracked file either.
    tracked = {os.fsdecode(p) for p in git(root, "ls-files", "-z").split(b"\0") if p}
    hidden = [p for p in tracked if any(c in ("target", "__pycache__", ".git") for c in PurePosixPath(p).parts)]
    if hidden:
        raise ValueError(f"cache exclusion overlaps tracked files: {hidden}")
    return {"identity": identity, "files": files}


def snapshot(root, tier, caches):
    root = repository(root)
    selected = select(tier)
    for item in selected:
        path = root / item.path
        if path.resolve() != path:
            raise ValueError(f"artifact uses a symlink: {item.path}")
    caches = checked_caches(root, caches)
    return {"schema": 1, "root": str(root), "tier": str(tier),
            "manifest": manifest_digest(), "caches": caches,
            "before": state(root, caches)}


def check(root, saved):
    root = repository(root)
    if (saved.get("schema") != 1 or saved.get("root") != str(root)
            or saved.get("manifest") != manifest_digest()):
        raise ValueError("snapshot schema, root, or manifest changed")
    selected = select(saved["tier"])
    before = saved["before"]
    after = state(root, saved["caches"])
    changes = sorted(p for p in before["files"].keys() | after["files"].keys()
                     if before["files"].get(p) != after["files"].get(p))
    allowed = {a.path for a in selected}
    unexpected = sorted(set(changes) - allowed)
    invalid = [a.path for a in selected if not (root / a.path).is_file()
               or (root / a.path).resolve() != root / a.path]
    if unexpected or invalid or before["identity"] != after["identity"]:
        raise ValueError(f"regeneration write-set rejected: unexpected={unexpected}; "
                         f"missing/nonregular/symlink outputs={invalid}; "
                         f"HEAD/index changed={before['identity'] != after['identity']}")
    return changes


def diff_committed(root, tier):
    root = repository(root)
    paths = [a.path for a in select(tier)]
    for path in paths:
        # git diff alone ignores a new output with no committed counterpart.
        git(root, "cat-file", "-e", f"HEAD:{path}")
        if not (root / path).is_file() or (root / path).resolve() != root / path:
            raise ValueError(f"missing/nonregular/symlink output: {path}")
    return subprocess.call(["git", "-C", str(root), "diff", "HEAD", "--exit-code", "--", *paths])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=Path(__file__).resolve().parents[2], type=Path)
    commands = parser.add_subparsers(dest="command", required=True)
    paths = commands.add_parser("paths")
    paths.add_argument("--tier", choices=("all", "1", "2"), default="all")
    paths.add_argument("--kind", choices=("all", "rust"), default="all")
    paths.add_argument("--producer")
    paths.add_argument("--absolute", action="store_true")
    path = commands.add_parser("path")
    path.add_argument("key")
    snap = commands.add_parser("snapshot")
    snap.add_argument("--tier", choices=("all", "1", "2"), required=True)
    snap.add_argument("--cache", action="append", default=[])
    snap.add_argument("--output", type=Path, required=True)
    verify = commands.add_parser("check")
    verify.add_argument("snapshot", type=Path)
    diff = commands.add_parser("diff")
    diff.add_argument("--tier", choices=("all", "1", "2"), default="all")
    args = parser.parse_args()
    args.root = args.root.resolve()
    try:
        validate()
        if args.command == "paths":
            for item in select(args.tier, args.kind, args.producer):
                print(args.root / item.path if args.absolute else item.path)
        elif args.command == "path":
            matches = [a for a in ARTIFACTS if a.key == args.key]
            if len(matches) != 1:
                raise ValueError(f"unknown artifact key: {args.key}")
            print(args.root / matches[0].path)
        elif args.command == "snapshot":
            if args.output.resolve().is_relative_to(args.root.resolve()):
                raise ValueError("write-set snapshot must be outside the repository")
            args.output.write_text(json.dumps(snapshot(args.root, args.tier, args.cache), sort_keys=True) + "\n")
        elif args.command == "diff":
            return diff_committed(args.root, args.tier)
        else:
            changes = check(args.root, json.loads(args.snapshot.read_text()))
            print(f">> regeneration write-set PASS: {len(changes)} declared net changes")
            for path in changes:
                print(f"   {path}")
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(f"artifacts: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
