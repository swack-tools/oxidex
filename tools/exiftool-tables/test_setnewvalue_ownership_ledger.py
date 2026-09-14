"""Regression coverage for persisted source-selected writer ownership."""
from copy import deepcopy
import unittest

from checkexif_recipes import RecipeRefused
from setnewvalue_ownership_ledger import build_ledger, owned_names, qualified_ownership, validate_ledger
from test_setnewvalue_addressing import refresh_find_tag_info_warmup, source


def rename(document, name):
    row = document["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
    row["properties"]["Name"]["value"] = name
    row["effective_properties"]["Name"]["value"] = name
    refresh_find_tag_info_warmup(document)


class SetNewValueOwnershipLedgerTests(unittest.TestCase):
    def bootstrap(self):
        return build_ledger(source(), None, bootstrap=True)

    def test_missing_initial_ledger_requires_explicit_bootstrap(self):
        with self.assertRaisesRegex(RecipeRefused, "bootstrap must be explicit"):
            build_ledger(source(), None, bootstrap=False)
        ledger = self.bootstrap()
        self.assertEqual(owned_names(ledger), frozenset({"noallowlist"}))
        self.assertEqual(qualified_ownership(ledger)[0].removed, False)

    def test_rename_keeps_old_name_terminal_and_records_current_name(self):
        first = self.bootstrap()
        changed = source()
        rename(changed, "NewName")
        ledger = build_ledger(changed, first, bootstrap=False)
        entries = {entry.name: entry for entry in qualified_ownership(ledger)}
        self.assertEqual(set(entries), {"noallowlist", "newname"})
        self.assertTrue(entries["noallowlist"].removed)
        self.assertFalse(entries["newname"].removed)
        self.assertEqual(owned_names(ledger), frozenset({"noallowlist", "newname"}))

    def test_type_change_is_current_not_an_ownership_retirement(self):
        first = self.bootstrap()
        changed = source()
        row = changed["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        row["properties"]["Writable"]["value"] = "int16u"
        row["effective_properties"]["Writable"]["value"] = "int16u"
        row["write_controls"]["Writable"]["value"] = "int16u"
        ledger = build_ledger(changed, first, bootstrap=False)
        entry = qualified_ownership(ledger)[0]
        self.assertFalse(entry.removed)
        self.assertEqual(len(validate_ledger(ledger)["entries"][0]["history"]), 2)

    def test_removal_is_explicit_and_prior_identity_or_digest_tampering_refuses(self):
        first = self.bootstrap()
        removed = source()
        del removed["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        refresh_find_tag_info_warmup(removed)
        ledger = build_ledger(removed, first, bootstrap=False)
        entry = qualified_ownership(ledger)[0]
        self.assertTrue(entry.removed)
        self.assertIn("noallowlist", owned_names(ledger))

        tampered = deepcopy(first)
        tampered["entries"][0]["name"] = "forged"
        with self.assertRaisesRegex(RecipeRefused, "digest"):
            build_ledger(source(), tampered, bootstrap=False)
        tampered = deepcopy(first)
        tampered["source_identity"] = "0" * 64
        # Rehashing a payload cannot invent a valid authenticated source identity.
        from setnewvalue_ownership_ledger import _digest
        payload = {key: value for key, value in tampered.items() if key != "ledger_sha256"}
        tampered["ledger_sha256"] = _digest(payload)
        with self.assertRaisesRegex(RecipeRefused, "source identity"):
            build_ledger(source(), tampered, bootstrap=False)
        tampered = deepcopy(first)
        source_id = tampered["source_identity"]
        tampered["sources"][source_id]["exiftool_version"] = "forged-release"
        payload = {key: value for key, value in tampered.items() if key != "ledger_sha256"}
        tampered["ledger_sha256"] = _digest(payload)
        with self.assertRaisesRegex(RecipeRefused, "source.*identity"):
            build_ledger(source(), tampered, bootstrap=False)


if __name__ == "__main__":
    unittest.main()
