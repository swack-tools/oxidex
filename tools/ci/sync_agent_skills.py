"""Keep Codex project skills as mirrors of the shared Claude skills."""

from __future__ import annotations

import argparse
import pathlib
import shutil
import stat
import sys
from collections.abc import Sequence


_ALLOWLIST_PREFIX = "!.claude/skills/"


def shared_skill_names(repo: pathlib.Path) -> tuple[str, ...]:
    """Return the direct skill directories explicitly allowlisted in gitignore."""

    names: list[str] = []
    gitignore = repo / ".gitignore"
    for raw_line in gitignore.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line.startswith(_ALLOWLIST_PREFIX):
            continue
        name = line[len(_ALLOWLIST_PREFIX) :].rstrip("/")
        # ``!.claude/skills/`` only re-enables traversal into the parent.
        if (
            not name
            or name in {".", ".."}
            or "/" in name
            or "\\" in name
            or name.startswith("!")
        ):
            continue
        if name not in names:
            names.append(name)
    return tuple(names)


def _regular_files(root: pathlib.Path) -> dict[pathlib.Path, bytes]:
    """Read regular files below *root*, keyed by paths relative to *root*."""

    if not root.is_dir():
        return {}
    files: dict[pathlib.Path, bytes] = {}
    for path in root.rglob("*"):
        if path.is_file() and stat.S_ISREG(path.stat().st_mode):
            relative = path.relative_to(root)
            files[relative] = path.read_bytes()
    return files


def compare_skill_mirror(repo: pathlib.Path) -> list[str]:
    """Report missing, extra, and different mirror files in stable order."""

    differences: list[str] = []
    for name in shared_skill_names(repo):
        canonical_root = repo / ".claude/skills" / name
        mirror_root = repo / ".agents/skills" / name
        canonical = _regular_files(canonical_root)
        mirror = _regular_files(mirror_root)
        for relative in sorted(canonical.keys() - mirror.keys()):
            differences.append(f"{name}/{relative}: missing from mirror")
        for relative in sorted(mirror.keys() - canonical.keys()):
            differences.append(f"{name}/{relative}: extra in mirror")
        for relative in sorted(canonical.keys() & mirror.keys()):
            if canonical[relative] != mirror[relative]:
                differences.append(f"{name}/{relative}: differs from canonical")
    return differences


def _remove_mirror_skill(path: pathlib.Path) -> None:
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.is_dir():
        shutil.rmtree(path)


def write_skill_mirror(repo: pathlib.Path) -> None:
    """Replace only allowlisted mirror skills with their canonical contents."""

    mirror_root = repo / ".agents/skills"
    mirror_root.mkdir(parents=True, exist_ok=True)
    for name in shared_skill_names(repo):
        canonical = repo / ".claude/skills" / name
        if not canonical.is_dir():
            raise FileNotFoundError(f"canonical skill directory does not exist: {canonical}")
        destination = mirror_root / name
        _remove_mirror_skill(destination)
        shutil.copytree(canonical, destination)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=pathlib.Path, default=pathlib.Path.cwd())
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--check", action="store_true", help="check the mirror for drift")
    modes.add_argument("--write", action="store_true", help="regenerate the mirror")
    args = parser.parse_args(argv)
    repo = args.repo.resolve()

    if args.write:
        write_skill_mirror(repo)
        return 0

    differences = compare_skill_mirror(repo)
    for difference in differences:
        print(difference)
    return 1 if differences else 0


if __name__ == "__main__":
    sys.exit(main())
