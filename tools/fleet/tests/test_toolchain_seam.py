#!/usr/bin/env python3
"""L1 -- one host, two `platform_id`s, and the tests that stop it.

THE INCIDENT (Keel Stage 1 LIVE acceptance run, 2026-08-27/28, real
private state repo, real gate on the i7). fleetd's host claim on `server`
recorded

    platform_id b2bdf493bcf6d1dc55181a0efc2774b6d9cf7bf5c8dcc5bed592c994b2ad38c4

while the gate that *the same fleetd had just spawned* wrote its verdict,
the same minute on the same host, under

    platform_id b6613b194bef01c71d7e040e4a15ba8999e2b8263b61c09e156819ccb213485a

`platform_id` is one third of the verdict cache key
(`verdict.verdict_ref`), so `verdict.lookup` under fleetd's key missed a
tree whose PASS was already published under the gate's key.
`classify_branch` never returned AWAITING_TRAIN, and a host with
`gates >= 1` re-gated the identical merge tree forever -- about
twenty-one minutes a pass -- while the answer it was paying for sat
unread. Proven on the i7 inside fleetd's own environment with
`verdict.lookup`: fleetd's key -> None, the gate's key -> PASS.

THE ROOT CAUSE WAS NOT WHICH COMPILER. Both sides resolved the same
rustc 1.97.1 out of `~/.cargo/bin`. The two ids differed by ONE TRAILING
NEWLINE: `RUSTC_VV=$(rustc -vV)` strips it, `subprocess.run(...).stdout`
keeps it. Measured on the real i7:

    sha256(vv)             = b2bdf493...   <- what fleetd stored
    sha256(vv.rstrip("\\n")) = b6613b19...   <- what the gate stored

Three implementations existed and no two agreed on both fields:

               platform_id   rustc_id
    gate.sh    b6613b19      b5d14336
    claim.py   b2bdf493      12562484
    verdict.py b2bdf493      b5d14336

WHY NOTHING CAUGHT IT. Stage 1's acceptance bullet asserted that
`git ls-remote 'refs/fleet/verdicts/*'` listed `(tree, 7, <i7
platform_id>)` -- true in the broken state, because the GATE's key was
perfectly well formed. An assertion that A value exists cannot catch two
components disagreeing about the value. `verdict.compute_ids`'s docstring
promised it computed things "the same way `gate.sh` does", and nothing
anywhere compared the two.

WHAT THIS FILE PINS.

  1. `TestGateShAndThePythonSideAgree` -- the seam itself. The left side
     is `bash`, running gate.sh's OWN source-and-call lines, lifted
     verbatim out of the real script text. The right side is
     `toolchain.compute_ids`. The test never spells the formula: a test
     that re-implements the thing under test proves only that its author
     and the code made the same mistake, which is exactly what
     `verdict.compute_ids` already was.
  2. Its control: point the resolver at a stand-in that emits sentinel
     ids and require gate.sh's own lines to FOLLOW it. Without this, a
     gate.sh that went back to computing the digest inline would satisfy
     (1) forever by coincidence.
  3. `TestTheMismatchCheckFires` -- `fleetd.check_toolchain_agreement`
     against a gate whose resolver is MADE to disagree, plus the
     negative control (an agreeing resolver must be silent) and the real
     `tools/fleet/gate.sh` (which must agree on this host).
  4. `TestTheRecordedIncidentBytes` -- the literal `rustc -vV` text the
     i7 produces, pinned to the digests the gate actually published, so
     a future change to the normalization has to argue with the incident
     rather than with a synthetic fixture.
  5. `TestThePathPrefixIsSpelledInExactlyTwoPlaces` and
     `TestNoFourthImplementation` -- the mirrored-literal fence and a
     grep-level guard against a fresh copy of the formula.

Instrument: `bash` for the shell half (the real `gate.sh` text, the real
`units/fleet-toolchain.sh`), plain imports for the Python half. Nothing
is executed against a hub; nothing is written outside a tempdir.

Run with:
    python3 -m unittest tools.fleet.tests.test_toolchain_seam -v
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import claim  # noqa: E402
import fleetd  # noqa: E402
import toolchain  # noqa: E402
import verdict  # noqa: E402
from _env import HermeticCase, scrub_env  # noqa: E402

GATE_SH = FLEET_DIR / "gate.sh"
FLEET_TOOLCHAIN_SH = FLEET_DIR / "units" / "fleet-toolchain.sh"

# gate.sh's own two lines. Matched against the real script text so that a
# gate.sh which stops sourcing the resolver, or stops calling it, fails
# HERE with a clear message instead of silently testing nothing.
# Matched WITHOUT a `$` anchor: gate.sh follows the source with an
# `|| { ...; exit 7; }` guard (a missing resolver must not surface as
# bash's own "unbound variable"), and lifting that continuation into the
# script below would leave an unbalanced brace. The `.` command itself is
# the whole of what this test needs to execute.
_SOURCE_LINE_RE = re.compile(r'^\. "\$SELF_DIR/units/fleet-toolchain\.sh"', re.MULTILINE)
_CALL_LINE_RE = re.compile(r"^fleet_toolchain_ids.*$", re.MULTILINE)

# The exact bytes `PATH=$HOME/.cargo/bin:$PATH rustc -vV` printed on the
# i7 (`server`, x86_64-unknown-linux-gnu) on 2026-08-28, captured with
# `cat -A` so the line terminators are not a guess -- this is the input
# that produced the incident.
I7_RUSTC_VV = (
    "rustc 1.97.1 (8bab26f4f 2026-07-14)\n"
    "binary: rustc\n"
    "commit-hash: 8bab26f4f68e0e26f0bb7960be334d5b520ea452\n"
    "commit-date: 2026-07-14\n"
    "host: x86_64-unknown-linux-gnu\n"
    "release: 1.97.1\n"
    "LLVM version: 22.1.6\n"
)
# What `gate.sh` published, and therefore what the fleet's verdict cache
# is keyed by. Adopting any OTHER normalization orphans every verdict
# already on the state repo.
I7_GATE_PLATFORM_ID = "b6613b194bef01c71d7e040e4a15ba8999e2b8263b61c09e156819ccb213485a"
# What `claim.compute_platform_id` stored instead -- the bug's value. It
# must never be produced again by anything.
I7_FLEETD_PLATFORM_ID_BEFORE = "b2bdf493bcf6d1dc55181a0efc2774b6d9cf7bf5c8dcc5bed592c994b2ad38c4"
# `doctor.CANONICAL_TOOLCHAIN_ID`, derived under gate.sh's formula in
# 2026-08 and unchanged by this fix -- the independent witness that
# gate.sh's spelling, not claim.py's, was the fleet's real canon.
I7_RUSTC_ID = "b5d143364ae0334870dfbce0e72e0ea6ecb1bc07d68d023ab6c88b6d20f58577"


def _gate_sh_line(pattern: re.Pattern, what: str) -> str:
    matches = pattern.findall(GATE_SH.read_text(encoding="utf-8"))
    assert len(matches) == 1, f"expected exactly one {what} line in gate.sh, got {matches}"
    return matches[0]


def _run_gate_sh_toolchain_lines(env: "dict | None" = None) -> dict:
    """`{"PLATFORM_ID", "RUSTC_ID"}` produced by GATE.SH'S OWN LINES.

    The script below contains no formula: it sets `SELF_DIR` the way
    gate.sh does, then executes gate.sh's literal source line and its
    literal `fleet_toolchain_ids` call, both lifted out of the real file.
    Everything that decides the answer therefore comes from the code
    under test.
    """
    script = "\n".join((
        f'SELF_DIR="{FLEET_DIR}"',
        _gate_sh_line(_SOURCE_LINE_RE, "resolver source"),
        _gate_sh_line(_CALL_LINE_RE, "fleet_toolchain_ids call"),
        'printf "PLATFORM_ID=%s\\nRUSTC_ID=%s\\nERROR=%s\\n" '
        '"$PLATFORM_ID" "$RUSTC_ID" "$FLEET_TOOLCHAIN_ERROR"',
        "",
    ))
    result = subprocess.run(
        ["bash", "-c", script], capture_output=True, text=True, timeout=60,
        env=scrub_env() if env is None else env,
    )
    assert result.returncode == 0, f"{result.stdout}\n{result.stderr}"
    out = {}
    for line in result.stdout.splitlines():
        key, _, value = line.partition("=")
        out[key] = value
    return out


class TestGateShAndThePythonSideAgree(HermeticCase):
    """The seam. Bash on the left, Python on the right, one host."""

    def test_the_platform_id_gate_sh_writes_is_the_one_fleetd_looks_up(self):
        from_gate_sh = _run_gate_sh_toolchain_lines()
        self.assertEqual(from_gate_sh.get("ERROR"), "",
                         f"gate.sh's resolver reported an error: {from_gate_sh}")
        expected_rustc_id, expected_platform_id = toolchain.compute_ids(toolchain.rustc_vv())

        self.assertEqual(
            from_gate_sh["PLATFORM_ID"], expected_platform_id,
            "gate.sh and the Python side derive different verdict-cache keys on "
            "this host -- the L1 defect, exactly as it stood on the i7 "
            f"(gate {I7_GATE_PLATFORM_ID[:8]}..., fleetd "
            f"{I7_FLEETD_PLATFORM_ID_BEFORE[:8]}...)",
        )
        self.assertEqual(from_gate_sh["RUSTC_ID"], expected_rustc_id)

    def test_every_python_entry_point_agrees_with_the_shell_one(self):
        """`claim` (what fleetd's heartbeat and host claim use), `verdict`
        (what the cache key is built from) and `toolchain` must be one
        answer, not three. Each of these was a separate implementation
        before L1, and claim.py's disagreed with both others."""
        from_gate_sh = _run_gate_sh_toolchain_lines()
        text = toolchain.rustc_vv()
        self.assertEqual(claim.compute_platform_id(), from_gate_sh["PLATFORM_ID"])
        self.assertEqual(claim.compute_rustc_id(), from_gate_sh["RUSTC_ID"])
        self.assertEqual(
            verdict.compute_ids(text),
            (from_gate_sh["RUSTC_ID"], from_gate_sh["PLATFORM_ID"]),
        )

    def test_gate_sh_reads_the_resolver_rather_than_spelling_the_formula(self):
        """The control for the two tests above.

        A `gate.sh` that still piped `rustc -vV` into `shasum` itself
        would satisfy them for as long as the two happened to match --
        which is precisely how the fleet ran for weeks. So: point the
        resolver at a stand-in that emits sentinel ids and require
        gate.sh's own lines to follow it.
        """
        tmp = Path(tempfile.mkdtemp(prefix="fleet-toolchain-control-"))
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        fake = tmp / "fake_toolchain.py"
        fake.write_text(
            "print('FLEET_TOOLCHAIN_PATH_PREFIX=/sentinel/bin')\n"
            "print('RUSTC_PATH=/sentinel/bin/rustc')\n"
            "print('RUSTC_ID=SENTINEL-RUSTC-ID')\n"
            "print('PLATFORM_ID=SENTINEL-PLATFORM-ID')\n"
        )
        followed = _run_gate_sh_toolchain_lines(
            env=scrub_env(FLEET_TOOLCHAIN_PY=str(fake)))
        self.assertEqual(
            followed["PLATFORM_ID"], "SENTINEL-PLATFORM-ID",
            "gate.sh ignored the resolver and produced "
            f"{followed['PLATFORM_ID']!r} -- it is computing the digest itself, "
            "so the agreement above is a coincidence and not a mechanism",
        )
        self.assertEqual(followed["RUSTC_ID"], "SENTINEL-RUSTC-ID")

    def test_a_resolver_that_cannot_run_is_loud_and_yields_no_id(self):
        """The other half of the control. An unresolvable toolchain must
        leave the ids EMPTY and set `FLEET_TOOLCHAIN_ERROR`, never fall
        back to a guess: gate.sh ABORTs on that (config stage), and a
        guessed platform_id is a verdict written to a slot nothing else
        addresses -- the incident, with extra steps."""
        broken = _run_gate_sh_toolchain_lines(
            env=scrub_env(FLEET_TOOLCHAIN_PY="/nonexistent/toolchain.py"))
        self.assertEqual(broken["PLATFORM_ID"], "")
        self.assertEqual(broken["RUSTC_ID"], "")
        self.assertIn("/nonexistent/toolchain.py", broken["ERROR"])

    def test_gate_sh_aborts_rather_than_gating_without_an_identity(self):
        """gate.sh must refuse the run, not proceed with an empty key."""
        text = GATE_SH.read_text(encoding="utf-8")
        self.assertIn('if [ -n "$FLEET_TOOLCHAIN_ERROR" ]', text,
                      "gate.sh no longer checks FLEET_TOOLCHAIN_ERROR")
        self.assertIn("ABORT config: toolchain identity unresolved", text)
        source_at = _SOURCE_LINE_RE.search(text)
        call_at = _CALL_LINE_RE.search(text)
        abort_at = text.find('if [ -n "$FLEET_TOOLCHAIN_ERROR" ]')
        self.assertIsNotNone(source_at)
        self.assertIsNotNone(call_at)
        self.assertLess(source_at.start(), call_at.start(),
                        "gate.sh calls fleet_toolchain_ids before sourcing it")
        self.assertLess(call_at.start(), abort_at,
                        "gate.sh checks the error before computing it")


def _fake_gate_tree(tmp: Path, platform_id: str, rustc_id: str = "FAKE-RUSTC-ID") -> list:
    """A directory shaped like a fleet checkout whose resolver emits
    exactly the ids we ask for. Returns a `gate_command`."""
    gate_dir = tmp / "tools" / "fleet"
    (gate_dir / "units").mkdir(parents=True, exist_ok=True)
    gate = gate_dir / "gate.sh"
    gate.write_text("#!/bin/bash\n# stub: never executed by these tests\n")
    gate.chmod(0o755)
    (gate_dir / "units" / "fleet-toolchain.sh").write_text(
        "fleet_toolchain_ids() {\n"
        '  FLEET_TOOLCHAIN_ERROR=""\n'
        f'  RUSTC_ID="{rustc_id}"\n'
        f'  PLATFORM_ID="{platform_id}"\n'
        "  return 0\n"
        "}\n"
    )
    return [str(gate)]


class TestTheMismatchCheckFires(HermeticCase):
    """L1(b): a silent disagreement is what made this invisible, so the
    daemon now derives the id both ways and says so."""

    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        self.warnings = fleetd.HostWarnings()

    def test_a_disagreeing_gate_refuses_the_daemon_and_leaves_a_warning(self):
        cmd = _fake_gate_tree(self.tmp, platform_id="DELIBERATELY-DIFFERENT")
        may_start, msg = fleetd.check_toolchain_agreement(cmd, "server", self.warnings)
        self.assertFalse(may_start, "fleetd started anyway on a toolchain mismatch")
        self.assertIn("DELIBERATELY-DIFFERENT", msg)
        self.assertIn(toolchain.compute_platform_id(), msg,
                      "the refusal must name BOTH ids, or an operator cannot tell "
                      "which side is wrong")
        reasons = [reason for reason, _ in self.warnings.current()]
        self.assertIn(fleetd.TOOLCHAIN_MISMATCH_WARNING, reasons)

    def test_an_agreeing_gate_is_silent(self):
        """The negative control. A check that fired on a correct host
        would be turned off within a week and would then be worth
        nothing."""
        cmd = _fake_gate_tree(self.tmp, platform_id=toolchain.compute_platform_id())
        may_start, msg = fleetd.check_toolchain_agreement(cmd, "server", self.warnings)
        self.assertTrue(may_start)
        self.assertEqual(msg, "")
        self.assertEqual(self.warnings.current(), [])

    def test_the_real_gate_script_agrees_with_this_scheduler(self):
        """The production shape: the gate command fleetd actually builds,
        against the real `tools/fleet/gate.sh`. Red on the i7 before L1."""
        cmd = fleetd.default_gate_command(FLEET_DIR.parents[1])
        self.assertEqual(cmd, [str(GATE_SH)])
        may_start, msg = fleetd.check_toolchain_agreement(cmd, "server", self.warnings)
        self.assertTrue(may_start, msg)
        self.assertEqual(msg, "", f"real gate.sh disagrees with fleetd: {msg}")

    def test_an_unrunnable_resolver_warns_but_does_not_take_the_host_down(self):
        """"We could not compare" is not "they disagree" -- the same
        distinction `reconcile_once` draws between an unreadable
        `desired` ref and `enabled: false`. Taking a host down because a
        checkout is missing would trade one silent failure for a louder
        wrong one."""
        cmd = [str(self.tmp / "no" / "such" / "gate.sh")]
        may_start, msg = fleetd.check_toolchain_agreement(cmd, "server", self.warnings)
        self.assertTrue(may_start)
        self.assertIn("could not compute", msg)
        reasons = [reason for reason, _ in self.warnings.current()]
        self.assertIn(fleetd.TOOLCHAIN_UNVERIFIED_WARNING, reasons)

    def test_the_escape_hatch_downgrades_the_refusal_but_not_the_warning(self):
        cmd = _fake_gate_tree(self.tmp, platform_id="DELIBERATELY-DIFFERENT")
        env = dict(os.environ)
        env[fleetd.ALLOW_TOOLCHAIN_MISMATCH_ENV] = "1"
        may_start, msg = fleetd.check_toolchain_agreement(
            cmd, "server", self.warnings, env=env)
        self.assertTrue(may_start)
        self.assertIn("DELIBERATELY-DIFFERENT", msg)
        reasons = [reason for reason, _ in self.warnings.current()]
        self.assertIn(fleetd.TOOLCHAIN_MISMATCH_WARNING, reasons)

    def test_a_noted_warning_survives_the_marker_sweep(self):
        """`HostWarnings.scan` re-derives the `verdict-store-failed`
        entries from disk every reconcile; a toolchain condition is not
        backed by a file and must not be swept away by the first loop
        that finds no markers."""
        cmd = _fake_gate_tree(self.tmp, platform_id="DELIBERATELY-DIFFERENT")
        fleetd.check_toolchain_agreement(cmd, "server", self.warnings)
        logs = self.tmp / "gatelogs"
        logs.mkdir()
        after = self.warnings.scan(logs)
        self.assertIn(fleetd.TOOLCHAIN_MISMATCH_WARNING, [r for r, _ in after])


class TestTheRecordedIncidentBytes(HermeticCase):
    """The measurement, pinned. These are not synthetic strings: this is
    what the i7 printed and what the two components stored."""

    def test_the_i7_text_hashes_to_the_id_the_gate_published(self):
        rustc_id, platform_id = toolchain.compute_ids(I7_RUSTC_VV)
        self.assertEqual(platform_id, I7_GATE_PLATFORM_ID)
        self.assertEqual(rustc_id, I7_RUSTC_ID)

    def test_the_buggy_id_is_no_longer_reachable(self):
        """`b2bdf493...` was `sha256(vv)` with the trailing newline left
        on. Nothing in the fleet may produce it again -- every verdict on
        the state repo is keyed by the other one."""
        for name, ids in (
            ("toolchain", toolchain.compute_ids(I7_RUSTC_VV)),
            ("verdict", verdict.compute_ids(I7_RUSTC_VV)),
        ):
            self.assertNotIn(I7_FLEETD_PLATFORM_ID_BEFORE, ids,
                             f"{name} still produces the pre-fix platform_id")
        self.assertEqual(claim.compute_platform_id(I7_RUSTC_VV), I7_GATE_PLATFORM_ID)
        self.assertEqual(claim.compute_rustc_id(I7_RUSTC_VV), I7_RUSTC_ID)

    def test_the_trailing_newline_no_longer_changes_the_answer(self):
        """The one-character difference that cost the run. `$(...)` strips
        it, `subprocess.run` does not, and now neither matters."""
        self.assertEqual(
            toolchain.compute_ids(I7_RUSTC_VV),
            toolchain.compute_ids(I7_RUSTC_VV.rstrip("\n")),
        )

    def test_an_empty_toolchain_matches_the_shell_pipeline(self):
        """`printf '%s\\n' "" | grep -v '^host:'` emits ONE empty line, so
        an absent rustc hashes `"\\n"` for rustc_id and `""` for
        platform_id. `splitlines()` would return `[]` here and silently
        pick a different answer -- the same class of one-character
        divergence as the incident itself."""
        import hashlib
        rustc_id, platform_id = toolchain.compute_ids("")
        self.assertEqual(platform_id, hashlib.sha256(b"").hexdigest())
        self.assertEqual(rustc_id, hashlib.sha256(b"\n").hexdigest())


class TestThePathPrefixIsSpelledInExactlyTwoPlaces(HermeticCase):
    """The one literal this fix deliberately mirrors, and the fence that
    keeps the two copies honest -- same treatment `config.py` and
    `units/fleet-env.sh` get for `EXIFTOOL_CACHE_DIR`."""

    def test_python_and_shell_agree_on_the_prefix(self):
        sh_src = FLEET_TOOLCHAIN_SH.read_text(encoding="utf-8")
        m = re.search(r'FLEET_TOOLCHAIN_PATH_PREFIX:=([^}]+)\}', sh_src)
        self.assertIsNotNone(
            m, "units/fleet-toolchain.sh must default FLEET_TOOLCHAIN_PATH_PREFIX "
               "via ${VAR:=...}")
        sh_default = m.group(1)
        py_default = toolchain.toolchain_path_prefix({"HOME": "$HOME"})
        self.assertEqual(
            sh_default, py_default,
            "toolchain.py's TOOLCHAIN_PATH_PREFIX_REL and "
            "units/fleet-toolchain.sh's default have drifted apart -- the gate "
            "would then build with one rustc and identify itself by another",
        )

    def test_gate_sh_builds_its_path_from_the_shared_prefix(self):
        text = GATE_SH.read_text(encoding="utf-8")
        self.assertIn('$FLEET_TOOLCHAIN_PATH_PREFIX', text,
                      "gate.sh must build PATH from the shared prefix")
        self.assertNotIn('export PATH="$HOME/.nvm/versions/node/v24.13.1/bin:'
                         '$HOME/.cargo/bin:$HOME/.local/bin:$PATH"', text,
                         "gate.sh went back to spelling the toolchain prefix itself")

    def test_both_units_carry_the_prefix(self):
        """L1(c): the plist had a PATH and fleetd.service did not, which
        is the whole reason the bug lived on the only gate host."""
        service = (FLEET_DIR / "units" / "fleetd.service").read_text()
        plist = (FLEET_DIR / "units" / "com.oxidex.fleetd.plist").read_text()
        self.assertIn("Environment=PATH=%h/.cargo/bin:%h/.local/bin", service,
                      "fleetd.service must set a PATH carrying the toolchain prefix")
        self.assertIn("/Users/allen/.cargo/bin:/Users/allen/.local/bin", plist)


class TestNoFourthImplementation(HermeticCase):
    """Three implementations is how this happened. Grep-level, cheap, and
    aimed at the shape a well-meaning future edit takes."""

    def test_gate_sh_no_longer_pipes_rustc_into_a_digest(self):
        live = "\n".join(
            line for line in GATE_SH.read_text(encoding="utf-8").splitlines()
            if not line.lstrip().startswith("#")
        )
        for needle in ("sha256sum", "shasum", "rustc -vV"):
            self.assertNotIn(
                needle, live,
                f"gate.sh computes a toolchain digest again ({needle!r} on a live "
                f"line) -- that is the fourth implementation this fix removed",
            )

    def test_the_python_modules_delegate_rather_than_hash(self):
        for name in ("claim.py", "verdict.py", "doctor.py"):
            src = (FLEET_DIR / name).read_text(encoding="utf-8")
            live = "\n".join(
                line for line in src.splitlines() if not line.lstrip().startswith("#")
            )
            self.assertIn("toolchain.", live, f"{name} no longer uses the resolver")
            self.assertNotIn(
                "hashlib.sha256", live,
                f"{name} hashes a toolchain string of its own again",
            )


if __name__ == "__main__":
    unittest.main()
