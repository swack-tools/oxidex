import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import runtime_evidence_inputs as runtime_inputs
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
import verify_quicktime_reader as read_verifier

PATH = Path(__file__).with_name("join_catalog_hydrated.py")
spec = importlib.util.spec_from_file_location("join_catalog_hydrated", PATH)
join = importlib.util.module_from_spec(spec)
spec.loader.exec_module(join)

TABLE = "Image::ExifTool::QuickTime::ItemList"
SOURCE = {"Image/ExifTool.pm": {"library_relative_path": "Image/ExifTool.pm", "sha256": "a"}}


def entry(name="Title", raw="titl", variant=0):
    return {"table": TABLE, "raw_key": raw, "variant_index": variant, "name": name,
            "normalized_name": name.lower(), "groups": {"0": "QuickTime", "1": "ItemList", "2": "Audio"},
            "no_lookup": False, "unknown": False}


def catalog(entries):
    names = sorted({item["normalized_name"] for item in entries})
    return {"schema": join.CATALOG_SCHEMA, "exiftool_version": "13.59", "entries": entries,
            "unique_names": names, "producer": {"sources": copy.deepcopy(SOURCE)},
            "counts": {"catalog_total_tag_entries": len(entries),
                       "distinct_case_insensitive_entry_names": len(names), "catalog_unique_tag_names": len(names)}}


def hydrated(tags, sources=SOURCE, total=1):
    return {"exiftool_version": "13.59", "hydrated_layouts": {
        "catalog_counts": {"total_tag_entries": total}, "source_provenance": {"sources": copy.deepcopy(sources)},
        "tables": {TABLE: {"full_name": TABLE, "tags": tags}}}}


def observation_rows(fixture_name, fixture_bytes, expected, actual):
    native_json = json.dumps([expected], sort_keys=True)
    oxidex_json = json.dumps([actual], sort_keys=True)
    return [{"fixture": fixture_name, "mode": mode,
             "fixture_sha256": hashlib.sha256(fixture_bytes).hexdigest(),
             "native_json": native_json,
             "native_json_sha256": hashlib.sha256(native_json.encode()).hexdigest(),
             "oxidex_json": oxidex_json,
             "oxidex_json_sha256": hashlib.sha256(oxidex_json.encode()).hexdigest(),
             "expected": expected, "actual": actual, "matched": expected == actual}
            for mode in ("print", "no-print-conv")]


class CatalogHydratedJoinTests(unittest.TestCase):
    def test_writer_replay_requires_exact_source_bound_artifacts_and_conserves_rows(self):
        source = json.dumps({"exiftool_version": "13.59"}).encode()
        row = {"module": "Exif", "table": "Main", "full_name": "Image::ExifTool::Exif::Main",
               "raw_tag_id": 315, "name": "Artist", "group0": "EXIF", "group1": "IFD0",
               "write_group": "IFD0", "source_control_sha256": "a" * 64, "semantics_sha256": "b" * 64,
               "state": "current"}
        current = {join.public_migration._entry_key(row): SimpleNamespace(
            full_name=row["full_name"], name=row["name"], source_control_sha256=row["source_control_sha256"],
            semantics_sha256=row["semantics_sha256"])}
        final = {"recipes": [{"full_name": row["full_name"], "raw_tag_id": 315, "name": "Artist", "physical_write_group": "IFD0"}], "registry": {"formats": ("TIFF", "JPEG")}}
        public = {"source": {"capture": "bound"}, "entries": [row]}
        with patch.object(join.final_scalar_stage, "generate", return_value=("final-rust", final)), \
             patch.object(join.public_migration, "compile_current", return_value=({"capture": "bound"}, current)), \
             patch.object(join.public_migration, "validate_ledger"), \
             patch.object(join.public_migration, "render_rust", return_value="public-rust"):
            rows = join.writer_implementation(source, json.loads(json.dumps(final)), "final-rust", public, "public-rust")
            self.assertEqual(rows, {(row["full_name"], "315", 0): {"name": "Artist", "write_group": "IFD0", "semantics_sha256": "b" * 64}})
            with self.assertRaisesRegex(ValueError, "final artifacts"):
                join.writer_implementation(source, json.loads(json.dumps(final)), "tampered", public, "public-rust")
            bad = copy.deepcopy(public); bad["entries"][0]["name"] = "Alias"
            with self.assertRaisesRegex(ValueError, "identity"):
                join.writer_implementation(source, json.loads(json.dumps(final)), "final-rust", bad, "public-rust")
    def replayed_quicktime_facts(self):
        source = json.loads((PATH.parent / "fixtures/quicktime_source_13_59.json").read_text())
        # This test-only bounded input is a fresh capture fixture: preserve all
        # source rows and protocol facts while binding it to this checkout's
        # dump tool so `report()` exercises its complete provenance contract.
        source["capture_scope"]["dump_tool_sha256"] = hashlib.sha256(
            (PATH.parent / "dump_tables.pl").read_bytes()).hexdigest()
        raw = json.dumps(source, sort_keys=True).encode()
        ledger = join.quicktime_specs.compile_document(source)
        capabilities = join.quicktime_selector.report(raw)
        rust = join.quicktime_specs.render_rust(ledger)
        return raw, source, ledger, capabilities, rust

    @staticmethod
    def quicktime_digests(raw, ledger, capabilities, rust):
        return {"source_sha256": hashlib.sha256(raw).hexdigest(),
                "ledger_sha256": hashlib.sha256(json.dumps(ledger).encode()).hexdigest(),
                "capabilities_sha256": hashlib.sha256(json.dumps(capabilities).encode()).hexdigest(),
                "rust_sha256": hashlib.sha256(rust.encode()).hexdigest()}

    def test_quicktime_replay_requires_complete_source_ledger_capabilities_and_rust(self):
        raw, _, ledger, capabilities, rust = self.replayed_quicktime_facts()
        rows = join.quicktime_implementation(ledger, capabilities, raw, rust)
        self.assertGreater(len(rows), 0)
        self.assertTrue(any(row["generated"] for row in rows.values()))

    def test_quicktime_replay_rejects_ledger_capability_and_rust_tampering(self):
        raw, _, ledger, capabilities, rust = self.replayed_quicktime_facts()
        changed_ledger = copy.deepcopy(ledger)
        changed_ledger["ledger"][0]["generated"] = not changed_ledger["ledger"][0]["generated"]
        changed_reasons = copy.deepcopy(ledger)
        changed_reasons["ledger"][0]["reasons"] = ["tampered"]
        changed_specs = copy.deepcopy(ledger)
        changed_specs["specs"][0]["name"] = "Tampered"
        changed_provenance = copy.deepcopy(ledger)
        changed_provenance["protocol"]["processor_provenance"]["source_sha256"] = "0" * 64
        changed_capabilities = copy.deepcopy(capabilities)
        changed_capabilities["families"][0]["records"][0]["reasons"] = ["tampered"]
        for bad_ledger, bad_capabilities, bad_rust in (
                (changed_ledger, capabilities, rust), (changed_reasons, capabilities, rust),
                (changed_specs, capabilities, rust), (changed_provenance, capabilities, rust),
                (ledger, changed_capabilities, rust), (ledger, capabilities, rust + "// tampered\n")):
            with self.subTest():
                with self.assertRaisesRegex(ValueError, "differ(?:s)? from (?:complete )?bounded-source replay"):
                    join.quicktime_implementation(bad_ledger, bad_capabilities, raw, bad_rust)

    def test_quicktime_replay_rejects_missing_paired_inputs_and_pin_disagreement(self):
        raw, _, ledger, capabilities, rust = self.replayed_quicktime_facts()
        with self.assertRaisesRegex(ValueError, "artifact together"):
            join.quicktime_implementation(ledger, capabilities, None, rust)
        changed_source = json.loads(raw)
        changed_source["exiftool_version"] = "13.58"
        with self.assertRaisesRegex(ValueError, "repository pin"):
            join.quicktime_implementation(ledger, capabilities, json.dumps(changed_source).encode(), rust)
        stale_catalog = catalog([entry()])
        stale_hydrated = hydrated({"titl": {"Name": "Title"}})
        stale_catalog["exiftool_version"] = stale_hydrated["exiftool_version"] = "13.58"
        with self.assertRaisesRegex(ValueError, "repository pin"):
            join.build(stale_catalog, stale_hydrated, "c", "h", ledger, capabilities, raw, rust,
                       self.quicktime_digests(raw, ledger, capabilities, rust))

    def test_quicktime_projector_resolves_inherited_groups_and_matching_variant_index(self):
        shared = {
            "defaults": {"kind": "HASH", "properties": {"0": "QuickTime", "1": "ItemList", "2": "Audio"}},
            "override": {"kind": "HASH", "properties": {"0": "QuickTime", "1": "ItemList", "2": "Author"}},
        }
        table = {"GROUPS": {"__ref": "HASH", "object_id": "defaults"}}
        row = {"Name": "AlbumArtist", "TagID": "aART",
               "Table": {"table_full_names": ["Image::ExifTool::QuickTime::ItemList"]},
               "Groups": {"__ref": "HASH", "object_id": "override"},
               "_extra_properties": {"GotGroups": "1", "Index": "1"}}
        projected = join.quicktime_selector_projection("Image::ExifTool::QuickTime::ItemList", "aART", row,
                                                       table_meta=table, shared_references=shared, variant_path=(1,))
        self.assertEqual(projected, {"Name": "AlbumArtist", "Groups": {"2": "Author"}})
        with self.assertRaisesRegex(ValueError, "Index"):
            join.quicktime_selector_projection("Image::ExifTool::QuickTime::ItemList", "aART", row,
                                               table_meta=table, shared_references=shared, variant_path=(2,))
        with self.assertRaisesRegex(ValueError, "reference"):
            join.quicktime_selector_projection("Image::ExifTool::QuickTime::ItemList", "aART",
                                               {"Groups": {"__ref": "HASH", "object_id": "missing"}},
                                               table_meta=table, shared_references=shared, variant_path=())

    def test_quicktime_projector_removes_bounded_inherited_groups(self):
        defaults = {"0": "QuickTime", "1": "ItemList", "2": "Audio"}
        projected = join.quicktime_selector_projection(
            "Image::ExifTool::QuickTime::ItemList", "titl",
            {"Name": "Title", "Groups": defaults}, table_meta={"GROUPS": defaults})
        self.assertEqual(projected, {"Name": "Title"})

    def test_exact_coordinate_variant_name_and_hash_join(self):
        result = join.build(catalog([entry("Title", "titl", 0), entry("Alternate", "titl", 1)]),
                            hydrated({"titl": {"_variants": [{"Name": "Title"}, {"Name": "Alternate"}]}}, total=2), "catalog", "hydrated")
        self.assertEqual(result["counts"]["status"], {"source_row_joined": 2})
        self.assertEqual(result["entries"][0]["source"]["state"], "joined")
        self.assertRegex(result["entries"][0]["source"]["row_sha256"], r"^[0-9a-f]{64}$")

    def test_name_conflict_and_absence_are_truthful(self):
        result = join.build(catalog([entry("Title"), entry("Missing", "miss")]),
                            hydrated({"titl": {"Name": "Different"}}, total=2), "c", "h")
        self.assertEqual(result["counts"]["status"], {"source_row_absent": 1, "source_row_name_conflict": 1})
        self.assertEqual({row["catalog"]["name"]: row["source"]["state"] for row in result["entries"]},
                         {"Title": "conflict", "Missing": "absent"})

    def test_quicktime_exact_identity_joins_generated_and_refused_selector_facts(self):
        raw, bounded, ledger, capabilities, rust = self.replayed_quicktime_facts()
        source = bounded["modules"]["QuickTime"]["tables"]["ItemList"]["tags"]["titl"]
        result = join.build(catalog([entry()]), hydrated({"titl": source}), "c", "h", ledger, capabilities, raw, rust,
                            self.quicktime_digests(raw, ledger, capabilities, rust))
        self.assertEqual(result["entries"][0]["source_derived_implementation"],
                         "generated_reader_declaration_unobserved")

    def test_authenticated_observed_read_requires_exact_generated_identity(self):
        raw, bounded, ledger, capabilities, rust = self.replayed_quicktime_facts()
        generated = next(row for row in ledger["ledger"]
                         if row["generated"] and row["identity"]["raw_key"] == "titl")
        digests = self.quicktime_digests(raw, ledger, capabilities, rust)
        fixture = read_verifier.baseline.fixture(b"titl", 1, b"Observed title")
        observations = observation_rows("text.m4a", fixture,
                                        {"ItemList:Title": "Observed title"},
                                        {"ItemList:Title": "Observed title"})
        with patch.object(read_verifier, "cases", return_value={"text.m4a": fixture}):
            occurrences, identities, manifest = read_verifier.observation_evidence(observations, ledger["specs"])
            evidence = {"schema": join.QUICKTIME_READ_EVIDENCE_SCHEMA, "inputs": dict(sorted(digests.items())),
                        "producer": {"source_dirty": False, "source_commit": "a" * 40,
                                     "source_fingerprint": join.baseline.source_fingerprint(join.quicktime_selector.ROOT), "runtime_artifact_sha256": "c" * 64,
                                     "runtime_input_manifest_sha256": join.runtime_inputs.runtime_input_manifest(join.quicktime_selector.ROOT),
                                     "fixture_manifest_sha256": manifest, "pin": "13.59"},
                        "observations": observations, "matched_occurrences": occurrences,
                        "observed_identities": identities}
            source = bounded["modules"]["QuickTime"]["tables"]["ItemList"]["tags"]["titl"]
            result = join.build(catalog([entry()]), hydrated({"titl": source}), "c", "h", ledger, capabilities, raw, rust,
                                digests, evidence)
            self.assertEqual(result["entries"][0]["observed_read"], "observed_matched_read")
            self.assertIn("quicktime_read_evidence", result["inputs"])
            with patch.object(join.runtime_inputs, "runtime_input_manifest", return_value="d" * 64):
                with self.assertRaisesRegex(ValueError, "runtime input manifest"):
                    join.quicktime_observed_reads(evidence, digests, ledger["specs"])
            for key, value in (("raw_key", "cpil"), ("source_sha256", "f" * 64)):
                changed = copy.deepcopy(evidence)
                changed["observed_identities"][0]["source_identity"][key] = value
                with self.assertRaisesRegex(ValueError, "claims differ"):
                    join.quicktime_observed_reads(changed, digests, ledger["specs"])
            changed = copy.deepcopy(evidence)
            changed["observations"][0]["actual"]["ItemList:Title"] = "Wrong"
            with self.assertRaisesRegex(ValueError, "projection claim"):
                join.quicktime_observed_reads(changed, digests, ledger["specs"])
            changed = copy.deepcopy(evidence)
            changed["observations"][0]["expected"] = {"ItemList:Title": "FORGED"}
            changed["observations"][0]["actual"] = {"ItemList:Title": "FORGED"}
            with self.assertRaisesRegex(ValueError, "projection claim"):
                join.quicktime_observed_reads(changed, digests, ledger["specs"])
            changed = copy.deepcopy(evidence)
            wrong_json = json.dumps([{"ItemList:Title": "Wrong"}])
            changed["observations"][0]["oxidex_json"] = wrong_json
            with self.assertRaisesRegex(ValueError, "transcript hash"):
                join.quicktime_observed_reads(changed, digests, ledger["specs"])
            changed = copy.deepcopy(evidence)
            changed["observations"][0]["oxidex_json"] = wrong_json
            changed["observations"][0]["oxidex_json_sha256"] = hashlib.sha256(wrong_json.encode()).hexdigest()
            changed["observations"][0]["actual"] = {"ItemList:Title": "Wrong"}
            with self.assertRaisesRegex(ValueError, "matched flag"):
                join.quicktime_observed_reads(changed, digests, ledger["specs"])
            changed = copy.deepcopy(evidence)
            changed["observations"].pop()
            with self.assertRaisesRegex(ValueError, "incomplete"):
                join.quicktime_observed_reads(changed, digests, ledger["specs"])
            changed = copy.deepcopy(evidence)
            changed["observations"][0]["fixture_sha256"] = "f" * 64
            with self.assertRaisesRegex(ValueError, "fixture bytes"):
                join.quicktime_observed_reads(changed, digests, ledger["specs"])

    def test_negative_fixture_cannot_supply_observed_read_identity(self):
        _, _, ledger, _, _ = self.replayed_quicktime_facts()
        fixture = read_verifier.baseline.fixture(b"zzzz", 1, b"unknown")
        rows = observation_rows("unknown.m4a", fixture, {}, {})
        with patch.object(read_verifier, "cases", return_value={"unknown.m4a": fixture}):
            occurrences, identities, _ = read_verifier.observation_evidence(rows, ledger["specs"])
        self.assertEqual(occurrences, [])
        self.assertEqual(identities, [])

    def test_historical_or_unbound_observed_read_evidence_is_refused(self):
        raw, _, ledger, capabilities, rust = self.replayed_quicktime_facts()
        with self.assertRaisesRegex(ValueError, "historical"):
            join.quicktime_observed_reads({"instrument": "historical"}, self.quicktime_digests(raw, ledger, capabilities, rust))

    def test_quicktime_name_or_hash_collision_cannot_consume_a_source_row(self):
        raw, bounded, ledger, capabilities, rust = self.replayed_quicktime_facts()
        source = copy.deepcopy(bounded["modules"]["QuickTime"]["tables"]["ItemList"]["tags"]["titl"])
        source["Name"] = "Other"
        result = join.build(catalog([entry()]), hydrated({"titl": source}), "c", "h", ledger, capabilities, raw, rust,
                            self.quicktime_digests(raw, ledger, capabilities, rust))
        self.assertEqual(result["entries"][0]["source_derived_implementation"], "source_row_not_yet_consumed")
        self.assertEqual(result["entries"][0]["observed_read"], "not_observed_yet")

    def test_catalog_name_conflict_cannot_inherit_generated_source_support(self):
        raw, bounded, ledger, capabilities, rust = self.replayed_quicktime_facts()
        source = bounded["modules"]["QuickTime"]["tables"]["ItemList"]["tags"]["titl"]
        result = join.build(catalog([entry("DifferentCatalogName")]), hydrated({"titl": source}),
                            "c", "h", ledger, capabilities, raw, rust,
                            self.quicktime_digests(raw, ledger, capabilities, rust))
        record = result["entries"][0]
        self.assertEqual(record["source"]["state"], "conflict")
        self.assertEqual(record["source_derived_implementation"], "source_row_not_yet_consumed")

    def test_rejects_malformed_native_denominators_and_names(self):
        bad = catalog([entry()])
        bad["counts"]["catalog_total_tag_entries"] = 2
        with self.assertRaisesRegex(ValueError, "catalog_total_tag_entries"):
            join.build(bad, hydrated({"titl": {"Name": "Title"}}), "c", "h")
        bad = catalog([entry()])
        bad["unique_names"] = []
        with self.assertRaisesRegex(ValueError, "unique_names conservation"):
            join.build(bad, hydrated({"titl": {"Name": "Title"}}), "c", "h")

    def test_rejects_missing_or_mismatched_source_manifest(self):
        with self.assertRaisesRegex(ValueError, "absent from hydrated manifest"):
            join.build(catalog([entry()]), hydrated({"titl": {"Name": "Title"}}, {}), "c", "h")
        with self.assertRaisesRegex(ValueError, "source provenance mismatch"):
            join.build(catalog([entry()]), hydrated({"titl": {"Name": "Title"}},
                                                     {"Image/ExifTool.pm": {"sha256": "wrong"}}), "c", "h")

    def test_cli_check_compares_existing_outputs_and_never_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            catalog_path, hydrated_path = root / "catalog.json", root / "hydrated.json"
            source_path, ledger_path = root / "quicktime-source.json", root / "quicktime-ledger.json"
            capabilities_path, rust_path = root / "quicktime-capabilities.json", root / "quicktime.rs"
            keys_ledger_path, keys_rust_path = root / "keys-ledger.json", root / "keys.rs"
            output, report = root / "join.json", root / "join.md"
            catalog_path.write_text(json.dumps(catalog([entry()])))
            hydrated_path.write_text(json.dumps(hydrated({"titl": {"Name": "Title"}})))
            raw, _, ledger, capabilities, rust = self.replayed_quicktime_facts()
            source_path.write_bytes(raw)
            ledger_path.write_text(json.dumps(ledger))
            capabilities_path.write_text(json.dumps(capabilities))
            rust_path.write_text(rust)
            keys = join.quicktime_keys_specs.compile_document(json.loads(raw)); keys_ledger_path.write_text(json.dumps(keys)); keys_rust_path.write_text(join.quicktime_keys_specs.render_rust(keys))
            command = ["python3", str(PATH), "--catalog", str(catalog_path), "--hydrated", str(hydrated_path),
                       "--quicktime-bounded-source", str(source_path), "--quicktime-itemlist-ledger", str(ledger_path),
                       "--quicktime-source-capabilities", str(capabilities_path), "--quicktime-itemlist-rust", str(rust_path),
                       "--quicktime-keys-ledger", str(keys_ledger_path), "--quicktime-keys-rust", str(keys_rust_path),
                       "--output", str(output), "--report", str(report)]
            self.assertEqual(subprocess.run(command).returncode, 0)
            self.assertEqual(json.loads(output.read_text())["inputs"]["quicktime"], {
                "source_sha256": hashlib.sha256(raw).hexdigest(),
                "ledger_sha256": hashlib.sha256(ledger_path.read_bytes()).hexdigest(),
                "capabilities_sha256": hashlib.sha256(capabilities_path.read_bytes()).hexdigest(),
                "rust_sha256": hashlib.sha256(rust.encode()).hexdigest(),
                "keys_ledger_sha256": hashlib.sha256(keys_ledger_path.read_bytes()).hexdigest(), "keys_rust_sha256": hashlib.sha256(keys_rust_path.read_bytes()).hexdigest(),
            })
            self.assertEqual(subprocess.run(command + ["--check"]).returncode, 0)
            output.write_text("stale\n")
            self.assertNotEqual(subprocess.run(command + ["--check"]).returncode, 0)
            self.assertEqual(output.read_text(), "stale\n")

    def test_cli_rejects_output_report_and_hardlink_aliases(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            catalog_path, hydrated_path = root / "catalog.json", root / "hydrated.json"
            source_path, ledger_path = root / "quicktime-source.json", root / "quicktime-ledger.json"
            capabilities_path, rust_path = root / "quicktime-capabilities.json", root / "quicktime.rs"
            keys_ledger_path, keys_rust_path = root / "keys-ledger.json", root / "keys.rs"
            catalog_path.write_text(json.dumps(catalog([entry()])))
            hydrated_path.write_text(json.dumps(hydrated({"titl": {"Name": "Title"}})))
            raw, _, ledger, capabilities, rust = self.replayed_quicktime_facts()
            source_path.write_bytes(raw)
            ledger_path.write_text(json.dumps(ledger))
            capabilities_path.write_text(json.dumps(capabilities))
            rust_path.write_text(rust)
            keys = join.quicktime_keys_specs.compile_document(json.loads(raw)); keys_ledger_path.write_text(json.dumps(keys)); keys_rust_path.write_text(join.quicktime_keys_specs.render_rust(keys))
            command = ["python3", str(PATH), "--catalog", str(catalog_path), "--hydrated", str(hydrated_path),
                       "--quicktime-bounded-source", str(source_path), "--quicktime-itemlist-ledger", str(ledger_path),
                       "--quicktime-source-capabilities", str(capabilities_path), "--quicktime-itemlist-rust", str(rust_path),
                       "--quicktime-keys-ledger", str(keys_ledger_path), "--quicktime-keys-rust", str(keys_rust_path),
                       "--output", str(root / "same"), "--report", str(root / "same")]
            self.assertNotEqual(subprocess.run(command).returncode, 0)
            hardlink = root / "catalog-link.json"
            os.link(catalog_path, hardlink)
            command[-3] = str(hardlink)
            command[-1] = str(root / "report.md")
            self.assertNotEqual(subprocess.run(command).returncode, 0)
            command[-3] = str(root / "join.json")
            command[command.index("--quicktime-itemlist-rust") + 1] = str(hardlink)
            self.assertNotEqual(subprocess.run(command).returncode, 0)


if __name__ == "__main__":
    unittest.main()
