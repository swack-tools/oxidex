#!/usr/bin/env python3
"""Project-local Vale feedback for Claude file edits and Codex patch/shell tools."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

EXTENSIONS = {'.md', '.rs', '.py', '.sh', '.pl'}


def git(cwd, *args):
    result = subprocess.run(['git', '-C', str(cwd), *args], capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors='replace').strip())
    return result.stdout


def selected_paths(payload, root, cwd):
    inputs = payload.get('tool_input') or {}
    direct = inputs.get('file_path')
    if isinstance(direct, str):
        names = [direct]
    elif payload.get('tool_name') in {'apply_patch', 'Edit', 'Write'}:
        patch = inputs.get('command', '')
        names = []
        for line in patch.splitlines():
            for prefix in ('*** Add File: ', '*** Update File: ', '*** Move to: '):
                if line.startswith(prefix):
                    names.append(line[len(prefix):])
    elif payload.get('tool_name') in {'Bash', 'exec_command'}:
        # A shell can write files without exposing paths in its command text.
        # NUL-separated Git output preserves whitespace and non-ASCII names.
        data = git(root, 'diff', '--name-only', '-z')
        data += git(root, 'diff', '--cached', '--name-only', '-z')
        data += git(root, 'ls-files', '--others', '--exclude-standard', '-z')
        names = [os.fsdecode(name) for name in data.split(b'\0') if name]
        cwd = root
    else:
        names = []
    paths = set()
    for name in names:
        path = Path(name)
        if not path.is_absolute():
            path = cwd / path
        path = path.resolve()
        if path.is_relative_to(root) and path.is_file() and path.suffix in EXTENSIONS:
            paths.add(path)
    return sorted(paths)


def main():
    try:
        payload = json.load(sys.stdin)
        inputs = payload.get('tool_input') or {}
        cwd = Path(inputs.get('workdir') or payload.get('cwd') or
                   os.environ.get('CLAUDE_PROJECT_DIR') or Path.cwd()).resolve()
        root = Path(os.fsdecode(git(cwd, 'rev-parse', '--show-toplevel')).strip()).resolve()
        paths = selected_paths(payload, root, cwd)
        config = root / '.vale.ini'
        if paths and not config.is_file():
            raise RuntimeError('project root .vale.ini is missing')
        cache = None
        fingerprints = {}
        if payload.get('tool_name') in {'Bash', 'exec_command'} and paths:
            # A read-only shell command should not repeat lint feedback for
            # unchanged dirty files. State is private to this Git worktree
            # and session; explicit patch/file edits always run the checker.
            git_dir = Path(os.fsdecode(git(root, 'rev-parse', '--absolute-git-dir')).strip())
            session = hashlib.sha256(str(payload.get('session_id', 'local')).encode()).hexdigest()[:16]
            cache = git_dir / f'prose-lint-{session}.json'
            try:
                old = json.loads(cache.read_text())
            except (OSError, ValueError):
                old = {}
            context = hashlib.sha256(config.read_bytes())
            for style in sorted((root / '.vale/styles').rglob('*')):
                if style.is_file():
                    context.update(str(style.relative_to(root)).encode())
                    context.update(style.read_bytes())
            config_bytes = context.digest()
            fingerprints = {str(path): hashlib.sha256(config_bytes + b'\0' + path.read_bytes()).hexdigest() for path in paths}
            paths = [path for path in paths if old.get(str(path)) != fingerprints[str(path)]]
        if not paths:
            return 0
        vale = shutil.which('vale')
        if not vale:
            print('prose-lint: vale not installed, skipping', file=sys.stderr)
            return 0
        if not (root / '.vale/styles/Google').is_dir():
            sync = subprocess.run([vale, '--config', str(config), 'sync'], cwd=root,
                                  capture_output=True, text=True, timeout=15)
            if sync.returncode:
                raise RuntimeError('vale sync failed: '+sync.stdout+sync.stderr)
        result = subprocess.run([vale, '--config', str(config), '--output=line',
                                 *map(str, paths)], cwd=root, capture_output=True,
                                text=True, timeout=20)
        if cache is not None and result.returncode == 0:
            # Concurrent completions can replace an older snapshot. A later
            # content mismatch causes another check, never an unchecked skip.
            pending = cache.with_name(cache.name + f'.{os.getpid()}.tmp')
            try:
                pending.write_text(json.dumps(fingerprints))
                pending.replace(cache)
            finally:
                pending.unlink(missing_ok=True)
        if result.returncode:
            print('Fix these style errors: '+result.stdout+result.stderr, file=sys.stderr)
            return 2
        return 0
    except (ValueError, OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f'prose-lint: {error}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
