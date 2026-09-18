"""Pin the relation between the accepted shared serial-processor grammars.

Each grammar is a complete token stream, so a reviewer cannot see from the
literals alone how two of them differ. These checks make that difference a
fixed, named fact: V0 is V1 with exactly the option-gated ``TAG_EXTRA`` stores
removed, and nothing else.
"""

import unittest

from serial_processor_grammar import (
    SERIAL_PROCESSOR_GRAMMARS,
    SERIAL_PROCESSOR_V0_TOKENS,
    SERIAL_PROCESSOR_V1_TOKENS,
)

# `($count and (my($key) = $et->FoundTag($tagInfo, $val)));` followed by the
# `if ($key) { SaveFormat -> G6; SaveBin -> BinVal }` block, as V1 tokenizes it.
V1_ONLY_SPAN = (
    "(", "my", "(", "$key", ")", "=", "$et", "->", "FoundTag", "(", "$tagInfo", ",", "$val", ")", ")", ")", ";",
    "if", "(", "$key", ")", "{",
    "(", "$et", "->", "{", "'OPTIONS'", "}", "{", "'SaveFormat'", "}", "and",
    "(", "$et", "->", "{", "'TAG_EXTRA'", "}", "{", "$key", "}", "{", "'G6'", "}", "=", "$format", ")", ")", ";",
    "(", "$et", "->", "{", "'OPTIONS'", "}", "{", "'SaveBin'", "}", "and",
    "(", "$et", "->", "{", "'TAG_EXTRA'", "}", "{", "$key", "}", "{", "'BinVal'", "}", "=",
    "substr", "(", "$$dataPt", ",", "(", "$pos", "+", "$offset", ")", ",", "$len", ")", ")", ")", ";",
    "}",
)
# `($count and $et->FoundTag($tagInfo, $val));`, the V0 reporting statement
# after the shared `($count and` prefix.
V0_ONLY_SPAN = ("$et", "->", "FoundTag", "(", "$tagInfo", ",", "$val", ")", ")", ";")


def _split(left, right):
    prefix = 0
    while prefix < min(len(left), len(right)) and left[prefix] == right[prefix]:
        prefix += 1
    suffix = 0
    while (suffix < min(len(left), len(right)) - prefix
           and left[len(left) - 1 - suffix] == right[len(right) - 1 - suffix]):
        suffix += 1
    return prefix, left[prefix:len(left) - suffix], right[prefix:len(right) - suffix], suffix


class SerialProcessorGrammarTests(unittest.TestCase):
    def test_v0_is_v1_without_exactly_the_option_gated_tag_extra_stores(self):
        prefix, v1_only, v0_only, suffix = _split(SERIAL_PROCESSOR_V1_TOKENS, SERIAL_PROCESSOR_V0_TOKENS)
        self.assertEqual(v1_only, V1_ONLY_SPAN)
        self.assertEqual(v0_only, V0_ONLY_SPAN)
        # The shared context on both sides is the guarded reporting branch and
        # the cursor advance, so the excision is inside that one statement.
        self.assertEqual(SERIAL_PROCESSOR_V1_TOKENS[prefix - 3:prefix], ("(", "$count", "and"))
        self.assertEqual(SERIAL_PROCESSOR_V1_TOKENS[len(SERIAL_PROCESSOR_V1_TOKENS) - suffix:][:5],
                         ("}", "(", "$pos", "+=", "$len"))

    def test_v1_only_span_writes_nothing_but_option_gated_tag_extra(self):
        # Three assignments: the local key, then one TAG_EXTRA store per
        # option, each the right operand of `$$et{OPTIONS}{<option>} and`.
        assignments = [index for index, token in enumerate(V1_ONLY_SPAN) if token == "="]
        self.assertEqual(len(assignments), 3)
        self.assertEqual(V1_ONLY_SPAN[assignments[0] - 4:assignments[0]], ("my", "(", "$key", ")"))
        for index, (option, field) in zip(assignments[1:], (("'SaveFormat'", "'G6'"), ("'SaveBin'", "'BinVal'")), strict=True):
            self.assertEqual(
                V1_ONLY_SPAN[index - 21:index],
                ("$et", "->", "{", "'OPTIONS'", "}", "{", option, "}", "and",
                 "(", "$et", "->", "{", "'TAG_EXTRA'", "}", "{", "$key", "}", "{", field, "}"),
            )

    def test_grammars_are_distinct_complete_and_uniquely_named(self):
        names = [name for name, _ in SERIAL_PROCESSOR_GRAMMARS]
        streams = [tokens for _, tokens in SERIAL_PROCESSOR_GRAMMARS]
        self.assertEqual(names, ["v1", "v0"])
        self.assertEqual(len(set(streams)), len(streams))
        for tokens in streams:
            self.assertEqual(tokens[:8], ("(", "$", "$", "$", ")", "{", "package", "Image::ExifTool::Canon"))
            self.assertEqual(tokens[-6:], ("(", "return", "1", ")", ";", "}"))


if __name__ == "__main__":
    unittest.main()
