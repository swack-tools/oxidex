"""Durability contracts for every release fleet-controller command."""

from __future__ import annotations

import importlib.util
import json
import os
import signal
import subprocess
import sys
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
            "- Add: tools/ambiguous.rs\n",
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
        self.assertNotIn("ambiguous.rs", " ".join(task_value["file_lease"]))

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
            "### Task 0: CLI task\n\ncli body\n\n"
            "### Task 1: Desktop task\n\ndesktop body\n\n"
            "### Task 2: Controller task\n\ncontroller body\n\n"
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
            fleet.read_remote_inventory(repository, controller_state)

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
        while not fleet.launch_handshake_path(intent_path).exists() and time.monotonic() < deadline:
            time.sleep(0.01)
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
