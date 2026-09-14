"""Content identity for compiled inputs, independent of report publication."""
import hashlib
from pathlib import Path


def runtime_input_manifest(root: Path) -> str:
    """Hash compiled parser/build inputs, deliberately excluding reports and docs."""
    paths = [root / name for name in ("Cargo.toml", "Cargo.lock", "build.rs", ".exiftool-version") if (root / name).is_file()]
    paths += [path for name in ("src", ".cargo") for path in (root / name).rglob("*") if path.is_file()]
    paths += [path for crate in [root / "oxidex-tags", *root.glob("oxidex-tags-*")] if crate.is_dir() for path in crate.rglob("*")
              if path.is_file() and path.suffix in {".rs", ".toml", ".yaml"}]
    digest = hashlib.sha256()
    for path in sorted(paths):
        digest.update(str(path.relative_to(root)).encode() + b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()

