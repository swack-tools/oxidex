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
import re
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




# The per-module files of the two split table artifacts (`binary/mod.rs` and
# `ifd/mod.rs` hubs, one `<stem>.rs` per ExifTool module beside each; the
# layout is tools/exiftool-tables/table_modules.py). Which modules exist is a
# property of the selected ExifTool release -- 11.78 has no DJI or InfiRay
# module, 13.59 has no JSON or Rsrc one -- so the member files are not listed
# here: they are exactly the stems the hub's `mod <stem>;` lines declare,
# read from the tree each time the inventory is used. A static 13.59 list
# made every other release's regeneration fail the write-set check (a missing
# `dji.rs`, an unexpected `json.rs`) until someone edited this file, which is
# an upgrade intervention, not a guarantee.
#
# The guarantee the static list gave is kept mechanically: `family_errors`
# rejects a hub that names a file which is not there, a `*.rs` beside the hub
# that it does not name (an orphan), and a stem list that is not the sorted,
# unique order `table_modules.render_hub` writes. `check` applies it to the
# regenerated tree, and a member may only appear or disappear together with
# its `mod` line.
MODULE_FAMILIES = ("binary", "ifd")
CONVERSION_REGISTRY_BEGIN = "// BEGIN GENERATED CONVERSION REGISTRY"
REPO_ROOT = Path(__file__).resolve().parents[2]
# HUB and table_modules.MOD_LINE_RE, restated so this file
# stays importable on its own (tests copy it into scratch repositories);
# test_artifacts.py pins the two copies equal.
HUB = "mod.rs"
MOD_LINE_RE = re.compile(r"^mod ([a-z0-9_]+);$", re.M)
_FAMILY_MEMBER = re.compile(r"src/exiftool_tables/(binary|ifd)/([a-z0-9_]+)\.rs")


def family_dir(kind):
    return f"src/exiftool_tables/{kind}"


def module_stems(kind, root=REPO_ROOT):
    """The module file stems the `kind` hub under `root` declares."""
    if kind not in MODULE_FAMILIES:
        raise ValueError(f"unknown split table artifact: {kind}")
    hub = Path(root) / family_dir(kind) / HUB
    if not hub.exists() and not hub.is_symlink():
        # The hub is itself a declared output, so its absence is reported
        # wherever outputs are required (`check`, `diff`, `family_errors`);
        # it declares no module files meanwhile.
        return ()
    if not hub.is_file() or hub.is_symlink():
        raise ValueError(f"nonregular/symlink split table hub: {family_dir(kind)}/{HUB}")
    return tuple(MOD_LINE_RE.findall(hub.read_text(encoding="utf-8")))


def family_errors(root=REPO_ROOT):
    """Why the split table directories under `root` are not a consistent
    generated set (empty when they are)."""
    errors = []
    for kind in MODULE_FAMILIES:
        if not (Path(root) / family_dir(kind) / HUB).is_file():
            errors.append(f"missing split table hub {family_dir(kind)}/{HUB}")
        stems = module_stems(kind, root)
        if list(stems) != sorted(set(stems)):
            errors.append(f"{family_dir(kind)}/{HUB} module lines are not sorted and unique")
        directory = Path(root) / family_dir(kind)
        on_disk = {p.stem for p in directory.glob("*.rs") if p.name != HUB}
        errors += [f"orphan module file {family_dir(kind)}/{stem}.rs (not declared by its hub)"
                   for stem in sorted(on_disk - set(stems))]
        errors += [f"declared module file {family_dir(kind)}/{stem}.rs is missing"
                   for stem in sorted(set(stems) - on_disk)]
    return errors


def is_family_member(path):
    """Whether `path` is where a split table artifact's module file lives."""
    match = _FAMILY_MEMBER.fullmatch(path)
    return bool(match) and match.group(2) + ".rs" != HUB


def _module_artifacts(kind, stems):
    return tuple(
        Artifact(f"{kind}-{stem}", 1, "codegen", f"{family_dir(kind)}/{stem}.rs")
        for stem in stems
    )

STATIC_ARTIFACTS = (
    Artifact("binary", 1, "codegen", "src/exiftool_tables/binary/mod.rs"),
    Artifact("ifd", 1, "codegen", "src/exiftool_tables/ifd/mod.rs"),
    Artifact("ifd-identity-ledger", 1, "codegen", "tools/exiftool-tables/ifd_identity_ledger.json"),
    Artifact("keyed", 1, "codegen", "src/exiftool_tables/keyed_tables.rs"),
    Artifact("garmin-fit", 1, "codegen", "src/exiftool_tables/fit_tables.rs"),
    Artifact("garmin-fit-ledger", 1, "codegen", "tools/exiftool-tables/garmin_fit_ledger.json"),
    Artifact("garmin-fit-source", 1, "garmin_fit_specs", "tools/exiftool-tables/fixtures/garmin_fit_source.json"),
    Artifact("serial", 1, "serial_directory", "src/exiftool_tables/serial_tables.rs"),
    Artifact("expr-ledger", 1, "verify_exprs", "tools/exiftool-tables/expr_oracle_ledger.json"),
    Artifact("value-ledger", 1, "codegen", "tools/exiftool-tables/value_conv_ledger.json"),
    Artifact("conv-exif-main", 1, "conv_codegen", "src/exiftool_tables/conv/exif_main.rs"),
    Artifact("conv-exif-main-ledger", 1, "conv_codegen", "tools/exiftool-tables/conv_exif_main_ledger.json"),
    Artifact("conv-exif-main-oracle", 1, "conv_oracle", "tools/exiftool-tables/testdata/conv_exif_main_outputs.json"),
    Artifact("scalar-helpers", 1, "scalar_helper_codegen", "src/writers/generated_scalar_rules.rs"),
    Artifact("scalar-helper-ledger", 1, "scalar_helper_codegen", "tools/exiftool-tables/scalar_helper_ledger.json"),
    Artifact("checkexif-rules", 1, "checkexif_rust_codegen", "src/writers/generated_checkexif_rules.rs"),
    Artifact("checkexif-ledger", 1, "checkexif_rust_codegen", "tools/exiftool-tables/checkexif_ledger.json"),
    Artifact("sanitize-rules", 1, "sanitize_rust_codegen", "src/writers/generated_sanitize_rules.rs"),
    Artifact("sanitize-ledger", 1, "sanitize_rust_codegen", "tools/exiftool-tables/sanitize_ledger.json"),
    Artifact("convinv-rules", 1, "convinv_rust_codegen", "src/writers/generated_convinv_rules.rs"),
    Artifact("convinv-ledger", 1, "convinv_rust_codegen", "tools/exiftool-tables/convinv_ledger.json"),
    Artifact("convinv-rows", 1, "convinv_row_codegen", "src/writers/generated_convinv_rows.rs"),
    Artifact("convinv-row-ledger", 1, "convinv_row_codegen", "tools/exiftool-tables/convinv_row_ledger.json"),
    Artifact("tiff-scalar-final-rules", 1, "final_scalar_stage", "src/writers/generated_tiff_scalar_final_rules.rs"),
    Artifact("tiff-scalar-final-ledger", 1, "final_scalar_stage", "tools/exiftool-tables/tiff_scalar_final_ledger.json"),
    Artifact("mandatory-default-rules", 1, "mandatory_defaults_codegen", "src/writers/generated_mandatory_defaults.rs"),
    Artifact("mandatory-default-ledger", 1, "mandatory_defaults_codegen", "tools/exiftool-tables/mandatory_defaults_ledger.json"),
    Artifact("raw-jfif-rules", 1, "raw_jfif_codegen", "src/writers/generated_raw_jfif.rs"),
    Artifact("raw-jfif-ledger", 1, "raw_jfif_codegen", "tools/exiftool-tables/raw_jfif_ledger.json"),
    Artifact("setnewvalue-address-rules", 1, "setnewvalue_address_rust_codegen", "src/writers/generated_setnewvalue_address_rules.rs"),
    Artifact("setnewvalue-address-ledger", 1, "setnewvalue_address_rust_codegen", "tools/exiftool-tables/setnewvalue_address_ledger.json"),
    Artifact("setnewvalue-ownership-ledger", 1, "setnewvalue_address_rust_codegen", "tools/exiftool-tables/setnewvalue_ownership_ledger.json"),
    Artifact("setnewvalue-public-migration-rules", 1, "setnewvalue_public_migration_ledger", "src/writers/generated_setnewvalue_public_migration_rules.rs"),
    Artifact("setnewvalue-public-migration-ledger", 1, "setnewvalue_public_migration_ledger", "tools/exiftool-tables/setnewvalue_public_migration_ledger.json"),
    Artifact("fresh-jpeg-byte-order-rules", 1, "fresh_jpeg_byte_order_codegen", "src/writers/generated_fresh_jpeg_byte_order.rs"),
    Artifact("fresh-jpeg-byte-order-ledger", 1, "fresh_jpeg_byte_order_codegen", "tools/exiftool-tables/fresh_jpeg_byte_order_ledger.json"),
    Artifact("filetypes", 1, "codegen_filetypes", "src/filetype/tables.rs"),
    Artifact("composite", 1, "codegen_composite", "src/composite/tables.rs"),
    Artifact("composite-compute", 1, "codegen_composite", "src/composite/generated_compute.rs"),
    Artifact("fits", 1, "codegen_fits", "src/parsers/specialized/fits/tables.rs"),
    Artifact("quicktime-itemlist-specs", 1, "quicktime_generated_specs", "src/parsers/quicktime/generated_itemlist_specs.rs"),
    Artifact("quicktime-itemlist-ledger", 1, "quicktime_generated_specs", "tools/exiftool-tables/quicktime_generated_itemlist_ledger.json"),
    Artifact("quicktime-userdata-specs", 1, "quicktime_userdata_specs", "src/parsers/quicktime/generated_userdata_specs.rs"),
    Artifact("quicktime-userdata-ledger", 1, "quicktime_userdata_specs", "tools/exiftool-tables/quicktime_generated_userdata_ledger.json"),
    Artifact("quicktime-keys-specs", 1, "quicktime_keys_specs", "src/parsers/quicktime/generated_keys_specs.rs"),
    Artifact("quicktime-keys-ledger", 1, "quicktime_keys_specs", "tools/exiftool-tables/quicktime_generated_keys_ledger.json"),
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
    Artifact("nikon-encrypted", 2, "gen_nikon_encrypted_tables", "src/parsers/tiff/makernotes/nikon/encrypted_tables.rs"),
    Artifact("sony-plain", 2, "gen_sony_plain_tables", "src/parsers/tiff/makernotes/sony/plain_tables.rs"),
    Artifact("mac-japanese", 2, "generate_charsets", "src/parsers/font/mac_charset/mac_japanese.rs"),
    Artifact("mac-chinese-tw", 2, "generate_charsets", "src/parsers/font/mac_charset/mac_chinese_tw.rs"),
    Artifact("mac-korean", 2, "generate_charsets", "src/parsers/font/mac_charset/mac_korean.rs"),
    Artifact("mac-chinese-cn", 2, "generate_charsets", "src/parsers/font/mac_charset/mac_chinese_cn.rs"),
    Artifact("geotiff", 2, "gen_geotiff_printconv", "src/parsers/tiff/geotiff_printconv.rs"),
    Artifact("dicom", 2, "gen_dicom_dict", "src/parsers/specialized/dicom_dict.rs"),
    Artifact("lens-alternatives", 2, "dump_lens_alternatives", "src/composite/lens_alternatives.rs"),
    Artifact("tag-exists", 2, "tag_exists_codegen", "src/writers/generated_tag_exists.rs"),
    Artifact("makernote-groups", 2, "makernote_groups_codegen", "src/writers/generated_makernote_groups.rs"),
)


def conversion_artifacts(root=REPO_ROOT):
    """Per-table outputs beyond the historical Exif::Main manifest rows."""
    hub = Path(root) / "src/exiftool_tables/conv/mod.rs"
    if (not hub.is_file()
            or CONVERSION_REGISTRY_BEGIN not in hub.read_text(encoding="utf-8")):
        # Older-release rehearsal fixtures have no generated registry yet.
        return ()
    # Keep this manifest importable on its own in scratch repositories.  The
    # registry generator is required only after its marker proves the checkout
    # has the generated registry whose entries need discovery.
    import conv_codegen

    entries = conv_codegen.discover_registry(root)
    extra = tuple(
        entry for entry in entries
        if entry.identity != conv_codegen.DEFAULT_TABLE
    )
    registry = (Artifact("conv-registry", 1, "conv_codegen",
                         "src/exiftool_tables/conv/mod.rs"),)
    return registry + tuple(
        artifact
        for entry in extra
        for artifact in (
            Artifact(f"conv-{entry.stem}", 1, "conv_codegen",
                     f"src/exiftool_tables/conv/{entry.stem}.rs"),
            Artifact(f"conv-{entry.stem}-ledger", 1, "conv_codegen",
                     f"tools/exiftool-tables/conv_{entry.stem}_ledger.json"),
            Artifact(f"conv-{entry.stem}-oracle", 1, "conv_oracle",
                     f"tools/exiftool-tables/testdata/conv_{entry.stem}_outputs.json"),
        )
    )


def inventory(root=REPO_ROOT):
    """Every declared output under `root`: the static entries, then each split
    table artifact's module files in its hub's order."""
    members = ()
    for kind in MODULE_FAMILIES:
        members += _module_artifacts(kind, module_stems(kind, root))
    return STATIC_ARTIFACTS + conversion_artifacts(root) + members


def __getattr__(name):
    # `ARTIFACTS` and the stem lists are read from this checkout's hubs on
    # every access, so a process that imported this module before a
    # regeneration (the rehearsal stage adapter) sees the regenerated set.
    if name == "ARTIFACTS":
        return inventory()
    if name == "BINARY_MODULE_STEMS":
        return module_stems("binary")
    if name == "IFD_MODULE_STEMS":
        return module_stems("ifd")
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


def validate(artifacts=None, root=REPO_ROOT):
    if artifacts is None:
        artifacts = inventory(root)
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


def select(tier="all", kind="all", producer=None, root=REPO_ROOT):
    items = inventory(root)
    validate(items)
    if str(tier) not in ("all", "1", "2") or kind not in ("all", "rust"):
        raise ValueError("invalid artifact selector")
    selected = [a for a in items if (str(tier) == "all" or a.tier == int(tier))
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
    # The split artifacts' member lists are release data, not manifest: a
    # regeneration that adds or drops a module must not read as a changed
    # manifest between `snapshot` and `check`.
    return digest(json.dumps({"static": [asdict(a) for a in STATIC_ARTIFACTS],
                              "families": [family_dir(kind) for kind in MODULE_FAMILIES]},
                             sort_keys=True).encode())


def checked_caches(root, caches):
    """Never let a cache exclusion hide tracked files or declared outputs."""
    tracked = [root / os.fsdecode(p) for p in git(root, "ls-files", "-z").split(b"\0") if p]
    protected = tracked + [root / a.path for a in inventory(root)]
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
    selected = select(tier, root=root)
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
    selected = select(saved["tier"], root=root)
    before = saved["before"]
    after = state(root, saved["caches"])
    changes = sorted(p for p in before["files"].keys() | after["files"].keys()
                     if before["files"].get(p) != after["files"].get(p))
    allowed = {a.path for a in selected}
    # A split artifact's module file may also appear or disappear, when the
    # selected tier writes that artifact; `family_errors` below then requires
    # the regenerated hub to name exactly the files that are there.
    writes_families = any(a.path == f"{family_dir(kind)}/{HUB}"
                          for a in selected for kind in MODULE_FAMILIES)
    unexpected = sorted(p for p in set(changes) - allowed
                        if not (writes_families and is_family_member(p)))
    invalid = [a.path for a in selected if not (root / a.path).is_file()
               or (root / a.path).resolve() != root / a.path]
    inconsistent = family_errors(root) if writes_families else []
    if unexpected or invalid or inconsistent or before["identity"] != after["identity"]:
        raise ValueError(f"regeneration write-set rejected: unexpected={unexpected}; "
                         f"missing/nonregular/symlink outputs={invalid}; "
                         f"inconsistent split tables={inconsistent}; "
                         f"HEAD/index changed={before['identity'] != after['identity']}")
    return changes


def diff_committed(root, tier):
    root = repository(root)
    paths = [a.path for a in select(tier, root=root)]
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
        validate(root=args.root)
        if args.command == "paths":
            for item in select(args.tier, args.kind, args.producer, root=args.root):
                print(args.root / item.path if args.absolute else item.path)
        elif args.command == "path":
            matches = [a for a in inventory(args.root) if a.key == args.key]
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
