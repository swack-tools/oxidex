"""Regression coverage for ProcessBinaryData sub-directory transcription."""
import unittest

import codegen_subdirs as generator


def fixture():
    return {
        "exiftool_version": "13.59",
        "modules": {
            "Pentax": {
                "tables": {
                    "AFInfo": {
                        "meta": {
                            "PROCESS_PROC": {
                                "__name": "Image::ExifTool::ProcessBinaryData"
                            },
                            "FORMAT": "int8u",
                        },
                        "tags": {
                            "11": {
                                "Name": "AFPointsInFocus",
                                "Condition": "$$self{Model} !~ /(K-(1|3|70|S1|S2)|KP)\\b/",
                                "PrintConv": {
                                    "kind": "enum",
                                    "directives": None,
                                    "map": {"0": "None", "15": "Lower-right, Mid-right"},
                                },
                            }
                        },
                    }
                }
            }
        },
    }


class PrintConvColumnsTests(unittest.TestCase):
    def render(self, document):
        pool = generator.ConstPool("PENTAX")
        return generator.gen_table(
            "Pentax", "AFInfo", document["modules"]["Pentax"]["tables"]["AFInfo"],
            pool, [], False, "super::print_conv", "super::value_conv",
        )[1]

    def test_positive_display_columns_preserve_reader_projection(self):
        baseline = self.render(fixture())
        for value in (2, "2", "3"):
            with self.subTest(value=value):
                document = fixture()
                document["modules"]["Pentax"]["tables"]["AFInfo"]["tags"]["11"]["PrintConvColumns"] = value
                rendered = self.render(document)
                self.assertEqual(rendered, baseline)
                self.assertIn('name: "AFPointsInFocus"', rendered)

    def test_malformed_display_columns_refuse(self):
        for value in (True, 0, -1, "0", "02", "two", {"__perl": "CODE"}, 1 << 32):
            with self.subTest(value=value):
                document = fixture()
                document["modules"]["Pentax"]["tables"]["AFInfo"]["tags"]["11"]["PrintConvColumns"] = value
                with self.assertRaisesRegex(generator.Unsupported, "invalid PrintConvColumns"):
                    self.render(document)


if __name__ == "__main__":
    unittest.main()
