"""Admission of an exact qualification source commit.

The detached attestation is issued by the maintainer controller after its
independent GitHub and PGP checks. Spot verifies the maintainer SSH signature;
it does not claim to repeat the controller's PGP or GitHub checks.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

NAMESPACE = "oxidex-qualification-source-v1"
PRINCIPAL = "swackhamer@users.noreply.github.com"
KEY = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKyzTK7pVP3daBqt3csludP15zyIBcTL25JqGYgnRzl/"
FINGERPRINT = "SHA256:187iTiUNnGG/OCFfAG+4wy37COW7tYTHusirU8CfC4Q"
GITHUB_FINGERPRINT = "968479A1AFF927E37D1A566BB5690EEEBB952194"
REPO = "swack-tools/oxidex"
REF = "refs/heads/refactor/tag-machinery"
REMOTE_TRUSTED_SIGNERS = Path("/etc/oxidex-remote-build/qualification-maintainer.allowed_signers")
HEX40 = re.compile(r"[0-9a-f]{40}\Z")
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
STRICT = "maintainer-ssh"
ATTESTED = "github-squash-attested"


def _git(repo: Path, *args: str) -> bytes:
    env = dict(os.environ, GIT_NO_REPLACE_OBJECTS="1")
    return subprocess.check_output(["git", "-C", str(repo), *args], env=env)


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _file_identity(path: Path) -> tuple[int, int, int, int, int]:
    info = path.stat()
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def _pairs(items):
    result = {}
    for key, value in items:
        if key in result:
            raise ValueError("duplicate qualification attestation JSON key")
        result[key] = value
    return result


def _identity(repo: Path, head: str) -> tuple[str, str, str, str]:
    fields = _git(repo, "show", "-s", "--format=%an%x00%ae%x00%cn%x00%ce", head).rstrip(b"\n").decode().split("\x00")
    if len(fields) != 4:
        raise ValueError("malformed source commit identity")
    return tuple(fields)


def _signed_payload(raw_commit: bytes) -> tuple[bytes, bytes]:
    """Extract the one embedded signature and bytes Git actually signed."""
    try:
        header, body = raw_commit.split(b"\n\n", 1)
    except ValueError as error:
        raise ValueError("commit has no header/body boundary") from error
    lines = header.split(b"\n")
    signatures = [i for i, line in enumerate(lines) if line.startswith(b"gpgsig ")]
    if len(signatures) != 1:
        raise ValueError("commit must contain exactly one PGP signature")
    start = signatures[0]
    end = start + 1
    while end < len(lines) and lines[end].startswith(b" "):
        end += 1
    if any(line.startswith(b"gpgsig") for line in lines[:start] + lines[end:]):
        raise ValueError("commit has ambiguous PGP signature headers")
    signature = b"\n".join([lines[start][7:], *(line[1:] for line in lines[start + 1:end])]) + b"\n"
    payload = b"\n".join(lines[:start] + lines[end:]) + b"\n\n" + body
    return signature, payload


def source_facts(repo: Path, head: str) -> dict:
    if not isinstance(head, str) or not HEX40.fullmatch(head):
        raise ValueError("qualification requires a full source commit ID")
    actual = _git(repo, "rev-parse", "--verify", head + "^{commit}").decode().strip()
    if actual != head:
        raise ValueError("qualification source resolves to another commit")
    author, author_email, committer, committer_email = _identity(repo, head)
    return {"repo": REPO, "ref": REF, "head": head,
            "tree": _git(repo, "rev-parse", head + "^{tree}").decode().strip(),
            "raw_commit_sha256": _sha(_git(repo, "cat-file", "commit", head)),
            "author": [author, author_email], "committer": [committer, committer_email],
            "exif_pin_sha256": _sha(_git(repo, "show", head + ":.exiftool-version")),
            "rust_pin_sha256": _sha(_git(repo, "show", head + ":rust-toolchain.toml"))}


def _trusted_key(signers: Path) -> None:
    if not signers.is_file():
        raise ValueError("trusted maintainer signer file is unavailable")
    lines = signers.read_text().splitlines()
    if len(lines) != 1 or lines[0].split()[:3] != [PRINCIPAL, *KEY.split()]:
        raise ValueError("qualification signer is not the approved maintainer key")
    observed = subprocess.check_output(["/usr/bin/ssh-keygen", "-lf", str(signers)], text=True)
    if len(observed.split()) < 2 or observed.split()[1] != FINGERPRINT:
        raise ValueError("qualification maintainer key fingerprint differs")


def verify_source(repo: Path, head: str, signers: Path,
                  attestation: Path | None = None, *, bundle_sha256: str | None = None,
                  run_id: str | None = None) -> dict:
    """Verify strict SSH commit or an independently issued squash attestation.

    `attestation` is a directory with document.json, document.sig and the
    three public issuer evidence files. Only the pinned key can authorize it.
    """
    facts = source_facts(repo, head)
    _trusted_key(signers)
    if attestation is None:
        # Signature status E from git show never passes this strict path.
        env = dict(os.environ, GIT_NO_REPLACE_OBJECTS="1")
        subprocess.run(["git", "-C", str(repo), "-c", "gpg.format=ssh", "-c",
                        "gpg.ssh.allowedSignersFile=" + str(signers), "verify-commit", head],
                       env=env, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        raw_signature, _ = _signed_payload(_git(repo, "cat-file", "commit", head))
        if not raw_signature.startswith(b"-----BEGIN SSH SIGNATURE-----\n"):
            raise ValueError("qualification source does not carry an SSH commit signature")
        status = _git(repo, "-c", "gpg.format=ssh", "-c",
                      "gpg.ssh.allowedSignersFile=" + str(signers), "show", "-s",
                      "--format=%G?%x00%GS%x00%GF", head).strip().decode().split("\x00")
        if (facts["author"] != ["swackhamer", PRINCIPAL]
                or facts["committer"] != ["swackhamer", PRINCIPAL]
                or status != ["G", PRINCIPAL, FINGERPRINT]):
            raise ValueError("qualification candidate lacks verified signed maintainer identity")
        return {"mode": STRICT, **facts, "principal": PRINCIPAL,
                "trusted_key_fingerprint": FINGERPRINT}
    if attestation.is_symlink() or not attestation.is_dir():
        raise ValueError("source attestation directory is unavailable")
    document_path, signature = attestation / "document.json", attestation / "document.sig"
    if any(path.is_symlink() or not path.is_file() for path in (document_path, signature)):
        raise ValueError("source attestation document or signature is unavailable")
    frozen_paths = (document_path, signature,
                    *(attestation / name for name in ("github-commit.json", "pgp-proof.txt", "integration-ref.json")))
    before = {path: _file_identity(path) for path in frozen_paths}
    raw = document_path.read_bytes()
    signature_bytes = signature.read_bytes()
    if len(raw) > 65536 or len(signature_bytes) > 16384:
        raise ValueError("source attestation is oversized")
    doc = json.loads(raw, object_pairs_hook=_pairs)
    expected_keys = set(facts) | {"schema", "mode", "namespace", "principal",
        "trusted_key_fingerprint", "github_signature_fingerprint", "pr_number",
        "merge_sha", "observed_integration_head", "bundle_sha256", "run_id",
        "evidence_sha256"}
    if not isinstance(doc, dict) or set(doc) != expected_keys or doc["schema"] != 1:
        raise ValueError("source attestation schema differs")
    if any(doc[key] != value for key, value in facts.items()):
        raise ValueError("source attestation does not bind exact commit, tree or pins")
    if (doc["mode"] != ATTESTED or doc["namespace"] != NAMESPACE
            or doc["principal"] != PRINCIPAL or doc["trusted_key_fingerprint"] != FINGERPRINT
            or doc["github_signature_fingerprint"] != GITHUB_FINGERPRINT
            or doc["author"] != ["swackhamer", PRINCIPAL]
            or doc["committer"] != ["GitHub", "noreply@github.com"]
            or type(doc["pr_number"]) is not int or doc["pr_number"] <= 0
            or doc["merge_sha"] != head or doc["observed_integration_head"] != head):
        raise ValueError("source attestation issuer or GitHub squash identity differs")
    if (not isinstance(doc["bundle_sha256"], str) or not HEX64.fullmatch(doc["bundle_sha256"])
            or not isinstance(doc["run_id"], str)
            or not re.fullmatch(r"qualification-" + head[:12] + r"-[0-9a-f]{32}", doc["run_id"])):
        raise ValueError("source attestation transfer identity is malformed")
    if bundle_sha256 is not None and doc["bundle_sha256"] != bundle_sha256:
        raise ValueError("source attestation bundle digest differs")
    if run_id is not None and doc["run_id"] != run_id:
        raise ValueError("source attestation run identity differs")
    names = {"github-commit.json", "pgp-proof.txt", "integration-ref.json"}
    evidence = doc["evidence_sha256"]
    if not isinstance(evidence, dict) or set(evidence) != names:
        raise ValueError("source attestation evidence schema differs")
    for name in names:
        path = attestation / name
        if (path.is_symlink() or not path.is_file() or not isinstance(evidence[name], str)
                or not HEX64.fullmatch(evidence[name]) or _sha(path.read_bytes()) != evidence[name]):
            raise ValueError("source attestation evidence bytes differ")
    allowed = (PRINCIPAL + ' namespaces="' + NAMESPACE + '" ' + KEY + "\n").encode()
    # Verify using the built-in pinned key, never a key supplied with the tar.
    # OpenSSH accepts a regular allowed-signers file, so use a pipe via stdin
    # only for the immutable document and a temporary trusted-key file.
    import tempfile
    with tempfile.TemporaryDirectory() as temporary:
        key_file = Path(temporary) / "allowed_signers"
        signature_file = Path(temporary) / "document.sig"
        key_file.write_bytes(allowed)
        signature_file.write_bytes(signature_bytes)
        subprocess.run(["ssh-keygen", "-Y", "verify", "-f", str(key_file), "-I", PRINCIPAL,
                        "-n", NAMESPACE, "-s", str(signature_file)], input=raw, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if (any(_file_identity(path) != before[path] for path in frozen_paths)
            or document_path.read_bytes() != raw or signature.read_bytes() != signature_bytes
            or any(_sha_file(attestation / name) != evidence[name] for name in names)):
        raise ValueError("source attestation changed while verifying signature")
    return {"mode": ATTESTED, **facts, "principal": PRINCIPAL,
            "trusted_key_fingerprint": FINGERPRINT,
            "attestation_sha256": _sha(raw), "attestation_signature_sha256": _sha(signature_bytes),
            "github_signature_fingerprint": GITHUB_FINGERPRINT,
            "pr_number": doc["pr_number"], "merge_sha": head,
            "observed_integration_head": head, "bundle_sha256": doc["bundle_sha256"],
            "run_id": doc["run_id"], "evidence_sha256": evidence,
            "source_identity_authority": "remote-verified maintainer SSH attestation of controller PGP and GitHub evidence"}


def issue_github_squash_document(repo: Path, head: str, pr_number: int,
                                 bundle: Path, run_id: str, gpg_home: Path,
                                 destination: Path) -> Path:
    """Construct an unsigned document only after fresh API and local PGP proof.

    The maintainer controller must review the evidence and sign document.json
    separately with its already approved SSH key. This function never sees a
    signing private key. A caller-supplied `verified` JSON cannot authorize it.
    """
    if (type(pr_number) is not int or pr_number <= 0 or destination.exists()
            or not gpg_home.is_dir() or gpg_home.is_symlink() or not bundle.is_file()
            or not re.fullmatch(r"qualification-" + head[:12] + r"-[0-9a-f]{32}", run_id)):
        raise ValueError("source attestation issuer inputs are malformed")
    if (_git(repo, "rev-parse", "HEAD").decode().strip() != head
            or _git(repo, "status", "--porcelain")):
        raise ValueError("issuer checkout is not the exact clean integration commit")
    facts = source_facts(repo, head)
    if facts["author"] != ["swackhamer", PRINCIPAL] or facts["committer"] != ["GitHub", "noreply@github.com"]:
        raise ValueError("source is not a GitHub squash integration commit")
    env = dict(os.environ, GNUPGHOME=str(gpg_home), GIT_NO_REPLACE_OBJECTS="1",
               GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null")
    proof = subprocess.run(["git", "-C", str(repo), "-c", "gpg.format=openpgp",
                            "-c", "gpg.program=gpg", "verify-commit", "--raw", head],
                           env=env, capture_output=True, text=True, check=True)
    proof_text = proof.stdout + proof.stderr
    proof_lines = proof_text.splitlines()
    valid_fingerprints = [line.split()[2] for line in proof_lines
                          if line.startswith("[GNUPG:] VALIDSIG ") and len(line.split()) > 2]
    if (not any(line.startswith("[GNUPG:] GOODSIG ") for line in proof_lines)
            or valid_fingerprints != [GITHUB_FINGERPRINT]
            or not any(line.startswith("[GNUPG:] TRUST_FULLY") for line in proof_lines)):
        raise ValueError("independent PGP proof lacks approved full GitHub fingerprint")
    def api(path: str) -> tuple[dict, bytes]:
        raw = subprocess.check_output(["gh", "api", "repos/" + REPO + "/" + path])
        return json.loads(raw, object_pairs_hook=_pairs), raw
    commit, commit_raw = api("commits/" + head)
    pull, _ = api("pulls/" + str(pr_number))
    ref, ref_raw = api("git/ref/heads/refactor/tag-machinery")
    verification = commit.get("commit", {}).get("verification", {})
    raw_signature, raw_payload = _signed_payload(_git(repo, "cat-file", "commit", head))
    if (commit.get("sha") != head or commit.get("commit", {}).get("tree", {}).get("sha") != facts["tree"]
            or verification.get("verified") is not True or verification.get("reason") != "valid"
            or not isinstance(verification.get("signature"), str)
            or not isinstance(verification.get("payload"), str)
            or verification["signature"].encode().rstrip(b"\n") != raw_signature.rstrip(b"\n")
            or verification["payload"].encode() != raw_payload
            or pull.get("number") != pr_number or pull.get("merged_at") is None
            or pull.get("merge_commit_sha") != head or pull.get("base", {}).get("ref") != "refactor/tag-machinery"
            or ref.get("object", {}).get("sha") != head):
        raise ValueError("fresh GitHub commit, merged PR or integration ref differs")
    evidence = {"github-commit.json": commit_raw, "pgp-proof.txt": proof_text.encode(),
                "integration-ref.json": ref_raw}
    bundle_sha = _sha_file(bundle)
    document = {"schema": 1, "mode": ATTESTED, "namespace": NAMESPACE,
                **facts, "principal": PRINCIPAL, "trusted_key_fingerprint": FINGERPRINT,
                "github_signature_fingerprint": GITHUB_FINGERPRINT,
                "pr_number": pr_number, "merge_sha": head,
                "observed_integration_head": head, "bundle_sha256": bundle_sha,
                "run_id": run_id,
                "evidence_sha256": {name: _sha(data) for name, data in evidence.items()}}
    destination.mkdir(parents=True)
    shutil.copyfile(bundle, destination / "repository.bundle")
    if _sha_file(destination / "repository.bundle") != bundle_sha:
        raise ValueError("source bundle changed while issuing attestation")
    for name, data in evidence.items():
        (destination / name).write_bytes(data)
    (destination / "document.json").write_bytes((json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode())
    return destination / "document.json"


def main() -> None:
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("issue", choices=("issue",))
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--pr-number", type=int, required=True)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--gpg-home", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(issue_github_squash_document(args.repo, args.head, args.pr_number,
          args.bundle, args.run_id, args.gpg_home, args.output))


if __name__ == "__main__":
    main()
