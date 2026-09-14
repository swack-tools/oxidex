"""Family 0/1 selectors keep Writer's multi-selector conjunction semantics."""
import unittest

from setnewvalue_addressing import compile_addressing
from setnewvalue_group_selectors import parse, resolve
from test_setnewvalue_addressing import observations, source


class SetNewValueGroupSelectorTests(unittest.TestCase):
    def test_last_colon_and_family_conjunction_resolve_static_write_group(self):
        document = source()
        addressing, _ = compile_addressing(document)
        native = observations(addressing.rows)
        result = resolve(document, native, "0EXIF:1IFD0:NoAllowlist")
        self.assertEqual(result.state, "resolved")
        self.assertEqual(result.row.identity, addressing.rows[0].identity)
        self.assertEqual(result.write_group, "IFD0")

    def test_unmatched_or_unrepresented_family_is_terminal(self):
        document = source()
        addressing, _ = compile_addressing(document)
        native = observations(addressing.rows)
        self.assertEqual(resolve(document, native, "0EXIF:1Other:NoAllowlist").state,
                         "owned_unsupported")
        self.assertEqual(resolve(document, native, "EXIF:IFD0:NoAllowlist").state,
                         "owned_unsupported")
        self.assertEqual(parse("IFD0:EXIF:NoAllowlist").state, "owned_unsupported")


if __name__ == "__main__":
    unittest.main()
