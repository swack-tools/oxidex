"""Controls for the generated/runtime ownership inventory."""

import copy
import hashlib
import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path


def load_ownership():
    path = Path(__file__).with_name("runtime_ownership.py")
    spec = importlib.util.spec_from_file_location("runtime_ownership", path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class RuntimeOwnershipTests(unittest.TestCase):
    def setUp(self):
        self.ownership = load_ownership()
        self.root = Path(__file__).parents[2]
        self.generated_row = {
            "module": "Exif", "table": "Main",
            "field": {"kind": "numeric", "value": "0x829a"},
            "owner": "generated", "symbol": "generated::decode",
            "source_release": "13.59", "source_sha256": "0" * 64,
            "refusal": None, "fixture": "tools/exiftool-tables/conv_exif_main_ledger.json",
        }

    def temporary_root(self):
        temp = tempfile.TemporaryDirectory(
            prefix="runtime-ownership-legacy-",
            dir=os.environ.get("TMPDIR"),
        )
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        files = {
            "src/core/exif_dir_engine.rs": "pub(crate) const IFD0_HAND_KEPT: &[u16] = &[0x83bb];\n",
            "src/core/tiff_helpers.rs": (
                "pub(crate) const EXIF_IFD_HAND_KEPT: &[u16] = &[];\n"
                "pub(crate) const IFD1_RESIDUAL_IDS: &[u16] = &[];\n"
            ),
            "src/exiftool_tables/enabled_ifd.rs": (
                'pub const ENABLED: &[(&str, &str)] = &[("Exif", "Main")];\n'
            ),
            "src/exiftool_tables/conv/exif_main.rs": "pub fn decode() {}\n",
        }
        for relative, content in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        ledger = root / "tools/exiftool-tables/conv_exif_main_ledger.json"
        ledger.parent.mkdir(parents=True, exist_ok=True)
        ledger.write_text(json.dumps({
            "exiftool_version": "13.59",
            "table_sha256": "a" * 64,
            "generated": [],
            "refused": [],
            "not_conversion_fields": [],
        }))
        fragment = root / "tools/exiftool-tables/runtime_ownership.d/exif_main_residuals.json"
        fragment.parent.mkdir(parents=True, exist_ok=True)
        fragment.write_text(json.dumps(self.ownership._expected_residual_rows(root)))
        return root

    def test_schema_row_is_generated(self):
        self.assertEqual(self.generated_row["owner"], "generated")

    def test_duplicate_owner_is_refused(self):
        residual_row = dict(self.generated_row, owner="residual", symbol="core::tiff_helpers")
        with self.assertRaisesRegex(self.ownership.Refused, "duplicate owner"):
            self.ownership.verify_rows([self.generated_row, residual_row])

    def test_enabled_field_without_owner_is_refused(self):
        unowned = dict(self.generated_row, owner=None, symbol=None)
        with self.assertRaisesRegex(self.ownership.Refused, "unowned enabled field"):
            self.ownership.verify_rows([unowned])

    def test_repository_inventory_is_complete_sorted_and_counted(self):
        inventory = self.ownership.build_inventory(self.root)
        self.assertEqual(inventory["category_totals"], {"generated": 551, "not-applicable": 29, "refused": 17, "residual": 19, "walker-owned": 0})
        self.assertEqual(inventory["rows"], sorted(inventory["rows"], key=lambda r: (r["module"], r["table"], r["field"]["kind"], r["field"]["value"], r["owner"])))
        self.assertIn('("Exif", "Main")', (self.root / "src/exiftool_tables/enabled_ifd.rs").read_text())

    def test_live_residual_arrays_exactly_match_fragments(self):
        fragments = self.ownership._fragment_digest(self.root / "tools/exiftool-tables/runtime_ownership.d")
        self.assertEqual(
            self.ownership._canonical(fragments.ownership_rows["exif_main_residuals.json"]),
            self.ownership._canonical(self.ownership._expected_residual_rows(self.root)),
        )

    def test_residual_fragment_provenance_matches_live_carriers(self):
        fragments = self.ownership._fragment_digest(self.root / "tools/exiftool-tables/runtime_ownership.d")
        expected = {
            row["field"]["value"]: row["source_sha256"]
            for row in self.ownership._expected_residual_rows(self.root)
        }
        actual = {
            row["field"]["value"]: row["source_sha256"]
            for row in fragments.ownership_rows["exif_main_residuals.json"]
        }
        self.assertEqual(actual, expected)

    def test_missing_fixture_symbol_and_malformed_provenance_refuse(self):
        row = dict(self.generated_row)
        row["fixture"] = "src/DOES_NOT_EXIST.rs"
        with self.assertRaisesRegex(self.ownership.Refused, "missing fixture"):
            self.ownership._validate_paths_and_symbols(self.root, [row])
        row = dict(self.generated_row, symbol="src/exiftool_tables/conv/exif_main.rs::NOPE")
        with self.assertRaisesRegex(self.ownership.Refused, "missing symbol"):
            self.ownership._validate_paths_and_symbols(self.root, [row])
        row = dict(self.generated_row, source_sha256="not-a-sha")
        with self.assertRaisesRegex(self.ownership.Refused, "source sha256"):
            self.ownership.verify_rows([row])

    def test_symbol_must_be_an_exact_declaration_not_a_prefix_or_comment(self):
        root = self.temporary_root()
        row = self.ownership._expected_residual_rows(root)[0]
        source = root / "src/core/exif_dir_engine.rs"
        source.write_text(
            source.read_text().replace("IFD0_HAND_KEPT", "IFD0_HAND_KEPT_REMOVED")
            + "\n// IFD0_HAND_KEPT remains only as text\n"
        )
        with self.assertRaisesRegex(self.ownership.Refused, "missing symbol"):
            self.ownership._validate_paths_and_symbols(root, [row])

    def test_declaration_shaped_block_comments_and_literals_do_not_count(self):
        root = self.temporary_root()
        row = self.ownership._expected_residual_rows(root)[0]
        source = root / "src/core/exif_dir_engine.rs"
        decoys = '''
/* outer /* nested */
pub const IFD0_HAND_KEPT: &[u16] = &[0x83bb];
*/
let decoy = "\n pub const IFD0_HAND_KEPT: &[u16] = &[];";
let raw = r###"\n pub const IFD0_HAND_KEPT: &[u16] = &[];"###;
let ch = 'd';
'''
        source.write_text(decoys)
        with self.assertRaisesRegex(self.ownership.Refused, "missing symbol"):
            self.ownership._validate_paths_and_symbols(root, [row])

    def test_structural_reference_requires_its_own_category(self):
        row = next(row for row in self.ownership.build_inventory(self.root)["rows"] if row["owner"] == "not-applicable")
        self.ownership._validate_paths_and_symbols(self.root, [row])
        wrong = dict(row, owner="generated")
        with self.assertRaisesRegex(self.ownership.Refused, "(invalid|missing) symbol"):
            self.ownership._validate_paths_and_symbols(self.root, [wrong])

    def test_registry_and_residual_discovery_ignore_non_code_decoys(self):
        root = self.temporary_root()
        registry = root / "src/exiftool_tables/enabled_ifd.rs"
        registry.write_text('// ("Exif", "Main"),\n/* ("Exif", "Main"), */\nlet x = r#"("Exif", "Main")"#;\n')
        with self.assertRaisesRegex(self.ownership.Refused, "enabled-table registry"):
            self.ownership.build_inventory(root)
        registry.write_text('pub const ENABLED: &[(&str, &str)] = &[("Exif", "Main")];\n')
        carrier = root / "src/core/exif_dir_engine.rs"
        carrier.write_text('/* pub(crate) const IFD0_HAND_KEPT: &[u16] = &[0x83bb]; */\npub(crate) const IFD0_HAND_KEPT: &[u16] = &[];\n')
        rows = self.ownership._expected_residual_rows(root)
        self.assertNotIn("IFD0/0x83bb", {row["field"]["value"] for row in rows})

    def test_stale_inventory_and_empty_refusal_refuse(self):
        root = self.temporary_root()
        self.ownership.write_inventory(root)
        inventory_path = root / "tools/exiftool-tables/runtime_ownership.json"
        stale = json.loads(inventory_path.read_text())
        stale["category_totals"]["generated"] = 1
        inventory_path.write_text(json.dumps(stale))
        with self.assertRaisesRegex(self.ownership.Refused, "stale or tampered"):
            self.ownership.load_rows(root)
        refused = dict(self.generated_row, owner="refused", refusal="")
        with self.assertRaisesRegex(self.ownership.Refused, "refused row has no reason"):
            self.ownership.verify_rows([refused])


class TypedFragmentTests(unittest.TestCase):
    """Synthetic controls for typed fragments; never mutate the checkout."""

    def setUp(self):
        self.ownership = load_ownership()

    @staticmethod
    def write(path, content):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    @classmethod
    def write_json(cls, path, value):
        cls.write(path, json.dumps(value, indent=2, sort_keys=True) + "\n")

    @staticmethod
    def sha(data):
        return hashlib.sha256(data).hexdigest()

    def temporary_root(self):
        temp = tempfile.TemporaryDirectory(
            prefix="runtime-ownership-",
            dir=os.environ.get("TMPDIR"),
        )
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        self.write(
            root / "src/core/exif_dir_engine.rs",
            "pub(crate) const IFD0_HAND_KEPT: &[u16] = &[0x83bb];\n",
        )
        self.write(
            root / "src/core/tiff_helpers.rs",
            "pub(crate) const EXIF_IFD_HAND_KEPT: &[u16] = &[];\n"
            "pub(crate) const IFD1_RESIDUAL_IDS: &[u16] = &[];\n",
        )
        self.write(
            root / "src/exiftool_tables/enabled_ifd.rs",
            'pub const ENABLED: &[(&str, &str)] = &[("Exif", "Main")];\n',
        )
        self.write(root / "src/exiftool_tables/conv/exif_main.rs", "pub fn decode() {}\n")
        self.write(root / "src/vendor.rs", "pub const QUALITY_OWNER: u8 = 1;\n")
        self.write(root / "src/legacy.rs", "pub const OLD_QUALITY: u8 = 1;\n")
        carrier = b"synthetic native carrier\n"
        carrier_path = root / "fixtures/vendor.bin"
        carrier_path.parent.mkdir(parents=True, exist_ok=True)
        carrier_path.write_bytes(carrier)
        self.write_json(
            root / "tools/exiftool-tables/conv_exif_main_ledger.json",
            {
                "exiftool_version": "13.59",
                "table_sha256": "a" * 64,
                "generated": [],
                "refused": [],
                "not_conversion_fields": [],
            },
        )
        self.write_json(
            root / "tools/exiftool-tables/ifd_identity_ledger.json",
            {
                "schema": "oxidex_ifd_identity_ledger_v1",
                "exiftool_version": "13.59",
                "rows": [
                    {
                        "full_name": "Image::ExifTool::Vendor::Main",
                        "module": "Vendor",
                        "table": "Main",
                        "raw_key": "513",
                        "name": "Quality",
                        "variant_path": [],
                        "source_sha256": "1" * 64,
                    },
                    {
                        "full_name": "Image::ExifTool::Vendor::Main",
                        "module": "Vendor",
                        "table": "Main",
                        "raw_key": "519",
                        "name": "CameraType",
                        "variant_path": [],
                        "source_sha256": "2" * 64,
                    },
                ],
            },
        )
        self.write_json(
            root / "tools/exiftool-tables/runtime_ownership.d/exif_main_residuals.json",
            self.ownership._expected_residual_rows(root),
        )
        return root

    def vendor_row(self, *, ops_fixture=False):
        return {
            "module": "Vendor",
            "table": "Main",
            "field": {"kind": "numeric", "value": "0x0201"},
            "owner": "generated",
            "symbol": "src/vendor.rs::QUALITY_OWNER",
            "source_release": "13.59",
            "source_sha256": "1" * 64,
            "provenance_schema": "source-carrier/v1",
            "source_binding": {
                "ledger": "tools/exiftool-tables/ifd_identity_ledger.json",
                "full_name": "Image::ExifTool::Vendor::Main",
                "raw_key": "513",
                "name": "Quality",
                "variant_path": [],
            },
            "refusal": None,
            "fixture": {
                "root": "OXIDEX_OPS_DIR" if ops_fixture else "REPOSITORY",
                "relative_path": "carriers/vendor.bin" if ops_fixture else "fixtures/vendor.bin",
                "sha256": self.sha(b"synthetic native carrier\n"),
            },
        }

    def candidate(self, *, ops_fixture=False):
        return {
            "old_symbol": "src/legacy.rs::OLD_QUALITY",
            "source_fields": ["Vendor::Main:numeric:0x0201"],
            "new_owner": "generated",
            "fixture": {
                "root": "OXIDEX_OPS_DIR" if ops_fixture else "REPOSITORY",
                "relative_path": "carriers/vendor.bin" if ops_fixture else "fixtures/vendor.bin",
                "sha256": self.sha(b"synthetic native carrier\n"),
            },
            "oracle_receipt": {
                "root": "OXIDEX_OPS_DIR",
                "relative_path": "evidence/vendor/oracle-receipt.json",
            },
            "attribution_receipt": {
                "root": "OXIDEX_OPS_DIR",
                "relative_path": "evidence/vendor/attribution-receipt.json",
            },
            "generated_on": "matched",
            "generated_off": "missing-or-residual",
            "deletion_commit": None,
        }

    def seed_documents(self, root, *, ops_fixture=False):
        row, candidate = self.vendor_row(ops_fixture=ops_fixture), self.candidate(ops_fixture=ops_fixture)
        vendor_path = root / "tools/exiftool-tables/runtime_ownership.d/vendor.json"
        candidate_path = root / "tools/exiftool-tables/runtime_ownership.d/vendor-candidates.json"
        self.write_json(vendor_path, [row])
        self.write_json(candidate_path, {"schema": "runtime-deletion-candidates/v1", "candidates": [candidate]})
        return row, candidate, vendor_path, candidate_path

    def ops_root(self, *, carrier="file", oracle="file", oracle_target=None):
        temp = tempfile.TemporaryDirectory(
            prefix="runtime-ownership-ops-",
            dir=os.environ.get("TMPDIR"),
        )
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        entries = (
            ("carriers/vendor.bin", b"synthetic native carrier\n", carrier),
            ("evidence/vendor/oracle-receipt.json", b"{}\n", oracle),
            ("evidence/vendor/attribution-receipt.json", b"{}\n", "file"),
        )
        for relative, content, mode in entries:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            if mode == "missing":
                continue
            if mode == "directory":
                path.mkdir()
            elif mode == "symlink":
                path.symlink_to(oracle_target)
            else:
                path.write_bytes(content)
        return root

    def test_vendor_fragment_is_appended_without_weakening_exact_exif_fence(self):
        root = self.temporary_root()
        self.seed_documents(root)
        inventory = self.ownership.build_inventory(root)
        self.assertEqual(inventory["category_totals"]["generated"], 1)
        self.assertEqual(len(inventory["rows"]), 2)
        self.write_json(root / "tools/exiftool-tables/runtime_ownership.d/exif_main_residuals.json", [])
        with self.assertRaisesRegex(self.ownership.Refused, "residual fragments differ from live Rust residual arrays"):
            self.ownership.build_inventory(root)

    def test_only_the_exact_exif_fragment_may_use_legacy_provenance(self):
        root = self.temporary_root()
        legacy = copy.deepcopy(self.ownership._expected_residual_rows(root)[0])
        legacy["field"]["value"] = "IFD0/0x9999"
        self.write_json(root / "tools/exiftool-tables/runtime_ownership.d/extra-exif.json", [legacy])
        with self.assertRaisesRegex(self.ownership.Refused, "source-carrier/v1"):
            self.ownership.build_inventory(root)

    def test_duplicate_owner_across_fragments_names_both_symbols(self):
        root = self.temporary_root()
        row, _, _, _ = self.seed_documents(root)
        duplicate = copy.deepcopy(row)
        duplicate["symbol"] = "src/legacy.rs::OLD_QUALITY"
        self.write_json(root / "tools/exiftool-tables/runtime_ownership.d/other-vendor.json", [duplicate])
        with self.assertRaisesRegex(self.ownership.Refused, "(QUALITY_OWNER.*OLD_QUALITY|OLD_QUALITY.*QUALITY_OWNER)"):
            self.ownership.build_inventory(root)

    def test_typed_candidates_change_digest_but_not_rows_or_totals(self):
        root = self.temporary_root()
        self.write_json(root / "tools/exiftool-tables/runtime_ownership.d/vendor.json", [self.vendor_row()])
        before = self.ownership.build_inventory(root)
        self.write_json(
            root / "tools/exiftool-tables/runtime_ownership.d/vendor-candidates.json",
            {"schema": "runtime-deletion-candidates/v1", "candidates": [self.candidate()]},
        )
        after = self.ownership.build_inventory(root)
        self.assertNotEqual(before["fragment_inputs_sha256"], after["fragment_inputs_sha256"])
        self.assertEqual(before["rows"], after["rows"])
        self.assertEqual(before["category_totals"], after["category_totals"])
        self.assertNotIn("candidates", after)
        self.assertNotIn("receipts", after)

    def test_fragment_parser_refuses_unknown_mixed_and_malformed_documents(self):
        invalid = (
            ({"schema": "unknown/v1", "candidates": []}, "unsupported fragment schema"),
            ({"schema": "runtime-deletion-candidates/v1", "candidates": [], "rows": []}, "candidate document keys"),
            ({"schema": "runtime-deletion-candidates/v1", "candidates": "no"}, "candidates"),
        )
        for index, (value, message) in enumerate(invalid):
            root = self.temporary_root()
            fragments = root / "tools/exiftool-tables/runtime_ownership.d"
            path = fragments / f"bad-{index}.json"
            self.write_json(path, value)
            with self.subTest(index=index), self.assertRaisesRegex(self.ownership.Refused, message):
                self.ownership._fragment_digest(fragments)
        root = self.temporary_root()
        fragments = root / "tools/exiftool-tables/runtime_ownership.d"
        malformed = fragments / "malformed.json"
        malformed.write_text("{")
        with self.assertRaisesRegex(self.ownership.Refused, "malformed fragment"):
            self.ownership._fragment_digest(fragments)
        root = self.temporary_root()
        self.write_json(
            root / "tools/exiftool-tables/runtime_ownership.d/candidate-list.json",
            [self.candidate()],
        )
        with self.assertRaisesRegex(self.ownership.Refused, "source-carrier/v1"):
            self.ownership.build_inventory(root)

    def test_source_carrier_rejects_swapped_valid_and_forged_provenance(self):
        root = self.temporary_root()
        valid = self.vendor_row()
        vendor = root / "tools/exiftool-tables/runtime_ownership.d/vendor.json"
        self.write_json(vendor, [valid])
        self.ownership.build_inventory(root)
        mutations = []
        missing_profile = copy.deepcopy(valid)
        missing_profile.pop("provenance_schema")
        missing_profile.pop("source_binding")
        mutations.append((missing_profile, "source-carrier"))
        swapped = copy.deepcopy(valid)
        swapped["source_binding"].update(raw_key="519", name="CameraType")
        swapped["source_sha256"] = "2" * 64
        mutations.append((swapped, "stable identity"))
        forged = copy.deepcopy(valid)
        forged["source_sha256"] = "f" * 64
        mutations.append((forged, "source sha256"))
        release = copy.deepcopy(valid)
        release["source_release"] = "13.58"
        mutations.append((release, "source release"))
        no_hash = copy.deepcopy(valid)
        no_hash["fixture"].pop("sha256")
        mutations.append((no_hash, "fixture.*keys"))
        bad_hash = copy.deepcopy(valid)
        bad_hash["fixture"]["sha256"] = "0" * 64
        mutations.append((bad_hash, "carrier sha256"))
        rust_carrier = copy.deepcopy(valid)
        rust_carrier["fixture"].update(
            relative_path="src/vendor.rs",
            sha256=self.sha((root / "src/vendor.rs").read_bytes()),
        )
        mutations.append((rust_carrier, "native carrier"))
        rust_source = copy.deepcopy(valid)
        rust_source["source_binding"]["ledger"] = "src/vendor.rs"
        mutations.append((rust_source, "source ledger"))
        extra_field_key = copy.deepcopy(valid)
        extra_field_key["field"]["display"] = "Quality"
        mutations.append((extra_field_key, "stable identity"))
        decimal_identity = copy.deepcopy(valid)
        decimal_identity["field"]["value"] = "513"
        mutations.append((decimal_identity, "stable identity"))
        malformed_variant = copy.deepcopy(valid)
        malformed_variant["source_binding"]["variant_path"] = ["0"]
        mutations.append((malformed_variant, "source binding types"))
        for index, (row, message) in enumerate(mutations):
            self.write_json(vendor, [row])
            with self.subTest(index=index), self.assertRaisesRegex(self.ownership.Refused, message):
                self.ownership.build_inventory(root)

    def test_source_binding_must_select_one_complete_ledger_row(self):
        root = self.temporary_root()
        ledger_path = root / "tools/exiftool-tables/ifd_identity_ledger.json"
        ledger = json.loads(ledger_path.read_text())
        ledger["rows"].append(copy.deepcopy(ledger["rows"][0]))
        self.write_json(ledger_path, ledger)
        self.write_json(root / "tools/exiftool-tables/runtime_ownership.d/vendor.json", [self.vendor_row()])
        with self.assertRaisesRegex(self.ownership.Refused, "unique source binding"):
            self.ownership.build_inventory(root)

    def test_source_binding_requires_the_committed_ledger_schema(self):
        root = self.temporary_root()
        ledger_path = root / "tools/exiftool-tables/ifd_identity_ledger.json"
        ledger = json.loads(ledger_path.read_text())
        ledger["schema"] = "invented-ledger/v1"
        self.write_json(ledger_path, ledger)
        self.write_json(root / "tools/exiftool-tables/runtime_ownership.d/vendor.json", [self.vendor_row()])
        with self.assertRaisesRegex(self.ownership.Refused, "source ledger schema"):
            self.ownership.build_inventory(root)

    def test_portable_reference_tokens_and_paths_are_strict(self):
        root = self.temporary_root()
        valid = self.vendor_row()
        invalid = (
            {"root": "/Users/alice", "relative_path": "fixture.bin", "sha256": "0" * 64},
            {"root": "REPOSITORY", "relative_path": "/fixture.bin", "sha256": "0" * 64},
            {"root": "REPOSITORY", "relative_path": "../fixture.bin", "sha256": "0" * 64},
            {"root": "REPOSITORY", "relative_path": "a/../fixture.bin", "sha256": "0" * 64},
            {"root": "REPOSITORY", "relative_path": "./fixture.bin", "sha256": "0" * 64},
            {"root": "REPOSITORY", "relative_path": "$HOME/fixture.bin", "sha256": "0" * 64},
            {"root": "REPOSITORY", "relative_path": "~/fixture.bin", "sha256": "0" * 64},
            {"root": "REPOSITORY", "relative_path": "${OXIDEX_OPS_DIR}/fixture.bin", "sha256": "0" * 64},
            {"root": "REPOSITORY", "relative_path": "fixture\0.bin", "sha256": "0" * 64},
        )
        vendor = root / "tools/exiftool-tables/runtime_ownership.d/vendor.json"
        for index, fixture in enumerate(invalid):
            row = copy.deepcopy(valid)
            row["fixture"] = fixture
            self.write_json(vendor, [row])
            with self.subTest(index=index), self.assertRaisesRegex(self.ownership.Refused, "fixture"):
                self.ownership.build_inventory(root)

    def test_explicit_ops_root_checks_two_roots_files_containment_and_hashes(self):
        root = self.temporary_root()
        _, _, vendor_path, candidate_path = self.seed_documents(root, ops_fixture=True)
        vendor_bytes, candidate_bytes = vendor_path.read_bytes(), candidate_path.read_bytes()
        first, second = self.ops_root(), self.ops_root()
        self.assertEqual(
            self.ownership.build_inventory(root, ops_root=first),
            self.ownership.build_inventory(root, ops_root=second),
        )
        self.assertEqual(vendor_path.read_bytes(), vendor_bytes)
        self.assertEqual(candidate_path.read_bytes(), candidate_bytes)
        with self.assertRaisesRegex(self.ownership.Refused, "absolute"):
            self.ownership.build_inventory(root, ops_root=Path("relative"))
        with self.assertRaisesRegex(self.ownership.Refused, "missing"):
            self.ownership.build_inventory(root, ops_root=second / "absent-root")
        (first / "carriers/vendor.bin").write_bytes(b"wrong")
        with self.assertRaisesRegex(self.ownership.Refused, "carrier sha256"):
            self.ownership.build_inventory(root, ops_root=first)
        (first / "carriers/vendor.bin").write_bytes(b"synthetic native carrier\n")
        directory_root = self.ops_root(carrier="directory")
        with self.assertRaisesRegex(self.ownership.Refused, "regular file"):
            self.ownership.build_inventory(root, ops_root=directory_root)
        missing = self.ops_root(oracle="missing")
        with self.assertRaisesRegex(self.ownership.Refused, "missing"):
            self.ownership.build_inventory(root, ops_root=missing)
        escape_target = second / "outside.json"
        escape_target.write_text("{}\n")
        symlink_root = self.ops_root(oracle="symlink", oracle_target=escape_target)
        with self.assertRaisesRegex(self.ownership.Refused, "symlink"):
            self.ownership.build_inventory(root, ops_root=symlink_root)

    def test_candidate_contract_refuses_invalid_fields_and_authority(self):
        root = self.temporary_root()
        row, valid, _, candidate_path = self.seed_documents(root)
        self.ownership.build_inventory(root)
        mutations = []
        missing = copy.deepcopy(valid)
        missing.pop("fixture")
        mutations.append((missing, "candidate keys"))
        extra = copy.deepcopy(valid)
        extra["receipt_passed"] = True
        mutations.append((extra, "candidate keys"))
        bad_symbol = copy.deepcopy(valid)
        bad_symbol["old_symbol"] = "src/legacy.rs::OLD_QUALITY[0]"
        mutations.append((bad_symbol, "old symbol"))
        duplicate_fields = copy.deepcopy(valid)
        duplicate_fields["source_fields"].append(duplicate_fields["source_fields"][0])
        mutations.append((duplicate_fields, "unique source_fields"))
        unresolved = copy.deepcopy(valid)
        unresolved["source_fields"] = ["Vendor::Main:numeric:0xffff"]
        mutations.append((unresolved, "source field"))
        mismatch = copy.deepcopy(valid)
        mismatch["new_owner"] = "src/vendor.rs::QUALITY_OWNER"
        mutations.append((mismatch, "new_owner"))
        absent_fixture = copy.deepcopy(valid)
        absent_fixture["fixture"]["relative_path"] = "fixtures/missing.bin"
        mutations.append((absent_fixture, "missing.*fixture"))
        bad_fixture_root = copy.deepcopy(valid)
        bad_fixture_root["fixture"]["root"] = "HOME"
        mutations.append((bad_fixture_root, "root token"))
        bad_receipt_root = copy.deepcopy(valid)
        bad_receipt_root["oracle_receipt"]["root"] = "REPOSITORY"
        mutations.append((bad_receipt_root, "root token"))
        bad_on = copy.deepcopy(valid)
        bad_on["generated_on"] = "passing"
        mutations.append((bad_on, "generated_on"))
        bad_off = copy.deepcopy(valid)
        bad_off["generated_off"] = "matched"
        mutations.append((bad_off, "generated_off"))
        committed = copy.deepcopy(valid)
        committed["deletion_commit"] = "abc123"
        mutations.append((committed, "deletion_commit"))
        for index, (candidate, message) in enumerate(mutations):
            self.write_json(candidate_path, {"schema": "runtime-deletion-candidates/v1", "candidates": [candidate]})
            with self.subTest(index=index), self.assertRaisesRegex(self.ownership.Refused, message):
                self.ownership.build_inventory(root)
        walker = copy.deepcopy(row)
        walker["owner"] = "walker-owned"
        self.write_json(root / "tools/exiftool-tables/runtime_ownership.d/vendor.json", [walker])
        self.write_json(candidate_path, {"schema": "runtime-deletion-candidates/v1", "candidates": [valid]})
        with self.assertRaisesRegex(self.ownership.Refused, "new_owner"):
            self.ownership.build_inventory(root)

    def test_candidate_symbols_are_unique_across_documents(self):
        root = self.temporary_root()
        _, candidate, _, _ = self.seed_documents(root)
        self.write_json(
            root / "tools/exiftool-tables/runtime_ownership.d/second-candidates.json",
            {"schema": "runtime-deletion-candidates/v1", "candidates": [candidate]},
        )
        with self.assertRaisesRegex(self.ownership.Refused, "duplicate candidate"):
            self.ownership.build_inventory(root)

    def test_writer_preserves_inputs_and_is_byte_deterministic(self):
        root = self.temporary_root()
        _, _, vendor_path, candidate_path = self.seed_documents(root)
        vendor_bytes, candidate_bytes = vendor_path.read_bytes(), candidate_path.read_bytes()
        self.ownership.write_inventory(root)
        first_exif = (root / "tools/exiftool-tables/runtime_ownership.d/exif_main_residuals.json").read_bytes()
        first_inventory = (root / "tools/exiftool-tables/runtime_ownership.json").read_bytes()
        self.ownership.write_inventory(root)
        self.assertEqual(vendor_path.read_bytes(), vendor_bytes)
        self.assertEqual(candidate_path.read_bytes(), candidate_bytes)
        self.assertEqual((root / "tools/exiftool-tables/runtime_ownership.d/exif_main_residuals.json").read_bytes(), first_exif)
        self.assertEqual((root / "tools/exiftool-tables/runtime_ownership.json").read_bytes(), first_inventory)


if __name__ == "__main__":
    unittest.main()
