"""Log parsing of the weights generator, on a synthetic `gh run view --log`."""
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "unittest_weights_from_run", pathlib.Path(__file__).with_name("unittest_weights_from_run.py"))
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)

JOB = "Verify Generated Tables / tools 2/8\tRun tools suite shard\t"
LOG = [
    "2026-09-18T21:17:11.0000000Z   SHARD_TOTAL: 8",
    "2026-09-18T21:17:12.0000000Z shard 2/8: 4 of 30 tests",
    "2026-09-18T21:17:13.5000000Z test_a (test_m.C.test_a) ... ok",
    # Output interleaved after the "... ": the result lands on a later line.
    "2026-09-18T21:17:14.0000000Z test_b (test_m.C.test_b) ... fatal: noise",
    "2026-09-18T21:17:20.0000000Z ok",
    # A docstring moves the result onto the description's second line.
    "2026-09-18T21:17:20.1000000Z test_c (test_n.D.test_c)",
    "2026-09-18T21:17:52.0000000Z The docstring line. ... ok",
    "2026-09-18T21:17:53.0000000Z test_d (test_n.D.test_d) ... skipped 'pinned tree absent'",
    "2026-09-18T21:17:53.0100000Z ",
    "2026-09-18T21:17:53.0200000Z Ran 4 tests in 41.000s",
]


class ParseShardTests(unittest.TestCase):
    def test_seconds_run_from_result_to_result(self):
        banner, seconds, ran, ran_seconds = gen.parse_shard([JOB + line for line in LOG])
        self.assertEqual(banner.group(1, 2, 3, 4), ("2", "8", "4", "30"))
        self.assertEqual((ran, ran_seconds), (4, 41.0))
        self.assertEqual(
            {k: round(v, 3) for k, v in seconds.items()},
            {"test_m.C.test_a": 1.5, "test_m.C.test_b": 6.5,
             "test_n.D.test_c": 32.0, "test_n.D.test_d": 1.0})
        self.assertAlmostEqual(sum(seconds.values()), ran_seconds)

    def test_weights_are_module_totals_plus_slow_ids(self):
        out = gen.weights({"test_m.C.test_a": 1.5, "test_m.C.test_b": 6.5,
                           "test_n.D.test_c": 32.0, "test_n.D.test_d": 0.01})
        self.assertEqual(out, {"test_m": 8.0, "test_n": 32.0, "test_n.D.test_c": 32.0})

    def test_runs_average_per_test_over_the_runs_that_ran_it(self):
        self.assertEqual(gen.average([{"t.C.a": 10.0, "t.C.b": 2.0}, {"t.C.a": 30.0}]),
                         {"t.C.a": 20.0, "t.C.b": 2.0})

    def test_a_log_without_the_banner_refuses(self):
        with self.assertRaises(ValueError):
            gen.parse_shard([JOB + line for line in LOG if "shard 2/8" not in line])


if __name__ == "__main__":
    unittest.main()
