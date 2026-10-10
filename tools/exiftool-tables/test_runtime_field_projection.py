"""Small raw-child controls for the Task18 field proof boundary."""
import copy
import hashlib
import json
from pathlib import Path
import struct
import tempfile
import unittest

import runtime_field_projection as projection


class FieldProjectionControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.tag = 0x013b
        source = json.loads((Path(projection.__file__).parent / "conv_exif_main_ledger.json").read_text())
        definition = next(item for item in source["generated"] if item["id"] == "0x013b")
        self.row = {"module": "Exif", "table": "Main", "field": {"kind": "numeric", "value": "0x013b"},
                    "owner": "generated", "symbol": "src/exiftool_tables/conv/exif_main.rs::decode",
                    "source_sha256": definition["source_sha256"]}
        self.stable = "Exif::Main:numeric:0x013b"

    def carrier(self, *, tag=None):
        # A minimal little-endian TIFF with one actual IFD0 entry.
        tag = self.tag if tag is None else tag
        raw = b"II\x2a\x00" + struct.pack("<I", 8) + struct.pack("<H", 1)
        raw += struct.pack("<HHII", tag, 2, 2, 0x41) + struct.pack("<I", 0)
        path = self.root / "artist.tif"
        path.write_bytes(raw)
        digest = hashlib.sha256(raw).hexdigest()
        return {"relative_path": "artist.tif", "staged_path": str(path), "sha256": digest}

    def receipt(self, oracle, on, off, *, tag=None, token="engine"):
        manifest = self.carrier(tag=tag)
        children = {}
        for mode, candidate in (("control-empty", on), (token, off)):
            folder = self.root / mode
            folder.mkdir(exist_ok=True)
            process = {"source_sha256": manifest["sha256"], "staged_sha256": manifest["sha256"]}
            for side, value in (("oracle", oracle), ("candidate", candidate)):
                path = folder / f"{side}.stdout"
                path.write_text(json.dumps([value]))
                process[side] = {"stdout": {"path": str(path)}}
            path = folder / "process.json"
            path.write_text(json.dumps(process))
            children[mode] = {"relative_path": "artist.tif", "process": {"path": str(path)}}
        return {"selection": {"ordered_paths": ["artist.tif"], "ordered_manifest": [manifest]},
                "runs": {mode: {"children": [child]} for mode, child in children.items()}}

    def spec(self, receipt, oracle_key="EXIF:IFD0:Artist", candidate_key="IFD0:Artist", token="engine"):
        def occurrence(mode, side, key):
            return [row for row in projection._child(receipt, mode, "artist.tif", side)
                    if row["raw_key"] == key]
        return {"source_field": self.stable, "stable_field_id": self.stable, "owner": self.row["symbol"],
                "carrier": "artist.tif", "token": token, "oracle_key": oracle_key,
                "candidate_key": candidate_key, "on": occurrence("control-empty", "candidate", candidate_key),
                "off": occurrence(token, "candidate", candidate_key),
                "fallback_carrier": None, "fallback_on": [], "fallback_off": []}

    def project(self, receipt, spec):
        return projection.project(receipt, [spec], [self.stable], [self.row])

    def test_actual_ifd0_field_is_required(self):
        receipt = self.receipt({"EXIF:IFD0:Artist": "A"}, {"IFD0:Artist": "A"}, {})
        self.assertEqual(self.project(receipt, self.spec(receipt))[0]["on_count"], 1)
        wrong = self.receipt({"EXIF:IFD0:Artist": "A"}, {"IFD0:Artist": "A"}, {}, tag=0x0110)
        with self.assertRaisesRegex(projection.BlockedAttribution, "BLOCKED_ATTRIBUTION"):
            self.project(wrong, self.spec(wrong))

    def test_foreign_artist_and_wrong_token_cannot_claim_exif_main(self):
        foreign = self.receipt({"Real-RA4:Artist": "A"}, {"Real-RA4:Artist": "A"}, {}, token="serial")
        spec = self.spec(foreign, "Real-RA4:Artist", "Real-RA4:Artist", "serial")
        with self.assertRaisesRegex(projection.BlockedAttribution, "BLOCKED_ATTRIBUTION"):
            self.project(foreign, spec)

    def test_manifest_and_owner_provenance_are_required(self):
        receipt = self.receipt({"EXIF:IFD0:Artist": "A"}, {"IFD0:Artist": "A"}, {})
        spec = self.spec(receipt)
        receipt["selection"]["ordered_manifest"][0]["sha256"] = "0" * 64
        with self.assertRaisesRegex(projection.BlockedAttribution, "carrier bytes"):
            self.project(receipt, spec)
        receipt["selection"]["ordered_manifest"][0]["sha256"] = hashlib.sha256(
            (self.root / "artist.tif").read_bytes()).hexdigest()
        bad_owner = dict(self.row, owner="residual")
        with self.assertRaisesRegex(projection.BlockedAttribution, "source owner"):
            projection.project(receipt, [spec], [self.stable], [bad_owner])

    def test_jpeg_carrier_requires_one_exif_segment(self):
        tiff = self.carrier()
        payload = b"Exif\x00\x00" + Path(tiff["staged_path"]).read_bytes()
        segment = b"\xff\xe1" + struct.pack(">H", len(payload) + 2) + payload
        self.assertEqual(projection._ifd0_tags(b"\xff\xd8" + segment + b"\xff\xd9"), [self.tag])
        with self.assertRaisesRegex(projection.BlockedAttribution, "unique"):
            projection._ifd0_tags(b"\xff\xd8" + segment + segment + b"\xff\xd9")

    def test_raw_json_types_and_numeric_spelling(self):
        parsed = projection.parse_field_json(b'[{"A":1.0,"B":"1.0","C":true,"D":1,"E":1.00000000000000001}]', "candidate")
        rows = projection.field_occurrences(parsed)
        self.assertNotEqual(rows[0]["type"], rows[1]["type"])
        self.assertNotEqual(rows[2]["type"], rows[3]["type"])
        self.assertEqual(rows[4]["raw_serialized"], "1.00000000000000001")
        receipt = self.receipt({"EXIF:IFD0:Artist": 1.0}, {"IFD0:Artist": "1.0"}, {})
        with self.assertRaisesRegex(projection.BlockedAttribution, "oracle value differs"):
            self.project(receipt, self.spec(receipt))

    def test_project_refuses_number_string_and_bool_int(self):
        for oracle_value, candidate_value in ((1.0, "1.0"), (True, 1)):
            with self.subTest(oracle_value=oracle_value, candidate_value=candidate_value):
                receipt = self.receipt({"EXIF:IFD0:Artist": oracle_value},
                                       {"IFD0:Artist": candidate_value}, {})
                with self.assertRaisesRegex(projection.BlockedAttribution, "oracle value differs"):
                    self.project(receipt, self.spec(receipt))

    def test_numeric_spelling_is_part_of_field_proof(self):
        receipt = self.receipt({"EXIF:IFD0:Artist": 1.0}, {"IFD0:Artist": 1.0}, {})
        spec = self.spec(receipt)
        process = json.loads(Path(receipt["runs"]["control-empty"]["children"][0]["process"]["path"]).read_text())
        Path(process["oracle"]["stdout"]["path"]).write_text('[{"EXIF:IFD0:Artist":1.00000000000000001}]')
        with self.assertRaisesRegex(projection.BlockedAttribution, "oracle value differs"):
            self.project(receipt, spec)

    def test_multiple_oracle_copies_refuse(self):
        oracle = {"EXIF:IFD0:Artist": "A", "EXIF:IFD0:Copy1:Artist": "B"}
        receipt = self.receipt(oracle, {"IFD0:Artist": "A"}, {})
        with self.assertRaisesRegex(projection.BlockedAttribution, "multi-copy"):
            self.project(receipt, self.spec(receipt))

    def test_signed_position_is_exact_but_fallback_comparison_is_relative(self):
        receipt = self.receipt({"EXIF:IFD0:Artist": "A"}, {"IFD0:Artist": "A"}, {})
        spec = self.spec(receipt)
        changed = copy.deepcopy(spec)
        changed["on"][0]["position"] += 1
        with self.assertRaisesRegex(projection.BlockedAttribution, "typed ordered"):
            self.project(receipt, changed)

        fallback = self.root / "declined.tif"
        fallback.write_bytes((self.root / "artist.tif").read_bytes())
        digest = hashlib.sha256(fallback.read_bytes()).hexdigest()
        receipt["selection"]["ordered_paths"].append("declined.tif")
        receipt["selection"]["ordered_manifest"].append(
            {"relative_path": "declined.tif", "staged_path": str(fallback), "sha256": digest})
        for mode, candidate in (("control-empty", {"IFD0:Make": "M", "IFD0:Artist": "A"}),
                                ("engine", {"IFD0:Artist": "A"})):
            folder = self.root / f"fallback-{mode}"
            folder.mkdir()
            process = {"source_sha256": digest, "staged_sha256": digest}
            for side, output in (("oracle", {"EXIF:IFD0:Artist": "A"}), ("candidate", candidate)):
                path = folder / f"{side}.stdout"
                path.write_text(json.dumps([output]))
                process[side] = {"stdout": {"path": str(path)}}
            path = folder / "process.json"
            path.write_text(json.dumps(process))
            receipt["runs"][mode]["children"].append(
                {"relative_path": "declined.tif", "process": {"path": str(path)}})
        spec["fallback_carrier"] = "declined.tif"
        spec["fallback_on"] = [row for row in projection._child(receipt, "control-empty", "declined.tif", "candidate")
                               if row["raw_key"] == "IFD0:Artist"]
        spec["fallback_off"] = [row for row in projection._child(receipt, "engine", "declined.tif", "candidate")
                                if row["raw_key"] == "IFD0:Artist"]
        residual = {"module": "Exif", "table": "Main", "field": {"kind": "index", "value": "IFD0/0x013b"},
                    "residual_disposition": "fallback-on-decline"}
        self.assertEqual(projection.project(receipt, [spec], [self.stable], [self.row, residual])[0]["fallback_count"], 1)
        changed = copy.deepcopy(spec)
        changed["fallback_off"][0]["position"] += 1
        with self.assertRaisesRegex(projection.BlockedAttribution, "typed ordered"):
            projection.project(receipt, [changed], [self.stable], [self.row, residual])
