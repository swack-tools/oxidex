#!/usr/bin/env python3
"""Produce, but never approve, a complete Linux Perl installation candidate.

The public Just recipe uses the ordinary Spot build client.  The worker runs
only in its fresh /target project; no qualification launcher or shared oracle
cache is involved.  The retained tar and receipt require independent review.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import importlib.util
import io
import json
import os
import platform
import re
import stat
import subprocess
import sys
import tarfile
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parents[2]
OPS = Path("/target/ops")
MAX_MEMBERS = 100_000
MAX_BYTES = 2 * 1024**3
MAX_LINK_HOPS = 40
HEX40 = re.compile(r"[0-9a-f]{40}\Z")


class Refused(RuntimeError):
    """A candidate cannot be safely produced or inspected."""


def _git(*args: str) -> str:
    return subprocess.check_output(["git", "-C", str(ROOT), *args], text=True).strip()


def launch() -> None:
    if Path.cwd().resolve() != ROOT or _git("rev-parse", "--show-toplevel") != str(ROOT):
        raise Refused("launch from the producer checkout root")
    if _git("status", "--porcelain", "--untracked-files=all"):
        raise Refused("producer checkout must be clean exact HEAD")
    head = _git("rev-parse", "HEAD")
    tree = _git("rev-parse", "HEAD^{tree}")
    if not HEX40.fullmatch(head) or not HEX40.fullmatch(tree):
        raise Refused("invalid Git source identity")
    subprocess.run(["git", "-C", str(ROOT), "verify-commit", head], check=True)
    identity = _git("log", "-1", "--format=%an|%ae|%cn|%ce|%G?|%GS", head)
    expected = "swackhamer|swackhamer@users.noreply.github.com|swackhamer|swackhamer@users.noreply.github.com|G|swackhamer@users.noreply.github.com"
    if identity != expected:
        raise Refused("producer HEAD is not the verified signed maintainer commit")
    subprocess.run([
        sys.executable, str(ROOT / "tools/remote-build/build.py"),
        "--just-recipe", "freeze-linux-perl-worker",
        "--just-arg=" + head, "--just-arg=" + tree,
        "--max-attempts", "1",
    ], check=True)


def _safe_name(name: str) -> PurePosixPath:
    if not name or name.startswith("/") or "\\" in name or "\x00" in name:
        raise Refused(f"unsafe tar member name: {name!r}")
    parts = name.split("/")
    if any(part in ("", ".", "..") for part in parts):
        raise Refused(f"ambiguous tar member name: {name!r}")
    return PurePosixPath(name)


def _safe_link(name: PurePosixPath, target: str) -> None:
    if not target or target.startswith("/") or "\\" in target or "\x00" in target:
        raise Refused(f"unsafe symlink target: {name} -> {target!r}")
    parts = list(name.parent.parts)
    for part in target.split("/"):
        if part == "..":
            if not parts:
                raise Refused(f"symlink escapes Perl tree: {name} -> {target}")
            parts.pop()
        elif part not in ("", "."):
            parts.append(part)


def _validate_link_graph(entries: dict[str, str | None]) -> None:
    """Resolve every link through the complete tree, including link/../ chains."""
    available = {""}
    for name in entries:
        available.add(name)
        available.update(str(parent) for parent in PurePosixPath(name).parents if str(parent) != ".")
    for name, target in entries.items():
        if target is None:
            continue
        pending = name.split("/")
        resolved: list[str] = []
        hops = 0
        while pending:
            part = pending.pop(0)
            if part in ("", "."):
                continue
            if part == "..":
                if not resolved:
                    raise Refused(f"symlink escapes Perl tree: {name}")
                resolved.pop()
                continue
            resolved.append(part)
            current = "/".join(resolved)
            link = entries.get(current)
            if link is not None:
                hops += 1
                if hops > MAX_LINK_HOPS:
                    raise Refused(f"symlink cycle or excessive chain: {name}")
                resolved.pop()
                pending = link.split("/") + pending
        if "/".join(resolved) not in available:
            raise Refused(f"dangling symlink in Perl tree: {name}")


def _members(source: Path) -> list[tuple[Path, str, os.stat_result]]:
    if source.is_symlink() or not source.is_dir():
        raise Refused("Perl prefix must be a real directory")
    result = []
    links: dict[str, str | None] = {}
    total = 0
    canonical = source.resolve(strict=True)
    for path in sorted(source.rglob("*"), key=lambda p: p.relative_to(source).as_posix()):
        name = path.relative_to(source).as_posix()
        relative = _safe_name(name)
        info = path.lstat()
        if stat.S_ISLNK(info.st_mode):
            target = os.readlink(path)
            _safe_link(relative, target)
            try:
                resolved = path.resolve(strict=True)
            except (OSError, RuntimeError) as exc:
                raise Refused(f"dangling or cyclic Perl tree symlink: {name}") from exc
            if not resolved.is_relative_to(canonical):
                raise Refused(f"symlink escapes Perl tree: {name}")
            links[name] = target
        elif stat.S_ISREG(info.st_mode):
            total += info.st_size
            links[name] = None
        elif not stat.S_ISDIR(info.st_mode):
            raise Refused(f"unsupported Perl tree object: {name}")
        else:
            links[name] = None
        result.append((path, name, info))
        if len(result) > MAX_MEMBERS or total > MAX_BYTES:
            raise Refused("Perl tree exceeds bounded archive limits")
    if not result:
        raise Refused("Perl prefix is empty")
    _validate_link_graph(links)
    return result


def freeze_tree(source: Path, archive: Path) -> None:
    """Write one deterministic tar.gz; no output is opened before the tree scan."""
    entries = _members(source)
    if archive.exists() or archive.is_symlink():
        raise Refused(f"candidate archive already exists: {archive}")
    archive.parent.mkdir(parents=True, exist_ok=True)
    try:
        with archive.open("xb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode="w", format=tarfile.PAX_FORMAT) as tar:
                for path, name, before in entries:
                    current = path.lstat()
                    if (current.st_mode, current.st_size, current.st_mtime_ns) != (before.st_mode, before.st_size, before.st_mtime_ns):
                        raise Refused(f"Perl tree changed during freeze: {name}")
                    member = tarfile.TarInfo(name)
                    member.mode = stat.S_IMODE(before.st_mode)
                    member.uid = member.gid = member.mtime = 0
                    member.uname = member.gname = ""
                    if stat.S_ISDIR(before.st_mode):
                        member.type = tarfile.DIRTYPE
                    elif stat.S_ISLNK(before.st_mode):
                        member.type = tarfile.SYMTYPE
                        member.linkname = os.readlink(path)
                    else:
                        member.type = tarfile.REGTYPE
                        member.size = before.st_size
                    if member.isfile():
                        with path.open("rb") as body:
                            tar.addfile(member, body)
                    else:
                        tar.addfile(member)
    except Exception:
        archive.unlink(missing_ok=True)
        raise


def inspect_archive(archive: Path) -> list[tarfile.TarInfo]:
    """Reject unsafe/ambiguous tar input before any member is extracted."""
    if archive.is_symlink() or not archive.is_file():
        raise Refused("frozen archive is not a regular file")
    if archive.stat().st_size > MAX_BYTES:
        raise Refused("frozen archive exceeds bounded limits")
    members: list[tarfile.TarInfo] = []
    seen: dict[str, tarfile.TarInfo] = {}
    links: dict[str, str | None] = {}
    total = 0
    with tarfile.open(archive, "r|gz") as tar:
        for member in tar:
            if len(members) >= MAX_MEMBERS:
                raise Refused("frozen archive exceeds bounded member count")
            name = _safe_name(member.name)
            if member.name in seen:
                raise Refused(f"duplicate tar member: {member.name}")
            if member.isdir():
                links[member.name] = None
            elif member.issym():
                _safe_link(name, member.linkname)
                links[member.name] = member.linkname
            elif member.isfile():
                if member.size < 0:
                    raise Refused(f"negative tar member size: {member.name}")
                total += member.size
                links[member.name] = None
            else:
                raise Refused(f"unsupported tar member type: {member.name}")
            if member.mode & ~0o777 or member.mode & 0o6000 or total > MAX_BYTES:
                raise Refused(f"unsafe tar member mode or size: {member.name}")
            seen[member.name] = member
            members.append(member)
    if not members:
        raise Refused("frozen archive is empty")
    for name, member in seen.items():
        for parent in PurePosixPath(name).parents:
            if str(parent) == ".":
                break
            prior = seen.get(str(parent))
            if prior is not None and not prior.isdir():
                raise Refused(f"tar member has non-directory ancestor: {name}")
    _validate_link_graph(links)
    return members


def extract_frozen(archive: Path, destination: Path) -> None:
    members = inspect_archive(archive)
    if destination.exists() or destination.is_symlink():
        raise Refused("frozen extraction destination must be absent")
    destination.mkdir(parents=True)
    with tarfile.open(archive, "r:gz") as tar:
        for member in members:
            path = destination / member.name
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                path.parent.mkdir(parents=True, exist_ok=True)
                body = tar.extractfile(member)
                if body is None:
                    raise Refused(f"missing tar member body: {member.name}")
                with path.open("xb") as output:
                    while chunk := body.read(1024 * 1024):
                        output.write(chunk)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.symlink_to(member.linkname)
            if member.isfile():
                path.chmod(member.mode)
        # Apply directory modes after children exist; a read-only parent must
        # not turn an otherwise valid archive into a partial extraction.
        for member in reversed(members):
            if member.isdir():
                (destination / member.name).chmod(member.mode)


def _load_oracle():
    module = ROOT / "tools/release/bootstrap_oracle.py"
    spec = importlib.util.spec_from_file_location("bootstrap_oracle", module)
    assert spec and spec.loader
    oracle = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(oracle)
    return oracle


def _command(argv: list[str]) -> str:
    result = subprocess.run(argv, capture_output=True, text=True, check=False)
    if result.returncode:
        raise Refused(f"{argv[0]} failed ({result.returncode}): {result.stderr.strip() or result.stdout.strip()}")
    return result.stdout.strip()


def worker(head: str, tree: str) -> None:
    if not HEX40.fullmatch(head) or not HEX40.fullmatch(tree):
        raise Refused("invalid source identity arguments")
    if platform.system() != "Linux" or platform.machine() not in ("x86_64", "AMD64"):
        raise Refused("producer requires Linux x86_64")
    if Path.cwd() != Path("/src"):
        raise Refused("producer requires mounted /src")
    if OPS.exists() or OPS.is_symlink():
        raise Refused("producer requires fresh owned /target/ops")
    if not Path("/target").is_dir() or Path("/target").is_symlink():
        raise Refused("producer requires owned /target mount")
    os.environ["OXIDEX_OPS_DIR"] = str(OPS)
    oracle = _load_oracle()
    oracle.validate_lock(oracle.LOCK)
    OPS.mkdir(mode=0o700)
    lock = oracle.LOCK
    # Fetch only the two authenticated source archives.  Never call materialize(),
    # which also pulls ExifTool and the full corpus.
    archives = {
        name: oracle.download_locked(OPS, name, lock["archives"][name])
        for name in ("perl", "archive_zip")
    }
    for name, path in archives.items():
        if oracle.sha256_file(path) != lock["archives"][name]["sha256"]:
            raise Refused(f"locked {name} source changed after download")
    prefix = oracle.perl_prefix(OPS)
    if prefix.exists() or prefix.is_symlink() or oracle.manifest_path(OPS).exists():
        raise Refused("producer refuses an existing Perl installation or manifest")
    if list(prefix.parent.glob(f".{prefix.name}.staging-*")):
        raise Refused("producer refuses an existing Perl staging tree")
    oracle._materialize_perl(OPS, archives["perl"], archives["archive_zip"])
    oracle._verify_perl_tree(OPS, prefix)
    perl = oracle.perl_path(OPS)
    zip_module = oracle.archive_zip_library_path(OPS)
    before = oracle.sha256_tree(prefix)
    evidence = OPS / "evidence/linux-perl-independent-identity"
    evidence.mkdir(parents=True, exist_ok=False)
    archive = evidence / "perl-5.38.2-prefix.tar.gz"
    freeze_tree(prefix, archive)
    inspect_archive(archive)
    replay = evidence / "replay-prefix"
    extract_frozen(archive, replay)
    after = oracle.sha256_tree(replay)
    if after != before:
        raise Refused("frozen Perl archive round-trip changed the complete tree")
    os_release = Path("/etc/os-release").read_text() if Path("/etc/os-release").is_file() else None
    receipt = {
        "schema_version": 1,
        "kind": "linux_perl_unapproved_candidate",
        "source_head": head,
        "source_tree": tree,
        "source_clean_context": "asserted_by_public_launcher; verify remote-build.json source_status independently",
        "config_prefix": str(prefix),
        "lock_sha256": oracle.sha256_file(oracle.LOCK_PATH),
        "locked_archives": {name: {"path": str(path), "sha256": oracle.sha256_file(path),
                                   "url": lock["archives"][name]["url"]} for name, path in archives.items()},
        "archive_path": str(archive),
        "archive_sha256": oracle.sha256_file(archive),
        "archive_bytes": archive.stat().st_size,
        "perl_tree_sha256": before,
        "replay_tree_sha256": after,
        "perl_executable_sha256": oracle.sha256_file(perl),
        "archive_zip_library_sha256": oracle.sha256_file(zip_module),
        "archive_zip_library_path": str(zip_module),
        "perl_version": _command([str(perl), "-v"]),
        "archive_zip_version": _command([str(perl), "-MArchive::Zip", "-e", "print $Archive::Zip::VERSION"]),
        "perl_configured_prefix": _command([str(perl), "-MConfig", "-e", "print $Config{prefix}"]),
        "builder_image_identity": os.environ.get("OXIDEX_BUILDER_IMAGE_DIGEST"),
        "builder_image_identity_status": "environment_observed" if os.environ.get("OXIDEX_BUILDER_IMAGE_DIGEST") else "unknown",
        "os_release": os_release,
        "architecture": platform.machine(),
        "compiler_version": _command(["cc", "--version"]).splitlines()[0],
        "status": "candidate_only_requires_independent_review",
    }
    receipt_path = evidence / "candidate-receipt.json"
    with receipt_path.open("x", encoding="utf-8") as output:
        json.dump(receipt, output, indent=2, sort_keys=True)
        output.write("\n")
    print(json.dumps(receipt, sort_keys=True), flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("launch")
    remote = sub.add_parser("worker")
    remote.add_argument("--source-head", required=True)
    remote.add_argument("--source-tree", required=True)
    args = parser.parse_args()
    if args.action == "launch":
        launch()
    else:
        worker(args.source_head, args.source_tree)


if __name__ == "__main__":
    try:
        main()
    except (Refused, OSError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f"freeze-linux-perl refused: {exc}") from exc
