#!/usr/bin/env python3
"""Clone and authenticate a staged candidate before Task19 preparation."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import subprocess
from qualification_source import verify_source, REMOTE_TRUSTED_SIGNERS


def verify_staged_checkout(checkout: Path, head: str, signers: Path,
                           attestation: Path | None, bundle_sha256: str) -> dict:
    actual = subprocess.check_output(["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True).strip()
    if actual != head:
        raise ValueError("staged Git bundle does not contain the selected candidate HEAD")
    return verify_source(checkout, head, signers, attestation, bundle_sha256=bundle_sha256)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--head", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--bundle-sha256", required=True)
    parser.add_argument("--signers-sha256", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{40}", args.head):
        raise ValueError("candidate HEAD must be a full Git object ID")
    bundle = Path("/src/repository.bundle")
    signers = Path("/src/maintainer.allowed_signers")
    pin = Path("/src/rust-toolchain.toml")
    reference = Path("/src/reference")
    checkout = Path("/target/checkout")
    if (checkout.exists() or checkout.is_symlink() or not bundle.is_file()
            or not signers.is_file() or not signers.read_text().strip()
            or not reference.is_dir() or not pin.is_file()):
        raise ValueError("staged signed source or approved reference is unavailable")
    for path, expected in ((bundle, args.bundle_sha256), (signers, args.signers_sha256)):
        if not re.fullmatch(r"[0-9a-f]{64}", expected):
            raise ValueError("staged source digest is malformed")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != expected:
            raise ValueError("staged signed source differs from upload")
    subprocess.run(["git", "clone", "-q", str(bundle), str(checkout)], check=True)
    subprocess.run(["git", "-C", str(checkout), "checkout", "-q", "--detach", args.head], check=True)
    if pin.read_bytes() != (checkout / "rust-toolchain.toml").read_bytes():
        raise ValueError("launcher toolchain pin differs from signed candidate")
    attestation = Path("/src/source-attestation") if Path("/src/source-attestation").exists() else None
    verify_staged_checkout(checkout, args.head, REMOTE_TRUSTED_SIGNERS, attestation,
                           args.bundle_sha256)
    subprocess.run(["git", "-C", str(checkout), "config", "gpg.format", "ssh"], check=True)
    subprocess.run(["git", "-C", str(checkout), "config", "gpg.ssh.allowedSignersFile", str(signers)], check=True)
    if attestation is not None:
        os.environ["OXIDEX_QUALIFICATION_SOURCE_ATTESTATION"] = str(attestation)
    env = dict(os.environ, OXIDEX_OPS_DIR="/target/ops", OXIDEX_TARGET_ROOT="/target/targets")
    subprocess.run(["python3", str(checkout / "tools/remote-build/qualification.py"),
                    "--prepare-inputs", "--reference", str(reference), "--output", str(args.output),
                    "--expected-head", args.head], check=True, env=env)


if __name__ == "__main__":
    main()
