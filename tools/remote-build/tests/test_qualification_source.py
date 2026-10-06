"""Cryptographic and object-binding controls for qualification admission."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import shutil
import tarfile
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import qualification_source as source
import qualification_bootstrap as bootstrap
import qualification
import qualification_transport as transport
from types import SimpleNamespace


def run(*args, cwd=None, env=None):
    return subprocess.check_output(args, cwd=cwd, env=env, stderr=subprocess.DEVNULL).decode().strip()


class SourceAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temp = TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.repo = root / "repo"
        self.repo.mkdir()
        run("git", "init", "-q", str(self.repo))
        (self.repo / ".exiftool-version").write_text("13.59\n")
        (self.repo / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.99.0"\n')
        run("git", "add", ".", cwd=self.repo)
        env = dict(os.environ, GIT_AUTHOR_NAME="swackhamer",
                   GIT_AUTHOR_EMAIL=source.PRINCIPAL,
                   GIT_COMMITTER_NAME="GitHub", GIT_COMMITTER_EMAIL="noreply@github.com")
        run("git", "-c", "commit.gpgsign=false", "commit", "-qm", "squash", cwd=self.repo, env=env)
        self.head = run("git", "rev-parse", "HEAD", cwd=self.repo)
        self.key = root / "id_ed25519"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(self.key)], check=True)
        self.public = (root / "id_ed25519.pub").read_text().split()
        self.signers = root / "allowed_signers"
        self.signers.write_text(source.PRINCIPAL + " " + " ".join(self.public) + "\n")
        self.fingerprint = run("ssh-keygen", "-lf", str(self.signers)).split()[1]
        self.attestation = root / "attestation"
        self.attestation.mkdir()
        self.run_id = "qualification-" + self.head[:12] + "-" + "b" * 32
        self.bundle_sha = "a" * 64
        for name in ("github-commit.json", "pgp-proof.txt", "integration-ref.json"):
            (self.attestation / name).write_text(name)
        self.doc = {"schema": 1, "mode": source.ATTESTED,
                    "namespace": source.NAMESPACE, **source.source_facts(self.repo, self.head),
                    "principal": source.PRINCIPAL, "trusted_key_fingerprint": self.fingerprint,
                    "github_signature_fingerprint": source.GITHUB_FINGERPRINT,
                    "pr_number": 1065, "merge_sha": self.head,
                    "observed_integration_head": self.head,
                    "bundle_sha256": self.bundle_sha, "run_id": self.run_id,
                    "evidence_sha256": {name: hashlib.sha256((self.attestation / name).read_bytes()).hexdigest()
                                         for name in ("github-commit.json", "pgp-proof.txt", "integration-ref.json")}}
        self.patches = [patch.object(source, "KEY", " ".join(self.public[:2])),
                        patch.object(source, "FINGERPRINT", self.fingerprint)]
        for item in self.patches:
            item.start()
            self.addCleanup(item.stop)

    def sign(self):
        document = self.attestation / "document.json"
        document.write_text(json.dumps(self.doc, sort_keys=True) + "\n")
        subprocess.run(["ssh-keygen", "-Y", "sign", "-f", str(self.key),
                        "-n", source.NAMESPACE, str(document)], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        document.with_suffix(".json.sig").replace(self.attestation / "document.sig")

    def verify(self):
        return source.verify_source(self.repo, self.head, self.signers, self.attestation,
                                    bundle_sha256=self.bundle_sha, run_id=self.run_id)

    def test_attested_commit_requires_real_ssh_signature_and_exact_object(self):
        self.sign()
        self.assertEqual(self.verify()["mode"], source.ATTESTED)
        for key, changed in (("head", "0" * 40), ("tree", "0" * 40),
                             ("raw_commit_sha256", "0" * 64),
                             ("exif_pin_sha256", "0" * 64),
                             ("rust_pin_sha256", "0" * 64),
                             ("repo", "other/repo"), ("ref", "refs/heads/main"),
                             ("bundle_sha256", "0" * 64),
                             ("namespace", "git"), ("principal", "spot-qualification@local"),
                             ("trusted_key_fingerprint", "other"),
                             ("github_signature_fingerprint", "0" * 40),
                             ("merge_sha", "0" * 40), ("observed_integration_head", "0" * 40)):
            with self.subTest(key=key):
                original = self.doc[key]
                self.doc[key] = changed
                (self.attestation / "document.json").write_text(json.dumps(self.doc))
                with self.assertRaises(ValueError):
                    self.verify()
                self.doc[key] = original
        self.sign()
        (self.attestation / "pgp-proof.txt").write_text("changed")
        with self.assertRaisesRegex(ValueError, "evidence bytes differ"):
            self.verify()

    def test_missing_or_duplicate_attestation_never_falls_back_to_unsigned_commit(self):
        with self.assertRaises(subprocess.CalledProcessError):
            source.verify_source(self.repo, self.head, self.signers)
        self.sign()
        (self.attestation / "document.sig").unlink()
        with self.assertRaises(ValueError):
            self.verify()
        self.sign()
        (self.attestation / "document.json").write_text('{"schema":1,"schema":1}')
        with self.assertRaisesRegex(ValueError, "duplicate"):
            self.verify()

    def test_signature_path_swap_during_crypto_verification_is_rejected(self):
        self.sign()
        original_run = subprocess.run
        original_signature = (self.attestation / "document.sig").read_bytes()
        def swap(argv, *args, **kwargs):
            if argv[:3] == ["ssh-keygen", "-Y", "verify"]:
                self.assertNotEqual(Path(argv[argv.index("-s") + 1]), self.attestation / "document.sig")
                (self.attestation / "document.sig").write_bytes(b"swapped")
            return original_run(argv, *args, **kwargs)
        with patch.object(source.subprocess, "run", side_effect=swap):
            with self.assertRaisesRegex(ValueError, "changed while verifying"):
                self.verify()
        (self.attestation / "document.sig").write_bytes(original_signature)
        def swap_document(argv, *args, **kwargs):
            if argv[:3] == ["ssh-keygen", "-Y", "verify"]:
                (self.attestation / "document.json").write_text("swapped")
            return original_run(argv, *args, **kwargs)
        with patch.object(source.subprocess, "run", side_effect=swap_document):
            with self.assertRaisesRegex(ValueError, "changed while verifying"):
                self.verify()

    def test_untrusted_uploaded_key_is_rejected(self):
        self.sign()
        self.signers.write_text("spot-qualification@local " + " ".join(self.public) + "\n")
        with self.assertRaisesRegex(ValueError, "approved maintainer key"):
            self.verify()

    def test_remote_scope_uses_host_anchor_and_missing_anchor_refuses(self):
        with patch.object(qualification, "ROOT", Path("/target/checkout")):
            self.assertEqual(qualification.source_signers(), source.REMOTE_TRUSTED_SIGNERS)
        self.sign()
        with self.assertRaisesRegex(ValueError, "trusted maintainer signer file"):
            source.verify_source(self.repo, self.head, self.repo.parent / "missing-anchor",
                                 self.attestation, bundle_sha256=self.bundle_sha, run_id=self.run_id)

    def test_transport_requires_host_gate_for_attested_mode(self):
        calls = []
        ssh = calls.append
        self.assertTrue(transport.host_source_admission(ssh, self.run_id, self.head,
                         self.bundle_sha, {"mode": source.ATTESTED}))
        self.assertEqual(len(calls), 1)
        self.assertIn("verify-source", calls[0])
        self.assertIn(self.head, calls[0])
        self.assertFalse(transport.host_source_admission(ssh, self.run_id, self.head,
                         self.bundle_sha, {"mode": source.STRICT}))
        self.assertEqual(len(calls), 1)

    def test_publication_requires_packed_and_archived_source_identity(self):
        bundle = self.repo.parent / "repository.bundle"
        bundle.write_bytes(b"frozen bundle")
        self.bundle_sha = hashlib.sha256(bundle.read_bytes()).hexdigest()
        self.doc["bundle_sha256"] = self.bundle_sha
        self.sign()
        expected = self.verify()
        output = (self.repo.parent / "remote-output").resolve()
        output.mkdir()
        shutil.copytree(self.attestation, output / "source-attestation")
        (output / "source-identity.json").write_text(json.dumps(expected))
        (output / "remote-qualification.json").write_text(json.dumps({"source_identity": expected}))
        archive = self.repo.parent / "results.tar.gz"
        with tarfile.open(archive, "w:gz") as stream:
            stream.add(output / "source-identity.json", arcname="remote-output/source-identity.json")
            for name in self.doc["evidence_sha256"] | {"document.json": "", "document.sig": ""}:
                stream.add(output / "source-attestation" / name,
                           arcname="remote-output/source-attestation/" + name)
        with patch.object(qualification, "ROOT", self.repo), \
             patch.object(qualification, "ops_root", return_value=output.parent):
            qualification.verify_source_result_binding(output, self.head, self.signers,
                bundle, self.attestation, self.run_id, expected, expected)
            qualification.verify_archived_source_members(archive, output, True)
            with self.assertRaisesRegex(RuntimeError, "packed or archived"):
                qualification.verify_source_result_binding(output, self.head, self.signers,
                    bundle, self.attestation, self.run_id, expected, {"mode": source.STRICT})
            (output / "source-identity.json").write_text("{}")
            with self.assertRaisesRegex(RuntimeError, "packed or archived"):
                qualification.verify_source_result_binding(output, self.head, self.signers,
                    bundle, self.attestation, self.run_id, expected, expected)
            with self.assertRaisesRegex(RuntimeError, "result archive source evidence differs"):
                qualification.verify_archived_source_members(archive, output, True)

    def test_strict_ssh_commit_remains_accepted_with_exact_principal(self):
        (self.repo / "signed.txt").write_text("maintainer")
        run("git", "add", "signed.txt", cwd=self.repo)
        env = dict(os.environ, GIT_AUTHOR_NAME="swackhamer", GIT_AUTHOR_EMAIL=source.PRINCIPAL,
                   GIT_COMMITTER_NAME="swackhamer", GIT_COMMITTER_EMAIL=source.PRINCIPAL)
        run("git", "-c", "user.signingkey=" + str(self.key), "-c", "gpg.format=ssh",
            "-c", "commit.gpgsign=true", "commit", "-qm", "signed", cwd=self.repo, env=env)
        signed = run("git", "rev-parse", "HEAD", cwd=self.repo)
        self.assertEqual(source.verify_source(self.repo, signed, self.signers)["mode"], source.STRICT)

    def test_real_openpgp_commit_with_maintainer_uid_cannot_enter_strict_mode(self):
        gpg_home = self.repo.parent / "foreign-gpg"
        gpg_home.mkdir(mode=0o700)
        env = dict(os.environ, GNUPGHOME=str(gpg_home),
                   GIT_AUTHOR_NAME="swackhamer", GIT_AUTHOR_EMAIL=source.PRINCIPAL,
                   GIT_COMMITTER_NAME="swackhamer", GIT_COMMITTER_EMAIL=source.PRINCIPAL)
        subprocess.run(["gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "",
                        "--quick-generate-key", source.PRINCIPAL,
                        "ed25519", "sign", "0"], env=env, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        listing = run("gpg", "--batch", "--with-colons", "--list-secret-keys", env=env)
        fingerprint = next(line.split(":")[9] for line in listing.splitlines() if line.startswith("fpr:"))
        (self.repo / "foreign.txt").write_text("pgp")
        run("git", "add", "foreign.txt", cwd=self.repo)
        run("git", "-c", "user.signingkey=" + fingerprint, "-c", "gpg.format=openpgp",
            "-c", "commit.gpgsign=true", "commit", "-qm", "foreign", cwd=self.repo, env=env)
        head = run("git", "rev-parse", "HEAD", cwd=self.repo)
        with patch.dict(os.environ, GNUPGHOME=str(gpg_home)):
            git_status = source._git(self.repo, "-c", "gpg.format=ssh", "-c",
                "gpg.ssh.allowedSignersFile=" + str(self.signers), "show", "-s",
                "--format=%G?%x00%GS%x00%GF", head).strip().decode().split("\x00")
            self.assertEqual(git_status[:2], ["G", source.PRINCIPAL])
            self.assertNotEqual(git_status[2], source.FINGERPRINT)
            with self.assertRaisesRegex(ValueError, "SSH commit signature"):
                source.verify_source(self.repo, head, self.signers)

    def test_bootstrap_and_both_input_seams_use_real_attested_verifier(self):
        self.sign()
        verified = bootstrap.verify_staged_checkout(self.repo, self.head, self.signers,
                                                     self.attestation, self.bundle_sha)
        self.assertEqual(verified["raw_commit_sha256"], self.doc["raw_commit_sha256"])
        with self.assertRaises(ValueError):
            bootstrap.verify_staged_checkout(self.repo, "0" * 40, self.signers,
                                             self.attestation, self.bundle_sha)
        output = (self.repo.parent / "prepared").resolve()
        (output / "provisioned").mkdir(parents=True)
        (output / "read-policy-input.json").write_text("{}")
        module = SimpleNamespace(snapshot_caller=lambda _root: {"head": self.head},
            CANONICAL_MATRIX=Path("unused"), load_matrix=lambda *_: {},
            materialize_matrix=lambda *_args, **_kwargs: {"rows": []},
            _read_policy_input=lambda *_args: None)
        def git(*args):
            if args == ("rev-parse", "HEAD"):
                return self.head
            if args == ("config", "--path", "--get", "gpg.ssh.allowedSignersFile"):
                return str(self.signers)
            return ""
        with patch.object(qualification, "ROOT", self.repo), \
             patch.object(qualification, "ops_root", return_value=output.parent), \
             patch.object(qualification, "qualification_module", return_value=module), \
             patch.object(qualification, "git", side_effect=git), \
             patch.object(qualification, "source_signers", return_value=self.signers), \
             patch.object(qualification, "source_attestation", return_value=self.attestation):
            self.assertEqual(qualification.exact_inputs(output)[0], self.head)
            self.assertEqual(qualification.transport_inputs(output)[0], self.head)
            bundle = self.repo.parent / "repository.bundle"
            bundle.write_bytes(b"signed source bundle")
            self.bundle_sha = hashlib.sha256(bundle.read_bytes()).hexdigest()
            self.doc["bundle_sha256"] = self.bundle_sha
            self.sign()
            archive = self.repo.parent / "spot-input.tar.gz"
            self.assertEqual(qualification.prepare_archive(output, archive, bundle)[0], self.head)
            import tarfile
            with tarfile.open(archive) as stream:
                self.assertEqual(stream.extractfile("source-attestation/document.json").read(),
                                 (self.attestation / "document.json").read_bytes())
            (self.attestation / "pgp-proof.txt").write_text("mutated")
            with self.assertRaisesRegex(ValueError, "evidence bytes differ"):
                qualification.exact_inputs(output)
            with self.assertRaisesRegex(ValueError, "evidence bytes differ"):
                qualification.transport_inputs(output)

    def test_issuer_requires_fresh_api_and_independent_pgp_proof(self):
        root = self.repo.parent
        bundle = root / "repository.bundle"
        bundle.write_bytes(b"bundle")
        gpg_home = root / "isolated-gpg"
        gpg_home.mkdir()
        facts = source.source_facts(self.repo, self.head)
        commit = {"sha": self.head, "commit": {"tree": {"sha": facts["tree"]},
                  "verification": {"verified": True, "reason": "valid",
                                   "signature": "sig", "payload": "payload"}}}
        pull = {"number": 1065, "merged_at": "2026-10-05T00:00:00Z",
                "merge_commit_sha": self.head, "base": {"ref": "refactor/tag-machinery"}}
        ref = {"object": {"sha": self.head}}
        original_run, original_check = subprocess.run, subprocess.check_output
        def fake_run(argv, *args, **kwargs):
            if "verify-commit" in argv:
                return subprocess.CompletedProcess(argv, 0, "", "[GNUPG:] GOODSIG X\n[GNUPG:] VALIDSIG " +
                    source.GITHUB_FINGERPRINT + "\n[GNUPG:] TRUST_FULLY\n")
            return original_run(argv, *args, **kwargs)
        def fake_check(argv, *args, **kwargs):
            if argv[:2] == ["gh", "api"]:
                value = commit if "/commits/" in argv[2] else pull if "/pulls/" in argv[2] else ref
                return json.dumps(value).encode()
            return original_check(argv, *args, **kwargs)
        with patch.object(source, "_signed_payload", return_value=(b"sig", b"payload")), \
             patch.object(source.subprocess, "run", side_effect=fake_run), \
             patch.object(source.subprocess, "check_output", side_effect=fake_check):
            target = root / "issued"
            document = source.issue_github_squash_document(self.repo, self.head, 1065,
                       bundle, self.run_id, gpg_home, target)
            self.assertEqual(json.loads(document.read_text())["merge_sha"], self.head)
            self.assertTrue((target / "pgp-proof.txt").is_file())
            self.assertEqual((target / "repository.bundle").read_bytes(), b"bundle")
            commit["commit"]["verification"]["verified"] = False
            with self.assertRaisesRegex(ValueError, "fresh GitHub"):
                source.issue_github_squash_document(self.repo, self.head, 1065,
                    bundle, self.run_id, gpg_home, root / "rejected")
            self.assertFalse((root / "rejected").exists())
            commit["commit"]["verification"]["verified"] = True
            commit["commit"]["verification"]["payload"] = "other"
            with self.assertRaisesRegex(ValueError, "fresh GitHub"):
                source.issue_github_squash_document(self.repo, self.head, 1065,
                    bundle, self.run_id, gpg_home, root / "wrong-payload")
            self.assertFalse((root / "wrong-payload").exists())


if __name__ == "__main__":
    unittest.main()
