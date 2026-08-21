#!/usr/bin/env python3
"""Guard: no host-specific URL/path survives in the runtime-config surface
(PLAN Stage 1 task 4, "de-hardcode URLs and paths").

`gate.sh` and `fleetd.py` used to hardcode the old single-repo topology --
the production hub over ssh (`work2.oxidex.net`, `/home/allen/git/
oxidex.git`) and the pinned-oracle cache directory (`/tmp/oxidex-exiftool-
cache`) -- in several places, and the `units/*` templates baked the same
ssh URL into their `Environment`/`EnvironmentVariables`/crontab lines.
Stage 1 splits the hub into two required remotes (`FLEET_HUB_URL` for the
verdict/state repo, `FLEET_CODE_URL` for the repo holding staging
branches) and makes the oracle cache directory `EXIFTOOL_CACHE_DIR`-
overridable; this test is the mechanical fence that keeps either literal
from creeping back into the files this task fixed.

R6 (review of `staging/agent-server` @ 99f06cb3) widened this fence twice
over, after `doctor.py`/`ledger.py` and `fleetd.py`/`gate.sh` were each
found holding their own copy of the oracle-cache default:
  * `doctor.py`/`ledger.py` are now scanned like any other candidate file
    (they used to be exempted outright -- class 1 below no longer
    exists). Both now import `tools/fleet/config.py`'s
    `exiftool_cache_dir()`/`DEFAULT_EXIFTOOL_CACHE_DIR` instead of
    spelling the literal a second and third time, so they score zero
    hits without any file-specific exemption.
  * The forbidden cache-dir needle is now the BASENAME
    (`oxidex-exiftool-cache`), not the full `/tmp/...` path, specifically
    to catch a "two-piece assembly" idiom -- splitting the basename into
    its own variable and reconstructing `f"/tmp/{basename}"` /
    `"/tmp/$basename"` elsewhere -- that is functionally identical to the
    literal but was written specifically to dodge a substring search for
    the whole path (see `fleetd.py`'s own `_exiftool_cache_dir()`
    docstring, which says as much: "Built from two pieces ... so this
    line is not itself a hardcoded-host match"). A basename assignment is
    exactly as much a hardcode as the full literal it reassembles into,
    and this fence now treats it as one.
  * `tools/fleet/config.py` (imported, not scanned) and
    `units/fleet-env.sh` (sourced, not scanned) are the ONLY two places
    the basename may be written out as a value -- `test_config_and_
    fleet_env_agree_on_the_default` below pins them against each other so
    they cannot silently drift apart into two different defaults.

SCOPE. This scans `gate.sh`, `fleetd.py`, `agentworker.py`, `doctor.py`,
`ledger.py`, and every file under `units/` EXCEPT `fleet-env.sh` -- the
runtime-config surface this task, the M2 follow-up, and R6 actually
touched -- not the whole `tools/fleet` tree, and not `config.py`/
`fleet-env.sh` themselves (the one place each literal is allowed to live).
One class of pre-existing, *intentional* literal reference lives
elsewhere and must not be flagged (a second class used to, and is
recorded below because its retirement is what closed the last hole):
  1. Half the test suite (`test_fleetlib.py`, `test_claim.py`,
     `test_queue.py`, ...) asserts `assertNotIn("work2.oxidex.net",
     resolved)` as a fixture-isolation guard -- the string appears there
     as a *negative* to check against, never as a value anything
     connects to. Scanning `tests/` would flag the safety net, not a
     regression.
  2. (Retired.) `agentworker.py` used to carry the identical
     `CACHE_DIR = os.environ.get("EXIFTOOL_CACHE_DIR",
     "/tmp/oxidex-exiftool-cache")` idiom (M2) so its prompt strings
     could stop hardcoding the path four times over, and `_scan`
     exempted that one line. It now reads `config.exiftool_cache_dir()`
     like `doctor.py`/`ledger.py`, so NO line in any scanned file is
     exempted any more: `_CACHE_DIR_DEFAULT_IDIOM` below survives only as
     the shape `test_cache_dir_default_idiom_is_no_longer_exempted` pins
     as forbidden. Every occurrence in `agentworker.py` -- in particular
     every prompt string -- is forbidden outside a comment.
A file-scope fence beats a tree-wide grep here: it catches the real
regression (one of these needles reappearing in the config surface that
ships to a host) without producing noise on code that has a legitimate,
different reason to hold the same substring.

"Outside comment lines": a comment line (`#`-prefixed for the shell/
Python/ini/crontab files here, `<!-- ... -->` for the one XML file) may
still document the old topology as history -- gate.sh's own header keeps
several such lines, e.g. the PLAN Stage 1 task 4 addendum naming
`work2.oxidex.net` and `/tmp/oxidex-exiftool-cache` as what USED to be
hardcoded. Only a non-comment occurrence is a hardcoded value actually in
effect.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]  # tools/fleet
REPO_ROOT = FLEET_DIR.parents[1]

# R6: the two canonical files the oracle-cache default is allowed to live
# in -- imported/sourced by every consumer, never re-derived. Excluded
# from `_candidate_files()` by name (not scanned at all), the same way
# this module's own PLAN-era predecessor never scanned itself.
_CANONICAL_CACHE_DIR_DEFAULT_FILES = ("config.py", "fleet-env.sh")

# The exact strings named in PLAN Stage 1 task 4, widened by R6. Plain
# substrings, not a regex -- these are literal hostnames/paths (or, for
# the cache dir, the literal's invariant BASENAME -- see module
# docstring), and treating them as regex would risk "." matching more
# than intended. The basename form (not the full "/tmp/..." path) is
# deliberate: it is the one substring common to both the straight literal
# AND a "two-piece assembly" that splits the basename into its own
# variable and reconstructs the path elsewhere specifically to dodge a
# full-path substring search -- see `test_two_piece_basename_assembly_
# is_still_caught` below for the shape this is aimed at.
_FORBIDDEN = (
    "work2.oxidex.net",
    "/home/allen/git/oxidex.git",
    "oxidex-exiftool-cache",
)

# The former class-2 exemption (module docstring): the one-piece
# `EXIFTOOL_CACHE_DIR`-with-fallback idiom `agentworker.py` carried until
# it was moved onto `config.exiftool_cache_dir()` (R6). `_scan` no longer
# exempts it -- it is kept only so a test can pin that this exact shape
# is now caught like any other spelling of the default.
_CACHE_DIR_DEFAULT_IDIOM = 'os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache")'


def _candidate_files():
    """gate.sh, fleetd.py, agentworker.py, doctor.py, ledger.py, and
    every file directly under units/ except fleet-env.sh (the canonical
    shell definition -- see `_CANONICAL_CACHE_DIR_DEFAULT_FILES`) -- the
    runtime-config surface PLAN Stage 1 task 4, the M2 follow-up, and R6
    de-hardcoded. Sorted for a deterministic scan order and failure
    message.
    """
    files = [
        FLEET_DIR / "gate.sh",
        FLEET_DIR / "fleetd.py",
        FLEET_DIR / "agentworker.py",
        FLEET_DIR / "doctor.py",
        FLEET_DIR / "ledger.py",
    ]
    units_dir = FLEET_DIR / "units"
    files.extend(
        sorted(
            p for p in units_dir.iterdir()
            if p.is_file() and p.name not in _CANONICAL_CACHE_DIR_DEFAULT_FILES
        )
    )
    return files


def _is_xml(path: Path) -> bool:
    return path.suffix == ".plist"


def _strip_comments(path: Path, text: str):
    """Yield (lineno, line) for every line of `text` that is NOT a full
    comment line, given `path`'s comment syntax.

    XML (`.plist`): a line is dropped only while inside a `<!-- ... -->`
    span that covers the WHOLE line (a `<!--`/`-->` pair on its own lines,
    or a single-line comment) -- this is a line-level filter, adequate for
    the small hand-written unit templates here, not a general XML parser.

    Everything else (`.sh`, `.py`, `.service`, `.txt`): a line is dropped
    when its stripped content starts with `#` (also `;`, the other
    systemd-ini comment marker, for `.service` files).
    """
    if _is_xml(path):
        in_comment = False
        for lineno, line in enumerate(text.splitlines(), start=1):
            stripped = line.strip()
            if in_comment:
                if "-->" in line:
                    in_comment = False
                continue
            if stripped.startswith("<!--"):
                if "-->" not in line:
                    in_comment = True
                continue
            yield lineno, line
        return

    comment_prefixes = ("#",) if path.suffix != ".service" else ("#", ";")
    for lineno, line in enumerate(text.splitlines(), start=1):
        if line.strip().startswith(comment_prefixes):
            continue
        yield lineno, line


def _scan(path: Path):
    """(lineno, line_text, matched_string) for every forbidden literal
    found on a non-comment line of `path`. No per-line exemptions: the
    only places a default may be spelled are `config.py` and
    `units/fleet-env.sh`, and neither is a candidate file.
    """
    hits = []
    text = path.read_text(encoding="utf-8")
    for lineno, line in _strip_comments(path, text):
        for needle in _FORBIDDEN:
            if needle in line:
                hits.append((lineno, line.strip(), needle))
    return hits


class TestNoHardcodedHosts(unittest.TestCase):
    def test_no_hardcoded_hosts_outside_comments(self):
        offenders = []
        for path in _candidate_files():
            for lineno, line, needle in _scan(path):
                rel = path.relative_to(REPO_ROOT)
                offenders.append(f"{rel}:{lineno}: [{needle}] {line}")

        self.assertEqual(
            offenders,
            [],
            "hardcoded host/path literal(s) found outside comment lines -- read "
            "FLEET_HUB_URL / FLEET_CODE_URL / EXIFTOOL_CACHE_DIR from the "
            "environment instead (see this test's module docstring):\n  "
            + "\n  ".join(offenders),
        )

    def test_candidate_files_exist(self):
        """A typo in `_candidate_files()` (or a renamed/removed unit file)
        must fail loudly, not silently scan zero files and report a vacuous
        pass -- the same class of trap AGENTS.md's "implicit binary
        resolution" note warns about.
        """
        files = _candidate_files()
        self.assertGreaterEqual(
            len(files), 5 + 3,
            f"expected at least gate.sh + fleetd.py + agentworker.py + "
            f"doctor.py + ledger.py + the 3 known units/ files "
            f"(fleet-env.sh deliberately excluded -- it is the canonical "
            f"definition, not a consumer), got {[str(p) for p in files]}",
        )
        for path in files:
            self.assertTrue(path.is_file(), f"candidate file missing: {path}")
        names = {p.name for p in files}
        self.assertNotIn(
            "fleet-env.sh", names,
            "fleet-env.sh is the canonical shell definition of the cache-dir "
            "default (R6) -- it must be excluded from the scan, not merely "
            "pass it, or the fence would forbid its own only legal home",
        )

    def test_detector_catches_a_known_shape_and_ignores_comments(self):
        """Sanity check on the detector itself, independent of the current
        tree's contents, mirroring test_no_raw_hub_push.py's own pattern
        check: a regression in `_scan`/`_strip_comments` must not silently
        turn this whole test into a no-op.
        """
        sample_bad = 'HUB_URL="${FLEET_HUB_URL:-ssh://allen@work2.oxidex.net:2244/home/allen/git/oxidex.git}"\n'
        hits = list(_strip_comments(FLEET_DIR / "gate.sh", sample_bad))
        self.assertEqual(len(hits), 1)
        self.assertIn("work2.oxidex.net", hits[0][1])

        sample_comment = "# the old hub used to be work2.oxidex.net over ssh\n"
        hits = list(_strip_comments(FLEET_DIR / "gate.sh", sample_comment))
        self.assertEqual(hits, [], "a '#'-prefixed comment line must be excluded")

        sample_indented_comment = "    # /tmp/oxidex-exiftool-cache lived here\n"
        hits = list(_strip_comments(FLEET_DIR / "fleetd.py", sample_indented_comment))
        self.assertEqual(hits, [], "leading whitespace before '#' must still count as a comment")

        sample_xml_comment = "<!-- ssh://allen@work2.oxidex.net:2244/home/allen/git/oxidex.git -->\n"
        hits = list(_strip_comments(FLEET_DIR / "units" / "com.oxidex.fleetd.plist", sample_xml_comment))
        self.assertEqual(hits, [], "a single-line XML comment must be excluded")

        sample_xml_live = "<string>ssh://allen@work2.oxidex.net:2244/home/allen/git/oxidex.git</string>\n"
        hits = list(_strip_comments(FLEET_DIR / "units" / "com.oxidex.fleetd.plist", sample_xml_live))
        self.assertEqual(len(hits), 1, "a live (non-comment) XML line must NOT be excluded")

    def test_cache_dir_default_idiom_is_no_longer_exempted(self):
        """Former class 2 (module docstring): the exact `os.environ.get(
        "EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache")` line used to
        be the one exempted shape. With every consumer on `config.py` it
        is a hardcode like any other: `_scan` must flag it, and must flag
        the near-miss (a prompt string that mentions the env var name while
        still spelling out the raw path) exactly the same way.
        """
        idiom_line = 'CACHE_DIR = os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache")\n'
        self.assertIn(_CACHE_DIR_DEFAULT_IDIOM, idiom_line)
        near_miss = '- oracle default is EXIFTOOL_CACHE_DIR, usually /tmp/oxidex-exiftool-cache\n'
        for sample in (idiom_line, near_miss):
            hits = list(_strip_comments(FLEET_DIR / "agentworker.py", sample))
            needled = [(lineno, line, needle) for lineno, line in hits
                       for needle in _FORBIDDEN if needle in line]
            self.assertEqual(len(needled), 1, f"must be caught: {sample!r}")

    def test_agentworker_prompt_strings_use_the_variable_not_the_literal(self):
        """The M2 fix itself: `build_prompt`/`build_authoring_prompt` used
        to hardcode `/tmp/oxidex-exiftool-cache` four times in the prompt
        text handed to a headless agent. Assert the rendered prompts carry
        the resolved `CACHE_DIR` value (so the agent still gets a real,
        working path) while the SOURCE no longer spells the literal at all
        -- `CACHE_DIR` comes from `config.exiftool_cache_dir()` (R6), the
        same single default `doctor.py`/`ledger.py`/`fleetd.py` import --
        i.e. the fence in `test_no_hardcoded_hosts_outside_comments`
        actually has teeth here, not just headroom to pass vacuously.
        """
        agentworker_path = FLEET_DIR / "agentworker.py"
        hits = _scan(agentworker_path)
        self.assertEqual(
            hits, [],
            "agentworker.py should have zero forbidden-literal hits -- a hit "
            "here means a prompt string (or the CACHE_DIR line) is "
            "hardcoding the path again",
        )
        source = agentworker_path.read_text()
        self.assertIn(
            "CACHE_DIR = str(config.exiftool_cache_dir())", source,
            "agentworker.py's CACHE_DIR must come from config.py, not a local default",
        )
        idiom_hits = [
            lineno for lineno, line in _strip_comments(agentworker_path, source)
            if _CACHE_DIR_DEFAULT_IDIOM in line
        ]
        self.assertEqual(idiom_hits, [], f"the one-piece default idiom is back: {idiom_hits}")

    def test_two_piece_basename_assembly_is_still_caught(self):
        """R6's whole point: splitting the basename into its own variable
        and reconstructing the path elsewhere is not a way around this
        fence. Both real shapes this was found in -- `fleetd.py`'s
        Python f-string and `gate.sh`'s shell parameter expansion --
        never spell `/tmp/oxidex-exiftool-cache` as one contiguous
        literal, yet both must still be caught.
        """
        py_shape = (
            '    default_basename = "oxidex-exiftool-cache"\n'
            '    return Path(os.environ.get("EXIFTOOL_CACHE_DIR", f"/tmp/{default_basename}"))\n'
        )
        hits = list(_strip_comments(FLEET_DIR / "fleetd.py", py_shape))
        needled = [(lineno, line, needle)
                   for lineno, line in hits
                   for needle in _FORBIDDEN if needle in line]
        self.assertEqual(
            len(needled), 1,
            "the basename-assignment line of a two-piece Python assembly must "
            "be flagged even though '/tmp/oxidex-exiftool-cache' never appears "
            "as one contiguous substring anywhere in it",
        )
        self.assertIn("oxidex-exiftool-cache", needled[0][1])

        sh_shape = (
            ': "${_OXIDEX_CACHE_BASENAME:=oxidex-exiftool-cache}"\n'
            'export EXIFTOOL_CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/$_OXIDEX_CACHE_BASENAME}"\n'
        )
        hits = list(_strip_comments(FLEET_DIR / "gate.sh", sh_shape))
        needled = [(lineno, line, needle)
                   for lineno, line in hits
                   for needle in _FORBIDDEN if needle in line]
        self.assertEqual(
            len(needled), 1,
            "the basename-assignment line of a two-piece shell assembly must "
            "be flagged the same way",
        )
        self.assertIn("_OXIDEX_CACHE_BASENAME:=oxidex-exiftool-cache", needled[0][1])

    def test_doctor_and_ledger_score_zero_hits(self):
        """R6: both files used to spell the literal directly; both now
        import `config.exiftool_cache_dir()` instead. A hit here means
        one of them regressed back to a local literal (or a new one crept
        in some other line) -- the whole reason R6 added them to
        `_candidate_files()` in the first place.
        """
        for name in ("doctor.py", "ledger.py"):
            path = FLEET_DIR / name
            hits = _scan(path)
            self.assertEqual(hits, [], f"{name} should have zero forbidden-literal hits: {hits}")
            self.assertIn(
                "config.exiftool_cache_dir()", path.read_text(),
                f"{name} should source the cache dir from config.py, not a local literal",
            )

    def test_config_and_fleet_env_agree_on_the_default(self):
        """The two canonical definitions (module docstring) must spell the
        IDENTICAL default -- both are excluded from the substring fence
        above precisely because they are allowed to hold the literal, so
        this is the one place their agreement is actually checked. A
        divergence here is invisible to every consumer (each reads its
        own file and never compares) until two hosts disagree on which
        cache directory is the default.
        """
        import re

        config_src = (FLEET_DIR / "config.py").read_text()
        m = re.search(r'^DEFAULT_EXIFTOOL_CACHE_DIR = "([^"]+)"', config_src, re.MULTILINE)
        self.assertIsNotNone(m, "config.py must define DEFAULT_EXIFTOOL_CACHE_DIR as a plain string literal")
        py_default = m.group(1)

        sh_src = (FLEET_DIR / "units" / "fleet-env.sh").read_text()
        m = re.search(r'EXIFTOOL_CACHE_DIR:=([^}]+)\}', sh_src)
        self.assertIsNotNone(m, "units/fleet-env.sh must set an EXIFTOOL_CACHE_DIR default via ${VAR:=...}")
        sh_default = m.group(1)

        self.assertEqual(
            py_default, sh_default,
            f"config.py's DEFAULT_EXIFTOOL_CACHE_DIR ({py_default!r}) and "
            f"units/fleet-env.sh's default ({sh_default!r}) have drifted apart",
        )
        self.assertEqual(py_default, "/tmp/oxidex-exiftool-cache")


if __name__ == "__main__":
    unittest.main()
