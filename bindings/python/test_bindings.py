import hashlib
import os
import shutil
import tempfile
import unittest

from oxidex import (
    OXIDEX_ERR_TAG_NOT_WRITTEN,
    Oxidex,
    OxidexError,
    OxidexTagsNotWrittenError,
)


REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def fixture(*parts):
    return os.path.join(REPO, "tests", "fixtures", *parts)


FIXTURE = fixture("jpeg", "sample_with_exif.jpg")
JPEG = fixture("jpeg", "simple", "synthetic_001.jpg")
PNG = fixture("png", "sample.png")


def sha(path):
    with open(path, "rb") as handle:
        return hashlib.sha256(handle.read()).hexdigest()


class OxidexBindingTests(unittest.TestCase):
    def test_import_read_and_count_tags(self):
        with Oxidex() as ox:
            ox.read_file(FIXTURE)
            self.assertGreater(ox.get_tag_count(), 0)


class OxidexWriteTests(unittest.TestCase):
    """exiftool_write_file applies every requested change or refuses the
    write, naming each key it would not write, with the file untouched.

    Pinned ExifTool 13.59 writes each refused key below (XMP-dc Title, the
    COM segment, IFD1 ImageDescription); oxidex has no writer for them in
    these formats, so success would be a lie.
    """

    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def copy(self, source, name):
        path = os.path.join(self.dir, name)
        shutil.copyfile(source, path)
        return path

    def assert_refused(self, source, name, keys, writable=None):
        path = self.copy(source, name)
        before = sha(path)
        with Oxidex() as ox:
            ox.read_file(path)
            if writable:
                ox.set_tag(writable, "v")
            for key in keys:
                ox.set_tag(key, "v")
            with self.assertRaises(OxidexError) as caught:
                ox.write_file(path)
        message = str(caught.exception)
        for key in keys:
            self.assertIn(key, message)
        if writable:
            self.assertNotIn("'%s'" % writable, message)
        # Typed: the error code, and each refused tag as the request spelled it.
        self.assertIsInstance(caught.exception, OxidexTagsNotWrittenError)
        self.assertEqual(caught.exception.code, OXIDEX_ERR_TAG_NOT_WRITTEN)
        named = [tag for tag, _reason in caught.exception.tags]
        self.assertEqual(sorted(named), sorted(keys))
        self.assertTrue(all(reason for _tag, reason in caught.exception.tags))
        self.assertEqual(sha(path), before, "a refused write changed the file")
        return caught.exception

    def test_xmp_in_jpeg_is_refused(self):
        self.assert_refused(JPEG, "xmp.jpg", ["XMP:Title"])

    def test_xmp_in_png_is_refused(self):
        self.assert_refused(PNG, "xmp.png", ["XMP:Title"])

    def test_file_comment_is_refused(self):
        self.assert_refused(JPEG, "comment.jpg", ["File:Comment"])

    def test_ifd1_key_the_writer_drops_is_refused(self):
        self.assert_refused(JPEG, "ifd1.jpg", ["IFD1:ImageDescription"])

    def test_multi_key_request_is_refused_whole(self):
        self.assert_refused(
            JPEG, "multi.jpg", ["XMP:Title", "File:Comment"], writable="IFD0:XPTitle"
        )

    def test_ungrouped_name_resolves_like_the_cli(self):
        path = self.copy(JPEG, "ungrouped.jpg")
        with Oxidex() as ox:
            ox.read_file(path)
            ox.set_tag("XPTitle", "v")
            ox.write_file(path)
        with Oxidex() as ox:
            ox.read_file(path)
            self.assertEqual(ox.get_tag("IFD0:XPTitle"), "v")

    def test_writable_key_is_written(self):
        path = self.copy(JPEG, "artist.jpg")
        with Oxidex() as ox:
            ox.read_file(path)
            ox.set_tag("IFD0:Artist", "someone")
            ox.write_file(path)
        with Oxidex() as ox:
            ox.read_file(path)
            self.assertEqual(ox.get_tag("IFD0:Artist"), "someone")


if __name__ == "__main__":
    unittest.main()
