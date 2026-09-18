"""Regression coverage for generic generated-table retirement."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))
import retire_binary_tables as retire
import table_modules


ROOT = Path(__file__).resolve().parents[2]
SOURCE = "src/parsers/tiff/makernotes/sony/enciphered_tables.rs"
INPUT = ROOT / SOURCE
BINARY_DATA = ROOT / "src/parsers/tiff/makernotes/sony/binary_data.rs"
MANIFEST = ROOT / "tools/exiftool-tables/table_ownership.json"
SHARED_TABLES = ROOT / "src/exiftool_tables/binary/mod.rs"
# The split artifact as one text (hub with every module file spliced in), the
# form the retirement tool reads it in; the temp copies below are single files.
SHARED_TABLES_TEXT = table_modules.read_logical(SHARED_TABLES).encode("utf-8")
ENABLED_TABLES = ROOT / "src/exiftool_tables/enabled.rs"
LEGACY = """//! generated fixture

use super::binary_data::{BinTable, BinTag};

#[rustfmt::skip]
static T0: &[BinTag] = &[
    BinTag { index: 1, subdir: None },
];
#[rustfmt::skip]
static T1: &[BinTag] = &[
    BinTag { index: 2, subdir: Some(1) },
];

pub static TABLES: &[BinTable] = &[
    BinTable {
        name: "Tag202a",
        fmt: Fmt::U8,
        tags: T0,
    },
    BinTable {
        name: "Keep",
        fmt: Fmt::U8,
        tags: T1,
    },
];

/// Table numbers, by ExifTool table name.
#[allow(dead_code)]
pub mod idx {
    pub const TAG202A: usize = 0;
    pub const KEEP: usize = 1;
}
"""


class RetireBinaryTablesTests(unittest.TestCase):
    def render(self, text, manifest=MANIFEST.read_text(), consumer_root=None, identity_seed=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            input_path = root / "input.rs"
            output_path = root / "output.rs"
            identity_path = root / "identity.json"
            manifest_path = root / "ownership.json"
            if consumer_root is None:
                consumer_root = root / "consumers"
                consumer_root.mkdir()
            input_path.write_text(text)
            manifest_path.write_text(manifest)
            if identity_seed is not None:
                identity_path.write_text(identity_seed)
            result = subprocess.run(
                [sys.executable, str(Path(retire.__file__)), "--manifest", str(manifest_path),
                 "--source", SOURCE, "--input", str(input_path), "--output", str(output_path),
                 "--shared-tables", str(SHARED_TABLES), "--enabled-tables", str(ENABLED_TABLES),
                 "--consumer-root", str(consumer_root), "--identity-out", str(identity_path)],
                # The source inputs are intentionally the real generated shared
                # table and gate-B allowlist, not a name-only fixture.
                text=True, capture_output=True,
            )
            return result, output_path.read_text() if output_path.exists() else None, (
                identity_path.read_text() if identity_path.exists() else None
            )

    def test_retires_only_the_shared_owner_table(self):
        result, after, identity = self.render(LEGACY)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('"table": "Tag202a"', result.stdout)
        self.assertEqual(len(json.loads(identity)["records"]), 1)
        self.assertNotIn('name: "Tag202a"', after)
        self.assertNotIn("static T0: &[BinTag]", after)
        self.assertNotIn("TAG202A", after)

        before_tables, _, _ = retire.parse_tables(LEGACY)
        after_tables, _, _ = retire.parse_tables(after)
        self.assertEqual(
            [(table["name"], table["tag"], table["text"]) for table in after_tables],
            [(table["name"], table["tag"], table["text"])
             for table in before_tables if table["name"] != "Tag202a"],
        )
        self.assertEqual(len(after_tables), len(before_tables) - 1)

    def test_reindexes_literal_child_references(self):
        result, after, _ = self.render(text=LEGACY)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("subdir: Some(0)", after)

    def test_refuses_missing_or_malformed_manifest_without_overwrite(self):
        missing = MANIFEST.read_text().replace('"Tag202a"', '"Absent"')
        result, after, _ = self.render(LEGACY, manifest=missing)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(after)
        self.assertIn("shared table is absent", result.stderr)

        malformed = "{}\n"
        result, after, _ = self.render(LEGACY, manifest=malformed)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(after)
        self.assertIn("manifest shape differs", result.stderr)

    def test_refuses_a_surviving_child_that_targets_the_retired_table(self):
        text = LEGACY.replace("subdir: Some(1)", "subdir: Some(0)", 1)
        result, after, _ = self.render(text=text)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(after)
        self.assertIn("points to retired child", result.stderr)

    def test_completed_transform_is_idempotent_and_records_each_identity(self):
        result, after, first_identity = self.render(LEGACY)
        self.assertEqual(result.returncode, 0, result.stderr)
        result, again, second_identity = self.render(text=after, identity_seed=first_identity)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(again, after, "replay to a separate output must materialize its verified bytes")
        self.assertEqual(first_identity, second_identity, "replay must not rewrite the ledger")
        self.assertIn('"replay": true', result.stdout)

    def test_duplicate_identity_refusal_leaves_output_untouched(self):
        result, _, first_identity = self.render(LEGACY)
        self.assertEqual(result.returncode, 0, result.stderr)
        result, after, identity = self.render(LEGACY, identity_seed=first_identity)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(after)
        self.assertEqual(identity, first_identity)
        self.assertIn("identity ledger already records this input", result.stderr)

    def test_refuses_hard_coded_consumer_index(self):
        with tempfile.TemporaryDirectory() as directory:
            consumer = Path(directory)
            (consumer / "caller.rs").write_text("let _ = TABLES[0];\n")
            result, after, _ = self.render(LEGACY, consumer_root=consumer)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(after)
        self.assertIn("retired legacy table handle", result.stderr)

    def test_refuses_a_raw_index_that_shifts_after_retirement(self):
        for source in (
            "let _ = TABLES[1];\n",
            "process(TABLES, 1, input);\n",
            "let edge = Edge { table: Some(1) };\n",
        ):
            with self.subTest(source=source), tempfile.TemporaryDirectory() as directory:
                consumer = Path(directory)
                (consumer / "caller.rs").write_text(source)
                result, after, _ = self.render(LEGACY, consumer_root=consumer)
            self.assertNotEqual(result.returncode, 0)
            self.assertIsNone(after)
            self.assertIn("shifted or retired legacy table handle", result.stderr)

    def test_refuses_a_reintroduced_root_handle(self):
        with tempfile.TemporaryDirectory() as directory:
            consumer = Path(directory)
            (consumer / "roots.rs").write_text("let _ = idx::TAG202A;\n")
            result, after, _ = self.render(LEGACY, consumer_root=consumer)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(after)
        self.assertIn("retired legacy table handle", result.stderr)

    def test_refuses_retirement_when_the_shared_route_is_not_enabled(self):
        migrations = retire.migrations_for(MANIFEST, SOURCE)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary_path = root / "binary_tables.rs"
            enabled_path = root / "enabled.rs"
            binary_path.write_bytes(SHARED_TABLES_TEXT)
            enabled_path.write_text(
                ENABLED_TABLES.read_text().replace('(\"Sony\", \"Tag202a\"),\n', "", 1)
            )
            with self.assertRaisesRegex(retire.Refusal, "not enabled"):
                retire.verify_shared_route(migrations, binary_path, enabled_path)

    def test_ignores_a_commented_enablement_and_rejects_a_blocked_gate_a(self):
        migrations = retire.migrations_for(MANIFEST, SOURCE)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary_path = root / "binary_tables.rs"
            enabled_path = root / "enabled.rs"
            binary_path.write_bytes(SHARED_TABLES_TEXT)
            enabled_path.write_text(
                ENABLED_TABLES.read_text().replace(
                    '("Sony", "Tag202a"),', '// ("Sony", "Tag202a"),', 1
                )
            )
            with self.assertRaisesRegex(retire.Refusal, "not enabled"):
                retire.verify_shared_route(migrations, binary_path, enabled_path)
            shared = SHARED_TABLES_TEXT
            sony_start = shared.index(b"pub static SONY_TAG202A")
            binary_path.write_bytes(shared[:sony_start] + shared[sony_start:].replace(
                b"gate_a: GateA { blocked_by: &[] },",
                b'gate_a: GateA { blocked_by: &[("format", 1)] },', 1,
            ))
            enabled_path.write_text(ENABLED_TABLES.read_text())
            with self.assertRaisesRegex(retire.Refusal, "blocked by Gate A"):
                retire.verify_shared_route(migrations, binary_path, enabled_path)

    def test_remaps_only_a_recognized_subdir_field(self):
        text = LEGACY.replace(
            "BinTag { index: 2, subdir: Some(1) },",
            'BinTag { index: 2, label: "subdir: Some(1)", subdir: Some(1) },',
        )
        result, after, _ = self.render(text)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('label: "subdir: Some(1)", subdir: Some(0)', after)

    def test_refuses_an_unrecognized_subdir_expression(self):
        text = LEGACY.replace("subdir: Some(1)", "subdir: Some(1usize)", 1)
        result, after, _ = self.render(text)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(after)
        self.assertIn("unreadable BinTag subdir expression", result.stderr)

    def test_real_source_has_no_hard_coded_retired_index_consumers(self):
        retire.check_consumers(ROOT / "src", range(14, 37), ["Tag202a"], INPUT)

    def test_replay_rejects_a_raw_index_from_the_original_ordering(self):
        original_identity = (ROOT / "tools/exiftool-tables/table_ownership_identity.json").read_text()
        with tempfile.TemporaryDirectory() as directory:
            consumer = Path(directory)
            (consumer / "caller.rs").write_text("let _ = TABLES[14];\n")
            result, after, _ = self.render(
                INPUT.read_text(), consumer_root=consumer, identity_seed=original_identity
            )
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(after)
        self.assertIn("shifted or retired legacy table handle", result.stderr)

    def test_replay_checks_current_route_without_requiring_old_provenance_hashes(self):
        migrations = retire.migrations_for(MANIFEST, SOURCE)
        ledger = retire.read_identity_ledger(
            ROOT / "tools/exiftool-tables/table_ownership_identity.json"
        )
        with tempfile.TemporaryDirectory() as directory:
            shared_path = Path(directory) / "binary_tables.rs"
            shared_path.write_bytes(b"// unrelated generated table change\n" + SHARED_TABLES_TEXT)
            retire.verify_shared_route(migrations, shared_path, ENABLED_TABLES)
        previous = retire.require_recorded_replay(ledger, SOURCE, migrations, INPUT.read_text())
        self.assertEqual(previous["output_sha256"], retire.hashlib.sha256(INPUT.read_bytes()).hexdigest())

    def test_only_the_obsolete_data_member_was_retired(self):
        variants = [line.strip().rstrip(",") for line in BINARY_DATA.read_text().splitlines()]
        variants = variants[variants.index("pub enum Dm {") + 1:]
        variants = variants[:variants.index("}")]
        self.assertEqual(variants, [
            "AFType", "Battery2", "BatteryStatus1", "BatteryStatus2",
            "FaceInfoLength", "FaceInfoOffset", "FacesDetected", "FlashFired",
            "LensMount", "MetaVersion", "TagB042", "TagVersion", "TempTest1",
            "TempTest2", "Ver9401",
        ])
        self.assertNotIn("Dm::Locations", "\n".join(
            path.read_text() for path in (ROOT / "src").rglob("*.rs")
        ))

    def test_committed_artifact_is_marked_and_idempotent(self):
        before = INPUT.read_text()
        original_identity = (ROOT / "tools/exiftool-tables/table_ownership_identity.json").read_text()
        result, after, identity = self.render(before, identity_seed=original_identity)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(after, before)
        self.assertEqual(identity, original_identity)
        self.assertIn('"replay": true', result.stdout)

    def test_in_place_replay_preserves_source_and_ledger(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.rs"
            ledger = root / "identity.json"
            source.write_bytes(INPUT.read_bytes())
            ledger.write_bytes((ROOT / "tools/exiftool-tables/table_ownership_identity.json").read_bytes())
            before = [(path.read_bytes(), path.stat().st_mtime_ns) for path in (source, ledger)]
            result = subprocess.run(
                [sys.executable, str(Path(retire.__file__)), "--manifest", str(MANIFEST),
                 "--source", SOURCE, "--input", str(source), "--output", str(source),
                 "--shared-tables", str(SHARED_TABLES), "--enabled-tables", str(ENABLED_TABLES),
                 "--consumer-root", str(ROOT / "src"), "--identity-out", str(ledger)],
                text=True, capture_output=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('"replay": true', result.stdout)
            self.assertEqual(
                [(path.read_bytes(), path.stat().st_mtime_ns) for path in (source, ledger)], before
            )


if __name__ == "__main__":
    unittest.main()
