#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Unit tests for scripts/fleet_up.sh.

WHY THESE TESTS LOOK LIKE THIS
------------------------------
fleet_up.sh is a shell script, but the three things that have actually gone
wrong in production are pure decisions with exact right answers, so they are
tested as pure functions rather than through a live fleet:

  1. "Does this worktree carry unpublished work?" -- answered with
     `rev-list origin/main..HEAD`, NOT a diff. A diff measures total
     divergence and reports a branch that is merely BEHIND, which is how a
     salvage list fills with noise until the operator stops reading it.
     test_unique_commits_is_zero_when_merely_behind pins that distinction.

  2. "Is this tier alive?" -- answered by verifying a pid WE forked still
     carries the expected argv. Reproduced in
     test_scan_excludes_self_where_naive_scan_self_matches: a naive
     `ps | grep` for a marker returns 3 hits when the honest answer is 0,
     because the searching process's own command line contains the marker.
     That is the 2026-07-26 false positive that made a dead merger tier look
     healthy while nothing published for hours.

  3. "Which checkout runs?" -- pinned by BOTH sha and branch name, because
     parallel_model_fix_loop.ensure_integration_branch only retargets
     round-end merges to model-fix-sweep-local when HEAD is literally "main".
     test_main_tracking_requires_branch_name_main pins that a sha-only match
     is rejected.

The script is sourced with FLEET_UP_SOURCE_ONLY=1, which defines every
function and runs nothing. Git-touching tests build real repositories in a
tempdir -- an `origin/main` ref is created with `update-ref`, so nothing here
needs a network or the live fleet.

Run:
    cd scripts && python3 -m unittest test_fleet_up -q
"""
import json
import os
import shutil
import signal
import subprocess  # nosec B404 -- list-argv only, no shell=True except the
# deliberate `bash -c` harness below, whose input is test-authored.
import tempfile
import textwrap
import time
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "fleet_up.sh"


def sh(snippet, env=None, cwd=None, check=False):
    """Source fleet_up.sh and run `snippet` against its functions.

    Returns (rc, stdout, stderr). `set +e` is applied after sourcing: the
    script sets `-euo pipefail` for its own top-level flow, but a test that
    deliberately calls a function expected to return non-zero must not abort
    the harness shell before it can assert.
    """
    full = f"export FLEET_UP_SOURCE_ONLY=1\nsource {SCRIPT}\nset +e\n{snippet}\n"
    e = dict(os.environ)
    # Never let a test inherit the operator's real fleet paths.
    e.pop("OXIDEX_HOME", None)
    if env:
        e.update({k: str(v) for k, v in env.items()})
    p = subprocess.run(  # nosec B603 B607
        ["bash", "-c", full], capture_output=True, text=True, env=e, cwd=cwd, check=check,
    )
    return p.returncode, p.stdout, p.stderr


def git(repo, *args):
    return subprocess.run(  # nosec B603 B607
        ["git", "-C", str(repo), *args], capture_output=True, text=True, check=True,
    ).stdout.strip()


class ShellHarnessMixin:
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="fleet-up-test-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.logdir = self.tmp / "logs"
        self.logdir.mkdir()
        self.env = {
            "FLEET_LOG_DIR": str(self.logdir),
            "FLEET_LOG": str(self.logdir / "fleet-up.log"),
            "FLEET_PIDFILE": str(self.logdir / "fleet-up.pid"),
            "FLEET_STATEFILE": str(self.logdir / "fleet-up.state"),
        }


class TestParseSquads(ShellHarnessMixin, unittest.TestCase):
    """squads.toml is the source of truth for how many mergers to run.

    Hardcoding the list is how half the tier stayed dead: the operator brief
    for this launcher said "7 squads", squads.toml defines 14, and on
    2026-07-26 exactly 7 mergers were alive -- exif-core, one of the 7 dead
    ones, was the 2nd-largest quarantine producer (14 of 80 ledger entries).
    """

    def test_returns_every_squad_and_strips_toml_quotes(self):
        toml = self.tmp / "squads.toml"
        toml.write_text(textwrap.dedent("""\
            [meta]
            snapshot_date = "2026-07-24"

            [squads.canon]
            modules = ["Canon"]

            [squads."sony-minolta"]
            modules = ["Sony"]

            [squads.tail]
            modules = []
            """))
        rc, out, _ = sh(f'parse_squads "{toml}"', env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(out.split(), ["canon", "sony-minolta", "tail"])

    def test_real_manifest_yields_fourteen_squads(self):
        real = SCRIPT.parent / "squads.toml"
        if not real.is_file():
            self.skipTest("squads.toml not present in this checkout")
        rc, out, _ = sh(f'parse_squads "{real}"', env=self.env)
        self.assertEqual(rc, 0)
        names = out.split()
        self.assertEqual(len(names), 14, f"expected 14 squads, got {names}")
        self.assertIn("exif-core", names)
        self.assertNotIn('"exif-core"', names)

    def test_missing_manifest_is_an_error_not_an_empty_fleet(self):
        rc, out, _ = sh(f'parse_squads "{self.tmp}/nope.toml"', env=self.env)
        self.assertNotEqual(rc, 0)
        self.assertEqual(out.strip(), "")


class GitRepoMixin(ShellHarnessMixin):
    """Builds a real repo plus worker worktrees, with a local origin/main ref."""

    def setUp(self):
        super().setUp()
        self.repo = self.tmp / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "user.email", "test@example.invalid")
        git(self.repo, "config", "user.name", "Test")
        (self.repo / "README.md").write_text("base\n")
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "-m", "base")
        self.base_sha = git(self.repo, "rev-parse", "HEAD")
        # origin/main without a network: the fleet only ever reads this ref.
        git(self.repo, "update-ref", "refs/remotes/origin/main", self.base_sha)
        self.wtbase = self.tmp / "parallel-fix"
        self.wtbase.mkdir()
        self.env["FLEET_WORKTREE_BASE"] = str(self.wtbase)

    def add_worker(self, name, commits=0, dirty=False):
        wt = self.wtbase / name
        git(self.repo, "worktree", "add", "-q", "-b", f"model-fix-{name}", str(wt), "main")
        for i in range(commits):
            (wt / f"fix-{i}.rs").write_text(f"// worker fix {i}\n")
            git(wt, "add", "-A")
            git(wt, "commit", "-q", "-m", f"fix {i}")
        if dirty:
            (wt / "README.md").write_text("uncommitted worker edit\n")
        return wt

    def advance_origin_main(self, n=1):
        """Move origin/main forward so a worker worktree becomes BEHIND."""
        scratch = self.tmp / "scratch"
        if not scratch.exists():
            git(self.repo, "worktree", "add", "-q", "--detach", str(scratch), "main")
        for i in range(n):
            (scratch / f"upstream-{i}.rs").write_text(f"// upstream {i}\n")
            git(scratch, "add", "-A")
            git(scratch, "commit", "-q", "-m", f"upstream {i}")
        head = git(scratch, "rev-parse", "HEAD")
        git(self.repo, "update-ref", "refs/remotes/origin/main", head)
        return head


class TestUniqueCommits(GitRepoMixin, unittest.TestCase):
    def test_counts_commits_beyond_the_merge_base(self):
        wt = self.add_worker("w-ahead", commits=2)
        rc, out, _ = sh(f'worktree_unique_commits "{wt}"', env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(out.strip(), "2")

    def test_unique_commits_is_zero_when_merely_behind(self):
        """THE anti-`git diff` test.

        This worktree has NO work of its own -- origin/main simply moved on.
        `git diff <branch> main` would report every upstream file as a
        difference and mark it for salvage; the merge-base comparison
        correctly reports 0. Measured 2026-07-26: 30 of 32 worker worktrees
        sat in exactly this state.
        """
        wt = self.add_worker("w-behind", commits=0)
        self.advance_origin_main(3)
        rc, out, _ = sh(f'worktree_unique_commits "{wt}"', env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(out.strip(), "0")
        # And prove the naive measure really would have disagreed.
        diff = subprocess.run(  # nosec B603 B607
            ["git", "-C", str(wt), "diff", "--name-only", "HEAD", "origin/main"],
            capture_output=True, text=True, check=True,
        ).stdout.split()
        self.assertTrue(diff, "precondition: a diff-based check would see changes here")

    def test_diverged_worktree_counts_only_its_own_side(self):
        wt = self.add_worker("w-both", commits=2)
        self.advance_origin_main(4)
        rc, out, _ = sh(f'worktree_unique_commits "{wt}"', env=self.env)
        self.assertEqual(out.strip(), "2")


class TestSalvage(GitRepoMixin, unittest.TestCase):
    def test_preserves_commits_before_reset(self):
        wt = self.add_worker("w1", commits=2)
        tip = git(wt, "rev-parse", "HEAD")
        rc, out, _ = sh(f'salvage_worktree "{wt}" w1 20260727', env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(out.strip(), "salvage/w1-20260727")
        self.assertEqual(git(self.repo, "rev-parse", "salvage/w1-20260727"), tip)

    def test_preserves_uncommitted_tracked_changes(self):
        """`stash create` writes a commit object without touching the index or
        working tree, so the salvage exists BEFORE anything is destroyed."""
        wt = self.add_worker("w2", commits=0, dirty=True)
        rc, out, _ = sh(f'salvage_worktree "{wt}" w2 20260727', env=self.env)
        self.assertEqual(rc, 0)
        branch = out.strip()
        blob = git(self.repo, "show", f"{branch}:README.md")
        self.assertEqual(blob, "uncommitted worker edit")
        # The worktree itself must be untouched by salvaging.
        self.assertEqual((wt / "README.md").read_text(), "uncommitted worker edit\n")

    def test_clean_and_behind_worktree_is_not_salvaged(self):
        """No branch, no noise. A salvage list nobody trusts is a salvage list
        nobody reads."""
        wt = self.add_worker("w3", commits=0)
        self.advance_origin_main(2)
        rc, out, _ = sh(f'salvage_worktree "{wt}" w3 20260727', env=self.env)
        self.assertNotEqual(rc, 0)
        self.assertEqual(out.strip(), "")
        branches = git(self.repo, "branch", "--list", "salvage/*")
        self.assertEqual(branches, "")


class TestSyncWorktrees(GitRepoMixin, unittest.TestCase):
    def test_salvages_then_resets_every_worktree(self):
        ahead = self.add_worker("ahead", commits=1)
        ahead_tip = git(ahead, "rev-parse", "HEAD")
        behind = self.add_worker("behind", commits=0)
        new_main = self.advance_origin_main(2)

        rc, _, err = sh(f'sync_worktrees "{self.wtbase}"', env=self.env)
        self.assertEqual(rc, 0, err)

        self.assertEqual(git(ahead, "rev-parse", "HEAD"), new_main)
        self.assertEqual(git(behind, "rev-parse", "HEAD"), new_main)

        salvaged = git(self.repo, "branch", "--list", "salvage/*", "--format=%(refname:short)")
        self.assertEqual(len(salvaged.split()), 1, f"only the ahead worktree should salvage: {salvaged}")
        self.assertTrue(salvaged.startswith("salvage/ahead-"))
        # The salvaged commit must still be reachable after the reset.
        self.assertEqual(git(self.repo, "rev-parse", salvaged), ahead_tip)

    def test_skips_a_worktree_with_no_resolvable_head(self):
        broken = self.wtbase / "not-a-worktree"
        broken.mkdir()
        (broken / ".git").write_text("gitdir: /nonexistent\n")
        rc, _, err = sh(f'sync_worktrees "{self.wtbase}"', env=self.env)
        self.assertEqual(rc, 0)
        self.assertIn("SKIP not-a-worktree", err)

    def test_periodic_sync_preserves_in_flight_work(self):
        dirty = self.add_worker("dirty", commits=0, dirty=True)
        committed = self.add_worker("committed", commits=1)
        committed_tip = git(committed, "rev-parse", "HEAD")
        clean = self.add_worker("clean", commits=0)
        new_main = self.advance_origin_main(2)

        rc, _, err = sh(
            f'sync_worktrees "{self.wtbase}" preserve-work', env=self.env)
        self.assertEqual(rc, 0, err)

        # A live edit and a just-committed worker result both remain exactly
        # where the worker left them. Resetting either underneath the process
        # invalidates the attempt even if a salvage branch technically keeps
        # the bytes reachable.
        self.assertEqual((dirty / "README.md").read_text(),
                         "uncommitted worker edit\n")
        self.assertEqual(git(committed, "rev-parse", "HEAD"), committed_tip)
        self.assertIn("PRESERVE dirty", err)
        self.assertIn("PRESERVE committed", err)

        # Worktrees with no local work still catch up during the same pass.
        self.assertEqual(git(clean, "rev-parse", "HEAD"), new_main)

        salvaged = git(
            self.repo, "branch", "--list", "salvage/*",
            "--format=%(refname:short)").split()
        self.assertEqual(len(salvaged), 2)

    def test_missing_base_directory_is_not_an_error(self):
        rc, _, err = sh(f'sync_worktrees "{self.tmp}/absent"', env=self.env)
        self.assertEqual(rc, 0)
        self.assertIn("nothing to sync", err)


class TestRepoPinning(GitRepoMixin, unittest.TestCase):
    def test_accepts_a_checkout_on_main_at_origin_main(self):
        rc, _, _ = sh(f'repo_is_main_tracking "{self.repo}"', env=self.env)
        self.assertEqual(rc, 0)

    def test_main_tracking_requires_branch_name_main(self):
        """Same sha, different branch name -- and that is NOT good enough.

        ensure_integration_branch (parallel_model_fix_loop.py:405) returns the
        current branch verbatim unless it is literally "main", so a feature
        branch sitting exactly at origin/main silently redirects every
        round-end merge to itself. Measured 2026-07-26: the live dispatcher
        logged "merging into 'feat/fleet-runtime-defect-fixes'".
        """
        git(self.repo, "checkout", "-q", "-b", "feat/looks-fine")
        self.assertEqual(git(self.repo, "rev-parse", "HEAD"),
                         git(self.repo, "rev-parse", "origin/main"))
        rc, _, _ = sh(f'repo_is_main_tracking "{self.repo}"', env=self.env)
        self.assertNotEqual(rc, 0, "a non-main branch at origin/main must be rejected")

    def test_main_tracking_requires_head_at_origin_main(self):
        self.advance_origin_main(1)  # repo's main is now behind origin/main
        rc, _, _ = sh(f'repo_is_main_tracking "{self.repo}"', env=self.env)
        self.assertNotEqual(rc, 0)

    def test_find_main_worktree_prefers_the_main_checkout(self):
        self.add_worker("w1", commits=1)
        rc, out, _ = sh(f'find_main_worktree "{self.repo}"', env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(Path(out.strip()).resolve(), self.repo.resolve())

    def test_find_main_worktree_fails_when_nothing_tracks_main(self):
        git(self.repo, "checkout", "-q", "-b", "feat/x")
        rc, out, _ = sh(f'find_main_worktree "{self.repo}"', env=self.env)
        self.assertNotEqual(rc, 0)
        self.assertEqual(out.strip(), "")


class TestLiveness(ShellHarnessMixin, unittest.TestCase):
    @staticmethod
    def _marked_argv(marker):
        # A trailing argument, not `exec -a`: on this machine `sleep` is a
        # coreutils multi-call binary that dispatches on argv[0], so renaming
        # it makes it exit immediately with "unknown program".
        return ["python3", "-c", "import time; time.sleep(30)", marker]

    def spawn_marked(self, marker):
        """A real process whose argv contains `marker`."""
        p = subprocess.Popen(self._marked_argv(marker))  # nosec B603 B607
        self.addCleanup(self._reap, p)
        time.sleep(0.4)  # let exec land before anything inspects the argv
        return p

    @staticmethod
    def _reap(p):
        try:
            p.send_signal(signal.SIGKILL)
            p.wait(timeout=5)
        except Exception:  # noqa: BLE001 -- best-effort cleanup
            pass

    def test_alive_pid_with_matching_argv(self):
        p = self.spawn_marked("fleetprobe_alive")
        rc, _, _ = sh(f'pid_matches {p.pid} fleetprobe_alive', env=self.env)
        self.assertEqual(rc, 0)

    def test_alive_pid_with_wrong_argv_is_not_our_tier(self):
        """The PID-REUSE GUARD. A recycled pid belongs to something else, and
        signalling it is the launcher inflicting the exact collateral damage
        it exists to prevent (cf. 2fbf051c, "one recycled pgid must not kill
        the whole dispatcher")."""
        p = self.spawn_marked("fleetprobe_other")
        rc, _, _ = sh(f'pid_matches {p.pid} "squad_merge_loop.py --squad canon "', env=self.env)
        self.assertNotEqual(rc, 0)

    def test_dead_pid_is_dead(self):
        p = self.spawn_marked("fleetprobe_dead")
        pid = p.pid
        p.send_signal(signal.SIGKILL)
        p.wait(timeout=5)
        time.sleep(0.2)
        rc, _, _ = sh(f'pid_matches {pid} fleetprobe_dead', env=self.env)
        self.assertNotEqual(rc, 0)

    def test_refuses_to_call_itself_alive(self):
        rc, _, _ = sh('pid_matches $$ bash', env=self.env)
        self.assertNotEqual(rc, 0)

    def test_zero_and_garbage_pids_are_rejected(self):
        for bad in ("0", "", "-1", "notapid"):
            rc, _, _ = sh(f'pid_matches "{bad}" bash', env=self.env)
            self.assertNotEqual(rc, 0, f"pid {bad!r} must not be considered alive")

    def test_scan_excludes_self_where_naive_scan_self_matches(self):
        """Reproduces the 2026-07-26 false positive in miniature.

        The harness shell's own command line contains the marker (it is the
        text of the snippet being run), so a naive `ps | grep` finds itself
        and reports a running process that does not exist. `pgrep -f` fails
        the same way against an ancestor shell -- that is how a completely
        dead merger tier looked alive while nothing published for hours.
        """
        marker = "FLEETPROBE_SELFMATCH_ZQ7"
        rc, out, _ = sh(
            f'echo "naive=$(ps -axo pid=,command= | grep -c {marker})"\n'
            f'echo "scan=$(scan_foreign_pids "{marker}" | wc -l | tr -d " ")"',
            env=self.env,
        )
        self.assertEqual(rc, 0)
        values = dict(line.split("=", 1) for line in out.split())
        self.assertGreater(int(values["naive"]), 0,
                           "precondition: a naive scan must self-match here")
        self.assertEqual(int(values["scan"]), 0,
                         "scan_foreign_pids must never report the checking process")

    def test_scan_finds_a_genuine_foreign_process(self):
        """The exclusion must not be so broad that it hides a real fleet --
        that would turn preflight into a rubber stamp."""
        p = self.spawn_marked("fleetprobe_foreign")
        rc, out, _ = sh('scan_foreign_pids "fleetprobe_foreign"', env=self.env)
        self.assertEqual(rc, 0)
        self.assertIn(str(p.pid), out)


class TestBackoff(ShellHarnessMixin, unittest.TestCase):
    def test_doubles_then_caps(self):
        rc, out, _ = sh(
            'for n in 1 2 3 4 5 6 7 8; do printf "%s " "$(backoff_seconds $n 10 300)"; done',
            env=self.env,
        )
        self.assertEqual(rc, 0)
        self.assertEqual(out.split(), ["10", "20", "40", "80", "160", "300", "300", "300"])

    def test_never_exceeds_the_ceiling(self):
        rc, out, _ = sh('backoff_seconds 40 10 300', env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(out.strip(), "300")


class TestDiskPreflight(ShellHarnessMixin, unittest.TestCase):
    """Disk, not CPU, is this fleet's bottleneck: 32 worker worktrees each with
    their own cargo target dir. Measured 2026-07-26: / was 93% full."""

    def test_fails_below_the_floor_with_an_actionable_remedy(self):
        rc, _, err = sh(
            'free_gb() { printf 7; }\n'
            'PREFLIGHT_ERRORS=()\n'
            'preflight_disk\n'
            'printf "%s\\n" "${PREFLIGHT_ERRORS[@]}"',
            env={**self.env, "FLEET_MIN_FREE_GB": "40", "OXIDEX_HOME": str(self.tmp)},
        )
        self.assertEqual(rc, 0)
        _, out, _ = sh(
            'free_gb() { printf 7; }\nPREFLIGHT_ERRORS=()\npreflight_disk\n'
            'printf "%s\\n" "${PREFLIGHT_ERRORS[@]}"',
            env={**self.env, "FLEET_MIN_FREE_GB": "40", "OXIDEX_HOME": str(self.tmp)},
        )
        self.assertIn("only 7G free", out)
        self.assertIn("dashboard.log", out, "the remedy must name where the space actually is")
        self.assertIn("model-fix-requests", out)

    def test_passes_above_the_floor(self):
        _, out, _ = sh(
            'free_gb() { printf 500; }\nPREFLIGHT_ERRORS=()\npreflight_disk\n'
            'printf "count=%s\\n" "${#PREFLIGHT_ERRORS[@]}"',
            env={**self.env, "FLEET_MIN_FREE_GB": "40", "OXIDEX_HOME": str(self.tmp)},
        )
        self.assertIn("count=0", out)


class TestStatusAndDown(ShellHarnessMixin, unittest.TestCase):
    def test_status_without_state_reports_not_running(self):
        rc, out, _ = sh("cmd_status", env=self.env)
        self.assertEqual(rc, 3)
        self.assertIn("no state file", out)

    def test_status_reports_dead_tier_as_dead(self):
        Path(self.env["FLEET_STATEFILE"]).write_text(
            "supervisor\t999999\trunning\tfleet_up.sh\n"
            "merger:canon\t999998\trunning\tsquad_merge_loop.py --squad canon \n"
        )
        rc, out, _ = sh("cmd_status", env=self.env)
        self.assertEqual(rc, 1, "a recorded-running-but-dead tier must be a non-zero status")
        self.assertIn("DEAD", out)

    def test_down_does_not_kill_a_pid_whose_argv_no_longer_matches(self):
        """PID reuse again, on the most dangerous path: shutdown."""
        p = subprocess.Popen(  # nosec B603 B607
            TestLiveness._marked_argv("fleetprobe_innocent"),
        )
        self.addCleanup(TestLiveness._reap, p)
        time.sleep(0.4)
        Path(self.env["FLEET_STATEFILE"]).write_text(
            "supervisor\t0\tstopped\tfleet_up.sh\n"
            f"merger:canon\t{p.pid}\trunning\tsquad_merge_loop.py --squad canon \n"
        )
        rc, _, _ = sh("cmd_down", env=self.env)
        self.assertEqual(rc, 0)
        time.sleep(0.3)
        self.assertIsNone(p.poll(), "an unrelated process holding a recycled pid must survive --down")
        self.assertFalse(Path(self.env["FLEET_STATEFILE"]).exists())

    def test_down_without_state_is_a_noop(self):
        rc, out, _ = sh("cmd_down", env=self.env)
        self.assertEqual(rc, 0)
        self.assertIn("nothing to stop", out)


class TestArgParsing(ShellHarnessMixin, unittest.TestCase):
    def run_script(self, *args, env=None):
        e = dict(os.environ)
        e.pop("OXIDEX_HOME", None)
        e.update(self.env)
        if env:
            e.update(env)
        p = subprocess.run(  # nosec B603
            ["bash", str(SCRIPT), *args], capture_output=True, text=True, check=False, env=e,
        )
        return p.returncode, p.stdout, p.stderr

    def test_unknown_argument_is_a_usage_error(self):
        rc, _, err = self.run_script("--frobnicate")
        self.assertEqual(rc, 64)
        self.assertIn("unknown argument", err)

    def test_workers_must_be_a_positive_integer(self):
        for bad in ("abc", "0", "-4"):
            rc, _, err = self.run_script("--workers", bad)
            self.assertEqual(rc, 64, f"--workers {bad} should be rejected")
            self.assertIn("--workers", err)

    def test_help_exits_zero(self):
        rc, out, _ = self.run_script("--help")
        self.assertEqual(rc, 0)
        self.assertIn("fleet_up.sh", out)

    def test_status_is_safe_to_run_with_no_fleet(self):
        rc, out, _ = self.run_script("--status")
        self.assertEqual(rc, 3)
        self.assertIn("no state file", out)

    def test_squad_mode_is_forwarded_to_dispatcher(self):
        snippet = textwrap.dedent("""\
            FLEET_SQUAD_MODE=1
            PINNED_REPO=/tmp/fleet-test-repo
            PINNED_CONFIG=/tmp/fleet-test-config.toml
            spawn() { printf '%s\\n' "$*"; SPAWNED_PID=123; }
            log() { :; }
            now_epoch() { printf '1\\n'; }
            tier_add dispatcher dispatcher '' parallel_model_fix_loop.py
            tier_start 0
            """)
        rc, out, _ = sh(snippet, env=self.env)
        self.assertEqual(rc, 0)
        self.assertIn("--max-parallel 32", out)
        self.assertIn("--squad-mode", out)


class TestSupervisorIntegration(ShellHarnessMixin, unittest.TestCase):
    """The only test that actually starts, restarts and stops tiers.

    Everything else here is a pure function; spawn/supervise/shutdown is the
    part that has to be exercised for real, because its failure modes (a
    wrapper pid recorded instead of the child's, a restart that never fires, a
    shutdown that orphans children) are invisible to unit tests. Real tier
    scripts are replaced by sleep-forever stand-ins so the test costs seconds,
    not a 32-worker round.
    """

    POLL = 1
    TIMEOUT = 25

    def setUp(self):
        super().setUp()
        self.fake = self.tmp / "fakerepo"
        (self.fake / "scripts").mkdir(parents=True)
        for name in ("parallel_model_fix_loop.py", "squad_merge_loop.py",
                     "judgment_queue_daemon.py"):
            (self.fake / "scripts" / name).write_text(
                "import sys, time\n"
                "sys.stderr.write('fake tier up: ' + ' '.join(sys.argv[1:]) + '\\n')\n"
                "sys.stderr.flush()\n"
                "while True: time.sleep(0.5)\n"
            )
        (self.fake / "scripts" / "squads.toml").write_text(
            "[squads.canon]\nmodules = []\n\n[squads.\"exif-core\"]\nmodules = []\n"
        )
        (self.fake / "config.toml").write_text("[worker]\n")
        self.env.update({
            "FLEET_POLL_SECONDS": str(self.POLL),
            "FLEET_BACKOFF_BASE": "1",
            "FLEET_BACKOFF_MAX": "2",
            "FLEET_GRACE_SECONDS": "3",
            "FLEET_MAX_RESTARTS": "2",
        })
        self.sup = None

    def start_supervisor(self):
        # pin_repo / preflight / sync are stubbed: this test is about the
        # lifecycle, and all three are covered by their own tests above.
        snippet = textwrap.dedent(f"""\
            export FLEET_UP_SOURCE_ONLY=1
            source {SCRIPT}
            set +e
            pin_repo() {{ PINNED_REPO="{self.fake}"; PINNED_CONFIG="{self.fake}/config.toml"; cd "$PINNED_REPO"; }}
            run_preflight() {{ :; }}
            sync_worktrees() {{ :; }}
            cmd_up
            """)
        e = dict(os.environ)
        e.pop("OXIDEX_HOME", None)
        e.update(self.env)
        self.sup = subprocess.Popen(  # nosec B603 B607
            ["bash", "-c", snippet], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, env=e,
        )
        self.addCleanup(self._stop_supervisor)

    def _stop_supervisor(self):
        if self.sup and self.sup.poll() is None:
            self.sup.send_signal(signal.SIGKILL)
            self.sup.wait(timeout=5)
        if self.sup:
            for stream in (self.sup.stdout, self.sup.stderr):
                if stream is not None:
                    stream.close()
        for pid in self._recorded_pids():
            try:
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass
        # Belt and braces: the state file only lists tiers that were RUNNING
        # at the last poll, so a tier sitting in backoff (pid 0), or any child
        # of a supervisor that was killed before it could write state, would
        # leak a sleep-forever process. Sweeping by this test's own unique
        # tempdir path cannot touch anything outside the test.
        marker = str(self.fake)
        listing = subprocess.run(  # nosec B603 B607
            ["ps", "-axo", "pid=,command="], capture_output=True, text=True, check=False,
        ).stdout
        for line in listing.splitlines():
            if marker not in line:
                continue
            pid_text = line.strip().split(" ", 1)[0]
            if not pid_text.isdigit() or int(pid_text) == os.getpid():
                continue
            try:
                os.kill(int(pid_text), signal.SIGKILL)
            except OSError:
                pass

    def _state(self):
        path = Path(self.env["FLEET_STATEFILE"])
        if not path.exists():
            return {}
        rows = {}
        for line in path.read_text().splitlines():
            parts = line.split("\t")
            if len(parts) == 4:
                rows[parts[0]] = (int(parts[1]), parts[2], parts[3])
        return rows

    def _recorded_pids(self):
        return [pid for tag, (pid, _, _) in self._state().items()
                if tag != "supervisor" and pid > 0]

    @staticmethod
    def _alive(pid):
        try:
            os.kill(pid, 0)
        except OSError:
            return False
        return True

    def _wait_for(self, predicate, what):
        deadline = time.time() + self.TIMEOUT
        while time.time() < deadline:
            value = predicate()
            if value:
                return value
            time.sleep(0.25)
        self.fail(f"timed out waiting for {what}; state={self._state()}")

    def test_starts_every_tier_records_real_pids_restarts_and_stops_clean(self):
        self.start_supervisor()

        # 2 squads + dispatcher + judgment == 4 tiers, plus the supervisor row.
        state = self._wait_for(
            lambda: self._state() if len(self._state()) == 5 else None,
            "all five state rows",
        )
        self.assertEqual(
            sorted(state),
            ["dispatcher", "judgment", "merger:canon", "merger:exif-core", "supervisor"],
        )

        # The recorded pid must be the CHILD, not a wrapper: `spawn` uses
        # `exec` precisely so that $! is the python process. If it were a
        # wrapper, its argv would not contain the tier script name.
        for tag, (pid, _, pattern) in state.items():
            if tag == "supervisor":
                continue
            self.assertTrue(self._alive(pid), f"{tag} pid {pid} is not alive")
            argv = subprocess.run(  # nosec B603 B607
                ["ps", "-o", "command=", "-p", str(pid)],
                capture_output=True, text=True, check=False,
            ).stdout
            self.assertIn(pattern.strip(), argv,
                          f"{tag}: recorded pid {pid} does not carry the tier argv")

        # Consolidated log: one file, one prefix per tier.
        log = Path(self.env["FLEET_LOG"])
        self._wait_for(lambda: "[merger:canon]" in log.read_text(), "prefixed merger output")
        text = log.read_text()
        self.assertIn("[dispatcher]", text)
        self.assertIn("[judgment]", text)

        # Kill a tier the way an OOM killer would and confirm it comes back
        # with a DIFFERENT pid. Nothing in the fleet did this before: on
        # 2026-07-25 seven mergers died together and stayed dead for an hour.
        old = state["merger:canon"][0]
        os.kill(old, signal.SIGKILL)
        new = self._wait_for(
            lambda: (lambda p: p if p and p != old and self._alive(p) else None)(
                self._state().get("merger:canon", (0,))[0]),
            "merger:canon to be restarted",
        )
        self.assertNotEqual(new, old)

        # Clean shutdown: SIGTERM the supervisor, every child must go with it.
        pids = self._recorded_pids()
        self.assertTrue(pids)
        self.sup.send_signal(signal.SIGTERM)
        self.sup.wait(timeout=self.TIMEOUT)
        self.assertEqual(self.sup.returncode, 0)
        deadline = time.time() + 10
        while time.time() < deadline and any(self._alive(p) for p in pids):
            time.sleep(0.25)
        survivors = [p for p in pids if self._alive(p)]
        self.assertEqual(survivors, [], "SIGTERM must not orphan tier processes")
        self.assertFalse(Path(self.env["FLEET_PIDFILE"]).exists(),
                         "the pidfile must not outlive the supervisor")

    def test_gives_up_on_a_tier_that_cannot_stay_up(self):
        """A crash-looping tier has to SURFACE. Burying it under an infinite
        restart loop is how the fleet used to look busy while doing nothing."""
        (self.fake / "scripts" / "judgment_queue_daemon.py").write_text(
            "import sys\nsys.stderr.write('judgment tier: exploding\\n')\nraise SystemExit(3)\n"
        )
        self.start_supervisor()
        self._wait_for(
            lambda: self._state().get("judgment", (0, "", ""))[1] == "failed",
            "the judgment tier to be marked failed",
        )
        # ...and the tiers that ARE healthy must keep running.
        state = self._state()
        self.assertEqual(state["dispatcher"][1], "running")
        self.assertTrue(self._alive(state["dispatcher"][0]))
        text = Path(self.env["FLEET_LOG"]).read_text()
        self.assertIn("GIVING UP on judgment", text)


class TestPeriodicResync(ShellHarnessMixin, unittest.TestCase):
    """sync_worktrees ran ONCE, immediately before supervise().

    A fleet left up for hours therefore produced work against an ever-staler
    base. Measured 2026-07-28 on a live run: 71 of 72 worker worktrees behind
    origin/main, 11 of them by 21 commits with their own uncaptured commits on
    top. Stale bases are precisely how the 155-patch backlog became
    unmergeable, so supervise() now re-syncs on an interval.

    supervise() itself needs the whole tier array set plus its fd-3 sleep
    channel to run at all, so what these pin is `resync_due` -- the policy --
    plus a structural check that the loop actually consults it.
    """

    def _due(self, now, last, interval):
        rc, out, _err = sh(
            f'export FLEET_UP_SOURCE_ONLY=1; source {SCRIPT}; '
            f'resync_due {now} {last} {interval} && echo DUE || echo NOT',
            env=self.env)
        return out.strip()

    def test_fires_once_the_interval_has_elapsed(self):
        self.assertEqual(self._due(1800, 0, 1800), "DUE")
        self.assertEqual(self._due(3600, 1800, 1800), "DUE")

    def test_does_not_fire_early(self):
        self.assertEqual(self._due(1799, 0, 1800), "NOT")
        self.assertEqual(self._due(0, 0, 1800), "NOT")

    def test_zero_disables_it(self):
        # An operator must be able to turn it off: a resync competing with a
        # mid-round worker is a real hazard, so this is not forced on.
        self.assertEqual(self._due(999999, 0, 0), "NOT")

    def test_live_dispatcher_worker_blocks_resync(self):
        pgids = self.logdir / "dispatcher-pgids.json"
        pgids.write_text(json.dumps({"pgids": [os.getpgrp()]}))
        rc, out, _err = sh(
            f'dispatcher_workers_active "{pgids}" && echo ACTIVE || echo IDLE',
            env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(out.strip(), "ACTIVE")

    def test_stale_dispatcher_state_does_not_block_resync(self):
        pgids = self.logdir / "dispatcher-pgids.json"
        pgids.write_text(json.dumps({"pgids": [99999999]}))
        rc, out, _err = sh(
            f'dispatcher_workers_active "{pgids}" && echo ACTIVE || echo IDLE',
            env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(out.strip(), "IDLE")

        pgids.write_text(json.dumps({"pgids": [10 ** 100]}))
        rc, out, _err = sh(
            f'dispatcher_workers_active "{pgids}" && echo ACTIVE || echo IDLE',
            env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(out.strip(), "IDLE")

    def test_torn_dispatcher_state_fails_closed(self):
        pgids = self.logdir / "dispatcher-pgids.json"
        pgids.write_text("{torn")
        rc, out, _err = sh(
            f'dispatcher_workers_active "{pgids}" && echo ACTIVE || echo IDLE',
            env=self.env)
        self.assertEqual(rc, 0)
        self.assertEqual(out.strip(), "ACTIVE")

    def test_the_supervise_loop_consults_it(self):
        # Structural, because the loop is not directly runnable here. Without
        # this the predicate could be perfect and never called -- the exact
        # shape of dead wiring this session has hit twice.
        body = SCRIPT.read_text() if hasattr(SCRIPT, "read_text") else open(SCRIPT).read()
        loop = body[body.index("supervise() {"):]
        self.assertIn("resync_due", loop)
        self.assertIn("dispatcher_workers_active", loop)
        self.assertIn("periodic worktree sync deferred", loop)
        self.assertIn(
            'sync_worktrees "$FLEET_WORKTREE_BASE" preserve-work', loop)


if __name__ == "__main__":
    unittest.main()
