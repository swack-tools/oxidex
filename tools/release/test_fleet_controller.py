"""Durability contracts for every release fleet-controller command."""

from __future__ import annotations

import importlib.util
import json
import os
import signal
import subprocess
import sys
import threading
import time
import unittest
from pathlib import Path
from unittest import mock


MODULE = Path(__file__).with_name("fleet_controller.py")
spec = importlib.util.spec_from_file_location("fleet_controller", MODULE)
assert spec and spec.loader
fleet = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = fleet
spec.loader.exec_module(fleet)


TEST_ROOT = Path(
    "/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller-tests"
)
SHA_A = "a" * 40
SHA_B = "b" * 40


def task(number: int, state: str = "launchable", dependencies: list[int] | None = None) -> dict:
    return {
        "number": number,
        "slug": f"task-{number}",
        "state": state,
        "state_history": [{"state": state, "at": "2026-09-19T00:00:00Z"}],
        "dependencies": dependencies or [],
        "file_lease": [f"task-{number}.txt"],
        "worker": {
            "kind": "CLI",
            "model": "gpt-5.6-terra",
            "effort": "fast",
            "identity": None,
        },
        "process": None,
        "launch_count": 0,
        "prd_sha256": None,
        "report_sha256": None,
        "review_sha256": None,
        "base_sha": SHA_A,
        "head_sha": None,
        "pushed_sha": None,
        "merge_sha": None,
        "target_sha": SHA_A,
        "paths": {
            "worktree": f"/Users/allen/git/task-{number}",
            "target": f"/Users/allen/git/oxidex-beta1-targets/task-{number}",
            "evidence": str(TEST_ROOT / f"task-{number}"),
            "prd": str(TEST_ROOT / "prds" / f"{number:02d}-task.md"),
            "report": str(TEST_ROOT / "reports" / f"{number:02d}-task.md"),
            "review": str(TEST_ROOT / "reviews" / f"{number:02d}-task.md"),
        },
        "heartbeat": None,
        "pr_ci_state": None,
        "receipt_hashes": {},
        "ruling": None,
        "blocker": None,
        "expected_merge_parent": SHA_A,
        "next_command": f"materialize --task {number}",
    }


def state(*tasks: dict) -> dict:
    return {
        "schema_version": 1,
        "plan_sha256": "1" * 64,
        "spec_sha256": "2" * 64,
        "target_ref": "origin/staging/beta1-functional-integration",
        "target_sha": SHA_A,
        "merge_lease": None,
        "expected_merge_parent": SHA_A,
        "controller_identity": "controller-test",
        "tasks": {str(value["number"]): value for value in tasks},
    }


def process_record(number: int, events: Path, session_id: str | None = None) -> dict:
    return {
        "pid": 99_999_999,
        "start_time": "dead",
        "token": f"token-{number}",
        "task": number,
        "executable": "/usr/bin/false",
        "model": "gpt-5.6-terra",
        "argv": ["/usr/bin/false", f"token-{number}"],
        "observed_command": f"/usr/bin/false token-{number}",
        "events": str(events),
        "final": str(events.with_name("final-1.md")),
        "segment": 1,
        "session_id": session_id,
        "offset": 0,
        "launch_count": 1,
    }


def git(repo: Path, *arguments: str, input_text: str | None = None) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo), *arguments],
        input=input_text,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


def initialize_git_repository(repo: Path) -> str:
    repo.mkdir(parents=True)
    subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
    git(repo, "config", "user.name", "Fleet Test")
    git(repo, "config", "user.email", "fleet@example.invalid")
    (repo / "owned.txt").write_text("base\n", encoding="utf-8")
    git(repo, "add", "owned.txt")
    git(repo, "commit", "-qm", "base")
    return git(repo, "rev-parse", "HEAD")


def write_signed_shaped_commit(repo: Path, branch: str) -> str:
    tree = git(repo, "mktree", input_text="")
    commit = (
        f"tree {tree}\n"
        "author Fleet Test <fleet@example.invalid> 1789804800 +0000\n"
        "committer Fleet Test <fleet@example.invalid> 1789804800 +0000\n"
        "gpgsig -----BEGIN PGP SIGNATURE-----\n"
        " fake-test-signature\n"
        " -----END PGP SIGNATURE-----\n"
        "\ncheckpoint\n"
    )
    sha = git(repo, "hash-object", "-t", "commit", "-w", "--stdin", input_text=commit)
    git(repo, "update-ref", f"refs/heads/{branch}", sha)
    return sha


def commit_from_parent(repo: Path, parent: str, message: str) -> str:
    tree = git(repo, "rev-parse", f"{parent}^{{tree}}")
    return git(
        repo,
        "commit-tree",
        tree,
        "-p",
        parent,
        input_text=message + "\n",
    )


def write_fake_gh(root: Path, pull_requests: dict[int, dict]) -> Path:
    payload = root / "pull-requests.json"
    payload.write_text(
        json.dumps({str(number): value for number, value in pull_requests.items()}),
        encoding="utf-8",
    )
    executable = root / "fake-gh"
    executable.write_text(
        "#!/usr/bin/env python3\n"
        "import json, pathlib, sys\n"
        f"payload = json.loads(pathlib.Path({str(payload)!r}).read_text())\n"
        "if sys.argv[1:3] != ['pr', 'view']:\n"
        "    raise SystemExit('repair and bound reconciliation must use gh pr view')\n"
        "print(json.dumps(payload[sys.argv[3]]))\n",
        encoding="utf-8",
    )
    executable.chmod(0o755)
    return executable


def write_repair_manifest(path: Path, manifest: dict) -> str:
    path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return fleet.sha256_file(path)


def repair_fixture(root: Path) -> dict:
    repository = root / "repository"
    current = initialize_git_repository(repository)
    initial_target = current
    remote = root / "remote.git"
    subprocess.run(["git", "init", "--bare", "-q", str(remote)], check=True)
    git(repository, "remote", "add", "origin", str(remote))
    task_metadata = {
        3: (886, "typed-consumers", "staging/beta1/typed-consumers"),
        4: (883, "conv-registry", "staging/beta1/conv-registry-integration-proof"),
        6: (875, "conformance-receipts", "staging/beta1/conformance-receipts"),
        7: (885, "file-session", "staging/beta1/file-session"),
    }
    facts: dict[int, dict] = {}
    pull_requests: dict[int, dict] = {}
    tasks: list[dict] = []
    target_name = "staging/beta1-functional-integration"
    for number, (pr, slug, branch) in task_metadata.items():
        base_sha = current
        head_sha = commit_from_parent(repository, base_sha, f"Task {number} head")
        merge_sha = commit_from_parent(
            repository, base_sha, f"Task {number} squash merge"
        )
        current = merge_sha
        git(repository, "update-ref", f"refs/heads/{branch}", head_sha)
        old_parent = f"{1000 + number:040x}"
        value = task(number, "committed")
        value["slug"] = slug
        value["expected_merge_parent"] = old_parent
        value["head_sha"] = head_sha
        tasks.append(value)
        facts[number] = {
            "task": number,
            "old_expected_merge_parent": old_parent,
            "pr": pr,
            "remote_branch": branch,
            "github_base_sha": base_sha,
            "merge_sha": merge_sha,
            "head_sha": head_sha,
        }
        pull_requests[pr] = {
            "number": pr,
            "url": f"https://example.invalid/pr/{pr}",
            "state": "MERGED",
            "isDraft": False,
            "headRefName": branch,
            "headRefOid": head_sha,
            "baseRefName": target_name,
            "baseRefOid": base_sha,
            "mergeCommit": {"oid": merge_sha},
            "statusCheckRollup": [
                {"status": "COMPLETED", "conclusion": "SUCCESS"}
            ],
        }
    git(repository, "update-ref", f"refs/heads/{target_name}", current)
    for number, (pr, _slug, branch) in task_metadata.items():
        git(repository, "push", "-q", "origin", f"{branch}:{branch}")
        git(
            remote,
            "update-ref",
            f"refs/pull/{pr}/head",
            facts[number]["head_sha"],
        )
    git(repository, "push", "-q", "origin", f"{target_name}:{target_name}")

    unrelated = task(9, "blocked", [8])
    unrelated["blocker"] = "dependency"
    unrelated["ruling"] = "preserve-unrelated-task"
    tasks.append(unrelated)
    controller_state = state(*tasks)
    controller_state["target_sha"] = current
    store = fleet.StateStore(root / "controller")
    store.write_snapshot(controller_state)
    store.append_event({"event": "before-repair"})
    store.write_receipt_index({"preserved": {"sha256": "9" * 64}})
    manifest = {
        "schema_version": 1,
        "transaction": "repair-merge-receipts",
        "before": {
            "fleet_state_sha256": fleet.sha256_file(store.snapshot_path),
            "fleet_events_sha256": fleet.sha256_file(store.events_path),
            "receipt_index_sha256": fleet.sha256_file(store.receipt_index_path),
        },
        "old_target_sha": current,
        "target_sha": current,
        "tasks": [dict(facts[number]) for number in sorted(facts)],
    }
    manifest_path = root / "repair-manifest.json"
    manifest_sha256 = write_repair_manifest(manifest_path, manifest)
    return {
        "store": store,
        "repository": repository,
        "remote": remote,
        "initial_target": initial_target,
        "facts": facts,
        "pull_requests": pull_requests,
        "manifest": manifest,
        "manifest_path": manifest_path,
        "manifest_sha256": manifest_sha256,
        "fake_gh": write_fake_gh(root, pull_requests),
        "unrelated": json.loads(json.dumps(unrelated)),
    }


class FleetControllerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.root = TEST_ROOT / self.id().rsplit(".", 1)[-1]
        fleet.remove_test_root(self.root)
        self.root.mkdir(parents=True)
        self.processes: list[int] = []

    def tearDown(self) -> None:
        for pid in self.processes:
            try:
                os.kill(pid, 15)
            except ProcessLookupError:
                pass

    def test_command_surface_is_complete(self) -> None:
        parser = fleet.build_parser()
        subcommands = next(
            action.choices
            for action in parser._actions
            if getattr(action, "choices", None)
        )
        self.assertEqual(
            set(subcommands),
            {
                "init",
                "materialize",
                "event",
                "checkpoint",
                "repair-merge-receipts",
                "reconcile",
                "recover",
                "launch",
                "monitor",
                "status",
                "heartbeat",
                "resume",
                "stop",
                "fixture",
            },
        )

    def test_remote_branch_is_optional_validated_and_used_without_changing_slug(self) -> None:
        canonical = task(4)
        canonical["slug"] = "conv-registry"
        fleet.validate_state(state(canonical))
        self.assertEqual(fleet._task_branch(canonical), "staging/beta1/conv-registry")

        renamed = json.loads(json.dumps(canonical))
        renamed["remote_branch"] = "staging/beta1/conv-registry-integration-proof"
        fleet.validate_state(state(renamed))
        self.assertEqual(renamed["slug"], "conv-registry")
        self.assertEqual(
            fleet._task_branch(renamed),
            "staging/beta1/conv-registry-integration-proof",
        )

        invalid = json.loads(json.dumps(canonical))
        for branch in (
            "refs/heads/main",
            "staging/beta1/foo./bar",
            "staging/beta1/foo/",
        ):
            invalid["remote_branch"] = branch
            with self.subTest(branch=branch), self.assertRaisesRegex(
                fleet.Refused, "remote_branch"
            ):
                fleet.validate_state(state(invalid))
        schema = json.loads(MODULE.with_name("fleet-schema.json").read_text())
        task_properties = schema["properties"]["tasks"]["additionalProperties"]["properties"]
        self.assertEqual(
            task_properties["remote_branch"]["pattern"],
            "^staging/beta1/(?!.*(?:\\.\\.|//|/\\.|\\.(?:lock)?(?:/|$)|/$))[a-z0-9][a-z0-9._/-]*$",
        )

    def test_repair_merge_receipts_authenticates_then_normal_reconcile_succeeds(self) -> None:
        fixture = repair_fixture(self.root)
        store = fixture["store"]
        before = store.read_snapshot()
        environment = {
            "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
            "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
        }
        with mock.patch.dict(os.environ, environment, clear=False):
            args = fleet.build_parser().parse_args(
                [
                    "repair-merge-receipts",
                    "--root",
                    str(store.root),
                    "--repo",
                    str(fixture["repository"]),
                    "--manifest",
                    str(fixture["manifest_path"]),
                    "--manifest-sha256",
                    fixture["manifest_sha256"],
                ]
            )
            result = fleet.dispatch(args)

        self.assertFalse(result["idempotent"])
        self.assertEqual(result["manifest_sha256"], fixture["manifest_sha256"])
        repaired = store.read_snapshot()
        for number, facts in fixture["facts"].items():
            value = repaired["tasks"][str(number)]
            self.assertEqual(value["expected_merge_parent"], facts["github_base_sha"])
            self.assertEqual(value["remote_branch"], facts["remote_branch"])
            self.assertEqual(value["pr_ci_state"]["pr"], facts["pr"])
            self.assertIsNone(value["merge_sha"])
            self.assertEqual(value["slug"], before["tasks"][str(number)]["slug"])
        self.assertEqual(repaired["tasks"]["9"], fixture["unrelated"])

        events = [json.loads(line) for line in store.events_path.read_text().splitlines()]
        repair_event = events[-1]
        self.assertEqual(repair_event["event"], "repair-merge-receipts")
        task4_event = repair_event["tasks"]["4"]
        self.assertEqual(
            task4_event["old"],
            {
                "expected_merge_parent": fixture["facts"][4]["old_expected_merge_parent"],
                "pr": None,
                "remote_branch": None,
                "head_sha": fixture["facts"][4]["head_sha"],
                "pushed_sha": None,
            },
        )
        self.assertEqual(
            task4_event["new"],
            {
                "expected_merge_parent": fixture["facts"][4]["github_base_sha"],
                "pr": 883,
                "remote_branch": "staging/beta1/conv-registry-integration-proof",
                "head_sha": fixture["facts"][4]["head_sha"],
                "pushed_sha": fixture["facts"][4]["head_sha"],
            },
        )
        index = store.read_receipt_index()
        receipt = index["merge_receipt_repairs"][fixture["manifest_sha256"]]
        self.assertEqual(receipt["manifest"], str(fixture["manifest_path"].resolve()))
        self.assertEqual(receipt["tasks"], repair_event["tasks"])
        self.assertEqual(index["preserved"], {"sha256": "9" * 64})
        journal = json.loads(
            fleet.repair_transaction_path(
                store, fixture["manifest_sha256"]
            ).read_text()
        )
        self.assertEqual(journal["phase"], "complete")

        with mock.patch.dict(os.environ, environment, clear=False):
            for number in (3, 4, 6, 7):
                reconciled = fleet.dispatch(
                    fleet.build_parser().parse_args(
                        [
                            "reconcile",
                            "--root",
                            str(store.root),
                            "--task",
                            str(number),
                            "--repo",
                            str(fixture["repository"]),
                        ]
                    )
                )
                self.assertEqual(reconciled["state"], "merged")
                self.assertEqual(
                    reconciled["merge_sha"], fixture["facts"][number]["merge_sha"]
                )

    def test_repair_merge_receipts_uses_pull_ref_after_branch_deletion_and_repairs_heads(self) -> None:
        fixture = repair_fixture(self.root)
        snapshot = fixture["store"].read_snapshot()
        snapshot["target_sha"] = fixture["initial_target"]
        for number, facts in fixture["facts"].items():
            git(
                fixture["remote"],
                "update-ref",
                "-d",
                f"refs/heads/{facts['remote_branch']}",
            )
            snapshot["tasks"][str(number)]["head_sha"] = None
            snapshot["tasks"][str(number)]["pushed_sha"] = (
                None if number % 2 else "f" * 40
            )
        fixture["store"].write_snapshot(snapshot)
        fixture["manifest"]["old_target_sha"] = fixture["initial_target"]
        fixture["manifest"]["before"]["fleet_state_sha256"] = fleet.sha256_file(
            fixture["store"].snapshot_path
        )
        manifest_sha256 = write_repair_manifest(
            fixture["manifest_path"], fixture["manifest"]
        )

        with mock.patch.dict(
            os.environ,
            {
                "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
                "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
            },
            clear=False,
        ):
            fleet.repair_merge_receipts(
                fixture["store"],
                fixture["repository"],
                fixture["manifest_path"],
                manifest_sha256,
            )

        repaired = fixture["store"].read_snapshot()
        self.assertEqual(repaired["target_sha"], fixture["manifest"]["target_sha"])
        for number, facts in fixture["facts"].items():
            self.assertEqual(
                repaired["tasks"][str(number)]["head_sha"], facts["head_sha"]
            )
            self.assertEqual(
                repaired["tasks"][str(number)]["pushed_sha"], facts["head_sha"]
            )

    def test_repair_merge_receipts_replay_is_exactly_idempotent(self) -> None:
        fixture = repair_fixture(self.root)
        environment = {
            "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
            "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
        }
        with mock.patch.dict(os.environ, environment, clear=False):
            first = fleet.repair_merge_receipts(
                fixture["store"],
                fixture["repository"],
                fixture["manifest_path"],
                fixture["manifest_sha256"],
            )
            after_first = {
                path.name: path.read_bytes()
                for path in (
                    fixture["store"].snapshot_path,
                    fixture["store"].events_path,
                    fixture["store"].receipt_index_path,
                )
            }
            second = fleet.repair_merge_receipts(
                fixture["store"],
                fixture["repository"],
                fixture["manifest_path"],
                fixture["manifest_sha256"],
            )
        after_second = {
            path.name: path.read_bytes()
            for path in (
                fixture["store"].snapshot_path,
                fixture["store"].events_path,
                fixture["store"].receipt_index_path,
            )
        }
        self.assertFalse(first["idempotent"])
        self.assertTrue(second["idempotent"])
        self.assertEqual(after_second, after_first)

    def test_repair_manifest_hashes_and_parses_the_same_bytes(self) -> None:
        fixture = repair_fixture(self.root)
        original = json.loads(json.dumps(fixture["manifest"]))
        changed = json.loads(json.dumps(original))
        changed["tasks"][0]["pr"] += 1
        real_sha256_file = fleet.sha256_file

        def hash_then_replace(path: Path) -> str:
            digest = real_sha256_file(path)
            path.write_text(
                json.dumps(changed, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            return digest

        with mock.patch.object(fleet, "sha256_file", side_effect=hash_then_replace):
            _path, loaded = fleet._load_repair_manifest(
                fixture["manifest_path"], fixture["manifest_sha256"]
            )
        self.assertEqual(loaded, original)

    def test_repair_replaces_stale_pr_metadata_with_only_authenticated_binding(self) -> None:
        fixture = repair_fixture(self.root)
        snapshot = fixture["store"].read_snapshot()
        snapshot["tasks"]["4"]["pr_ci_state"] = {
            "pr": 42,
            "url": "https://example.invalid/pr/42",
            "state": "open",
            "draft": True,
            "ci": "failure",
        }
        fixture["store"].write_snapshot(snapshot)
        fixture["manifest"]["before"]["fleet_state_sha256"] = fleet.sha256_file(
            fixture["store"].snapshot_path
        )
        manifest_sha256 = write_repair_manifest(
            fixture["manifest_path"], fixture["manifest"]
        )
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
                "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
            },
            clear=False,
        ):
            fleet.repair_merge_receipts(
                fixture["store"],
                fixture["repository"],
                fixture["manifest_path"],
                manifest_sha256,
            )
        self.assertEqual(
            fixture["store"].read_snapshot()["tasks"]["4"]["pr_ci_state"],
            {"pr": 883},
        )

    def test_repair_transaction_resumes_after_each_durable_write_boundary(self) -> None:
        for boundary in ("snapshot", "event", "receipt-index"):
            with self.subTest(boundary=boundary):
                fixture = repair_fixture(self.root / boundary)
                store = fixture["store"]
                environment = {
                    "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
                    "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
                }
                original_atomic_json = fleet.atomic_json
                original_append_event = fleet._append_event_locked
                failed = False

                def fail_after_atomic(path: Path, value: dict) -> None:
                    nonlocal failed
                    original_atomic_json(path, value)
                    target = (
                        boundary == "snapshot" and path == store.snapshot_path
                    ) or (
                        boundary == "receipt-index"
                        and path == store.receipt_index_path
                    )
                    if target and not failed:
                        failed = True
                        raise OSError(f"forced crash after {boundary}")

                def fail_after_event(
                    event_store: object, event: dict
                ) -> dict:
                    nonlocal failed
                    record = original_append_event(event_store, event)
                    if boundary == "event" and not failed:
                        failed = True
                        raise OSError("forced crash after event")
                    return record

                with mock.patch.dict(
                    os.environ, environment, clear=False
                ), mock.patch.object(
                    fleet, "atomic_json", side_effect=fail_after_atomic
                ), mock.patch.object(
                    fleet, "_append_event_locked", side_effect=fail_after_event
                ), self.assertRaisesRegex(OSError, "forced crash"):
                    fleet.repair_merge_receipts(
                        store,
                        fixture["repository"],
                        fixture["manifest_path"],
                        fixture["manifest_sha256"],
                    )
                with mock.patch.dict(os.environ, environment, clear=False):
                    resumed = fleet.repair_merge_receipts(
                        store,
                        fixture["repository"],
                        fixture["manifest_path"],
                        fixture["manifest_sha256"],
                    )
                self.assertFalse(resumed["idempotent"])
                self.assertEqual(
                    json.loads(
                        fleet.repair_transaction_path(
                            store, fixture["manifest_sha256"]
                        ).read_text()
                    )["phase"],
                    "complete",
                )
                self.assertIn(
                    fixture["manifest_sha256"],
                    store.read_receipt_index()["merge_receipt_repairs"],
                )

    def test_repair_transaction_recovers_a_torn_event_append(self) -> None:
        fixture = repair_fixture(self.root)
        store = fixture["store"]
        environment = {
            "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
            "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
        }

        def tear_event(event_store: object, event: dict) -> dict:
            encoded = (json.dumps(event, sort_keys=True) + "\n").encode("utf-8")
            with store.events_path.open("ab") as output:
                output.write(encoded[: len(encoded) // 2])
                output.flush()
                os.fsync(output.fileno())
            raise OSError("forced crash during event append")

        with mock.patch.dict(
            os.environ, environment, clear=False
        ), mock.patch.object(
            fleet, "_append_event_locked", side_effect=tear_event
        ), self.assertRaisesRegex(OSError, "during event append"):
            fleet.repair_merge_receipts(
                store,
                fixture["repository"],
                fixture["manifest_path"],
                fixture["manifest_sha256"],
            )

        with mock.patch.dict(os.environ, environment, clear=False):
            resumed = fleet.repair_merge_receipts(
                store,
                fixture["repository"],
                fixture["manifest_path"],
                fixture["manifest_sha256"],
            )
        self.assertFalse(resumed["idempotent"])
        events = [json.loads(line) for line in store.events_path.read_text().splitlines()]
        self.assertEqual(events[-1]["event"], "repair-merge-receipts")
        self.assertEqual(
            fleet.sha256_file(store.events_path),
            json.loads(
                fleet.repair_transaction_path(
                    store, fixture["manifest_sha256"]
                ).read_text()
            )["after"]["fleet_events_sha256"],
        )

    def test_pending_repair_fences_command_side_effects_before_dispatch(self) -> None:
        fixture = repair_fixture(self.root)
        store = fixture["store"]
        original_atomic_json = fleet.atomic_json

        def fail_before_snapshot(path: Path, value: dict) -> None:
            if path == store.snapshot_path:
                raise OSError("forced crash before snapshot")
            original_atomic_json(path, value)

        with mock.patch.dict(
            os.environ,
            {
                "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
                "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
            },
            clear=False,
        ), mock.patch.object(
            fleet, "atomic_json", side_effect=fail_before_snapshot
        ), self.assertRaisesRegex(OSError, "before snapshot"):
            fleet.repair_merge_receipts(
                store,
                fixture["repository"],
                fixture["manifest_path"],
                fixture["manifest_sha256"],
            )

        parser = fleet.build_parser()
        command_cases = (
            (
                "launch_task",
                [
                    "launch",
                    "--root",
                    str(store.root),
                    "--task",
                    "3",
                    "--repo",
                    str(fixture["repository"]),
                    "--executable",
                    "/usr/bin/false",
                ],
            ),
            (
                "materialize_prd",
                [
                    "materialize",
                    "--root",
                    str(store.root),
                    "--task",
                    "3",
                    "--plan",
                    str(fixture["manifest_path"]),
                ],
            ),
            (
                "stop_task",
                ["stop", "--root", str(store.root), "--task", "3"],
            ),
        )
        for operation_name, arguments in command_cases:
            with self.subTest(command=arguments[0]), mock.patch.object(
                fleet, operation_name
            ) as operation, self.assertRaisesRegex(
                fleet.Blocked, "pending merge-receipt repair"
            ):
                fleet.dispatch(parser.parse_args(arguments))
            operation.assert_not_called()

    def test_second_repair_manifest_cannot_strand_prepared_transaction(self) -> None:
        fixture = repair_fixture(self.root)
        store = fixture["store"]
        original_atomic_json = fleet.atomic_json
        environment = {
            "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
            "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
        }

        def fail_before_snapshot(path: Path, value: dict) -> None:
            if path == store.snapshot_path:
                raise OSError("forced crash before first snapshot")
            original_atomic_json(path, value)

        with mock.patch.dict(
            os.environ, environment, clear=False
        ), mock.patch.object(
            fleet, "atomic_json", side_effect=fail_before_snapshot
        ), self.assertRaisesRegex(OSError, "before first snapshot"):
            fleet.repair_merge_receipts(
                store,
                fixture["repository"],
                fixture["manifest_path"],
                fixture["manifest_sha256"],
            )

        original_journal = fleet.repair_transaction_path(
            store, fixture["manifest_sha256"]
        )
        self.assertEqual(json.loads(original_journal.read_text())["phase"], "prepared")
        second_manifest = json.loads(json.dumps(fixture["manifest"]))
        second_manifest["tasks"] = second_manifest["tasks"][:1]
        second_manifest_path = self.root / "second-repair-manifest.json"
        second_hash = write_repair_manifest(second_manifest_path, second_manifest)
        durable_paths = (
            store.snapshot_path,
            store.events_path,
            store.receipt_index_path,
            original_journal,
        )
        before = {path: path.read_bytes() for path in durable_paths}

        with mock.patch.dict(
            os.environ, environment, clear=False
        ), self.assertRaisesRegex(
            fleet.Blocked, "pending merge-receipt repair"
        ):
            fleet.repair_merge_receipts(
                store,
                fixture["repository"],
                second_manifest_path,
                second_hash,
            )

        self.assertEqual({path: path.read_bytes() for path in durable_paths}, before)
        self.assertFalse(fleet.repair_transaction_path(store, second_hash).exists())
        with mock.patch.dict(os.environ, environment, clear=False):
            resumed = fleet.repair_merge_receipts(
                store,
                fixture["repository"],
                fixture["manifest_path"],
                fixture["manifest_sha256"],
            )
        self.assertFalse(resumed["idempotent"])
        self.assertEqual(json.loads(original_journal.read_text())["phase"], "complete")

    def test_prepared_replay_refusal_never_mutates_any_durable_file(self) -> None:
        for case in (
            "divergent-event",
            "divergent-index",
            "planned-state-hash",
            "planned-event-hash",
            "planned-index-hash",
        ):
            with self.subTest(case=case):
                fixture = repair_fixture(self.root / case)
                store = fixture["store"]
                original_atomic_json = fleet.atomic_json
                environment = {
                    "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
                    "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
                }

                def fail_before_snapshot(path: Path, value: dict) -> None:
                    if path == store.snapshot_path:
                        raise OSError("forced crash before replay snapshot")
                    original_atomic_json(path, value)

                with mock.patch.dict(
                    os.environ, environment, clear=False
                ), mock.patch.object(
                    fleet, "atomic_json", side_effect=fail_before_snapshot
                ), self.assertRaisesRegex(OSError, "before replay snapshot"):
                    fleet.repair_merge_receipts(
                        store,
                        fixture["repository"],
                        fixture["manifest_path"],
                        fixture["manifest_sha256"],
                    )

                journal_path = fleet.repair_transaction_path(
                    store, fixture["manifest_sha256"]
                )
                if case == "divergent-event":
                    with store.events_path.open("a", encoding="utf-8") as output:
                        output.write('{"event":"divergent"}\n')
                elif case == "divergent-index":
                    index = store.read_receipt_index()
                    index["divergent"] = True
                    original_atomic_json(store.receipt_index_path, index)
                else:
                    journal = json.loads(journal_path.read_text())
                    field = {
                        "planned-state-hash": "fleet_state_sha256",
                        "planned-event-hash": "fleet_events_sha256",
                        "planned-index-hash": "receipt_index_sha256",
                    }[case]
                    journal["after"][field] = "0" * 64
                    original_atomic_json(journal_path, journal)

                durable_paths = (
                    store.snapshot_path,
                    store.events_path,
                    store.receipt_index_path,
                    journal_path,
                )
                before = {path: path.read_bytes() for path in durable_paths}
                with mock.patch.dict(
                    os.environ, environment, clear=False
                ), self.assertRaisesRegex(
                    fleet.Refused, "repair"
                ):
                    fleet.repair_merge_receipts(
                        store,
                        fixture["repository"],
                        fixture["manifest_path"],
                        fixture["manifest_sha256"],
                    )
                self.assertEqual(
                    {path: path.read_bytes() for path in durable_paths}, before
                )

    def test_monitor_follow_releases_lock_for_status_and_authenticated_stop(self) -> None:
        store = fleet.StateStore(self.root)
        current = task(0, "running")
        current["process"] = process_record(0, self.root / "events.jsonl")
        current["process"]["start_time"] = "start"
        store.write_snapshot(state(current))
        parser = fleet.build_parser()
        follow_args = parser.parse_args(
            ["monitor", "--root", str(store.root), "--task", "0", "--follow"]
        )
        status_args = parser.parse_args(["status", "--root", str(store.root)])
        stop_args = parser.parse_args(["stop", "--root", str(store.root), "--task", "0"])
        sleeping = threading.Event()
        release_sleep = threading.Event()
        worker_running = threading.Event()
        worker_running.set()
        results: dict[str, object] = {}

        def controlled_sleep(_seconds: float) -> None:
            sleeping.set()
            release_sleep.wait(2)

        def run(name: str, args: object) -> None:
            try:
                results[name] = fleet.dispatch(args)
            except BaseException as exc:
                results[name] = exc

        def authenticated_kill(_pid: int, _signal: int) -> None:
            worker_running.clear()

        patches = (
            mock.patch.object(fleet, "monitor_task", return_value={"events": []}),
            mock.patch.object(
                fleet, "worker_is_live", side_effect=lambda _record: worker_running.is_set()
            ),
            mock.patch.object(fleet, "process_start_time", return_value="start"),
            mock.patch.object(fleet, "process_command", return_value="worker token-0"),
            mock.patch.object(fleet.os, "kill", side_effect=authenticated_kill),
            mock.patch.object(fleet.time, "sleep", side_effect=controlled_sleep),
        )
        with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
            follow = threading.Thread(target=run, args=("follow", follow_args))
            status = threading.Thread(target=run, args=("status", status_args))
            stop = threading.Thread(target=run, args=("stop", stop_args))
            follow.start()
            self.assertTrue(sleeping.wait(1), "follow monitor never reached wait")
            status.start()
            stop.start()
            status_available = status.join(0.25) is None and not status.is_alive()
            stop_available = stop.join(0.25) is None and not stop.is_alive()
            release_sleep.set()
            follow.join(2)
            status.join(2)
            stop.join(2)

        self.assertTrue(status_available, "status blocked behind monitor --follow")
        self.assertTrue(stop_available, "stop blocked behind monitor --follow")
        self.assertFalse(follow.is_alive())
        self.assertIsInstance(results["status"], dict)
        self.assertEqual(results["stop"], {"stopped": True, "signal": "TERM"})
        self.assertNotIsInstance(results["follow"], BaseException)

    def test_repair_merge_receipts_refuses_stale_before_hash_and_manifest_hash(self) -> None:
        for case in ("before-state", "manifest"):
            with self.subTest(case=case):
                fixture = repair_fixture(self.root / case)
                if case == "before-state":
                    fixture["manifest"]["before"]["fleet_state_sha256"] = "0" * 64
                    supplied_hash = write_repair_manifest(
                        fixture["manifest_path"], fixture["manifest"]
                    )
                else:
                    supplied_hash = "0" * 64
                before = {
                    path.name: path.read_bytes()
                    for path in (
                        fixture["store"].snapshot_path,
                        fixture["store"].events_path,
                        fixture["store"].receipt_index_path,
                    )
                }
                environment = {
                    "FLEET_GH_EXECUTABLE": str(fixture["fake_gh"]),
                    "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
                }
                with mock.patch.dict(
                    os.environ, environment, clear=False
                ), self.assertRaisesRegex(fleet.Refused, "hash"):
                    fleet.repair_merge_receipts(
                        fixture["store"],
                        fixture["repository"],
                        fixture["manifest_path"],
                        supplied_hash,
                    )
                after = {
                    path.name: path.read_bytes()
                    for path in (
                        fixture["store"].snapshot_path,
                        fixture["store"].events_path,
                        fixture["store"].receipt_index_path,
                    )
                }
                self.assertEqual(after, before)

    def test_repair_merge_receipts_refuses_every_wrong_identity_without_partial_mutation(self) -> None:
        for case in (
            "pr",
            "head",
            "head-oid",
            "remote-head",
            "base",
            "merge",
            "parent",
            "old-target",
            "target",
            "old-parent",
            "cross-task-pr",
            "cross-task-branch",
        ):
            with self.subTest(case=case):
                fixture = repair_fixture(self.root / case)
                manifest = fixture["manifest"]
                pull_requests = fixture["pull_requests"]
                second = manifest["tasks"][1]
                pr = second["pr"]
                if case == "pr":
                    pull_requests[pr]["number"] = pr + 1000
                elif case == "head":
                    pull_requests[pr]["headRefName"] = "staging/beta1/wrong-head"
                elif case == "head-oid":
                    second["head_sha"] = fixture["facts"][3]["head_sha"]
                elif case == "remote-head":
                    git(
                        fixture["remote"],
                        "update-ref",
                        f"refs/heads/{second['remote_branch']}",
                        fixture["facts"][3]["head_sha"],
                    )
                elif case == "base":
                    pull_requests[pr]["baseRefOid"] = fixture["facts"][3][
                        "github_base_sha"
                    ]
                elif case == "merge":
                    pull_requests[pr]["mergeCommit"] = {
                        "oid": fixture["facts"][3]["merge_sha"]
                    }
                elif case == "parent":
                    second["github_base_sha"] = fixture["facts"][4]["head_sha"]
                    pull_requests[pr]["baseRefOid"] = second["github_base_sha"]
                elif case == "old-target":
                    manifest["old_target_sha"] = "e" * 40
                elif case == "target":
                    manifest["target_sha"] = fixture["initial_target"]
                else:
                    if case == "old-parent":
                        second["old_expected_merge_parent"] = "e" * 40
                    else:
                        snapshot = fixture["store"].read_snapshot()
                        if case == "cross-task-pr":
                            snapshot["tasks"]["9"]["pr_ci_state"] = {"pr": pr}
                        else:
                            snapshot["tasks"]["9"]["remote_branch"] = second[
                                "remote_branch"
                            ]
                        fixture["store"].write_snapshot(snapshot)
                        manifest["before"][
                            "fleet_state_sha256"
                        ] = fleet.sha256_file(fixture["store"].snapshot_path)
                supplied_hash = write_repair_manifest(
                    fixture["manifest_path"], manifest
                )
                fake_gh = write_fake_gh(self.root / case, pull_requests)
                before = {
                    path.name: path.read_bytes()
                    for path in (
                        fixture["store"].snapshot_path,
                        fixture["store"].events_path,
                        fixture["store"].receipt_index_path,
                    )
                }
                environment = {
                    "FLEET_GH_EXECUTABLE": str(fake_gh),
                    "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
                }
                with mock.patch.dict(
                    os.environ, environment, clear=False
                ), self.assertRaises((fleet.Refused, fleet.Blocked)):
                    fleet.repair_merge_receipts(
                        fixture["store"],
                        fixture["repository"],
                        fixture["manifest_path"],
                        supplied_hash,
                    )
                after = {
                    path.name: path.read_bytes()
                    for path in (
                        fixture["store"].snapshot_path,
                        fixture["store"].events_path,
                        fixture["store"].receipt_index_path,
                    )
                }
                self.assertEqual(after, before)

    def test_production_cli_refuses_caller_supplied_remote_observations(self) -> None:
        """Only authenticated Git/GitHub inventory may drive CLI reconciliation."""
        parser = fleet.build_parser()
        with self.assertRaises(SystemExit):
            parser.parse_args(
                [
                    "recover",
                    "--root",
                    str(self.root),
                    "--repo",
                    str(self.root),
                    "--remote-json",
                    '{"target_sha":"' + SHA_A + '","tasks":{}}',
                ]
            )

    def test_paths_are_fenced_snapshot_and_receipt_index_are_atomic(self) -> None:
        with self.assertRaisesRegex(fleet.Refused, "outside"):
            fleet.require_operational_path(Path("/tmp/not-durable"))
        store = fleet.StateStore(self.root)
        store.write_snapshot(state(task(0)))
        store.write_receipt_index({"0": {"sha256": "a" * 64}})
        self.assertEqual(
            json.loads((self.root / "fleet-state.json").read_text())["schema_version"],
            1,
        )
        self.assertEqual(
            json.loads((self.root / "receipt-index.json").read_text())["0"]["sha256"],
            "a" * 64,
        )
        self.assertFalse(list(self.root.glob("*.tmp-*")))

    def test_operational_paths_refuse_symlinks_even_when_the_target_is_in_root(self) -> None:
        real = self.root / "real"
        real.mkdir()
        alias = self.root / "alias"
        alias.symlink_to(real, target_is_directory=True)
        with self.assertRaisesRegex(fleet.Refused, "symlink"):
            fleet.require_operational_path(alias / "state.json")

    def test_init_is_idempotent_and_preserves_existing_receipts_and_identity(self) -> None:
        plan = self.root / "plan.md"
        spec_file = self.root / "spec.md"
        plan.write_text(
            "# Plan\n\n## Global Constraints\n\n- Keep durable.\n\n"
            "### Task 0: Durable Controller, Oracle, and Corpus Bootstrap\n\n"
            "**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode\n"
            "**Files:**\n\n- Create: `tools/release/fleet_controller.py`\n",
            encoding="utf-8",
        )
        spec_file.write_text("# Spec\n", encoding="utf-8")
        store = fleet.StateStore(self.root)
        first = fleet.initialize_controller(
            store, plan, spec_file, "origin/integration", SHA_A
        )
        store.write_receipt_index({"preserved": {"sha256": "a" * 64}})
        before_events = store.events_path.read_text(encoding="utf-8")

        second = fleet.initialize_controller(
            store, plan, spec_file, "origin/integration", SHA_A
        )

        self.assertEqual(second, first)
        self.assertEqual(second["controller_identity"], first["controller_identity"])
        self.assertEqual(
            store.read_receipt_index(), {"preserved": {"sha256": "a" * 64}}
        )
        self.assertEqual(
            store.events_path.read_text(encoding="utf-8"), before_events
        )

    def test_schema_requires_every_controller_and_task_recovery_field(self) -> None:
        complete = state(task(0))
        fleet.validate_state(complete)
        for field in (
            "plan_sha256",
            "spec_sha256",
            "target_ref",
            "target_sha",
            "merge_lease",
            "expected_merge_parent",
            "controller_identity",
        ):
            broken = json.loads(json.dumps(complete))
            broken.pop(field)
            with self.subTest(field=field), self.assertRaisesRegex(
                fleet.Refused, field
            ):
                fleet.validate_state(broken)
        for field in (
            "state_history",
            "dependencies",
            "file_lease",
            "worker",
            "process",
            "launch_count",
            "prd_sha256",
            "report_sha256",
            "review_sha256",
            "base_sha",
            "head_sha",
            "pushed_sha",
            "merge_sha",
            "target_sha",
            "paths",
            "heartbeat",
            "pr_ci_state",
            "receipt_hashes",
            "ruling",
            "blocker",
            "expected_merge_parent",
            "next_command",
        ):
            broken = json.loads(json.dumps(complete))
            broken["tasks"]["0"].pop(field)
            with self.subTest(task_field=field), self.assertRaisesRegex(
                fleet.Refused, field
            ):
                fleet.validate_state(broken)

    def test_json_schema_matches_runtime_required_recovery_fields(self) -> None:
        schema = json.loads(MODULE.with_name("fleet-schema.json").read_text())
        self.assertEqual(set(schema["required"]), fleet.STATE_REQUIRED)
        task_schema = schema["properties"]["tasks"]["additionalProperties"]
        self.assertEqual(set(task_schema["required"]), fleet.TASK_REQUIRED)
        self.assertEqual(
            set(task_schema["properties"]["worker"]["required"]),
            fleet.WORKER_REQUIRED,
        )
        self.assertEqual(
            set(task_schema["properties"]["paths"]["required"]),
            fleet.PATH_REQUIRED,
        )
        process_schema = schema["$defs"]["process"]
        self.assertEqual(set(process_schema["required"]), fleet.PROCESS_REQUIRED)

    def test_runtime_schema_rejects_incomplete_process_identity(self) -> None:
        complete = state(task(0, "running"))
        complete["tasks"]["0"]["process"] = {"pid": 123, "session_id": "session"}
        with self.assertRaisesRegex(fleet.Refused, "process missing"):
            fleet.validate_state(complete)
    def test_events_append_with_sequence_and_dependencies_require_remote_merge(self) -> None:
        store = fleet.StateStore(self.root)
        first = store.append_event({"event": "init"})
        second = store.append_event({"event": "checkpoint"})
        events = [json.loads(line) for line in (self.root / "fleet-events.jsonl").read_text().splitlines()]
        self.assertEqual([first["sequence"], second["sequence"]], [1, 2])
        self.assertEqual([entry["event"] for entry in events], ["init", "checkpoint"])
        with self.assertRaisesRegex(fleet.Blocked, "remote merge"):
            fleet.require_dependencies(task(2, dependencies=[1]), {"1": task(1, "committed")})
        merged = task(1, "merged")
        merged["merge_sha"] = SHA_B
        fleet.require_dependencies(task(2, dependencies=[1]), {"1": merged})

    def test_init_builds_all_plan_tasks_with_literal_dag_and_paths(self) -> None:
        plan = self.root / "plan.md"
        spec_file = self.root / "spec.md"
        plan.write_text(
            "# Plan\n\n## Global Constraints\n\n- Keep durable.\n\n"
            "### Task 0: Durable Controller, Oracle, and Corpus Bootstrap\n\n"
            "**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode\n"
            "**Files:**\n\n- Create: `tools/release/fleet_controller.py`\n\n"
            "### Task 1: Ownership Inventory and Duplicate-Owner Verifier\n\n"
            "**Worker:** Desktop, `gpt-5.6-terra`, fast mode\n"
            "**Depends on:** 0\n**Files:**\n\n- Create: `owned.py`\n",
            encoding="utf-8",
        )
        spec_file.write_text("# Spec\n", encoding="utf-8")
        value = fleet.initialize_state(plan, spec_file, "origin/integration", SHA_A, self.root)
        self.assertEqual(set(value["tasks"]), {"0", "1"})
        self.assertEqual(value["tasks"]["1"]["dependencies"], [0])
        self.assertEqual(value["tasks"]["0"]["file_lease"], ["tools/release/fleet_controller.py"])
        for item in value["tasks"].values():
            for path in item["paths"].values():
                fleet.require_operational_path(Path(path))

    def test_file_lease_includes_literal_adds_and_checkpoint_command(self) -> None:
        plan = self.root / "plan.md"
        spec_file = self.root / "spec.md"
        plan.write_text(
            "# Plan\n\n## Global Constraints\n\n- Keep durable.\n\n"
            "### Task 3: Typed Consumers\n\n"
            "**Files:**\n\n"
            "- Modify: `src/cli/output_formatter.rs`\n"
            "- Add: `tools/exiftool-tables/fixtures/typed_value_projection.json`\n"
            "- Modify: `src/cli/output_formatter.rs`\n"
            "- Add: `tests/typed_value_projection_tests.rs`\n"
            "- Do not modify Task 2 core files or generated/engine files\n"
            "- Do not modify: `tools/exiftool-tables/conformance.py`\n",
            encoding="utf-8",
        )
        spec_file.write_text("# Spec\n", encoding="utf-8")

        value = fleet.initialize_state(
            plan, spec_file, "origin/integration", SHA_A, self.root
        )
        task_value = value["tasks"]["3"]
        expected = [
            "src/cli/output_formatter.rs",
            "tests/typed_value_projection_tests.rs",
            "tools/exiftool-tables/fixtures/typed_value_projection.json",
        ]
        self.assertEqual(task_value["file_lease"], expected)
        footer = fleet._materialized_footer(task_value, SHA_A)
        self.assertIn(
            "git add -- "
            "src/cli/output_formatter.rs "
            "tests/typed_value_projection_tests.rs "
            "tools/exiftool-tables/fixtures/typed_value_projection.json",
            footer,
        )
        self.assertNotIn("Task 2 core files", " ".join(task_value["file_lease"]))
        self.assertNotIn("conformance.py", " ".join(task_value["file_lease"]))

    def test_canonical_plan_preserves_controller_owned_task_and_full_dag(self) -> None:
        repository = MODULE.parents[2]
        plan = repository / "docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md"
        spec_file = repository / "docs/superpowers/specs/2026-09-19-generated-runtime-release-functional-design.md"
        value = fleet.initialize_state(plan, spec_file, "origin/integration", SHA_A, self.root)
        self.assertEqual(set(value["tasks"]), {str(number) for number in range(21)})
        self.assertEqual(value["tasks"]["20"]["dependencies"], list(range(20)))
        self.assertEqual(value["tasks"]["20"]["worker"]["kind"], "Controller")
        self.assertEqual(value["tasks"]["20"]["worker"]["model"], "gpt-6-astra")
        self.assertEqual(len(value["tasks"]["0"]["file_lease"]), 11)

    def test_materialize_contains_only_global_contract_and_complete_selected_task(self) -> None:
        store = fleet.StateStore(self.root)
        value = state(task(0))
        store.write_snapshot(value)
        plan = self.root / "plan.md"
        plan.write_text(
            "# Plan\n\n## Global Constraints\n\n- Durable.\n\n"
            "### Task 0: Zero\n\nzero body\n\n"
            "### Task 1: One\n\none body\n\n## Execution Handoff\nend\n",
            encoding="utf-8",
        )
        prd = fleet.materialize_prd(store, plan, 0, SHA_A, value["tasks"]["0"]["paths"])
        content = prd.read_text()
        self.assertIn("## Global Constraints", content)
        self.assertIn("### Task 0: Zero", content)
        self.assertIn("base_sha: " + SHA_A, content)
        self.assertIn("RETURN_TO_CONTROLLER", content)
        self.assertNotIn("### Task 1: One", content)
        index = json.loads((self.root / "receipt-index.json").read_text())
        self.assertEqual(index["0"]["sha256"], fleet.sha256_file(prd))

    def test_materialize_uses_worker_kind_correct_launch_instructions(self) -> None:
        store = fleet.StateStore(self.root)
        cli = task(0)
        desktop = task(1)
        desktop["worker"]["kind"] = "Desktop"
        controller = task(2)
        controller["worker"]["kind"] = "Controller"
        value = state(cli, desktop, controller)
        store.write_snapshot(value)
        plan = self.root / "plan.md"
        plan.write_text(
            "# Plan\n\n## Global Constraints\n\n- Durable.\n\n"
            "### Task 0: CLI task\n\n"
            "**Worker:** Codex CLI, `gpt-5.6-terra`, fast mode\n\n"
            "cli body\n\n"
            "### Task 1: Desktop task\n\n"
            "**Worker:** Desktop subagent, `gpt-5.6-terra`\n\n"
            "desktop body\n\n"
            "### Task 2: Controller task\n\n"
            "**Worker:** Controller-owned; no implementation worker\n\n"
            "controller body\n\n"
            "## Execution Handoff\nend\n",
            encoding="utf-8",
        )

        cli_text = fleet.materialize_prd(store, plan, 0, SHA_A).read_text()
        desktop_text = fleet.materialize_prd(store, plan, 1, SHA_A).read_text()
        controller_text = fleet.materialize_prd(store, plan, 2, SHA_A).read_text()

        self.assertIn("fleet_controller.py launch", cli_text)
        self.assertNotIn("collaboration.spawn_agent", cli_text)
        self.assertIn("collaboration.spawn_agent", desktop_text)
        self.assertIn('"fork_turns": "none"', desktop_text)
        self.assertNotIn("fleet_controller.py launch", desktop_text)
        self.assertIn("Controller-owned task; do not launch an implementation worker", controller_text)
        self.assertNotIn("fleet_controller.py launch", controller_text)
        self.assertNotIn("collaboration.spawn_agent", controller_text)

    def test_materialize_refreshes_worker_policy_from_current_plan_before_launch(self) -> None:
        store = fleet.StateStore(self.root)
        current = task(8)
        current["worker"]["identity"] = "stale-worker-identity"
        store.write_snapshot(state(current))
        plan = self.root / "plan.md"
        plan.write_text(
            "# Plan\n\n## Global Constraints\n\n- Durable.\n\n"
            "### Task 8: Generated attribution\n\n"
            "**Worker:** Codex CLI, `gpt-5.6-terra`\n\n"
            "attribute generated tables without enabling fast mode\n\n"
            "## Execution Handoff\nend\n",
            encoding="utf-8",
        )

        prd = fleet.materialize_prd(store, plan, 8, SHA_A)

        materialized = store.read_snapshot()["tasks"]["8"]
        self.assertEqual(materialized["worker"]["kind"], "CLI")
        self.assertEqual(materialized["worker"]["model"], "gpt-5.6-terra")
        self.assertEqual(materialized["worker"]["effort"], "medium")
        self.assertIsNone(materialized["worker"]["identity"])
        content = prd.read_text(encoding="utf-8")
        self.assertIn("Worker policy: `CLI` / `gpt-5.6-terra` / `medium`", content)
        self.assertNotIn("--enable fast_mode", content)

    def test_materialize_refreshes_file_lease_from_task8_style_plan(self) -> None:
        store = fleet.StateStore(self.root)
        current = task(8)
        current["file_lease"] = ["stale/retired-lease-entry.rs"]
        store.write_snapshot(state(current))
        plan = MODULE.parents[2] / "docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md"

        prd = fleet.materialize_prd(store, plan, 8, SHA_A)

        expected = [
            "src/exiftool_tables/attribution.rs",
            "tools/exiftool-tables/genshare/attribute.py",
            "tools/exiftool-tables/genshare/census.sh",
            "tools/exiftool-tables/genshare/probe.patch",
            "tools/exiftool-tables/genshare/README.md",
            "tools/exiftool-tables/genshare/test_attribute.py",
            "tools/exiftool-tables/genshare/test_census.py",
            "tools/exiftool-tables/genshare/testdata/bounded-corpus.txt",
            "src/exiftool_tables/mod.rs",
            "src/exiftool_tables/engine.rs",
            "src/exiftool_tables/ifd_engine.rs",
            "src/exiftool_tables/keyed_engine.rs",
            "src/exiftool_tables/serial_engine.rs",
            "src/exiftool_tables/runtime.rs",
        ]
        expected.sort()
        materialized = store.read_snapshot()["tasks"]["8"]
        self.assertEqual(materialized["file_lease"], expected)
        self.assertEqual(len(materialized["file_lease"]), 14)
        self.assertNotIn("stale/retired-lease-entry.rs", materialized["file_lease"])
        self.assertIn("- `tools/exiftool-tables/genshare/probe.patch`", prd.read_text())

    def test_file_lease_refuses_nonliteral_file_operation(self) -> None:
        section = "**Files:**\n\n- Add: tools/exiftool-tables/genshare/test_attribute.py\n"

        with self.assertRaisesRegex(fleet.Refused, "literal backtick path"):
            fleet._parse_file_lease(section)

    def test_file_lease_refuses_suffix_after_literal_operation_path(self) -> None:
        for entry in (
            "- Add: `a.py` or `b.py`",
            "- Modify: `a.py` only after approval",
        ):
            with self.subTest(entry=entry), self.assertRaisesRegex(
                fleet.Refused, "literal backtick path"
            ):
                fleet._parse_file_lease("**Files:**\n\n" + entry + "\n")

    def test_rendered_prd_file_lease_refuses_mixed_none_and_paths_in_both_orders(self) -> None:
        prd = self.root / "prds/mixed-none.md"
        prd.parent.mkdir(parents=True, exist_ok=True)
        for lines in (
            "- none\n- `a.py`\n",
            "- `a.py`\n- none\n",
        ):
            with self.subTest(lines=lines):
                prd.write_text(
                    "# Materialized Beta 1 Functional Task PRD\n\n"
                    "### Exact file lease\n\n" + lines,
                    encoding="utf-8",
                )
                with self.assertRaisesRegex(fleet.Blocked, "ambiguous file lease"):
                    fleet._rendered_prd_file_lease(prd)

    def test_launch_preflight_refuses_prd_lease_that_differs_from_state(self) -> None:
        repository = self.root / "launch-preflight-repository"
        sha = initialize_git_repository(repository)
        git(repository, "checkout", "-qb", "staging/beta1/task-0")
        current = task(0)
        current["paths"]["worktree"] = str(repository)
        current["base_sha"] = sha
        current["target_sha"] = sha
        current["expected_merge_parent"] = sha
        prd = Path(current["paths"]["prd"])
        prd.parent.mkdir(parents=True, exist_ok=True)
        prd.write_text(
            "# Materialized Beta 1 Functional Task PRD\n\n"
            "### Exact file lease\n\n"
            "- `different-file.txt`\n",
            encoding="utf-8",
        )
        current["prd_sha256"] = fleet.sha256_file(prd)
        value = state(current)
        value["target_sha"] = sha
        value["expected_merge_parent"] = sha

        with self.assertRaisesRegex(fleet.Blocked, "PRD file lease"):
            fleet.validate_launch_preflight(current, value)

    def test_launch_preflight_requires_all_five_materialized_identities(self) -> None:
        repository = self.root / "aligned-launch-preflight-repository"
        sha = initialize_git_repository(repository)
        git(repository, "checkout", "-qb", "staging/beta1/task-0")
        (repository / "HANDOFF.md").write_text("untracked but allowed\n", encoding="utf-8")
        current = task(0)
        current["paths"]["worktree"] = str(repository)
        for field in ("base_sha", "target_sha", "expected_merge_parent"):
            current[field] = sha
        prd = Path(current["paths"]["prd"])
        prd.parent.mkdir(parents=True, exist_ok=True)
        prd.write_text(
            "# Materialized Beta 1 Functional Task PRD\n\n"
            "### Exact file lease\n\n"
            "- `task-0.txt`\n",
            encoding="utf-8",
        )
        current["prd_sha256"] = fleet.sha256_file(prd)
        aligned = state(current)
        aligned["target_sha"] = sha

        fleet.validate_launch_preflight(current, aligned)

        for location, field in (
            ("task", "base_sha"),
            ("task", "target_sha"),
            ("task", "expected_merge_parent"),
            ("controller", "target_sha"),
        ):
            broken = json.loads(json.dumps(aligned))
            target = broken["tasks"]["0"] if location == "task" else broken
            target[field] = SHA_A
            with self.subTest(identity=f"{location} {field}"), self.assertRaisesRegex(
                fleet.Blocked, "launch identities diverged"
            ):
                fleet.validate_launch_preflight(broken["tasks"]["0"], broken)

        (repository / "owned.txt").write_text("new head\n", encoding="utf-8")
        git(repository, "add", "owned.txt")
        git(repository, "commit", "-qm", "different head")
        with self.assertRaisesRegex(fleet.Blocked, "launch identities diverged"):
            fleet.validate_launch_preflight(current, aligned)

    def test_materialize_preserves_identity_when_worker_policy_is_unchanged(self) -> None:
        store = fleet.StateStore(self.root)
        current = task(0)
        current["worker"]["identity"] = "live-worker-identity"
        current["worker"]["effort"] = "medium"
        store.write_snapshot(state(current))
        plan = self.root / "plan.md"
        plan.write_text(
            "# Plan\n\n## Global Constraints\n\n- Durable.\n\n"
            "### Task 0: Zero\n\n"
            "**Worker:** Codex CLI, `gpt-5.6-terra`\n\n"
            "body\n\n## Execution Handoff\nend\n",
            encoding="utf-8",
        )

        fleet.materialize_prd(store, plan, 0, SHA_A)

        self.assertEqual(
            store.read_snapshot()["tasks"]["0"]["worker"]["identity"],
            "live-worker-identity",
        )

    def test_event_checkpoint_and_reconcile_are_idempotent(self) -> None:
        repository = self.root / "repository"
        repository.mkdir()
        subprocess.run(["git", "init", "-q"], cwd=repository, check=True)
        checkpoint_sha = write_signed_shaped_commit(
            repository, "staging/beta1/task-0"
        )
        store = fleet.StateStore(self.root)
        value = state(task(0))
        store.write_snapshot(value)
        fleet.record_external_event(store, 0, "desktop-identity", {"state": "running"})
        fleet.record_checkpoint(
            store,
            0,
            head_sha=checkpoint_sha,
            pushed_sha=None,
            repo=repository,
        )
        remote = {
            "head_sha": checkpoint_sha,
            "pushed_sha": checkpoint_sha,
            "pr": 900,
            "ci": "pending",
        }
        first = fleet.reconcile_task(store, 0, remote)
        second = fleet.reconcile_task(store, 0, remote)
        self.assertEqual(first, second)
        self.assertEqual(second["worker"]["identity"], "desktop-identity")
        self.assertEqual(second["pushed_sha"], checkpoint_sha)

    def test_checkpoint_authenticates_the_task_branch_with_or_without_worktree(self) -> None:
        repository = self.root / "repository"
        repository.mkdir()
        subprocess.run(["git", "init", "-q"], cwd=repository, check=True)
        checkpoint_sha = write_signed_shaped_commit(
            repository, "staging/beta1/task-0"
        )
        current = task(0)
        current["paths"]["worktree"] = str(self.root / "missing-worktree")
        store = fleet.StateStore(self.root)
        store.write_snapshot(state(current))

        recorded = fleet.record_checkpoint(
            store,
            0,
            head_sha=checkpoint_sha,
            pushed_sha=None,
            repo=repository,
        )
        self.assertEqual(recorded["head_sha"], checkpoint_sha)

        with self.assertRaisesRegex(fleet.Blocked, "task branch|commit"):
            fleet.record_checkpoint(
                store,
                0,
                head_sha="f" * 40,
                pushed_sha=None,
                repo=repository,
            )

        with self.assertRaisesRegex(fleet.Blocked, "repository"):
            fleet.record_checkpoint(
                store,
                0,
                head_sha=checkpoint_sha,
                pushed_sha=None,
            )

    def test_recovery_observes_worktree_and_refuses_changes_outside_lease(self) -> None:
        worktree = self.root / "worktree"
        worktree.mkdir()
        subprocess.run(["git", "init", "-q"], cwd=worktree, check=True)
        subprocess.run(["git", "config", "user.name", "Fleet Test"], cwd=worktree, check=True)
        subprocess.run(["git", "config", "user.email", "fleet@example.invalid"], cwd=worktree, check=True)
        (worktree / "owned.txt").write_text("owned\n", encoding="utf-8")
        subprocess.run(["git", "add", "owned.txt"], cwd=worktree, check=True)
        subprocess.run(["git", "commit", "-qm", "base"], cwd=worktree, check=True)
        current = task(0, "committed")
        current["paths"]["worktree"] = str(worktree)
        current["file_lease"] = ["owned.txt"]
        (worktree / "HANDOFF.md").write_text("handoff\n", encoding="utf-8")
        (worktree / "owned.txt").write_text("modified but leased\n", encoding="utf-8")
        observation = fleet.inspect_worktree(current)
        self.assertEqual(len(observation["head_sha"]), 40)
        self.assertEqual(len(observation["handoff_sha256"]), 64)
        self.assertEqual(observation["changed_paths"], ["owned.txt"])

        (worktree / "rogue.txt").write_text("outside lease\n", encoding="utf-8")
        with self.assertRaisesRegex(fleet.Blocked, "outside file lease"):
            fleet.inspect_worktree(current)

    def test_recovery_ignores_preserved_edits_in_terminal_worktree(self) -> None:
        worktree = self.root / "terminal-worktree"
        initialize_git_repository(worktree)
        (worktree / "post-merge-edit.txt").write_text("preserved\n", encoding="utf-8")
        terminal = task(0, "merged")
        terminal["merge_sha"] = SHA_B
        terminal["paths"]["worktree"] = str(worktree)
        terminal["file_lease"] = ["owned.txt"]
        store = fleet.StateStore(self.root / "controller")
        store.write_snapshot(state(terminal))

        recovered = fleet.recover_controller_for_test(
            store,
            self.root,
            {"target_sha": SHA_A, "tasks": {}},
        )

        self.assertNotIn("0", recovered["actions"])
        self.assertEqual(store.read_snapshot()["tasks"]["0"]["state"], "merged")

    def test_recover_reconciles_remote_branch_pr_ci_and_merge_read_only(self) -> None:
        repository = self.root / "repository"
        head_sha = initialize_git_repository(repository)
        remote = self.root / "remote.git"
        subprocess.run(["git", "init", "--bare", "-q", str(remote)], check=True)
        git(repository, "remote", "add", "origin", str(remote))
        branches = (
            "staging/beta1-functional-integration",
            "staging/beta1/task-0",
            "staging/beta1/task-1",
        )
        for branch in branches:
            git(repository, "branch", branch, head_sha)
            git(repository, "push", "-q", "origin", f"{branch}:{branch}")

        pull_requests = [
            {
                "number": 100,
                "url": "https://example.invalid/pr/100",
                "state": "OPEN",
                "isDraft": True,
                "headRefName": "staging/beta1/task-0",
                "headRefOid": head_sha,
                "baseRefName": "staging/beta1-functional-integration",
                "baseRefOid": SHA_A,
                "mergeCommit": None,
                "statusCheckRollup": [
                    {"status": "COMPLETED", "conclusion": "SUCCESS"}
                ],
            },
            {
                "number": 101,
                "url": "https://example.invalid/pr/101",
                "state": "MERGED",
                "isDraft": False,
                "headRefName": "staging/beta1/task-1",
                "headRefOid": head_sha,
                "baseRefName": "staging/beta1-functional-integration",
                "baseRefOid": SHA_A,
                "mergeCommit": {"oid": head_sha},
                "statusCheckRollup": [
                    {"status": "COMPLETED", "conclusion": "SUCCESS"}
                ],
            },
        ]
        fake_gh = self.root / "fake-gh"
        fake_gh.write_text(
            "#!/usr/bin/env python3\n"
            f"payload = {pull_requests!r}\n"
            "import json\n"
            "print(json.dumps(payload))\n",
            encoding="utf-8",
        )
        fake_gh.chmod(0o755)

        pushed = task(0, "committed")
        merged = task(1, "pushed")
        dependent = task(2, "blocked", [1])
        dependent["blocker"] = "dependency"
        controller_state = state(pushed, merged, dependent)
        controller_state["target_sha"] = head_sha
        store = fleet.StateStore(self.root / "controller")
        store.write_snapshot(controller_state)
        remote_before = subprocess.run(
            ["git", "--git-dir", str(remote), "show-ref"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        status_before = git(repository, "status", "--porcelain=v1")

        with mock.patch.dict(
            os.environ,
            {
                "FLEET_GH_EXECUTABLE": str(fake_gh),
                "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
            },
            clear=False,
        ):
            recovered = fleet.recover_controller(store, repository)

        after = store.read_snapshot()
        self.assertEqual(after["tasks"]["0"]["pushed_sha"], head_sha)
        self.assertEqual(after["tasks"]["0"]["state"], "pushed")
        self.assertEqual(after["tasks"]["0"]["pr_ci_state"]["ci"], "success")
        self.assertEqual(after["tasks"]["1"]["state"], "merged")
        self.assertEqual(after["tasks"]["1"]["merge_sha"], head_sha)
        self.assertEqual(after["tasks"]["2"]["state"], "launchable")
        self.assertEqual(recovered["actions"]["2"], "materialize --task 2")
        self.assertEqual(git(repository, "status", "--porcelain=v1"), status_before)
        remote_after = subprocess.run(
            ["git", "--git-dir", str(remote), "show-ref"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        self.assertEqual(remote_after, remote_before)

    def test_recover_does_not_release_explicit_typed_producer_blocker(self) -> None:
        dependency = task(2, "merged")
        dependency["merge_sha"] = SHA_B
        blocked = task(3, "blocked", [2])
        blocked["blocker"] = "typed-producer"
        blocked["next_command"] = "review --task 3"
        store = fleet.StateStore(self.root / "controller")
        store.write_snapshot(state(dependency, blocked))
        remote = {
            "target_sha": SHA_A,
            "tasks": {"2": {"merge_sha": SHA_B, "merge_parent": SHA_A}},
        }

        first = fleet.recover_controller_for_test(store, self.root, remote)
        after_first = store.read_snapshot()
        second = fleet.recover_controller_for_test(store, self.root, remote)
        after_second = store.read_snapshot()

        self.assertEqual(after_first, after_second)
        self.assertEqual(after_second["tasks"]["2"]["state"], "merged")
        self.assertEqual(after_second["tasks"]["3"]["state"], "blocked")
        self.assertEqual(after_second["tasks"]["3"]["blocker"], "typed-producer")
        self.assertEqual(after_second["tasks"]["3"]["next_command"], "review --task 3")
        self.assertEqual(second["actions"]["3"], "reconcile --task 3")
        self.assertEqual(first["actions"], second["actions"])

    def test_recover_preserves_review_and_runtime_blockers(self) -> None:
        remote = {
            "target_sha": SHA_A,
            "tasks": {"2": {"merge_sha": SHA_B, "merge_parent": SHA_A}},
        }
        for blocker in ("review", "runtime"):
            with self.subTest(blocker=blocker):
                dependency = task(2, "merged")
                dependency["merge_sha"] = SHA_B
                blocked = task(3, "blocked", [2])
                blocked["blocker"] = blocker
                store = fleet.StateStore(self.root / f"controller-{blocker}")
                store.write_snapshot(state(dependency, blocked))

                fleet.recover_controller_for_test(store, self.root, remote)
                fleet.recover_controller_for_test(store, self.root, remote)

                recovered = store.read_snapshot()["tasks"]["3"]
                self.assertEqual(recovered["state"], "blocked")
                self.assertEqual(recovered["blocker"], blocker)

    def test_recover_preserves_explicit_blockers_with_remote_pushed_task(self) -> None:
        remote = {
            "target_sha": SHA_A,
            "tasks": {
                "3": {
                    "head_sha": SHA_B,
                    "pushed_sha": SHA_B,
                    "pr": 103,
                    "pr_state": "OPEN",
                    "ci": "pending",
                }
            },
        }
        for blocker in ("manual", "review", "runtime"):
            with self.subTest(blocker=blocker):
                blocked = task(3, "blocked")
                blocked["blocker"] = blocker
                blocked["next_command"] = f"{blocker} --task 3"
                store = fleet.StateStore(self.root / f"remote-{blocker}")
                store.write_snapshot(state(blocked))

                first = fleet.recover_controller_for_test(store, self.root, remote)
                after_first = store.read_snapshot()
                second = fleet.recover_controller_for_test(store, self.root, remote)
                after_second = store.read_snapshot()

                self.assertEqual(after_first, after_second)
                recovered = after_second["tasks"]["3"]
                self.assertEqual(recovered["state"], "blocked")
                self.assertEqual(recovered["blocker"], blocker)
                self.assertEqual(recovered["next_command"], f"{blocker} --task 3")
                self.assertEqual(second["actions"]["3"], "reconcile --task 3")
                self.assertEqual(first["actions"], second["actions"])

    def test_recover_does_not_adopt_live_process_for_explicit_blockers(self) -> None:
        remote = {"target_sha": SHA_A, "tasks": {}}
        for blocker in ("manual", "review", "runtime"):
            with self.subTest(blocker=blocker):
                store = fleet.StateStore(self.root / f"live-process-{blocker}")
                blocked = task(3, "blocked")
                blocked["blocker"] = blocker
                blocked["next_command"] = f"{blocker} --task 3"
                store.write_snapshot(state(blocked))
                process = subprocess.Popen(
                    [sys.executable, "-c", "import time; time.sleep(30)", blocker],
                    start_new_session=True,
                )
                self.processes.append(process.pid)
                process_root = store.root / "processes/03"
                process_root.mkdir(parents=True)
                argv = fleet.process_kernel_argv(process.pid)
                durable = {
                    "pid": process.pid,
                    "start_time": fleet.process_start_time(process.pid),
                    "token": blocker,
                    "task": 3,
                    "executable": sys.executable,
                    "model": "gpt-5.6-terra",
                    "argv": argv,
                    "observed_command": fleet.process_command(process.pid),
                    "events": str(process_root / "events-1.jsonl"),
                    "final": str(process_root / "final-1.md"),
                    "segment": 1,
                    "session_id": f"{blocker}-session",
                    "offset": 0,
                    "launch_count": 1,
                    "kernel_argv": argv,
                    "kernel_executable": fleet.process_kernel_executable(process.pid),
                    "process_group": os.getpgid(process.pid),
                }
                fleet.atomic_json(process_root / "process-1.json", durable)

                first = fleet.recover_controller_for_test(store, self.root, remote)
                after_first = store.read_snapshot()
                second = fleet.recover_controller_for_test(store, self.root, remote)
                after_second = store.read_snapshot()

                self.assertEqual(after_first, after_second)
                recovered = after_second["tasks"]["3"]
                self.assertEqual(recovered["state"], "blocked")
                self.assertEqual(recovered["blocker"], blocker)
                self.assertEqual(recovered["next_command"], f"{blocker} --task 3")
                self.assertIsNone(recovered["process"])
                self.assertEqual(second["actions"]["3"], "reconcile --task 3")
                self.assertEqual(first["actions"], second["actions"])

    def test_recover_does_not_adopt_stale_process_for_explicit_blockers(self) -> None:
        remote = {"target_sha": SHA_A, "tasks": {}}
        for blocker in ("manual", "review", "runtime"):
            with self.subTest(blocker=blocker):
                store = fleet.StateStore(self.root / f"stale-process-{blocker}")
                blocked = task(3, "blocked")
                blocked["blocker"] = blocker
                blocked["next_command"] = f"{blocker} --task 3"
                store.write_snapshot(state(blocked))
                process_root = store.root / "processes/03"
                process_root.mkdir(parents=True)
                fleet.atomic_json(
                    process_root / "process-1.json",
                    process_record(3, process_root / "events-1.jsonl", "stale-session"),
                )

                first = fleet.recover_controller_for_test(store, self.root, remote)
                after_first = store.read_snapshot()
                second = fleet.recover_controller_for_test(store, self.root, remote)
                after_second = store.read_snapshot()

                self.assertEqual(after_first, after_second)
                recovered = after_second["tasks"]["3"]
                self.assertEqual(recovered["state"], "blocked")
                self.assertEqual(recovered["blocker"], blocker)
                self.assertEqual(recovered["next_command"], f"{blocker} --task 3")
                self.assertIsNone(recovered["process"])
                self.assertEqual(second["actions"]["3"], "reconcile --task 3")
                self.assertEqual(first["actions"], second["actions"])

    def test_recover_does_not_adopt_prepared_intent_for_explicit_blockers(self) -> None:
        remote = {"target_sha": SHA_A, "tasks": {}}
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nwhile :; do sleep 1; done\n", encoding="utf-8")
        fake.chmod(0o755)
        for blocker in ("manual", "review", "runtime"):
            with self.subTest(blocker=blocker):
                store = fleet.StateStore(self.root / f"intent-{blocker}")
                store.write_snapshot(state(task(3, "launchable")))
                worktree = self.root / f"worktree-{blocker}"
                worktree.mkdir()
                prd = self.root / f"prd-{blocker}.md"
                prd.write_text("task\n", encoding="utf-8")
                record = fleet.launch_worker(
                    store,
                    fake,
                    worktree,
                    f"intent-{blocker}-token",
                    prd,
                    task_number=3,
                    model="gpt-5.6-terra",
                    segment=1,
                )
                self.processes.append(record["pid"])
                intent_path = fleet.launch_intent_path(store, 3, 1)
                intent = json.loads(intent_path.read_text(encoding="utf-8"))
                intent["lifecycle"] = "prepared"
                intent.pop("record", None)
                fleet.atomic_json(intent_path, intent)
                (store.root / "processes/03/process-1.json").unlink()

                def block(snapshot: dict[str, object]) -> None:
                    current = snapshot["tasks"]["3"]
                    fleet._transition(current, "blocked")
                    current["blocker"] = blocker
                    current["process"] = None
                    current["launch_count"] = 0
                    current["next_command"] = f"{blocker} --task 3"

                store.mutate(block)
                first = fleet.recover_controller_for_test(store, self.root, remote)
                after_first = store.read_snapshot()
                second = fleet.recover_controller_for_test(store, self.root, remote)
                after_second = store.read_snapshot()

                self.assertEqual(after_first, after_second)
                recovered = after_second["tasks"]["3"]
                self.assertEqual(recovered["state"], "blocked")
                self.assertEqual(recovered["blocker"], blocker)
                self.assertIsNone(recovered["process"])
                self.assertEqual(recovered["launch_count"], 0)
                self.assertEqual(recovered["next_command"], f"{blocker} --task 3")
                self.assertEqual(
                    json.loads(intent_path.read_text(encoding="utf-8"))["lifecycle"],
                    "prepared",
                )
                self.assertEqual(second["actions"]["3"], "reconcile --task 3")
                self.assertEqual(first["actions"], second["actions"])

    def test_remote_pr_must_target_the_controller_integration_branch(self) -> None:
        """A valid PR against another base cannot release this controller's task."""
        repository = self.root / "repository"
        head_sha = initialize_git_repository(repository)
        remote = self.root / "remote.git"
        subprocess.run(["git", "init", "--bare", "-q", str(remote)], check=True)
        git(repository, "remote", "add", "origin", str(remote))
        for branch in (
            "staging/beta1-functional-integration",
            "staging/beta1/task-0",
        ):
            git(repository, "branch", branch, head_sha)
            git(repository, "push", "-q", "origin", f"{branch}:{branch}")
        fake_gh = self.root / "fake-gh"
        fake_gh.write_text(
            "#!/usr/bin/env python3\n"
            "import json\n"
            "print(json.dumps([{"
            "'number': 100, 'url': 'https://example.invalid/pr/100', "
            "'state': 'OPEN', 'isDraft': False, "
            "'headRefName': 'staging/beta1/task-0', "
            f"'headRefOid': '{head_sha}', 'baseRefName': 'main', "
            f"'baseRefOid': '{head_sha}', 'mergeCommit': None, "
            "'statusCheckRollup': [{'status': 'COMPLETED', 'conclusion': 'SUCCESS'}]"
            "}]))\n",
            encoding="utf-8",
        )
        fake_gh.chmod(0o755)
        controller_state = state(task(0, "pushed"))
        controller_state["target_sha"] = head_sha
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_GH_EXECUTABLE": str(fake_gh),
                "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
            },
            clear=False,
        ), self.assertRaisesRegex(fleet.Blocked, "target.*integration"):
            fleet.read_remote_inventory(repository, controller_state, task_number=0)

    def test_reconcile_ignores_another_tasks_historical_wrong_base_pr(self) -> None:
        """A bad historical PR cannot prevent a different task's reconciliation."""
        repository = self.root / "repository"
        head_sha = initialize_git_repository(repository)
        remote = self.root / "remote.git"
        subprocess.run(["git", "init", "--bare", "-q", str(remote)], check=True)
        git(repository, "remote", "add", "origin", str(remote))
        for branch in (
            "staging/beta1-functional-integration",
            "staging/beta1/upgrade-transaction",
            "staging/beta1/task-6",
        ):
            git(repository, "branch", branch, head_sha)
            git(repository, "push", "-q", "origin", f"{branch}:{branch}")
        pull_requests = [
            {
                "number": 864,
                "url": "https://example.invalid/pr/864",
                "state": "MERGED",
                "isDraft": False,
                "headRefName": "staging/beta1/upgrade-transaction",
                "headRefOid": head_sha,
                "baseRefName": "refactor/tag-machinery",
                "baseRefOid": head_sha,
                "mergeCommit": {"oid": head_sha},
                "statusCheckRollup": [],
            },
            {
                "number": 865,
                "url": "https://example.invalid/pr/865",
                "state": "OPEN",
                "isDraft": False,
                "headRefName": "staging/beta1/task-6",
                "headRefOid": head_sha,
                "baseRefName": "staging/beta1-functional-integration",
                "baseRefOid": head_sha,
                "mergeCommit": None,
                "statusCheckRollup": [
                    {"status": "COMPLETED", "conclusion": "SUCCESS"}
                ],
            },
        ]
        fake_gh = self.root / "fake-gh"
        fake_gh.write_text(
            "#!/usr/bin/env python3\n"
            f"payload = {pull_requests!r}\n"
            "import json\n"
            "print(json.dumps(payload))\n",
            encoding="utf-8",
        )
        fake_gh.chmod(0o755)
        store = fleet.StateStore(self.root / "controller")
        historical = task(5, "pushed")
        historical["slug"] = "upgrade-transaction"
        controller_state = state(historical, task(6, "committed"))
        controller_state["target_sha"] = head_sha
        store.write_snapshot(controller_state)
        args = fleet.build_parser().parse_args(
            [
                "reconcile",
                "--root",
                str(store.root),
                "--task",
                "6",
                "--repo",
                str(repository),
            ]
        )
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_GH_EXECUTABLE": str(fake_gh),
                "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
            },
            clear=False,
        ):
            reconciled = fleet.dispatch(args)

        self.assertEqual(reconciled["pr_ci_state"]["pr"], 865)
        self.assertEqual(reconciled["pushed_sha"], head_sha)
        self.assertIsNone(store.read_snapshot()["tasks"]["5"]["pr_ci_state"])

    def test_reconcile_known_task_pr_uses_direct_view_not_inventory_listing(self) -> None:
        """A recorded task PR is reconciled without fetching unrelated PR history."""
        repository = self.root / "repository"
        head_sha = initialize_git_repository(repository)
        remote = self.root / "remote.git"
        subprocess.run(["git", "init", "--bare", "-q", str(remote)], check=True)
        git(repository, "remote", "add", "origin", str(remote))
        for branch in (
            "staging/beta1-functional-integration",
            "staging/beta1/task-6",
        ):
            git(repository, "branch", branch, head_sha)
            git(repository, "push", "-q", "origin", f"{branch}:{branch}")
        pull_request = {
            "number": 875,
            "url": "https://example.invalid/pr/875",
            "state": "MERGED",
            "isDraft": False,
            "headRefName": "staging/beta1/task-6",
            "headRefOid": head_sha,
            "baseRefName": "staging/beta1-functional-integration",
            "baseRefOid": head_sha,
            "mergeCommit": {"oid": head_sha},
            "statusCheckRollup": [{"status": "COMPLETED", "conclusion": "SUCCESS"}],
        }
        fake_gh = self.root / "fake-gh"
        fake_gh.write_text(
            "#!/usr/bin/env python3\n"
            "import json\n"
            "import sys\n"
            f"payload = {pull_request!r}\n"
            "if sys.argv[1:3] != ['pr', 'view'] or sys.argv[3] != '875':\n"
            "    raise SystemExit('task-scoped reconcile must use gh pr view 875')\n"
            "print(json.dumps(payload))\n",
            encoding="utf-8",
        )
        fake_gh.chmod(0o755)
        store = fleet.StateStore(self.root / "controller")
        recorded = task(6, "pushed")
        recorded["pr_ci_state"] = {"pr": 875, "ci": "pending"}
        recorded["expected_merge_parent"] = head_sha
        controller_state = state(recorded)
        controller_state["target_sha"] = head_sha
        store.write_snapshot(controller_state)
        args = fleet.build_parser().parse_args(
            [
                "reconcile",
                "--root",
                str(store.root),
                "--task",
                "6",
                "--repo",
                str(repository),
            ]
        )
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_GH_EXECUTABLE": str(fake_gh),
                "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
            },
            clear=False,
        ):
            reconciled = fleet.dispatch(args)

        self.assertEqual(reconciled["state"], "merged")
        self.assertEqual(reconciled["pr_ci_state"]["pr"], 875)
        self.assertEqual(reconciled["merge_sha"], head_sha)

    def test_reconcile_boolean_recorded_pr_falls_back_to_inventory_listing(self) -> None:
        """Boolean durable PR data is not a valid direct-view identifier."""
        repository = self.root / "repository"
        head_sha = initialize_git_repository(repository)
        remote = self.root / "remote.git"
        subprocess.run(["git", "init", "--bare", "-q", str(remote)], check=True)
        git(repository, "remote", "add", "origin", str(remote))
        for branch in (
            "staging/beta1-functional-integration",
            "staging/beta1/task-6",
        ):
            git(repository, "branch", branch, head_sha)
            git(repository, "push", "-q", "origin", f"{branch}:{branch}")
        fake_gh = self.root / "fake-gh"
        fake_gh.write_text(
            "#!/usr/bin/env python3\n"
            "import json\n"
            "import sys\n"
            "if sys.argv[1:3] != ['pr', 'list']:\n"
            "    raise SystemExit('invalid recorded PR must use gh pr list')\n"
            "print(json.dumps([]))\n",
            encoding="utf-8",
        )
        fake_gh.chmod(0o755)
        recorded = task(6, "pushed")
        recorded["pr_ci_state"] = {"pr": True, "ci": "pending"}
        controller_state = state(recorded)
        controller_state["target_sha"] = head_sha
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_GH_EXECUTABLE": str(fake_gh),
                "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
            },
            clear=False,
        ):
            inventory = fleet.read_remote_inventory(
                repository, controller_state, task_number=6
            )

        self.assertEqual(inventory["tasks"]["6"]["branch"], "staging/beta1/task-6")

    def test_reconcile_non_mapping_pr_ci_state_falls_back_to_inventory_listing(self) -> None:
        """Legacy non-object durable PR state cannot crash targeted reconciliation."""
        repository = self.root / "repository"
        head_sha = initialize_git_repository(repository)
        remote = self.root / "remote.git"
        subprocess.run(["git", "init", "--bare", "-q", str(remote)], check=True)
        git(repository, "remote", "add", "origin", str(remote))
        for branch in (
            "staging/beta1-functional-integration",
            "staging/beta1/task-6",
        ):
            git(repository, "branch", branch, head_sha)
            git(repository, "push", "-q", "origin", f"{branch}:{branch}")
        fake_gh = self.root / "fake-gh"
        fake_gh.write_text(
            "#!/usr/bin/env python3\n"
            "import json\n"
            "import sys\n"
            "if sys.argv[1:3] != ['pr', 'list']:\n"
            "    raise SystemExit('invalid PR state must use gh pr list')\n"
            "print(json.dumps([]))\n",
            encoding="utf-8",
        )
        fake_gh.chmod(0o755)
        recorded = task(6, "pushed")
        recorded["pr_ci_state"] = ["legacy"]
        controller_state = state(recorded)
        controller_state["target_sha"] = head_sha
        with mock.patch.dict(
            os.environ,
            {
                "FLEET_GH_EXECUTABLE": str(fake_gh),
                "FLEET_GITHUB_REPOSITORY": "swack-tools/oxidex",
            },
            clear=False,
        ):
            inventory = fleet.read_remote_inventory(
                repository, controller_state, task_number=6
            )

        self.assertEqual(inventory["tasks"]["6"]["branch"], "staging/beta1/task-6")

    def test_identity_requires_pid_start_token_executable_task_and_exact_argv(self) -> None:
        record = {
            "pid": 7,
            "start_time": "then",
            "token": "token-a",
            "task": 3,
            "executable": "/usr/bin/codex",
            "argv": ["/usr/bin/codex", "token-a", "task 3"],
        }
        self.assertFalse(
            fleet.process_identity_matches(record, 7, "then", ["/usr/bin/codex", "token-b", "task 3"], 3)
        )
        self.assertFalse(
            fleet.process_identity_matches(record, 7, "then", record["argv"], 4)
        )
        self.assertFalse(
            fleet.process_identity_matches(record, 8, "then", record["argv"], 3)
        )
        self.assertTrue(
            fleet.process_identity_matches(record, 7, "then", record["argv"], 3)
        )

    def test_launch_is_detached_and_uses_canonical_prompt_paths(self) -> None:
        store = fleet.StateStore(self.root)
        worktree = self.root / "worktree"
        worktree.mkdir()
        prd = self.root / "prds/task.md"
        prd.parent.mkdir()
        prd.write_text("task", encoding="utf-8")
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nwhile :; do sleep 1; done\n", encoding="utf-8")
        fake.chmod(0o755)
        record = fleet.launch_worker(
            store,
            fake,
            worktree,
            "token-x",
            prd,
            task_number=5,
            model="gpt-5.6-terra",
            segment=1,
        )
        self.processes.append(record["pid"])
        self.assertTrue(record["pid"] > 0)
        self.assertTrue(fleet.worker_is_live(record))
        self.assertIn("Process token token-x", record["argv"][-1])
        self.assertIn(str(prd), record["argv"][-1])
        self.assertTrue(any(value.endswith("/final-1.md") for value in record["argv"]))
        self.assertEqual(record["launch_count"], 1)
        self.assertEqual(record["model"], "gpt-5.6-terra")
        argv = record["argv"]
        self.assertNotIn(("--enable", "fast_mode"), list(zip(argv, argv[1:])))
        self.assertEqual(
            argv,
            [
                str(fake.resolve()), "--yolo", "exec", "--disable", "fast_mode",
                "--model", "gpt-5.6-terra", "--json", "-o",
                str(self.root / "processes/05/final-1.md"),
                "-C", str(worktree),
                f"Process token token-x. Execute the canonical PRD at {prd}",
            ],
        )

    def test_launch_writes_a_durable_intent_before_spawn_and_marks_it_recorded(self) -> None:
        store = fleet.StateStore(self.root)
        worktree = self.root / "worktree"
        worktree.mkdir()
        prd = self.root / "prds/task.md"
        prd.parent.mkdir()
        prd.write_text("task", encoding="utf-8")
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nsleep 30\n", encoding="utf-8")
        fake.chmod(0o755)
        intent_path = fleet.launch_intent_path(store, 5, 1)
        observed: dict[str, object] = {}

        def after_spawn(_process: subprocess.Popen[object]) -> None:
            observed.update(json.loads(intent_path.read_text(encoding="utf-8")))
            self.assertEqual(observed["lifecycle"], "prepared")

        record = fleet.launch_worker(
            store, fake, worktree, "intent-token", prd,
            task_number=5, model="gpt-5.6-terra", segment=1,
            post_spawn_hook=after_spawn,
        )
        self.processes.append(record["pid"])
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        self.assertEqual(intent["lifecycle"], "recorded")
        self.assertEqual(intent["argv"], record["argv"])
        self.assertEqual(intent["executable"], str(fake.resolve()))
        self.assertEqual(intent["token"], "intent-token")
        self.assertEqual(intent["prd_sha256"], fleet.sha256_file(prd))
        self.assertEqual(intent["record"]["pid"], record["pid"])

    def test_post_spawn_record_failure_terminates_child_and_marks_intent_failed(self) -> None:
        store = fleet.StateStore(self.root)
        worktree = self.root / "worktree"
        worktree.mkdir()
        prd = self.root / "task.md"
        prd.write_text("task", encoding="utf-8")
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nsleep 30\n", encoding="utf-8")
        fake.chmod(0o755)
        original = fleet.atomic_json

        def fail_process_record(path: Path, value: dict) -> None:
            if path.name == "process-1.json":
                raise OSError("forced durable record failure")
            original(path, value)

        with mock.patch.object(fleet, "atomic_json", side_effect=fail_process_record), self.assertRaisesRegex(
            OSError, "forced durable record failure"
        ):
            fleet.launch_worker(
                store, fake, worktree, "failed-intent-token", prd,
                task_number=5, model="gpt-5.6-terra", segment=1,
            )
        intent = json.loads(fleet.launch_intent_path(store, 5, 1).read_text())
        self.assertEqual(intent["lifecycle"], "failed")
        self.assertFalse(fleet.worker_is_live(intent["record"]))

    def test_record_write_failure_escalates_after_leader_exits_to_empty_group(self) -> None:
        """A TERM-resistant same-group child cannot outlive a failed record write."""
        store = fleet.StateStore(self.root)
        worktree = self.root / "worktree"
        worktree.mkdir()
        prd = self.root / "task.md"
        prd.write_text("task", encoding="utf-8")
        fake = self.root / "term-resistant-group-worker"
        fake.write_text(
            "#!/bin/sh\n"
            "trap 'exit 0' TERM\n"
            "( trap '' TERM; while :; do sleep 1; done ) &\n"
            "while :; do sleep 1; done\n",
            encoding="utf-8",
        )
        fake.chmod(0o755)
        original = fleet.atomic_json

        def fail_process_record(path: Path, value: dict) -> None:
            if path.name == "process-1.json":
                raise OSError("forced durable record failure")
            original(path, value)

        with mock.patch.object(fleet, "atomic_json", side_effect=fail_process_record), self.assertRaisesRegex(
            OSError, "forced durable record failure"
        ):
            fleet.launch_worker(
                store, fake, worktree, "resistant-group-token", prd,
                task_number=5, model="gpt-5.6-terra", segment=1,
            )
        intent = json.loads(fleet.launch_intent_path(store, 5, 1).read_text())
        self.assertEqual(intent["lifecycle"], "failed")
        self.assertFalse(fleet._process_group_exists(intent["record"]["process_group"]))

    def test_authenticated_group_cleanup_survives_leader_exit_and_proves_esrch(self) -> None:
        """A prior authenticated record retains authority after its leader exits."""
        token = "leader-exited-group-token"
        script = self.root / "leader-exits-with-resistant-children"
        children = self.root / "children"
        script.write_text(
            "#!/bin/sh\n"
            "( trap '' TERM; while :; do sleep 1; done ) & echo $! >> \"$1\"\n"
            "( trap '' TERM; while :; do sleep 1; done ) & echo $! >> \"$1\"\n"
            "sleep 1\n",
            encoding="utf-8",
        )
        script.chmod(0o755)
        process = subprocess.Popen([str(script), str(children), token], start_new_session=True)
        deadline = time.monotonic() + 5
        while (not children.exists() or len(children.read_text().splitlines()) < 2) and time.monotonic() < deadline:
            time.sleep(0.01)
        record = {
            "pid": process.pid, "start_time": fleet.process_start_time(process.pid),
            "token": token, "argv": [str(script), str(children), token],
            "kernel_argv": fleet.process_kernel_argv(process.pid),
            "kernel_executable": fleet.process_kernel_executable(process.pid),
            "process_group": os.getpgid(process.pid),
        }
        process.wait(timeout=5)
        self.assertFalse(fleet.worker_is_live(record))
        self.assertTrue(fleet._process_group_exists(record["process_group"]))
        fleet._terminate_authenticated_process(record)
        self.assertFalse(fleet._process_group_exists(record["process_group"]))

    def test_group_exit_wait_retries_a_transient_kernel_permission_race(self) -> None:
        """macOS may briefly report EPERM while a just-signalled group exits."""
        with mock.patch.object(
            fleet,
            "_process_group_exists",
            side_effect=[fleet.Refused("transient kernel state"), False],
        ):
            self.assertTrue(fleet._wait_for_group_exit(12345, time.monotonic() + 1))

    def test_expected_kernel_argv_expands_linux_shebang_and_env_split_string(self) -> None:
        script = self.root / "env split script"
        script.write_text("#!/usr/bin/env -S python3 -u\n", encoding="utf-8")
        argv = [str(script), "", "argument with spaces"]
        self.assertEqual(
            fleet.expected_kernel_argv(script, argv, platform="linux"),
            ["python3", "-u", str(script), "", "argument with spaces"],
        )

    def test_expected_kernel_argv_preserves_quoted_env_split_argument(self) -> None:
        """``env -S`` must split its payload once, not discard quote boundaries."""
        script = self.root / "quoted env split script"
        script.write_text(
            '#!/usr/bin/env -S /bin/sh -c "sleep 2" ""\n', encoding="utf-8"
        )
        argv = [str(script), "worker-token"]
        self.assertEqual(
            fleet.expected_kernel_argv(script, argv, platform="linux"),
            ["/bin/sh", "-c", "sleep 2", "", str(script), "worker-token"],
        )

    def test_expected_kernel_argv_accepts_env_options_before_split_string(self) -> None:
        script = self.root / "env options split script"
        script.write_text("#!/usr/bin/env -i -S python3 -u\n", encoding="utf-8")
        self.assertEqual(
            fleet.expected_kernel_argv(script, [str(script), "token"], platform="darwin"),
            ["python3", "-u", str(script), "token"],
        )

    def test_darwin_shebang_options_are_separate_kernel_arguments(self) -> None:
        """Darwin tokenizes optional interpreter arguments before ``env`` runs."""
        script = self.root / "darwin multi option script"
        script.write_text("#!/bin/sh -e -u\n", encoding="utf-8")
        self.assertEqual(
            fleet.expected_kernel_argv(script, [str(script), "token"], platform="darwin"),
            ["/bin/sh", "-e", "-u", str(script), "token"],
        )

    def test_darwin_env_split_uses_kernel_tokenized_options(self) -> None:
        script = self.root / "darwin env tokenized options script"
        script.write_text(
            "#!/usr/bin/env -i -S /bin/sh -e -u\n", encoding="utf-8"
        )
        self.assertEqual(
            fleet.expected_kernel_argv(script, [str(script), "token"], platform="darwin"),
            ["/bin/sh", "-e", "-u", str(script), "token"],
        )

    def test_darwin_refuses_quoted_env_split_string(self) -> None:
        """Darwin does not provide Linux's one-tail quoted split-string input."""
        script = self.root / "darwin quoted env split script"
        script.write_text(
            '#!/usr/bin/env -S /bin/sh -c "sleep 2"\n', encoding="utf-8"
        )
        self.assertEqual(
            fleet.expected_kernel_argv(script, [str(script), "token"], platform="darwin"),
            [],
        )

    def test_linux_shebang_tail_is_one_kernel_argument(self) -> None:
        script = self.root / "linux one tail script"
        script.write_text("#!/bin/sh -e -u\n", encoding="utf-8")
        self.assertEqual(
            fleet.expected_kernel_argv(script, [str(script), "token"], platform="linux"),
            ["/bin/sh", "-e -u", str(script), "token"],
        )

    @unittest.skipUnless(sys.platform == "darwin", "exercises Darwin multi-option shebang argv")
    def test_current_darwin_kernel_observes_multi_option_shebang_exec(self) -> None:
        """A live Darwin process exposes separate optional interpreter arguments."""
        script = self.root / "darwin multi option kernel worker"
        script.write_text(
            "#!/bin/sh -e -u\nwhile :; do sleep 1; done\n", encoding="utf-8"
        )
        script.chmod(0o755)
        token = "darwin-multi-option-kernel-token"
        process = subprocess.Popen([str(script), token])
        self.processes.append(process.pid)
        deadline = time.monotonic() + 1
        observed: list[str] = []
        expected = fleet.expected_kernel_argv(script, [str(script), token])
        while time.monotonic() < deadline:
            observed = fleet.process_kernel_argv(process.pid)
            if observed == expected:
                break
            time.sleep(0.01)
        self.assertEqual(observed, expected)
        self.assertEqual(observed, ["/bin/sh", "-e", "-u", str(script), token])
        process.terminate()
        process.wait(timeout=5)

    def test_shebang_env_refuses_malformed_and_unsupported_forms(self) -> None:
        malformed = self.root / "bad env"
        malformed.write_text("#!/usr/bin/env -S 'unterminated\n", encoding="utf-8")
        unsupported = self.root / "unsupported env"
        unsupported.write_text("#!/usr/bin/env --argv0 worker python3\n", encoding="utf-8")
        self.assertEqual(
            fleet.expected_kernel_argv(malformed, [str(malformed)], platform="darwin"),
            [],
        )
        self.assertEqual(
            fleet.expected_kernel_argv(unsupported, [str(unsupported)], platform="darwin"),
            [],
        )

    def test_macos_procargs_preserves_empty_argv_element(self) -> None:
        raw = (3).to_bytes(4, sys.byteorder) + b"/usr/bin/python3\0\0script\0\0value\0"
        self.assertEqual(fleet.parse_macos_procargs(raw), ["script", "", "value"])

    @unittest.skipUnless(sys.platform.startswith("linux"), "exercises Linux /proc argv")
    def test_linux_proc_argv_observes_real_shebang_expansion_with_empty_argument(self) -> None:
        script = self.root / "linux shebang worker"
        script.write_text("#!/bin/sh\nwhile :; do sleep 1; done\n", encoding="utf-8")
        script.chmod(0o755)
        token = "linux-shebang-token"
        process = subprocess.Popen([str(script), "", token])
        self.processes.append(process.pid)
        deadline = time.monotonic() + 2
        while not fleet.process_kernel_argv(process.pid) and time.monotonic() < deadline:
            time.sleep(0.01)
        argv = fleet.process_kernel_argv(process.pid)
        self.assertEqual(argv, ["/bin/sh", str(script), "", token])
        self.assertEqual(argv, fleet.expected_kernel_argv(script, [str(script), "", token]))

    def test_kernel_observation_read_failure_and_blank_start_refuse_liveness(self) -> None:
        with mock.patch("pathlib.Path.read_bytes", side_effect=OSError("unreadable")), mock.patch.object(
            fleet.ctypes, "CDLL", side_effect=OSError("native query unavailable")
        ):
            self.assertEqual(fleet.process_kernel_argv(12345), [])
        record = {
            "pid": 12345, "start_time": "expected", "kernel_argv": ["worker", "token"],
            "kernel_executable": "/worker", "token": "token", "process_group": 12345,
        }
        with mock.patch.object(fleet.os, "kill"), mock.patch.object(fleet, "process_start_time", return_value=""), mock.patch.object(
            fleet, "process_kernel_argv", return_value=["worker", "token"]
        ):
            self.assertFalse(fleet.worker_is_live(record))

    def test_wrong_kernel_executable_refuses_authenticated_worker(self) -> None:
        record = {
            "pid": 12345, "start_time": "start", "kernel_argv": ["worker", "token"],
            "kernel_executable": "/expected-worker", "token": "token", "process_group": 12345,
        }
        with mock.patch.object(fleet.os, "kill"), mock.patch.object(fleet.os, "getpgid", return_value=12345), mock.patch.object(
            fleet, "process_start_time", return_value="start"
        ), mock.patch.object(fleet, "process_kernel_argv", return_value=["worker", "token"]), mock.patch.object(
            fleet, "process_kernel_executable", return_value="/different-worker"
        ):
            self.assertFalse(fleet.worker_is_live(record))

    def test_fresh_controller_crash_before_spawn_makes_only_fence_authorized_retry(self) -> None:
        store = fleet.StateStore(self.root)
        worktree = self.root / "worktree"
        worktree.mkdir()
        prd = self.root / "task.md"
        prd.write_text("task", encoding="utf-8")
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nwhile :; do sleep 1; done\n", encoding="utf-8")
        fake.chmod(0o755)
        script = (
            "import importlib.util,os,signal,sys; from pathlib import Path;"
            "s=importlib.util.spec_from_file_location('fresh',Path(sys.argv[1]));m=importlib.util.module_from_spec(s);s.loader.exec_module(m);"
            "m.launch_worker(m.StateStore(Path(sys.argv[2])),Path(sys.argv[3]),Path(sys.argv[4]),'before-spawn-token',Path(sys.argv[5]),"
            "task_number=5,model='gpt-5.6-terra',segment=1,pre_spawn_hook=lambda:os.kill(os.getpid(),signal.SIGKILL))"
        )
        child = subprocess.run([sys.executable, "-c", script, str(MODULE), str(self.root), str(fake), str(worktree), str(prd)], capture_output=True, text=True)
        self.assertEqual(child.returncode, -9, child.stderr)
        intent = fleet.launch_intent_path(store, 5, 1)
        self.assertTrue(intent.is_file())
        self.assertFalse(fleet.launch_handshake_path(intent).exists())
        fleet.reconcile_pending_launch_intents(store)
        self.assertEqual(json.loads(intent.read_text())["lifecycle"], "retryable")

    def test_pending_intent_no_child_and_ambiguity_are_fenced_before_retry(self) -> None:
        store = fleet.StateStore(self.root)
        pending = task(0, "launchable")
        store.write_snapshot(state(pending))
        path = fleet.launch_intent_path(store, 0, 1)
        intent = {
            "schema_version": 1, "lifecycle": "prepared", "task": 0, "segment": 1,
            "token": "intent-recovery-token", "executable": sys.executable,
            "argv": [sys.executable, "intent-recovery-token"], "worktree": str(self.root),
            "prd": str(self.root / "prd"), "prd_sha256": "a" * 64,
            "prompt": str(self.root / "prd"), "prompt_sha256": "a" * 64,
            "model": "gpt-5.6-terra", "events": str(self.root / "events"),
            "final": str(self.root / "final"), "session_id": None, "mode": "initial",
            "fence": str(path.with_suffix(".fence")),
        }
        fleet.atomic_json(path, intent)
        with mock.patch.object(fleet, "_find_intent_children", return_value=[]):
            fleet.reconcile_pending_launch_intents(store)
        self.assertEqual(json.loads(path.read_text())["lifecycle"], "retryable")
        intent["lifecycle"] = "prepared"
        fleet.atomic_json(path, intent)
        duplicate = {"pid": 1}
        with mock.patch.object(fleet, "_find_intent_children", return_value=[duplicate, duplicate]), self.assertRaisesRegex(
            fleet.Blocked, "ambiguous live children"
        ):
            fleet.reconcile_pending_launch_intents(store)

    def test_kernel_argv_rejects_a_forged_ready_file_with_spaced_arguments(self) -> None:
        """A ready file is a notification, never evidence of an argv boundary."""
        fake = self.root / "fake executable"
        fake.write_text("#!/bin/sh\nwhile :; do sleep 1; done\n", encoding="utf-8")
        fake.chmod(0o755)
        token = "kernel-argv-token"
        intent = {
            "task": 0, "segment": 1, "token": token, "executable": str(fake),
            "argv": [str(fake), "arg with spaces", token], "model": "test",
            "events": str(self.root / "events"), "final": str(self.root / "final"),
        }
        ready = self.root / "forged.ready.json"
        intent["handshake"] = str(ready)
        process = subprocess.Popen(["/bin/sh", "-c", "while :; do sleep 1; done"])
        self.processes.append(process.pid)
        fleet.atomic_json(ready, {"pid": process.pid, "token": token, "argv": intent["argv"]})
        self.assertIsNone(fleet._intent_matches_process(intent, process.pid))

    def test_binary_executable_expected_kernel_identity_never_decodes_payload(self) -> None:
        binary = self.root / "ordinary-binary"
        binary.write_bytes(b"\x7fELF\xff\x00not-text")
        binary.chmod(0o755)
        self.assertEqual(fleet.expected_kernel_executable(binary), str(binary.resolve()))

    def test_unlocked_pre_spawn_intent_becomes_retryable_not_permanently_blocked(self) -> None:
        store = fleet.StateStore(self.root)
        pending = task(0, "launchable")
        store.write_snapshot(state(pending))
        path = fleet.launch_intent_path(store, 0, 1)
        fence = path.with_suffix(".fence")
        intent = {
            "schema_version": 1, "lifecycle": "prepared", "task": 0, "segment": 1,
            "token": "pre-spawn-token", "executable": sys.executable,
            "argv": [sys.executable, "pre-spawn-token"], "worktree": str(self.root),
            "prd": str(self.root / "prd"), "prd_sha256": "a" * 64,
            "prompt": str(self.root / "prd"), "prompt_sha256": "a" * 64,
            "model": "gpt-5.6-terra", "events": str(self.root / "events"),
            "final": str(self.root / "final"), "session_id": None, "mode": "initial",
            "handshake": str(path.with_suffix(".ready.json")), "fence": str(fence),
        }
        fleet.atomic_json(path, intent)
        fence.touch()
        fleet.reconcile_pending_launch_intents(store)
        self.assertEqual(json.loads(path.read_text())["lifecycle"], "retryable")

    def test_resume_writes_and_records_the_same_pre_spawn_intent_protocol(self) -> None:
        worktree = self.root / "worktree"
        head_sha = initialize_git_repository(worktree)
        current = task(0, "running")
        current["paths"]["worktree"] = str(worktree)
        current["head_sha"] = head_sha
        prd = Path(current["paths"]["prd"])
        prd.parent.mkdir(parents=True, exist_ok=True)
        prd.write_text("task\n", encoding="utf-8")
        current["prd_sha256"] = fleet.sha256_file(prd)
        current["process"] = process_record(0, self.root / "events-1.jsonl", "recorded-session")
        store = fleet.StateStore(self.root)
        store.write_snapshot(state(current))
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nwhile :; do sleep 1; done\n", encoding="utf-8")
        fake.chmod(0o755)
        resumed = fleet.resume_task(store, 0, fake)
        record = resumed["process"]
        self.processes.append(record["pid"])
        intent = json.loads(fleet.launch_intent_path(store, 0, 2).read_text())
        self.assertEqual(intent["lifecycle"], "recorded")
        self.assertEqual(intent["mode"], "resume")
        self.assertEqual(intent["record"]["pid"], record["pid"])
        argv = record["argv"]
        self.assertNotIn(("--enable", "fast_mode"), list(zip(argv, argv[1:])))
        self.assertEqual(
            argv,
            [
                str(fake.resolve()), "--yolo", "exec", "resume", "--disable",
                "fast_mode", "--model", "gpt-5.6-terra", "--json", "-o",
                str(self.root / "final-2.md"), "recorded-session",
                f"Process token {record['token']}. Continue from "
                f"{self.root / 'processes/00/recovery-prompt.md'}",
            ],
        )
        self.assertEqual(intent["argv"], argv)

    def test_fresh_controller_adopts_post_spawn_pre_record_worker_without_duplicate(self) -> None:
        """Real SIGKILL leaves only an intent; a fresh controller adopts its exact child."""
        store = fleet.StateStore(self.root)
        worktree = self.root / "worktree"
        head = initialize_git_repository(worktree)
        pending = task(0, "launchable")
        pending["paths"]["worktree"] = str(worktree)
        pending["head_sha"] = head
        prd = Path(pending["paths"]["prd"])
        prd.parent.mkdir(parents=True, exist_ok=True)
        prd.write_text("task\n", encoding="utf-8")
        pending["prd_sha256"] = fleet.sha256_file(prd)
        store.write_snapshot(state(pending))
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nwhile :; do sleep 1; done\n", encoding="utf-8")
        fake.chmod(0o755)
        script = (
            "import importlib.util, os, signal, sys, time; from pathlib import Path;"
            "spec=importlib.util.spec_from_file_location('fresh_controller',Path(sys.argv[1]));"
            "m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m);"
            "store=m.StateStore(Path(sys.argv[2]));"
            "m.launch_worker(store,Path(sys.argv[3]),Path(sys.argv[4]),'post-spawn-token',Path(sys.argv[5]),"
            "task_number=0,model='gpt-5.6-terra',segment=1,"
            "post_handshake_hook=lambda _p: os.kill(os.getpid(), signal.SIGKILL))"
        )
        child = subprocess.run(
            [sys.executable, "-c", script, str(MODULE), str(self.root), str(fake), str(worktree), str(prd)],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(child.returncode, -9, child.stderr)
        self.assertFalse((self.root / "processes/00/process-1.json").exists())
        recovered = fleet.recover_controller(store, self.root)
        adopted = store.read_snapshot()["tasks"]["0"]["process"]
        self.assertIsNotNone(
            adopted,
            (self.root / "processes/00/launch-intent-1.json").read_text(encoding="utf-8"),
        )
        self.processes.append(adopted["pid"])
        self.assertEqual(adopted["token"], "post-spawn-token")
        self.assertIn(recovered["actions"]["0"], {"monitor --task 0 --follow", "reconcile --task 0"})
        self.assertEqual(len(list((self.root / "processes/00").glob("process-*.json"))), 1)

    def test_fresh_controller_fences_earliest_post_spawn_pre_handshake_then_adopts(self) -> None:
        """SIGSTOP at the Popen boundary leaves a held OS fence, never a retry race."""
        store = fleet.StateStore(self.root)
        worktree = self.root / "worktree"
        head = initialize_git_repository(worktree)
        pending = task(0, "launchable")
        pending["paths"]["worktree"] = str(worktree)
        pending["head_sha"] = head
        prd = Path(pending["paths"]["prd"])
        prd.parent.mkdir(parents=True, exist_ok=True)
        prd.write_text("task\n", encoding="utf-8")
        pending["prd_sha256"] = fleet.sha256_file(prd)
        store.write_snapshot(state(pending))
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nwhile :; do sleep 1; done\n", encoding="utf-8")
        fake.chmod(0o755)
        stopped_pid = self.root / "stopped-pid"
        script = (
            "import importlib.util, os, signal, sys; from pathlib import Path;"
            "spec=importlib.util.spec_from_file_location('fresh_controller',Path(sys.argv[1]));"
            "m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m);"
            "store=m.StateStore(Path(sys.argv[2])); marker=Path(sys.argv[6]);"
            "stop=lambda p:(marker.write_text(str(p.pid)),os.kill(p.pid,signal.SIGSTOP),os.kill(os.getpid(),signal.SIGKILL));"
            "m.launch_worker(store,Path(sys.argv[3]),Path(sys.argv[4]),'pre-handshake-token',Path(sys.argv[5]),"
            "task_number=0,model='gpt-5.6-terra',segment=1,post_spawn_hook=stop)"
        )
        child = subprocess.run(
            [sys.executable, "-c", script, str(MODULE), str(self.root), str(fake), str(worktree), str(prd), str(stopped_pid)],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(child.returncode, -9, child.stderr)
        pid = int(stopped_pid.read_text())
        intent_path = fleet.launch_intent_path(store, 0, 1)
        self.assertFalse(fleet.launch_handshake_path(intent_path).exists())
        with self.assertRaisesRegex(fleet.Blocked, "fence remains held"):
            fleet.recover_controller(store, self.root)
        self.assertFalse((self.root / "processes/00/process-1.json").exists())
        os.kill(pid, signal.SIGCONT)
        deadline = time.monotonic() + 5
        children: list[dict] = []
        while time.monotonic() < deadline:
            if fleet.launch_handshake_path(intent_path).exists():
                intent = json.loads(intent_path.read_text(encoding="utf-8"))
                children = fleet._find_intent_children(intent)
                if len(children) == 1:
                    break
            time.sleep(0.01)
        self.assertEqual(
            len(children),
            1,
            intent_path.read_text(encoding="utf-8"),
        )
        recovered = fleet.recover_controller(store, self.root)
        adopted = store.read_snapshot()["tasks"]["0"]["process"]
        self.processes.append(adopted["pid"])
        self.assertEqual(adopted["pid"], pid)
        self.assertEqual(adopted["token"], "pre-handshake-token")
        self.assertEqual(recovered["actions"]["0"], "monitor --task 0 --follow")
        self.assertEqual(len(list((self.root / "processes/00").glob("process-*.json"))), 1)

    def test_monitor_is_incremental_extracts_thread_and_updates_task_heartbeat(self) -> None:
        store = fleet.StateStore(self.root)
        value = state(task(0, "running"))
        events = self.root / "processes/00/events-1.jsonl"
        events.parent.mkdir(parents=True)
        events.write_text(
            '{"type":"thread.started","thread_id":"session-42"}\n'
            '{"type":"item.completed","item":"first"}\n',
            encoding="utf-8",
        )
        value["tasks"]["0"]["process"] = process_record(0, events)
        store.write_snapshot(value)
        first = fleet.monitor_task(store, 0)
        second = fleet.monitor_task(store, 0)
        self.assertEqual(first["session_id"], "session-42")
        self.assertEqual(len(first["events"]), 2)
        self.assertEqual(second["events"], [])
        self.assertEqual(store.read_snapshot()["tasks"]["0"]["heartbeat"]["session_id"], "session-42")

    def test_monitor_does_not_consume_partial_jsonl_record(self) -> None:
        store = fleet.StateStore(self.root)
        value = state(task(0, "running"))
        events = self.root / "processes/00/events-1.jsonl"
        events.parent.mkdir(parents=True)
        events.write_text('{"type":"thread.started","thread_id":"session', encoding="utf-8")
        value["tasks"]["0"]["process"] = process_record(0, events)
        store.write_snapshot(value)
        first = fleet.monitor_task(store, 0)
        self.assertEqual(first["events"], [])
        with events.open("a", encoding="utf-8") as output:
            output.write('-partial"}\n')
        second = fleet.monitor_task(store, 0)
        self.assertEqual(second["session_id"], "session-partial")

    def test_status_heartbeat_resume_and_stop_enforce_process_rules(self) -> None:
        store = fleet.StateStore(self.root)
        value = state(task(0, "running"))
        value["tasks"]["0"]["process"] = process_record(
            0, self.root / "events.jsonl", "recorded-session"
        )
        store.write_snapshot(value)
        self.assertEqual(fleet.task_status(store, 0)["state"], "running")
        heartbeat = fleet.heartbeat_task(store, 0)
        self.assertEqual(heartbeat["task"], 0)
        argv = fleet.resume_argv(value["tasks"]["0"]["process"], Path("/usr/bin/codex"), "new-token", self.root / "recovery.md")
        self.assertIn("recorded-session", argv)
        self.assertNotIn("--last", argv)
        self.assertIn(str(self.root / "recovery.md"), argv[-1])
        stopped = fleet.stop_task(store, 0, "TERM")
        self.assertEqual(stopped, {"stopped": False, "reason": "not-live"})

    def test_resume_authenticates_replacement_before_durable_state_changes(self) -> None:
        worktree = self.root / "worktree"
        head_sha = initialize_git_repository(worktree)
        current = task(0, "running")
        current["paths"]["worktree"] = str(worktree)
        current["head_sha"] = head_sha
        prd = Path(current["paths"]["prd"])
        prd.parent.mkdir(parents=True, exist_ok=True)
        prd.write_text("task\n", encoding="utf-8")
        current["prd_sha256"] = fleet.sha256_file(prd)
        current["process"] = process_record(
            0, self.root / "processes/00/events-1.jsonl", "recorded-session"
        )
        store = fleet.StateStore(self.root)
        store.write_snapshot(state(current))
        before = store.read_snapshot()
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        fake.chmod(0o755)

        with self.assertRaisesRegex(fleet.Refused, "identity"):
            fleet.resume_task(store, 0, fake)

        self.assertEqual(store.read_snapshot(), before)
        self.assertFalse((self.root / "processes/00/process-2.json").exists())
        if store.events_path.exists():
            events = [
                json.loads(line)["event"]
                for line in store.events_path.read_text(encoding="utf-8").splitlines()
            ]
            self.assertNotIn("resume", events)

    def test_merge_lease_is_exclusive_and_unexpected_squash_parent_blocks_release(self) -> None:
        store = fleet.StateStore(self.root)
        store.write_snapshot(state(task(0)))
        self.assertTrue(fleet.acquire_merge_lease(store, "first"))
        self.assertFalse(fleet.acquire_merge_lease(store, "second"))
        self.assertTrue(fleet.release_merge_lease(store, "first"))
        with self.assertRaisesRegex(fleet.Blocked, "squash parent"):
            fleet.validate_merge_parent(SHA_A, SHA_B)

    def test_task20_round_receipts_never_collide_or_overwrite(self) -> None:
        first = fleet.write_round_receipt(self.root, 20, 1, {"result": "first"})
        second = fleet.write_round_receipt(self.root, 20, 2, {"result": "second"})
        self.assertNotEqual(first, second)
        with self.assertRaisesRegex(fleet.Refused, "exists"):
            fleet.write_round_receipt(self.root, 20, 1, {"result": "overwrite"})
        self.assertEqual(json.loads(first.read_text())["result"], "first")

    def test_recovery_has_one_action_per_nonterminal_task_without_duplicate_dispatch(self) -> None:
        completed = task(0, "completed")
        committed = task(1, "committed")
        pushed = task(2, "pushed")
        dead = task(3, "running")
        dead["process"] = {"pid": 99999999, "session_id": "session-3"}
        desktop = task(4, "running")
        desktop["worker"]["kind"] = "Desktop"
        desktop["worker"]["identity"] = None
        blocked = task(5, "blocked", [4])
        actions = fleet.recovery_actions(
            state(completed, committed, pushed, dead, desktop, blocked)
        )
        self.assertEqual(set(actions), {"1", "2", "3", "4", "5"})
        self.assertEqual(actions["3"], "resume --task 3")
        self.assertEqual(actions["4"], "reconcile --task 4")
        self.assertEqual(len(actions.values()), len(set(actions.values())))

    def test_recovery_monitors_matching_live_worker_instead_of_resuming(self) -> None:
        store = fleet.StateStore(self.root)
        worktree = self.root / "worktree"
        worktree.mkdir()
        prd = self.root / "task.md"
        prd.write_text("task", encoding="utf-8")
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nsleep 30\n", encoding="utf-8")
        fake.chmod(0o755)
        process = fleet.launch_worker(
            store,
            fake,
            worktree,
            "live-token",
            prd,
            task_number=3,
            model="gpt-5.6-sol",
            segment=2,
        )
        self.processes.append(process["pid"])
        process["session_id"] = "live-session"
        running = task(3, "running")
        running["process"] = process
        actions = fleet.recovery_actions(state(running))
        self.assertEqual(actions["3"], "monitor --task 3 --follow")

    def test_recovery_adopts_durable_launch_record_after_controller_death(self) -> None:
        store = fleet.StateStore(self.root)
        pending = task(0, "launchable")
        store.write_snapshot(state(pending))
        worktree = self.root / "worktree"
        worktree.mkdir()
        prd = self.root / "task.md"
        prd.write_text("task", encoding="utf-8")
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nsleep 30\n", encoding="utf-8")
        fake.chmod(0o755)
        launched = fleet.launch_worker(
            store,
            fake,
            worktree,
            "orphan-safe-token",
            prd,
            task_number=0,
            model="gpt-5.6-terra",
            segment=1,
        )
        self.processes.append(launched["pid"])
        recovered = fleet.recover_controller(store, self.root)
        adopted = store.read_snapshot()["tasks"]["0"]
        self.assertEqual(adopted["process"]["token"], "orphan-safe-token")
        self.assertEqual(adopted["launch_count"], 1)
        self.assertEqual(recovered["actions"]["0"], "monitor --task 0 --follow")

    def test_recovery_adopts_newer_live_record_when_snapshot_has_dead_worker(self) -> None:
        """The resume crash window must watch the newer durable worker, never relaunch."""
        store = fleet.StateStore(self.root)
        current = task(0, "running")
        current["process"] = process_record(0, self.root / "events-1.jsonl", "old-session")
        current["launch_count"] = 1
        store.write_snapshot(state(current))
        worktree = self.root / "worktree"
        worktree.mkdir()
        prd = self.root / "task.md"
        prd.write_text("task", encoding="utf-8")
        fake = self.root / "fake-codex"
        fake.write_text("#!/bin/sh\nsleep 30\n", encoding="utf-8")
        fake.chmod(0o755)
        resumed = fleet.launch_worker(
            store,
            fake,
            worktree,
            "crash-window-token",
            prd,
            task_number=0,
            model="gpt-5.6-terra",
            segment=2,
        )
        self.processes.append(resumed["pid"])
        recovered = fleet.recover_controller(store, self.root)
        adopted = store.read_snapshot()["tasks"]["0"]
        self.assertEqual(adopted["process"]["token"], "crash-window-token")
        self.assertEqual(adopted["launch_count"], 2)
        self.assertEqual(recovered["actions"]["0"], "monitor --task 0 --follow")

    def test_rehearsal_persists_jsonl_and_fresh_process_recovers_same_state(self) -> None:
        receipt = fleet.run_fixture_rehearsal(self.root)
        self.assertTrue(Path(receipt["events"]).is_file())
        self.assertTrue(Path(receipt["receipt"]).is_file())
        persisted = json.loads(Path(receipt["receipt"]).read_text())
        self.assertEqual(persisted["before_states"], persisted["after_states"])
        self.assertTrue(persisted["no_duplicate_launch"])
        self.assertTrue(persisted["newest_supervisor_selected"])
        self.assertTrue(persisted["newest_worker_live_after_supervisor_exit"])
        self.assertTrue(persisted["fresh_process_crash_window_adoption"])
        self.assertTrue(persisted["no_duplicate_launch_after_adoption"])
        self.assertTrue(all(persisted["actions"].values()))

    def test_recovery_selects_newest_supervisor_and_never_duplicates_live_worker(self) -> None:
        process_root = self.root / "processes/00"
        process_root.mkdir(parents=True)
        for segment, token in ((1, "old-token"), (2, "new-token")):
            (process_root / f"bootstrap-process-{segment}.json").write_text(
                json.dumps(
                    {
                        "segment": segment,
                        "pid": os.getpid(),
                        "start_time": fleet.process_start_time(os.getpid()),
                        "token": token,
                        "task": 0,
                        "executable": sys.executable,
                        "argv": [sys.executable, token],
                    }
                ),
                encoding="utf-8",
            )
        selected = fleet.select_latest_process_record(process_root)
        self.assertEqual(selected["segment"], 2)
        self.assertEqual(
            fleet.recovery_decision(selected, selected["pid"], selected["start_time"], selected["argv"]),
            "watch",
        )
        changed = dict(selected, argv=[sys.executable, "different-token"])
        with self.assertRaisesRegex(fleet.Blocked, "identity"):
            fleet.recovery_decision(selected, selected["pid"], selected["start_time"], changed["argv"])


if __name__ == "__main__":
    unittest.main()
