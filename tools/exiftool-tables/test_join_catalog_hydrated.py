import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import runtime_evidence_inputs as runtime_inputs
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
import verify_quicktime_reader as read_verifier
import verify_quicktime_keys_reader as keys_verifier

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
    def ifd_facts(self, tags=None):
        document = {"exiftool_version": "13.59", "modules": {"Exif": {"tables": {"Main": {
            "meta": {}, "tags": tags if tags is not None else {"315": {"Name": "Artist", "Format": "int16u"},
                                "316": {"Name": "Omitted", "Format": "int16u", "RawConv": "$val"}}}}}}}
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        source, binary, rust, ledger = (root / name for name in ("source.json", "binary.rs", "ifd.rs", "ledger.json"))
        source.write_text(json.dumps(document))
        command = ["python3", str(PATH.with_name("codegen.py")), str(source), "-o", str(binary),
                   "--ifd-out", str(rust), "--ifd-identity-ledger-out", str(ledger)]
        result = subprocess.run(command, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return source.read_bytes(), json.loads(ledger.read_text()), rust.read_text()

    def test_unnamed_detached_ifd_refusal_is_accounted_without_declaration_credit(self):
        source, ledger, rust = self.ifd_facts({"Artist": "Artist"})
        entry_row = entry("Artist", "Artist")
        entry_row["table"] = "Image::ExifTool::Exif::Main"
        cat = catalog([entry_row])
        hyd = {"exiftool_version": "13.59", "hydrated_layouts": {
            "catalog_counts": {"total_tag_entries": 1}, "source_provenance": {"sources": copy.deepcopy(SOURCE)},
            "tables": {entry_row["table"]: {"full_name": entry_row["table"],
                                          "tags": {"Artist": {"Name": "Artist"}}}}}}
        digests = {"source_sha256": hashlib.sha256(source).hexdigest(),
                   "ledger_sha256": hashlib.sha256(json.dumps(ledger).encode()).hexdigest(),
                   "rust_sha256": hashlib.sha256(rust.encode()).hexdigest()}
        result = join.build(cat, hyd, "catalog", "hydrated", ifd_source=source, ifd_ledger=ledger,
                            ifd_rust=rust, ifd_input_digests=digests)
        row = result["entries"][0]
        self.assertEqual(row["reader_implementation"], "ifd_schema_declaration_refused_unobserved")
        self.assertTrue(row["implementation_refusal_reasons"])
        self.assertEqual(row["observed_read"], "not_observed_yet")

    def test_ifd_schema_declarations_replay_and_remain_unobserved(self):
        source, ledger, rust = self.ifd_facts()
        rows = join.ifd_implementation(source, ledger, rust)
        self.assertEqual(rows[("Image::ExifTool::Exif::Main", "315", 0)]["reader_state"], "eligible")
        self.assertEqual(rows[("Image::ExifTool::Exif::Main", "316", 0)]["reader_state"], "omitted")
        entries = [
            {"table": "Image::ExifTool::Exif::Main", "raw_key": "315", "variant_index": 0,
             "name": "Artist", "normalized_name": "artist", "groups": {"0": "EXIF", "1": "IFD0", "2": "Author"}, "no_lookup": False, "unknown": False},
            {"table": "Image::ExifTool::Exif::Main", "raw_key": "316", "variant_index": 0,
             "name": "Omitted", "normalized_name": "omitted", "groups": {"0": "EXIF", "1": "IFD0", "2": "Author"}, "no_lookup": False, "unknown": False},
        ]
        cat = catalog(entries)
        hyd = {"exiftool_version": "13.59", "hydrated_layouts": {
            "catalog_counts": {"total_tag_entries": 2}, "source_provenance": {"sources": copy.deepcopy(SOURCE)},
            "tables": {"Image::ExifTool::Exif::Main": {"full_name": "Image::ExifTool::Exif::Main", "tags": {
                "315": {"Name": "Artist"}, "316": {"Name": "Omitted"}}}}}}
        digests = {"source_sha256": hashlib.sha256(source).hexdigest(),
                   "ledger_sha256": hashlib.sha256(json.dumps(ledger).encode()).hexdigest(),
                   "rust_sha256": hashlib.sha256(rust.encode()).hexdigest()}
        result = join.build(cat, hyd, "catalog", "hydrated", ifd_source=source, ifd_ledger=ledger,
                            ifd_rust=rust, ifd_input_digests=digests)
        states = {row["catalog"]["name"]: row for row in result["entries"]}
        self.assertEqual(states["Artist"]["reader_implementation"], "ifd_schema_declaration_eligible_unobserved")
        self.assertEqual(states["Omitted"]["reader_implementation"], "ifd_schema_declaration_omitted_unobserved")
        self.assertEqual(states["Artist"]["observed_read"], "not_observed_yet")
        self.assertEqual(result["inputs"]["ifd"], digests)
        for bad_source, bad_ledger, bad_rust in (
            (source + b" ", ledger, rust),
            (source, {**ledger, "rows": ledger["rows"][:-1]}, rust),
            (source, ledger, rust + "// tampered\n"),
        ):
            with self.subTest(mutated=True), self.assertRaises(ValueError):
                join.ifd_implementation(bad_source, bad_ledger, bad_rust)
        forged_rust = rust.replace('name: "Artist"', 'name: "Forged"', 1)
        self.assertNotEqual(forged_rust, rust)
        forged_ledger = copy.deepcopy(ledger)
        forged_ledger["source"]["ifd_rust_sha256"] = join.codegen._canonical_ifd_rust_sha256(forged_rust)
        with self.assertRaisesRegex(ValueError, "compiler replay"):
            join.ifd_implementation(source, forged_ledger, forged_rust)
        self.assertIsNone(ledger["source"]["expr_ledger_sha256"])
        bound = copy.deepcopy(ledger)
        oracle = b'{"authenticated":true}'
        bound["source"]["expr_ledger_sha256"] = hashlib.sha256(oracle).hexdigest()
        with patch.object(join.codegen, "load_oracle_ledger", return_value=set()) as replay:
            join.ifd_implementation(source, bound, rust, oracle)
            replay.assert_called_once()
        for supplied in (None, b'{"substituted":true}'):
            with self.subTest(expr_ledger=supplied), self.assertRaises(ValueError):
                join.ifd_implementation(source, bound, rust, supplied)


    def test_table_report_keeps_source_only_variants_and_separate_contexts(self):
        second = "Image::ExifTool::Other::Main"
        source_only = "Image::ExifTool::Other::Internal"
        base = {"identity": {"table": TABLE},
                "catalog": {"normalized_name": "title"},
                "reader_implementation": "generated_reader_declaration_unobserved",
                "writer_implementation": "writer_not_declared",
                "implementation_refusal_reasons": None,
                "observed_read": "not_observed_yet", "observed_write": "not_observed_yet"}
        refused = copy.deepcopy(base)
        refused.update(reader_implementation="blocked_generated_reader_refusal",
                       implementation_refusal_reasons=["unsupported_conversion", "perl_callback"])
        other = copy.deepcopy(base)
        other["identity"]["table"] = second
        other.update(writer_implementation="generated_writer_declaration_unobserved",
                     observed_read="observed_matched_read", observed_write="observed_matched_write")
        source = {(TABLE, "one", 0): {}, (TABLE, "one", 1): {},
                  (TABLE, "source-only", 0): {}, (second, "same-name", 0): {},
                  (source_only, "internal", 0): {}}
        empty = "Image::ExifTool::Other::Empty"
        result = join.table_coverage([base, refused, other], source, [empty])
        self.assertEqual(result[empty]["source_variant_rows"], 0)
        self.assertEqual(result[empty]["catalog_entries"], 0)
        self.assertEqual(result[TABLE]["source_variant_rows"], 3)
        self.assertEqual(result[TABLE]["catalog_entries"], 2)
        self.assertEqual(result[TABLE]["catalog_unique_case_insensitive_names"], 1)
        self.assertEqual(result[TABLE]["reader_refusal_reasons"], {"perl_callback": 1, "unsupported_conversion": 1})
        self.assertEqual(result[TABLE]["observed_read_catalog_entries"], 0)
        self.assertEqual(result[second]["observed_write_catalog_entries"], 1)
        self.assertEqual(result[source_only]["catalog_entries"], 0)
        self.assertEqual(result[source_only]["source_variant_rows"], 1)
        self.assertEqual(sum(item["catalog_entries"] for item in result.values()), 3)
        self.assertEqual(sum(item["source_variant_rows"] for item in result.values()), 5)

    def test_keys_evidence_import_replays_complete_grid_and_deduplicates_modes(self):
        source = (PATH.parent / "fixtures/quicktime_source_13_59.json").read_bytes()
        ledger = join.quicktime_keys_specs.compile_document(json.loads(source))
        rust = join.quicktime_keys_specs.render_rust(ledger)
        rows = []
        for fixture, data in keys_verifier.cases().items():
            spec = keys_verifier.resolved_spec(keys_verifier.fixture_identity(data), ledger)
            expected = {} if spec is None else {"Keys:" + spec["name"]: "value"}
            transcript = json.dumps([expected], sort_keys=True)
            for mode in keys_verifier.MODES:
                rows.append({"fixture": fixture, "mode": mode, "fixture_sha256": keys_verifier.sha(data),
                             "fixture_identity": keys_verifier.fixture_identity(data), "expected": expected, "actual": expected,
                             "native_json": transcript, "native_json_sha256": keys_verifier.sha(transcript.encode()),
                             "oxidex_json": transcript, "oxidex_json_sha256": keys_verifier.sha(transcript.encode()), "matched": True})
        inputs = {"source_sha256": hashlib.sha256(source).hexdigest(),
                  "keys_ledger_sha256": hashlib.sha256(json.dumps(ledger).encode()).hexdigest(),
                  "keys_rust_sha256": hashlib.sha256(rust.encode()).hexdigest()}
        credited = keys_verifier.validate_report({"schema": keys_verifier.SCHEMA, "observations": rows}, ledger)
        identities = sorted({f"{row['group1']}:{row['tag_name']}" for row in credited})
        evidence = {"schema": keys_verifier.SCHEMA, "inputs": {"source_sha256": inputs["source_sha256"], "ledger_sha256": inputs["keys_ledger_sha256"], "rust_sha256": inputs["keys_rust_sha256"]},
                    "producer": {"source_dirty": False, "pin": "13.59", "source_commit": "a" * 40, "runtime_artifact_sha256": "b" * 64,
                                 "runtime_input_manifest_sha256": runtime_inputs.runtime_input_manifest(join.quicktime_selector.ROOT),
                                 "fixture_manifest_sha256": keys_verifier.sha(keys_verifier.canonical({n: keys_verifier.sha(d) for n, d in keys_verifier.cases().items()}))},
                    "observations": rows, "matched_occurrences": credited, "observed_identities": identities,
                    "metric_c": {"distinct_group1_tag_identities": len(identities), "fixture_tag_occurrences": len({(x['fixture'], x['group1'], x['tag_name']) for x in credited}), "matched_mode_observations": len(credited)}}
        observed = join.quicktime_keys_observed_reads(evidence, source, ledger, rust, inputs)
        self.assertGreater(len(observed), 0)
        self.assertEqual(len(observed), len({(raw, path, name) for raw, path, name in observed}))
        for mutate in (lambda d: d["inputs"].update(ledger_sha256="0" * 64), lambda d: d["observations"].pop(),
                       lambda d: d["observations"][0].update(native_json="[]"),
                       lambda d: d["producer"].update(runtime_input_manifest_sha256="0" * 64),
                       lambda d: d["metric_c"].update(fixture_tag_occurrences=0)):
            bad = copy.deepcopy(evidence); mutate(bad)
            with self.assertRaises(ValueError):
                join.quicktime_keys_observed_reads(bad, source, ledger, rust, inputs)
    def test_write_observations_have_separate_operation_and_group1_counts(self):
        import write_readback_evidence
        table = "Image::ExifTool::Exif::Main"
        identity = (table, "315", 0)
        coordinate = {"table": table, "raw_key": "315", "variant_index": 0}
        item = {**entry("Artist", "315"), **coordinate,
                "groups": {"0": "EXIF", "1": "IFD0", "2": "Author"}}
        cat = catalog([item])
        capture = {"exiftool_version": "13.59"}
        sources = {}
        loaded = {}
        for field, path in write_readback_evidence.SOURCE_FILES.items():
            capture[field] = "a" * 64
            sources[path] = {"library_relative_path": path, "sha256": "a" * 64}
            loaded[path] = "a" * 64
        cat["producer"]["sources"] = sources
        hyd = {"exiftool_version": "13.59", "hydrated_layouts": {
            "catalog_counts": {"total_tag_entries": 1}, "source_provenance": {"sources": sources},
            "tables": {table: {"full_name": table, "tags": {"315": {"Name": "Artist"}}}}}}
        source = json.dumps({"native_write_capture_context": {"loaded_modules": loaded}}).encode()
        writer = {identity: {"name": "Artist", "write_group": "IFD0"}}
        digests = {key: "b" * 64 for key in ("source_sha256", "final_ledger_sha256", "final_rust_sha256", "public_ledger_sha256", "public_rust_sha256")}
        observations = [{"source_identity": coordinate, "group1_name": "IFD1:Artist", "case_id": name}
                        for name in ("little-update", "big-update")]
        sidecar = {"producer": {"fixture": True}}
        with patch.object(join, "writer_implementation", return_value=writer), \
             patch.object(write_readback_evidence, "validate_evidence", return_value=observations) as verify:
            args = dict(writer_source=source, writer_final_ledger={}, writer_final_rust="final",
                        writer_public_ledger={"source": {"capture": capture}}, writer_public_rust="public",
                        writer_input_digests=digests)
            before = join.build(cat, hyd, "catalog", "hydrated", **args)
            self.assertEqual(before["entries"][0]["observed_write"], "not_observed_yet")
            self.assertNotIn("write_readback", before["counts"])
            after = join.build(cat, hyd, "catalog", "hydrated", writer_read_evidence=sidecar, **args)
            verify.assert_called_once_with(sidecar, writer, digests, capture)
        self.assertEqual(after["entries"][0]["observed_write"], "not_observed_yet")
        self.assertEqual(after["entries"][0]["observed_write_group1_names"], [])
        self.assertEqual(after["entries"][0]["alternate_context_write_group1_names"], ["IFD1:Artist"])
        self.assertEqual(after["counts"]["write_readback"]["catalog_matched_write_operations"], 0)
        self.assertEqual(after["counts"]["write_readback"]["alternate_context_write_operations"], 2)
        self.assertEqual(after["counts"]["write_readback"]["successful_write_operations"], 2)
        self.assertEqual(after["counts"]["write_readback"]["distinct_group1_names"], 1)
        self.assertEqual(after["entries"][0]["observed_read"], "not_observed_yet")

        # Exact Group1 matches credit the catalog row. Observations in another
        # directory keep their own operation/name counts even in a mixed run.
        correct = {"source_identity": coordinate, "group1_name": "IFD0:Artist", "case_id": "ifd0-update"}
        for actual, expected_counts, alternate_names in (
            ([correct], (1, 0, 1), []),
            ([correct, *observations], (1, 2, 2), ["IFD1:Artist"]),
        ):
            with patch.object(join, "writer_implementation", return_value=writer), \
                 patch.object(write_readback_evidence, "validate_evidence", return_value=actual):
                matched = join.build(cat, hyd, "catalog", "hydrated", writer_read_evidence=sidecar, **args)
            record = matched["entries"][0]
            self.assertEqual(record["observed_write"], "observed_matched_write")
            self.assertEqual(record["observed_write_group1_names"], ["IFD0:Artist"])
            self.assertEqual(record["alternate_context_write_group1_names"], alternate_names)
            counts = matched["counts"]["write_readback"]
            self.assertEqual((counts["catalog_matched_write_operations"], counts["alternate_context_write_operations"],
                              counts["distinct_group1_names"]), expected_counts)

        # The catalog may itself describe an alternate physical context. Match
        # that actual Group1; do not substitute the writer's preferred group.
        alternate_catalog = copy.deepcopy(cat)
        alternate_catalog["entries"][0]["groups"]["1"] = "IFD1"
        with patch.object(join, "writer_implementation", return_value=writer), \
             patch.object(write_readback_evidence, "validate_evidence", return_value=observations):
            matched = join.build(alternate_catalog, hyd, "catalog", "hydrated", writer_read_evidence=sidecar, **args)
        self.assertEqual(matched["entries"][0]["observed_write"], "observed_matched_write")
        self.assertEqual(matched["entries"][0]["observed_write_group1_names"], ["IFD1:Artist"])
        self.assertEqual(matched["counts"]["write_readback"]["catalog_matched_write_operations"], 2)

    def test_write_evidence_without_authenticated_writer_inputs_refuses(self):
        with self.assertRaisesRegex(ValueError, "complete authenticated writer"):
            join.build(catalog([entry()]), hydrated({"titl": {"Name": "Title"}}), "a", "b", writer_read_evidence={})

    def test_writer_replay_requires_exact_source_bound_artifacts_and_conserves_rows(self):
        source = json.dumps({"exiftool_version": "13.59"}).encode()
        row = {"module": "Exif", "table": "Main", "full_name": "Image::ExifTool::Exif::Main",
               "raw_tag_id": 315, "name": "Artist", "group0": "EXIF", "group1": "IFD0",
               "write_group": "IFD0", "source_control_sha256": "a" * 64, "semantics_sha256": "b" * 64,
               "state": "current"}
        current = {join.public_migration._entry_key(row): SimpleNamespace(
            full_name=row["full_name"], name=row["name"], source_control_sha256=row["source_control_sha256"],
            semantics_sha256=row["semantics_sha256"])}
        final = {"recipes": [{"full_name": row["full_name"], "raw_tag_id": 315, "name": "Artist", "physical_write_group": "IFD0", "wire_format": "string"}], "registry": {"formats": ("TIFF", "JPEG")}}
        public = {"source": {"capture": "bound"}, "entries": [row]}
        with patch.object(join.final_scalar_stage, "generate", return_value=("final-rust", final)), \
             patch.object(join.public_migration, "compile_current", return_value=({"capture": "bound"}, current)), \
             patch.object(join.public_migration, "validate_ledger"), \
             patch.object(join.public_migration, "render_rust", return_value="public-rust"):
            rows = join.writer_implementation(source, json.loads(json.dumps(final)), "final-rust", public, "public-rust")
            self.assertEqual(rows, {(row["full_name"], "315", 0): {"name": "Artist", "write_group": "IFD0", "group0": "EXIF", "wire_format": "string", "semantics_sha256": "b" * 64}})
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
            observed_snapshot, observed_report = root / "observed.json", root / "observed.md"
            self.assertNotEqual(subprocess.run(command + ["--observed-snapshot", str(observed_snapshot),
                                                           "--observed-report", str(observed_report)]).returncode, 0)
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

    def test_cli_observed_snapshot_is_only_emitted_after_join_evidence_path(self):
        from catalog_observed_snapshot import canonical_hash, validate_snapshot
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, ledger, capabilities, rust, keys_ledger, keys_rust = [root / name for name in
                ("source.json", "ledger.json", "capabilities.json", "specs.rs", "keys-ledger.json", "keys.rs")]
            catalog_path, hydrated_path = root / "catalog.json", root / "hydrated.json"
            evidence_path, output, report_path = root / "evidence.json", root / "join.json", root / "join.md"
            snapshot_path, observed_report = root / "observed.json", root / "observed.md"
            for path in (source, ledger, capabilities, rust, keys_ledger, keys_rust):
                path.write_text("{}")
            catalog_path.write_text("{}")
            hydrated_path.write_text("{}")
            producer = {"source_commit": "1" * 40, "runtime_input_manifest_sha256": "2" * 64}
            evidence = {"schema": "fixture-native-receipt", "producer": producer}
            evidence_path.write_text(json.dumps(evidence))
            source_join = {"schema": join.SCHEMA, "inputs": {"exiftool_version": "13.59"},
                           "counts": {"joined_records": 1, "observed_read": {"not_observed_yet": 1}},
                           "entries": [{"identity": {"table": TABLE, "raw_key": "titl", "variant_index": 0},
                                        "catalog": {"name": "Title"}, "source": {}, "source_layout_status": "source_row_joined",
                                        "source_derived_implementation": "generated_reader_declaration_unobserved",
                                        "reader_implementation": "generated_reader_declaration_unobserved",
                                        "writer_implementation": "writer_not_declared", "implementation_refusal_reasons": None,
                                        "observed_read": "not_observed_yet", "observed_write": "not_observed_yet"}]}
            observed_join = copy.deepcopy(source_join)
            observed_join["inputs"]["quicktime_read_evidence"] = {"sha256": canonical_hash(evidence), "producer": producer}
            command = ["join_catalog_hydrated.py", "--catalog", str(catalog_path), "--hydrated", str(hydrated_path),
                       "--quicktime-bounded-source", str(source), "--quicktime-itemlist-ledger", str(ledger),
                       "--quicktime-source-capabilities", str(capabilities), "--quicktime-itemlist-rust", str(rust),
                       "--quicktime-keys-ledger", str(keys_ledger), "--quicktime-keys-rust", str(keys_rust),
                       "--quicktime-read-evidence", str(evidence_path), "--observed-snapshot", str(snapshot_path),
                       "--observed-report", str(observed_report), "--output", str(output), "--report", str(report_path)]
            with patch.object(sys, "argv", command), patch.object(join, "build", side_effect=[observed_join, source_join]), \
                    patch.object(join, "report", return_value="join report\n"):
                self.assertEqual(join.main(), 0)
            validate_snapshot(json.loads(snapshot_path.read_text()))


if __name__ == "__main__":
    unittest.main()
