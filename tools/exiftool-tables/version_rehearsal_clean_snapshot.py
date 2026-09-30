"""Commit-backed clean view of an owned, freshly regenerated checkout.

The rehearsal keeps its execution checkout at the original source HEAD. Its
generated files are therefore legitimately dirty, while conformance receipts
require a clean Git identity. This module copies *those exact bytes* to a
separate signed child commit for read measurement; it never edits the owned
execution checkout or relaxes conformance's clean-source rule.

The snapshot accepts only an explicitly configured SSH signing key and
user.name/user.email. OpenPGP selectors and defaultKeyCommand are refused
because their effective signer cannot be rebound from a stable local file.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
from typing import Any, NamedTuple


SNAPSHOT_DIR = "measurement-source"
OID = re.compile(r"[0-9a-f]{40}\Z")
SHA = re.compile(r"[0-9a-f]{64}\Z")
EXCLUDED_DIRS = {".git", "target", "__pycache__"}


class Refused(ValueError):
    pass


class SigningTrust(NamedTuple):
    signing_format: str
    effective_key: str
    key_mode: str | None
    public_path: str | None
    public_sha: str
    allowed_path: str | None
    allowed_sha: str | None
    fingerprint: str | None
    revocation_path: str | None
    revocation_sha: str | None
    default_command_sha: str | None
    program_path: str | None
    program_sha: str | None
    keygen_path: str
    keygen_sha: str
    author_name: str
    author_email: str
    minimum_trust: str


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


def _ssh_keygen() -> tuple[str, str]:
    executable = shutil.which("ssh-keygen")
    if not executable:
        raise Refused("clean measurement snapshot ssh-keygen is absent")
    path = Path(executable).resolve()
    if not path.is_file():
        raise Refused("clean measurement snapshot ssh-keygen is absent")
    return str(path), hashlib.sha256(path.read_bytes()).hexdigest()


def _ssh_fingerprint(public_key: bytes, keygen_path: str | None = None) -> str:
    """Use OpenSSH's certificate-aware identity, as Git's %GF does."""
    try:
        result = subprocess.run([keygen_path or _ssh_keygen()[0], "-E", "sha256", "-lf", "/dev/stdin"],
                                input=public_key, capture_output=True, timeout=5, check=False)
    except subprocess.TimeoutExpired as error:
        raise Refused("clean measurement snapshot SSH public-key fingerprint timed out") from error
    lines = result.stdout.decode("utf-8", errors="replace").splitlines()
    if result.returncode != 0 or len(lines) != 1:
        raise Refused("clean measurement snapshot SSH public key is malformed")
    match = re.search(r"(?:^|\s)(SHA256:[A-Za-z0-9+/]+)(?:\s|$)", lines[0])
    if match is None:
        raise Refused("clean measurement snapshot SSH public key has no SHA256 fingerprint")
    return match.group(1)


def _public_key_line(contents: bytes) -> bytes | None:
    """Recognize one public key despite leading comments or blank lines."""
    try:
        lines = [line.strip() for line in contents.decode("utf-8").splitlines()
                 if line.strip() and not line.lstrip().startswith("#")]
    except UnicodeError:
        return None
    if len(lines) != 1 or not lines[0].startswith(("ssh-", "ecdsa-", "sk-")):
        return None
    return lines[0].encode("utf-8") + b"\n"


def _configured_file(source: Path, option: str, label: str) -> tuple[str | None, str | None]:
    configured = _git(source, "config", "--path", "--get", "--default=", option)
    assert isinstance(configured, str)
    if not configured:
        return None, None
    path = Path(configured).expanduser()
    if not path.is_absolute():
        path = source / path
    if path.is_symlink() or not path.is_file():
        raise Refused(f"clean measurement snapshot {label} is absent or not regular")
    path = path.resolve()
    return str(path), hashlib.sha256(path.read_bytes()).hexdigest()


def _ssh_program(source: Path, default_path: str) -> tuple[str, str]:
    configured = _git(source, "config", "--get", "--default=", "gpg.ssh.program")
    assert isinstance(configured, str)
    if not configured:
        candidate = Path(default_path)
        return str(candidate), hashlib.sha256(candidate.read_bytes()).hexdigest()
    candidate = Path(configured).expanduser()
    if not candidate.is_absolute():
        candidate = source / candidate if "/" in configured else Path(shutil.which(configured) or "")
    if not candidate.is_file():
        raise Refused("clean measurement snapshot SSH program is absent")
    candidate = candidate.resolve()
    return str(candidate), hashlib.sha256(candidate.read_bytes()).hexdigest()


def _signature_trust(source: Path) -> SigningTrust:
    """Resolve effective source signing and trust configuration before cloning."""
    signing_format = _git(source, "config", "--get", "--default=openpgp", "gpg.format")
    assert isinstance(signing_format, str)
    signing_key = _git(source, "config", "--get", "--default=", "user.signingkey")
    assert isinstance(signing_key, str)
    author_name = _git(source, "config", "--get", "--default=", "user.name")
    author_email = _git(source, "config", "--get", "--default=", "user.email")
    minimum_trust = _git(source, "config", "--get", "--default=", "gpg.minTrustLevel")
    assert isinstance(author_name, str) and isinstance(author_email, str) and isinstance(minimum_trust, str)
    if signing_format != "ssh":
        raise Refused("clean measurement snapshot requires SSH signing; non-SSH signing is unsupported")
    if not author_name:
        raise Refused("clean measurement snapshot requires explicit user.name")
    if not author_email:
        raise Refused("clean measurement snapshot requires explicit user.email")
    if not signing_key:
        # Git's defaultKeyCommand can launch arbitrary descendants and select
        # a different key on replay. Qualification requires a fixed key.
        raise Refused("clean measurement snapshot requires explicit user.signingkey")
    keygen_path, keygen_sha = _ssh_keygen()
    if signing_key.startswith("key::") or signing_key.startswith(("ssh-", "ecdsa-", "sk-")):
        literal = signing_key.removeprefix("key::").strip()
        if not literal:
            raise Refused("clean measurement snapshot literal SSH signing key is empty")
        key_mode, public_path = "literal", None
        public_sha = hashlib.sha256(literal.encode()).hexdigest()
        fingerprint = _ssh_fingerprint(literal.encode(), keygen_path)
        effective_key = "key::" + literal
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
        public_key = (key_path if _public_key_line(key_bytes) is not None
                      else Path(str(key_path) + ".pub"))
        if public_key == key_path:
            public_bytes = key_bytes
            public_path = str(key_path)
        else:
            # Derive even when a .pub sidecar exists: an encrypted private key
            # would otherwise reach git commit -S and prompt after the build.
            try:
                derived = subprocess.run([keygen_path, "-y", "-P", "", "-f", str(key_path)],
                                         stdin=subprocess.DEVNULL, capture_output=True,
                                         timeout=5, check=False)
            except subprocess.TimeoutExpired as error:
                raise Refused("clean measurement snapshot SSH public-key derivation timed out") from error
            if derived.returncode != 0:
                raise Refused("clean measurement snapshot cannot derive SSH public key noninteractively")
            if public_key.is_file():
                public_bytes = public_key.read_bytes()
                public_path = str(public_key.resolve())
                sidecar = _public_key_line(public_bytes)
                emitted = _public_key_line(derived.stdout)
                if sidecar is None or emitted is None or sidecar.split()[:2] != emitted.split()[:2]:
                    raise Refused("clean measurement snapshot SSH private key and public sidecar differ")
            else:
                public_bytes, public_path = derived.stdout, None
        key_mode = "file" if public_path is not None else "derived"
        public_sha = hashlib.sha256(public_bytes).hexdigest()
        public_line = _public_key_line(public_bytes)
        if public_line is None:
            raise Refused("clean measurement snapshot SSH public key is malformed")
        fingerprint = _ssh_fingerprint(public_line, keygen_path)
        effective_key = str(key_path)
    allowed_path, allowed_sha = _configured_file(source, "gpg.ssh.allowedSignersFile", "allowed signers file")
    if allowed_path is None:
        raise Refused("clean measurement snapshot allowed signers file is absent")
    revocation_path, revocation_sha = _configured_file(source, "gpg.ssh.revocationFile", "SSH revocation file")
    program_path, program_sha = _ssh_program(source, keygen_path)
    return SigningTrust(signing_format, effective_key, key_mode, public_path, public_sha,
                        allowed_path, allowed_sha, fingerprint, revocation_path, revocation_sha,
                        None, program_path, program_sha, keygen_path, keygen_sha, author_name, author_email,
                        minimum_trust)


def _signing_options(trust: SigningTrust) -> list[str]:
    options = ["-c", f"gpg.format={trust.signing_format}"]
    for name, value in (("user.signingkey", trust.effective_key),
                        ("user.name", trust.author_name), ("user.email", trust.author_email),
                        ("gpg.ssh.program", trust.program_path)):
        if value:
            options += ["-c", f"{name}={value}"]
    return options


def _verification_options(trust: SigningTrust) -> list[str]:
    options = ["-c", f"gpg.format={trust.signing_format}"]
    if trust.minimum_trust:
        options += ["-c", f"gpg.minTrustLevel={trust.minimum_trust}"]
    if trust.signing_format == "ssh":
        options += ["-c", f"gpg.ssh.allowedSignersFile={trust.allowed_path}"]
        if trust.revocation_path:
            options += ["-c", f"gpg.ssh.revocationFile={trust.revocation_path}"]
        if trust.program_path:
            options += ["-c", f"gpg.ssh.program={trust.program_path}"]
    return options


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
    trust = _signature_trust(source)
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
    _git(measured, *_signing_options(trust), "commit", "--quiet", "--allow-empty", "-S", "-m",
         "test: snapshot generated source for native read measurement")
    commit = _git(measured, "rev-parse", "HEAD")
    tree = _git(measured, "rev-parse", "HEAD^{tree}")
    assert isinstance(commit, str) and isinstance(tree, str)
    paths, patch_sha = _diff(measured, parent, commit)
    proof: dict[str, Any] = {
        "schema": 3, "path": str(measured), "parent_commit": parent,
        "commit": commit, "tree": tree, "source_tree_sha256": expected_digest,
        "changed_paths": paths, "patch_sha256": patch_sha,
        "signing_format": trust.signing_format,
        "signing_key_mode": trust.key_mode, "signing_key_public_path": trust.public_path,
        "signing_key_sha256": trust.public_sha, "signing_key_fingerprint": trust.fingerprint,
        "allowed_signers_path": trust.allowed_path, "allowed_signers_sha256": trust.allowed_sha,
        "revocation_path": trust.revocation_path, "revocation_sha256": trust.revocation_sha,
        "default_key_command_sha256": trust.default_command_sha,
        "ssh_program_path": trust.program_path, "ssh_program_sha256": trust.program_sha,
        "ssh_keygen_path": trust.keygen_path, "ssh_keygen_sha256": trust.keygen_sha,
        "author_identity_sha256": hashlib.sha256(
            json.dumps([trust.author_name, trust.author_email]).encode()).hexdigest(),
        "minimum_trust_level": trust.minimum_trust,
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
                       "allowed_signers_path", "allowed_signers_sha256",
                       "revocation_path", "revocation_sha256", "default_key_command_sha256",
                       "ssh_program_path", "ssh_program_sha256",
                       "ssh_keygen_path", "ssh_keygen_sha256", "author_identity_sha256",
                       "minimum_trust_level"}
            or proof["schema"] != 3 or proof["path"] != str(measured)
            or proof["parent_commit"] != parent or proof["source_tree_sha256"] != expected_digest
            or not isinstance(proof["commit"], str) or not OID.fullmatch(proof["commit"])
            or not isinstance(proof["tree"], str) or not OID.fullmatch(proof["tree"])
            or not isinstance(proof["patch_sha256"], str) or not SHA.fullmatch(proof["patch_sha256"])
            or not isinstance(proof["signing_format"], str)
            or not proof["signing_format"]
            or not isinstance(proof["signing_key_sha256"], str)
            or not SHA.fullmatch(proof["signing_key_sha256"])
            or (proof["signing_format"] == "ssh" and
                (proof["signing_key_mode"] not in {"file", "derived", "literal"}
                 or (proof["signing_key_mode"] == "file" and
                     not isinstance(proof["signing_key_public_path"], str))
                 or (proof["signing_key_mode"] in {"literal", "derived"} and
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
                 or proof["allowed_signers_sha256"] is not None))
            or any(proof[path] is not None and (not isinstance(proof[path], str) or
                       not isinstance(proof[sha], str) or not SHA.fullmatch(proof[sha]))
                   for path, sha in (("revocation_path", "revocation_sha256"),
                                    ("ssh_program_path", "ssh_program_sha256"),
                                    ("ssh_keygen_path", "ssh_keygen_sha256")))
            or any(proof[path] is None and proof[sha] is not None
                   for path, sha in (("revocation_path", "revocation_sha256"),
                                    ("ssh_program_path", "ssh_program_sha256"),
                                    ("ssh_keygen_path", "ssh_keygen_sha256")))
            or not isinstance(proof["ssh_keygen_path"], str)
            or not isinstance(proof["ssh_keygen_sha256"], str)
            or not SHA.fullmatch(proof["ssh_keygen_sha256"])
            or (proof["default_key_command_sha256"] is not None and
                (not isinstance(proof["default_key_command_sha256"], str) or
                 not SHA.fullmatch(proof["default_key_command_sha256"])))
            or not isinstance(proof["author_identity_sha256"], str)
            or not SHA.fullmatch(proof["author_identity_sha256"])
            or not isinstance(proof["minimum_trust_level"], str)):
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
    trust = _signature_trust(source)
    if (proof["signing_format"] != trust.signing_format
            or proof["signing_key_mode"] != trust.key_mode
            or proof["signing_key_public_path"] != trust.public_path
            or proof["signing_key_sha256"] != trust.public_sha
            or proof["signing_key_fingerprint"] != trust.fingerprint
            or proof["allowed_signers_path"] != trust.allowed_path
            or proof["allowed_signers_sha256"] != trust.allowed_sha
            or proof["revocation_path"] != trust.revocation_path
            or proof["revocation_sha256"] != trust.revocation_sha
            or proof["default_key_command_sha256"] != trust.default_command_sha
            or proof["ssh_program_path"] != trust.program_path
            or proof["ssh_program_sha256"] != trust.program_sha
            or proof["ssh_keygen_path"] != trust.keygen_path
            or proof["ssh_keygen_sha256"] != trust.keygen_sha
            or proof["author_identity_sha256"] != hashlib.sha256(
                json.dumps([trust.author_name, trust.author_email]).encode()).hexdigest()
            or proof["minimum_trust_level"] != trust.minimum_trust):
        raise Refused("clean measurement snapshot SSH trust differs from signed proof")
    verification_options = _verification_options(trust)
    _git(measured, *verification_options, "verify-commit", "HEAD")
    if trust.signing_format == "ssh" and _git(measured, *verification_options,
                                               "show", "-s", "--format=%GF", "HEAD") != trust.fingerprint:
        raise Refused("clean measurement snapshot signer differs from bound public key")
    if _signature_trust(source) != trust:
        raise Refused("clean measurement snapshot SSH trust changed during verification")
    paths, patch_sha = _diff(measured, parent, proof["commit"])
    if (paths != proof["changed_paths"] or patch_sha != proof["patch_sha256"]
            or set(paths) - sanctioned_paths):
        raise Refused("clean measurement snapshot patch or sanctioned paths changed")
    if _file_rows(measured) != _file_rows(source) or source_tree_sha256(measured) != expected_digest:
        raise Refused("clean measurement snapshot bytes differ from generated source")
    return measured
