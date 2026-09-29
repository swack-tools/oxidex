"""Bounded real-command controls for the authenticated raw-map capture."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import conformance
import version_rehearsal_raw_maps as raw_maps


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class RawMapCaptureTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name).resolve()
        self.checkout = root / "snapshot"
        self.checkout.mkdir()
        subprocess.run(["git", "init", "-q", str(self.checkout)], check=True)
        (self.checkout / "source.txt").write_text("signed source\n")
        subprocess.run(["git", "-C", str(self.checkout), "add", "source.txt"], check=True)
        subprocess.run(["git", "-C", str(self.checkout), "-c", "user.name=Test",
                        "-c", "user.email=test@example.invalid", "commit", "-qm", "source"], check=True)
        self.binary = root / "oxidex"
        self.binary.write_text("#!/usr/bin/env python3\nimport json\nfrom pathlib import Path\n"
                               "p=Path(__file__)\n"
                               "if p.with_suffix('.empty').exists(): print('{}'); raise SystemExit(0)\n"
                               "make=p.with_suffix('.make').read_text() if p.with_suffix('.make').exists() else 'Canon'\n"
                               "date=p.with_suffix('.volatile').read_text() if p.with_suffix('.volatile').exists() else 'now'\n"
                               "print(json.dumps([{'File:FileType':'JPEG','EXIF:Make':make,'File:FileAccessDate':date}]))\n")
        self.binary.chmod(0o755)
        self.native = root / "native"
        (self.native / "lib" / "Image").mkdir(parents=True)
        (self.native / "lib" / "Image" / "ExifTool.pm").write_text("native\n")
        (self.native / "exiftool").write_text("native command\n")
        self.perl = root / "perl"
        self.perl.write_text("#!/usr/bin/env python3\nimport json\nfrom pathlib import Path\n"
                             "p=Path(__file__)\n"
                             "date=p.with_suffix('.volatile').read_text() if p.with_suffix('.volatile').exists() else 'now'\n"
                             "print(json.dumps([{'File:FileType':'JPEG','EXIF:IFD0:Make':'Canon','File:FileAccessDate':date}]))\n")
        self.perl.chmod(0o755)
        self.fixture = root / "fixture.jpg"
        self.fixture.write_bytes(b"fixture")
        self.entry = {"source": str(self.fixture), "sha256": sha(self.fixture),
                      "bytes": self.fixture.stat().st_size,
                      "corpus_path": str(self.fixture), "corpus_sha256": sha(self.fixture),
                      "corpus_bytes": self.fixture.stat().st_size}
        oracle = {"argv": [str(self.perl), str(self.native / "exiftool"), "-config", ""]}
        native_map = {"File:FileType": "JPEG", "EXIF:IFD0:Make": "Canon",
                      "File:FileAccessDate": "now"}
        candidate_map = {"File:FileType": "JPEG", "EXIF:Make": "Canon",
                         "File:FileAccessDate": "now"}
        row = conformance.transcript_row(str(self.fixture), native_map, candidate_map,
                                         conformance.compare(native_map, candidate_map))
        transcript = conformance.measurement_transcript([row])
        self.report = root / "conformance.json"
        self.report.write_text(json.dumps({"instrument": {
            "repo": {"root": str(self.checkout),
                     "commit": raw_maps._git(self.checkout, "rev-parse", "HEAD"),
                     "tree": raw_maps._git(self.checkout, "rev-parse", "HEAD^{tree}"),
                     "dirty": False, "dirty_files": [], "dirty_overridden": False},
            "binary": {"path": str(self.binary), "sha256": sha(self.binary)},
            "oracle": oracle, "measurement_transcript": transcript}}))

    def capture(self) -> dict:
        return raw_maps.capture_authenticated_maps(
            self.report, [self.entry], self.checkout, self.binary, self.perl, self.native)

    def test_real_commands_bind_exact_maps_and_replay(self) -> None:
        captured = self.capture()
        self.assertEqual(captured["rows"][0]["oracle_raw_map"]["EXIF:IFD0:Make"], "Canon")
        self.assertEqual(captured["rows"][0]["candidate_raw_map"]["EXIF:Make"], "Canon")
        self.assertEqual(captured["rows"][0]["oracle_ordered_pairs"][1],
                         ["EXIF:IFD0:Make", "Canon"])
        self.assertEqual(captured["rows"][0]["native_status"], 0)
        self.assertEqual(captured["rows"][0]["native_command"]["argv"][-5:],
                         ["-G0:1:4", "-s", "-j", "-a", str(self.fixture)])
        raw_maps.validate_capture(captured, self.report, [self.entry], self.checkout,
                                  self.binary, self.perl, self.native)
        self.binary.write_text(self.binary.read_text().replace("Canon", "Nikon"))
        with self.assertRaisesRegex(raw_maps.Refused, "built binary"):
            self.capture()

    def test_duplicate_json_keys_refuse_instead_of_collapsing(self) -> None:
        with self.assertRaisesRegex(raw_maps.Refused, "duplicate JSON key"):
            raw_maps._json_map(b'[{"EXIF:Make":"A","EXIF:Make":"B"}]', oracle=True)

    def test_fixture_and_transcript_drift_refuse(self) -> None:
        saved = self.capture()
        self.fixture.write_bytes(b"changed")
        with self.assertRaisesRegex(raw_maps.Refused, "fixture changed"):
            raw_maps.validate_capture(saved, self.report, [self.entry], self.checkout,
                                      self.binary, self.perl, self.native)

    def test_changed_candidate_with_rebound_binary_still_refuses_transcript(self) -> None:
        self.binary.with_suffix(".make").write_text("Nikon")
        report = json.loads(self.report.read_text())
        self.report.write_text(json.dumps(report))
        with self.assertRaisesRegex(raw_maps.Refused, "authenticated transcript"):
            self.capture()

    def test_ignored_access_date_drift_keeps_original_bytes_and_accepts(self) -> None:
        saved = self.capture()
        original_sha = saved["rows"][0]["native_command"]["stdout_sha256"]
        self.perl.with_suffix(".volatile").write_text("later")
        self.binary.with_suffix(".volatile").write_text("later")
        current = self.capture()
        self.assertNotEqual(current["rows"][0]["native_command"]["stdout_sha256"], original_sha)
        raw_maps.validate_capture(saved, self.report, [self.entry], self.checkout,
                                  self.binary, self.perl, self.native)

    def test_saved_original_stdout_tamper_refuses(self) -> None:
        saved = self.capture()
        saved["rows"][0]["native_command"]["stdout_base64"] = "e30="
        with self.assertRaisesRegex(raw_maps.Refused, "command byte hashes changed"):
            raw_maps.validate_capture(saved, self.report, [self.entry], self.checkout,
                                      self.binary, self.perl, self.native)

    def test_empty_candidate_still_requires_its_command_receipt(self) -> None:
        self.binary.with_suffix(".empty").write_text("")
        report = json.loads(self.report.read_text())
        native_map = {"File:FileType": "JPEG", "EXIF:IFD0:Make": "Canon",
                      "File:FileAccessDate": "now"}
        expected = conformance.transcript_row(
            str(self.fixture), native_map, {}, conformance.compare(native_map, {}))
        report["instrument"]["measurement_transcript"] = conformance.measurement_transcript([expected])
        self.report.write_text(json.dumps(report))
        saved = self.capture()
        self.assertEqual(saved["rows"][0]["candidate_raw_map"], {})
        self.assertEqual(saved["rows"][0]["candidate_command"]["status"], 0)
        saved["rows"][0]["candidate_command"] = None
        with self.assertRaisesRegex(raw_maps.Refused, "candidate command is missing"):
            raw_maps.validate_capture(saved, self.report, [self.entry], self.checkout,
                                      self.binary, self.perl, self.native)


if __name__ == "__main__":
    unittest.main()
