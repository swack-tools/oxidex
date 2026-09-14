"""Source mechanism tests, not native parity or executable admission proof."""
import hashlib
import unittest

from checkexif_recipes import RecipeRefused, RecipeMalformed
from sanitize_recipes import _CANONICAL, compile_sanitize


def source_fact(body=_CANONICAL):
    # Synthetic unit input has a real digest of its synthetic source. Native
    # capture is separately checked using the official dump_tables.pl producer.
    bindings = ("Encode::encode", "Encode::is_utf8",
                "Image::ExifTool::XMP::UnescapeXML", "Image::ExifTool::HTML::UnescapeHTML")

    def unresolved(name):
        return {"__perl": "CODE", "__name": name, "resolved": False,
                "source_file": None, "source_sha256": None,
                "__deparse": None, "reason": "unit_fixture_unavailable"}

    return {
        "__perl": "CODE", "resolved": True,
        "__name": "Image::ExifTool::Sanitize",
        "requested_binding": "Image::ExifTool::Sanitize",
        "__deparse": body, "source_file": "Image/ExifTool/Writer.pl",
        "source_sha256": hashlib.sha256(body.encode()).hexdigest(),
        "dependencies": {name: unresolved(name) for name in bindings},
        "callback_references": {"Image::ExifTool::SetWarning": unresolved("Image::ExifTool::SetWarning")},
    }


class SanitizeSourceMechanism(unittest.TestCase):
    def test_source_guards_change_operands(self):
        for old, new, field, expected in (
            ("5.006", "10.006", "downgrade_at_or_after", 10_006_000),
            ("5.01", "5.008", "manual_pack_before", 5_008_000),
        ):
            with self.subTest(field=field):
                changed = compile_sanitize(source_fact(_CANONICAL.replace(old, new)))
                self.assertEqual(getattr(changed, field), expected)

    def test_unresolved_branches_remain_evidence_without_admission(self):
        fact = source_fact()
        recipe = compile_sanitize(fact)
        self.assertEqual(recipe.encode_name, "utf8")
        self.assertEqual(recipe.raw_dependencies, fact["dependencies"])
        self.assertEqual(recipe.callback_references, fact["callback_references"])
        fact["dependencies"]["Encode::encode"]["resolved"] = True
        fact["callback_references"].clear()
        self.assertFalse(recipe.raw_dependencies["Encode::encode"]["resolved"])
        self.assertIn("Image::ExifTool::SetWarning", recipe.callback_references)

    def test_explicit_argument_call_spellings_match(self):
        for binding in source_fact()["dependencies"]:
            ordinary = _CANONICAL.replace("&" + binding + "(", binding + "(")
            ampersand = ordinary.replace(binding + "(", "&" + binding + "(")
            a, b = (compile_sanitize(source_fact(body)) for body in (ordinary, ampersand))
            self.assertEqual(a.encode_name, b.encode_name)
            self.assertEqual(a.downgrade_at_or_after, b.downgrade_at_or_after)
            self.assertNotEqual(a.provenance.body_sha256, b.provenance.body_sha256)

    def test_unknown_executable_changes_refuse(self):
        for body in (
            _CANONICAL.replace("'utf8'", "'latin1'"),
            _CANONICAL.replace("'C*'", "'U*'"),
            _CANONICAL.replace("$] >=", "$] <="),
            _CANONICAL.replace("Encode::is_utf8($$valPt)", "&Encode::is_utf8"),
            _CANONICAL.replace("ref($$valPt) eq 'SCALAR'", "ref($$valPt) eq 'ARRAY'"),
            _CANONICAL.replace("(require Encode);", "(require Encode); die 'changed';"),
        ):
            with self.subTest(body=body), self.assertRaises(RecipeRefused):
                compile_sanitize(source_fact(body))

    def test_missing_or_malformed_dependency_evidence_refuses(self):
        for field in ("dependencies", "callback_references"):
            for value in ({}, None, {"unknown": {}}, {"unknown": "CODE"}):
                fact = source_fact()
                fact[field] = value
                with self.subTest(field=field, value=value), self.assertRaises(RecipeRefused):
                    compile_sanitize(fact)

    def test_root_source_and_binding_still_required(self):
        for field, value in (("resolved", False), ("__deparse", None),
                             ("requested_binding", "Image::ExifTool::Other")):
            fact = source_fact()
            fact[field] = value
            with self.subTest(field=field), self.assertRaises(RecipeRefused):
                compile_sanitize(fact)

    def test_resolved_dependency_requires_source_provenance(self):
        for field, binding in (("dependencies", "Encode::encode"),
                               ("callback_references", "Image::ExifTool::SetWarning")):
            fact = source_fact()
            fact[field][binding] = {"__perl": "CODE", "resolved": True,
                                    "__name": "Other::fake", "__deparse": "bad",
                                    "source_file": None, "source_sha256": None}
            with self.subTest(field=field), self.assertRaises((RecipeRefused, RecipeMalformed)):
                compile_sanitize(fact)

    def test_valid_resolved_callback_preserves_unresolved_child(self):
        fact = source_fact()
        callback = source_fact("($) { return undef; }")
        callback["__name"] = "Image::ExifTool::SetWarning"
        callback.pop("requested_binding")
        callback.pop("callback_references")
        fact["callback_references"]["Image::ExifTool::SetWarning"] = callback
        recipe = compile_sanitize(fact)
        captured = recipe.callback_references["Image::ExifTool::SetWarning"]
        self.assertTrue(captured["resolved"])
        self.assertFalse(captured["dependencies"]["Encode::encode"]["resolved"])

    def test_unresolved_dependency_requires_a_reason_and_no_claimed_source(self):
        for key, value in (("reason", None), ("source_sha256", "a" * 64),
                           ("resolved", "false"), ("__name", "not qualified")):
            fact = source_fact()
            fact["dependencies"]["Encode::encode"][key] = value
            with self.subTest(key=key), self.assertRaises(RecipeRefused):
                compile_sanitize(fact)


if __name__ == "__main__":
    unittest.main()
