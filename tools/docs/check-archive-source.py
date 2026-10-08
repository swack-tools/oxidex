#!/usr/bin/env python3
"""Refuse Git attributes that can change a committed docs archive."""
import os
from pathlib import Path
import re
import subprocess
import sys


def git(repo, *args):
    return subprocess.check_output(['git', '-C', str(repo), *args])


def check(repo, head):
    if not re.fullmatch(r'[0-9a-f]{40}', head):
        raise RuntimeError('docs archive requires an exact commit SHA')
    private = Path(os.fsdecode(git(repo, 'rev-parse', '--git-path', 'info/attributes').strip()))
    if not private.is_absolute():
        private = repo / private
    if private.exists() or private.is_symlink():
        raise RuntimeError('docs archive refuses private Git attributes')
    paths = git(repo, 'ls-tree', '-r', '-z', '--name-only', head).split(b'\0')
    for raw_path in paths:
        if not raw_path or raw_path.rsplit(b'/', 1)[-1] != b'.gitattributes':
            continue
        contents = git(repo, 'show', f'{head}:{os.fsdecode(raw_path)}')
        for line in contents.splitlines():
            stripped = line.lstrip()
            if stripped.startswith(b'#'):
                continue
            if b'export-ignore' in stripped or b'export-subst' in stripped:
                raise RuntimeError('docs archive refuses committed export attribute: '
                                   + os.fsdecode(raw_path))


if __name__ == '__main__':
    try:
        if len(sys.argv) != 3:
            raise RuntimeError('usage: check-archive-source.py REPOSITORY COMMIT_SHA')
        check(Path(sys.argv[1]), sys.argv[2])
    except (RuntimeError, subprocess.CalledProcessError) as exc:
        raise SystemExit(str(exc)) from exc
