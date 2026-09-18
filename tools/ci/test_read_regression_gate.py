"""The read regression gate's verdicts: lost reads, new credit, degraded and malformed inputs."""
import contextlib
import importlib.util
import io
import json
import pathlib
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "read_regression_gate", pathlib.Path(__file__).with_name("read_regression_gate.py"))
gate = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = gate  # dataclasses resolve their module by name
spec.loader.exec_module(gate)

EXIF = "Image::ExifTool::Exif::Main"
MAKE, MODEL, ORIENTATION, SOFTWARE = (EXIF, "271", 0), (EXIF, "272", 0), (EXIF, "274", 0), (EXIF, "305", 0)
NAMES = {MAKE: "Make", MODEL: "Model", ORIENTATION: "Orientation", SOFTWARE: "Software"}


def entry(identity, state):
    return {"identity": {"table": identity[0], "raw_key": identity[1], "variant_index": identity[2]},
            "catalog": {"name": NAMES[identity]}, "observed_read": state}


def snapshot(read=(MAKE, MODEL), native_only=(ORIENTATION,), unobserved=(SOFTWARE,), corpus_files=3):
    entries = ([entry(i, "observed_matched_read") for i in read]
               + [entry(i, "native_read_not_matched") for i in native_only]
               + [entry(i, "not_observed_yet") for i in unobserved])
    states = {}
    for item in entries:
        states[item["observed_read"]] = states.get(item["observed_read"], 0) + 1
    return {"schema": gate.SNAPSHOT_SCHEMA, "native_evidence": {"source_commit": "c" * 40},
            "observed_join": {"inputs": {"exiftool_version": "13.59"}, "entries": entries,
                              "counts": {"observed_read": states, "corpus_read_attribution": {
                                  "credited_catalog_entries": len(read),
                                  "metric_c": {"corpus_files": corpus_files}}}}}


def published(**kwargs):
    return gate.published_reads(snapshot(**kwargs), "13.59")


class VerdictTests(unittest.TestCase):
    def test_every_published_read_still_credited_passes(self):
        verdict = gate.evaluate(published(), {MAKE, MODEL}, {MAKE, MODEL, ORIENTATION}, [], 3)
        self.assertEqual((verdict.status, verdict.exit_code, verdict.lost), ("PASS", 0, []))

    def test_lost_entry_refuses_and_names_it(self):
        pub = published()
        verdict = gate.evaluate(pub, {MAKE}, {MAKE, MODEL, ORIENTATION}, [], 3)
        self.assertEqual((verdict.status, verdict.exit_code), ("REGRESSION", 1))
        self.assertEqual(verdict.lost, [MODEL])
        text = gate.render(pub, verdict)
        self.assertIn(f"LOST {EXIF} 272 v0 (Model)", text)
        self.assertTrue(text.endswith("verdict: REGRESSION"))

    def test_newly_credited_is_information_only(self):
        pub = published()
        verdict = gate.evaluate(pub, {MAKE, MODEL, ORIENTATION}, {MAKE, MODEL, ORIENTATION}, [], 3)
        self.assertEqual((verdict.status, verdict.exit_code), ("PASS", 0))
        self.assertEqual(verdict.newly_credited, [ORIENTATION])
        self.assertIn(f"info: NEWLY-CREDITED {EXIF} 274 v0 (Orientation)", gate.render(pub, verdict))

    def test_timeout_refuses_as_degraded_never_as_regression(self):
        # Even with every read "lost", a timed-out public read means the
        # measurement is degraded: it must not be reported as a regression.
        verdict = gate.evaluate(published(), set(), {MAKE, MODEL}, [("a.jpg", "raw", -1)], 3)
        self.assertEqual((verdict.status, verdict.exit_code), ("REFUSED", 2))
        self.assertIn("measurement degraded", verdict.reasons[0])
        self.assertNotIn("REGRESSION", gate.render(published(), verdict))

    def test_crash_or_unparseable_public_read_is_a_regression(self):
        for code in (101, 0):
            verdict = gate.evaluate(published(), {MAKE, MODEL}, {MAKE, MODEL}, [("a.jpg", "print", code)], 3)
            self.assertEqual((verdict.status, verdict.exit_code), ("REGRESSION", 1), code)

    def test_row_exiftool_no_longer_reads_refuses_the_measurement(self):
        verdict = gate.evaluate(published(), {MAKE}, {MAKE}, [], 3)
        self.assertEqual((verdict.status, verdict.exit_code), ("REFUSED", 2))
        self.assertEqual(verdict.lost, [MODEL])

    def test_different_corpus_refuses(self):
        verdict = gate.evaluate(published(), {MAKE, MODEL}, {MAKE, MODEL}, [], 4)
        self.assertEqual((verdict.status, verdict.exit_code), ("REFUSED", 2))

    def test_credit_outside_the_catalog_refuses(self):
        stray = ("Image::ExifTool::Nope::Main", "1", 0)
        verdict = gate.evaluate(published(), {MAKE, MODEL, stray}, {MAKE, MODEL, stray}, [], 3)
        self.assertEqual(verdict.status, "REFUSED")


class SnapshotTests(unittest.TestCase):
    def assertRefused(self, document, pattern):
        with self.assertRaisesRegex(gate.Refused, pattern):
            gate.published_reads(document, "13.59")

    def test_empty_or_malformed_snapshot_refuses(self):
        self.assertRefused({}, "schema")
        self.assertRefused(None, "schema")
        empty = snapshot()
        empty["observed_join"]["entries"] = []
        self.assertRefused(empty, "no catalog entries")
        self.assertRefused(snapshot(read=()), "counts disagree|proves no reads")
        bad = snapshot()
        bad["observed_join"]["entries"][0]["identity"]["variant_index"] = "0"
        self.assertRefused(bad, "malformed")
        missing = snapshot()
        del missing["observed_join"]["entries"][0]["identity"]
        self.assertRefused(missing, "no identity")

    def test_snapshot_with_no_reads_refuses_rather_than_passing_vacuously(self):
        document = snapshot(read=())
        document["observed_join"]["counts"]["corpus_read_attribution"]["credited_catalog_entries"] = 0
        self.assertRefused(document, "proves no reads")

    def test_inconsistent_counts_duplicates_and_other_pins_refuse(self):
        wrong = snapshot()
        wrong["observed_join"]["counts"]["observed_read"]["observed_matched_read"] = 5
        self.assertRefused(wrong, "counts disagree")
        dup = snapshot()
        dup["observed_join"]["entries"].append(dup["observed_join"]["entries"][0])
        self.assertRefused(dup, "repeats identity")
        with self.assertRaisesRegex(gate.Refused, "pinned ExifTool"):
            gate.published_reads(snapshot(), "13.60")
        no_corpus = snapshot()
        no_corpus["observed_join"]["counts"]["corpus_read_attribution"]["metric_c"] = {}
        self.assertRefused(no_corpus, "corpus size")

    def test_cli_refuses_malformed_inputs_with_the_measurement_exit_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths = {}
            for name, body in (("snapshot", "{}"), ("catalog", "{}"), ("receipt", "{}")):
                paths[name] = pathlib.Path(tmp) / f"{name}.json"
                paths[name].write_text(body)
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                code = gate.main(["--receipt", str(paths["receipt"]), "--snapshot", str(paths["snapshot"]),
                                  "--catalog", str(paths["catalog"])])
            self.assertEqual(code, gate.EXIT_REFUSED)
            self.assertIn("REFUSED", out.getvalue())
            (pathlib.Path(tmp) / "snapshot.json").write_text("not json")
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(gate.main(["--receipt", str(paths["receipt"]), "--snapshot", str(paths["snapshot"]),
                                            "--catalog", str(paths["catalog"])]), gate.EXIT_REFUSED)


class PublicFailureTests(unittest.TestCase):
    def test_failures_are_reclassified_from_the_transcripts(self):
        def row(name, mode, code, stdout):
            return {"file": name, "mode": mode, "oxidex": {"returncode": code, "stdout_hex": stdout.hex()}}
        good = json.dumps([{"SourceFile": "a", "IFD0:Make": "X"}]).encode()
        receipt = {"observations": [row("a.jpg", "print", 0, good), row("a.jpg", "raw", -1, b""),
                                    row("b.jpg", "print", 101, b""), row("b.jpg", "raw", 0, b"garbage")]}
        sys.path.insert(0, str(gate.ROOT / "tools/exiftool-tables"))
        self.assertEqual(gate._public_failures(receipt),
                         [("a.jpg", "raw", -1), ("b.jpg", "print", 101), ("b.jpg", "raw", 0)])


if __name__ == "__main__":
    unittest.main()
