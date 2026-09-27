#!/usr/bin/env python3
"""Project-local Vale feedback for Claude file edits and Codex patch/shell tools.

Reload the Codex session after changing hook registration. Without a matching
PreToolUse baseline, shell PostToolUse safely skips attribution; direct file
edits still receive feedback. Installed Codex 0.157.1 normalizes exec_command
into Bash/command and drops workdir, retaining the session cwd. Hidden sibling
shell workdirs cannot be observed; explicit file paths and supplied workdir can.
"""
import fcntl
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import time

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
        if not isinstance(patch, str):
            return []
        names = []
        for line in patch.splitlines():
            for prefix in ('*** Add File: ', '*** Update File: ', '*** Move to: '):
                if line.startswith(prefix):
                    names.append(line[len(prefix):])
    else:
        names = []
    paths = set()
    for name in names:
        path = Path(name)
        if not path.is_absolute():
            path = cwd / path
        if path.is_symlink() or any(parent.is_symlink() for parent in path.parents):
            continue
        path = path.resolve()
        if not path.is_file() or path.suffix not in EXTENSIONS:
            continue
        try:
            target_root = Path(os.fsdecode(git(path.parent, 'rev-parse', '--show-toplevel')).strip()).resolve()
            common = git(root, 'rev-parse', '--path-format=absolute', '--git-common-dir')
            target_common = git(target_root, 'rev-parse', '--path-format=absolute', '--git-common-dir')
        except RuntimeError:
            continue
        if common == target_common and path.is_relative_to(target_root):
            name = str(path.relative_to(target_root))
            if name.startswith(('src/exiftool_tables/', 'oxidex-tags-', '.vale/')):
                continue
            ignored = subprocess.run(['git', '-C', str(target_root), 'check-ignore', '--no-index', '-q', '--', name])
            if ignored.returncode != 0:
                paths.add(path)
    return sorted(paths)


SHELL_TOOLS = {'Bash', 'exec_command'}
SNAPSHOT_TTL = 3600
MAX_PENDING_SNAPSHOTS = 64
OWNED_SNAPSHOT = re.compile(r"[0-9a-f]{64}(?:\.json|\.[0-9]+\.tmp)\Z")
MAX_SNAPSHOT_FILES = 20000
MAX_LINT_FILES = 100
MAX_LINT_BYTES = 1024 * 1024


def shell_snapshot(root):
    """Stat eligible Git files; never hash the checkout on a read command."""
    data = git(root, 'ls-files', '-z', '--cached', '--others', '--exclude-standard')
    names = {os.fsdecode(name) for name in data.split(b'\0') if name}
    if len(names) > MAX_SNAPSHOT_FILES:
        return None
    ignored = subprocess.run(['git', '-C', str(root), 'check-ignore', '--no-index', '-z', '--stdin'],
                             input=b'\0'.join(os.fsencode(name) for name in names)+b'\0',
                             capture_output=True).stdout
    names -= {os.fsdecode(name) for name in ignored.split(b'\0') if name}
    result = {}
    for name in names:
        path = root / name
        if (path.suffix not in EXTENSIONS or path.is_symlink() or not path.is_file()
                or not path.resolve().is_relative_to(root)):
            continue
        if name.startswith(('src/exiftool_tables/', 'oxidex-tags-', '.vale/')):
            continue
        try:
            stat = path.stat()
        except FileNotFoundError:
            continue
        result[name] = [stat.st_mtime_ns, stat.st_ctime_ns, stat.st_size, stat.st_ino]
    return result


def input_digest(inputs):
    """Compare inputs without retaining shell commands in baseline files."""
    return hashlib.sha256(json.dumps(inputs, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def store_snapshot(state_dir, state, snapshot):
    """Prune owned expired files; preserve active baselines when capacity is full."""
    state_dir.mkdir(exist_ok=True)
    if state_dir.is_symlink():
        return
    lock = state_dir / '.lock'
    if lock.is_symlink():
        return
    with lock.open('a') as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        active = 0
        now = time.time()
        for path in state_dir.iterdir():
            if not OWNED_SNAPSHOT.fullmatch(path.name) or path.is_symlink() or not path.is_file():
                continue
            try:
                age = now - path.stat().st_mtime
                if age > SNAPSHOT_TTL:
                    path.unlink(missing_ok=True)
                else:
                    active += 1
            except FileNotFoundError:
                continue
        # A repeated pre event must not replace an active call's baseline.
        if state.exists() or active >= MAX_PENDING_SNAPSHOTS:
            return
        pending = state.with_suffix(f'.{os.getpid()}.tmp')
        try:
            pending.write_text(json.dumps(snapshot))
            pending.replace(state)
        finally:
            pending.unlink(missing_ok=True)


def shell_paths(payload, root, cwd):
    """Consume a matching pre-call baseline once, including failed lint runs."""
    session = payload.get('session_id')
    call = payload.get('tool_use_id')
    event = payload.get('hook_event_name')
    if not isinstance(session, str) or not session or not isinstance(call, str) or not call:
        return []
    key = hashlib.sha256(json.dumps([session, call]).encode()).hexdigest()
    git_dir = Path(os.fsdecode(git(root, 'rev-parse', '--absolute-git-dir')).strip())
    state_dir = git_dir / 'prose-lint-snapshots'
    state = state_dir / (key + '.json')
    if state_dir.is_symlink() or state.is_symlink():
        return []
    if event == 'PreToolUse':
        snapshot = shell_snapshot(root)
        if snapshot is None:
            return []
        store_snapshot(state_dir, state, {'root': str(root), 'cwd': str(cwd),
                                         'input_digest': input_digest(payload.get('tool_input')),
                                         'created': time.time(), 'files': snapshot})
        return []
    if event != 'PostToolUse':
        return []
    try:
        before = json.loads(state.read_text())
    except (OSError, ValueError):
        return []
    finally:
        state.unlink(missing_ok=True)
    if not isinstance(before, dict):
        return []
    if (before.get('root') != str(root) or before.get('cwd') != str(cwd)
            or before.get('input_digest') != input_digest(payload.get('tool_input'))
            or not isinstance(before.get('created'), (float, int))
            or not 0 <= time.time() - before['created'] <= SNAPSHOT_TTL
            or not isinstance(before.get('files'), dict)):
        return []
    if any(not isinstance(signature, list) or len(signature) != 4
           or any(type(value) is not int or value < 0 for value in signature)
           for signature in before['files'].values()):
        return []
    after = shell_snapshot(root)
    if after is None:
        return []
    return [root / name for name, signature in sorted(after.items())
            if before['files'].get(name) != signature]


def main():
    try:
        try:
            payload = json.load(sys.stdin)
        except ValueError:
            return 0
        if not isinstance(payload, dict):
            return 0
        inputs = payload.get('tool_input') or {}
        if not isinstance(inputs, dict):
            return 0
        cwd_name = inputs.get('workdir') or inputs.get('cwd') or payload.get('cwd')
        if not isinstance(cwd_name, str) or not cwd_name:
            return 0
        cwd = Path(cwd_name).resolve()
        try:
            root = Path(os.fsdecode(git(cwd, 'rev-parse', '--show-toplevel')).strip()).resolve()
        except RuntimeError:
            return 0
        if payload.get('tool_name') in SHELL_TOOLS:
            paths = shell_paths(payload, root, cwd)
        elif payload.get('hook_event_name') == 'PreToolUse':
            return 0
        else:
            paths = selected_paths(payload, root, cwd)
        # Keep feedback bounded and exclude generated headers on changed paths.
        selected = []
        for path in paths[:MAX_LINT_FILES]:
            if path.stat().st_size > MAX_LINT_BYTES:
                continue
            with path.open('rb') as source:
                header = b'\n'.join(source.read(2048).splitlines()[:15]).lower()
            if any(marker in header for marker in (b'auto-generated', b'autogenerated', b'@generated', b'do not edit')):
                continue
            selected.append(path)
        paths = selected
        groups = {}
        for path in paths:
            target_root = Path(os.fsdecode(git(path.parent, 'rev-parse', '--show-toplevel')).strip()).resolve()
            groups.setdefault(target_root, []).append(path)
        return max((lint_paths(target_root, files) for target_root, files in groups.items()), default=0)
    except (ValueError, OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f'prose-lint: {error}', file=sys.stderr)
        return 2


def lint_paths(root, paths):
    config = root / '.vale.ini'
    if not config.is_file():
        raise RuntimeError('project root .vale.ini is missing')
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
    if result.returncode:
        print('Fix these style errors: '+result.stdout+result.stderr, file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
