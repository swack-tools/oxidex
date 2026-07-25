import os
import unittest

from oxidex import Oxidex


FIXTURE = os.path.abspath(
    os.path.join(
        os.path.dirname(__file__),
        "..",
        "..",
        "tests",
        "fixtures",
        "jpeg",
        "sample_with_exif.jpg",
    )
)


class OxidexBindingTests(unittest.TestCase):
    def test_import_read_and_count_tags(self):
        with Oxidex() as ox:
            ox.read_file(FIXTURE)
            self.assertGreater(ox.get_tag_count(), 0)


if __name__ == "__main__":
    unittest.main()
