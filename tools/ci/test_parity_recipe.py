"""Execute the documented parity recipe across the real bootstrap CLI seam.

Only the durable installation root and external oracle probes are substituted.
CLI parsing, verification path restrictions, hashing, manifest publication,
shell ordering and retained evidence copies remain real.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from tools.release import bootstrap_oracle as bootstrap


REPO = Path(__file__).resolve().parents[2]
CANONICAL_ROOT = "/Users/allen/oxidex-ops"
MANIFEST = "evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap/storage-manifest.json"
PERL = "toolchains/perl-5.38.2/prefix/bin/perl5.38.2"
CACHE = "cache/exiftool/13.59"


def bootstrap_cli(argv: list[str]) -> int:
    """Run main/verify with tiny real files instead of the installed corpus."""
    root = Path(os.environ["RECIPE_ROOT"])
    with (
        mock.patch.object(bootstrap, "DURABLE_ROOT", root),
        mock.patch.object(bootstrap, "MIN_CORPUS_FILES", 1),
        mock.patch.object(bootstrap, "_verify_corpus_tree"),
        mock.patch.object(bootstrap, "_verify_archive_sources", return_value={}),
        mock.patch.object(bootstrap, "run", side_effect=[
            bootstrap.LOCK["exiftool"]["tag_object"], "v5.38.2", "1.68",
            "13.59", "DOCX", str(root / "toolchains/perl-5.38.2/prefix"),
        ]),
        mock.patch.object(sys, "argv", ["bootstrap_oracle.py", *argv]),
    ):
        return bootstrap.main()


def record_boundary(kind: str, argv: list[str]) -> None:
    root = Path(os.environ["RECIPE_ROOT"])
    (root / f"{kind}.json").write_text(json.dumps({
        "argv": argv,
        "perl": os.environ.get("EXIFTOOL_PERL"),
        "cache": os.environ.get("EXIFTOOL_CACHE_DIR"),
    }), encoding="utf-8")


class ParityRecipeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="parity-recipe-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        files = {
            PERL: "synthetic perl\n",
            "toolchains/perl-5.38.2/prefix/lib/Archive/Zip.pm": "synthetic zip\n",
            f"{CACHE}/exiftool/exiftool": "synthetic exiftool\n",
            f"{CACHE}/exiftool/lib/Image/ExifTool.pm": "synthetic library\n",
            f"{CACHE}/exiftool/t/images/OOXML.docx": "synthetic docx\n",
            f"{CACHE}/combined-samples/sample.jpg": "synthetic sample\n",
            ".exiftool-version": "13.59\n",
            "lock.py": "unused heavy-job boundary\n",
            "tools/preflight.sh": "#!/bin/sh\nexit 0\n",
        }
        for relative, contents in files.items():
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents, encoding="utf-8")
        (self.root / "tools/preflight.sh").chmod(0o755)
        (self.root / "evidence").mkdir()

    def run_recipe(self, overrides: dict[str, str] | None = None):
        for kind in ("probe", "conformance"):
            (self.root / f"{kind}.json").unlink(missing_ok=True)
        text = (REPO / ".claude/skills/exiftool-parity/references/harnesses.md").read_text()
        snippets = re.findall(r"```bash\n(.*?)```", text, re.DOTALL)
        setup = snippets[0]
        conformance = next(s for s in snippets if "conformance.py" in s)
        # Substitute deployment locations only; do not rewrite flags or commands.
        recipe = (setup + "\n" + conformance).replace(CANONICAL_ROOT, str(self.root))
        recipe = recipe.replace("/absolute/durable/evidence/root", str(self.root / "evidence"))
        recipe = recipe.replace("/absolute/dedicated/parity-target", str(self.root / "target"))
        harness = r'''
python3() {
  case "$1" in
    tools/release/bootstrap_oracle.py)
      shift
      command "$RECIPE_PYTHON" -m tools.ci.test_parity_recipe --bootstrap-cli "$@" ;;
    tools/ci/release_oracle.py)
      command "$RECIPE_PYTHON" -m tools.ci.test_parity_recipe --record-probe "$@" ;;
    "$PARITY_LOCK")
      command "$RECIPE_PYTHON" -m tools.ci.test_parity_recipe --record-conformance "$@" ;;
    *) printf 'unexpected Python call\n' >&2; return 99 ;;
  esac
}
git() {
  case "$1" in
    rev-parse) printf '%040d\n' 1 ;;
    status) return 0 ;;
    *) return 99 ;;
  esac
}
'''
        environment = {**os.environ, "PYTHONPATH": str(REPO), "RECIPE_ROOT": str(self.root),
                       "RECIPE_PYTHON": sys.executable, "PYTHONDONTWRITEBYTECODE": "1",
                       "PARITY_LOCK": str(self.root / "lock.py"), "PARITY_MIN_FILES": "1",
                       "PARITY_MIN_TAGS": "1", "PARITY_BIN": str(self.root / "oxidex")}
        environment.pop("EXIFTOOL_PERL", None)
        environment.pop("EXIFTOOL_CACHE_DIR", None)
        environment.update(overrides or {})
        return subprocess.run(["bash", "-c", harness + recipe], cwd=self.root,
                              env=environment, text=True, capture_output=True, timeout=20)

    def test_recipe_verifies_canonical_manifest_before_measurement_and_retains_copy(self):
        result = self.run_recipe()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / "conformance.json").is_file())
        canonical = self.root / MANIFEST
        retained = list((self.root / "evidence").glob("parity-*/bootstrap-oracle.json"))
        self.assertEqual(len(retained), 1)
        self.assertEqual(retained[0].read_bytes(), canonical.read_bytes())
        digest = hashlib.sha256(canonical.read_bytes()).hexdigest()
        self.assertIn(digest, (retained[0].parent / "bootstrap-oracle.sha256").read_text())
        payload = json.loads(canonical.read_text())
        self.assertEqual(payload["artifacts"]["corpus_tree"]["path"],
                         str(self.root / CACHE / "combined-samples"))

    def test_failed_bootstrap_cannot_reach_measurement(self):
        (self.root / ".exiftool-version").write_text("13.60\n")
        result = self.run_recipe()
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("ExifTool pin must be 13.59", result.stderr)
        self.assertFalse((self.root / "conformance.json").exists())

    def test_inherited_oracle_paths_cannot_redirect_verified_measurement(self):
        alternate = self.root / "alternate-cache"
        alternate.mkdir()
        (alternate / "combined-samples.manifest").write_text("stale alternate manifest\n")
        canonical_perl = str(self.root / PERL)
        canonical_cache = str(self.root / CACHE)
        for overrides in (
            {"EXIFTOOL_PERL": canonical_perl, "EXIFTOOL_CACHE_DIR": canonical_cache},
            {"EXIFTOOL_PERL": str(self.root / "alternate-perl")},
            {"EXIFTOOL_CACHE_DIR": str(alternate)},
            {"EXIFTOOL_PERL": str(self.root / "alternate-perl"),
             "EXIFTOOL_CACHE_DIR": str(alternate)},
        ):
            with self.subTest(overrides=overrides):
                result = self.run_recipe(overrides)
                self.assertEqual(result.returncode, 0, result.stderr)
                manifest = json.loads((self.root / MANIFEST).read_text())["artifacts"]
                probe = json.loads((self.root / "probe.json").read_text())
                measured = json.loads((self.root / "conformance.json").read_text())
                self.assertEqual(probe["perl"], canonical_perl)
                self.assertEqual(measured["perl"], canonical_perl)
                self.assertEqual(measured["cache"], canonical_cache)
                self.assertEqual(measured["perl"], manifest["perl_executable"]["path"])
                argv = measured["argv"]
                corpus_start = argv.index("tools/exiftool-tables/conformance.py") + 1
                self.assertEqual(argv[corpus_start:corpus_start + 2], [
                    str(self.root / CACHE / "exiftool/t/images"),
                    manifest["corpus_tree"]["path"],
                ])
                self.assertEqual(argv[argv.index("--exiftool-dir") + 1],
                                 manifest["exiftool_tree"]["path"])
                self.assertEqual(probe["argv"][probe["argv"].index("--perl") + 1],
                                 manifest["perl_executable"]["path"])


if __name__ == "__main__":
    if sys.argv[1:2] == ["--bootstrap-cli"]:
        raise SystemExit(bootstrap_cli(sys.argv[2:]))
    if sys.argv[1:2] in (["--record-probe"], ["--record-conformance"]):
        record_boundary(sys.argv[1].removeprefix("--record-"), sys.argv[2:])
    else:
        unittest.main()
