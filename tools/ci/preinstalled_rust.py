#!/usr/bin/env python3
"""Select only the fixed, read-only Spot runner Rust image before CI setup."""

import os
from pathlib import Path
import re
import stat
import subprocess
import tomllib

ROOT = Path('/opt/runner-rust')
PIN_FILE = Path('rust-toolchain.toml')
TOOLS = ('rustc', 'cargo', 'rustfmt', 'clippy-driver')


def _trusted_storage(root: Path) -> None:
    if root != ROOT or root.is_symlink() or root.resolve(strict=True) != root:
        raise RuntimeError('Preinstalled Rust root is not the canonical fixed path')
    for path in (Path('/opt'), root, root / 'cargo', root / 'cargo/bin',
                 root / 'rustup', root / 'cargo/bin/rustup'):
        info = path.lstat()
        expected = stat.S_ISREG if path == root / 'cargo/bin/rustup' else stat.S_ISDIR
        if (path.is_symlink() or path.resolve(strict=True) != path
                or not expected(info.st_mode) or info.st_uid != 0
                or info.st_mode & 0o022):
            raise RuntimeError(f'Untrusted preinstalled Rust path: {path}')
    _readonly_mount(root)


def _readonly_mount(root: Path) -> None:
    if not os.path.ismount(root) or not os.statvfs(root).f_flag & os.ST_RDONLY:
        raise RuntimeError('Preinstalled Rust root is not a read-only mount')


def _trusted_tool(path: Path, tool: str, root: Path) -> None:
    info = path.lstat()
    if (path.is_symlink() or not stat.S_ISREG(info.st_mode)
            or info.st_uid != 0 or info.st_mode & 0o022):
        raise RuntimeError(f'Preinstalled {tool} is not trusted read-only content')
    parent = path.parent
    while parent != root:
        info = parent.lstat()
        if (parent.is_symlink() or not stat.S_ISDIR(info.st_mode)
                or info.st_uid != 0 or info.st_mode & 0o022):
            raise RuntimeError(f'Preinstalled {tool} parent is not trusted read-only content')
        parent = parent.parent


def _pin(source: Path) -> tuple[str, tuple[str, ...], tuple[str, ...]]:
    config = tomllib.loads((source / PIN_FILE).read_text())['toolchain']
    pin = config['channel']
    components = config.get('components', [])
    targets = config.get('targets', [])
    if (type(pin) is not str or not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', pin)
            or type(components) is not list
            or any(type(name) is not str or not re.fullmatch(r'[a-z0-9-]+', name)
                   for name in components)
            or type(targets) is not list
            or any(type(name) is not str or not re.fullmatch(r'[a-z0-9_-]+', name)
                   for name in targets)):
        raise RuntimeError('Source Rust pin or component list is invalid')
    return pin, tuple(components), tuple(targets)


def probe(source: Path = Path('.'), root: Path = ROOT) -> bool:
    """Absent image keeps legacy setup; a present but invalid image always fails."""
    pin, required, required_targets = _pin(source)
    try:
        root.lstat()
    except FileNotFoundError:
        return False
    _trusted_storage(root)
    rustup = root / 'cargo/bin/rustup'
    env = dict(os.environ, RUSTUP_HOME=str(root / 'rustup'),
               PATH=str(root / 'cargo/bin') + os.pathsep + os.environ.get('PATH', '/usr/bin:/bin'))
    def run(*args: str) -> str:
        return subprocess.check_output(args, env=env, text=True, timeout=20).strip()
    resolved = {}
    for tool in TOOLS:
        path = Path(run(str(rustup), 'which', '--toolchain', pin, tool))
        if (not path.is_absolute() or path.is_symlink()
                or path.resolve(strict=True) != path
                or not path.is_relative_to(root / 'rustup/toolchains')
                or path.parent.name != 'bin'):
            raise RuntimeError(f'Preinstalled {tool} is outside the fixed toolchain')
        _trusted_tool(path, tool, root)
        resolved[tool] = path
    rustc = run(str(resolved['rustc']), '-vV')
    release = re.search(r'^release: (\S+)$', rustc, re.M)
    commit = re.search(r'^commit-hash: ([0-9a-f]{40})$', rustc, re.M)
    host = re.search(r'^host: ([a-z0-9_-]+)$', rustc, re.M)
    if not release or release[1] != pin or not commit or not host:
        raise RuntimeError('Preinstalled rustc differs from the source pin')
    if any(path.parents[1].name != f'{pin}-{host[1]}' for path in resolved.values()):
        raise RuntimeError('Preinstalled tools resolve to different toolchains')
    if not run(str(resolved['cargo']), '-vV').startswith(f'cargo {pin} '):
        raise RuntimeError('Preinstalled Cargo differs from the source pin')
    installed = set(run(str(rustup), 'component', 'list', '--installed', '--toolchain', pin).splitlines())
    for name in required:
        if not any(row.startswith(name + '-') or row.startswith(name + ' ') for row in installed):
            raise RuntimeError(f'Preinstalled Rust component missing: {name}')
    targets = set(run(str(rustup), 'target', 'list', '--installed', '--toolchain', pin).splitlines())
    if not {host[1], *required_targets} <= targets:
        raise RuntimeError('Preinstalled required Rust target is missing')
    return True


def main() -> None:
    ready = probe()
    with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
        output.write(f'ready={str(ready).lower()}\n')
    if ready:
        with open(os.environ['GITHUB_ENV'], 'a') as output:
            output.write(f'RUSTUP_HOME={ROOT / "rustup"}\n')
        with open(os.environ['GITHUB_PATH'], 'a') as output:
            output.write(f'{ROOT / "cargo/bin"}\n')


if __name__ == '__main__':
    main()
