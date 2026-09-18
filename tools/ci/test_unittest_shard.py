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
    """`ci.yml`'s shard matrix must be the only place the count is written.

    `unittest_shard.py` partitions the suite against `--of`; the matrix decides
    which partitions actually run. A second literal for the count lets those
    drift, and the drift is *silent*: the shards that do run still pass, so the
    job stays green while their tests never execute. Measured on this suite at
    `0f92071b`, matrix `[1..8]` against a stray `--of 10` left 312 of 1512 ids
    (20.6%) in no shard that runs, with no failure anywhere.

    So `--of` is `${{ strategy.job-total }}` -- the matrix's own length -- and
    these tests keep it that way. What remains to check is that the matrix
    enumerates `1..len(matrix)`: `unittest_shard.py` rejects a shard index
    above `--of` loudly, but a matrix that merely *skips* an index in range
    would drop that partition quietly.
    """

    CI_YAML = pathlib.Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"
    SUITE = pathlib.Path(__file__).resolve().parents[1] / "exiftool-tables"

    def job(self):
        text = self.CI_YAML.read_text()
        parts = text.split("\n  verify-tables-tools:\n", 1)
        self.assertEqual(len(parts), 2, "verify-tables-tools job not found in ci.yml")
        return re.split(r"\n  [a-z0-9-]+:\n", parts[1], maxsplit=1)[0]

    def matrix(self):
        found = re.search(r"shard:\s*\[([0-9,\s]+)\]", self.job())
        self.assertIsNotNone(found, "could not read the shard matrix from ci.yml")
        return [int(n) for n in found.group(1).split(",")]

    def test_the_shard_count_is_written_once_in_the_matrix(self):
        body = self.job()
        # Anchor on the invocation, not a prose mention of the same filename.
        invocation = re.search(r"python3[^\n]*unittest_shard\.py[^\n]*--shard[^\n]*", body)
        self.assertIsNotNone(invocation, "unittest_shard.py invocation not found")
        self.assertIn(
            '--of "$SHARD_TOTAL"',
            invocation.group(0),
            "--of must come from the matrix via $SHARD_TOTAL, never a second literal",
        )
        self.assertRegex(
            body,
            r"SHARD_TOTAL:\s*\$\{\{\s*strategy\.job-total\s*\}\}",
            "SHARD_TOTAL must be strategy.job-total, the matrix's own length",
        )
        self.assertNotRegex(
            invocation.group(0),
            r"--of\s+\d",
            "a literal --of reintroduces the drift this job was rewired to remove",
        )
        # Tolerant of spacing inside `${{ }}`, like the SHARD_TOTAL check
        # above: a reformat should not fail this, only a real second literal.
        # Anchored on the job's own `name:` (the first in the body), not a
        # step's `- name:`.
        self.assertRegex(
            body.split("\n", 1)[0] if body.startswith("    name:")
            else re.search(r"^    name:[^\n]*", body, re.M).group(0),
            r"tools \$\{\{\s*matrix\.shard\s*\}\}/\$\{\{\s*strategy\.job-total\s*\}\}",
            "the display name should report strategy.job-total too, not a literal")

    def test_the_matrix_enumerates_every_index_from_one(self):
        matrix = self.matrix()
        self.assertEqual(
            matrix,
            list(range(1, len(matrix) + 1)),
            "the matrix must be 1..N with no gaps: strategy.job-total is its "
            "length, so a skipped index leaves that partition's tests unrun",
        )

    def test_the_committed_wiring_runs_every_discovered_test(self):
        """The invariant the other two guard, checked end to end.

        Discovery is dynamic, so this also covers a suite that has grown: a
        module added for newly-generated tags is discovered here and must land
        in one of the shards `ci.yml` actually runs.
        """
        matrix = self.matrix()
        # `top_level_dir` explicitly: the suite directory is not a package, and
        # without it Python 3.9's loader raises "Start directory is not
        # importable". CI runs this job on whatever python3 the image ships.
        suite = unittest.defaultTestLoader.discover(
            str(self.SUITE), pattern="test_*.py", top_level_dir=str(self.SUITE))
        tests = list(shard.flatten(suite))
        self.assertGreater(len(tests), 100, "discovery found almost nothing; wrong start dir?")
        weights = shard.weigh(tests, json.loads(
            pathlib.Path(__file__).with_name("unittest_weights.json").read_text())["seconds"])
        partitions = shard.partition(weights, len(matrix))
        covered = {i for index in matrix for i in partitions[index - 1]}
        missing = sorted(set(weights) - covered)
        self.assertEqual(
            missing, [], f"{len(missing)} of {len(weights)} tools tests are in no shard ci.yml runs")
        # And exactly once: the union of the shards ci.yml runs is the
        # discovered suite, with no id in two shards.
        ran = [i for index in matrix for i in partitions[index - 1]]
        self.assertEqual(len(ran), len(set(ran)), "a test id is in more than one shard")
        self.assertEqual(sorted(ran), sorted(test.id() for test in tests))

    def test_a_module_with_no_recorded_timing_still_lands_in_a_run_shard(self):
        """The growth case: tags generate a new test module, unweighted.

        `weigh` gives an unknown module `DEFAULT_MODULE_SECONDS` *in total* and
        spreads it across its tests, so its tests are partitioned like any
        other -- never dropped. Note the estimate is a module total, not a
        per-test one: 50 new tests share 5 s, so a genuinely slow new module is
        under-weighted until `unittest_weights.json` records it. That
        misbalances a shard against the 30-minute timeout; it does not lose a
        test.
        """
        matrix = self.matrix()
        newcomers = [f"test_newly_generated_tags.Case.test_{i}" for i in range(50)]
        weights = shard.weigh([Fake(i) for i in newcomers], {})
        self.assertAlmostEqual(sum(weights.values()), shard.DEFAULT_MODULE_SECONDS)
        partitions = shard.partition(weights, len(matrix))
        covered = {i for index in matrix for i in partitions[index - 1]}
        self.assertEqual(sorted(covered), sorted(newcomers))


class WeightsFreshnessTests(unittest.TestCase):
    """`unittest_weights.json` must cover (nearly) every discovered module.

    An unweighted module is still run -- `weigh` gives it
    `DEFAULT_MODULE_SECONDS` in total -- so staleness never fails a shard; it
    only unbalances them, silently. At PR run 35394817483, 16 of 139 modules
    had no weight and shard wall time spread from 248 s to 436 s. This fails
    once more than MAX_UNWEIGHTED_MODULES discovered modules lack an entry, so
    the file is refreshed while the drift is still small. Discovery only; no
    test is executed.
    """

    MAX_UNWEIGHTED_MODULES = 5
    REFRESH = "python3 tools/ci/unittest_weights_from_run.py <green-run-id>"
    SUITE = WorkflowWiringTests.SUITE

    def test_few_discovered_modules_lack_a_recorded_weight(self):
        suite = unittest.defaultTestLoader.discover(
            str(self.SUITE), pattern="test_*.py", top_level_dir=str(self.SUITE))
        tests = list(shard.flatten(suite))
        self.assertGreater(len(tests), 100, "discovery found almost nothing; wrong start dir?")
        seconds = json.loads(
            pathlib.Path(__file__).with_name("unittest_weights.json").read_text())["seconds"]
        unweighted = shard.unweighted_modules(tests, seconds)
        self.assertLessEqual(
            len(unweighted), self.MAX_UNWEIGHTED_MODULES,
            f"{len(unweighted)} discovered test modules have no timing in "
            f"tools/ci/unittest_weights.json (limit {self.MAX_UNWEIGHTED_MODULES}), so the "
            f"CI tools shards are balanced on guesses: {', '.join(unweighted)}. "
            f"Refresh from a recent green CI run: {self.REFRESH}")

    def test_unweighted_modules_ignores_import_failures_and_weighted_modules(self):
        tests = [Fake("test_a.C.test_x"), Fake("test_b.C.test_y"),
                 Fake("unittest.loader._FailedTest.test_c")]
        self.assertEqual(shard.unweighted_modules(tests, {"test_a": 3}), ["test_b"])
        # A full-id entry alone does not weigh its module.
        self.assertEqual(shard.unweighted_modules(tests, {"test_b.C.test_y": 40, "test_a": 1}),
                         ["test_b"])
