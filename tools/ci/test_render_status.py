"""The docs status page (/status/) must match the committed measurements.

`tools/docs/render_status.py` renders `docs/status/index.md` and
`docs/public/measurements/status.json` from committed sources only. These
tests keep it honest under plain `python3`, with no PyYAML:

* the page on disk is exactly what the sources render to today, so a snapshot,
  floor, coverage report or rehearsal change without a page refresh fails the
  lint job's unittest step and names the refresh command;
* a missing source or a reworded Markdown row is an error, never a zero.

The one input these tests cannot recount without PyYAML is the tag-definition
count from the `oxidex-tags-*` YAML databases. They take it from the committed
status.json. `uv run tools/docs/render_status.py --check`, a separate lint
step, recounts it.
"""
import importlib.util
import json
import pathlib
import shutil
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "render_status", ROOT / "tools" / "docs" / "render_status.py")
render_status = importlib.util.module_from_spec(spec)
spec.loader.exec_module(render_status)


def committed_definitions():
    status = json.loads((ROOT / render_status.STATUS_JSON).read_text(encoding="utf-8"))
    catalog = status["catalog"]
    return {"definitions": catalog["tag_definitions"], "tables": catalog["tag_tables"],
            "crates": catalog["tag_crates"]}


class StatusPageIsCurrent(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.outputs = render_status.render(ROOT, committed_definitions())

    def test_committed_outputs_match_their_sources(self):
        stale = render_status.stale_outputs(ROOT, self.outputs)
        self.assertEqual(
            stale, [],
            f"the docs status page is stale: {', '.join(map(str, stale))}. "
            f"Run `{render_status.REFRESH}` and commit the result.")

    def test_render_is_deterministic(self):
        again = render_status.render(ROOT, committed_definitions())
        self.assertEqual(again, self.outputs)

    def test_every_meter_is_backed_by_status_json(self):
        page = self.outputs[render_status.PAGE]
        status = json.loads(self.outputs[render_status.STATUS_JSON])
        self.assertIn(':value="%d" :total="%d"' % (
            status["read"]["observed_matched_read"], status["catalog"]["catalog_entries"]), page)
        self.assertFalse(status["generated_share"]["stale"])
        self.assertNotIn("STALE", page)
        self.assertIn(':value="%s"' % status["generated_share"]["share"].rstrip("%"), page)

    def test_three_read_buckets_partition_the_catalog(self):
        status = json.loads(self.outputs[render_status.STATUS_JSON])
        read = status["read"]
        self.assertEqual(
            read["observed_matched_read"] + read["native_read_not_matched"]
            + read["not_observed_yet"], status["catalog"]["catalog_entries"])


class SourcesFailLoudly(unittest.TestCase):
    """A moved or reworded source must refuse, never render a zero."""

    def copy_tree(self, tmp):
        root = pathlib.Path(tmp)
        for rel in (render_status.OBSERVED, render_status.JOIN, render_status.FLOORS,
                    render_status.RATCHET, render_status.COVERAGE, render_status.SESSION,
                    render_status.HELPERS_RS, render_status.ARTIFACTS, render_status.REHEARSAL,
                    render_status.PLAN, render_status.PROGRESS, render_status.DESIGN,
                    render_status.PIN, render_status.GENSHARE):
            (root / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / rel, root / rel)
        # artifacts.py validates against the committed hub files it lists.
        for rel in ("src/exiftool_tables/binary/mod.rs", "src/exiftool_tables/ifd/mod.rs"):
            (root / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / rel, root / rel)
        return root

    def test_missing_source_is_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.copy_tree(tmp)
            (root / render_status.COVERAGE).unlink()
            with self.assertRaisesRegex(render_status.SourceError,
                                        "source not found: " + render_status.COVERAGE):
                render_status.render(root, committed_definitions())

    def test_floor_change_without_refresh_is_stale(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.copy_tree(tmp)
            for rel in (render_status.PAGE, render_status.STATUS_JSON):
                (root / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(ROOT / rel, root / rel)
            floors = root / render_status.FLOORS
            doc = json.loads(floors.read_text(encoding="utf-8"))
            doc["measured_at"]["commit"] = "0123456789ab"
            floors.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            outputs = render_status.render(root, committed_definitions())
            self.assertEqual(sorted(render_status.stale_outputs(root, outputs)),
                             sorted([render_status.PAGE, render_status.STATUS_JSON]))

    def test_reworded_plan_row_is_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.copy_tree(tmp)
            plan = root / render_status.PLAN
            plan.write_text(plan.read_text(encoding="utf-8").replace(
                "| Generated share of correct output |", "| Generated share |"),
                encoding="utf-8")
            with self.assertRaisesRegex(render_status.SourceError, "matched 0 times"):
                render_status.render(root, committed_definitions())

    def test_plan_row_that_disagrees_with_the_result_is_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.copy_tree(tmp)
            result = json.loads((root / render_status.GENSHARE).read_text(encoding="utf-8"))
            plan = root / render_status.PLAN
            plan.write_text(plan.read_text(encoding="utf-8").replace(
                "**" + result["current"]["all_generated"]["share"] + "**", "**1.00%**"),
                encoding="utf-8")
            with self.assertRaisesRegex(render_status.SourceError, "does not quote"):
                render_status.render(root, committed_definitions())

    def test_missing_generated_share_result_is_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.copy_tree(tmp)
            (root / render_status.GENSHARE).unlink()
            with self.assertRaisesRegex(render_status.SourceError,
                                        "source not found: " + render_status.GENSHARE):
                render_status.render(root, committed_definitions())

    def test_missing_count_is_an_error(self):
        doc = {"counts": {"observed_read": {}}}
        with self.assertRaisesRegex(render_status.SourceError, "missing key"):
            render_status.count(doc, "counts.observed_read.observed_matched_read", "x.json")

    def test_non_integer_count_is_an_error(self):
        with self.assertRaisesRegex(render_status.SourceError, "expected an integer"):
            render_status.count({"a": "12"}, "a", "x.json")


if __name__ == "__main__":
    unittest.main()
