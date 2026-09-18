"""Independent completeness tests for verify.py's binary native inventory.

The small unit fixtures pin the accounting law: a native rule has exactly one
outcome.  The optional live control copies the pinned Sony source before every
mutation, so it never edits the oracle the rest of the workspace uses.  It
models the two artifacts the verifier sees (stale generated facts and a fresh
source-derived set) without importing codegen.py or a producer's rule list.
That remains a checker-unit control; the separate live test below proves the
native dump -> shared codegen -> emitted Rust chain.
"""

from collections import defaultdict
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import verify
import table_modules


REPO_ROOT = Path(__file__).resolve().parents[2]
DUMP_TABLES = REPO_ROOT / "tools/exiftool-tables/dump_tables.pl"
CODEGEN = REPO_ROOT / "tools/exiftool-tables/codegen.py"


def pinned_source():
    """Configured source first, then the established pinned-cache layout.

    Tests must not bake a developer's evidence checkout into the repository.
    CI and callers may set OXIDEX_PINNED_EXIFTOOL; a local standard cache is a
    useful default but is still optional, so the live controls skip cleanly on
    a fresh checkout.
    """
    candidates = []
    if source := os.environ.get("OXIDEX_PINNED_EXIFTOOL"):
        candidates.append(Path(source))
    cache = Path(os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))
    candidates.append(cache / "exiftool")
    return next(
        (candidate for candidate in candidates
         if (candidate / "lib/Image/ExifTool/Sony.pm").is_file()),
        None,
    )


PINNED_SOURCE = pinned_source()


def generated(fields, enums=None, refused=()):
    """The generated-source facts the inventory consumes, with one table."""
    return verify.ParsedRust(
        dict(fields), defaultdict(dict, enums or {}), {}, set(), set(), {}, {}, set(),
        {}, {}, {}, set(refused), {}, {}, {("Sony", "Tag202a"): ("Sony", "Sony", "Camera")}, {},
    )


def inventory(gen, names, enums=None, omissions=None, properties=None, rawfmts=None):
    return verify.native_inventory(
        gen,
        verify.ParsedNativeOmissions(True, omissions or {}),
        names,
        defaultdict(dict, enums or {}),
        rawfmts or {}, {}, set(), set(), defaultdict(set, properties or {}),
    )


class OmissionSidecarParsing(unittest.TestCase):
    def parse(self, text):
        with tempfile.NamedTemporaryFile("w", suffix=".rs", encoding="utf-8", delete=False) as fh:
            fh.write(text)
            fh.flush()
            self.addCleanup(Path(fh.name).unlink)
            return verify.parse_omitted_native_fields(fh.name)

    def test_empty_sidecar_is_distinct_from_an_old_artifact(self):
        old = self.parse("pub static SOMETHING: &[u8] = &[];\n")
        fresh = self.parse("pub static OMITTED_NATIVE_FIELDS: &[OmittedNativeField] = &[];\n")
        self.assertFalse(old.present)
        self.assertTrue(fresh.present)
        self.assertEqual(fresh.rows, {})

    def test_preserves_exact_raw_ids_and_reasons(self):
        parsed = self.parse('''
pub static OMITTED_NATIVE_FIELDS: &[OmittedNativeField] = &[
    OmittedNativeField {
        module: "Sony", table: "Tag202a", raw_id: "276.1#0",
        variant: true, name: Some("ImageNumber"), reasons: &["condition", "raw_conv"],
    },
];
''')
        self.assertEqual(
            parsed.rows[("Sony", "Tag202a", "276.1#0")],
            verify.NativeOmission("ImageNumber", True, ("condition", "raw_conv")),
        )

    def test_duplicate_or_unreadable_omissions_refuse(self):
        duplicate = '''
pub static OMITTED_NATIVE_FIELDS: &[OmittedNativeField] = &[
 OmittedNativeField { module: "Sony", table: "Tag202a", raw_id: "1", variant: false, name: Some("A"), reasons: &["unknown"] },
 OmittedNativeField { module: "Sony", table: "Tag202a", raw_id: "1", variant: false, name: Some("A"), reasons: &["unknown"] },
];
'''
        with self.assertRaisesRegex(SystemExit, "duplicate"):
            self.parse(duplicate)

    def test_current_shared_schema_parses_every_declared_field(self):
        path = REPO_ROOT / "src/exiftool_tables/binary/mod.rs"
        source = table_modules.read_logical(path)
        # Exercise the real regenerated schema. Injecting new members into
        # this file would duplicate them after the first regeneration and
        # test invalid Rust instead of the artifact the verifier must read.
        parsed = verify.parse_rust(path)
        self.assertEqual(len(parsed.fields), len(verify.FIELD_COUNT_RE.findall(source)))

    def test_remainder_string_and_parameterized_default_format_parse(self):
        self.assertEqual(
            verify.expected_fmt_literal("string"),
            ("Some(Fmt::RemainderString)", 1),
        )
        self.assertEqual(verify.expected_fmt_literal(None), ("None", 1))

        # Mutate a real regenerated artifact only in a temporary file. This
        # checks both new literal shapes without depending on a future full
        # regeneration: table FORMAT => string has a stride payload, while an
        # explicit per-field bare string is a separate remainder form.
        source = table_modules.read_logical(REPO_ROOT / "src/exiftool_tables/binary/mod.rs")
        source = source.replace(
            "default_format: Fmt::Int16u,", "default_format: Fmt::Str(1),", 1
        )
        source = source.replace(
            "format: Some(Fmt::Str(4)),", "format: Some(Fmt::RemainderString),", 1
        )
        with tempfile.NamedTemporaryFile("w", suffix=".rs", encoding="utf-8", delete=False) as fh:
            fh.write(source)
            path = Path(fh.name)
        self.addCleanup(path.unlink)
        parsed = verify.parse_rust(path)
        self.assertIn(("Some(Fmt::RemainderString)", 1), parsed.formats.values())


class NativeInventoryAccounting(unittest.TestCase):
    def test_every_native_row_is_accounted_once(self):
        names = {("Sony", "Tag202a", "1"): "Kept", ("Sony", "Tag202a", "2"): "Unknown"}
        gen = generated({("Sony", "Tag202a", "1"): "Kept"})
        omissions = {("Sony", "Tag202a", "2"): verify.NativeOmission("Unknown", False, ("unknown",))}
        got = inventory(gen, names, omissions=omissions,
                        properties={("Sony", "Tag202a", "2"): {"unknown"}})
        self.assertEqual((got.native_rows, got.generated_rows, got.accounted), (2, 1, 2))
        self.assertFalse(got.missing)
        self.assertFalse(got.bad_reasons)

    def test_new_native_row_is_unclassified_not_silently_omitted(self):
        names = {("Sony", "Tag202a", "1"): "Kept", ("Sony", "Tag202a", "2"): "New"}
        got = inventory(generated({("Sony", "Tag202a", "1"): "Kept"}), names)
        self.assertEqual(got.missing, (("Sony", "Tag202a", "2"),))
        self.assertEqual(got.accounted, 1)

    def test_generated_name_drift_is_a_named_native_inventory_failure(self):
        key = ("Sony", "Tag202a", "1")
        got = inventory(generated({key: "OldName"}), {key: "NativeName"})
        self.assertEqual(got.generated_name_mismatches, ((key, "OldName", "NativeName"),))

    def test_explicit_table_scope_keeps_incremental_inventory_honest(self):
        sony = ("Sony", "Tag202a", "1")
        other = ("Other", "Table", "1")
        full = generated({sony: "Sony", other: "Other"})._replace(
            table_groups={
                ("Sony", "Tag202a"): ("MakerNotes", "Sony", "Camera"),
                ("Other", "Table"): ("Other", "Other", "Other"),
            }
        )
        scoped = verify.restrict_native_inventory(full, ["Sony:Tag202a"])
        self.assertEqual(scoped.fields, {sony: "Sony"})
        self.assertEqual(set(scoped.table_groups), {("Sony", "Tag202a")})

    def test_omission_reason_must_describe_a_live_native_property(self):
        names = {("Sony", "Tag202a", "1"): "Plain"}
        omissions = {("Sony", "Tag202a", "1"): verify.NativeOmission("Plain", False, ("raw_conv",))}
        got = inventory(generated({}), names, omissions=omissions)
        self.assertEqual(got.bad_reasons, ((("Sony", "Tag202a", "1"), ("raw_conv",)),))
        self.assertEqual(got.missing, ())  # one rule, one named failure

    def test_atomic_variant_omission_authenticates_its_sibling_cause(self):
        names = {
            ("Sony", "Tag202a", "2#0"): "First",
            ("Sony", "Tag202a", "2#1"): "Second",
        }
        omissions = {
            ("Sony", "Tag202a", "2#0"): verify.NativeOmission("First", True, ("condition",)),
            ("Sony", "Tag202a", "2#1"): verify.NativeOmission("Second", True, ("condition",)),
        }
        got = inventory(
            generated({}), names, omissions=omissions,
            properties={("Sony", "Tag202a", "2#0"): {"condition"}},
        )
        self.assertFalse(got.bad_reasons)
        self.assertEqual(got.accounted, 2)

    def test_overlap_and_stale_records_do_not_hide_or_double_count_rules(self):
        names = {("Sony", "Tag202a", "1"): "Kept"}
        omissions = {
            ("Sony", "Tag202a", "1"): verify.NativeOmission("Kept", False, ("unknown",)),
            ("Sony", "Tag202a", "9"): verify.NativeOmission("Gone", False, ("unknown",)),
        }
        got = inventory(generated({("Sony", "Tag202a", "1"): "Kept"}), names, omissions=omissions)
        self.assertEqual(got.overlaps, (("Sony", "Tag202a", "1"),))
        self.assertEqual(got.stale, (("Sony", "Tag202a", "9"),))
        self.assertEqual(got.accounted, 1)

    def test_native_enum_addition_is_symmetric_for_an_emitted_field(self):
        key = ("Sony", "Tag202a", "1")
        got = inventory(generated({key: "Point"}), {key: "Point"}, enums={key: {"1": "One"}})
        self.assertEqual(got.missing_enum_entries, ((key, "1"),))
        fresh = inventory(generated({key: "Point"}, {key: {"1": "One"}}), {key: "Point"},
                          enums={key: {"1": "One"}})
        self.assertFalse(fresh.missing_enum_entries)

    def test_missing_enum_under_native_other_stays_a_failure_but_is_classified(self):
        key = ("Sony", "Tag202a", "1")
        got = verify.native_inventory(
            generated({key: "Point"}), verify.ParsedNativeOmissions(True, {}),
            {key: "Point"}, defaultdict(dict, {key: {"1": "One"}}),
            {}, {}, set(), set(), defaultdict(set), {key},
        )
        self.assertEqual(got.missing_enum_entries, ((key, "1"),))
        self.assertEqual(got.missing_enum_other_entries, ((key, "1"),))


@unittest.skipUnless(PINNED_SOURCE is not None, "pinned Sony source unavailable")
class CopiedSonyTag202aMutations(unittest.TestCase):
    """Cheap native oracle controls for the 17-row shared-table pilot."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.source = Path(self.tmp.name) / "source"
        shutil.copytree(PINNED_SOURCE / "lib", self.source / "lib")
        self.sony = self.source / "lib/Image/ExifTool/Sony.pm"

    def oracle(self):
        facts = verify.parse_binary_oracle(verify.run_oracle(
            self.source / "lib", str(REPO_ROOT / "tools/exiftool-tables/oracle.pl")))
        names, enums = facts[0], facts[1]
        selected = {k: v for k, v in names.items() if k[:2] == ("Sony", "Tag202a")}
        selected_enums = {k: v for k, v in enums.items() if k[:2] == ("Sony", "Tag202a")}
        return selected, selected_enums

    def regenerate(self):
        """Dump the copied native tree and run the real shared generator.

        This deliberately crosses the same native-data boundary as a normal
        regeneration.  It neither imports codegen.py nor rebuilds Rust facts
        in the test, so a source mutation can only pass after the generator
        actually writes the corresponding artifact.
        """
        tables = Path(self.tmp.name) / "tables.json"
        binary = Path(self.tmp.name) / "binary" / "mod.rs"
        ifd = Path(self.tmp.name) / "ifd" / "mod.rs"
        with tables.open("w", encoding="utf-8") as fh:
            subprocess.run(
                [
                    os.environ.get("EXIFTOOL_PERL", "/usr/bin/perl"), str(DUMP_TABLES),
                    str(self.source / "lib"), "Sony",
                ],
                cwd=REPO_ROOT, stdout=fh, check=True, text=True,
            )
        subprocess.run(
            [
                sys.executable, str(CODEGEN), str(tables), "-o", str(binary),
                "--ifd-out", str(ifd), "--modules", "Sony",
            ],
            cwd=REPO_ROOT, check=True, text=True, capture_output=True,
        )
        return binary, verify.parse_rust(binary), verify.parse_omitted_native_fields(binary)

    @staticmethod
    def tag202a_only(parsed):
        """Keep the verifier's native-minus-generated scope to the pilot."""
        scope = ("Sony", "Tag202a")
        fields = {k: v for k, v in parsed.fields.items() if k[:2] == scope}
        enums = defaultdict(dict, {k: v for k, v in parsed.enums.items() if k[:2] == scope})
        return parsed._replace(
            fields=fields,
            enums=enums,
            masks={k: v for k, v in parsed.masks.items() if k[:2] == scope},
            hooks={k for k in parsed.hooks if k[:2] == scope},
            subdirs={k for k in parsed.subdirs if k[:2] == scope},
            subdir_edges={k: v for k, v in parsed.subdir_edges.items() if k[:2] == scope},
            sound_until={scope: parsed.sound_until[scope]} if scope in parsed.sound_until else {},
            variant_keys={k for k in parsed.variant_keys if k[:2] == scope},
            bitmasks=defaultdict(dict, {k: v for k, v in parsed.bitmasks.items() if k[:2] == scope}),
            other_ids={k for k in parsed.other_ids if k[:2] == scope},
            print_hexes={k: v for k, v in parsed.print_hexes.items() if k[:2] == scope},
            pc_refused={k for k in parsed.pc_refused if k[:2] == scope},
            pc_kinds={k: v for k, v in parsed.pc_kinds.items() if k[:2] == scope},
            formats={k: v for k, v in parsed.formats.items() if k[:2] == scope},
            table_groups={scope: parsed.table_groups[scope]} if scope in parsed.table_groups else {},
            tag_groups={k: v for k, v in parsed.tag_groups.items() if k[:2] == scope},
        )

    def real_inventory(self, parsed, omissions):
        facts = verify.parse_binary_oracle(
            verify.run_oracle(self.source / "lib", str(REPO_ROOT / "tools/exiftool-tables/oracle.pl"))
        )
        scoped_omissions = omissions._replace(
            rows={k: v for k, v in omissions.rows.items() if k[:2] == ("Sony", "Tag202a")}
        )
        return verify.native_inventory(
            self.tag202a_only(parsed), scoped_omissions,
            facts[0], facts[1], facts[11], facts[2], facts[3], facts[4], facts[14],
        )

    @staticmethod
    def source_derived(names, enums):
        return generated(names, enums)

    def mutate(self, old, new):
        text = self.sony.read_text(encoding="utf-8")
        self.assertEqual(text.count(old), 1)
        self.sony.write_text(text.replace(old, new), encoding="utf-8")

    # Each oracle() call runs oracle.pl over the whole copied library (~90s
    # on a 4-vCPU runner). The unmutated baseline is identical for every
    # mutation below, so it is computed once per class, and each mutation is
    # its own test on its own fresh copy so CI can shard them.
    _baseline = None

    def baseline(self):
        cls = type(self)
        if cls._baseline is None:
            cls._baseline = self.oracle()
        return cls._baseline

    def test_pristine_tag202a_oracle_has_seventeen_rows_and_no_enums(self):
        base_names, base_enums = self.baseline()
        self.assertEqual(len(base_names), 17)
        self.assertFalse(base_enums)

    def test_renamed_row_is_stale_and_source_derived_artifact_agrees(self):
        # Rename: the ordinary soundness name comparison catches the stale
        # value; a fresh source-derived artifact agrees without a tag rule.
        base_names, _ = self.baseline()
        self.mutate("Name => 'FocalPlaneAFPointLocation1',", "Name => 'FocalPlaneAFPointLocationOne',")
        names, enums = self.oracle()
        self.assertNotEqual(base_names, names)
        self.assertEqual(self.source_derived(names, enums).fields, names)

    def test_moved_offset_is_missing_until_source_derived_refresh(self):
        # Offset and a newly added row each produce one unclassified native
        # key in stale output, then disappear after source-driven refresh.
        base_names, base_enums = self.baseline()
        self.mutate("0x06 => { Name => 'FocalPlaneAFPointLocation1'", "0x07 => { Name => 'FocalPlaneAFPointLocation1'")
        names, enums = self.oracle()
        self.assertTrue(inventory(self.source_derived(base_names, base_enums), names, enums).missing)
        self.assertFalse(inventory(self.source_derived(names, enums), names, enums).missing)

    def test_added_row_is_missing_until_source_derived_refresh(self):
        base_names, base_enums = self.baseline()
        self.mutate(
            "    0x02 => {\n        Name => 'FocalPlaneAFPointArea',",
            "    0x00 => { Name => 'NativeOnlyPoint', Format => 'int8u' },\n"
            "    0x02 => {\n        Name => 'FocalPlaneAFPointArea',",
        )
        names, enums = self.oracle()
        self.assertTrue(inventory(self.source_derived(base_names, base_enums), names, enums).missing)
        self.assertFalse(inventory(self.source_derived(names, enums), names, enums).missing)

    def test_added_enum_is_missing_until_source_derived_refresh(self):
        # The real table has no enum.  A native PrintConv map is the smallest
        # upgrade change that proves the inverse enum check is live.
        base_names, base_enums = self.baseline()
        self.mutate("Format => 'int8u',\n        RawConv", "Format => 'int8u', PrintConv => { 1 => 'One' },\n        RawConv")
        names, enums = self.oracle()
        stale = inventory(self.source_derived(base_names, base_enums), names, enums)
        self.assertTrue(stale.missing_enum_entries)
        self.assertFalse(inventory(self.source_derived(names, enums), names, enums).missing_enum_entries)

    # A copied native edit must travel through dump_tables.py and codegen.py.
    #
    # This is intentionally distinct from the small accounting tests above:
    # those supply ParsedRust facts to isolate verifier behavior; these prove
    # that no hand-maintained Rust fact is needed for a supported native
    # source change to reach the artifact and clear the independent
    # inventory. The unmutated regeneration is shared by every case, so it is
    # computed once per class; each case is its own test so CI can shard them.
    _regenerated_baseline = None

    def regenerated_baseline(self):
        cls = type(self)
        if cls._regenerated_baseline is None:
            _, base, base_omissions = self.regenerate()
            cls._regenerated_baseline = (base, base_omissions)
        return cls._regenerated_baseline

    def test_real_codegen_baseline_is_complete(self):
        base, base_omissions = self.regenerated_baseline()
        self.assertTrue(base_omissions.present)
        self.assertFalse(self.real_inventory(base, base_omissions).missing)

    def assert_real_codegen_refreshes(self, label, old, new, stale_failure):
        base, base_omissions = self.regenerated_baseline()
        self.mutate(old, new)
        stale = self.real_inventory(base, base_omissions)
        self.assertTrue(stale_failure(stale), f"stale artifact hid native {label}")
        _, fresh, fresh_omissions = self.regenerate()
        got = self.real_inventory(fresh, fresh_omissions)
        self.assertFalse(got.missing)
        self.assertFalse(got.generated_name_mismatches)
        self.assertFalse(got.name_mismatches)
        self.assertFalse(got.missing_enum_entries)

    def test_real_codegen_refreshes_renamed_row(self):
        self.assert_real_codegen_refreshes(
            "rename",
            "Name => 'FocalPlaneAFPointLocation1',",
            "Name => 'FocalPlaneAFPointLocationOne',",
            lambda stale: stale.generated_name_mismatches,
        )

    def test_real_codegen_refreshes_moved_offset(self):
        self.assert_real_codegen_refreshes(
            "offset",
            "0x06 => { Name => 'FocalPlaneAFPointLocation1'",
            "0x07 => { Name => 'FocalPlaneAFPointLocation1'",
            lambda stale: stale.missing,
        )

    def test_real_codegen_refreshes_added_row(self):
        self.assert_real_codegen_refreshes(
            "new row",
            "    0x02 => {\n        Name => 'FocalPlaneAFPointArea',",
            "    0x00 => { Name => 'NativeOnlyPoint', Format => 'int8u' },\n"
            "    0x02 => {\n        Name => 'FocalPlaneAFPointArea',",
            lambda stale: stale.missing,
        )

    def test_real_codegen_refreshes_added_enum(self):
        self.assert_real_codegen_refreshes(
            "enum",
            "Format => 'int8u',\n        RawConv",
            "Format => 'int8u', PrintConv => { 1 => 'One' },\n        RawConv",
            lambda stale: stale.missing_enum_entries,
        )


if __name__ == "__main__":
    unittest.main()
