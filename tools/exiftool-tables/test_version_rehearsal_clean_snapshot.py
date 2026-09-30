"""Small real-Git controls for the generated read measurement snapshot."""
from __future__ import annotations

import os
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import version_rehearsal_clean_snapshot as snapshot
import version_rehearsal_executor as executor
import version_transition_qualification as qualification


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


class CleanSnapshotTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.owned = self.root / "owned"
        self.owned.mkdir()
        self.target = self.root / "target"
        self.target.mkdir()
        subprocess.run(["git", "init", "-q", str(self.owned)], check=True)
        self.key = self.root / "signing-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(self.key)],
                       check=True, capture_output=True)
        self.allowed_signers = self.root / "allowed-signers"
        self.allowed_signers.write_text(f"test@example.invalid {self.key.with_suffix('.pub').read_text().strip()}\n")
        self.git_config = self.root / ".gitconfig"
        self.git_config.write_text(
            "[user]\n\tname = Test Signer\n\temail = test@example.invalid\n"
            "[gpg]\n\tformat = ssh\n"
            f"[user]\n\tsigningkey = {self.key.with_suffix('.pub')}\n"
            f"[gpg \"ssh\"]\n\tallowedSignersFile = {self.allowed_signers}\n"
        )
        env = patch.dict(os.environ, {"HOME": str(self.root),
                                      "GIT_CONFIG_GLOBAL": str(self.git_config)})
        env.start()
        self.addCleanup(env.stop)
        (self.owned / ".exiftool-version").write_text("13.59\n")
        (self.owned / "generated.txt").write_text("old\n")
        (self.owned / "ordinary.py").write_text("print('unchanged')\n")
        git(self.owned, "add", "-A")
        git(self.owned, "commit", "-q", "-m", "base")
        self.parent = git(self.owned, "rev-parse", "HEAD")
        (self.owned / "generated.txt").write_text("new\n")

    def create(self) -> dict:
        return snapshot.create(
            self.owned, self.target, self.parent,
            snapshot.source_tree_sha256(self.owned),
            {"generated.txt", ".exiftool-version"},
        )

    def test_signed_clean_child_has_exact_generated_bytes_and_preserves_owned_checkout(self) -> None:
        before = snapshot.source_tree_sha256(self.owned)
        proof = self.create()
        measured = Path(proof["path"])
        self.assertEqual(proof["parent_commit"], self.parent)
        self.assertEqual(proof["source_tree_sha256"], before)
        self.assertEqual(git(measured, "rev-parse", "HEAD^"), self.parent)
        self.assertEqual(git(measured, "status", "--porcelain=v1"), "")
        self.assertEqual(git(self.owned, "rev-parse", "HEAD"), self.parent)
        self.assertEqual((self.owned / "generated.txt").read_bytes(), (measured / "generated.txt").read_bytes())
        snapshot.validate(proof, self.owned, self.target, self.parent, before,
                          {"generated.txt", ".exiftool-version"})

    def test_repo_local_ssh_trust_survives_isolated_clone_verification(self) -> None:
        # A local Git config is inherited by worktrees, but not by git clone.
        self.git_config.write_text(
            "[user]\n\tname = Test Signer\n\temail = test@example.invalid\n"
        )
        git(self.owned, "config", "--local", "gpg.format", "ssh")
        git(self.owned, "config", "--local", "user.signingkey", str(self.key.with_suffix('.pub')))
        git(self.owned, "config", "--local", "gpg.ssh.allowedSignersFile", str(self.allowed_signers))
        proof = self.create()
        measured = Path(proof["path"])
        self.assertIn("-----BEGIN SSH SIGNATURE-----", git(measured, "cat-file", "-p", "HEAD"))
        self.assertEqual(proof["allowed_signers_path"], str(self.allowed_signers.resolve()))
        self.assertFalse(subprocess.run(
            ["git", "-C", str(measured), "config", "--local", "--get",
             "gpg.ssh.allowedSignersFile"], capture_output=True).returncode == 0)
        snapshot.validate(proof, self.owned, self.target, self.parent,
                          snapshot.source_tree_sha256(self.owned),
                          {"generated.txt", ".exiftool-version"})

    def test_repo_local_ssh_trust_rejects_unauthorized_signer(self) -> None:
        self.git_config.write_text(
            "[user]\n\tname = Test Signer\n\temail = test@example.invalid\n"
            "[gpg]\n\tformat = ssh\n"
            f"[user]\n\tsigningkey = {self.key.with_suffix('.pub')}\n"
        )
        other = self.root / "other-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(other)],
                       check=True, capture_output=True)
        self.allowed_signers.write_text(f"test@example.invalid {other.with_suffix('.pub').read_text().strip()}\n")
        git(self.owned, "config", "--local", "gpg.ssh.allowedSignersFile", str(self.allowed_signers))
        with self.assertRaisesRegex(snapshot.Refused, "verify-commit"):
            self.create()

    def test_relative_repo_local_trust_and_changed_file_replay_refusal(self) -> None:
        self.git_config.write_text(
            "[user]\n\tname = Test Signer\n\temail = test@example.invalid\n"
            "[gpg]\n\tformat = ssh\n"
            f"[user]\n\tsigningkey = {self.key.with_suffix('.pub')}\n"
        )
        git(self.owned, "config", "--local", "gpg.ssh.allowedSignersFile", "../allowed-signers")
        proof = self.create()
        digest = snapshot.source_tree_sha256(self.owned)
        self.assertEqual(proof["allowed_signers_path"], str(self.allowed_signers.resolve()))
        snapshot.validate(proof, self.owned, self.target, self.parent, digest,
                          {"generated.txt", ".exiftool-version"})
        self.allowed_signers.write_text(self.allowed_signers.read_text() + "# changed\n")
        with self.assertRaisesRegex(snapshot.Refused, "trust differs"):
            snapshot.validate(proof, self.owned, self.target, self.parent, digest,
                              {"generated.txt", ".exiftool-version"})

    def test_authorized_key_file_swap_refuses_saved_snapshot_proof(self) -> None:
        other = self.root / "other-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(other)],
                       check=True, capture_output=True)
        active = self.root / "active-key"
        shutil.copyfile(self.key, active)
        active.chmod(0o600)
        shutil.copyfile(self.key.with_suffix(".pub"), active.with_suffix(".pub"))
        self.allowed_signers.write_text(
            f"test@example.invalid {self.key.with_suffix('.pub').read_text().strip()}\n"
            f"test@example.invalid {other.with_suffix('.pub').read_text().strip()}\n"
        )
        git(self.owned, "config", "--local", "gpg.format", "ssh")
        git(self.owned, "config", "--local", "user.signingkey", "../active-key")
        git(self.owned, "config", "--local", "gpg.ssh.allowedSignersFile", str(self.allowed_signers))
        proof = self.create()
        digest = snapshot.source_tree_sha256(self.owned)
        self.assertEqual(proof["signing_key_public_path"], str(active.with_suffix(".pub").resolve()))
        self.assertEqual(proof["signing_key_sha256"],
                         hashlib.sha256(active.with_suffix(".pub").read_bytes()).hexdigest())
        shutil.copyfile(other, active)
        active.chmod(0o600)
        shutil.copyfile(other.with_suffix(".pub"), active.with_suffix(".pub"))
        with self.assertRaisesRegex(snapshot.Refused, "SSH trust differs"):
            snapshot.validate(proof, self.owned, self.target, self.parent, digest,
                              {"generated.txt", ".exiftool-version"})

    def test_literal_ssh_key_identity_uses_public_bytes(self) -> None:
        git(self.owned, "config", "--local", "gpg.format", "ssh")
        literal = self.key.with_suffix(".pub").read_text().strip()
        git(self.owned, "config", "--local", "user.signingkey", "key::" + literal)
        git(self.owned, "config", "--local", "gpg.ssh.allowedSignersFile", str(self.allowed_signers))
        identity = snapshot._signature_trust(self.owned)
        self.assertEqual(identity[2:5], ("literal", None, hashlib.sha256(literal.encode()).hexdigest()))

    def test_public_key_file_without_pub_suffix_uses_its_bytes(self) -> None:
        public = self.root / "signing-public-arbitrary-name"
        shutil.copyfile(self.key.with_suffix(".pub"), public)
        git(self.owned, "config", "--local", "gpg.format", "ssh")
        git(self.owned, "config", "--local", "user.signingkey", "../signing-public-arbitrary-name")
        git(self.owned, "config", "--local", "gpg.ssh.allowedSignersFile", str(self.allowed_signers))
        identity = snapshot._signature_trust(self.owned)
        self.assertEqual(identity[2:5],
                         ("file", str(public.resolve()), hashlib.sha256(public.read_bytes()).hexdigest()))
        agent = subprocess.check_output(["ssh-agent", "-s"], text=True)
        socket = re.search(r"SSH_AUTH_SOCK=([^;]+);", agent)
        pid = re.search(r"SSH_AGENT_PID=([0-9]+);", agent)
        self.assertIsNotNone(socket)
        self.assertIsNotNone(pid)
        agent_env = {"SSH_AUTH_SOCK": socket.group(1), "SSH_AGENT_PID": pid.group(1)}
        try:
            with patch.dict(os.environ, agent_env):
                subprocess.run(["ssh-add", str(self.key)], check=True, capture_output=True)
                proof = self.create()
                self.assertEqual(proof["signing_key_public_path"], str(public.resolve()))
        finally:
            subprocess.run(["ssh-agent", "-k"], env={**os.environ, **agent_env},
                           check=True, capture_output=True)

    def test_public_key_file_with_comments_and_blank_lines_signs(self) -> None:
        public = self.root / "commented-public-key"
        public.write_text("# selected signing key\n\n" + self.key.with_suffix(".pub").read_text())
        git(self.owned, "config", "--local", "user.signingkey", str(public))
        agent = subprocess.check_output(["ssh-agent", "-s"], text=True)
        socket = re.search(r"SSH_AUTH_SOCK=([^;]+);", agent)
        pid = re.search(r"SSH_AGENT_PID=([0-9]+);", agent)
        self.assertIsNotNone(socket)
        self.assertIsNotNone(pid)
        agent_env = {"SSH_AUTH_SOCK": socket.group(1), "SSH_AGENT_PID": pid.group(1)}
        try:
            with patch.dict(os.environ, agent_env):
                subprocess.run(["ssh-add", str(self.key)], check=True, capture_output=True)
                proof = self.create()
                self.assertEqual(proof["signing_key_public_path"], str(public.resolve()))
        finally:
            subprocess.run(["ssh-agent", "-k"], env={**os.environ, **agent_env},
                           check=True, capture_output=True)

    def test_ssh_certificate_fingerprint_matches_openssh(self) -> None:
        ca = self.root / "signing-ca"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(ca)],
                       check=True, capture_output=True)
        subprocess.run(["ssh-keygen", "-q", "-s", str(ca), "-I", "snapshot-test",
                        "-n", "test@example.invalid", "-V", "+1d",
                        str(self.key.with_suffix(".pub"))], check=True, capture_output=True)
        certificate = Path(str(self.key) + "-cert.pub")
        output = subprocess.check_output(["ssh-keygen", "-lf", str(certificate)], text=True)
        expected = re.search(r"SHA256:[A-Za-z0-9+/]+", output)
        self.assertIsNotNone(expected)
        self.assertEqual(snapshot._ssh_fingerprint(certificate.read_bytes()), expected.group())
        self.allowed_signers.write_text(
            f"test@example.invalid cert-authority {ca.with_suffix('.pub').read_text().strip()}\n"
        )
        git(self.owned, "config", "--local", "user.signingkey", str(certificate))
        agent = subprocess.check_output(["ssh-agent", "-s"], text=True)
        socket = re.search(r"SSH_AUTH_SOCK=([^;]+);", agent)
        pid = re.search(r"SSH_AGENT_PID=([0-9]+);", agent)
        self.assertIsNotNone(socket)
        self.assertIsNotNone(pid)
        agent_env = {"SSH_AUTH_SOCK": socket.group(1), "SSH_AGENT_PID": pid.group(1)}
        try:
            with patch.dict(os.environ, agent_env):
                subprocess.run(["ssh-add", str(self.key)], check=True, capture_output=True)
                proof = self.create()
                self.assertEqual(proof["signing_key_fingerprint"], expected.group())
        finally:
            subprocess.run(["ssh-agent", "-k"], env={**os.environ, **agent_env},
                           check=True, capture_output=True)

    def test_implicit_email_is_refused_before_clone(self) -> None:
        self.git_config.write_text(self.git_config.read_text().replace("\temail = test@example.invalid\n", ""))
        with patch.dict(os.environ, {"EMAIL": "other@example.invalid"}):
            with self.assertRaisesRegex(snapshot.Refused, "explicit.*user.email"):
                self.create()
        self.assertFalse((self.target / snapshot.SNAPSHOT_DIR).exists())

    def test_non_ssh_signing_configuration_is_refused_before_clone(self) -> None:
        git(self.owned, "config", "--local", "gpg.format", "openpgp")
        git(self.owned, "config", "--local", "gpg.program", str(self.root / "custom-gpg"))
        with self.assertRaisesRegex(snapshot.Refused, "SSH signing"):
            self.create()
        self.assertFalse((self.target / snapshot.SNAPSHOT_DIR).exists())

    def test_swapped_private_key_with_stale_public_sidecar_refuses(self) -> None:
        other = self.root / "other-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(other)],
                       check=True, capture_output=True)
        active = self.root / "active-key"
        shutil.copyfile(self.key, active)
        active.chmod(0o600)
        shutil.copyfile(self.key.with_suffix(".pub"), active.with_suffix(".pub"))
        self.allowed_signers.write_text(
            f"test@example.invalid {self.key.with_suffix('.pub').read_text().strip()}\n"
            f"test@example.invalid {other.with_suffix('.pub').read_text().strip()}\n"
        )
        git(self.owned, "config", "--local", "gpg.format", "ssh")
        git(self.owned, "config", "--local", "user.signingkey", "../active-key")
        git(self.owned, "config", "--local", "gpg.ssh.allowedSignersFile", str(self.allowed_signers))
        shutil.copyfile(other, active)
        active.chmod(0o600)
        with self.assertRaisesRegex(snapshot.Refused, "private key and public sidecar differ"):
            self.create()

    def test_source_local_ssh_revocation_is_not_lost_in_clone(self) -> None:
        revoked = self.root / "revoked-keys"
        signed = git(self.owned, "commit-tree", "-S", "HEAD^{tree}")
        self.assertEqual(subprocess.run(
            ["git", "-C", str(self.owned), "verify-commit", signed],
            capture_output=True).returncode, 0)
        revoked.write_bytes(self.key.with_suffix(".pub").read_bytes())
        git(self.owned, "config", "--local", "gpg.ssh.revocationFile", "../revoked-keys")
        self.assertNotEqual(subprocess.run(
            ["git", "-C", str(self.owned), "verify-commit", signed],
            capture_output=True).returncode, 0)
        with self.assertRaises(snapshot.Refused):
            self.create()

    def test_revocation_content_change_refuses_saved_proof(self) -> None:
        other = self.root / "other-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(other)],
                       check=True, capture_output=True)
        revoked = self.root / "revoked-keys"
        revoked.write_bytes(other.with_suffix(".pub").read_bytes())
        git(self.owned, "config", "--local", "gpg.ssh.revocationFile", "../revoked-keys")
        proof = self.create()
        digest = snapshot.source_tree_sha256(self.owned)
        revoked.write_bytes(self.key.with_suffix(".pub").read_bytes())
        with self.assertRaisesRegex(snapshot.Refused, "trust differs"):
            snapshot.validate(proof, self.owned, self.target, self.parent, digest,
                              {"generated.txt", ".exiftool-version"})

    def test_source_local_minimum_trust_applies_to_snapshot_verification(self) -> None:
        git(self.owned, "config", "--local", "gpg.minTrustLevel", "fully")
        proof = self.create()
        digest = snapshot.source_tree_sha256(self.owned)
        git(self.owned, "config", "--local", "gpg.minTrustLevel", "ultimate")
        with self.assertRaisesRegex(snapshot.Refused, "trust differs"):
            snapshot.validate(proof, self.owned, self.target, self.parent, digest,
                              {"generated.txt", ".exiftool-version"})
        self.target = self.root / "other-target"
        self.target.mkdir()
        with self.assertRaisesRegex(snapshot.Refused, "verify-commit"):
            self.create()

    def test_default_key_command_is_refused_without_running_it(self) -> None:
        self.git_config.write_text("[user]\n\tname = Test Signer\n\temail = test@example.invalid\n"
                                   "[gpg]\n\tformat = ssh\n")
        marker = self.root / "default-key-ran"
        command = self.root / "default-key-command"
        command.write_text("#!/bin/sh\nprintf ran > '" + str(marker) + "'\n")
        command.chmod(0o700)
        git(self.owned, "config", "--local", "gpg.ssh.defaultKeyCommand", str(command))
        with self.assertRaisesRegex(snapshot.Refused, "explicit.*user.signingkey"):
            self.create()
        self.assertFalse(marker.exists())
        self.assertFalse((self.target / snapshot.SNAPSHOT_DIR).exists())

    def test_escaped_default_key_descendant_is_never_started(self) -> None:
        self.git_config.write_text("[user]\n\tname = Test Signer\n\temail = test@example.invalid\n"
                                   "[gpg]\n\tformat = ssh\n")
        marker = self.root / "escaped-child-marker"
        command = self.root / "detaching-default-key-command"
        command.write_text("#!/bin/sh\nsetsid sh -c 'sleep 1; printf child > "
                           + str(marker) + "' &\nwait\n")
        command.chmod(0o700)
        git(self.owned, "config", "--local", "gpg.ssh.defaultKeyCommand", str(command))
        with self.assertRaisesRegex(snapshot.Refused, "explicit.*user.signingkey"):
            snapshot._signature_trust(self.owned)
        self.assertFalse(marker.exists())

    def test_private_signing_key_without_public_sidecar_can_sign(self) -> None:
        git(self.owned, "config", "--local", "user.signingkey", str(self.key))
        self.key.with_suffix(".pub").unlink()
        git(self.owned, "commit-tree", "-S", "HEAD^{tree}")
        proof = self.create()
        self.assertEqual(proof["signing_key_mode"], "derived")
        other = self.root / "other-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(other)],
                       check=True, capture_output=True)
        shutil.copyfile(other, self.key)
        self.key.chmod(0o600)
        with self.assertRaisesRegex(snapshot.Refused, "trust differs"):
            snapshot.validate(proof, self.owned, self.target, self.parent,
                              snapshot.source_tree_sha256(self.owned),
                              {"generated.txt", ".exiftool-version"})

    def test_encrypted_private_key_without_sidecar_refuses_without_prompt(self) -> None:
        encrypted = self.root / "encrypted-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "secret", "-f", str(encrypted)],
                       check=True, capture_output=True)
        encrypted.with_suffix(".pub").unlink()
        git(self.owned, "config", "--local", "user.signingkey", str(encrypted))
        with self.assertRaisesRegex(snapshot.Refused, "cannot derive SSH public key noninteractively"):
            snapshot._signature_trust(self.owned)

    def test_encrypted_private_key_with_sidecar_refuses_before_clone(self) -> None:
        encrypted = self.root / "encrypted-with-sidecar"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "secret", "-f", str(encrypted)],
                       check=True, capture_output=True)
        git(self.owned, "config", "--local", "user.signingkey", str(encrypted))
        with self.assertRaisesRegex(snapshot.Refused, "cannot derive SSH public key noninteractively"):
            self.create()
        self.assertFalse((self.target / snapshot.SNAPSHOT_DIR).exists())

    def test_default_ssh_executable_is_bound_and_path_swap_refuses(self) -> None:
        proof = self.create()
        original = Path(shutil.which("ssh-keygen")).resolve()
        self.assertEqual(proof["ssh_program_path"], str(original))
        self.assertEqual(proof["ssh_keygen_path"], str(original))
        self.assertEqual(proof["ssh_program_sha256"], hashlib.sha256(original.read_bytes()).hexdigest())
        replacement = self.root / "alternate-bin"
        replacement.mkdir()
        wrapper = replacement / "ssh-keygen"
        wrapper.write_text("#!/bin/sh\nexec '" + str(original) + "' \"$@\"\n")
        wrapper.chmod(0o700)
        with patch.dict(os.environ, {"PATH": str(replacement) + os.pathsep + os.environ["PATH"]}):
            with self.assertRaisesRegex(snapshot.Refused, "SSH trust differs"):
                snapshot.validate(proof, self.owned, self.target, self.parent,
                                  snapshot.source_tree_sha256(self.owned),
                                  {"generated.txt", ".exiftool-version"})

    def test_git_executable_is_bound_before_replay(self) -> None:
        proof = self.create()
        git_path = Path(shutil.which("git")).resolve()
        self.assertEqual(proof["git_path"], str(git_path))
        self.assertEqual(proof["git_sha256"], hashlib.sha256(git_path.read_bytes()).hexdigest())
        alternate = self.root / "alternate-git-bin"
        alternate.mkdir()
        marker = self.root / "replacement-git-invoked"
        wrapper = alternate / "git"
        wrapper.write_text("#!/bin/sh\nprintf invoked > '" + str(marker) + "'\nexec '" +
                           str(git_path) + "' \"$@\"\n")
        wrapper.chmod(0o700)
        with patch.dict(os.environ, {"PATH": str(alternate) + os.pathsep + os.environ["PATH"]}):
            with self.assertRaisesRegex(snapshot.Refused, "Git executable differs"):
                snapshot.validate(proof, self.owned, self.target, self.parent,
                                  snapshot.source_tree_sha256(self.owned),
                                  {"generated.txt", ".exiftool-version"})
        self.assertFalse(marker.exists())

    def test_owned_worktree_preflight_refuses_relative_signing_path(self) -> None:
        git(self.owned, "config", "--local", "user.signingkey", "../signing-key.pub")
        with self.assertRaisesRegex(qualification.Refused, "absolute user.signingkey"):
            qualification._preflight_owned_signing(self.owned, self.root)
        git(self.owned, "config", "--local", "user.signingkey", str(self.key.with_suffix(".pub")))
        git(self.owned, "config", "--local", "gpg.ssh.allowedSignersFile", "../allowed-signers")
        with self.assertRaisesRegex(qualification.Refused, "absolute gpg.ssh.allowedSignersFile"):
            qualification._preflight_owned_signing(self.owned, self.root)
        git(self.owned, "config", "--local", "gpg.ssh.allowedSignersFile", str(self.allowed_signers))
        git(self.owned, "config", "--local", "user.signingkey", str(self.key))
        qualification._preflight_owned_signing(self.owned, self.root)

    def test_owned_preflight_refuses_public_key_without_agent_before_stages(self) -> None:
        orphan = self.root / "public-only.pub"
        orphan.write_bytes(self.key.with_suffix(".pub").read_bytes())
        git(self.owned, "config", "--local", "user.signingkey", str(orphan))
        with patch.dict(os.environ, {"SSH_AUTH_SOCK": ""}):
            with self.assertRaisesRegex(qualification.Refused, "signing probe failed"):
                qualification._preflight_owned_signing(self.owned, self.root)
        self.assertFalse(list(self.root.glob(".task19-signing-probe-*")))

    def test_owned_preflight_refuses_includeif_config_lost_in_worktree(self) -> None:
        included = self.root / "caller-only-config"
        included.write_text(f"[user]\n\tsigningkey = {self.key}\n")
        source_gitdir = Path(git(self.owned, "rev-parse", "--absolute-git-dir")).resolve()
        self.git_config.write_text(
            "[user]\n\tname = Test Signer\n\temail = test@example.invalid\n"
            "[gpg]\n\tformat = ssh\n"
            f"[gpg \"ssh\"]\n\tallowedSignersFile = {self.allowed_signers}\n"
            f"[includeIf \"gitdir:{source_gitdir}\"]\n\tpath = {included}\n"
        )
        self.assertEqual(snapshot._signature_trust(self.owned).effective_key, str(self.key.resolve()))
        with self.assertRaisesRegex(qualification.Refused, "trust changes in the owned checkout"):
            qualification._preflight_owned_signing(self.owned, self.root)
        self.assertFalse(list(self.root.glob(".task19-signing-probe-*")))

    def test_actual_owned_checkout_refuses_includeif_signer_swap(self) -> None:
        other = self.root / "other-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(other)],
                       check=True, capture_output=True)
        self.allowed_signers.write_text(
            f"test@example.invalid {self.key.with_suffix('.pub').read_text().strip()}\n"
            f"test@example.invalid {other.with_suffix('.pub').read_text().strip()}\n")
        source_gitdir = Path(git(self.owned, "rev-parse", "--absolute-git-dir")).resolve()
        included = self.root / "owned-only-config"
        included.write_text(f"[user]\n\tsigningkey = {other}\n")
        with self.git_config.open("a") as config:
            config.write(f"[includeIf \"gitdir:{source_gitdir / 'worktrees' / 'owned-checkout'}\"]\n"
                         f"\tpath = {included}\n")
        preflight = qualification._preflight_owned_signing(self.owned, self.root)
        checkout = self.root / "checkouts" / "owned-checkout"
        checkout.parent.mkdir()
        git(self.owned, "worktree", "add", "--detach", "--no-checkout", str(checkout), "HEAD")
        try:
            self.assertEqual(snapshot._signature_trust(checkout).effective_key, str(other.resolve()))
            with self.assertRaisesRegex(qualification.Refused, "actual owned checkout"):
                qualification._confirm_owned_signing(checkout, preflight)
        finally:
            git(self.owned, "worktree", "remove", "--force", str(checkout))

    def test_repo_local_ssh_program_is_used_by_snapshot(self) -> None:
        marker = self.root / "program-invoked"
        wrapper = self.root / "ssh-keygen-wrapper"
        wrapper.write_text("#!/bin/sh\nprintf called >> '" + str(marker) + "'\nexec '" +
                           str(shutil.which("ssh-keygen")) + "' \"$@\"\n")
        wrapper.chmod(0o700)
        git(self.owned, "config", "--local", "gpg.ssh.program", str(wrapper))
        proof = self.create()
        self.assertTrue(marker.is_file())
        wrapper.write_text(wrapper.read_text() + "# changed\n")
        with self.assertRaisesRegex(snapshot.Refused, "trust differs"):
            snapshot.validate(proof, self.owned, self.target, self.parent,
                              snapshot.source_tree_sha256(self.owned),
                              {"generated.txt", ".exiftool-version"})

    def test_repo_local_author_identity_is_used_by_snapshot(self) -> None:
        self.git_config.write_text("[gpg]\n\tformat = ssh\n"
                                   f"[user]\n\tsigningkey = {self.key.with_suffix('.pub')}\n"
                                   f"[gpg \"ssh\"]\n\tallowedSignersFile = {self.allowed_signers}\n")
        git(self.owned, "config", "--local", "user.name", "Local Signer")
        git(self.owned, "config", "--local", "user.email", "test@example.invalid")
        proof = self.create()
        self.assertIn("Local Signer", git(Path(proof["path"]), "show", "-s", "--format=%an", "HEAD"))
        git(self.owned, "config", "--local", "user.name", "Changed Signer")
        with self.assertRaisesRegex(snapshot.Refused, "trust differs"):
            snapshot.validate(proof, self.owned, self.target, self.parent,
                              snapshot.source_tree_sha256(self.owned),
                              {"generated.txt", ".exiftool-version"})

    def test_unexpected_untracked_and_symlink_inputs_refuse(self) -> None:
        (self.owned / "unexpected.txt").write_text("unsafe")
        with self.assertRaisesRegex(snapshot.Refused, "untracked"):
            self.create()
        (self.owned / "unexpected.txt").unlink()
        (self.owned / "ordinary.py").unlink()
        (self.owned / "ordinary.py").symlink_to("generated.txt")
        with self.assertRaisesRegex(snapshot.Refused, "symlink"):
            self.create()

    def test_unrelated_tracked_change_refuses(self) -> None:
        (self.owned / "ordinary.py").write_text("print('changed')\n")
        with self.assertRaisesRegex(snapshot.Refused, "non-generated"):
            self.create()

    def test_staged_generated_change_refuses(self) -> None:
        git(self.owned, "add", "generated.txt")
        with self.assertRaisesRegex(snapshot.Refused, "index is staged"):
            self.create()

    def test_tampered_snapshot_source_parent_and_proof_refuse(self) -> None:
        proof = self.create()
        measured = Path(proof["path"])
        (measured / "generated.txt").write_text("tampered\n")
        with self.assertRaises(snapshot.Refused):
            snapshot.validate(proof, self.owned, self.target, self.parent,
                              proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})
        shutil.copy2(self.owned / "generated.txt", measured / "generated.txt")
        wrong = dict(proof, parent_commit="f" * 40)
        with self.assertRaises(snapshot.Refused):
            snapshot.validate(wrong, self.owned, self.target, self.parent,
                              proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})

    def test_signed_merge_with_identical_bytes_is_not_a_child_snapshot(self) -> None:
        proof = self.create()
        measured = Path(proof["path"])
        merged = subprocess.run(
            ["git", "-C", str(measured), "commit-tree", "-S", proof["tree"],
             "-p", self.parent, "-p", proof["commit"]], input="merge with same bytes\n",
            text=True, capture_output=True, check=True).stdout.strip()
        git(measured, "update-ref", "HEAD", merged)
        paths, patch_sha = snapshot._diff(measured, self.parent, merged)
        forged = dict(proof, commit=merged, changed_paths=paths, patch_sha256=patch_sha)
        self.assertEqual(snapshot.source_tree_sha256(measured), proof["source_tree_sha256"])
        with self.assertRaisesRegex(snapshot.Refused, "commit or checkout changed"):
            snapshot.validate(forged, self.owned, self.target, self.parent,
                              proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})

    def test_executor_replays_snapshot_and_rejects_rehashed_conformance_tamper(self) -> None:
        proof = self.create()
        report_path = self.root / "conformance.json"
        report = {"instrument": {
            "repo": {"root": proof["path"], "commit": proof["commit"], "tree": proof["tree"],
                     "dirty": False, "dirty_files": [], "dirty_overridden": False},
            "binary": {"path": "/authenticated/oxidex", "sha256": "a" * 64}}}
        report_path.write_text(json.dumps(report))
        result = {"measurement_snapshot": proof, "conformance_report": {
            "path": str(report_path), "sha256": hashlib.sha256(report_path.read_bytes()).hexdigest()},
            "binary": {"path": "/authenticated/oxidex", "sha256": "a" * 64}}
        generation = {"clean_source_before": {"artifact_paths": ["generated.txt"]}}
        source_tree = {"sha256": proof["source_tree_sha256"]}
        with patch.object(executor.artifacts, "inventory", return_value=[]):
            executor._require_read_measurement_snapshot(
                result, generation, self.owned, self.target, self.parent, source_tree)
            report["instrument"]["repo"]["commit"] = "f" * 40
            report_path.write_text(json.dumps(report))
            result["conformance_report"]["sha256"] = hashlib.sha256(report_path.read_bytes()).hexdigest()
            with self.assertRaisesRegex(executor.Refused, "not bound to the signed source"):
                executor._require_read_measurement_snapshot(
                    result, generation, self.owned, self.target, self.parent, source_tree)
            with self.assertRaisesRegex(executor.Refused, "lacks signed clean"):
                executor._require_read_measurement_snapshot(
                    {key: value for key, value in result.items() if key != "measurement_snapshot"},
                    generation, self.owned, self.target, self.parent, source_tree)

    def test_committed_result_side_replays_underlying_signed_source(self) -> None:
        run_dir = self.root / "row" / "before"
        checkout = run_dir / "checkouts" / executor._safe_name("13.59")
        checkout.parent.mkdir(parents=True)
        shutil.move(self.owned, checkout)
        self.owned = checkout
        proof = self.create()
        conformance = self.root / "conformance.json"
        data = {"instrument": {
            "repo": {"root": proof["path"], "commit": proof["commit"], "tree": proof["tree"],
                     "dirty": False, "dirty_files": [], "dirty_overridden": False},
            "binary": {"path": "/authenticated/oxidex", "sha256": "a" * 64}}}
        conformance.write_text(json.dumps(data))
        generation = {"clean_source_before": {"artifact_paths": ["generated.txt"]}}
        binary = self.target / "debug" / "oxidex"
        binary.parent.mkdir()
        binary.write_bytes(b"authenticated")
        binary_row = {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                      "bytes": binary.stat().st_size}
        native_source = self.root / "native"
        native_lib = native_source / "lib"
        native_pm = native_lib / "Image" / "ExifTool.pm"
        native_pm.parent.mkdir(parents=True)
        native_pm.write_text("native")
        native_module = native_pm.parent / "Canon.pm"
        native_module.write_text("native module")
        native_module_sha = hashlib.sha256(native_module.read_bytes()).hexdigest()
        native_perl = self.root / "perl"
        native_perl.write_text("perl")
        native_identity = {"release": "13.59",
                           "source": {"path": str(native_source.resolve())},
                           "lib": {"path": str(native_lib.resolve()),
                                   "exiftool_pm_sha256": hashlib.sha256(native_pm.read_bytes()).hexdigest()},
                           "perl": {"path": str(native_perl.resolve()),
                                    "sha256": hashlib.sha256(native_perl.read_bytes()).hexdigest()}}
        data["instrument"]["binary"] = {"path": str(binary), "sha256": binary_row["sha256"]}
        conformance.write_text(json.dumps(data))
        source_row = {"source_commit": self.parent, "source_tree_sha256": proof["source_tree_sha256"],
                      "generated_artifacts": [], "binary": binary_row,
                      "native_identity": native_identity}
        native_probe = {"probe_sha256": "b" * 64}
        fixture = self.root / "t" / "images" / "fixture.jpg"
        fixture.parent.mkdir(parents=True)
        fixture.write_bytes(b"fixture")
        staged = self.target / "fixture.jpg"
        staged.write_bytes(fixture.read_bytes())
        fixture_sha = hashlib.sha256(fixture.read_bytes()).hexdigest()
        fixture_manifest = self.root / "fixture-manifest.json"
        fixture_manifest.write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest",
            "fixtures": [{"path": str(fixture), "sha256": fixture_sha,
                          "bytes": fixture.stat().st_size}],
        }))
        original = {"read_manifest": str(fixture_manifest),
                    "read_binding": executor._fixture_binding(
                        str(fixture_manifest),
                        kind="oxidex_version_rehearsal_fixture_manifest", jpeg_only=False)}
        union = qualification._freeze_read_union(original, original)
        read_union = qualification._materialize_read_union(union, original, original,
                                                            self.root / "row")
        read = {"state": "measured", "acceptance": "pending_pair_policy",
                "measurement_snapshot": proof, "conformance_report": {
            "path": str(conformance), "sha256": hashlib.sha256(conformance.read_bytes()).hexdigest()},
            **source_row,
            "native_probe_sha256": native_probe["probe_sha256"],
            "fixtures": {"manifest": read_union["manifest"]["path"],
                         "manifest_sha256": read_union["manifest"]["sha256"],
                         "entries": [{"source": str(fixture), "sha256": fixture_sha,
                                      "bytes": fixture.stat().st_size, "corpus_path": str(staged),
                                      "corpus_sha256": fixture_sha, "corpus_bytes": staged.stat().st_size}]}}
        build = dict(source_row)
        reports = run_dir / "reports"
        reports.mkdir(parents=True)
        generate_path, read_path = reports / "generate.json", reports / "read.json"
        build_path = reports / "build.json"
        test_path, write_path = reports / "test.json", reports / "write.json"
        native_path = reports / "native.json"
        commands = run_dir / "raw"
        commands.mkdir()
        release_test, write = {}, {}
        for stage, result in (("generate", generation), ("build", build),
                              ("read", read), ("test", release_test), ("write", write)):
            command_path = commands / f"{stage}.json"
            command_path.write_text('{"status":"ok"}')
            result["raw_report"] = {"path": str(command_path),
                                    "sha256": hashlib.sha256(command_path.read_bytes()).hexdigest()}
        config_path = run_dir / "inputs" / "config.json"
        config_path.parent.mkdir(parents=True)
        bundle = self.root / "verified-input-bundle"
        bundle.mkdir()
        archive_cache = self.root / "archive-cache"
        archive_cache.mkdir()
        (bundle / "locations.json").write_text(json.dumps({
            "kind": "oxidex_version_transition_input_locations",
            "archive_cache": str(archive_cache), "source_root": str(native_source),
        }))
        config = {"execution_source_commit": self.parent,
                  "target_directories": {"13.59": str(self.target)},
                  "verified_input_bundle": str(bundle)}
        config_path.write_text(json.dumps(config))
        journal_path = run_dir / "execution-status.json"

        def update_outer_receipts() -> dict:
            generate_path.write_text(json.dumps(generation))
            build_path.write_text(json.dumps(build))
            test_path.write_text(json.dumps(release_test))
            write_path.write_text(json.dumps(write))
            native_path.write_text(json.dumps(native_probe))
            read_path.write_text(json.dumps(read))
            journal = {"config_sha256": qualification.rehearsal.sha256_json(config),
                       "phase": "complete",
                       "scope": {"read_acceptance": "pending_pair_policy",
                                 "write_acceptance": "passed_per_release"},
                       "releases": {"13.59": {"state": "measured_pending_pair_policy",
                                                "stages": {"read": "measured"}, "reports": {
                           "generate": {"path": "reports/generate.json",
                                        "sha256": qualification.rehearsal.sha256_json(generation)},
                           "build": {"path": "reports/build.json",
                                     "sha256": qualification.rehearsal.sha256_json(build)},
                           "test": {"path": "reports/test.json",
                                    "sha256": qualification.rehearsal.sha256_json(release_test)},
                           "write": {"path": "reports/write.json",
                                     "sha256": qualification.rehearsal.sha256_json(write)},
                           "native": {"path": "reports/native.json",
                                      "sha256": qualification.rehearsal.sha256_json(native_probe)},
                           "read": {"path": "reports/read.json",
                                    "sha256": qualification.rehearsal.sha256_json(read),
                                    "acceptance": "pending_pair_policy"}}}}}
            journal_path.write_text(json.dumps(journal))
            return {"id": "row", "read_union": read_union,
                    "before": {"release": "13.59",
                    "instrument": {"source_commit": self.parent, "native_identity": native_identity,
                                   "binary": binary_row,
                                   "native_probe_sha256": native_probe["probe_sha256"],
                    "read_fixture_manifest": read_union["manifest"]["path"],
                    "read_fixture_manifest_sha256": read_union["manifest"]["sha256"],
                                   "read_fixture_count": 1},
                    "read_report_sha256": qualification.rehearsal.sha256_json(read),
                    "execution_journal_sha256": qualification._sha_file(journal_path)}}

        row = update_outer_receipts()
        self.assertEqual(executor._source_tree(checkout)["sha256"], proof["source_tree_sha256"])
        snapshot.validate(proof, checkout, self.target, self.parent,
                          proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})
        def verify_materialization(actual_run_dir: Path, actual_cache: Path,
                                   actual_source_root: Path) -> tuple:
            self.assertEqual((actual_run_dir, actual_cache, actual_source_root),
                             (run_dir, archive_cache, native_source))
            if hashlib.sha256(native_module.read_bytes()).hexdigest() != native_module_sha:
                raise executor.Refused("native module bytes changed")
            return ()

        with (patch.object(executor.artifacts, "inventory", return_value=[]),
              patch.object(executor, "_verify_inputs", side_effect=verify_materialization),
              patch.object(executor, "_require_read_counts") as read_counts,
              patch.object(executor, "_require_read_raw_maps") as raw_maps):
            reread = qualification._report_for(run_dir, json.loads(journal_path.read_text()),
                                               "13.59", "read")
            self.assertEqual(reread["measurement_snapshot"], proof)
            qualification._replay_committed_read_snapshot(row, "before", self.root)
            read_counts.assert_called_once_with(read)
            self.assertEqual(raw_maps.call_args.args[0], read)
            self.assertEqual(raw_maps.call_args.args[1], read_path)
            native_module.write_text("tampered native module")
            with self.assertRaisesRegex(qualification.Refused, "materialized native source changed"):
                qualification._replay_committed_read_snapshot(row, "before", self.root)
            native_module.write_text("native module")
            binary.write_bytes(b"changed")
            with self.assertRaisesRegex(qualification.Refused, "read measurement replay refused"):
                qualification._replay_committed_read_snapshot(row, "before", self.root)
            binary.write_bytes(b"authenticated")
            data["instrument"]["repo"]["commit"] = "f" * 40
            conformance.write_text(json.dumps(data))
            read["conformance_report"]["sha256"] = hashlib.sha256(conformance.read_bytes()).hexdigest()
            row = update_outer_receipts()
            with self.assertRaisesRegex(qualification.Refused, "read measurement replay refused"):
                qualification._replay_committed_read_snapshot(row, "before", self.root)
        wrong = dict(proof, commit="f" * 40)
        with self.assertRaises(snapshot.Refused):
            snapshot.validate(wrong, self.owned, self.target, self.parent,
                              proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})


if __name__ == "__main__":
    unittest.main()
