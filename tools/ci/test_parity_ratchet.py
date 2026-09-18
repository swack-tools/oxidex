"""The ratchet's refusals: regression, absence, and re-baselining."""
import contextlib
import importlib.util
import json
import os
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "parity_ratchet", pathlib.Path(__file__).with_name("parity_ratchet.py"))
ratchet = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ratchet)

FLOORS_PATH = pathlib.Path(__file__).with_name("parity_floors.json")


def floors(**metrics):
    """Floors over one source named `s`; metric keys are given unprefixed."""
    return {"schema": 1, "measured_at": {"commit": "abc", "exiftool": "13.59"},
            "sources": {"s": "unused.json"},
            "metrics": {f"s:counts.{name}": {"direction": d, "floor": f}
                        for name, (d, f) in metrics.items()}}


def sources(counts):
    return {"s": {"schema": 3, "counts": counts}}


@contextlib.contextmanager
def scratch(counts, floors_doc, body=None):
    """A temp root holding `m.json` plus a floors file that points at it.

    Yields `(argv_prefix, floors_path)`; pass the prefix to `ratchet.main` so
    sources resolve inside the temp root rather than the real repository.
    """
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / "m.json").write_text(json.dumps(
            body if body is not None else {"schema": 3, "counts": counts}))
        for name, spec in floors_doc.get("sources", {}).items():
            if not spec.startswith("m.json"):
                floors_doc["sources"][name] = "m.json"
        path = root / "floors.json"
        path.write_text(json.dumps(floors_doc))
        yield ["--root", str(root), "--floors", str(path)], path


class DirectionTests(unittest.TestCase):
    def test_at_least_falls_is_a_regression_and_rising_is_not(self):
        spec = floors(reads=("at_least", 100))
        self.assertEqual(ratchet.evaluate(sources({"reads": 100}), spec), ([], [], []))
        _, improved, _ = ratchet.evaluate(sources({"reads": 140}), spec)
        self.assertEqual(improved, ["s:counts.reads: 100 -> 140"])
        regressed, _, _ = ratchet.evaluate(sources({"reads": 99}), spec)
        self.assertEqual(regressed, ["s:counts.reads: fell to 99 from a floor of 100"])

    def test_at_most_rising_is_a_regression_and_falling_is_not(self):
        spec = floors(refused=("at_most", 156))
        regressed, _, _ = ratchet.evaluate(sources({"refused": 157}), spec)
        self.assertEqual(regressed, ["s:counts.refused: rose to 157 from a ceiling of 156"])
        _, improved, _ = ratchet.evaluate(sources({"refused": 12}), spec)
        self.assertEqual(improved, ["s:counts.refused: 156 -> 12"])

    def test_exact_refuses_movement_in_either_direction(self):
        spec = floors(denominator=("exact", 33487))
        for value in (33486, 33488):
            regressed, _, _ = ratchet.evaluate(sources({"denominator": value}), spec)
            self.assertEqual(
                regressed, [f"s:counts.denominator: changed to {value} from a pinned 33487"])
        self.assertEqual(ratchet.evaluate(sources({"denominator": 33487}), spec), ([], [], []))


class AbsenceTests(unittest.TestCase):
    """An absent metric is the failure mode this repo keeps paying for."""

    def test_a_missing_metric_is_reported_missing_not_passed(self):
        spec = floors(gone=("at_least", 5))
        regressed, _, missing = ratchet.evaluate(sources({"other": 5}), spec)
        self.assertEqual((regressed, missing), ([], ["s:counts.gone"]))

    def test_check_exits_non_zero_on_a_missing_metric_alone(self):
        with scratch({"other": 5}, floors(gone=("at_least", 5))) as (argv, _):
            self.assertEqual(ratchet.main(["check", *argv]), 1)

    def test_a_non_numeric_or_boolean_value_counts_as_absent(self):
        # `True` is an int in Python; a flag flipping into a counted slot must
        # not silently satisfy a numeric floor.
        for value in ("many", None, True, {"nested": 1}):
            _, _, missing = ratchet.evaluate(
                sources({"reads": value}), floors(reads=("at_least", 1)))
            self.assertEqual(missing, ["s:counts.reads"], f"{value!r} should read as absent")

    def test_a_dotted_path_through_a_missing_parent_is_absent(self):
        _, _, missing = ratchet.evaluate(
            sources({}), floors(**{"write_parity.observed": ("at_least", 0)}))
        self.assertEqual(missing, ["s:counts.write_parity.observed"])


class RaiseTests(unittest.TestCase):
    def test_raise_moves_floors_up_and_records_the_note(self):
        with scratch({"reads": 140}, floors(reads=("at_least", 100))) as (argv, path):
            code = ratchet.main(["raise", *argv, "--note", "batch 3"])
            written = json.loads(path.read_text())
        self.assertEqual(code, 0)
        self.assertEqual(written["metrics"]["s:counts.reads"]["floor"], 140)
        self.assertEqual(written["measured_at"]["note"], "batch 3")

    def test_raise_refuses_while_a_tracked_metric_is_absent(self):
        with scratch({"other": 1}, floors(reads=("at_least", 100))) as (argv, _):
            with self.assertRaisesRegex(SystemExit, "absent"):
                ratchet.main(["raise", *argv])


class SourceTests(unittest.TestCase):
    def test_a_source_with_no_counts_object_refuses(self):
        with scratch(None, floors(reads=("at_least", 1)), body={"schema": 3}) as (argv, _):
            with self.assertRaisesRegex(SystemExit, "no `counts` object"):
                ratchet.main(["check", *argv])

    def test_a_sub_document_selector_reads_the_inner_join(self):
        spec = floors(reads=("at_least", 1))
        spec["sources"]["s"] = "m.json#observed_join"
        with scratch(None, spec, body={"observed_join": {"counts": {"reads": 7}}}) as (argv, _):
            self.assertEqual(ratchet.main(["check", *argv]), 0)

    def test_a_missing_sub_document_refuses(self):
        spec = floors(reads=("at_least", 1))
        spec["sources"]["s"] = "m.json#observed_join"
        with scratch(None, spec, body={"counts": {"reads": 7}}) as (argv, _):
            with self.assertRaisesRegex(SystemExit, "sub-document"):
                ratchet.main(["check", *argv])

    def test_a_metric_naming_an_undeclared_source_refuses(self):
        spec = floors(reads=("at_least", 1))
        spec["metrics"]["ghost:counts.reads"] = {"direction": "at_least", "floor": 1}
        with scratch({"reads": 5}, spec) as (argv, _):
            with self.assertRaisesRegex(SystemExit, "not declared"):
                ratchet.main(["check", *argv])

    def test_a_metric_with_no_source_prefix_refuses(self):
        spec = floors(reads=("at_least", 1))
        spec["metrics"]["counts.reads"] = {"direction": "at_least", "floor": 1}
        with scratch({"reads": 5}, spec) as (argv, _):
            with self.assertRaisesRegex(SystemExit, "must be"):
                ratchet.main(["check", *argv])


class MalformedInputTests(unittest.TestCase):
    def test_a_typo_in_a_direction_refuses_rather_than_passing(self):
        """A misspelled direction must not read as "nothing to check"."""
        spec = floors(reads=("at_lesat", 1))
        with self.assertRaisesRegex(SystemExit, "expected one of"):
            ratchet.evaluate(sources({"reads": 1}), spec)

    def test_a_floors_file_missing_a_required_key_refuses(self):
        with scratch({"reads": 1}, {"schema": 1, "metrics": {}}) as (argv, _):
            with self.assertRaisesRegex(SystemExit, "has no 'sources'"):
                ratchet.main(["check", *argv])

    def test_an_empty_metric_set_refuses_rather_than_passing_everything(self):
        with scratch({"reads": 1}, {"schema": 1, "measured_at": {}, "metrics": {},
                                    "sources": {"s": "m.json"}}) as (argv, _):
            with self.assertRaisesRegex(SystemExit, "tracks no metrics"):
                ratchet.main(["check", *argv])

    def test_a_missing_file_refuses_with_its_path(self):
        with self.assertRaisesRegex(SystemExit, "not found"):
            ratchet.main(["check", "--floors", "/nonexistent/floors.json"])


class CommittedFloorsTests(unittest.TestCase):
    """The committed floors must themselves be well-formed and honest."""

    def setUp(self):
        self.floors = json.loads(FLOORS_PATH.read_text())

    def test_every_metric_declares_a_known_direction_and_integer_floor(self):
        self.assertTrue(self.floors["metrics"], "no metrics tracked")
        for name, spec in self.floors["metrics"].items():
            with self.subTest(name):
                self.assertIn(spec["direction"], ratchet.DIRECTIONS)
                self.assertIsInstance(spec["floor"], int)
                self.assertNotIsInstance(spec["floor"], bool)

    def test_the_seeded_floors_match_the_committed_join_report(self):
        """The floors claim to come from `docs/reference/catalog-hydrated-join.md`.

        That claim is checkable, so check it: a seed transcribed by hand from
        a report is exactly the kind of number that drifts from its source
        without anyone noticing.
        """
        report = (pathlib.Path(__file__).resolve().parents[2]
                  / "docs" / "reference" / "catalog-hydrated-join.md").read_text()
        rows = dict(self._table_rows(report))
        for metric, key in (
            ("join:counts.catalog_ordinary_entries", "Ordinary catalog entries"),
            ("join:counts.implementation.blocked_generated_reader_refusal", "`blocked_generated_reader_refusal`"),
            ("join:counts.implementation.generated_reader_declaration_option_gated", "`generated_reader_declaration_option_gated`"),
            ("join:counts.implementation.generated_reader_declaration_unobserved", "`generated_reader_declaration_unobserved`"),
            ("join:counts.implementation.source_row_not_yet_consumed", "`source_row_not_yet_consumed`"),
            ("join:counts.writer_implementation.writer_not_declared", "`writer_not_declared`"),
            ("join:counts.write_parity.native_writable_entries", "Entries ExifTool writes directly"),
            ("join:counts.write_parity.native_writable_unique_case_insensitive_names", "Distinct case-insensitive writable names"),
            ("join:counts.write_parity.generated_writer_declarations", "With a generated writer declaration"),
        ):
            with self.subTest(metric):
                self.assertIn(key, rows, f"{key!r} not found in the committed report")
                self.assertEqual(self.floors["metrics"][metric]["floor"], rows[key])

    @staticmethod
    def _table_rows(markdown):
        for line in markdown.splitlines():
            if not line.startswith("|"):
                continue
            cells = [cell.strip() for cell in line.strip("|").split("|")]
            if len(cells) == 2 and cells[1].isdigit():
                yield cells[0], int(cells[1])


if __name__ == "__main__":
    unittest.main()
