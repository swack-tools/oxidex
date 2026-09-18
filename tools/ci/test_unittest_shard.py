"""Partition properties of the unittest sharder, and its wiring in ci.yml."""
import importlib.util
import json
import pathlib
import re
import unittest

spec = importlib.util.spec_from_file_location("unittest_shard", pathlib.Path(__file__).with_name("unittest_shard.py"))
shard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shard)


class Fake(unittest.TestCase):
    def __init__(self, test_id):
        super().__init__("run")
        self._id = test_id

    def id(self):
        return self._id

    def run(self, result=None):
        pass


class ShardTests(unittest.TestCase):
    def ids(self):
        return [f"test_mod{m}.Case.test_{t}" for m in range(7) for t in range(m + 1)]

    def test_every_test_runs_in_exactly_one_shard(self):
        tests = [Fake(i) for i in self.ids()]
        weights = shard.weigh(tests, {"test_mod3": 40, "test_mod5.Case.test_0": 90})
        for count in (1, 2, 5, 40):
            parts = shard.partition(weights, count)
            flat = [i for part in parts for i in part]
            self.assertEqual(sorted(flat), sorted(self.ids()))
            self.assertEqual(len(flat), len(set(flat)))

    def test_partition_is_deterministic_and_balances_heavy_ids(self):
        tests = [Fake(i) for i in self.ids()]
        weights = shard.weigh(tests, {"test_mod5.Case.test_0": 90, "test_mod6.Case.test_0": 90})
        first = shard.partition(weights, 3)
        self.assertEqual(first, shard.partition(dict(reversed(list(weights.items()))), 3))
        heavy = [i for i, part in enumerate(first) if {"test_mod5.Case.test_0", "test_mod6.Case.test_0"} & set(part)]
        self.assertEqual(len(heavy), 2)

    def test_explicit_ids_take_their_weight_from_the_module_total(self):
        tests = [Fake("test_a.C.test_x"), Fake("test_a.C.test_y"), Fake("test_a.C.test_z")]
        weights = shard.weigh(tests, {"test_a": 30, "test_a.C.test_x": 20})
        self.assertEqual(weights, {"test_a.C.test_x": 20.0, "test_a.C.test_y": 5.0, "test_a.C.test_z": 5.0})
        unknown = shard.weigh([Fake("test_new.C.test_x")], {})
        self.assertEqual(unknown, {"test_new.C.test_x": shard.DEFAULT_MODULE_SECONDS})


if __name__ == "__main__":
    unittest.main()


class WorkflowWiringTests(unittest.TestCase):
    """The shard count lives in three separate literals in `ci.yml`.

    `verify-tables-tools` names itself `.../tools ${{ matrix.shard }}/8`,
    enumerates `shard: [1..8]`, and passes `--of 8`. Nothing tied those
    together, and a drift between them removes tests from CI *silently*:
    every shard that does run still passes, so the job stays green. Measured
    on this suite at `0f92071b` -- matrix `[1..8]` against `--of 10` runs
    1,200 of 1,512 ids, dropping 312 (20.6%) with no failure anywhere.

    That is also the shape the sharding has to survive as the catalog grows:
    `unittest_shard.py` discovers ids dynamically, so a new test module is
    picked up on its own, but only if every shard index is actually run.
    """

    CI_YAML = pathlib.Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"
    SUITE = pathlib.Path(__file__).resolve().parents[1] / "exiftool-tables"

    def wiring(self):
        """-> (display_n, matrix_list, of_n) read from the shard job."""
        text = self.CI_YAML.read_text()
        job = text.split("\n  verify-tables-tools:\n", 1)
        self.assertEqual(len(job), 2, "verify-tables-tools job not found in ci.yml")
        # The job ends at the next top-level job key.
        body = re.split(r"\n  [a-z0-9-]+:\n", job[1], maxsplit=1)[0]

        display = re.search(r"name:.*?/\s*tools\s*\$\{\{\s*matrix\.shard\s*\}\}/(\d+)", body)
        matrix = re.search(r"shard:\s*\[([0-9,\s]+)\]", body)
        of = re.search(r"unittest_shard\.py[^\n]*--of\s+(\d+)", body)
        for name, found in (("display name", display), ("matrix list", matrix), ("--of", of)):
            self.assertIsNotNone(found, f"could not read the shard {name} from ci.yml")
        return (
            int(display.group(1)),
            [int(n) for n in matrix.group(1).split(",")],
            int(of.group(1)),
        )

    def test_the_three_shard_count_literals_agree(self):
        display_n, matrix_list, of_n = self.wiring()
        self.assertEqual(
            matrix_list,
            list(range(1, of_n + 1)),
            "ci.yml's matrix must enumerate exactly 1..--of; any other list "
            "leaves the missing shards' tests unrun while CI stays green",
        )
        self.assertEqual(display_n, of_n, "the job's display name disagrees with --of")

    def test_the_committed_wiring_runs_every_discovered_test(self):
        """The invariant the other two guard, checked end to end.

        Discovery is dynamic, so this also covers a suite that has grown: a
        module added for newly-generated tags is discovered here and must land
        in one of the shards `ci.yml` actually runs.
        """
        _, matrix_list, of_n = self.wiring()
        suite = unittest.defaultTestLoader.discover(str(self.SUITE), pattern="test_*.py")
        tests = list(shard.flatten(suite))
        self.assertGreater(len(tests), 100, "discovery found almost nothing; wrong start dir?")
        weights = shard.weigh(tests, json.loads(
            (pathlib.Path(__file__).with_name("unittest_weights.json")).read_text())["seconds"])
        partitions = shard.partition(weights, of_n)
        covered = {i for index in matrix_list for i in partitions[index - 1]}
        missing = sorted(set(weights) - covered)
        self.assertEqual(
            missing,
            [],
            f"{len(missing)} of {len(weights)} tools tests are in no shard ci.yml runs",
        )

    def test_a_module_with_no_recorded_timing_still_lands_in_a_run_shard(self):
        """The growth case: tags generate a new test module, unweighted.

        `weigh` gives an unknown module `DEFAULT_MODULE_SECONDS` *in total*
        and spreads it across its tests, so its tests are partitioned like any
        other -- never dropped. Note the estimate is a module total, not a
        per-test one: 50 new tests share 5 s, so a genuinely slow new module
        is under-weighted until `unittest_weights.json` records it. That
        misbalances a shard; it does not lose a test.
        """
        _, matrix_list, of_n = self.wiring()
        newcomers = [f"test_newly_generated_tags.Case.test_{i}" for i in range(50)]
        weights = shard.weigh([Fake(i) for i in newcomers], {})
        self.assertAlmostEqual(sum(weights.values()), shard.DEFAULT_MODULE_SECONDS)
        partitions = shard.partition(weights, of_n)
        covered = {i for index in matrix_list for i in partitions[index - 1]}
        self.assertEqual(sorted(covered), sorted(newcomers))
