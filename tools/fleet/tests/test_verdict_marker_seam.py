#!/usr/bin/env python3
"""T2 -- one filename, written by a shell script and read by a daemon, and
the test that stops the two from drifting.

THE BUG THIS CLOSES. `gate.sh`'s `store_verdict()` writes
`$HOME/gatelogs/gate-<tag>.verdict-store-failed` when it could not push a
gate's verdict to the hub cache (R4). `fleetd`'s reap loop reads that exact
path back and turns it into a `refused` reason, and (T3) `HostWarnings`
sweeps the directory for it. The filename therefore has to be identical on
both sides -- and until this file existed it was spelled TWICE, once as a
shell string in `gate.sh` and once as a Python f-string in
`fleetd._verdict_store_failed_marker`, with nothing anywhere comparing them.

Renaming either spelling leaves the ENTIRE suite green. `gate.sh`'s own
tests (`test_gate_script.py::TestStoreVerdictLoudFailure`) set `SV=`
themselves before calling the extracted function, so they never see
gate.sh's real `SV=` line; `fleetd`'s tests
(`test_fleetd.py::TestGateVerdictStoreFailureSurfaced`) write the marker
using fleetd's OWN helper, so they agree with whatever fleetd currently
believes. Both halves are individually well tested and the seam between
them was tested by nobody. The production symptom is silence: gate.sh keeps
writing a marker, fleetd keeps looking for a file that is not there, and
the verdict-store failure the marker exists to surface goes back to being
invisible -- with no error, on any host, ever.

THE FIX, and what this file pins. The suffix is spelled in exactly two
places -- `config.py`'s `VERDICT_STORE_FAILED_SUFFIX` (Python) and
`units/fleet-env.sh`'s `FLEET_VERDICT_STORE_FAILED_SUFFIX` (shell, which
`gate.sh` already sources for `EXIFTOOL_CACHE_DIR`) -- and this file:

  1. EVALUATES gate.sh's own `SV=` line in a real shell, having sourced the
     real `units/fleet-env.sh`, and compares the resulting path byte for
     byte against `config.verdict_store_failed_marker()` and
     `fleetd._verdict_store_failed_marker()`. Not a grep for a literal:
     what a shell assignment expands to is the only thing gate.sh actually
     writes.
  2. Proves gate.sh READS the variable rather than embedding the value, by
     sourcing a stand-in env file that exports a different suffix and
     requiring the derived path to follow it. Without this control, a
     gate.sh that hardcoded the string would pass step 1 forever.
  3. Pins the two canonical literals against each other, and forbids the
     suffix from being spelled anywhere else in `gate.sh` or `fleetd.py`.

Instrument: `bash` for the shell half (the real `gate.sh` text, the real
`fleet-env.sh`), plain imports for the Python half. Nothing is executed
against a hub and nothing is written outside a tempdir.

Run with:
    python3 -m unittest tools.fleet.tests.test_verdict_marker_seam -v
"""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import config  # noqa: E402
import fleetd  # noqa: E402

GATE_SH = FLEET_DIR / "gate.sh"
FLEET_ENV_SH = FLEET_DIR / "units" / "fleet-env.sh"

_SV_LINE_RE = re.compile(r"^SV=.*$", re.MULTILINE)
_SOURCE_ENV_RE = re.compile(r'^\. "\$SELF_DIR/units/fleet-env\.sh"$', re.MULTILINE)


def _sv_assignment() -> str:
    """gate.sh's own `SV=` line, verbatim."""
    matches = _SV_LINE_RE.findall(GATE_SH.read_text(encoding="utf-8"))
    assert len(matches) == 1, f"expected exactly one SV= line in gate.sh, got {matches}"
    return matches[0]


def _expand_sv(home: str, tag: str, env_file: Path) -> str:
    """What gate.sh's `SV=` line expands to, run by bash, with `env_file`
    sourced first exactly the way gate.sh sources `units/fleet-env.sh`.

    `HOME` is set in the script rather than in the process environment so
    the expansion is reproducible regardless of who runs the suite.
    """
    script = (
        f"HOME={home!r}\n"
        f"TAG={tag!r}\n"
        f". {str(env_file)!r}\n"
        f"{_sv_assignment()}\n"
        'printf "%s" "$SV"\n'
    ).replace("'", '"')  # repr() quotes with ', bash is happier with "
    result = subprocess.run(["bash", "-c", script], capture_output=True, text=True, timeout=15)
    assert result.returncode == 0, result.stderr
    return result.stdout


class TestGateShAndFleetdAgreeOnTheMarkerPath(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        self.home = str(self.tmp / "home")
        self.tag = "m5-staging-example-123"

    def test_the_path_gate_sh_writes_is_the_path_fleetd_reads(self):
        """The seam itself, and the only test in the tree that crosses it.

        Left side: bash, expanding gate.sh's real `SV=` line after sourcing
        the real `units/fleet-env.sh`. Right side: `fleetd`'s helper, which
        is what the reap loop and the durable sweep both call.
        """
        from_gate_sh = _expand_sv(self.home, self.tag, FLEET_ENV_SH)
        from_fleetd = fleetd._verdict_store_failed_marker(
            Path(self.home) / "gatelogs", self.tag)
        self.assertEqual(from_gate_sh, str(from_fleetd))
        self.assertEqual(from_gate_sh,
                         str(config.verdict_store_failed_marker(
                             Path(self.home) / "gatelogs", self.tag)))

    def test_gate_sh_reads_the_variable_rather_than_spelling_the_suffix(self):
        """The control for the test above.

        A `gate.sh` that still hardcoded `.verdict-store-failed` would
        satisfy the equality above forever -- the two sides would agree by
        coincidence, exactly as they did before this seam had a single
        source. So: source a stand-in env file that exports a DIFFERENT
        suffix and require gate.sh's own line to follow it.
        """
        fake_env = self.tmp / "fake-fleet-env.sh"
        fake_env.write_text(
            ': "${FLEET_VERDICT_STORE_FAILED_SUFFIX:=.SENTINEL-SUFFIX}"\n'
            "export FLEET_VERDICT_STORE_FAILED_SUFFIX\n"
        )
        followed = _expand_sv(self.home, self.tag, fake_env)
        self.assertTrue(
            followed.endswith(".SENTINEL-SUFFIX"),
            f"gate.sh's SV= line ignored FLEET_VERDICT_STORE_FAILED_SUFFIX "
            f"and produced {followed!r} -- it is spelling the suffix itself",
        )
        self.assertNotEqual(followed, _expand_sv(self.home, self.tag, FLEET_ENV_SH))

    def test_gate_sh_sources_the_env_file_before_it_builds_the_path(self):
        """Order, not just presence: sourcing `fleet-env.sh` AFTER the
        `SV=` line would expand the variable to the empty string and write
        `gate-<tag>` with no suffix at all -- a filename that collides with
        nothing, matches no glob, and is never read."""
        text = GATE_SH.read_text(encoding="utf-8")
        source_at = _SOURCE_ENV_RE.search(text)
        sv_at = _SV_LINE_RE.search(text)
        self.assertIsNotNone(source_at, "gate.sh no longer sources units/fleet-env.sh")
        self.assertIsNotNone(sv_at)
        self.assertLess(source_at.start(), sv_at.start(),
                        "gate.sh builds SV before it sources units/fleet-env.sh")

    def test_an_unset_variable_cannot_silently_produce_a_suffixless_path(self):
        """Belt and braces on the same failure: with nothing sourced at
        all, the expansion must NOT quietly yield the bare `gate-<tag>`
        path. This is the shape a future edit that drops the `. fleet-env.sh`
        line would produce, and it is silent -- the marker is still written,
        just under a name nothing looks for.
        """
        empty_env = self.tmp / "empty.sh"
        empty_env.write_text("# exports nothing\n")
        bare = _expand_sv(self.home, self.tag, empty_env)
        self.assertNotEqual(
            bare, _expand_sv(self.home, self.tag, FLEET_ENV_SH),
            "gate.sh produces the same marker path with and without its env "
            "file, so the variable is not load-bearing and this whole seam "
            "is decoration",
        )


class TestTheSuffixIsSpelledInExactlyTwoPlaces(unittest.TestCase):
    def test_config_py_and_fleet_env_sh_agree(self):
        """The two canonical definitions. Each file is read by a different
        language and neither ever compares itself to the other, so a
        divergence is invisible until a host writes one name and reads the
        other."""
        py_src = (FLEET_DIR / "config.py").read_text()
        m = re.search(r'^VERDICT_STORE_FAILED_SUFFIX = "([^"]+)"', py_src, re.MULTILINE)
        self.assertIsNotNone(
            m, "config.py must define VERDICT_STORE_FAILED_SUFFIX as a plain literal")
        py_value = m.group(1)

        sh_src = FLEET_ENV_SH.read_text()
        m = re.search(r'FLEET_VERDICT_STORE_FAILED_SUFFIX:=([^}]+)\}', sh_src)
        self.assertIsNotNone(
            m, "units/fleet-env.sh must set FLEET_VERDICT_STORE_FAILED_SUFFIX "
               "via ${VAR:=...}")
        sh_value = m.group(1)

        self.assertEqual(py_value, sh_value,
                         f"config.py ({py_value!r}) and units/fleet-env.sh "
                         f"({sh_value!r}) have drifted apart")
        self.assertEqual(py_value, config.VERDICT_STORE_FAILED_SUFFIX)

    def test_no_third_spelling_survives_in_gate_sh_or_fleetd(self):
        """The fence. Both files used to carry the literal; a re-introduced
        copy is exactly the state this seam was in before, and it looks
        harmless right up until somebody renames the constant."""
        suffix = config.VERDICT_STORE_FAILED_SUFFIX
        for path in (GATE_SH, FLEET_DIR / "fleetd.py"):
            hits = [
                (n, line) for n, line in enumerate(path.read_text().splitlines(), 1)
                if suffix in line and not line.lstrip().startswith("#")
            ]
            self.assertEqual(
                hits, [],
                f"{path.name} spells {suffix!r} itself; it must come from "
                f"{'units/fleet-env.sh' if path.suffix == '.sh' else 'config.py'}",
            )


class TestMarkerHelpersRoundTrip(unittest.TestCase):
    """`verdict_store_failed_marker` / `_tag` / `_glob` are one another's
    inverses, because `HostWarnings.scan` composes all three: it globs the
    directory, then recovers the tag from each filename to name the gate in
    the warning. A glob that does not match what the composer writes yields
    a sweep that finds nothing and reports nothing wrong."""

    def test_marker_then_tag_is_the_identity(self):
        for tag in ("m5-abc", "train-staging-alpha-42", "a", "x.y-z"):
            with self.subTest(tag=tag):
                marker = config.verdict_store_failed_marker("/logs", tag)
                self.assertEqual(config.verdict_store_failed_tag(marker.name), tag)

    def test_the_glob_matches_what_the_composer_writes(self):
        import fnmatch
        marker = config.verdict_store_failed_marker("/logs", "some-tag")
        self.assertTrue(fnmatch.fnmatch(marker.name, config.verdict_store_failed_glob()))

    def test_the_glob_does_not_match_the_gates_other_artefacts(self):
        """`~/gatelogs` also holds `gate-<tag>.log`, `.verdict`, `.json`
        and fleetd's own `fleetd-gate-*` launch logs. A sweep that matched
        any of those would report a warning on every healthy gate."""
        import fnmatch
        pattern = config.verdict_store_failed_glob()
        for name in ("gate-t.log", "gate-t.verdict", "gate-t.json",
                     "fleetd-gate-t.launch.log", "fleetd-agent-t.log"):
            with self.subTest(name=name):
                self.assertFalse(fnmatch.fnmatch(name, pattern))

    def test_tag_returns_none_for_a_non_marker(self):
        for name in ("gate-t.log", "notagate.verdict-store-failed", "gate-.verdict-store-failed"):
            with self.subTest(name=name):
                self.assertIsNone(config.verdict_store_failed_tag(name))


if __name__ == "__main__":
    unittest.main()
