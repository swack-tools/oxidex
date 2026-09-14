"""Source-selected ordinary EXIF addressing tests."""
from copy import deepcopy
from dataclasses import replace
import hashlib
import json
from pathlib import Path
import unittest

from checkexif_recipes import RecipeRefused
from convinv_rows import compile_rows
from setnewvalue_addressing import _observation_capture, compile_addressing, resolve, resolve_batch
from test_checkexif_recipes import fact
from test_convinv_rows import ready


ROOT = Path(__file__).parent
SETNEW = json.loads((ROOT / "setnewvalue_convinv_full_template.json").read_text())
FIND = json.loads((ROOT / "findtaginfo_full_template.json").read_text())


def closure_manifest(modules=None):
    modules = modules or [{"inc": "Image/ExifTool.pm", "source_file": "Image/ExifTool.pm",
                           "source_sha256": "c" * 64}]
    encoded = json.dumps(modules, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    return {"sha256": hashlib.sha256(encoded).hexdigest(), "modules": modules}


def source():
    value = ready()
    helpers = value["native_write_helpers"]
    conv = fact("Image::ExifTool::ConvInv", "return;", source="Image/ExifTool/Writer.pl")
    helpers["set_new_value"] = fact(
        "Image::ExifTool::SetNewValue", " ".join(SETNEW),
        requested="Image::ExifTool::SetNewValue", source="Image/ExifTool/Writer.pl")
    helpers["set_new_value"]["dependencies"] = {"Image::ExifTool::ConvInv": conv}
    helpers["find_tag_info"] = fact(
        "Image::ExifTool::TagLookup::FindTagInfo", " ".join(FIND),
        requested="Image::ExifTool::TagLookup::FindTagInfo", source="Image/ExifTool/TagLookup.pm")
    table = value["native_write_tables"]["Exif"]["Main"]
    table["table_properties"]["GROUPS"] = {
        "present": True, "value": {"0": "EXIF", "1": "IFD0", "2": "Image"}}
    value["exiftool_version"] = "13.59"
    source_hashes = {
        "Image/ExifTool.pm": "c" * 64,
        "Image/ExifTool/WriteExif.pl": "b" * 64,
        "Image/ExifTool/Writer.pl": "a" * 64,
        "Image/ExifTool/Exif.pm": "d" * 64,
    }
    closure_modules = [{"inc": path, "source_file": path, "source_sha256": digest}
                       for path, digest in sorted(source_hashes.items())]
    value["native_capture_context"] = {
        "schema": "native_exiftool_capture_context_v1",
        "selected_library": "/selected/exiftool/lib",
        "perl_path": "/selected/perl",
        "perl_version": "5.038002",
        "exiftool_version": "13.59",
        "loaded_closure": closure_manifest(closure_modules),
    }
    table["effective_write_proc"] = {"present": True, "effective": fact(
        "Image::ExifTool::Exif::WriteExif", "return;",
        source="Image/ExifTool/WriteExif.pl")}
    table["effective_write_proc"]["effective"]["source_sha256"] = source_hashes["Image/ExifTool/WriteExif.pl"]
    value["native_write_capture_context"] = {
        "kind": "write_exif_postload_context_v1", "resolved": True,
        "loaded_modules": source_hashes,
    }
    value["native_write_format_registry"] = {
        "state": "resolved",
        "source": {"library_relative_path": "Image/ExifTool/Exif.pm",
                   "sha256": source_hashes["Image/ExifTool/Exif.pm"]},
    }
    value["native_find_tag_info_warmup"] = {
        "warmed": True, "query_name_count": 1,
        "query_names_sha256": hashlib.sha256(b'["noallowlist"]').hexdigest(),
    }
    return value


def refresh_find_tag_info_warmup(value):
    """Model a fresh native capture after changing source-owned names."""
    from setnewvalue_addressing import _owned_names, _query_names_digest
    names = frozenset(_owned_names(value))
    value["native_find_tag_info_warmup"] = {
        "warmed": True, "query_name_count": len(names),
        "query_names_sha256": _query_names_digest(names),
    }


def observations(rows, *, external=False, addressing=None):
    row = rows[0]
    candidate = {"module": row.module, "table": row.table, "full_name": row.full_name,
                 "raw_id": row.raw_id, "name": row.name,
                 "groups": {"0": "EXIF", "1": "IFD0", "2": "Image"}}
    values = [candidate]
    if external:
        values.append({"external_table": "Image::ExifTool::XMP::Main", "raw_id": "other",
                       "name": row.name, "groups": {"0": "XMP", "1": "XMP", "2": "Image"}})
    # The source rows are compiled again so this test fixture carries the same
    # sealed native-capture context as the real probe input.
    if addressing is None:
        addressing, _ = compile_addressing(source())
    capture = _observation_capture(addressing)
    context = capture["native_capture_context"]
    return {"schema": "native_setnewvalue_addressing_v2", "capture": capture,
            "runtime": {**{key: context[key] for key in ("selected_library", "perl_path", "perl_version", "exiftool_version")},
                        "loaded_closure": deepcopy(context["loaded_closure"]),
                        "helpers": {"find_tag_info": capture["find_tag_info"],
                                    "set_new_value": capture["set_new_value"]}},
            "queries": {row.name.lower(): {"query": row.name, "candidates": values}}}


class SetNewValueAddressingTests(unittest.TestCase):
    def compiled(self):
        addressing, report = compile_addressing(source())
        self.assertEqual(report["rows_emitted"], 1)
        self.assertEqual(addressing.rows[0].name, "NoAllowlist")
        return addressing

    def test_case_insensitive_name_and_exif_ifd0_qualifiers_resolve_identity(self):
        addressing = self.compiled()
        native = observations(addressing.rows)
        expected = addressing.rows[0].identity
        for spelling in ("NoAllowlist", "nOaLlOwLiSt", "EXIF:NoAllowlist", "ifd0:nOaLlOwLiSt"):
            with self.subTest(spelling=spelling):
                result = resolve(addressing, native, spelling)
                self.assertEqual(result.state, "resolved")
                self.assertEqual(result.row.identity, expected)

    def test_ambiguity_and_external_native_candidates_are_terminal(self):
        addressing = self.compiled()
        native = observations(addressing.rows, external=True)
        result = resolve(addressing, native, "NoAllowlist")
        self.assertEqual(result.state, "owned_unsupported")
        self.assertIn("outside generated", result.reason)

        duplicate_row = replace(addressing.rows[0], raw_id="other")
        addressing = replace(addressing, rows=(addressing.rows[0], duplicate_row))
        native = observations(addressing.rows)
        duplicate = deepcopy(native["queries"]["noallowlist"]["candidates"][0])
        duplicate["raw_id"] = duplicate_row.raw_id
        native["queries"]["noallowlist"]["candidates"].append(duplicate)
        result = resolve(addressing, native, "NoAllowlist")
        self.assertEqual(result.state, "owned_unsupported")
        self.assertIn("ambiguous", result.reason)

    def test_owned_unsupported_never_becomes_outside_scope(self):
        addressing = self.compiled()
        native = observations(addressing.rows)
        self.assertEqual(resolve(addressing, native, "ExifIFD:NoAllowlist").state, "owned_unsupported")
        self.assertEqual(resolve(addressing, native, "UnknownTag").state, "outside_migrated_scope")
        unavailable = observations(addressing.rows)
        unavailable["queries"] = {}
        with self.assertRaisesRegex(RecipeRefused, "query set"):
            resolve(addressing, unavailable, "NoAllowlist")

    def test_explicit_ifd_destination_does_not_replace_source_preference(self):
        addressing = self.compiled()
        native = observations(addressing.rows)
        selected = resolve(addressing, native, "IFD1:NoAllowlist")
        self.assertEqual(selected.state, "resolved")
        self.assertEqual(selected.row.write_group, "IFD0")
        self.assertEqual(selected.selected_group, "IFD1")
        accepted, failures = resolve_batch(addressing, native, {"IFD0:NoAllowlist": "one", "IFD1:NoAllowlist": "two"})
        self.assertEqual(failures, ())
        self.assertEqual({item.selected_group for item, value in accepted}, {"IFD0", "IFD1"})

    def test_aliases_deduplicate_and_conflicts_refuse_same_physical_field(self):
        addressing = self.compiled()
        native = observations(addressing.rows)
        accepted, failures = resolve_batch(addressing, native, {
            "NoAllowlist": "one", "IFD0:noallowlist": "one"})
        self.assertEqual(len(accepted), 1)
        self.assertEqual(failures, ())
        accepted, failures = resolve_batch(addressing, native, {
            "NoAllowlist": "one", "EXIF:noallowlist": "two"})
        self.assertEqual(accepted, ())
        self.assertEqual(len(failures), 1)
        self.assertIn("conflicting duplicate", failures[0].reason)

    def test_explicit_unmigrated_group_is_outside_scope_despite_bare_name_collision(self):
        addressing = self.compiled()
        native = observations(addressing.rows, external=True)
        self.assertEqual(resolve(addressing, native, "XMP:NoAllowlist").state,
                         "outside_migrated_scope")
        # The same spelling without an explicit native group remains terminal:
        # it is ambiguous between a migrated source field and XMP.
        self.assertEqual(resolve(addressing, native, "NoAllowlist").state,
                         "owned_unsupported")
        self.assertEqual(resolve(addressing, native, "UnknownGroup:NoAllowlist").state,
                         "owned_unsupported")

    def test_native_xmp_software_does_not_claim_qualified_exif_ownership(self):
        document = source()
        raw = document["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        raw["properties"]["Name"]["value"] = "Software"
        raw["effective_properties"]["Name"]["value"] = "Software"
        refresh_find_tag_info_warmup(document)
        addressing, _ = compile_addressing(document)
        native = observations(addressing.rows, external=True, addressing=addressing)
        self.assertEqual(resolve(addressing, native, "EXIF:Software").state, "resolved")
        self.assertEqual(resolve(addressing, native, "XMP:Software").state,
                         "outside_migrated_scope")

    def test_qualified_external_candidate_stays_outside_when_source_recipe_is_omitted(self):
        addressing = self.compiled()
        native = observations(addressing.rows, external=True)
        native["queries"]["noallowlist"]["candidates"] = [
            native["queries"]["noallowlist"]["candidates"][-1]]
        omitted = replace(addressing, rows=())
        self.assertEqual(resolve(omitted, native, "XMP:NoAllowlist").state,
                         "outside_migrated_scope")
        self.assertEqual(resolve(omitted, native, "EXIF:NoAllowlist").state,
                         "owned_unsupported")

    def test_source_name_and_group_changes_do_not_retain_old_address_recipe(self):
        changed = source()
        table = changed["native_write_tables"]["Exif"]["Main"]
        table["table_properties"]["GROUPS"]["value"]["1"] = "Other"
        addressing, report = compile_addressing(changed)
        self.assertEqual(addressing.rows, ())
        self.assertEqual(report["rows_omitted"], 1)
        result = resolve(addressing, {}, "NoAllowlist")
        self.assertEqual(result.state, "owned_unsupported")

        changed = source()
        changed["native_write_helpers"]["find_tag_info"]["__deparse"] = " ".join(FIND).replace("lc", "uc", 1)
        with self.assertRaisesRegex(RecipeRefused, "FindTagInfo"):
            compile_addressing(changed)

    def test_address_rows_are_projected_from_source_rows_not_a_name_or_id_list(self):
        doc = source()
        raw = doc["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        raw["properties"]["Name"]["value"] = "ChangedName"
        raw["effective_properties"]["Name"]["value"] = "ChangedName"
        refresh_find_tag_info_warmup(doc)
        addressing, _report = compile_addressing(doc)
        self.assertEqual(addressing.rows[0].name, "ChangedName")
        self.assertEqual(addressing.rows[0].raw_id, "raw-not-name")

    def test_stale_same_name_lookup_and_mutated_probe_rows_refuse_before_resolution(self):
        addressing = self.compiled()
        native = observations(addressing.rows)
        native["runtime"]["helpers"]["find_tag_info"] = {
            **native["runtime"]["helpers"]["find_tag_info"], "body_sha256": "d" * 64}
        with self.assertRaisesRegex(RecipeRefused, "find_tag_info identity"):
            resolve(addressing, native, "NoAllowlist")

        native = observations(addressing.rows)
        native["capture"]["source_rows_sha256"] = "e" * 64
        with self.assertRaisesRegex(RecipeRefused, "source rows/helpers"):
            resolve(addressing, native, "NoAllowlist")

        native = observations(addressing.rows)
        closure = native["runtime"]["loaded_closure"]
        closure["modules"][0]["source_sha256"] = "f" * 64
        closure["sha256"] = closure_manifest(closure["modules"])["sha256"]
        with self.assertRaisesRegex(RecipeRefused, "does not match the dump capture"):
            resolve(addressing, native, "NoAllowlist")

    def test_query_set_and_changed_source_name_are_not_reused(self):
        addressing = self.compiled()
        native = observations(addressing.rows)
        native["queries"]["changedname"] = native["queries"].pop("noallowlist")
        with self.assertRaisesRegex(RecipeRefused, "query set"):
            resolve(addressing, native, "NoAllowlist")

        changed = source()
        raw = changed["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        raw["properties"]["Name"]["value"] = "ChangedName"
        raw["effective_properties"]["Name"]["value"] = "ChangedName"
        refresh_find_tag_info_warmup(changed)
        changed_addressing, _ = compile_addressing(changed)
        stale = observations(addressing.rows)
        with self.assertRaisesRegex(RecipeRefused, "source rows/helpers"):
            resolve(changed_addressing, stale, "ChangedName")


if __name__ == "__main__":
    unittest.main()


class ExplicitDirectoryCaptureTests(unittest.TestCase):
    def test_changed_caller_and_mixed_closures_cannot_emit_directory_authority(self):
        from scalar_helper_codegen import generate
        original = source()
        numeric = json.loads((ROOT / 'testdata/numeric_scalar_source.json').read_text())
        for key in ('write_value', 'check_value'):
            original['native_write_helpers'][key] = numeric['native_write_helpers'][key]
        rendered, report = generate(original)
        self.assertEqual(report['helpers']['explicit_directories']['state'], 'compiled')
        for mutation in ('body', 'writer', 'generic'):
            changed = deepcopy(original)
            if mutation == 'body':
                changed['native_write_helpers']['set_new_value']['__deparse'] += ' $val += 1;'
            elif mutation == 'writer':
                changed['native_write_capture_context']['loaded_modules']['Image/ExifTool/Writer.pl'] = 'f' * 64
            else:
                modules = changed['native_capture_context']['loaded_closure']['modules']
                next(item for item in modules if item['inc'] == 'Image/ExifTool/Writer.pl')['source_sha256'] = 'f' * 64
                changed['native_capture_context']['loaded_closure'] = closure_manifest(modules)
            rendered, report = generate(changed)
            with self.subTest(mutation=mutation):
                self.assertEqual(report['helpers']['explicit_directories']['state'], 'unsupported')
                self.assertIn('PUBLIC_SET_NEW_VALUE_CALLER: Option<crate::writers::generated_scalar::PublicSetNewValueCallerRecipe> = None', rendered)
