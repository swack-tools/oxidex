"""Focused source-only checks for inactive write candidates."""

import copy
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest

import write_descriptors


CONTROLS = (
    "CanCreate", "DelValue", "Deletable", "Mandatory", "PrintConvInv",
    "RawConvInv", "Validate", "ValueConvInv", "Writable", "WriteAlso",
    "WriteCheck", "WriteCondition", "WriteGroup", "WriteHook", "WriteLast",
    "WritePseudo",
)


def absent_controls():
    return {key: {"present": False} for key in CONTROLS}


def code_fact(name, body="sub body", *, resolved=True, dependencies=None):
    result = {
        "__perl": "CODE", "__name": name, "resolved": resolved,
        "__deparse": body if resolved else None,
        "source_file": "Image/ExifTool/WriteExif.pl" if resolved else None,
        "source_sha256": "a" * 64 if resolved else None,
    }
    if not resolved:
        result["reason"] = "code_ref_unavailable"
    if dependencies is not None:
        result["dependencies"] = dependencies
    return result


def procedures(*, resolved=True, dependency=False):
    dependencies = (
        {"Image::ExifTool::Exif::Helper": code_fact("Image::ExifTool::Exif::Helper", "helper")}
        if dependency else {}
    )
    return {
        "effective_write_proc": {
            "present": True,
            "effective": code_fact("Image::ExifTool::Exif::WriteExif", "write-body", resolved=resolved,
                                   dependencies=dependencies),
        },
        "effective_check_proc": {
            "present": True,
            "effective": code_fact("Image::ExifTool::Exif::CheckExif", "check-body", resolved=resolved,
                                   dependencies=dependencies),
        },
    }


def row(name="SourceName", writable="string", group="IFD0", *, raw_controls=None, extra=None, unknown=None):
    controls = absent_controls()
    controls["Writable"] = {"present": True, "value": writable}
    controls["WriteGroup"] = {"present": True, "value": group}
    controls.update(raw_controls or {})
    properties = {
        "Name": {"present": True, "value": name},
        "Writable": {"present": True, "value": writable},
        "WriteGroup": {"present": True, "value": group},
    }
    properties.update(extra or {})
    return {
        "entry_kind": "HASH", "properties": properties,
        "write_controls": controls, "unknown_properties": unknown or {},
    }


def table(rows=None, *, module="Exif", table_name="Main", resolved=True, unknown_table=None, dependency=False):
    table_controls = {
        "WRITABLE": {"present": False},
        "WRITE_GROUP": {"present": True, "value": "ExifIFD"},
        "WRITE_PROC": {"present": True, "value": {"__perl": "CODE"}},
        "CHECK_PROC": {"present": True, "value": {"__perl": "CODE"}},
    }
    result = {
        "module": module, "table": table_name, "full_name": f"Image::ExifTool::{module}::{table_name}",
        "table_properties": {
            "GROUPS": {"present": True, "value": {"0": "EXIF"}},
            "SET_GROUP1": {"present": True, "value": "IFD0"},
            "WRITE_GROUP": table_controls["WRITE_GROUP"],
            "WRITE_PROC": table_controls["WRITE_PROC"],
            "CHECK_PROC": table_controls["CHECK_PROC"],
        },
        "write_controls": table_controls,
        "unknown_table_properties": unknown_table or {},
        "rows": rows if rows is not None else {"316": row()},
    }
    result.update(procedures(resolved=resolved, dependency=dependency))
    return result


def document(tables=None, *, router_supported=True):
    return {
        "native_write_autoload": {
            "supported": router_supported,
            "router": code_fact("Image::ExifTool::DoAutoLoad", "router-body", dependencies={}),
        },
        "native_write_tables": tables if tables is not None else {"Exif": {"Main": table()}},
    }


def population(doc):
    # The renderer is intentionally a pure source compiler. These local
    # reverse checks inspect its unambiguous emitted literals rather than
    # reconstructing a Rust artifact by hand.
    source, report = write_descriptors.generate(doc)
    return source, report


class InactiveWriteDescriptorTests(unittest.TestCase):
    def test_compatible_source_row_emits_dynamic_name_id_placement_and_provenance(self):
        source, report = population(document({"Exif": {"Main": table(dependency=True)}}))
        self.assertEqual(report.emitted_tables, 1)
        self.assertEqual(report.emitted_rows, 1)
        self.assertIn('raw_id: 0x013c, name: "SourceName", physical_group: WritePhysicalGroup::IFD0', source)
        self.assertIn('name: "Image::ExifTool::Exif::WriteExif"', source)
        self.assertIn('name: "Image::ExifTool::Exif::Helper"', source)
        self.assertIn('INACTIVE_WRITE_RUNTIME_STATUS: &str = "inactive_source_candidates_no_writer_route"', source)
        self.assertIn('do not activate a writer or authenticate the', source)

    def test_dependency_binding_survives_anonymous_final_callable(self):
        changed = document()
        deps = changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["dependencies"]
        deps["Image::ExifTool::Exif::WriteHelper"] = code_fact(
            "Image::ExifTool::__ANON__", "anonymous-helper-body"
        )
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 1)
        self.assertIn(
            'binding: "Image::ExifTool::Exif::WriteHelper", fact: NativeWriteProcedureProvenance { name: "Image::ExifTool::__ANON__"',
            source,
        )

    def test_malformed_dependency_binding_refuses_candidate(self):
        changed = document()
        deps = changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["dependencies"]
        deps["not a callable binding"] = code_fact("Image::ExifTool::Exif::WriteHelper", "helper")
        source, report = population(changed)
        self.assertEqual((report.emitted_tables, report.emitted_rows), (0, 0))
        self.assertIn('"write_provenance_unresolved"', source)

    def test_name_and_compatible_new_row_follow_source_without_tag_allowlist(self):
        changed = document()
        changed["native_write_tables"]["Exif"]["Main"]["rows"] = {
            "317": row(name="UpgradeName"),
            "400": row(name="AdditionalSourceRow"),
        }
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 2)
        self.assertIn('raw_id: 0x013d, name: "UpgradeName"', source)
        self.assertIn('raw_id: 0x0190, name: "AdditionalSourceRow"', source)
        self.assertNotIn("HostComputer", source)

    def test_type_and_group_changes_are_explicit_refusals(self):
        changed = document()
        rows = changed["native_write_tables"]["Exif"]["Main"]["rows"]
        rows["316"] = row(writable="int16u")
        rows["317"] = row(name="Elsewhere", group="UnmodeledGroup")
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 0)
        self.assertEqual(report.omitted_rows, 2)
        self.assertIn('"write_value_type"', source)
        self.assertIn('"write_physical_group_unmodeled"', source)
        self.assertIn('"write_table_has_no_admitted_rows"', source)

    def test_ordinary_native_group_is_data_not_a_runtime_admission(self):
        changed = document()
        changed["native_write_tables"]["Exif"]["Main"]["rows"]["316"] = row(group="ExifIFD")
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 1)
        self.assertIn('physical_group: WritePhysicalGroup::ExifIFD', source)

    def test_table_write_group_default_is_source_placement(self):
        changed = document()
        candidate = changed["native_write_tables"]["Exif"]["Main"]["rows"]["316"]
        candidate["properties"].pop("WriteGroup")
        candidate["write_controls"]["WriteGroup"] = {"present": False}
        group_default = {"present": True, "value": "GPS"}
        changed["native_write_tables"]["Exif"]["Main"]["write_controls"]["WRITE_GROUP"] = group_default
        changed["native_write_tables"]["Exif"]["Main"]["table_properties"]["WRITE_GROUP"] = group_default
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 1)
        self.assertIn('physical_group: WritePhysicalGroup::GPS', source)

    def test_mismatched_source_projections_cannot_select_the_convenient_copy(self):
        changed = document()
        candidate = changed["native_write_tables"]["Exif"]["Main"]["rows"]["316"]
        candidate["properties"]["Writable"] = {"present": True, "value": "int16u"}
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 0)
        self.assertIn('"write_row_control_projection_mismatch"', source)

        changed = document()
        changed["native_write_tables"]["Exif"]["Main"]["table_properties"]["WRITE_GROUP"] = {
            "present": True, "value": "GPS"
        }
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 0)
        self.assertIn('"write_table_control_projection_mismatch"', source)

    def test_reference_shaped_group_is_not_collapsed_to_literal(self):
        changed = document()
        candidate = changed["native_write_tables"]["Exif"]["Main"]["rows"]["316"]
        candidate["properties"]["WriteGroup"] = {"present": True, "value": {"__ref": "SCALAR", "value": "IFD0"}}
        candidate["write_controls"]["WriteGroup"] = {"present": True, "value": {"__ref": "SCALAR", "value": "IFD0"}}
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 0)
        self.assertIn('"write_physical_group_missing_or_nonliteral"', source)

    def test_unknown_conversion_and_writer_controls_are_withheld(self):
        changed = document()
        changed["native_write_tables"]["Exif"]["Main"]["rows"]["316"] = row(
            extra={"RawConvInv": {"present": True, "value": "$val"}},
            raw_controls={"RawConvInv": {"present": True, "value": "$val"}},
            unknown={"FutureSwitch": {"present": True, "value": "1"}},
        )
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 0)
        self.assertIn('"write_row_property_RawConvInv"', source)
        self.assertIn('"write_row_control_RawConvInv"', source)
        self.assertIn('"write_row_unknown_property_FutureSwitch"', source)

    def test_variants_and_zero_row_tables_are_conserved_in_omissions(self):
        first = row(name="First")
        second = row(name="Second", writable="int16u")
        main = table(rows={"320": {"entry_kind": "ARRAY", "alternatives": [first, second]}})
        zero = table(rows={}, module="Exif", table_name="Zero")
        foreign = table(rows={"1": row(name="Foreign")}, module="Other", table_name="Main")
        doc = document({"Exif": {"Main": main, "Zero": zero}, "Other": {"Main": foreign}})
        source, report = population(doc)
        self.assertEqual(report.emitted_rows, 1)
        self.assertIn('raw_id: "320", variant: true, alternative: 1, name: Some("Second")', source)
        self.assertIn('OmittedWriteNativeTable { module: "Exif", table: "Zero"', source)
        self.assertIn('OmittedWriteNativeTable { module: "Other", table: "Main"', source)
        self.assertIn('"write_source_class_unimplemented"', source)

    def test_unresolved_provenance_and_router_are_named_refusals(self):
        unresolved = document({"Exif": {"Main": table(resolved=False)}})
        source, report = population(unresolved)
        self.assertEqual(report.emitted_rows, 0)
        self.assertIn('"write_provenance_unresolved"', source)
        router = document(router_supported=False)
        source, report = population(router)
        self.assertEqual(report.emitted_rows, 0)
        self.assertIn('"write_provenance_unresolved"', source)

    def test_procedure_body_change_changes_emitted_provenance(self):
        before, _ = population(document())
        changed = document()
        changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["__deparse"] = "changed-write-body"
        after, _ = population(changed)
        self.assertNotEqual(before, after)

    def test_malformed_procedure_path_or_digest_is_a_named_refusal(self):
        for key, value in (
            ("source_file", "../../untracked.pm"),
            ("source_file", "/absolute/Image/ExifTool/WriteExif.pl"),
            ("source_file", "untracked.pm"),
            ("source_sha256", "not-a-sha256"),
        ):
            with self.subTest(key=key, value=value):
                changed = document()
                changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"][key] = value
                source, report = population(changed)
                self.assertEqual(report.emitted_tables, 0)
                self.assertEqual(report.emitted_rows, 0)
                self.assertIn('"write_provenance_unresolved"', source)
                self.assertNotIn(value, source)

    def test_outer_and_inner_table_identity_must_match_before_source_class_selection(self):
        # A stale/mutated inner record cannot borrow Exif::Main eligibility
        # merely by claiming that identity while living under another map key.
        cases = {
            "forged_inner_claim": document({"Other": {"Elsewhere": table(module="Exif", table_name="Main")}}),
            "mutated_saved_exif_main": document(),
        }
        mutated = cases["mutated_saved_exif_main"]["native_write_tables"]["Exif"]["Main"]
        mutated.update({
            "module": "Other", "table": "Elsewhere", "full_name": "Image::ExifTool::Other::Elsewhere",
        })
        for label, changed in cases.items():
            with self.subTest(label=label):
                source, report = population(changed)
                self.assertEqual(report.emitted_tables, 0)
                self.assertEqual(report.emitted_rows, 0)
                self.assertIn('"write_table_identity_mismatch"', source)
                self.assertNotIn('pub static WRITE_EXIF_MAIN:', source)

    def test_effective_groups_are_emitted_and_unrepresentable_groups_refuse(self):
        before, _ = population(document())
        changed = document()
        groups = changed["native_write_tables"]["Exif"]["Main"]["table_properties"]["GROUPS"]
        groups["value"] = {"0": "ChangedExif", "1": "ChangedIFD", "2": "ChangedImage"}
        after, report = population(changed)
        self.assertEqual(report.emitted_rows, 1)
        self.assertIn('group0: "ChangedExif", group1: "ChangedIFD", group2: "ChangedImage"', after)
        self.assertNotEqual(before, after)

        changed["native_write_tables"]["Exif"]["Main"]["table_properties"]["GROUPS"] = {
            "present": True, "value": {"3": "Unexpected"},
        }
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 0)
        self.assertIn('"write_table_groups_unrepresented"', source)

    def test_default_optional_write_output_does_not_change_primary_artifact(self):
        # The optional, inactive sidecar must not perturb ordinary codegen
        # output.  This invokes the public CLI with the same minimal input
        # twice rather than treating this module's renderer as a substitute.
        payload = document()
        payload.update({"exiftool_version": "13.59", "modules": {}})
        with TemporaryDirectory() as directory:
            root = Path(directory)
            tables = root / "tables.json"
            tables.write_text(json.dumps(payload), encoding="utf-8")
            # `-o` names a mod.rs hub (one file per ExifTool module beside it).
            ordinary = root / "ordinary" / "mod.rs"
            with_sidecar = root / "with-sidecar" / "mod.rs"
            sidecar = root / "inactive_write.rs"
            common = [sys.executable, str(Path(__file__).with_name("codegen.py")), str(tables)]
            subprocess.run([*common, "-o", str(ordinary)], check=True, text=True, capture_output=True)
            subprocess.run(
                [*common, "-o", str(with_sidecar), "--write-out", str(sidecar)],
                check=True, text=True, capture_output=True,
            )
            self.assertEqual(ordinary.read_bytes(), with_sidecar.read_bytes())
            self.assertTrue(sidecar.is_file())
            self.assertIn("INACTIVE_WRITE_DESCRIPTOR_VERSION", sidecar.read_text(encoding="utf-8"))

    def test_symbolic_write_alias_is_a_named_row_omission_not_a_dump_abort(self):
        changed = document()
        changed["native_write_tables"]["Exif"]["Main"]["rows"]["Alias"] = row(name="SourceAlias")
        source, report = population(changed)
        self.assertEqual(report.emitted_rows, 1)
        self.assertEqual(report.omitted_rows, 1)
        self.assertIn('raw_id: "Alias"', source)
        self.assertIn('"write_raw_id"', source)

    def test_malformed_native_facts_fail_loudly_not_as_empty_candidates(self):
        malformed = document()
        del malformed["native_write_tables"]["Exif"]["Main"]["rows"]["316"]["write_controls"]["Writable"]["present"]
        with self.assertRaisesRegex(write_descriptors.WriteDescriptorError, "Writable.present"):
            population(malformed)


if __name__ == "__main__":
    unittest.main()
