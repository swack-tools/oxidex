"""Contract tests for the durable ExifTool oracle bootstrap."""

from __future__ import annotations

import importlib.util
import io
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import tarfile
import textwrap
import threading
import unittest
from contextlib import contextmanager
from pathlib import Path
from unittest import mock


MODULE = Path(__file__).with_name("bootstrap_oracle.py")
spec = importlib.util.spec_from_file_location("bootstrap_oracle", MODULE)
assert spec and spec.loader
oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oracle)

ORACLE_MODULE = MODULE.parents[2] / "scripts/exiftool_oracle.py"
oracle_spec = importlib.util.spec_from_file_location("exiftool_oracle", ORACLE_MODULE)
assert oracle_spec and oracle_spec.loader
exiftool_oracle = importlib.util.module_from_spec(oracle_spec)
sys.path.insert(0, str(ORACLE_MODULE.parent))
oracle_spec.loader.exec_module(exiftool_oracle)


def setUpModule():
    oracle.DURABLE_ROOT.mkdir(parents=True, exist_ok=True)


class DurablePathTests(unittest.TestCase):
    def test_exact_durable_defaults(self) -> None:
        root = oracle.DURABLE_ROOT
        self.assertEqual(oracle.DURABLE_ROOT, root)
        self.assertEqual(
            oracle.perl_path(root),
            root / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2",
        )
        self.assertEqual(
            oracle.exiftool_path(root),
            root / "cache/exiftool/13.59/exiftool/exiftool",
        )
        self.assertEqual(
            oracle.corpus_path(root),
            root / "cache/exiftool/13.59/combined-samples",
        )
        self.assertEqual(
            oracle.manifest_path(root),
            root
            / "evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap/storage-manifest.json",
        )

    def test_public_cache_contract_matches_legacy_path_construction(self) -> None:
        """The helper and old callers share one sibling-corpus layout."""
        cache = exiftool_oracle.cache_dir()
        self.assertEqual(cache, oracle.DURABLE_ROOT / "cache/exiftool/13.59")
        self.assertEqual(exiftool_oracle.pinned_binary(), cache / "exiftool/exiftool")
        self.assertEqual(exiftool_oracle.pinned_lib(), cache / "exiftool/lib")
        self.assertEqual(oracle.exiftool_path(oracle.DURABLE_ROOT), cache / "exiftool/exiftool")
        self.assertEqual(oracle.corpus_path(oracle.DURABLE_ROOT), cache / "combined-samples")
        self.assertNotIn("exiftool", oracle.corpus_path(oracle.DURABLE_ROOT).relative_to(cache).parts)

    def test_rejects_system_temporary_roots_tmpdir_and_escape_symlink(self) -> None:
        candidates = {Path(tempfile.gettempdir()), Path("/tmp"), Path("/private/tmp")}
        if os.environ.get("TMPDIR"):
            candidates.add(Path(os.environ["TMPDIR"]))
        for candidate in candidates:
            with self.subTest(candidate=candidate), self.assertRaisesRegex(
                oracle.Refused, "durable"
            ):
                oracle.resolve_durable_root(candidate)

        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            (root / "escape").symlink_to(Path(tempfile.gettempdir()))
            with self.assertRaisesRegex(oracle.Refused, "outside"):
                oracle.require_descendant(root / "escape/file", root)

    def test_rejects_every_symlink_even_when_target_stays_inside_durable_root(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            target = root / "real"
            target.mkdir()
            (target / "artifact").write_text("durable\n", encoding="utf-8")
            alias = root / "alias"
            alias.symlink_to(target, target_is_directory=True)

            with self.assertRaisesRegex(oracle.Refused, "symlink"):
                oracle.require_descendant(alias / "artifact", root)

            linked_file = root / "linked-artifact"
            linked_file.symlink_to(target / "artifact")
            with self.assertRaisesRegex(oracle.Refused, "symlink"):
                oracle.authenticate_artifacts(root, {"artifact": linked_file})

    def test_release_environment_overrides_are_fenced_to_durable_root(self) -> None:
        for variable in (
            exiftool_oracle.CACHE_DIR_ENV,
            exiftool_oracle.PERL_ENV,
            exiftool_oracle.BINARY_ENV,
        ):
            with self.subTest(variable=variable), mock.patch.dict(
                os.environ, {variable: "/tmp/not-durable"}, clear=False
            ), self.assertRaisesRegex(exiftool_oracle.OracleError, "durable"):
                exiftool_oracle.durable_environment_path(variable)
        with self.assertRaisesRegex(oracle.Refused, "outside"):
            oracle.resolve_durable_override(Path("/tmp/not-durable"))

    def test_environment_overrides_refuse_in_root_symlink_spellings(self) -> None:
        """A lexical durable override must not smuggle a symlink past the fence."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            target = root / "real-cache"
            target.mkdir()
            alias = root / "cache-alias"
            alias.symlink_to(target, target_is_directory=True)
            with mock.patch.dict(
                os.environ,
                {exiftool_oracle.CACHE_DIR_ENV: str(alias)},
                clear=False,
            ), self.assertRaisesRegex(exiftool_oracle.OracleError, "symlink"):
                exiftool_oracle.cache_dir()

    def test_legacy_cache_consumers_receive_a_real_checkout_directory(self) -> None:
        """Callers that append ``exiftool`` must reach the durable source tree."""
        repository = MODULE.parents[2]
        cache = exiftool_oracle.cache_dir()
        checkout = cache / "exiftool"
        self.assertTrue((checkout / "exiftool").is_file())
        self.assertTrue((checkout / "lib/Image/ExifTool.pm").is_file())
        result = subprocess.run(
            ["uv", "run", "tools/exiftool-tables/duplicate_loss_scan.py", "--self-test"],
            cwd=repository,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_immutable_installer_materializes_a_fresh_tree(self) -> None:
        """A complete verified staging tree publishes only into an absent path."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"

            def populate(staging: Path) -> None:
                (staging / "sentinel").write_text("verified\n", encoding="utf-8")

            def verify(candidate: Path) -> None:
                self.assertEqual(
                    (candidate / "sentinel").read_text(encoding="utf-8"),
                    "verified\n",
                )

            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                result = oracle.install_immutable_tree(root, destination, populate, verify)
            self.assertEqual(result, "installed")
            verify(destination)
            self.assertFalse((root / ".bootstrap-tree-backups").exists())

    def test_immutable_installer_reuses_exact_verified_canonical_tree(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "toolchains/perl-5.38.2/prefix"
            destination.mkdir(parents=True)
            (destination / "sentinel").write_text("verified\n", encoding="utf-8")

            def populate(_staging: Path) -> None:
                self.fail("a verified canonical tree must never be restaged")

            def verify(candidate: Path) -> None:
                self.assertEqual(
                    (candidate / "sentinel").read_text(encoding="utf-8"),
                    "verified\n",
                )

            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                self.assertEqual(
                    oracle.install_immutable_tree(root, destination, populate, verify),
                    "reused",
                )
            verify(destination)

    def test_immutable_installer_refuses_conflicting_destination_without_moving_it(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            destination.mkdir(parents=True)
            (destination / "sentinel").write_text("conflict\n", encoding="utf-8")

            def populate(staging: Path) -> None:
                (staging / "sentinel").write_text("verified\n", encoding="utf-8")

            def verify(candidate: Path) -> None:
                if not (candidate / "sentinel").is_file() or (candidate / "sentinel").read_text(encoding="utf-8") != "verified\n":
                    raise oracle.Refused("candidate is not the locked tree")

            with mock.patch.object(oracle, "DURABLE_ROOT", root), self.assertRaisesRegex(
                oracle.Refused, "locked tree"
            ):
                oracle.install_immutable_tree(root, destination, populate, verify)
            self.assertEqual(
                (destination / "sentinel").read_text(encoding="utf-8"), "conflict\n"
            )
            self.assertFalse((root / ".bootstrap-tree-backups").exists())

    def test_immutable_installer_concurrent_loser_verifies_the_winner(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            ready = threading.Barrier(2)
            results: list[str] = []
            failures: list[BaseException] = []

            def populate(staging: Path) -> None:
                (staging / "sentinel").write_text("verified\n", encoding="utf-8")
                ready.wait(timeout=10)

            def verify(candidate: Path) -> None:
                if (candidate / "sentinel").read_text(encoding="utf-8") != "verified\n":
                    raise oracle.Refused("winner is not the locked tree")

            def install() -> None:
                try:
                    results.append(
                        oracle.install_immutable_tree(root, destination, populate, verify)
                    )
                except BaseException as exc:  # recorded for the parent assertion
                    failures.append(exc)

            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                workers = [threading.Thread(target=install) for _ in range(2)]
                for worker in workers:
                    worker.start()
                for worker in workers:
                    worker.join(timeout=20)
            self.assertFalse(any(worker.is_alive() for worker in workers))
            self.assertEqual(failures, [])
            self.assertCountEqual(results, ["installed", "reused"])
            verify(destination)

    def test_immutable_installer_hides_live_staging_until_ownership_intent_exists(self) -> None:
        """A second installer must not misclassify a cooperating owner's new stage."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            staging_created = threading.Event()
            allow_first_intent = threading.Event()
            second_attempted_lifecycle = threading.Event()
            second_recovery_entered = threading.Event()
            first_verified = threading.Event()
            second_verified = threading.Event()
            release_first_publish = threading.Event()
            release_second_publish = threading.Event()
            first_published = threading.Event()
            results: list[str] = []
            failures: list[BaseException] = []
            original_mkdtemp = oracle.tempfile.mkdtemp
            original_recover = oracle._recover_abandoned_staging
            original_lifecycle_lock = oracle._staging_lifecycle_lock
            first_mkdtemp = True

            def pausing_mkdtemp(*args: object, **kwargs: object) -> str:
                nonlocal first_mkdtemp
                created = original_mkdtemp(*args, **kwargs)
                if first_mkdtemp:
                    first_mkdtemp = False
                    staging_created.set()
                    self.assertTrue(allow_first_intent.wait(timeout=10))
                return created

            def observing_recovery(*args: object, **kwargs: object) -> Path | None:
                if threading.current_thread().name == "second-installer":
                    second_recovery_entered.set()
                return original_recover(*args, **kwargs)

            @contextmanager
            def observing_lifecycle_lock(*args: object, **kwargs: object):
                if threading.current_thread().name == "second-installer":
                    second_attempted_lifecycle.set()
                with original_lifecycle_lock(*args, **kwargs):
                    yield

            def hold_verified_staging(_staging: Path) -> None:
                if threading.current_thread().name == "first-installer":
                    first_verified.set()
                    self.assertTrue(release_first_publish.wait(timeout=10))
                else:
                    second_verified.set()
                    self.assertTrue(release_second_publish.wait(timeout=10))

            def populate(staging: Path) -> None:
                (staging / "sentinel").write_text("verified\n", encoding="utf-8")

            def verify(candidate: Path) -> None:
                if (candidate / "sentinel").read_text(encoding="utf-8") != "verified\n":
                    raise oracle.Refused("candidate is not the locked tree")

            def install() -> None:
                try:
                    results.append(
                        oracle.install_immutable_tree(
                            root,
                            destination,
                            populate,
                            verify,
                            after_verify=hold_verified_staging,
                        )
                    )
                    if threading.current_thread().name == "first-installer":
                        first_published.set()
                except BaseException as exc:  # retained for the parent assertion
                    failures.append(exc)

            with mock.patch.object(oracle, "DURABLE_ROOT", root), mock.patch.object(
                oracle.tempfile, "mkdtemp", side_effect=pausing_mkdtemp
            ), mock.patch.object(
                oracle, "_recover_abandoned_staging", side_effect=observing_recovery
            ), mock.patch.object(
                oracle, "_staging_lifecycle_lock", side_effect=observing_lifecycle_lock
            ):
                first = threading.Thread(target=install, name="first-installer")
                second = threading.Thread(target=install, name="second-installer")
                first.start()
                self.assertTrue(staging_created.wait(timeout=10))
                second.start()
                self.assertTrue(second_attempted_lifecycle.wait(timeout=10))
                self.assertFalse(second_recovery_entered.is_set())
                allow_first_intent.set()
                self.assertTrue(first_verified.wait(timeout=10))
                self.assertTrue(second_verified.wait(timeout=10))
                self.assertTrue(second_recovery_entered.is_set())
                release_first_publish.set()
                self.assertTrue(first_published.wait(timeout=10))
                release_second_publish.set()
                first.join(timeout=20)
                second.join(timeout=20)
            self.assertFalse(first.is_alive())
            self.assertFalse(second.is_alive())
            self.assertEqual(failures, [])
            self.assertCountEqual(results, ["installed", "reused"])
            self.assertFalse([
                path for path in destination.parent.glob(".exiftool.staging-*")
                if path.name != ".exiftool.staging-recovery.jsonl"
            ])
            verify(destination)
            journal = destination.parent / ".exiftool.staging-recovery.jsonl"
            quarantine_root = root / "evidence/bootstrap-staging-quarantine/exiftool"
            before = oracle.sha256_tree(destination)
            self.assertTrue(journal.is_file())
            self.assertTrue(any(quarantine_root.iterdir()))
            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                self.assertEqual(
                    oracle.install_immutable_tree(root, destination, populate, verify),
                    "reused",
                )
            self.assertEqual(oracle.sha256_tree(destination), before)
            self.assertTrue(journal.is_file())
            self.assertTrue(any(quarantine_root.iterdir()))
            self.assertFalse([
                path for path in destination.parent.glob(".exiftool.staging-*")
                if path.name != ".exiftool.staging-recovery.jsonl"
            ])

    def test_immutable_reuse_survives_journal_writing_dead_staging_recovery(self) -> None:
        """A recovery journal is metadata, never a future staging candidate."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            dead = destination.parent / ".exiftool.staging-999999999-dead"
            dead.mkdir(parents=True)
            (dead / "partial").write_text("partial\n", encoding="utf-8")
            oracle.write_staging_intent(dead, destination, 999999999, "materializing")

            def populate(staging: Path) -> None:
                (staging / "sentinel").write_text("verified\n", encoding="utf-8")

            def verify(candidate: Path) -> None:
                sentinel = candidate / "sentinel"
                if not sentinel.is_file() or sentinel.read_text(encoding="utf-8") != "verified\n":
                    raise oracle.Refused("candidate is not authenticated")

            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                self.assertEqual(
                    oracle.install_immutable_tree(root, destination, populate, verify),
                    "installed",
                )
            journal = destination.parent / ".exiftool.staging-recovery.jsonl"
            before = oracle.sha256_tree(destination)
            self.assertTrue(journal.is_file())
            self.assertFalse(dead.exists())
            self.assertFalse(oracle.staging_intent_path(dead).exists())
            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                self.assertEqual(
                    oracle.install_immutable_tree(root, destination, populate, verify),
                    "reused",
                )
            self.assertEqual(oracle.sha256_tree(destination), before)
            self.assertTrue(journal.is_file())

    def test_malformed_recovery_journal_fails_closed(self) -> None:
        """Only the exact, parseable lifecycle journal is exempt from candidates."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            destination.mkdir(parents=True)
            (destination / "sentinel").write_text("verified\n", encoding="utf-8")
            journal = destination.parent / ".exiftool.staging-recovery.jsonl"
            journal.write_text("not-json\n", encoding="utf-8")

            def verify(candidate: Path) -> None:
                if (candidate / "sentinel").read_text(encoding="utf-8") != "verified\n":
                    raise oracle.Refused("candidate is not authenticated")

            with mock.patch.object(oracle, "DURABLE_ROOT", root), self.assertRaisesRegex(
                oracle.Refused, "malformed staging recovery journal"
            ):
                oracle.install_immutable_tree(root, destination, lambda _: None, verify)

    def test_sigkill_after_the_old_replacement_boundary_keeps_verified_canonical_tree(self) -> None:
        """A killed resumer cannot remove the tree old replacement moved aside."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            destination.mkdir(parents=True)
            (destination / "sentinel").write_text("verified\n", encoding="utf-8")
            marker = root / "resumer-returned"
            script = textwrap.dedent(
                """
                import importlib.util
                import os
                import signal
                import sys
                from pathlib import Path

                module_path = Path(sys.argv[1])
                root = Path(sys.argv[2])
                destination = Path(sys.argv[3])
                marker = Path(sys.argv[4])
                spec = importlib.util.spec_from_file_location("child_bootstrap", module_path)
                module = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(module)
                module.DURABLE_ROOT = root
                original_replace = module.os.replace
                def kill_at_old_boundary(source, target):
                    if Path(source) == destination:
                        marker.write_text("old-boundary\\n", encoding="utf-8")
                        os.kill(os.getpid(), signal.SIGKILL)
                    return original_replace(source, target)
                module.os.replace = kill_at_old_boundary
                def populate(staging):
                    (staging / "sentinel").write_text("verified\\n", encoding="utf-8")
                def verify(candidate):
                    if (candidate / "sentinel").read_text(encoding="utf-8") != "verified\\n":
                        raise module.Refused("canonical tree changed")
                if module.install_immutable_tree(root, destination, populate, verify) != "reused":
                    raise AssertionError("resumer did not reuse canonical tree")
                marker.write_text("reused\\n", encoding="utf-8")
                os.kill(os.getpid(), signal.SIGKILL)
                """
            )
            result = subprocess.run(
                [sys.executable, "-c", script, str(MODULE), str(root), str(destination), str(marker)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, -signal.SIGKILL, result.stderr)
            self.assertEqual(marker.read_text(encoding="utf-8"), "reused\n")
            self.assertEqual(
                (destination / "sentinel").read_text(encoding="utf-8"), "verified\n"
            )
            self.assertFalse((root / ".bootstrap-tree-backups").exists())

    def test_startup_cleans_only_dead_owned_staging_state(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            abandoned = destination.parent / ".exiftool.staging-999999999-dead"
            abandoned.mkdir(parents=True)
            (abandoned / "partial").write_text("partial\n", encoding="utf-8")
            oracle.write_staging_intent(
                abandoned, destination, 999999999, "materializing"
            )

            def populate(staging: Path) -> None:
                (staging / "sentinel").write_text("verified\n", encoding="utf-8")

            def verify(candidate: Path) -> None:
                sentinel = candidate / "sentinel"
                if not sentinel.is_file() or sentinel.read_text(encoding="utf-8") != "verified\n":
                    raise oracle.Refused("candidate is not authenticated")

            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                self.assertEqual(
                    oracle.install_immutable_tree(root, destination, populate, verify),
                    "installed",
                )
            self.assertFalse(abandoned.exists())
            verify(destination)

    def test_dead_verified_staging_is_published_after_real_process_death(self) -> None:
        """SIGKILL after verification leaves a recoverable, authenticated candidate."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            marker = root / "verified-before-publication"
            script = textwrap.dedent(
                """
                import importlib.util, os, signal, sys
                from pathlib import Path
                spec = importlib.util.spec_from_file_location("child_bootstrap", Path(sys.argv[1]))
                module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
                root, destination, marker = map(Path, sys.argv[2:5]); module.DURABLE_ROOT = root
                def populate(staging): (staging / "sentinel").write_text("verified\\n")
                def verify(candidate):
                    if (candidate / "sentinel").read_text() != "verified\\n":
                        raise module.Refused("not locked")
                def die(_staging):
                    marker.write_text("verified\\n")
                    os.kill(os.getpid(), signal.SIGKILL)
                module.install_immutable_tree(root, destination, populate, verify, after_verify=die)
                """
            )
            result = subprocess.run(
                [sys.executable, "-c", script, str(MODULE), str(root), str(destination), str(marker)],
                capture_output=True, text=True, check=False,
            )
            self.assertEqual(result.returncode, -signal.SIGKILL, result.stderr)
            self.assertEqual(marker.read_text(encoding="utf-8"), "verified\n")
            self.assertFalse(destination.exists())
            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                self.assertEqual(
                    oracle.install_immutable_tree(
                        root, destination,
                        lambda _staging: self.fail("recovery must publish the verified candidate"),
                        lambda candidate: self.assertEqual((candidate / "sentinel").read_text(), "verified\n"),
                    ),
                    "installed",
                )
            self.assertEqual((destination / "sentinel").read_text(), "verified\n")

    def test_unowned_or_ambiguous_dead_staging_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            abandoned = destination.parent / ".exiftool.staging-999999999-dead"
            abandoned.mkdir(parents=True)
            (abandoned / "sentinel").write_text("verified\n", encoding="utf-8")
            with mock.patch.object(oracle, "DURABLE_ROOT", root), self.assertRaisesRegex(
                oracle.Refused, "unowned|ambiguous"
            ):
                oracle.install_immutable_tree(
                    root, destination, lambda _staging: None,
                    lambda candidate: self.assertEqual((candidate / "sentinel").read_text(), "verified\n"),
                )
            self.assertTrue(abandoned.is_dir())

    def test_redundant_authenticated_staging_is_quarantined_not_deleted(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            destination.mkdir(parents=True)
            (destination / "sentinel").write_text("verified\n", encoding="utf-8")
            staging = destination.parent / ".exiftool.staging-999999999-redundant"
            staging.mkdir()
            (staging / "sentinel").write_text("verified\n", encoding="utf-8")
            oracle.write_staging_intent(
                staging, destination, 999999999, "verified", oracle.sha256_tree(staging)
            )
            def verify(candidate: Path) -> None:
                if (candidate / "sentinel").read_text() != "verified\n":
                    raise oracle.Refused("not locked")
            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                self.assertEqual(oracle.install_immutable_tree(root, destination, lambda _: None, verify), "reused")
            quarantine = root / "evidence/bootstrap-staging-quarantine/exiftool" / staging.name
            self.assertEqual((quarantine / "sentinel").read_text(), "verified\n")

    def test_multiple_authenticated_dead_staging_candidates_refuse_recovery(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59/exiftool"
            for suffix in ("one", "two"):
                staging = destination.parent / f".exiftool.staging-999999999-{suffix}"
                staging.mkdir(parents=True)
                (staging / "sentinel").write_text("verified\n", encoding="utf-8")
                oracle.write_staging_intent(staging, destination, 999999999, "verified", oracle.sha256_tree(staging))
            with mock.patch.object(oracle, "DURABLE_ROOT", root), self.assertRaisesRegex(
                oracle.Refused, "multiple authenticated"
            ):
                oracle.install_immutable_tree(
                    root, destination, lambda _: None,
                    lambda candidate: None if (candidate / "sentinel").read_text() == "verified\n" else (_ for _ in ()).throw(oracle.Refused("not locked")),
                )

    def test_staged_perl_verification_and_make_steps_receive_candidate_library_path(self) -> None:
        root = oracle.DURABLE_ROOT / "staged-perl-contract"
        candidate = root / "toolchains/perl-5.38.2/prefix"
        environment = oracle.staged_perl_environment(candidate)
        self.assertIn(str(candidate / "lib"), environment["PERL5LIB"].split(":"))
        self.assertIn(str(candidate / "lib/site_perl"), environment["PERL5LIB"].split(":"))
        self.assertEqual(oracle.staged_perl_environment(candidate)["PERL5LIB"], environment["PERL5LIB"])

    def test_staged_perl_verifier_uses_candidate_libs_and_refuses_wrong_archive_zip(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            candidate = root / "toolchains/perl-5.38.2/prefix"
            perl = candidate / "bin/perl5.38.2"
            perl.parent.mkdir(parents=True)
            perl.write_text("placeholder\n", encoding="utf-8")
            observed: list[dict[str, str] | None] = []

            def fake_run(_argv: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None) -> str:
                del cwd
                observed.append(env)
                return str(candidate) if len(observed) == 1 else "1.67"

            with mock.patch.object(oracle, "run", side_effect=fake_run), self.assertRaisesRegex(
                oracle.Refused, "1.68"
            ):
                oracle._verify_perl_tree(root, candidate)
            self.assertTrue(all(env and str(candidate / "lib") in env["PERL5LIB"] for env in observed))

    def test_explicit_legacy_nested_corpus_quarantine_is_required_and_transactional(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            sibling = root / "cache/exiftool/13.59/combined-samples"
            nested = root / "cache/exiftool/13.59/exiftool/combined-samples"
            sibling.mkdir(parents=True)
            nested.mkdir(parents=True)
            for corpus in (sibling, nested):
                (corpus / "sample.bin").write_bytes(b"same authenticated bytes")
            nested_manifest = nested.parent / "combined-samples.manifest"
            nested_manifest.write_text("legacy manifest\n", encoding="utf-8")
            def write_verified_manifest(*_args: object) -> Path:
                manifest = oracle.manifest_path(root)
                manifest.parent.mkdir(parents=True, exist_ok=True)
                manifest.write_text("{}\n", encoding="utf-8")
                return manifest

            test_lock = {**oracle.LOCK, "corpus_tree_sha256": oracle.sha256_tree(sibling)}
            with mock.patch.object(oracle, "DURABLE_ROOT", root), mock.patch.object(
                oracle, "LOCK", test_lock
            ), mock.patch.object(oracle, "verify", side_effect=write_verified_manifest):
                with self.assertRaisesRegex(oracle.Refused, "legacy nested corpus"):
                    oracle.assert_no_legacy_nested_corpus(root)
                disposition = oracle.quarantine_legacy_nested_corpus(root)
                self.assertEqual(disposition["status"], "quarantined")
                self.assertFalse(nested.exists())
                self.assertTrue(sibling.is_dir())
                self.assertTrue(Path(disposition["quarantine"]).is_dir())
                self.assertFalse(nested_manifest.exists())
                self.assertTrue(Path(disposition["quarantined_manifest"]).is_file())
                oracle.assert_no_legacy_nested_corpus(root)

    def test_legacy_nested_corpus_conflict_fails_closed_without_mutation(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            sibling = root / "cache/exiftool/13.59/combined-samples"
            nested = root / "cache/exiftool/13.59/exiftool/combined-samples"
            sibling.mkdir(parents=True)
            nested.mkdir(parents=True)
            (sibling / "sample.bin").write_bytes(b"authenticated sibling")
            (nested / "sample.bin").write_bytes(b"stale nested")
            with mock.patch.object(oracle, "DURABLE_ROOT", root), self.assertRaisesRegex(
                oracle.Refused, "ambiguous legacy nested corpus"
            ):
                oracle.quarantine_legacy_nested_corpus(root)
            self.assertTrue(sibling.exists())
            self.assertTrue(nested.exists())

    def test_legacy_nested_corpus_quarantine_resumes_after_the_rename_window(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            sibling = root / "cache/exiftool/13.59/combined-samples"
            sibling.mkdir(parents=True)
            (sibling / "sample.bin").write_bytes(b"same authenticated bytes")
            digest = oracle.sha256_tree(sibling)
            quarantine = (
                root
                / "evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap"
                / "legacy-nested-corpus-quarantine"
                / f"{digest}-13.59"
            )
            quarantine.parent.mkdir(parents=True)
            os.rename(sibling, quarantine)
            sibling.mkdir(parents=True)
            (sibling / "sample.bin").write_bytes(b"same authenticated bytes")
            journal = root / "evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap/legacy-nested-corpus-quarantine.json"
            journal.write_text(
                json.dumps(
                    {
                        "status": "prepared",
                        "sha256": digest,
                        "quarantine": str(quarantine),
                        "quarantined_manifest": str(quarantine.parent / f"{quarantine.name}.manifest"),
                    }
                ),
                encoding="utf-8",
            )

            def write_verified_manifest(*_args: object) -> Path:
                manifest = oracle.manifest_path(root)
                manifest.parent.mkdir(parents=True, exist_ok=True)
                manifest.write_text("{}\n", encoding="utf-8")
                return manifest

            test_lock = {**oracle.LOCK, "corpus_tree_sha256": digest}
            with mock.patch.object(oracle, "DURABLE_ROOT", root), mock.patch.object(
                oracle, "LOCK", test_lock
            ), mock.patch.object(oracle, "verify", side_effect=write_verified_manifest):
                disposition = oracle.quarantine_legacy_nested_corpus(root)
            self.assertEqual(disposition["status"], "quarantined")

    def test_safe_extract_remains_secure_on_the_supported_python3_runtime(self) -> None:
        """A durable archive can be extracted without relying on new-only tarfile APIs."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            archive = root / "source.tar"
            with tarfile.open(archive, "w") as source:
                info = tarfile.TarInfo("source/payload.txt")
                payload = b"durable payload\n"
                info.size = len(payload)
                source.addfile(info, io.BytesIO(payload))
            destination = root / "extract"
            destination.mkdir()
            extracted = oracle._safe_extract(archive, destination)
            self.assertEqual(
                (extracted / "payload.txt").read_text(encoding="utf-8"),
                "durable payload\n",
            )

    def test_materialize_corpus_normalizes_modes_across_umasks_and_keeps_executables(self) -> None:
        """The locked corpus tree is host-umask independent without losing executability."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            base = oracle.exiftool_root(root) / "t/images"
            base.mkdir(parents=True)
            (base / "base.txt").write_bytes(b"base\n")
            (base / "base.txt").chmod(0o664)
            archive = root / "samples_fixture.tar"
            with tarfile.open(archive, "w") as source:
                nested = tarfile.TarInfo("nested")
                nested.type = tarfile.DIRTYPE
                nested.mode = 0o775
                source.addfile(nested)
                ordinary = tarfile.TarInfo("archive.txt")
                ordinary.mode = 0o664
                ordinary.size = len(b"archive\n")
                source.addfile(ordinary, io.BytesIO(b"archive\n"))
                executable = tarfile.TarInfo("run-fixture")
                executable.mode = 0o775
                executable.size = len(b"#!/bin/sh\n")
                source.addfile(executable, io.BytesIO(b"#!/bin/sh\n"))

            expected_tree = root / "expected"
            shutil.copytree(base, expected_tree)
            with tarfile.open(archive) as source:
                source.extractall(expected_tree)
            for item in expected_tree.rglob("*"):
                item.chmod(
                    0o755 if item.is_dir() or item.name == "run-fixture" else 0o644
                )
            expected = oracle.sha256_tree(expected_tree)
            test_lock = {**oracle.LOCK, "corpus_tree_sha256": expected}

            for mask in (0o002, 0o077):
                with self.subTest(umask=oct(mask)), mock.patch.object(
                    oracle, "DURABLE_ROOT", root
                ), mock.patch.object(oracle, "LOCK", test_lock), mock.patch.object(
                    oracle, "MIN_CORPUS_FILES", 3
                ):
                    old_umask = os.umask(mask)
                    try:
                        oracle._materialize_corpus(root, {"samples_fixture": archive})
                    finally:
                        os.umask(old_umask)
                    corpus = oracle.corpus_path(root)
                    self.assertEqual(oracle.sha256_tree(corpus), expected)
                    self.assertEqual((corpus / "nested").stat().st_mode & 0o777, 0o755)
                    self.assertEqual((corpus / "base.txt").stat().st_mode & 0o777, 0o644)
                    self.assertEqual((corpus / "archive.txt").stat().st_mode & 0o777, 0o644)
                    self.assertEqual((corpus / "run-fixture").stat().st_mode & 0o777, 0o755)
                    shutil.rmtree(corpus)

    def test_named_release_recipes_have_no_system_temporary_defaults(self) -> None:
        repository = MODULE.parents[2]
        for recipe in (
            "docs-coverage",
            "duplicate-loss-scan",
            "compare-exiftool-full",
            "compare-exiftool-full-update",
        ):
            result = subprocess.run(
                ["just", "--dry-run", recipe],
                cwd=repository,
                capture_output=True,
                text=True,
                check=True,
            )
            output = result.stdout + result.stderr
            with self.subTest(recipe=recipe):
                self.assertNotRegex(output, r"--json-out\s+/tmp/")
                self.assertNotRegex(output, r'(?m)^[A-Z_]+="/tmp/', msg=output)

    def test_named_release_recipes_refuse_nondurable_override_before_work(self) -> None:
        repository = MODULE.parents[2]
        environment = dict(os.environ, EXIFTOOL_CACHE_DIR="/tmp/not-durable")
        for recipe in (
            "docs-coverage",
            "duplicate-loss-scan",
            "compare-exiftool-full",
            "compare-exiftool-full-update",
        ):
            result = subprocess.run(
                ["just", recipe],
                cwd=repository,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )
            with self.subTest(recipe=recipe):
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("outside durable root", result.stderr)

    def test_full_comparison_recipes_refuse_same_version_alternate_cache_before_measurement(self) -> None:
        """A durable 13.59-looking override is not the authenticated canonical cache."""
        repository = MODULE.parents[2]
        alternate = Path(tempfile.mkdtemp(dir=oracle.DURABLE_ROOT)) / "alternate-cache"
        (alternate / "exiftool/lib").mkdir(parents=True)
        (alternate / "combined-samples").mkdir()
        (alternate / "exiftool/lib/changed.pm").write_text("changed\n", encoding="utf-8")
        (alternate / "combined-samples/changed.jpg").write_bytes(b"not canonical")
        fake_exiftool = alternate / "exiftool/exiftool"
        fake_exiftool.write_text('print "13.59\\n";\n', encoding="utf-8")
        fake_bin = alternate / "fake-bin"
        fake_bin.mkdir()
        cargo_marker = alternate / "cargo-was-called"
        cargo = fake_bin / "cargo"
        cargo.write_text(
            f'#!/bin/sh\nprintf called > "{cargo_marker}"\nexit 71\n', encoding="utf-8"
        )
        cargo.chmod(0o755)
        environment = dict(
            os.environ,
            EXIFTOOL_CACHE_DIR=str(alternate),
            EXIFTOOL_COMPARISON_SAMPLES=str(alternate / "combined-samples"),
            EXIFTOOL_COMPARISON_ALLOW_BOUNDED_AUTHENTICATED="1",
            PATH=f"{fake_bin}:{os.environ['PATH']}",
        )
        try:
            for recipe in ("compare-exiftool-full", "compare-exiftool-full-update"):
                cargo_marker.unlink(missing_ok=True)
                result = subprocess.run(
                    ["just", recipe], cwd=repository, env=environment,
                    capture_output=True, text=True, check=False,
                )
                with self.subTest(recipe=recipe):
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("requires canonical authenticated cache", result.stderr)
                    self.assertFalse(cargo_marker.exists(), result.stdout + result.stderr)
        finally:
            shutil.rmtree(alternate.parent)

    def test_comparison_wrappers_directly_exec_locked_perl_and_preserve_marker_contract(self) -> None:
        """Both recipes delegate publication to the behavioral durable helper."""
        repository = MODULE.parents[2]
        for recipe in ("compare-exiftool-full", "compare-exiftool-full-update"):
            result = subprocess.run(
                ["just", "--dry-run", recipe], cwd=repository,
                capture_output=True, text=True, check=True,
            )
            output = result.stdout + result.stderr
            with self.subTest(recipe=recipe):
                self.assertIn("write-comparison-wrapper", output)
                self.assertIn("cleanup-comparison-workdir", output)
                self.assertIn("MARKER_DIR=", output)
                self.assertIn('locked.py" --shared "$BUILD_LOG" -- cargo build', output)
                self.assertNotRegex(output, r"(?m)^cargo build ")
                self.assertIn("CANONICAL_CACHE_DIR=", output)
                self.assertIn("requires canonical authenticated cache", output)
                self.assertIn('SAMPLES_DIR="${EXIFTOOL_COMPARISON_SAMPLES:-$COMBINED_DIR}"', output)
                self.assertIn("EXIFTOOL_COMPARISON_ALLOW_BOUNDED_AUTHENTICATED", output)

    def test_comparison_wrapper_publication_is_behavioral_and_private(self) -> None:
        """Exercise the publisher under hostile umask, spaces, concurrency, and cleanup."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory) / "paths with spaces"
            root.mkdir()
            root.chmod(0o700)
            library = root / "library with spaces"
            library.mkdir()
            script = root / "script with spaces.pl"
            script.write_text(
                "use strict; use warnings; print join(chr(31), @ARGV);\n",
                encoding="utf-8",
            )
            perl = oracle.perl_path(oracle.DURABLE_ROOT)
            work = root / "wrapper work"
            work.mkdir()
            wrapper = work / "exiftool"
            markers = root / "markers with spaces"
            old_umask = os.umask(0o777)
            try:
                with mock.patch.object(
                    oracle, "_fsync_directory", wraps=oracle._fsync_directory
                ) as fsync_directory:
                    oracle.write_comparison_wrapper(
                        wrapper, perl, library, script, markers
                    )
                    self.assertEqual(wrapper.stat().st_mode & 0o777, 0o700)
                    result = subprocess.run(
                        [str(wrapper), "argument with spaces", ""],
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, "argument with spaces\x1f")
                    marker = next(markers.glob("perl-*.json"))
                    identity = json.loads(marker.read_text(encoding="utf-8"))
                    self.assertEqual(identity["perl"], str(perl))
                    self.assertEqual(identity["exiftool_lib"], str(library))
                    self.assertEqual(identity["exiftool_script"], str(script))
                    self.assertEqual(identity["argv"], ["argument with spaces", ""])
                    self.assertGreaterEqual(fsync_directory.call_count, 1)
                    oracle.cleanup_comparison_workdir(work, root)
                    self.assertFalse(work.exists())
                    self.assertGreaterEqual(fsync_directory.call_count, 2)

                    failing_work = root / "failed wrapper work"
                    failing_work.mkdir()
                    failing_work.chmod(0o700)
                    failing_script = root / "failing script.pl"
                    failing_script.write_text("die qq(expected failure\\n);\n", encoding="utf-8")
                    failing_script.chmod(0o600)
                    oracle.write_comparison_wrapper(
                        failing_work / "exiftool",
                        perl,
                        library,
                        failing_script,
                        root / "failure markers",
                    )
                    failed = subprocess.run(
                        [str(failing_work / "exiftool"), "failure argument"],
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    self.assertNotEqual(failed.returncode, 0)
                    oracle.cleanup_comparison_workdir(failing_work, root)
                    self.assertFalse(failing_work.exists())
            finally:
                os.umask(old_umask)

            starts = threading.Barrier(2)
            errors: list[BaseException] = []

            def publish(number: int) -> None:
                try:
                    concurrent_work = root / f"concurrent work {number}"
                    concurrent_work.mkdir()
                    starts.wait(timeout=5)
                    oracle.write_comparison_wrapper(
                        concurrent_work / "exiftool",
                        perl,
                        library,
                        script,
                        root / f"concurrent markers {number}",
                    )
                except BaseException as exc:  # assertion is made by the parent
                    errors.append(exc)

            threads = [threading.Thread(target=publish, args=(number,)) for number in (1, 2)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join(timeout=10)
            self.assertFalse(errors, errors)
            self.assertFalse(list(root.rglob(".exiftool.tmp-*")))

    def test_corpus_manifest_publication_is_safe_for_concurrent_verify_calls(self) -> None:
        """Two production publication calls cannot share or mutate one partial inode."""
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            corpus = oracle.corpus_path(root)
            corpus.mkdir(parents=True)
            (corpus / "one.bin").write_bytes(b"one")
            (corpus / "two.bin").write_bytes(b"two")
            manifest = corpus.parent / "combined-samples.manifest"
            barrier = threading.Barrier(2)
            original_replace = oracle.os.replace
            errors: list[BaseException] = []

            def synchronized_replace(source: os.PathLike[str], destination: os.PathLike[str]) -> None:
                if Path(destination) == manifest:
                    barrier.wait(timeout=5)
                original_replace(source, destination)

            def publish() -> None:
                try:
                    oracle._write_corpus_manifest(root)
                except BaseException as exc:  # assertion is made by the parent
                    errors.append(exc)

            with mock.patch.object(oracle, "MIN_CORPUS_FILES", 2), mock.patch.object(
                oracle.os, "replace", side_effect=synchronized_replace
            ):
                threads = [threading.Thread(target=publish) for _ in range(2)]
                for thread in threads:
                    thread.start()
                for thread in threads:
                    thread.join(timeout=10)
            self.assertFalse(errors, errors)
            self.assertTrue(manifest.is_file())
            self.assertEqual(len(manifest.read_text(encoding="utf-8").splitlines()), 2)
            self.assertFalse(list(corpus.parent.glob(".combined-samples.manifest.*.partial")))

    def test_compare_exiftool_samples_keeps_its_gcs_fallback_configuration(self) -> None:
        repository = MODULE.parents[2]
        result = subprocess.run(
            ["just", "--dry-run", "compare-exiftool-samples"],
            cwd=repository,
            capture_output=True,
            text=True,
            check=True,
        )
        output = result.stdout + result.stderr
        self.assertIn(
            'GCS_BUCKET="https://storage.googleapis.com/oxidex-samples/exiftool"',
            output,
        )
        self.assertIn('"$GCS_BUCKET/$mfr.tar.gz"', output)

    def test_lock_contains_and_validates_exact_source_identities(self) -> None:
        lock = oracle.load_lock(MODULE.with_name("oracle-lock.json"))
        oracle.validate_lock(lock)
        self.assertEqual(
            lock["perl"]["sha256"],
            "a0a31534451eb7b83c7d6594a497543a54d488bc90ca00f5e34762577f40655e",
        )
        self.assertEqual(
            lock["archive_zip"]["sha256"],
            "984e185d785baf6129c6e75f8eb44411745ac00bf6122fb1c8e822a3861ec650",
        )
        self.assertEqual(
            lock["exiftool"]["tag_object"],
            "2200871d9cef988051d2a99d67df3bda6cbb30a8",
        )
        for name, item in lock["archives"].items():
            if name.startswith("samples_"):
                self.assertRegex(item["url"], r"^https://exiftool\.org/[^/]+\.tar\.gz$")

        for field in ("perl", "archive_zip"):
            broken = json.loads(json.dumps(lock))
            broken[field].pop("sha256")
            with self.subTest(field=field), self.assertRaisesRegex(
                oracle.Refused, "hash"
            ):
                oracle.validate_lock(broken)
        broken = json.loads(json.dumps(lock))
        broken["exiftool"]["tag_object"] = "f" * 40
        with self.assertRaisesRegex(oracle.Refused, "tag object"):
            oracle.validate_lock(broken)
        broken = json.loads(json.dumps(lock))
        broken.pop("corpus_tree_sha256")
        with self.assertRaisesRegex(oracle.Refused, "corpus lock"):
            oracle.validate_lock(broken)

    def test_verify_refuses_missing_hashes_bad_probes_and_outside_manifest(self) -> None:
        manifest = {"artifacts": {"perl": {}}}
        with self.assertRaisesRegex(oracle.Refused, "hash"):
            oracle.validate_manifest(manifest, oracle.DURABLE_ROOT)
        with self.assertRaisesRegex(oracle.Refused, "outside"):
            oracle.validate_manifest(
                {
                    "artifacts": {
                        "perl": {"path": "/tmp/perl", "sha256": "a" * 64}
                    }
                },
                oracle.DURABLE_ROOT,
            )
        with self.assertRaisesRegex(oracle.Refused, "Perl"):
            oracle.validate_versions("v5.36.0", "1.68", "13.59", "DOCX", 4000)
        with self.assertRaisesRegex(oracle.Refused, "Archive"):
            oracle.validate_versions("v5.38.2", "1.67", "13.59", "DOCX", 4000)
        with self.assertRaisesRegex(oracle.Refused, "ExifTool"):
            oracle.validate_versions("v5.38.2", "1.68", "13.58", "DOCX", 4000)
        with self.assertRaisesRegex(oracle.Refused, "DOCX"):
            oracle.validate_versions("v5.38.2", "1.68", "13.59", "ZIP", 4000)
        with self.assertRaisesRegex(oracle.Refused, "corpus"):
            oracle.validate_versions("v5.38.2", "1.68", "13.59", "DOCX", 3999)

    def test_locked_download_is_atomic_and_hash_checked(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/downloads/source.tar.gz"
            destination.parent.mkdir(parents=True)
            destination.write_bytes(b"known-good")
            item = {
                "filename": destination.name,
                "url": "https://invalid.example/source.tar.gz",
                "sha256": oracle.sha256_bytes(b"replacement"),
            }

            def interrupted(_url: str, partial: Path) -> None:
                partial.write_bytes(b"partial")
                raise OSError("network interrupted")

            with self.assertRaisesRegex(oracle.Refused, "download"):
                oracle.download_locked(root, "source", item, fetch=interrupted)
            self.assertEqual(destination.read_bytes(), b"known-good")
            self.assertFalse(
                destination.with_suffix(destination.suffix + ".partial").exists()
            )

            def complete(_url: str, partial: Path) -> None:
                partial.write_bytes(b"replacement")

            path = oracle.download_locked(root, "source", item, fetch=complete)
            self.assertEqual(path.read_bytes(), b"replacement")

    def test_immutable_tree_install_keeps_canonical_tree_on_staging_failure(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "toolchains/perl-5.38.2/prefix"
            destination.parent.mkdir(parents=True)

            def fail(staging: Path) -> None:
                (staging / "sentinel").write_text("incomplete", encoding="utf-8")
                raise RuntimeError("build failed")

            with mock.patch.object(oracle, "DURABLE_ROOT", root), self.assertRaisesRegex(
                RuntimeError, "build failed"
            ):
                oracle.install_immutable_tree(
                    root, destination, fail,
                    lambda candidate: (_ for _ in ()).throw(oracle.Refused("incomplete"))
                    if (candidate / "sentinel").read_text() != "verified" else None,
            )
            self.assertFalse(destination.exists())
            self.assertFalse([
                path for path in destination.parent.glob(".prefix.staging-*")
                if path.name != ".prefix.staging-recovery.jsonl"
            ])

    def test_provision_restores_last_known_good_tree_when_final_probe_fails(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            destination = root / "cache/exiftool/13.59"
            destination.mkdir(parents=True)
            (destination / "sentinel").write_text(
                "last-known-good", encoding="utf-8"
            )

            def leave_last_known_good_untouched(
                _root: Path, _archives: object
            ) -> dict[str, object]:
                return {}

            with mock.patch.object(oracle, "DURABLE_ROOT", root), mock.patch.object(
                oracle, "materialize", side_effect=leave_last_known_good_untouched
            ), mock.patch.object(
                oracle, "verify", side_effect=oracle.Refused("final DOCX probe failed")
            ), self.assertRaisesRegex(oracle.Refused, "final DOCX probe failed"):
                oracle.provision(root)

            self.assertEqual(
                (destination / "sentinel").read_text(encoding="utf-8"),
                "last-known-good",
            )
            self.assertFalse(list(destination.parent.glob(".13.59.previous-*")))

    def test_destdir_payload_keeps_compiled_prefix_at_final_durable_path(self) -> None:
        destination = oracle.DURABLE_ROOT / "toolchains/perl-5.38.2/prefix"
        install_root = Path(
            oracle.DURABLE_ROOT / "toolchains/perl-5.38.2/.prefix.staging-1/.destdir"
        )
        self.assertEqual(
            oracle.destdir_payload(destination, install_root),
            install_root / destination.relative_to(destination.anchor),
        )

    def test_manifest_authenticates_sources_trees_corpus_and_executables(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            files = {
                "archive": root / "cache/downloads/archive.tar.gz",
                "perl": root / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2",
                "exiftool": root / "cache/exiftool/13.59/exiftool",
                "corpus_manifest": root
                / "cache/exiftool/13.59/combined-samples.manifest",
            }
            for name, path in files.items():
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name, encoding="utf-8")
            artifacts = oracle.authenticate_artifacts(root, files)
            self.assertEqual(set(artifacts), set(files))
            self.assertTrue(
                all(len(item["sha256"]) == 64 for item in artifacts.values())
            )
            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                oracle.validate_manifest({"artifacts": artifacts}, root)

    def test_locked_manifest_detects_tree_and_library_damage_and_plans_repair(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            perl_tree = root / "toolchains/perl-5.38.2/prefix"
            zip_library = perl_tree / "lib/5.38.2/Archive/Zip.pm"
            exiftool_tree = root / "cache/exiftool/13.59"
            exiftool_library = exiftool_tree / "lib/Image/ExifTool.pm"
            zip_library.parent.mkdir(parents=True)
            exiftool_library.parent.mkdir(parents=True)
            zip_library.write_text("package Archive::Zip;\n", encoding="utf-8")
            exiftool_library.write_text("package Image::ExifTool;\n", encoding="utf-8")
            named = {
                "perl_tree": perl_tree,
                "archive_zip_library": zip_library,
                "exiftool_tree": exiftool_tree,
                "exiftool_library": exiftool_library,
            }
            manifest = {
                "schema_version": 1,
                "lock_sha256": oracle.sha256_file(oracle.LOCK_PATH),
                "source_identities": {
                    "perl": oracle.LOCK["perl"],
                    "archive_zip": oracle.LOCK["archive_zip"],
                    "exiftool": oracle.LOCK["exiftool"],
                    "corpus_tree_sha256": oracle.LOCK["corpus_tree_sha256"],
                },
                "artifacts": oracle.authenticate_artifacts(root, named),
            }

            with mock.patch.object(oracle, "DURABLE_ROOT", root):
                oracle.validate_perl_prefix(perl_tree, str(perl_tree))
                with self.assertRaisesRegex(oracle.Refused, "configured prefix"):
                    oracle.validate_perl_prefix(
                        perl_tree, str(perl_tree.with_name("prefix.stage-old"))
                    )
                oracle.validate_locked_manifest(manifest, root, required=set(named))
                zip_library.write_text("tampered\n", encoding="utf-8")
                with self.assertRaisesRegex(oracle.Refused, "hash mismatch"):
                    oracle.validate_locked_manifest(manifest, root, required=set(named))
                self.assertEqual(
                    oracle.manifest_repair_components(manifest, root), {"perl"}
                )

                zip_library.write_text("package Archive::Zip;\n", encoding="utf-8")
                exiftool_library.write_text("tampered\n", encoding="utf-8")
                self.assertEqual(
                    oracle.manifest_repair_components(manifest, root),
                    {"exiftool", "corpus"},
                )

    def test_provision_downloads_locked_inputs_and_writes_complete_manifest(self) -> None:
        with tempfile.TemporaryDirectory(dir=oracle.DURABLE_ROOT) as directory:
            root = Path(directory)
            evidence = (
                root
                / "evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap"
            )
            evidence.mkdir(parents=True)
            staged = root / "cache/downloads/perl-5.38.2.tar.gz"
            staged.parent.mkdir(parents=True)
            staged.write_bytes(b"locked")
            digest = oracle.sha256_file(staged)
            verified = evidence / "storage-manifest.json"
            lock = {
                "schema_version": 1,
                "perl": {"version": "5.38.2", "sha256": digest},
                "archive_zip": {"version": "1.68", "sha256": digest},
                "exiftool": {
                    "version": "13.59",
                    "tag_object": "2200871d9cef988051d2a99d67df3bda6cbb30a8",
                },
                "corpus_tree_sha256": oracle.LOCK["corpus_tree_sha256"],
                "archives": {
                    "perl": {
                        "filename": staged.name,
                        "url": "https://invalid.example/perl.tar.gz",
                        "sha256": digest,
                    }
                },
            }
            with mock.patch.object(oracle, "DURABLE_ROOT", root), mock.patch.object(
                oracle, "LOCK", lock
            ), mock.patch.object(
                oracle,
                "materialize",
                return_value={
                    "perl_source": {"path": str(staged), "sha256": digest}
                },
            ), mock.patch.object(
                oracle,
                "verify",
                side_effect=lambda *_args: (
                    verified.write_text('{"schema_version": 1, "artifacts": {}}'),
                    verified,
                )[1],
            ):
                manifest = oracle.provision(root)
            value = json.loads(manifest.read_text())
            self.assertEqual(value["artifacts"]["perl_source"]["sha256"], digest)
            journal = evidence / "bootstrap-journal.json"
            self.assertEqual(json.loads(journal.read_text())["stage"], "complete")


if __name__ == "__main__":
    unittest.main()
