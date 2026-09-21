"""Portable root for durable OxiDex operational state.

Hosted CI scratch caches use their own explicit paths; they are not release
evidence. All durable tools share OXIDEX_OPS_DIR, defaulting to ~/oxidex-ops.
"""

from __future__ import annotations

import os
from pathlib import Path
import re
import sys
import tempfile


def ops_root() -> Path:
    raw = os.environ.get("OXIDEX_OPS_DIR", "").strip()
    path = Path(raw).expanduser() if raw else Path.home() / "oxidex-ops"
    return durable_root(path, "OXIDEX_OPS_DIR")


def durable_root(path: Path, name: str) -> Path:
    if not path.is_absolute():
        raise ValueError(f"{name} must name an absolute durable directory")
    resolved = path.resolve()
    temporary_roots = {Path("/tmp"), Path("/private/tmp"), Path("/var/tmp"),
                       Path(tempfile.gettempdir())}
    for temporary in temporary_roots:
        if resolved.is_relative_to(temporary.resolve()):
            raise ValueError(f"{name} must be durable, not temporary: {path}")
    if resolved == Path(resolved.anchor):
        raise ValueError(f"{name} must not be a filesystem root")
    cursor = Path(path.anchor)
    for component in path.parts[1:]:
        cursor /= component
        if cursor.is_symlink():
            raise ValueError(f"{name} must not contain a symlink: {cursor}")
    return resolved


def worktree_root() -> Path:
    raw = os.environ.get("OXIDEX_WORKTREE_ROOT", "").strip()
    return durable_root(Path(raw).expanduser() if raw else Path.home() / "git", "OXIDEX_WORKTREE_ROOT")


def target_root() -> Path:
    raw = os.environ.get("OXIDEX_TARGET_ROOT", "").strip()
    return durable_root(Path(raw).expanduser() if raw else worktree_root() / "oxidex-beta1-targets",
                        "OXIDEX_TARGET_ROOT")


def pinned_version() -> str:
    version = (Path(__file__).resolve().parents[1] / ".exiftool-version").read_text().strip()
    if not re.fullmatch(r"[0-9]+\.[0-9]+", version):
        raise ValueError(".exiftool-version must contain a numeric release")
    return version


def oracle_cache_root() -> Path:
    return ops_root() / "cache/exiftool" / pinned_version()


if __name__ == "__main__":
    if sys.argv[1:] == ["--cache"]:
        print(oracle_cache_root())
    elif not sys.argv[1:]:
        print(ops_root())
    else:
        raise SystemExit("usage: ops_paths.py [--cache]")
