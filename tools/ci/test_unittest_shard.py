"""Partition properties of the unittest sharder."""
import importlib.util
import pathlib
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
