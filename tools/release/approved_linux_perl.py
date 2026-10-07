"""Signed-source approval and bounded JSON transport for qualification Perl."""
from __future__ import annotations

import base64
import binascii
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import stat

DESCRIPTOR = Path(__file__).with_name("oracle-linux-perl-identity.json")
ENVELOPE = Path("/src/reference/approved-linux-perl.json")
QUALIFICATION_ROOT = Path("/target/ops")
PREFIX = QUALIFICATION_ROOT / "toolchains/perl-5.38.2/prefix"
MAX_ARCHIVE_BYTES = 256 * 1024 * 1024
MAX_ENVELOPE_BYTES = 4 * ((MAX_ARCHIVE_BYTES + 2) // 3) + 4096
HEX = re.compile(r"[0-9a-f]{64}\Z")


class Refused(RuntimeError):
    """Qualification has no independently approved Perl installation."""


def sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _document(path: Path, limit: int) -> dict:
    if path.is_symlink() or not path.is_file():
        raise Refused(f"approved Linux Perl input is missing or not regular: {path}")
    info = path.stat()
    if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= limit:
        raise Refused(f"approved Linux Perl input exceeds bound or is empty: {path}")
    def pairs(rows):
        result = {}
        for key, value in rows:
            if key in result:
                raise Refused(f"duplicate approved Linux Perl JSON key: {key}")
            result[key] = value
        return result
    try:
        value = json.loads(path.read_bytes(), object_pairs_hook=pairs)
    except (OSError, UnicodeError, ValueError, TypeError) as exc:
        raise Refused(f"approved Linux Perl JSON is unreadable: {path}") from exc
    if not isinstance(value, dict):
        raise Refused("approved Linux Perl JSON must be an object")
    return value


def load(lock_path: Path, *, descriptor: Path = DESCRIPTOR,
         envelope: Path = ENVELOPE,
         require_platform: bool = True) -> tuple[dict, bytes]:
    """Require signed-source approval and a matching bounded transport envelope."""
    if require_platform and (platform.system() != "Linux" or platform.machine() != "x86_64"):
        raise Refused("approved Linux Perl requires Linux x86_64")
    approved = _document(descriptor, 8192)
    expected = {"schema", "kind", "platform", "prefix", "lock_sha256",
                "perl_source_sha256", "archive_zip_source_sha256",
                "archive_sha256", "archive_bytes", "tree_sha256",
                "exe_sha256", "zip_sha256", "zip_relative_path", "producer"}
    if (set(approved) != expected or approved.get("schema") != 1
            or approved.get("kind") != "oxidex_approved_linux_perl_installation"
            or approved.get("platform") != "linux-x86_64"
            or approved.get("prefix") != str(PREFIX)
            or approved.get("lock_sha256") != sha(lock_path)):
        raise Refused("approved Linux Perl descriptor has wrong schema, prefix or lock")
    for name in ("lock_sha256", "perl_source_sha256", "archive_zip_source_sha256",
                 "archive_sha256", "tree_sha256", "exe_sha256", "zip_sha256"):
        if not isinstance(approved[name], str) or not HEX.fullmatch(approved[name]):
            raise Refused(f"approved Linux Perl {name} is invalid")
    lock = _document(lock_path, 128 * 1024)
    if (approved["perl_source_sha256"] != lock["archives"]["perl"]["sha256"]
            or approved["archive_zip_source_sha256"] != lock["archives"]["archive_zip"]["sha256"]):
        raise Refused("approved Linux Perl source archives differ from the lock")
    relative = approved["zip_relative_path"]
    if (not isinstance(relative, str) or not relative.startswith("lib/")
            or not relative.endswith("/Archive/Zip.pm")
            or any(part in ("", ".", "..") for part in relative.split("/"))
            or PurePosixPath(relative).is_absolute()):
        raise Refused("approved Linux Perl Archive::Zip path is invalid")
    producer = approved["producer"]
    if (not isinstance(producer, dict) or set(producer) != {"source_head", "receipt_sha256",
                                                           "image", "compiler"}
            or not all(isinstance(value, str) and value for value in producer.values())
            or not re.fullmatch(r"[0-9a-f]{40}", producer["source_head"])
            or not HEX.fullmatch(producer["receipt_sha256"])):
        raise Refused("approved Linux Perl producer evidence is incomplete")
    size = approved["archive_bytes"]
    if type(size) is not int or not 0 < size <= MAX_ARCHIVE_BYTES:
        raise Refused("approved Linux Perl archive size is out of bounds")
    carried = _document(envelope, MAX_ENVELOPE_BYTES)
    if (set(carried) != {"schema", "kind", "archive_base64"} or carried.get("schema") != 1
            or carried.get("kind") != "oxidex_approved_linux_perl_archive"):
        raise Refused("approved Linux Perl envelope schema differs")
    encoded = carried["archive_base64"]
    if not isinstance(encoded, str) or len(encoded) != 4 * ((size + 2) // 3):
        raise Refused("approved Linux Perl encoded archive size differs")
    try:
        archive = base64.b64decode(encoded, validate=True)
    except (ValueError, binascii.Error) as exc:
        raise Refused("approved Linux Perl archive encoding is invalid") from exc
    if len(archive) != size or hashlib.sha256(archive).hexdigest() != approved["archive_sha256"]:
        raise Refused("approved Linux Perl archive digest or length differs")
    return approved, archive


def check_tree(approved: dict, candidate: Path, sha256_tree) -> None:
    try:
        relative = candidate.relative_to(QUALIFICATION_ROOT)
    except ValueError as exc:
        raise Refused("approved Linux Perl prefix is outside qualification root") from exc
    current = QUALIFICATION_ROOT
    for component in (None, *relative.parts):
        if component is not None:
            current /= component
        try:
            info = current.lstat()
        except OSError as exc:
            raise Refused(f"approved Linux Perl prefix is absent: {current}") from exc
        if not stat.S_ISDIR(info.st_mode):
            raise Refused(f"approved Linux Perl prefix is not a directory: {current}")
        if info.st_uid != os.geteuid():
            raise Refused(f"approved Linux Perl prefix has foreign owner: {current}")
        if stat.S_IMODE(info.st_mode) not in (0o700, 0o755):
            raise Refused(f"approved Linux Perl prefix has unsafe mode: {current}")
    if sha256_tree(candidate) != approved["tree_sha256"]:
        raise Refused("approved Linux Perl whole-tree hash mismatch")
    perl = candidate / "bin/perl5.38.2"
    zip_module = candidate / approved["zip_relative_path"]
    for path, expected in ((perl, approved["exe_sha256"]),
                           (zip_module, approved["zip_sha256"])):
        if path.is_symlink() or not path.is_file() or sha(path) != expected:
            raise Refused(f"approved Linux Perl component hash mismatch: {path}")
