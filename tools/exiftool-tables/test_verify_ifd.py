"""Slice I-1: verify.py's IFD stage and reachability.py's IFD census, against
the hand-written sample `fixtures/ifd_tables_sample.rs` (the spec's section-2
shape, after rustfmt) and a transcript of the oracle rows for its keys.

What is pinned here is the INSTRUMENT: that the parser accounts for every
`IfdTag {`, fails loudly on a shape it does not know, and that each kind of
wrong fact -- a name, an enum value, a flag, a SetMember member, a subdir
target/start/byte order/FixFormat/SubIFD, a count, a group, a table fact --
produces exactly the mismatch it should, while a refusal never does. The
VALUES are the real oracle's job: the same fixture is also run through
`verify.py --ifd-generated fixtures/ifd_tables_sample.rs` against the pinned
tree, which must PASS. Run with
`python3 -m unittest discover -s tools/exiftool-tables -p 'test*.py'`.
"""
import pathlib
import re
import tempfile
import unittest

import reachability
import verify

HERE = pathlib.Path(__file__).resolve().parent
SAMPLE = HERE / "fixtures" / "ifd_tables_sample.rs"

# `oracle.pl`'s IFD rows for every key the sample carries, transcribed from
# a run over the pinned 13.59 lib (tab-separated exactly as emitted; the
# CameraType enum is truncated to the entries the sample uses -- the
# generator may emit fewer entries than ExifTool has).
ORACLE = "\n".join([
    "IFD\tExif\tMain\t\tTGROUPS\tEXIF\tIFD0\tImage",
    "IFD\tExif\tMain\t\tSETGROUP1\t1",
    "IFD\tExif\tMain\t254\tNAME\tSubfileType",
    "IFD\tExif\tMain\t254\tWRITABLE\tint32u",
    "IFD\tExif\tMain\t254\tFLAGS\t0\t0\t0\t1\t0\t-",
    "IFD\tExif\tMain\t254\tRAWCONV\tif ($val == ($val & 0x02)) { $$self{SubfileType} = $val; }",
    "IFD\tExif\tMain\t254\tBITMASK\t0\tReduced resolution",
    "IFD\tExif\tMain\t254\tBITMASK\t1\tSingle page",
    "IFD\tExif\tMain\t254\tBITMASK\t2\tTransparency mask",
    "IFD\tExif\tMain\t254\tBITMASK\t3\tTIFF/IT final page",
    "IFD\tExif\tMain\t254\tBITMASK\t4\tTIFF-FX mixed raster content",
    "IFD\tExif\tMain\t254\tENUM\t0\tFull-resolution image",
    "IFD\tExif\tMain\t254\tENUM\t1\tReduced-resolution image",
    "IFD\tExif\tMain\t254\tENUM\t16\tEnhanced image data",
    "IFD\tExif\tMain\t254\tENUM\t2\tSingle page of multi-page image",
    "IFD\tExif\tMain\t254\tENUM\t3\tSingle page of multi-page reduced-resolution image",
    "IFD\tExif\tMain\t254\tENUM\t4\tTransparency mask",
    "IFD\tExif\tMain\t254\tENUM\t4294967295\tinvalid",
    "IFD\tExif\tMain\t254\tENUM\t5\tTransparency mask of reduced-resolution image",
    "IFD\tExif\tMain\t254\tENUM\t6\tTransparency mask of multi-page image",
    "IFD\tExif\tMain\t254\tENUM\t65537\tAlternate reduced-resolution image",
    "IFD\tExif\tMain\t254\tENUM\t65540\tSemantic Mask",
    "IFD\tExif\tMain\t254\tENUM\t7\tTransparency mask of reduced-resolution multi-page image",
    "IFD\tExif\tMain\t254\tENUM\t8\tDepth map",
    "IFD\tExif\tMain\t254\tENUM\t9\tDepth map of reduced-resolution image",
    "IFD\tExif\tMain\t33424\tNAME\tKodakIFD",
    "IFD\tExif\tMain\t33424\tGROUPS\t\tKodakIFD\t",
    "IFD\tExif\tMain\t33424\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tExif\tMain\t33424\tSUBDIR\tImage::ExifTool::Kodak::IFD\t$val\t-\t-\t-\t0\t-\t1\t1\tKodakIFD",
    "IFD\tExif\tMain\t33723\tNAME\tIPTC-NAA",
    "IFD\tExif\tMain\t33723\tFORMAT\tundef",
    "IFD\tExif\tMain\t33723\tWRITABLE\tint32u",
    "IFD\tExif\tMain\t33723\tFLAGS\t0\t1\t0\t1\t0\t-",
    "IFD\tExif\tMain\t33723\tSUBDIR\tImage::ExifTool::IPTC::Main\t-\t-\t-\t-\t0\t-\t0\t-\tIPTC",
    "IFD\tExif\tMain\t34665\tNAME\tExifOffset",
    "IFD\tExif\tMain\t34665\tGROUPS\t\tExifIFD\t",
    "IFD\tExif\tMain\t34665\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tExif\tMain\t34665\tSUBDIR\t-\t$val\t-\t-\t-\t0\t-\t1\t-\tExifIFD",
    "IFD\tExif\tMain\t41985\tNAME\tCustomRendered",
    "IFD\tExif\tMain\t41985\tWRITABLE\tint16u",
    "IFD\tExif\tMain\t41985\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tExif\tMain\t41985\tENUM\t0\tNormal",
    "IFD\tExif\tMain\t41985\tENUM\t1\tCustom",
    "IFD\tExif\tMain\t41985\tENUM\t2\tHDR (no original saved)",
    "IFD\tExif\tMain\t50706\tNAME\tDNGVersion",
    "IFD\tExif\tMain\t50706\tCOUNT\t4",
    "IFD\tExif\tMain\t50706\tWRITABLE\tint8u",
    "IFD\tExif\tMain\t50706\tFLAGS\t0\t0\t0\t1\t0\t-",
    "IFD\tExif\tMain\t50706\tRAWCONV\t$$self{DNGVersion} = $val",
    "IFD\tExif\tMain\t51157\tNAME\tNikonNEFInfo",
    "IFD\tExif\tMain\t51157\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tExif\tMain\t51157\tCONDITION\t1",
    "IFD\tExif\tMain\t51157\tSUBDIR\tImage::ExifTool::Nikon::NEFInfo\t$valuePtr + 18\t$start - 8\t-\tUnknown\t0\t-\t0\t-\t-",
    "IFD\tFLIR\tMain\t\tTGROUPS\tMakerNotes\t\tCamera",
    "IFD\tFLIR\tMain\t\tPRIORITY\t0",
    "IFD\tFLIR\tMain\t1\tNAME\tImageTemperatureMax",
    "IFD\tFLIR\tMain\t1\tFORMAT\trational64s",
    "IFD\tFLIR\tMain\t1\tWRITABLE\trational64u",
    "IFD\tFLIR\tMain\t1\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tOlympus\tEquipment\t\tTGROUPS\tMakerNotes\t\tCamera",
    "IFD\tOlympus\tEquipment\t0\tNAME\tEquipmentVersion",
    "IFD\tOlympus\tEquipment\t0\tCOUNT\t4",
    "IFD\tOlympus\tEquipment\t0\tWRITABLE\tundef",
    "IFD\tOlympus\tEquipment\t0\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tOlympus\tEquipment\t0\tRAWCONV\t$val=~s/\\0+$//; $val",
    "IFD\tOlympus\tEquipment\t256\tNAME\tCameraType2",
    "IFD\tOlympus\tEquipment\t256\tCOUNT\t6",
    "IFD\tOlympus\tEquipment\t256\tWRITABLE\tstring",
    "IFD\tOlympus\tEquipment\t256\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tOlympus\tEquipment\t256\tENUM\tD4028\tX-2,C-50Z",
    "IFD\tOlympus\tEquipment\t256\tENUM\tD4029\tE-20,E-20N,E-20P",
    "IFD\tOlympus\tEquipment\t256\tENUM\tD4040\tE-1",
    "IFD\tOlympus\tMain\t\tTGROUPS\tMakerNotes\t\tCamera",
    "IFD\tOlympus\tMain\t1\tNAME\tMinoltaCameraSettingsOld",
    "IFD\tOlympus\tMain\t1\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tOlympus\tMain\t1\tSUBDIR\tImage::ExifTool::Minolta::CameraSettings\t-\t-\t-\tBigEndian\t0\t-\t0\t-\t-",
    "IFD\tOlympus\tMain\t256\tNAME\tThumbnailImage",
    "IFD\tOlympus\tMain\t256\tWRITABLE\tundef",
    "IFD\tOlympus\tMain\t256\tGROUPS\t\t\tPreview",
    "IFD\tOlympus\tMain\t256\tFLAGS\t0\t1\t0\t0\t0\t-",
    "IFD\tOlympus\tMain\t512\tNAME\tSpecialMode",
    "IFD\tOlympus\tMain\t512\tCOUNT\t3",
    "IFD\tOlympus\tMain\t512\tWRITABLE\tint32u",
    "IFD\tOlympus\tMain\t512\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tOlympus\tMain\t512\tPCREF\tCODE",
    "IFD\tOlympus\tMain\t519\tNAME\tCameraType",
    "IFD\tOlympus\tMain\t519\tWRITABLE\tstring",
    "IFD\tOlympus\tMain\t519\tFLAGS\t0\t0\t0\t0\t0\t0",
    "IFD\tOlympus\tMain\t519\tRAWCONV\t$self->{CameraType} = $val",
    "IFD\tOlympus\tMain\t519\tCONDITION\t1",
    "IFD\tOlympus\tMain\t519\tENUM\tD4028\tX-2,C-50Z",
    "IFD\tOlympus\tMain\t519\tENUM\tD4040\tE-1",
    "IFD\tOlympus\tMain\t8208#0\tNAME\tEquipment",
    "IFD\tOlympus\tMain\t8208#0\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tOlympus\tMain\t8208#0\tSUBDIR\tImage::ExifTool::Olympus::Equipment\t-\t-\t-\tUnknown\t0\t-\t0\t-\t-",
    "IFD\tOlympus\tMain\t8208#1\tNAME\tEquipmentIFD",
    "IFD\tOlympus\tMain\t8208#1\tGROUPS\t\tMakerNotes\t",
    "IFD\tOlympus\tMain\t8208#1\tFLAGS\t0\t0\t0\t0\t0\t-",
    "IFD\tOlympus\tMain\t8208#1\tSUBDIR\tImage::ExifTool::Olympus::Equipment\t$val\t-\t-\t-\t0\tifd\t1\t-\t-",
    # A binary-table row (no kind column) must be ignored by the IFD reader
    # and read by the binary one.
    "Canon\tCameraSettings\t1\tMacroMode",
]) + "\n"


def _parsed():
    return verify.parse_ifd_rust(SAMPLE)


def _oracle(text=ORACLE):
    return verify.parse_ifd_oracle(text)


def _run(gen=None, text=ORACLE):
    lines, failed = verify.verify_ifd(gen or _parsed(), _oracle(text))
    return failed, "\n".join(lines)


def _parse_text(rust):
    with tempfile.NamedTemporaryFile("w", suffix=".rs", delete=False, encoding="utf-8") as fh:
        fh.write(rust)
        path = fh.name
    return verify.parse_ifd_rust(path)


def _mutated_sample(old, new, count=1):
    src = SAMPLE.read_text(encoding="utf-8")
    assert src.count(old) == count, (old, src.count(old))
    return _parse_text(src.replace(old, new))


class ParseSample(unittest.TestCase):
    def test_accounts_for_every_table_tag_and_alternative(self):
        gen = _parsed()
        self.assertEqual(
            sorted(gen.tables),
            [("Exif", "Main"), ("FLIR", "Main"), ("Olympus", "Equipment"), ("Olympus", "Main")],
        )
        self.assertEqual(len(gen.tags), SAMPLE.read_text(encoding="utf-8").count("IfdTag {"))
        self.assertEqual(
            sorted(gen.variant_keys),
            [("Olympus", "Main", "8208#0"), ("Olympus", "Main", "8208#1")],
        )
        self.assertEqual(gen.structure, [])
        self.assertEqual(
            gen.all_tables,
            ["IFD_EXIF_MAIN", "IFD_FLIR_MAIN", "IFD_OLYMPUS_EQUIPMENT", "IFD_OLYMPUS_MAIN"],
        )

    def test_table_facts(self):
        gen = _parsed()
        exif = gen.tables[("Exif", "Main")]
        self.assertEqual(exif["groups"], ("EXIF", "IFD0", "Image"))
        self.assertEqual(exif["set_group1"], "1")
        self.assertIsNone(exif["priority"])
        self.assertFalse(exif["gate_a_blocked"])
        self.assertEqual(gen.tables[("FLIR", "Main")]["priority"], 0)
        self.assertTrue(gen.tables[("Olympus", "Main")]["gate_a_blocked"])

    def test_tag_facts_in_every_shape(self):
        gen = _parsed()
        t = gen.tags[("Exif", "Main", "254")]
        self.assertEqual(t["name"], "SubfileType")
        self.assertEqual((t["fmt"], t["count"], t["writable"]), ("None", None, "int32u"))
        self.assertEqual(t["flags"], (False, False, False, True, False, None))
        self.assertTrue(t["omitted"]["raw_conv"])
        self.assertEqual(len(gen.enums[("Exif", "Main", "254")]), 14)
        self.assertEqual(gen.bitmasks[("Exif", "Main", "254")]["3"], "TIFF/IT final page")

        t = gen.tags[("Exif", "Main", "50706")]
        self.assertEqual(t["raw_conv"], "DNGVersion")
        self.assertEqual(t["count"], 4)

        t = gen.tags[("Exif", "Main", "51157")]
        self.assertEqual(t["subdir"]["start"], ("ValuePtr", 18))
        self.assertEqual(t["subdir"]["base"], "Some(&BaseExpr::Sub(&BaseExpr::Start,&BaseExpr::Const(8)))")
        self.assertEqual(t["subdir"]["byte_order"], "Unknown")
        self.assertTrue(t["omitted"]["condition"])

        t = gen.tags[("Exif", "Main", "33424")]
        self.assertEqual(t["groups"], ("", "KodakIFD", ""))
        self.assertEqual(
            (t["subdir"]["start"], t["subdir"]["sub_ifd"], t["subdir"]["max_subdirs"],
             t["subdir"]["dir_name"]),
            (("Val", 0), True, 1, "KodakIFD"),
        )
        self.assertIsNone(gen.tags[("Exif", "Main", "34665")]["subdir"])

        t = gen.tags[("FLIR", "Main", "1")]
        self.assertEqual(t["fmt"], "Some(Fmt::Rational64s)")

        t = gen.tags[("Olympus", "Main", "519")]
        self.assertEqual(t["flags"][5], 0)
        self.assertEqual(gen.enums[("Olympus", "Main", "519")], {"D4028": "X-2,C-50Z", "D4040": "E-1"})
        self.assertEqual(gen.tags[("Olympus", "Main", "512")]["omitted"]["print_conv"], True)
        alt = gen.tags[("Olympus", "Main", "8208#1")]
        self.assertEqual(alt["subdir"]["start"], ("Val", 0))
        self.assertTrue(alt["subdir"]["sub_ifd"])


class ParseFailsLoudly(unittest.TestCase):
    """An unparsed `IfdTag {` is a coverage lie, so every shape the parser
    does not know is a SystemExit, never a silent skip."""

    def test_var_format_is_not_an_ifd_shape(self):
        with self.assertRaises(SystemExit):
            _mutated_sample(
                "format: Some(Fmt::Rational64s)",
                'format: Some(Fmt::Var(VarFmt { spelling: "var_string", kind: VarKind::String }))',
            )

    def test_unknown_print_conv_variant(self):
        with self.assertRaises(SystemExit):
            _mutated_sample(
                "print_conv: PrintConv::None,\n        subdir: None,\n    }],",
                "print_conv: PrintConv::Mystery(3),\n        subdir: None,\n    }],",
            )

    def test_unknown_byte_order(self):
        with self.assertRaises(SystemExit):
            _mutated_sample("byte_order: IfdByteOrder::Big,", "byte_order: IfdByteOrder::Middle,")

    def test_missing_variants_member(self):
        with self.assertRaises(SystemExit):
            _mutated_sample("    ],\n    variants: &[],\n};\n\npub static IFD_FLIR_MAIN",
                            "    ],\n};\n\npub static IFD_FLIR_MAIN")

    def test_table_header_out_of_shape(self):
        # A header the pattern cannot read must not let its tags be absorbed
        # into the previous table's range: the declaration count check and
        # the whole-file `IfdTag {` count both fire.
        with self.assertRaises(SystemExit):
            _mutated_sample('    group0: "MakerNotes",\n    group1: "FLIR",',
                            '    group1: "FLIR",\n    group0: "MakerNotes",')

    def test_missing_all_ifd_tables(self):
        with self.assertRaises(SystemExit):
            _mutated_sample("pub static ALL_IFD_TABLES: &[&IfdTable] = &[",
                            "pub static ALL_TABLES_RENAMED: &[&IfdTable] = &[")


class Structure(unittest.TestCase):
    def test_unsorted_tags_are_a_mismatch(self):
        gen = _mutated_sample("id: 41985,\n            name: \"CustomRendered\"",
                              "id: 41985,\n            name: \"CustomRendered\"")
        self.assertEqual(gen.structure, [])
        # Swap the KodakIFD id past IPTC-NAA's: 33724 > 33723 but the array
        # order stays, so `tags` is no longer sorted.
        gen = _mutated_sample("id: 33424,", "id: 33724,")
        self.assertTrue(any("not sorted" in s for s in gen.structure), gen.structure)
        failed, _ = _run(gen)
        self.assertGreaterEqual(failed, 1)

    def test_all_ifd_tables_order_and_completeness(self):
        gen = _mutated_sample("    &IFD_EXIF_MAIN,\n    &IFD_FLIR_MAIN,",
                              "    &IFD_FLIR_MAIN,\n    &IFD_EXIF_MAIN,")
        self.assertTrue(any("not sorted" in s for s in gen.structure), gen.structure)
        gen = _mutated_sample("    &IFD_FLIR_MAIN,\n", "")
        self.assertTrue(any("exactly once" in s for s in gen.structure), gen.structure)

    def test_alternative_id_must_match_group_id(self):
        gen = _mutated_sample('id: 8208,\n                    name: "EquipmentIFD"',
                              'id: 8209,\n                    name: "EquipmentIFD"')
        self.assertTrue(any("inside group id" in s for s in gen.structure), gen.structure)


class OracleReader(unittest.TestCase):
    def test_reads_every_row_kind_and_ignores_binary_rows(self):
        o = _oracle()
        self.assertEqual(o.tables[("Exif", "Main")],
                         {"tgroups": ("EXIF", "IFD0", "Image"), "set_group1": "1", "priority": None})
        self.assertEqual(o.tables[("FLIR", "Main")]["priority"], "0")
        self.assertEqual(o.names[("Olympus", "Main", "8208#1")], "EquipmentIFD")
        self.assertEqual(o.flags[("Exif", "Main", "33723")], (False, True, False, True, False, None))
        self.assertEqual(o.flags[("Olympus", "Main", "519")][5], 0)
        self.assertEqual(o.subdirs[("Exif", "Main", "51157")]["base"], "$start - 8")
        self.assertTrue(o.subdirs[("Olympus", "Main", "8208#1")]["subifd"])
        self.assertIn(("Exif", "Main", "51157"), o.conditions)
        self.assertEqual(o.pcrefs[("Olympus", "Main", "512")], "CODE")
        self.assertNotIn(("Canon", "CameraSettings", "1"), o.names)
        # ...and the binary reader sees only the binary row.
        names = verify.parse_binary_oracle(ORACLE)[0]
        self.assertEqual(names, {("Canon", "CameraSettings", "1"): "MacroMode"})

    def test_unknown_ifd_row_is_loud(self):
        with self.assertRaises(SystemExit):
            _oracle(ORACLE + "IFD\tExif\tMain\t1\tNOVELTY\tx\n")
        with self.assertRaises(SystemExit):
            # Right marker, wrong column count.
            _oracle(ORACLE + "IFD\tExif\tMain\t1\tFLAGS\t0\t0\n")


class CleanRunPasses(unittest.TestCase):
    def test_sample_against_its_oracle_rows(self):
        failed, report = _run()
        self.assertEqual(failed, 0, report)
        self.assertIn("IFD MISMATCH total 0", report)
        self.assertIn("subdirectory edges 6", report)
        # CameraType's `$self->{CameraType} = $val` is SetMember-shaped but
        # the sample refuses it: a note, never a mismatch.
        self.assertIn("SetMember-shaped RawConvs refused instead: 1", report)


class EachWrongFactIsExactlyOneMismatch(unittest.TestCase):
    """Soundness: a wrong emitted fact fails; a refusal does not."""

    def _one(self, old, new, label, count=1, expected=1):
        failed, report = _run(_mutated_sample(old, new, count))
        self.assertEqual(failed, expected, report)
        self.assertIn(label, report)
        return report

    def test_wrong_name(self):
        self._one('name: "CustomRendered"', 'name: "CustomRender"', "name  (")

    def test_tag_not_in_oracle(self):
        self._one("id: 41985,", "id: 41986,", "orphan tag")

    def test_wrong_enum_value(self):
        self._one('(1, "Custom")', '(1, "Customised")', "enum  (")

    def test_wrong_string_enum_key(self):
        # The pair occurs in two tags (Equipment 0x100 and Main 0x207): one
        # wrong key each, two mismatches.
        self._one('("D4040", "E-1")', '("D4041", "E-1")', "enum  (", count=2, expected=2)

    def test_invented_bitmask_bit(self):
        self._one('(4, "TIFF-FX mixed raster content")', '(5, "TIFF-FX mixed raster content")',
                  "bitmask  (")

    def test_wrong_flag(self):
        self._one("binary: true,\n                list: false,\n                protected: true,",
                  "binary: true,\n                list: true,\n                protected: true,",
                  "flags  (")

    def test_flag_expanded_from_flags_key_is_required(self):
        # `Binary => 1` on IPTC-NAA: dropping it is a wrong flag, not a refusal.
        self._one("binary: true,\n                list: false,\n                protected: true,",
                  "binary: false,\n                list: false,\n                protected: true,",
                  "flags  (")

    def test_wrong_tag_priority(self):
        self._one("priority: Some(0),\n            },", "priority: Some(1),\n            },", "flags  (")

    def test_wrong_set_member(self):
        self._one('member: "DNGVersion"', 'member: "DNGVer"', "raw_conv  (")

    def test_set_member_where_the_raw_conv_is_another_shape(self):
        # EquipmentVersion's RawConv strips NULs; claiming it captures a
        # member is a wrong fact.
        rep = self._one(
            "raw_conv: true,\n                condition: false,\n                hook: false,\n"
            "                subdirectory: false,\n                print_conv: false,\n"
            "            },\n            raw_conv: None,\n            value_conv: None,\n"
            "            print_conv: PrintConv::None,\n            subdir: None,\n        },\n"
            "        IfdTag {\n            id: 256,\n            name: \"CameraType2\"",
            "raw_conv: false,\n                condition: false,\n                hook: false,\n"
            "                subdirectory: false,\n                print_conv: false,\n"
            "            },\n            raw_conv: Some(RawConvEffect::SetMember { member: \"EquipmentVersion\" }),\n"
            "            value_conv: None,\n"
            "            print_conv: PrintConv::None,\n            subdir: None,\n        },\n"
            "        IfdTag {\n            id: 256,\n            name: \"CameraType2\"",
            "raw_conv  (",
        )
        self.assertIn("but RawConv is", rep)

    def test_raw_conv_dropped_silently(self):
        self._one("raw_conv: true,\n                condition: true,",
                  "raw_conv: false,\n                condition: true,", "dropped silently")

    def test_raw_conv_refused_where_none_exists(self):
        self._one("omitted: Omitted::NONE,\n            raw_conv: None,\n            value_conv: None,\n"
                  "            print_conv: PrintConv::IntEnum",
                  "omitted: Omitted { value_conv: false, raw_conv: true, condition: false, hook: false, "
                  "subdirectory: false, print_conv: false },\n            raw_conv: None,\n"
                  "            value_conv: None,\n            print_conv: PrintConv::IntEnum",
                  "omitted.raw_conv but ExifTool has no RawConv")

    def test_print_conv_code_ref_dropped_silently(self):
        self._one("subdirectory: false,\n                print_conv: true,",
                  "subdirectory: false,\n                print_conv: false,", "dropped silently")

    def test_condition_dropped_on_a_plain_entry(self):
        self._one("raw_conv: false,\n                condition: true,",
                  "raw_conv: false,\n                condition: false,", "condition flag")

    def test_condition_flag_on_an_alternative(self):
        self._one(
            "name: \"EquipmentIFD\",\n                    format: None,\n                    count: None,\n"
            "                    writable: None,\n                    groups: TagGroups {\n"
            "                        g0: None,\n                        g1: Some(\"MakerNotes\"),\n"
            "                        g2: None,\n                    },\n                    flags: IfdFlags::NONE,\n"
            "                    omitted: Omitted {\n                        value_conv: false,\n"
            "                        raw_conv: false,\n                        condition: false,",
            "name: \"EquipmentIFD\",\n                    format: None,\n                    count: None,\n"
            "                    writable: None,\n                    groups: TagGroups {\n"
            "                        g0: None,\n                        g1: Some(\"MakerNotes\"),\n"
            "                        g2: None,\n                    },\n                    flags: IfdFlags::NONE,\n"
            "                    omitted: Omitted {\n                        value_conv: false,\n"
            "                        raw_conv: false,\n                        condition: true,",
            "condition flag",
        )

    def test_wrong_count(self):
        self._one("count: Some(4),\n            writable: Some(\"int8u\")",
                  "count: Some(3),\n            writable: Some(\"int8u\")", "format  (")

    def test_invented_count(self):
        self._one('count: None,\n            writable: Some("int16u")',
                  'count: Some(1),\n            writable: Some("int16u")', "format  (")

    def test_wrong_format(self):
        self._one("format: Some(Fmt::Rational64s)", "format: Some(Fmt::Rational64u)", "format  (")

    def test_bare_undef_must_be_none_not_a_sized_format(self):
        self._one('name: "IPTC-NAA",\n            format: None,',
                  'name: "IPTC-NAA",\n            format: Some(Fmt::Undef(4)),', "format  (")

    def test_wrong_writable(self):
        self._one('writable: Some("rational64u")', 'writable: Some("rational64s")', "writable  (")

    def test_writable_where_none_declared(self):
        self._one('name: "ExifOffset",\n            format: None,\n            count: None,\n            writable: None,',
                  'name: "ExifOffset",\n            format: None,\n            count: None,\n            writable: Some("int32u"),',
                  "writable  (")

    def test_wrong_group_override(self):
        self._one('g2: Some("Preview")', 'g2: Some("Image")', "tag groups  (")

    def test_group_override_dropped(self):
        self._one("groups: TagGroups {\n                g0: None,\n                g1: Some(\"ExifIFD\"),\n"
                  "                g2: None,\n            },",
                  "groups: TagGroups::NONE,", "tag groups  (")

    def test_subdirectory_flag_dropped(self):
        rep = self._one(
            "hook: false,\n                subdirectory: true,\n                print_conv: false,\n"
            "            },\n            raw_conv: None,\n            value_conv: None,\n"
            "            print_conv: PrintConv::None,\n            subdir: None,",
            "hook: false,\n                subdirectory: false,\n                print_conv: false,\n"
            "            },\n            raw_conv: None,\n            value_conv: None,\n"
            "            print_conv: PrintConv::None,\n            subdir: None,",
            "subdirectory flag",
        )
        self.assertIn("ExifOffset", rep.replace("34665", "ExifOffset"))

    def test_wrong_subdir_target(self):
        self._one('table: "CameraSettings",\n                start: IfdStart::ValuePtr(0),',
                  'table: "CameraSettings2",\n                start: IfdStart::ValuePtr(0),', "subdir edge  (")

    def test_wrong_subdir_start(self):
        self._one("start: IfdStart::ValuePtr(18),", "start: IfdStart::ValuePtr(16),", "subdir edge  (")

    def test_val_start_confused_with_value_ptr(self):
        self._one("start: IfdStart::Val(0),\n                base: None,\n                byte_order: IfdByteOrder::Inherit,\n"
                  "                fix_format: None,\n                sub_ifd: true,\n                max_subdirs: Some(1),",
                  "start: IfdStart::ValuePtr(0),\n                base: None,\n                byte_order: IfdByteOrder::Inherit,\n"
                  "                fix_format: None,\n                sub_ifd: true,\n                max_subdirs: Some(1),",
                  "subdir edge  (")

    def test_wrong_byte_order(self):
        self._one("byte_order: IfdByteOrder::Big,", "byte_order: IfdByteOrder::Little,", "subdir edge  (")

    def test_wrong_fix_format(self):
        self._one("fix_format: None,\n                        sub_ifd: true,",
                  "fix_format: Some(Fmt::Int32u),\n                        sub_ifd: true,", "subdir edge  (")

    def test_wrong_sub_ifd(self):
        self._one("fix_format: None,\n                        sub_ifd: true,",
                  "fix_format: None,\n                        sub_ifd: false,", "subdir edge  (")

    def test_wrong_max_subdirs_and_dir_name(self):
        self._one("max_subdirs: Some(1),", "max_subdirs: Some(2),", "subdir edge  (")
        self._one('dir_name: Some("KodakIFD"),', 'dir_name: None,', "subdir edge  (")

    def test_base_dropped(self):
        self._one("base: Some(&BaseExpr::Sub(&BaseExpr::Start, &BaseExpr::Const(8))),",
                  "base: None,", "subdir edge  (")

    def test_edge_invented_where_exiftool_refuses(self):
        # ExifOffset's SubDirectory has no TagTable: any edge is wrong.
        self._one(
            "print_conv: PrintConv::None,\n            subdir: None,\n        },\n        IfdTag {\n"
            "            id: 41985,",
            "print_conv: PrintConv::None,\n            subdir: Some(IfdSubdirEdge { module: \"Exif\", "
            "table: \"Main\", start: IfdStart::Val(0), base: None, byte_order: IfdByteOrder::Inherit, "
            "fix_format: None, sub_ifd: true, max_subdirs: None, dir_name: Some(\"ExifIFD\"), "
            "validate: false }),\n        },\n        IfdTag {\n            id: 41985,",
            "require a refusal",
        )

    def test_wrong_table_groups_set_group1_priority(self):
        self._one('group2: "Image",', 'group2: "Other",', "table groups  (")
        self._one('set_group1: Some("1"),', "set_group1: None,", "set_group1  (")
        self._one("priority: Some(0),\n    gate_a", "priority: None,\n    gate_a", "table priority  (")

    def test_table_not_in_oracle(self):
        # The renamed table is an orphan, and so is its one tag: two.
        self._one('module: "FLIR",\n    table: "Main",', 'module: "FLIR",\n    table: "Mane",',
                  "not an IFD-scope table", expected=2)


class RefusalsAreNotMismatches(unittest.TestCase):
    def test_refused_edge_is_a_note(self):
        gen = _mutated_sample(
            "subdir: Some(IfdSubdirEdge {\n                module: \"Minolta\",\n"
            "                table: \"CameraSettings\",\n                start: IfdStart::ValuePtr(0),\n"
            "                base: None,\n                byte_order: IfdByteOrder::Big,\n"
            "                fix_format: None,\n                sub_ifd: false,\n"
            "                max_subdirs: None,\n                dir_name: None,\n"
            "                validate: false,\n            }),",
            "subdir: None,",
        )
        failed, report = _run(gen)
        self.assertEqual(failed, 0, report)
        self.assertIn("refused though the facts were modelable (note, not a mismatch) 1", report)

    def test_set_member_refused_is_a_note(self):
        gen = _mutated_sample(
            "omitted: Omitted::NONE,\n            raw_conv: Some(RawConvEffect::SetMember {\n"
            "                member: \"DNGVersion\",\n            }),",
            "omitted: Omitted { value_conv: false, raw_conv: true, condition: false, hook: false, "
            "subdirectory: false, print_conv: false },\n            raw_conv: None,",
        )
        failed, report = _run(gen)
        self.assertEqual(failed, 0, report)
        self.assertIn("SetMember-shaped RawConvs refused instead: 2", report)

    def test_fewer_tags_and_fewer_enum_entries_are_fine(self):
        src = SAMPLE.read_text(encoding="utf-8")
        start = src.index("        IfdTag {\n            id: 41985,")
        end = src.index("        IfdTag {\n            id: 50706,")
        gen = _parse_text(src[:start] + src[end:])
        failed, report = _run(gen)
        self.assertEqual(failed, 0, report)

    def test_byte_order_outside_the_spec_spellings(self):
        # `Little-endian` (Reconyx) is refused by the spec but read as
        # little-endian by ExifTool: either a refusal or `Little` is
        # accepted; `Big` is not.
        text = ORACLE.replace(
            "Image::ExifTool::Minolta::CameraSettings\t-\t-\t-\tBigEndian",
            "Image::ExifTool::Minolta::CameraSettings\t-\t-\t-\tLittle-endian",
        )
        failed, report = _run(text=text)  # sample says Big
        self.assertEqual(failed, 1, report)
        gen = _mutated_sample("byte_order: IfdByteOrder::Big,", "byte_order: IfdByteOrder::Little,")
        failed, report = _run(gen, text)
        self.assertEqual(failed, 0, report)
        self.assertIn("emitted per ExifTool's rule (note) 1", report)


class ExpectedFormat(unittest.TestCase):
    def test_spec_rules(self):
        f = verify.expected_ifd_format
        self.assertEqual(f(None, None), ("None", None))
        self.assertEqual(f(None, 3), ("None", 3))
        self.assertEqual(f("undef", 4), ("None", 4))
        self.assertEqual(f("string", None), ("None", None))
        self.assertEqual(f("int16u", None), ("Some(Fmt::Int16u)", None))
        self.assertEqual(f("int16u", 2), ("Some(Fmt::Int16u)", 2))
        self.assertEqual(f("int8u[8]", None), ("Some(Fmt::Int8u)", 8))
        self.assertEqual(f("string[11]", None), ("Some(Fmt::Str(11))", 11))
        self.assertEqual(f("undef[7360]", None), ("Some(Fmt::Undef(7360))", 7360))
        for refused in ("Rect", "string[0,32]", "var_string", "pstring", "unsigned", "ifd"):
            self.assertIsNone(f(refused, None), refused)
        self.assertEqual(verify._count_decl("-1"), None)
        self.assertEqual(verify._count_decl("16"), 16)


class ExpectedEdge(unittest.TestCase):
    def _fact(self, **kw):
        base = {"tagtable": "Image::ExifTool::Olympus::Equipment", "start": "-", "base": "-",
                "processproc": "-", "byteorder": "-", "validate": False, "fixformat": "-",
                "subifd": False, "maxsubdirs": "-", "dirname": "-"}
        base.update(kw)
        return base

    def test_grammar(self):
        e = verify.expected_ifd_edge
        edge, spec = e(self._fact())
        self.assertEqual((edge["start"], edge["byte_order"], spec), (("ValuePtr", 0), "Inherit", False))
        self.assertEqual(e(self._fact(start="$valuePtr + 12"))[0]["start"], ("ValuePtr", 12))
        self.assertEqual(e(self._fact(start="$val - 36"))[0]["start"], ("Val", -36))
        self.assertIsNone(e(self._fact(start="4"))[0])
        self.assertIsNone(e(self._fact(tagtable="-"))[0])
        self.assertIsNone(e(self._fact(processproc="Image::ExifTool::MakerNotes::ProcessUnknown"))[0])
        self.assertIsNotNone(e(self._fact(processproc="Image::ExifTool::ProcessBinaryData"))[0])
        self.assertIsNone(e(self._fact(base="$start + foo()"))[0])
        self.assertTrue(e(self._fact(base="$start - 8"))[0]["base_present"])
        self.assertEqual(e(self._fact(fixformat="ifd", subifd=True))[0]["fix_format"], "None")
        self.assertEqual(e(self._fact(fixformat="int8u"))[0]["fix_format"], "Some(Fmt::Int8u)")
        self.assertIsNone(e(self._fact(fixformat="Rect"))[0])
        self.assertEqual(e(self._fact(byteorder="MM"))[0]["byte_order"], "Big")
        self.assertEqual(e(self._fact(byteorder="Little-endian")), (
            e(self._fact(byteorder="LittleEndian"))[0], True))
        self.assertEqual(e(self._fact(maxsubdirs="20"))[0]["max_subdirs"], 20)
        self.assertIsNone(e(self._fact(maxsubdirs="many"))[0])


class ReachabilityCensus(unittest.TestCase):
    def test_ifd_tables_and_edges(self):
        tables = reachability.parse_tables(SAMPLE, "ifd")
        by = {(t["module"], t["table"]): t for t in tables}
        self.assertEqual(len(by), 4)
        self.assertTrue(by[("Exif", "Main")]["gate_a"])
        self.assertEqual(by[("Olympus", "Main")]["blocked_by"], [("ifd_expr_domain_unknown", 2)])
        self.assertEqual(by[("Olympus", "Main")]["fields"], 6)  # 4 tags + 2 alternatives
        self.assertEqual(sorted(by[("Exif", "Main")]["edges"]),
                         [("IPTC", "Main"), ("Kodak", "IFD"), ("Nikon", "NEFInfo")])
        self.assertEqual(by[("Olympus", "Main")]["edges"],
                         [("Minolta", "CameraSettings"), ("Olympus", "Equipment"), ("Olympus", "Equipment")])
        enabled, eligible, refused = reachability.classify(tables, {("Exif", "Main")}, {("FLIR", "Main")})
        self.assertEqual(([t["table"] for t in enabled], len(eligible), len(refused)), (["Main"], 2, 1))
        self.assertTrue(by[("FLIR", "Main")]["hand_wired"])

    def test_ifd_report_section_renders(self):
        import contextlib
        import io
        tables = reachability.parse_tables(SAMPLE, "ifd")
        enabled, eligible, refused = reachability.classify(
            tables, {("Olympus", "Equipment")}, {("Olympus", "Main"), ("FLIR", "Main")}
        )
        # Edge targets resolve across BOTH kinds: a binary target is labelled
        # as such, an unknown one as `no-layout`.
        status = {(t["module"], t["table"]): t["status"] for t in tables}
        status[("IPTC", "Main")] = "binary:eligible"
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            reasons = reachability.report(
                tables, enabled, eligible, refused, {("Olympus", "Main"), ("FLIR", "Main")}, 0,
                "find_ifd_table", status,
            )
        out = buf.getvalue()
        self.assertIn("tables emitted            4", out)
        self.assertIn("  enabled  (gate A + measured allowlist)   1", out)
        self.assertIn("  eligible (gate A, awaiting a gate B run) 2", out)
        self.assertIn("  refused  (gate A blocks)                 1", out)
        self.assertIn("hand-wired find_ifd_table call sites", out)
        self.assertIn("  FLIR::Main                   eligible", out)
        self.assertIn("  Olympus::Main                   ifd_expr_domain_unknown=2", out)
        self.assertIn("SubDirectory edges: 6 from 2 tables (1 of which are hand-wired today)", out)
        self.assertIn("  -> IPTC::Main                   x1   binary:eligible", out)
        self.assertIn("  -> Kodak::IFD                    x1   no-layout", out)
        self.assertIn("  -> Olympus::Equipment              x2   enabled", out)
        self.assertEqual(dict(reasons), {"ifd_expr_domain_unknown": 1})

    def test_empty_ifd_allowlist_parses(self):
        allowed = reachability.parse_allowlist(reachability.IFD_ALLOWLIST, "ENABLED_IFD")
        self.assertEqual(allowed, set())

    def test_call_site_scan_keeps_the_two_lookups_apart(self):
        pat = re.compile(r'\bfind_table\(\s*"([^"]+)"\s*,\s*(?:"([^"]+)"|(\w+))\s*\)')
        self.assertIsNone(pat.search('find_ifd_table("Olympus", "Main")'))
        self.assertIsNotNone(pat.search('find_table("Olympus", "Main")'))


if __name__ == "__main__":
    unittest.main()
