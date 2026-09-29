"""Commit-backed clean view of an owned, freshly regenerated checkout.

The rehearsal keeps its execution checkout at the original source HEAD. Its
generated files are therefore legitimately dirty, while conformance receipts
require a clean Git identity. This module copies *those exact bytes* to a
separate signed child commit for read measurement; it never edits the owned
execution checkout or relaxes conformance's clean-source rule.
"""
from __future__ import annotations

import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
from typing import Any


SNAPSHOT_DIR = "measurement-source"
OID = re.compile(r"[0-9a-f]{40}\Z")
SHA = re.compile(r"[0-9a-f]{64}\Z")
EXCLUDED_DIRS = {".git", "target", "__pycache__"}


class Refused(ValueError):
    pass


def _git(repo: Path, *args: str, binary: bool = False) -> str | bytes:
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    result = subprocess.run(["git", "-C", str(repo), *args], cwd=repo, env=env,
                            capture_output=True, text=not binary, check=False)
    if result.returncode != 0:
        raise Refused(f"clean measurement snapshot Git command failed: {' '.join(args)}: "
                      f"{result.stderr.decode(errors='replace') if binary else result.stderr}")
    return result.stdout if binary else result.stdout.strip()


def _file_rows(root: Path) -> dict[str, list[str]]:
    rows: dict[str, list[str]] = {}
    for directory, dirs, names in os.walk(root, followlinks=False):
        parent = Path(directory)
        if any((parent / name).is_symlink() for name in dirs):
            raise Refused("clean measurement snapshot source contains a symlink")
        dirs[:] = sorted(name for name in dirs if name not in EXCLUDED_DIRS)
        for name in sorted(names):
            if name == ".git":
                continue
            path = parent / name
            if path.is_symlink():
                raise Refused("clean measurement snapshot source contains a symlink")
            if not path.is_file():
                raise Refused("clean measurement snapshot source has a nonregular entry")
            relative = path.relative_to(root).as_posix()
            rows[relative] = ["file", hashlib.sha256(path.read_bytes()).hexdigest()]
    return rows


def source_tree_sha256(root: Path) -> str:
    encoded = json.dumps(_file_rows(root), sort_keys=True, separators=(",", ":"),
                         ensure_ascii=True).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _tracked(root: Path) -> set[str]:
    payload = _git(root, "ls-files", "-z", binary=True)
    assert isinstance(payload, bytes)
    return {os.fsdecode(item) for item in payload.split(b"\0") if item}


def _diff(root: Path, parent: str, child: str) -> tuple[list[str], str]:
    names = _git(root, "diff", "--name-only", "-z", parent, child, binary=True)
    patch = _git(root, "diff", "--binary", parent, child, binary=True)
    assert isinstance(names, bytes) and isinstance(patch, bytes)
    return ([os.fsdecode(item) for item in names.split(b"\0") if item],
            hashlib.sha256(patch).hexdigest())


def _ssh_fingerprint(public_key: bytes) -> str:
    fields = public_key.decode("utf-8").strip().split()
    if (len(fields) < 2 or not fields[0].startswith(("ssh-", "ecdsa-", "sk-"))):
        raise Refused("clean measurement snapshot SSH public key is malformed")
    try:
        blob = base64.b64decode(fields[1], validate=True)
    except ValueError as error:
        raise Refused("clean measurement snapshot SSH public key is malformed") from error
    return "SHA256:" + base64.b64encode(hashlib.sha256(blob).digest()).decode().rstrip("=")


def _signature_trust(source: Path) -> tuple[str, str, str | None, str | None, str, str | None, str | None, str | None]:
    """Resolve source signing configuration and public-key bytes, not clone config."""
    signing_format = _git(source, "config", "--get", "--default=openpgp", "gpg.format")
    assert isinstance(signing_format, str)
    signing_key = _git(source, "config", "--get", "--default=", "user.signingkey")
    assert isinstance(signing_key, str)
    if signing_format != "ssh":
        return (signing_format, signing_key, None, None,
                hashlib.sha256(signing_key.encode()).hexdigest(), None, None, None)
    if not signing_key:
        raise Refused("clean measurement snapshot SSH signing key is absent")
    if signing_key.startswith("key::"):
        literal = signing_key.removeprefix("key::").strip()
        if not literal:
            raise Refused("clean measurement snapshot literal SSH signing key is empty")
        key_mode, public_path = "literal", None
        public_sha = hashlib.sha256(literal.encode()).hexdigest()
        fingerprint = _ssh_fingerprint(literal.encode())
        effective_key = signing_key
    else:
        key_path = Path(signing_key).expanduser()
        if not key_path.is_absolute():
            key_path = source / key_path
        if not key_path.is_file():
            raise Refused("clean measurement snapshot SSH signing key path is absent")
        key_path = key_path.resolve()
        # A public key may have any filename; inspect its contents rather than
        # treating the .pub suffix as its identity.
        key_bytes = key_path.read_bytes()
        public_key = (key_path if key_bytes.startswith((b"ssh-", b"ecdsa-", b"sk-"))
                      else Path(str(key_path) + ".pub"))
        if not public_key.is_file():
            raise Refused("clean measurement snapshot SSH public signing key is absent")
        key_mode, public_path = "file", str(public_key.resolve())
        public_sha = hashlib.sha256(public_key.read_bytes()).hexdigest()
        fingerprint = _ssh_fingerprint(public_key.read_bytes())
        effective_key = str(key_path)
    configured = _git(source, "config", "--path", "--get", "gpg.ssh.allowedSignersFile")
    assert isinstance(configured, str)
    path = Path(configured).expanduser()
    if not path.is_absolute():
        path = source / path
    if path.is_symlink() or not path.is_file():
        raise Refused("clean measurement snapshot allowed signers file is absent or not regular")
    path = path.resolve()
    return (signing_format, effective_key, key_mode, public_path, public_sha,
            str(path), hashlib.sha256(path.read_bytes()).hexdigest(), fingerprint)


def _source_identity(source: Path, parent: str, expected_digest: str,
                     sanctioned_paths: set[str]) -> dict[str, list[str]]:
    if not OID.fullmatch(parent) or not SHA.fullmatch(expected_digest):
        raise Refused("clean measurement snapshot has malformed source identity")
    if source.is_symlink() or not source.is_dir() or _git(source, "rev-parse", "HEAD") != parent:
        raise Refused("clean measurement snapshot source HEAD differs from execution source")
    if _git(source, "diff", "--cached", "--name-only"):
        raise Refused("clean measurement snapshot source index is staged")
    rows = _file_rows(source)
    untracked = set(rows) - _tracked(source)
    if untracked - sanctioned_paths:
        raise Refused("clean measurement snapshot source contains unexpected untracked entries")
    if source_tree_sha256(source) != expected_digest:
        raise Refused("clean measurement snapshot source bytes differ from generation proof")
    return rows


def create(source: Path, target: Path, parent: str, expected_digest: str,
           sanctioned_paths: set[str]) -> dict[str, Any]:
    """Make one signed child from precisely the generated source bytes."""
    source, target = Path(source).absolute(), Path(target).absolute()
    if target.is_symlink() or not target.is_dir():
        raise Refused("clean measurement snapshot target must be an isolated directory")
    measured = target / SNAPSHOT_DIR
    if measured.exists() or measured.is_symlink():
        raise Refused("clean measurement snapshot target already exists")
    rows = _source_identity(source, parent, expected_digest, sanctioned_paths)
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    cloned = subprocess.run(["git", "clone", "--quiet", "--local", "--no-hardlinks", "--no-checkout",
                             "--", str(source), str(measured)], cwd=target, env=env,
                            capture_output=True, text=True, check=False)
    if cloned.returncode != 0:
        raise Refused(f"cannot isolate clean measurement snapshot: {cloned.stderr}")
    _git(measured, "checkout", "--quiet", "--detach", parent)
    clean = _file_rows(measured)
    changed = {name for name in rows.keys() | clean.keys() if rows.get(name) != clean.get(name)}
    if changed - sanctioned_paths:
        raise Refused("clean measurement snapshot changes non-generated source entries")
    for name in clean.keys() - rows.keys():
        (measured / name).unlink()
    for name in rows:
        destination = measured / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source / name, destination)
    if source_tree_sha256(measured) != expected_digest or source_tree_sha256(source) != expected_digest:
        raise Refused("clean measurement snapshot copy differs from generated source bytes")
    _git(measured, "add", "-A")
    newly_generated = sorted(set(rows) - _tracked(source))
    if newly_generated:
        _git(measured, "add", "-f", "--", *newly_generated)
    (signing_format, signing_key, key_mode, public_path, public_sha,
     trust_path, trust_sha, fingerprint) = _signature_trust(source)
    signing_options = ["-c", f"gpg.format={signing_format}"]
    if signing_key:
        signing_options += ["-c", f"user.signingkey={signing_key}"]
    _git(measured, *signing_options, "commit", "--quiet", "--allow-empty", "-S", "-m",
         "test: snapshot generated source for native read measurement")
    commit = _git(measured, "rev-parse", "HEAD")
    tree = _git(measured, "rev-parse", "HEAD^{tree}")
    assert isinstance(commit, str) and isinstance(tree, str)
    paths, patch_sha = _diff(measured, parent, commit)
    proof: dict[str, Any] = {
        "schema": 1, "path": str(measured), "parent_commit": parent,
        "commit": commit, "tree": tree, "source_tree_sha256": expected_digest,
        "changed_paths": paths, "patch_sha256": patch_sha,
        "signing_format": signing_format,
        "signing_key_mode": key_mode, "signing_key_public_path": public_path,
        "signing_key_sha256": public_sha, "signing_key_fingerprint": fingerprint,
        "allowed_signers_path": trust_path, "allowed_signers_sha256": trust_sha,
    }
    validate(proof, source, target, parent, expected_digest, sanctioned_paths)
    return proof


def validate(proof: dict[str, Any], source: Path, target: Path, parent: str,
             expected_digest: str, sanctioned_paths: set[str]) -> Path:
    """Replay the signed-parent, clean-tree, byte-equality and path checks."""
    source, target = Path(source).absolute(), Path(target).absolute()
    measured = target / SNAPSHOT_DIR
    if (set(proof) != {"schema", "path", "parent_commit", "commit", "tree",
                       "source_tree_sha256", "changed_paths", "patch_sha256",
                       "signing_format", "signing_key_mode", "signing_key_public_path",
                       "signing_key_sha256", "signing_key_fingerprint",
                       "allowed_signers_path", "allowed_signers_sha256"}
            or proof["schema"] != 1 or proof["path"] != str(measured)
            or proof["parent_commit"] != parent or proof["source_tree_sha256"] != expected_digest
            or not isinstance(proof["commit"], str) or not OID.fullmatch(proof["commit"])
            or not isinstance(proof["tree"], str) or not OID.fullmatch(proof["tree"])
            or not isinstance(proof["patch_sha256"], str) or not SHA.fullmatch(proof["patch_sha256"])
            or not isinstance(proof["signing_format"], str)
            or not proof["signing_format"]
            or not isinstance(proof["signing_key_sha256"], str)
            or not SHA.fullmatch(proof["signing_key_sha256"])
            or (proof["signing_format"] == "ssh" and
                (proof["signing_key_mode"] not in {"file", "literal"}
                 or (proof["signing_key_mode"] == "file" and
                     not isinstance(proof["signing_key_public_path"], str))
                 or (proof["signing_key_mode"] == "literal" and
                     proof["signing_key_public_path"] is not None)
                 or not isinstance(proof["signing_key_fingerprint"], str)
                 or not proof["signing_key_fingerprint"].startswith("SHA256:")))
            or (proof["signing_format"] != "ssh" and
                (proof["signing_key_mode"] is not None
                 or proof["signing_key_public_path"] is not None
                 or proof["signing_key_fingerprint"] is not None))
            or (proof["signing_format"] == "ssh" and
                (not isinstance(proof["allowed_signers_path"], str)
                 or not isinstance(proof["allowed_signers_sha256"], str)
                 or not SHA.fullmatch(proof["allowed_signers_sha256"])))
            or (proof["signing_format"] != "ssh" and
                (proof["allowed_signers_path"] is not None
                 or proof["allowed_signers_sha256"] is not None))):
        raise Refused("clean measurement snapshot proof identity is malformed")
    if measured.is_symlink() or not measured.is_dir():
        raise Refused("clean measurement snapshot checkout is absent or symlinked")
    _source_identity(source, parent, expected_digest, sanctioned_paths)
    parents = _git(measured, "rev-list", "--parents", "-n", "1", "HEAD")
    if (_git(measured, "rev-parse", "HEAD") != proof["commit"]
            or parents.split() != [proof["commit"], parent]
            or _git(measured, "rev-parse", "HEAD^{tree}") != proof["tree"]
            or _git(measured, "status", "--porcelain=v1", "--untracked-files=all")):
        raise Refused("clean measurement snapshot commit or checkout changed")
    (signing_format, signing_key, key_mode, public_path, public_sha,
     trust_path, trust_sha, fingerprint) = _signature_trust(source)
    if (proof["signing_format"] != signing_format
            or proof["signing_key_mode"] != key_mode
            or proof["signing_key_public_path"] != public_path
            or proof["signing_key_sha256"] != public_sha
            or proof["signing_key_fingerprint"] != fingerprint
            or proof["allowed_signers_path"] != trust_path
            or proof["allowed_signers_sha256"] != trust_sha):
        raise Refused("clean measurement snapshot SSH trust differs from signed proof")
    verification_options = ["-c", f"gpg.format={signing_format}"]
    if signing_format == "ssh":
        verification_options += ["-c", f"gpg.ssh.allowedSignersFile={trust_path}"]
    _git(measured, *verification_options, "verify-commit", "HEAD")
    if signing_format == "ssh" and _git(measured, *verification_options,
                                          "show", "-s", "--format=%GF", "HEAD") != fingerprint:
        raise Refused("clean measurement snapshot signer differs from bound public key")
    if _signature_trust(source) != (signing_format, signing_key, key_mode, public_path,
                                    public_sha, trust_path, trust_sha, fingerprint):
        raise Refused("clean measurement snapshot SSH trust changed during verification")
    paths, patch_sha = _diff(measured, parent, proof["commit"])
    if (paths != proof["changed_paths"] or patch_sha != proof["patch_sha256"]
            or set(paths) - sanctioned_paths):
        raise Refused("clean measurement snapshot patch or sanctioned paths changed")
    if _file_rows(measured) != _file_rows(source) or source_tree_sha256(measured) != expected_digest:
        raise Refused("clean measurement snapshot bytes differ from generated source")
    return measured
