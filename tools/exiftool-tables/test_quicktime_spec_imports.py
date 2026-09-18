"""The QuickTime Keys/UserData spec files import only the shared types they use.

The 11.78/12.64 upgrade rehearsal (F5) regenerated both files with every row
refused -- ProcessMOV/ProcessKeys changed, so the whole protocol is withheld --
and each still imported `EnumOperand` and `SourceFormat` from
generated_itemlist_specs.rs. That is two `unused_imports` warnings, which CI's
`cargo clippy -- -D warnings` rejects. The instrument here is rustc itself: the
rendered files are compiled beside the committed generated_itemlist_specs.rs
with `-D unused_imports`.
"""
from __future__ import annotations

import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

import quicktime_generated_specs as shared
import quicktime_keys_specs as keys
import quicktime_userdata_specs as userdata

ITEMLIST_RS = keys.ROOT / "src/parsers/quicktime/generated_itemlist_specs.rs"


def refused_keys():
    document = json.loads(keys.SNAPSHOT.read_text())
    document["modules"]["QuickTime"]["tables"]["Keys"]["meta"]["PROCESS_PROC"]["__deparse"] += " changed"
    return keys.compile_document(document)


def refused_userdata():
    document = json.loads(userdata.SNAPSHOT.read_text())
    document["modules"]["QuickTime"]["tables"]["UserData"]["meta"]["PROCESS_PROC"]["__deparse"] += " changed"
    return userdata.compile_document(document)


def use_lines(rust: str) -> list[str]:
    return [line for line in rust.splitlines() if line.startswith("use ")]


class ItemListUseTests(unittest.TestCase):
    def test_imports_exactly_the_names_the_code_uses(self):
        self.assertEqual(shared.itemlist_use("static X: &[u8] = &[];"), [])
        self.assertEqual(shared.itemlist_use("pub data: ItemListSpec,"),
                         ["use super::generated_itemlist_specs::ItemListSpec;"])
        self.assertEqual(shared.itemlist_use("ItemListSpec { source_format: SourceFormat::String }"),
                         ["use super::generated_itemlist_specs::{ItemListSpec, SourceFormat};"])

    def test_a_type_name_inside_a_string_literal_is_not_a_use(self):
        body = 'pub data: ItemListSpec,\n    "SourceFormat", "EnumOperand { raw: \\"x\\" }",'
        self.assertEqual(shared.itemlist_use(body), ["use super::generated_itemlist_specs::ItemListSpec;"])


class RenderedImportTests(unittest.TestCase):
    def test_pinned_release_keeps_every_import(self):
        # 13.59 renders all three, byte-identical to the committed files.
        for module, path in ((keys, keys.RUST), (userdata, userdata.RUST)):
            with self.subTest(module.__name__):
                rust = module.render_rust(module.compile_document(json.loads(module.SNAPSHOT.read_text())))
                self.assertEqual(rust, path.read_text())
                self.assertEqual(use_lines(rust),
                                 ["use super::generated_itemlist_specs::{EnumOperand, ItemListSpec, SourceFormat};"])

    def test_every_row_refused_imports_only_the_struct_type(self):
        for label, module, result in (("keys", keys, refused_keys()), ("userdata", userdata, refused_userdata())):
            with self.subTest(label):
                self.assertEqual(result["specs"], [])
                rust = module.render_rust(result)
                self.assertEqual(use_lines(rust), ["use super::generated_itemlist_specs::ItemListSpec;"])
                formatted = subprocess.run(["rustfmt", "--edition", "2024", "--emit", "stdout"], input=rust,
                                           text=True, capture_output=True, check=True).stdout
                self.assertEqual(formatted, rust)


@unittest.skipUnless(shutil.which("rustc"), "no rustc")
class RustcUnusedImportTests(unittest.TestCase):
    def compile(self, keys_rs: str, userdata_rs: str) -> subprocess.CompletedProcess:
        with tempfile.TemporaryDirectory(prefix="oxidex-qt-imports-") as directory:
            root = Path(directory)
            (root / "generated_itemlist_specs.rs").write_text(ITEMLIST_RS.read_text())
            (root / "generated_keys_specs.rs").write_text(keys_rs)
            (root / "generated_userdata_specs.rs").write_text(userdata_rs)
            (root / "lib.rs").write_text("mod generated_itemlist_specs;\nmod generated_keys_specs;\n"
                                         "mod generated_userdata_specs;\n")
            return subprocess.run(["rustc", "--edition", "2024", "--crate-type", "lib", "--emit", "metadata",
                                   "-A", "dead_code", "-D", "unused_imports", "--out-dir", str(root),
                                   str(root / "lib.rs")], capture_output=True, text=True)

    def test_refused_and_pinned_renders_compile_without_unused_imports(self):
        pinned = self.compile(keys.RUST.read_text(), userdata.RUST.read_text())
        self.assertEqual(pinned.returncode, 0, pinned.stderr)
        refused = self.compile(keys.render_rust(refused_keys()), userdata.render_rust(refused_userdata()))
        self.assertEqual(refused.returncode, 0, refused.stderr)


if __name__ == "__main__":
    unittest.main()
